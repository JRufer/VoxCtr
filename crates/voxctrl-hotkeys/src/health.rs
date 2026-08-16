//! Live state of the global hotkey listener.
//!
//! Without this the listener fails *silently*. On the portal path a compositor
//! that does not implement `GlobalShortcuts` simply answers "no such
//! interface"; on the evdev path `evdev::enumerate()` skips every device the
//! process cannot open. Either way the user gets an app whose shortcuts do
//! nothing and no indication why. The app needs to tell apart "shortcuts are
//! registered with the desktop", "the desktop has no portal so VoxCtrl would
//! have to read the keyboard directly, and may not" and "everything is fine",
//! both to explain itself at launch and to notice when the situation changes.

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

/// Which mechanism is delivering shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Nothing is listening yet.
    Starting,
    /// `org.freedesktop.portal.GlobalShortcuts` — the compositor owns the key
    /// grab and VoxCtrl reads no input devices.
    Portal,
    /// Reading `/dev/input/event*` directly. Only reachable when the user has
    /// already granted this process access to input devices.
    Evdev,
    /// The Win32 low-level keyboard hook.
    WindowsHook,
    /// No mechanism is available; shortcuts cannot fire.
    None,
}

/// One shortcut as the compositor actually bound it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BoundShortcut {
    /// Bindings that fire from this shortcut.
    pub binding_ids: Vec<String>,
    /// The accelerator VoxCtrl asked for, if the combination could be expressed.
    pub requested: Option<String>,
    /// How the desktop describes the shortcut to the user, e.g. "Meta+Space".
    /// Empty when the compositor did not say.
    pub trigger_description: String,
    /// The compositor acknowledged this shortcut.
    pub bound: bool,
}

#[derive(Debug, Default)]
pub struct ListenerHealth {
    keyboards_open: AtomicUsize,
    devices_denied: AtomicUsize,
    devices_total: AtomicUsize,
    scanned: AtomicBool,
    supported: AtomicBool,
    backend: Mutex<Option<Backend>>,
    /// Why the portal could not be used, if it could not.
    portal_error: Mutex<Option<String>>,
    /// The portal is present and answered, but refused the session. A different
    /// problem from "this desktop has no portal", and it needs different advice.
    portal_refused: AtomicBool,
    bound_shortcuts: Mutex<Vec<BoundShortcut>>,
}

impl ListenerHealth {
    /// Number of keyboard devices the evdev backend currently has open. Always
    /// zero on the portal backend, which reads no devices at all.
    pub fn keyboards_open(&self) -> usize {
        self.keyboards_open.load(Ordering::Relaxed)
    }

    /// Input devices that exist but could not be opened because of file
    /// permissions.
    pub fn devices_denied(&self) -> usize {
        self.devices_denied.load(Ordering::Relaxed)
    }

    /// Input event devices present on the system, readable or not.
    pub fn devices_total(&self) -> usize {
        self.devices_total.load(Ordering::Relaxed)
    }

    /// True once the evdev backend has completed at least one device scan.
    pub fn has_scanned(&self) -> bool {
        self.scanned.load(Ordering::Relaxed)
    }

    /// True on platforms where a global listener is implemented at all.
    pub fn is_supported(&self) -> bool {
        self.supported.load(Ordering::Relaxed)
    }

    pub fn backend(&self) -> Backend {
        self.backend
            .lock()
            .ok()
            .and_then(|b| *b)
            .unwrap_or(Backend::Starting)
    }

    /// Why the portal backend was not used. `None` means it was.
    pub fn portal_error(&self) -> Option<String> {
        self.portal_error.lock().ok().and_then(|e| e.clone())
    }

    /// The desktop has a shortcuts portal and it turned VoxCtrl away, rather
    /// than there being no portal at all. Telling the user to switch desktops
    /// would be useless advice in this state.
    pub fn portal_refused(&self) -> bool {
        self.portal_refused.load(Ordering::Relaxed)
    }

    pub fn bound_shortcuts(&self) -> Vec<BoundShortcut> {
        self.bound_shortcuts.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Hotkeys can fire right now.
    pub fn is_active(&self) -> bool {
        if !self.is_supported() {
            return true;
        }
        match self.backend() {
            Backend::Portal | Backend::WindowsHook => true,
            Backend::Evdev => self.keyboards_open() > 0,
            // Still deciding which backend to use — don't report a problem yet.
            Backend::Starting => true,
            Backend::None => false,
        }
    }

    /// VoxCtrl has fallen back to reading input devices and cannot: the portal
    /// is unavailable *and* the keyboard is unreadable. This is the one state
    /// that needs the user to do something outside the app.
    pub fn permission_blocked(&self) -> bool {
        self.backend() == Backend::None && self.devices_denied() > 0
    }

    /// Shortcuts are working without VoxCtrl having any access to input devices.
    pub fn is_private(&self) -> bool {
        matches!(self.backend(), Backend::Portal | Backend::WindowsHook)
    }

    pub fn set_supported(&self, supported: bool) {
        self.supported.store(supported, Ordering::Relaxed);
    }

    pub fn set_backend(&self, backend: Backend) {
        if let Ok(mut b) = self.backend.lock() {
            *b = Some(backend);
        }
    }

    pub fn set_portal_error(&self, error: String) {
        if let Ok(mut e) = self.portal_error.lock() {
            *e = Some(error);
        }
    }

    pub fn clear_portal_error(&self) {
        if let Ok(mut e) = self.portal_error.lock() {
            *e = None;
        }
    }

    pub fn set_portal_refused(&self, refused: bool) {
        self.portal_refused.store(refused, Ordering::Relaxed);
    }

    pub fn set_bound_shortcuts(&self, shortcuts: Vec<BoundShortcut>) {
        if let Ok(mut s) = self.bound_shortcuts.lock() {
            *s = shortcuts;
        }
    }

    /// A backend that was running has stopped and cannot recover on its own.
    pub fn set_backend_failed(&self, reason: String) {
        self.set_backend(Backend::None);
        self.set_portal_error(reason);
        self.set_bound_shortcuts(Vec::new());
    }

    pub fn set_keyboards_open(&self, n: usize) {
        self.keyboards_open.store(n, Ordering::Relaxed);
    }

    pub fn record_scan(&self, total: usize, denied: usize) {
        self.devices_total.store(total, Ordering::Relaxed);
        self.devices_denied.store(denied, Ordering::Relaxed);
        self.scanned.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_never_reports_a_problem() {
        let h = ListenerHealth::default();
        h.record_scan(0, 0);
        assert!(h.is_active(), "platforms without a listener must not warn");
        assert!(!h.permission_blocked());
    }

    #[test]
    fn the_portal_backend_is_active_without_reading_any_device() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.set_backend(Backend::Portal);
        h.record_scan(6, 6); // every device denied — and it does not matter

        assert!(h.is_active());
        assert!(h.is_private());
        assert!(
            !h.permission_blocked(),
            "the portal needs no device permissions, so nothing is blocked"
        );
    }

    #[test]
    fn no_portal_and_no_readable_keyboard_is_a_permission_problem() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.set_portal_error("no such interface".to_string());
        h.set_backend(Backend::None);
        h.record_scan(6, 6);
        h.set_keyboards_open(0);

        assert!(!h.is_active());
        assert!(!h.is_private());
        assert!(h.permission_blocked());
    }

    #[test]
    fn the_evdev_fallback_is_active_but_not_private() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.set_portal_error("no such interface".to_string());
        h.set_backend(Backend::Evdev);
        h.record_scan(6, 2);
        h.set_keyboards_open(1);

        assert!(h.is_active());
        assert!(!h.is_private(), "reading evdev means seeing every keystroke");
        assert!(!h.permission_blocked());
    }

    #[test]
    fn a_machine_with_no_input_devices_is_not_a_permission_problem() {
        // Headless CI or a VM is broken in a different way; telling the user to
        // fix permissions would be wrong.
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.set_backend(Backend::None);
        h.record_scan(0, 0);
        assert!(!h.permission_blocked());
    }

    #[test]
    fn nothing_is_reported_before_a_backend_is_chosen() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        assert_eq!(h.backend(), Backend::Starting);
        assert!(h.is_active(), "startup must not flash a failure");
        assert!(!h.permission_blocked());
    }

    #[test]
    fn a_refused_portal_is_distinguished_from_a_missing_one() {
        // "Your desktop has no shortcuts portal" and "your desktop refused us"
        // need different advice, so they cannot collapse into one state.
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.set_portal_error("An app id is required".to_string());
        h.set_portal_refused(true);
        h.set_backend(Backend::None);

        assert!(h.portal_refused());
        assert!(!h.is_active());

        let missing = ListenerHealth::default();
        missing.set_supported(true);
        missing.set_portal_error("no such interface".to_string());
        missing.set_backend(Backend::None);
        assert!(!missing.portal_refused());
    }

    #[test]
    fn a_lost_portal_session_stops_reporting_as_active() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.set_backend(Backend::Portal);
        h.set_bound_shortcuts(vec![BoundShortcut {
            binding_ids: vec!["a".into()],
            requested: Some("LOGO+space".into()),
            trigger_description: "Meta+Space".into(),
            bound: true,
        }]);
        assert!(h.is_active());

        h.set_backend_failed("the portal session ended".to_string());
        assert!(!h.is_active());
        assert!(h.bound_shortcuts().is_empty());
    }
}
