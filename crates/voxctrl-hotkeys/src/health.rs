//! Live health of the global hotkey listener.
//!
//! Without this the listener fails *silently*: on Linux, `evdev::enumerate()`
//! simply skips every device the process cannot open, so a user who has not
//! finished the permission setup gets an empty keyboard list, a single `warn!`
//! line in a log they never read, and hotkeys that do nothing. The app needs to
//! be able to tell "no keyboard is readable because permissions are missing"
//! apart from "everything is fine", both to warn the user and to notice the
//! moment the situation repairs itself.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct ListenerHealth {
    keyboards_open: AtomicUsize,
    devices_denied: AtomicUsize,
    devices_total: AtomicUsize,
    scanned: AtomicBool,
    supported: AtomicBool,
}

impl ListenerHealth {
    /// Number of keyboard devices the listener currently has open and is
    /// reading key events from. Zero means no hotkey can ever fire.
    pub fn keyboards_open(&self) -> usize {
        self.keyboards_open.load(Ordering::Relaxed)
    }

    /// Input devices that exist but could not be opened because of file
    /// permissions — the signature of an unfinished install.
    pub fn devices_denied(&self) -> usize {
        self.devices_denied.load(Ordering::Relaxed)
    }

    /// Input event devices present on the system, readable or not.
    pub fn devices_total(&self) -> usize {
        self.devices_total.load(Ordering::Relaxed)
    }

    /// True once the listener has completed at least one device scan, so the
    /// UI does not report "no keyboard" during the first few milliseconds of
    /// startup.
    pub fn has_scanned(&self) -> bool {
        self.scanned.load(Ordering::Relaxed)
    }

    /// True on platforms where a global listener is implemented at all.
    pub fn is_supported(&self) -> bool {
        self.supported.load(Ordering::Relaxed)
    }

    /// Hotkeys can fire right now.
    pub fn is_active(&self) -> bool {
        !self.is_supported() || self.keyboards_open() > 0
    }

    /// Input devices exist but none of them can be read: the app is blind to
    /// the keyboard and the user must finish the permission setup.
    pub fn permission_blocked(&self) -> bool {
        self.is_supported()
            && self.has_scanned()
            && self.keyboards_open() == 0
            && self.devices_denied() > 0
    }

    pub fn set_supported(&self, supported: bool) {
        self.supported.store(supported, Ordering::Relaxed);
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
    fn denied_devices_with_no_open_keyboard_is_a_permission_problem() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.record_scan(6, 6);
        h.set_keyboards_open(0);
        assert!(!h.is_active());
        assert!(h.permission_blocked());
    }

    #[test]
    fn an_open_keyboard_clears_the_permission_problem() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.record_scan(6, 2);
        h.set_keyboards_open(1);
        assert!(h.is_active());
        assert!(!h.permission_blocked(), "some devices are always unreadable");
    }

    #[test]
    fn no_devices_at_all_is_not_reported_as_a_permission_problem() {
        // A machine with no input devices (headless CI, VM) is broken in a
        // different way; telling the user to fix permissions would be wrong.
        let h = ListenerHealth::default();
        h.set_supported(true);
        h.record_scan(0, 0);
        assert!(!h.permission_blocked());
    }

    #[test]
    fn nothing_is_reported_before_the_first_scan() {
        let h = ListenerHealth::default();
        h.set_supported(true);
        assert!(!h.permission_blocked());
    }
}
