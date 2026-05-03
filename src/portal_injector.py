import dbus
import time
import os
import threading

class PortalInjector:
    """
    Native Wayland Text Injection using the xdg-desktop-portal RemoteDesktop interface.
    This provides a fallback for GNOME and sandboxed environments where /dev/uinput is unavailable.
    """
    def __init__(self):
        self.bus = None
        self.portal = None
        self.session_path = None
        self.initialized = False
        self._lock = threading.Lock()
        
        # Simple mapping for common ASCII characters to keysyms (X11/XKB standard)
        # 0x20-0x7e are standard ASCII
        self._keysym_map = {chr(i): i for i in range(32, 127)}
        self._keysym_map.update({
            "\n": 0xFF0D, # Return
            "\t": 0xFF09, # Tab
            " ": 0x0020,  # Space
        })

    def _setup(self):
        """Initializes the portal session. This WILL trigger a user permissions dialog on first run."""
        try:
            self.bus = dbus.SessionBus()
            self.portal = self.bus.get_object("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop")
            self.iface = dbus.Interface(self.portal, "org.freedesktop.portal.RemoteDesktop")
            
            # 1. Create Session
            options = {"session_handle_token": "whisper_wayland_session"}
            request_path = self.iface.CreateSession(options)
            
            # Note: In a production app we'd wait for the 'Response' signal here.
            # For this implementation, we assume the user accepts or the portal handles it.
            # We'll use a slightly hacky approach: try to proceed and catch errors if not ready.
            # The session path is predictable if we use a token.
            sender_name = self.bus.get_unique_name().replace(".", "_").replace(":", "_")
            self.session_path = f"/org/freedesktop/portal/desktop/session/{sender_name}/whisper_wayland_session"
            
            # 2. Select Devices (KEYBOARD = 1)
            self.iface.SelectDevices(self.session_path, {"types": dbus.UInt32(1)})
            
            # 3. Start
            self.iface.Start(self.session_path, "", {})
            
            self.initialized = True
            print("[Portal] RemoteDesktop session initialized.")
            return True
        except Exception as e:
            print(f"[Portal] Failed to initialize: {e}")
            return False

    def inject(self, text: str) -> bool:
        with self._lock:
            if not self.initialized:
                if not self._setup():
                    return False
            
            try:
                session_obj = self.bus.get_object("org.freedesktop.portal.Desktop", self.session_path)
                session_iface = dbus.Interface(session_obj, "org.freedesktop.portal.RemoteDesktop")
                
                for char in text:
                    # Very basic injection: map character to keysym and send Press/Release
                    # This handles ASCII. Unicode would require a more complex layout mapping.
                    keysym = self._keysym_map.get(char)
                    if keysym:
                        # NotifyKeyboardKeysym(options, keysym, state)
                        # state: 1=Pressed, 0=Released
                        session_iface.NotifyKeyboardKeysym({}, dbus.UInt32(keysym), dbus.UInt32(1))
                        time.sleep(0.005)
                        session_iface.NotifyKeyboardKeysym({}, dbus.UInt32(keysym), dbus.UInt32(0))
                        time.sleep(0.005)
                return True
            except Exception as e:
                print(f"[Portal] Injection error: {e}")
                self.initialized = False # Force re-setup next time
                return False
