import shutil
import threading
import queue
import time
import subprocess
import os

try:
    from evdev import ecodes as _ecodes
    _HAS_EVDEV = True
except ImportError:
    _ecodes = None
    _HAS_EVDEV = False

try:
    from portal_injector import PortalInjector
    _HAS_PORTAL = True
except ImportError:
    _HAS_PORTAL = False

try:
    import atspi_context as _atspi
    _HAS_ATSPI = True
except ImportError:
    _HAS_ATSPI = False

# Word count accumulator for P0.6 (session stats)
_session_word_count = 0

class TextInjector(threading.Thread):
    def __init__(self, config, text_queue, word_count_callback=None, router=None):
        super().__init__(daemon=True)
        self.config = config
        self.text_queue = text_queue
        self.word_count_callback = word_count_callback  # P0.6: called with new total
        self.router = router  # OutputTargetRouter; None = use legacy inject path
        self.running = True
        self._session_words = 0
        self.portal = PortalInjector() if _HAS_PORTAL else None

    def _send_notification(self, text):
        """P0.5: Fire a desktop notification via notify-send (best-effort)."""
        if not self.config.get("show_notification", False):
            return
        if not shutil.which("notify-send"):
            return
        preview = (text[:60] + "…") if len(text) > 60 else text
        subprocess.Popen(
            ["notify-send", "-t", "3000", "-i", "audio-input-microphone",
             "Whisper Wayland", preview],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def _env(self):
        env = os.environ.copy()
        # Auto-detect Wayland socket if WAYLAND_DISPLAY isn't set
        if 'WAYLAND_DISPLAY' not in env:
            uid = os.getuid()
            for i in range(3):
                if os.path.exists(f"/run/user/{uid}/wayland-{i}"):
                    env['WAYLAND_DISPLAY'] = f"wayland-{i}"
                    break
        return env

    # Modifier keysyms wtype understands for explicit release
    _WTYPE_MODIFIERS = ['ctrl', 'super', 'alt', 'shift']

    def _release_modifiers_wayland(self, env):
        """
        Send explicit modifier key-up events via wtype before injecting text.
        Prevents stuck-Ctrl bugs: the compositor sees the physical modifier held
        while virtual keyboard events arrive, and forwards them as e.g. Ctrl+H.
        """
        if not shutil.which('wtype'):
            return
        # Build: wtype -m ctrl -m super -m alt -m shift
        cmd = ['wtype']
        for mod in self._WTYPE_MODIFIERS:
            cmd += ['-m', mod]
        subprocess.run(cmd, env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def inject_text(self, text):
        if not text:
            return

        print(f"Injecting: {text}")
        env = self._env()
        is_wayland = 'WAYLAND_DISPLAY' in env
        is_x11 = 'DISPLAY' in env
        injected = False

        # Short pause so the hotkey's physical key-up events propagate to the
        # compositor before we send virtual keyboard events on top of them.
        time.sleep(0.12)

        # --- AT-SPI2: direct Text.insertText (no keystrokes, no modifier issues) ---
        if not injected and _HAS_ATSPI and self.config.get('atspi_injection', True):
            if _atspi.is_available() and _atspi.inject_text(text):
                injected = True

        # --- Wayland: release modifiers first, then type ---
        if is_wayland and shutil.which('wtype'):
            self._release_modifiers_wayland(env)
            result = subprocess.run(
                ['wtype', '--', text], env=env,
                stderr=subprocess.DEVNULL
            )
            if result.returncode == 0:
                injected = True

        # --- Wayland: Portal fallback (GNOME / Secure) ---
        if not injected and is_wayland and self.portal:
            if self.portal.inject(text):
                injected = True

        # --- X11: xdotool types directly, no clipboard needed ---
        if not injected and is_x11 and shutil.which('xdotool'):
            result = subprocess.run(
                ['xdotool', 'type', '--clearmodifiers', '--delay', '12', '--', text],
                env=env, stderr=subprocess.DEVNULL
            )
            if result.returncode == 0:
                injected = True

        # --- Clipboard + paste fallback ---
        if not injected:
            copied = False
            if is_wayland and shutil.which('wl-copy'):
                proc = subprocess.run(
                    ['wl-copy'], input=text.encode('utf-8'),
                    env=env, stderr=subprocess.DEVNULL
                )
                copied = proc.returncode == 0

            if not copied and is_x11 and shutil.which('xclip'):
                proc = subprocess.run(
                    ['xclip', '-selection', 'clipboard'],
                    input=text.encode('utf-8'), env=env,
                    stderr=subprocess.DEVNULL
                )
                copied = proc.returncode == 0

            if not copied:
                try:
                    import pyperclip
                    pyperclip.copy(text)
                    copied = True
                except Exception:
                    pass

            if copied:
                time.sleep(0.05)
                if is_wayland and shutil.which('ydotool'):
                    # Release stuck modifiers before sending Ctrl+V so the compositor
                    # doesn't see doubled modifiers.
                    if _HAS_EVDEV:
                        _mod_kcs = [
                            _ecodes.KEY_LEFTCTRL,
                            _ecodes.KEY_LEFTMETA,
                            _ecodes.KEY_LEFTALT,
                        ]
                    else:
                        _mod_kcs = [29, 125, 56]  # KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTALT
                    for kc in _mod_kcs:
                        subprocess.run(
                            ['ydotool', 'key', f'{kc}:0'], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                        )
                    subprocess.run(['ydotool', 'key', 'ctrl+v'], env=env, stderr=subprocess.DEVNULL)
                elif is_x11 and shutil.which('xdotool'):
                    subprocess.run(
                        ['xdotool', 'key', '--clearmodifiers', 'ctrl+v'],
                        env=env, stderr=subprocess.DEVNULL,
                    )
                injected = True


        if injected:
            # P0.5: desktop notification
            self._send_notification(text)
            # P0.6 + P1.2: word count + history (callback receives total_words, text)
            self._session_words += len(text.split())
            if self.word_count_callback:
                self.word_count_callback(self._session_words, text)
        else:
            print("All injection methods failed.")

    @property
    def session_word_count(self):
        """P0.6: Total words injected this session."""
        return self._session_words


    def _route_text(self, text: str, target_id: str) -> bool:
        """Deliver text via the router; return True on success."""
        if self.router is None:
            return False
        result = self.router.deliver(text, target_id)
        if not result.success:
            print(f"[Router] Delivery failed for target '{target_id}': {result.error}")
        return result.success

    def run(self):
        while self.running:
            try:
                item = self.text_queue.get(timeout=0.1)
                # Accept both plain strings (legacy) and (text, target_id) tuples
                if isinstance(item, tuple):
                    text, target_id = item
                else:
                    text, target_id = item, 'default'

                if target_id != 'default' and self.router is not None:
                    # Non-default target: route through the router
                    delivered = self._route_text(text, target_id)
                    if delivered:
                        self._session_words += len(text.split())
                        if self.word_count_callback:
                            self.word_count_callback(self._session_words, text)
                else:
                    # Default target: use existing injection path
                    self.inject_text(text)
            except queue.Empty:
                continue
            except Exception as e:
                print(f"Error in text injection: {e}")

    def stop(self):
        self.running = False
