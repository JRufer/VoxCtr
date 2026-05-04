"""Double-tap gesture state machine.

Uses threading.Timer (not asyncio) so it works correctly inside
the evdev reader thread without an event loop.
"""
import threading
from enum import Enum, auto


class DTState(Enum):
    IDLE        = auto()
    FIRST_DOWN  = auto()
    FIRST_UP    = auto()
    SECOND_DOWN = auto()


class DoubleTapMachine:
    """Per-binding state machine for double-tap + hold gesture detection.

    Call on_key_event() for every raw EV_KEY event from evdev.
    Supply key_code as the evdev key name string (e.g. "KEY_LEFTCTRL"),
    value as 1 (down), 0 (up), or 2 (repeat), and timestamp in seconds.
    """

    def __init__(self, binding, on_start, on_stop):
        """
        binding:  HotkeyBinding (keys, tap_ms, hold_threshold_ms)
        on_start: callable(binding) — begin recording
        on_stop:  callable(binding) — stop recording and deliver
        """
        self.binding = binding
        self.on_start = on_start
        self.on_stop = on_stop
        self.state = DTState.IDLE
        self._t_down1 = 0.0
        self._t_up1 = 0.0
        self._deadline_timer: threading.Timer | None = None
        self._lock = threading.Lock()

    # ── Public ──────────────────────────────────────────────────────────────

    def on_key_event(self, key_code: str, value: int, timestamp: float) -> None:
        """Process one EV_KEY event. Thread-safe."""
        with self._lock:
            self._process(key_code, value, timestamp)

    # ── Internal ─────────────────────────────────────────────────────────────

    def _process(self, key_code: str, value: int, timestamp: float) -> None:
        if key_code not in self.binding.keys:
            # A different key interrupted — abort if in first-tap window
            if self.state in (DTState.FIRST_DOWN, DTState.FIRST_UP):
                self._reset()
            return

        if value == 2:
            # Kernel key-repeat — ignore entirely to avoid false advances
            return

        if self.state == DTState.IDLE and value == 1:
            self._t_down1 = timestamp
            self.state = DTState.FIRST_DOWN

        elif self.state == DTState.FIRST_DOWN and value == 0:
            held_ms = (timestamp - self._t_down1) * 1000
            if held_ms >= self.binding.hold_threshold_ms:
                # Held too long — treat as a plain modifier hold, not a double-tap
                self._reset()
            else:
                self._t_up1 = timestamp
                self.state = DTState.FIRST_UP
                self._arm_deadline()

        elif self.state == DTState.FIRST_UP and value == 1:
            gap_ms = (timestamp - self._t_up1) * 1000
            if gap_ms <= self.binding.tap_ms:
                self._cancel_deadline()
                self.state = DTState.SECOND_DOWN
                self.on_start(self.binding)
            else:
                # Second tap arrived too late
                self._reset()

        elif self.state == DTState.SECOND_DOWN and value == 0:
            self.state = DTState.IDLE
            self.on_stop(self.binding)

    def _arm_deadline(self) -> None:
        self._cancel_deadline()
        self._deadline_timer = threading.Timer(
            self.binding.tap_ms / 1000.0, self._deadline_expired
        )
        self._deadline_timer.daemon = True
        self._deadline_timer.start()

    def _cancel_deadline(self) -> None:
        if self._deadline_timer is not None:
            self._deadline_timer.cancel()
            self._deadline_timer = None

    def _deadline_expired(self) -> None:
        with self._lock:
            if self.state == DTState.FIRST_UP:
                self._reset_locked()

    def _reset(self) -> None:
        """Reset without acquiring lock (already held)."""
        self._reset_locked()

    def _reset_locked(self) -> None:
        self._cancel_deadline()
        self.state = DTState.IDLE
