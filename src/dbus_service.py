"""
P2.3 — DBus Control Interface

Exposes Whisper Wayland as a DBus service so external tools (Waybar, KDE
widgets, shell scripts, rofi menus) can control dictation programmatically.

Service:   ai.whisperwayland.Dictation
Object:    /ai/whisperwayland/Dictation
Interface: ai.whisperwayland.Dictation

Methods (all callable via dbus-send or qdbus):
  StartRecording()   → void
  StopRecording()    → void
  ToggleRecording()  → void
  GetStatus()        → string  ("idle" | "recording" | "transcribing")
  GetWordCount()     → uint32

Example shell usage:
  dbus-send --session --type=method_call \\
    --dest=ai.whisperwayland.Dictation \\
    /ai/whisperwayland/Dictation \\
    ai.whisperwayland.Dictation.ToggleRecording

  qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetStatus

Graceful degradation:
  - If dbus-python or pygobject is not installed, DBusService is a no-op stub.
  - The rest of the app never sees an exception from this module.
"""

import threading

# ── Availability check ────────────────────────────────────────────────────
try:
    import dbus
    import dbus.service
    from dbus.mainloop.glib import DBusGMainLoop
    from gi.repository import GLib
    _HAS_DBUS = True
except ImportError:
    _HAS_DBUS = False

SERVICE_NAME = "ai.whisperwayland.Dictation"
OBJECT_PATH  = "/ai/whisperwayland/Dictation"
INTERFACE    = "ai.whisperwayland.Dictation"


# ── Stub used when dbus-python is not installed ───────────────────────────
class _DBusServiceStub:
    """Drop-in stub when dbus-python is unavailable."""
    available = False

    def __init__(self, *_, **__):
        pass

    def start(self):
        print("[DBus] dbus-python not installed — DBus service disabled.")
        print("[DBus] Install with: pip install dbus-python  (also needs: pacman -S python-dbus)")

    def stop(self):
        pass

    def set_status(self, _status: str):
        pass

    def set_word_count(self, _n: int):
        pass


# ── Real implementation ───────────────────────────────────────────────────
if _HAS_DBUS:
    class _WhisperDBusObject(dbus.service.Object):
        """The actual DBus object that external tools call."""

        def __init__(self, bus, on_start, on_stop, on_toggle):
            super().__init__(bus, OBJECT_PATH)
            self._on_start  = on_start
            self._on_stop   = on_stop
            self._on_toggle = on_toggle
            self._status     = "idle"
            self._word_count = 0

        # ── DBus methods ──────────────────────────────────────────────────
        @dbus.service.method(INTERFACE, in_signature="", out_signature="")
        def StartRecording(self):
            print("[DBus] StartRecording called")
            self._on_start()

        @dbus.service.method(INTERFACE, in_signature="", out_signature="")
        def StopRecording(self):
            print("[DBus] StopRecording called")
            self._on_stop()

        @dbus.service.method(INTERFACE, in_signature="", out_signature="")
        def ToggleRecording(self):
            print("[DBus] ToggleRecording called")
            self._on_toggle()

        @dbus.service.method(INTERFACE, in_signature="", out_signature="s")
        def GetStatus(self) -> str:
            return self._status

        @dbus.service.method(INTERFACE, in_signature="", out_signature="u")
        def GetWordCount(self) -> int:
            return self._word_count

        # ── DBus signals ──────────────────────────────────────────────────
        @dbus.service.signal(INTERFACE, signature="s")
        def StatusChanged(self, status: str):
            """Emitted whenever recording/transcribing state changes."""

        @dbus.service.signal(INTERFACE, signature="s")
        def TextInjected(self, text: str):
            """Emitted after each successful text injection."""

        # ── Internal state setters (called from the Qt thread) ────────────
        def update_status(self, status: str):
            self._status = status
            self.StatusChanged(status)

        def update_word_count(self, n: int):
            self._word_count = n

        def notify_text(self, text: str):
            self.TextInjected(text)


class _DBusServiceReal:
    """Manages the GLib main loop thread and the DBus bus name."""
    available = True

    def __init__(self, on_start, on_stop, on_toggle):
        self._on_start  = on_start
        self._on_stop   = on_stop
        self._on_toggle = on_toggle
        self._loop   = None
        self._thread = None
        self._obj    = None

    def start(self):
        def _run():
            try:
                DBusGMainLoop(set_as_default=True)
                bus = dbus.SessionBus()
                bus_name = dbus.service.BusName(SERVICE_NAME, bus)
                self._obj = _WhisperDBusObject(
                    bus, self._on_start, self._on_stop, self._on_toggle
                )
                self._loop = GLib.MainLoop()
                print(f"[DBus] Service running as '{SERVICE_NAME}'")
                self._loop.run()
            except dbus.exceptions.DBusException as e:
                print(f"[DBus] Could not acquire bus name: {e}")
            except Exception as e:
                print(f"[DBus] Service error: {e}")

        self._thread = threading.Thread(target=_run, daemon=True, name="dbus-service")
        self._thread.start()

    def stop(self):
        if self._loop:
            self._loop.quit()

    def set_status(self, status: str):
        if self._obj:
            try:
                self._obj.update_status(status)
            except Exception:
                pass

    def set_word_count(self, n: int):
        if self._obj:
            try:
                self._obj.update_word_count(n)
            except Exception:
                pass

    def notify_text(self, text: str):
        if self._obj:
            try:
                self._obj.notify_text(text)
            except Exception:
                pass


# ── Public factory ────────────────────────────────────────────────────────
def DBusService(on_start, on_stop, on_toggle):
    """
    Returns a running DBus service, or a silent stub if dbus-python
    is not installed. Always safe to call .start() / .stop() on the result.
    """
    if _HAS_DBUS:
        return _DBusServiceReal(on_start, on_stop, on_toggle)
    return _DBusServiceStub(on_start, on_stop, on_toggle)
