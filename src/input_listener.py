import evdev
from evdev import ecodes
import threading
import time
import os

from hotkeys.double_tap import DoubleTapMachine
from routing.loader import load_bindings
from routing.models import GestureType


class InputListener(threading.Thread):
    def __init__(self, config, on_press, on_release):
        """
        on_press(target_id):  called when a binding activates
        on_release(target_id): called when a binding deactivates
        """
        super().__init__(daemon=True)
        self.config = config
        self.on_press = on_press
        self.on_release = on_release
        self.device = None
        self.running = True

        # Loaded from bindings.toml
        self._bindings = []
        # Per-binding state for hold/toggle gestures
        self._hold_active: dict = {}     # binding_id → bool
        self._toggle_state: dict = {}    # binding_id → bool
        self._last_toggle: dict = {}     # binding_id → bool
        # Per-binding DoubleTapMachine instances
        self._dt_machines: dict = {}     # binding_id → DoubleTapMachine
        # Pressed evdev scancodes
        self.pressed_keys: set = set()
        # Lock for pressed_keys set (shared between reader and deadline timers)
        self._keys_lock = threading.Lock()

    _VIRTUAL_DEVICE_NAMES = ("whisper-wayland", "passthrough", "uinput", "virtual")

    def _is_virtual(self, dev):
        return any(kw in dev.name.lower() for kw in self._VIRTUAL_DEVICE_NAMES)

    def find_device(self):
        saved_path = self.config.get("evdev_device")
        if saved_path and os.path.exists(saved_path):
            try:
                dev = evdev.InputDevice(saved_path)
                if not self._is_virtual(dev):
                    return dev
            except Exception:
                pass

        paths = sorted(
            evdev.list_devices(),
            key=lambda p: int(p.replace("/dev/input/event", "") or 0)
        )
        for path in paths:
            try:
                dev = evdev.InputDevice(path)
            except Exception:
                continue
            if self._is_virtual(dev):
                continue
            if ecodes.EV_KEY in dev.capabilities():
                if ecodes.KEY_A in dev.capabilities()[ecodes.EV_KEY]:
                    print(f"Automatically selected input device: {dev.name} ({dev.path})")
                    return dev
        return None

    def update_hotkey(self):
        """Reload bindings from file (called after settings save)."""
        self._load_bindings()

    def update_device(self):
        if self.device and self.device.path != self.config.get("evdev_device"):
            try:
                self.device.close()
            except Exception:
                pass
            self.device = None

    def _load_bindings(self):
        """Load bindings.toml; fall back to legacy config.json hotkeys if absent."""
        for b in getattr(self, "_bindings", []):
            if b.gesture == GestureType.TOGGLE and self._toggle_state.get(b.id, False):
                try:
                    self.on_release(b.target_id)
                except Exception:
                    pass
            elif b.gesture == GestureType.HOLD and self._hold_active.get(b.id, False):
                try:
                    self.on_release(b.target_id)
                except Exception:
                    pass

        try:
            bindings = [b for b in load_bindings() if not b.disabled]
        except Exception as e:
            print(f"[InputListener] Could not load bindings.toml: {e}; using legacy config")
            bindings = self._legacy_bindings()

        self._bindings = bindings
        self._hold_active = {b.id: False for b in bindings}
        self._toggle_state = {b.id: False for b in bindings}
        self._last_toggle = {b.id: False for b in bindings}
        self._dt_machines = {}

        for b in bindings:
            if b.gesture == GestureType.DOUBLE_TAP:
                self._dt_machines[b.id] = DoubleTapMachine(
                    b,
                    on_start=lambda binding: self.on_press(binding.target_id),
                    on_stop=lambda binding: self.on_release(binding.target_id),
                )

        # Log for diagnostics
        for b in bindings:
            print(f"[InputListener] Binding: {b.id!r} keys={b.keys} gesture={b.gesture.value} → {b.target_id!r}")

    def _legacy_bindings(self):
        """Build minimal bindings from the old JSON config for backward compatibility."""
        from routing.models import HotkeyBinding
        result = []
        hold_keys = self.config.get("hotkey", ["KEY_LEFTMETA", "KEY_SPACE"])
        toggle_keys = self.config.get("toggle_hotkey", ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"])
        dt_keys = self.config.get("double_tap_hotkey", ["KEY_LEFTALT"])

        result.append(HotkeyBinding(
            id='default_hold', label='Dictate (Hold)',
            keys=hold_keys, gesture=GestureType.HOLD, target_id='default',
        ))
        result.append(HotkeyBinding(
            id='default_toggle', label='Dictate (Toggle)',
            keys=toggle_keys, gesture=GestureType.TOGGLE, target_id='default',
        ))
        result.append(HotkeyBinding(
            id='default_dt', label='Dictate (Double-Tap)',
            keys=dt_keys, gesture=GestureType.DOUBLE_TAP, target_id='default',
        ))
        return result

    def _scancode_for(self, key_name: str) -> int | None:
        try:
            return ecodes.ecodes[key_name]
        except KeyError:
            return None

    def _binding_key_scancodes(self, binding) -> set:
        codes = set()
        for name in binding.keys:
            code = self._scancode_for(name)
            if code is not None:
                codes.add(code)
        return codes

    def _dispatch_hold(self, binding, pressed: set) -> None:
        key_codes = self._binding_key_scancodes(binding)
        is_match = key_codes and key_codes.issubset(pressed)
        was_active = self._hold_active.get(binding.id, False)

        if is_match and not was_active:
            self._hold_active[binding.id] = True
            self.on_press(binding.target_id)
        elif not is_match and was_active:
            self._hold_active[binding.id] = False
            self.on_release(binding.target_id)

    def _dispatch_toggle(self, binding, pressed: set) -> None:
        key_codes = self._binding_key_scancodes(binding)
        is_match = key_codes and key_codes.issubset(pressed)
        last_match = self._last_toggle.get(binding.id, False)

        if is_match and not last_match:
            self._toggle_state[binding.id] = not self._toggle_state.get(binding.id, False)
            if self._toggle_state[binding.id]:
                self.on_press(binding.target_id)
            else:
                self.on_release(binding.target_id)

        self._last_toggle[binding.id] = is_match

    def run(self):
        self._load_bindings()

        while self.running:
            try:
                if self.device:
                    try:
                        self.device.close()
                    except Exception:
                        pass
                    self.device = None

                self.device = self.find_device()
                if not self.device:
                    print("[InputListener] No suitable input device found. Check /dev/input permissions (input group). Retrying in 5s...")
                    time.sleep(5)
                    continue

                print(f"[*] Listening on {self.device.path}")

                for event in self.device.read_loop():
                    if not self.running:
                        break

                    if event.type != ecodes.EV_KEY:
                        continue

                    key_event = evdev.categorize(event)
                    scancode = key_event.scancode
                    keystate = key_event.keystate  # 1=down, 0=up, 2=repeat
                    key_name = ecodes.KEY.get(scancode, f"KEY_{scancode}")
                    if isinstance(key_name, list):
                        key_name = key_name[0]
                    timestamp = event.timestamp()

                    with self._keys_lock:
                        if keystate == evdev.KeyEvent.key_down:
                            self.pressed_keys.add(scancode)
                        elif keystate == evdev.KeyEvent.key_up:
                            self.pressed_keys.discard(scancode)
                        pressed_snapshot = set(self.pressed_keys)

                    # Build snapshot of currently-pressed scancodes
                    # (already done above in the lock block)

                    # ── HOLD: fire on_press when all keys down, on_release when any key up ─
                    for b in self._bindings:
                        if b.gesture != GestureType.HOLD:
                            continue
                        b_codes = self._binding_key_scancodes(b)
                        if not b_codes:          # skip if no known scancodes
                            continue
                        is_match = b_codes.issubset(pressed_snapshot)
                        # Suppress: don't fire HOLD if a more-specific binding also matches
                        if is_match:
                            for other in self._bindings:
                                if other.id == b.id or other.gesture != GestureType.HOLD:
                                    continue
                                other_codes = self._binding_key_scancodes(other)
                                if (other_codes and b_codes.issubset(other_codes)
                                        and len(other_codes) > len(b_codes)
                                        and other_codes.issubset(pressed_snapshot)):
                                    is_match = False   # shadowed by a longer combo
                                    break
                        was_active = self._hold_active.get(b.id, False)
                        if is_match and not was_active:
                            print(f"[InputListener] HOLD start: {b.label or b.id}")
                            self._hold_active[b.id] = True
                            self.on_press(b.target_id)
                        elif not is_match and was_active:
                            print(f"[InputListener] HOLD end: {b.label or b.id}")
                            self._hold_active[b.id] = False
                            self.on_release(b.target_id)

                    # ── TOGGLE / DOUBLE_TAP: edge-triggered on key events ─────────────────
                    if keystate == evdev.KeyEvent.key_down:
                        for b in self._bindings:
                            b_codes = self._binding_key_scancodes(b)
                            if not b_codes:
                                continue

                            if b.gesture == GestureType.TOGGLE:
                                if b_codes.issubset(pressed_snapshot):
                                    # Rising-edge: only fire once per physical key press
                                    if not self._last_toggle.get(b.id, False):
                                        self._toggle_state[b.id] = not self._toggle_state.get(b.id, False)
                                        print(f"[InputListener] TOGGLE {'on' if self._toggle_state[b.id] else 'off'}: {b.label or b.id}")
                                        if self._toggle_state[b.id]:
                                            self.on_press(b.target_id)
                                        else:
                                            self.on_release(b.target_id)
                                    self._last_toggle[b.id] = True

                            elif b.gesture == GestureType.DOUBLE_TAP:
                                machine = self._dt_machines.get(b.id)
                                if machine:
                                    machine.on_key_event(key_name, keystate, timestamp)

                    elif keystate == evdev.KeyEvent.key_up:
                        for b in self._bindings:
                            if b.gesture == GestureType.TOGGLE:
                                b_codes = self._binding_key_scancodes(b)
                                if b_codes and not b_codes.issubset(pressed_snapshot):
                                    self._last_toggle[b.id] = False   # reset latch on release
                            elif b.gesture == GestureType.DOUBLE_TAP:
                                machine = self._dt_machines.get(b.id)
                                if machine:
                                    machine.on_key_event(key_name, keystate, timestamp)

            except Exception as e:
                print(f"Error in input listener: {e}")
                time.sleep(1)

    def stop(self):
        self.running = False
