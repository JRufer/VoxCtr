import dbus
import time
import os
import threading

try:
    from dbus.mainloop.glib import DBusGMainLoop
    _HAS_GLIB_LOOP = True
except ImportError:
    _HAS_GLIB_LOOP = False


class PortalInjector:
    """
    Native Wayland text injection via the xdg-desktop-portal RemoteDesktop interface.
    Fallback for GNOME and sandboxed environments where /dev/uinput is unavailable.

    Session setup is fully asynchronous: each portal call (CreateSession, SelectDevices,
    Start) returns a Request object path and signals completion via a Response signal on
    that path. This implementation waits for each Response before proceeding, so it works
    correctly regardless of portal implementation details.

    Requires a GLib main loop to be running in the process (provided by DBusService).
    """

    def __init__(self):
        self.bus = None
        self.iface = None
        self.session_path = None
        self.initialized = False
        self._lock = threading.Lock()

    # ── Keysym resolution ────────────────────────────────────────────────────

    @staticmethod
    def _char_to_keysym(char: str) -> int | None:
        cp = ord(char)
        if 0x20 <= cp <= 0x7E:
            return cp           # direct ASCII: keysym == codepoint
        if cp == 0x0A:
            return 0xFF0D       # Return
        if cp == 0x09:
            return 0xFF09       # Tab
        if cp > 0x7E:
            return 0x01000000 + cp  # XKB Unicode keysym (U+0080 and above)
        return None

    # ── Session lifecycle ────────────────────────────────────────────────────

    def _wait_response(self, sender: str, token: str, timeout: float = 30.0) -> tuple[int, dict] | None:
        """
        Subscribe to the Response signal for a portal request, then return
        (response_code, results) once it fires, or None on timeout.

        Must be called BEFORE issuing the portal call that creates the request,
        because the signal can fire before this method returns on fast portals.
        """
        req_path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
        event = threading.Event()
        result_holder = [None]

        def _on_response(response_code, results):
            result_holder[0] = (int(response_code), dict(results))
            event.set()

        self.bus.add_signal_receiver(
            _on_response,
            signal_name="Response",
            dbus_interface="org.freedesktop.portal.Request",
            path=req_path,
        )
        return event, result_holder, req_path

    def _setup(self):
        """
        Initialise the RemoteDesktop portal session.
        Blocks (with timeout) while waiting for async portal responses.
        On first call this will show a compositor permission dialog.
        """
        try:
            if not _HAS_GLIB_LOOP:
                print("[Portal] dbus-python GLib main loop unavailable; skipping portal init.")
                return False

            self.bus = dbus.SessionBus()
            portal_obj = self.bus.get_object(
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
            )
            self.iface = dbus.Interface(portal_obj, "org.freedesktop.portal.RemoteDesktop")

            # Derive the sender name used in request/session paths.
            # dbus unique names look like ":1.42"; portal converts them to "_1_42".
            sender = self.bus.get_unique_name().replace(".", "_").replace(":", "_")

            # ── 1. CreateSession ──────────────────────────────────────────────
            req_token1     = "ww_create"
            session_token  = "ww_session"
            evt1, res1, _  = self._wait_response(sender, req_token1)

            self.iface.CreateSession({
                "handle_token":         req_token1,
                "session_handle_token": session_token,
            })

            if not evt1.wait(timeout=30):
                print("[Portal] Timeout waiting for CreateSession response.")
                return False
            code1, data1 = res1[0]
            if code1 != 0:
                print(f"[Portal] CreateSession failed (response={code1}).")
                return False
            self.session_path = str(data1.get("session_handle", ""))
            if not self.session_path:
                print("[Portal] CreateSession returned no session_handle.")
                return False

            # ── 2. SelectDevices (KEYBOARD = 1) ───────────────────────────────
            req_token2     = "ww_select"
            evt2, res2, _  = self._wait_response(sender, req_token2)

            self.iface.SelectDevices(
                self.session_path,
                {"handle_token": req_token2, "types": dbus.UInt32(1)},
            )
            evt2.wait(timeout=30)   # failure here is non-fatal; Start will catch it

            # ── 3. Start (triggers compositor permission dialog if needed) ─────
            req_token3     = "ww_start"
            evt3, res3, _  = self._wait_response(sender, req_token3)

            self.iface.Start(self.session_path, "", {"handle_token": req_token3})

            if not evt3.wait(timeout=30):
                print("[Portal] Timeout waiting for Start response (user did not respond to dialog).")
                return False
            code3, _ = res3[0]
            if code3 != 0:
                print(f"[Portal] Start denied or failed (response={code3}).")
                return False

            self.initialized = True
            print("[Portal] RemoteDesktop session initialized.")
            return True

        except Exception as e:
            print(f"[Portal] Failed to initialize: {e}")
            return False

    # ── Text injection ────────────────────────────────────────────────────────

    def inject(self, text: str) -> bool:
        with self._lock:
            if not self.initialized:
                if not self._setup():
                    return False

            try:
                session_obj = self.bus.get_object(
                    "org.freedesktop.portal.Desktop",
                    self.session_path,
                )
                session_iface = dbus.Interface(
                    session_obj,
                    "org.freedesktop.portal.RemoteDesktop",
                )

                for char in text:
                    keysym = self._char_to_keysym(char)
                    if keysym is None:
                        continue
                    ks = dbus.UInt32(keysym)
                    session_iface.NotifyKeyboardKeysym({}, ks, dbus.UInt32(1))
                    time.sleep(0.005)
                    session_iface.NotifyKeyboardKeysym({}, ks, dbus.UInt32(0))
                    time.sleep(0.005)

                return True

            except Exception as e:
                print(f"[Portal] Injection error: {e}")
                self.initialized = False   # force re-setup on next call
                return False
