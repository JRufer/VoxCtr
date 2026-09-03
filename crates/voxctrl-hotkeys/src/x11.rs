//! Global shortcuts from X11 raw key events (XInput2).
//!
//! This is the backend for the large middle ground between "the compositor
//! serves the GlobalShortcuts portal" and "the user has already given this
//! process access to input devices": X11 desktops such as Cinnamon, MATE and
//! Xfce, which have no shortcuts portal and no intention of growing one.
//!
//! XInput2 raw key events are delivered to the root window independent of which
//! window has focus, so they work as global shortcuts, and — unlike the evdev
//! fallback — **any** X client may ask for them. There is no group to join, no
//! udev rule to install, and nothing for VoxCtrl to change about the machine.
//! That is what makes this backend usable where the evdev one is not.
//!
//! The version this declares to the server is load-bearing: see
//! `MIN_XI_MINOR`.
//!
//! It carries the same privacy cost as evdev, though: raw events are every key
//! the user presses, not just VoxCtrl's own shortcuts, so it ranks below the
//! portal and the app says which one it is running on.
//!
//! Unlike the portal, this sees individual presses and releases, so every
//! gesture style works here — hold and double-tap-hold included — as do
//! bare-modifier triggers like double-tapping Super, which no accelerator-based
//! backend can express.

use std::{collections::HashSet, sync::Arc};

use x11rb::{
    connection::Connection,
    protocol::{
        xinput::{self, ConnectionExt as _},
        Event,
    },
    rust_connection::RustConnection,
};

use voxctrl_routing::HotkeyBinding;

use crate::{
    linux::{is_synthetic_device_name, run_coordinator, ReaderEvent},
    Backend, GestureSender, ListenerHealth,
};

/// `XIAllMasterDevices`. Raw events are reported against the master device the
/// physical keyboard is attached to; selecting per-slave would mean re-selecting
/// on every hotplug.
const XI_ALL_MASTER_DEVICES: xinput::DeviceId = 1;

/// `XIAllDevices`, for querying the device list — the slave keyboards that
/// `sourceid` refers to are not master devices.
const XI_ALL_DEVICES: xinput::DeviceId = 0;

/// X11 keycodes are evdev codes offset by 8, fixed by the X11 protocol.
const X11_KEYCODE_OFFSET: u8 = 8;

/// XInput 2.1 is the floor, and not for a feature — for two behaviours that
/// this backend is unusable without.
///
/// Under XI 2.0 a raw event goes to the root window *or* to the grabbing
/// client, never both. Grabs are everywhere on a desktop: a compositor's own
/// key handling, and a passive grab every time a menu or dropdown opens. On
/// Cinnamon that means a hotkey pressed with any menu on screen is simply lost.
/// XI 2.1 delivers raw events to the root window at all times, whatever holds a
/// grab.
///
/// XI 2.1 is also where `sourceid` starts being set on raw events. Without it
/// every event claims to come from device 0, the XTEST filter below matches
/// nothing, and VoxCtrl reads back the transcription it just typed — retriggering
/// the hotkey inside its own output.
///
/// The server applies whichever semantics the client announces, so asking for
/// 2.0 would opt into both problems on a server that supports neither. A server
/// too old to offer 2.1 (pre-X.Org 1.11, 2011) is left to the evdev fallback
/// rather than silently given a backend that mis-handles its own keystrokes.
const MIN_XI_MINOR: u16 = 1;
const MIN_XI_MAJOR: u16 = 2;

/// Does a negotiated XInput version deliver raw events this backend can trust?
fn supports_reliable_raw_events(major: u16, minor: u16) -> bool {
    (major, minor) >= (MIN_XI_MAJOR, MIN_XI_MINOR)
}

/// Why the X11 backend could not be used. Never fatal: the caller falls through
/// to evdev.
#[derive(Debug, Clone)]
pub enum X11Error {
    /// No X server to talk to — a Wayland session, or no session at all.
    NoDisplay,
    /// There is a `DISPLAY`, but the connection failed.
    Connect(String),
    /// The X server is too old for XInput2, or has the extension disabled.
    NoXInput(String),
    /// The server refused the event selection.
    Select(String),
    /// Turned off by `VOXCTRL_DISABLE_X11_HOTKEYS`.
    Disabled,
}

impl std::fmt::Display for X11Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDisplay => write!(f, "this is not an X11 session"),
            Self::Connect(e) => write!(f, "cannot connect to the X server: {e}"),
            Self::NoXInput(e) => write!(f, "this X server has no usable XInput2: {e}"),
            Self::Select(e) => write!(f, "the X server refused raw key events: {e}"),
            Self::Disabled => write!(f, "disabled by VOXCTRL_DISABLE_X11_HOTKEYS"),
        }
    }
}

/// Connect, claim raw key events, and start dispatching them into `tx`.
///
/// Everything that can fail happens before this returns, so a caller that gets
/// `Err` knows the backend is not running and can fall through to the next one
/// without racing a thread that is still deciding.
pub fn start(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: crate::ReloaderReceiver,
    health: Arc<ListenerHealth>,
) -> Result<(), X11Error> {
    if std::env::var_os("VOXCTRL_DISABLE_X11_HOTKEYS").is_some() {
        return Err(X11Error::Disabled);
    }
    if std::env::var_os("DISPLAY").is_none() {
        return Err(X11Error::NoDisplay);
    }

    let (conn, _screen) = RustConnection::connect(None)
        .map_err(|e| X11Error::Connect(format!("{e}")))?;

    // The announced version decides the server's raw-event semantics, so this
    // asks for the lowest one whose semantics are correct — see `MIN_XI_MINOR`.
    let version = conn
        .xinput_xi_query_version(MIN_XI_MAJOR, MIN_XI_MINOR)
        .map_err(|e| X11Error::NoXInput(format!("{e}")))?
        .reply()
        .map_err(|e| X11Error::NoXInput(format!("{e}")))?;
    if !supports_reliable_raw_events(version.major_version, version.minor_version) {
        return Err(X11Error::NoXInput(format!(
            "the server offers XInput {}.{}, and reliable raw key events need \
             {MIN_XI_MAJOR}.{MIN_XI_MINOR}",
            version.major_version, version.minor_version
        )));
    }

    let mask = xinput::EventMask {
        deviceid: XI_ALL_MASTER_DEVICES,
        mask: vec![
            xinput::XIEventMask::RAW_KEY_PRESS
                | xinput::XIEventMask::RAW_KEY_RELEASE
                // Hotplug: a keyboard added later, or a new XTEST device, changes
                // which source ids are worth listening to.
                | xinput::XIEventMask::HIERARCHY,
        ],
    };

    // Raw events may only be selected on a root window. Every screen gets its
    // own selection so a multi-screen (not multi-monitor) display still works.
    let roots: Vec<u32> = conn.setup().roots.iter().map(|s| s.root).collect();
    for root in &roots {
        conn.xinput_xi_select_events(*root, std::slice::from_ref(&mask))
            .map_err(|e| X11Error::Select(format!("{e}")))?
            .check()
            .map_err(|e| X11Error::Select(format!("{e}")))?;
    }
    conn.flush().map_err(|e| X11Error::Select(format!("{e}")))?;

    let synthetic = synthetic_source_ids(&conn);

    let rt_handle = tokio::runtime::Handle::try_current().ok();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<ReaderEvent>();

    let rt = rt_handle.clone();
    std::thread::Builder::new()
        .name("voxctrl-hotkey-coord".into())
        .spawn(move || {
            let _guard = rt.as_ref().map(|h| h.enter());
            run_coordinator(bindings, tx, rx_reload, event_rx);
        })
        .map_err(|e| X11Error::Select(format!("cannot start the hotkey coordinator: {e}")))?;

    let reader_health = health.clone();
    std::thread::Builder::new()
        .name("voxctrl-x11-keys".into())
        .spawn(move || run_reader(conn, synthetic, event_tx, reader_health))
        .map_err(|e| X11Error::Select(format!("cannot start the X11 reader: {e}")))?;

    health.set_backend(Backend::X11);
    tracing::info!(
        "Global shortcuts are coming from X11 raw key events; no input-device access was \
         needed and none was requested"
    );
    Ok(())
}

/// Source ids that VoxCtrl must ignore, so it never reads back the keystrokes
/// it injects itself.
///
/// `xdotool` and `wtype` type through XTEST, which appears here as its own
/// device. Without this, injecting a transcription that contains the hotkey
/// would retrigger the hotkey.
fn synthetic_source_ids(conn: &RustConnection) -> HashSet<xinput::DeviceId> {
    let Ok(cookie) = conn.xinput_xi_query_device(XI_ALL_DEVICES) else {
        return HashSet::new();
    };
    let Ok(reply) = cookie.reply() else {
        return HashSet::new();
    };
    reply
        .infos
        .iter()
        .filter(|info| is_synthetic_device_name(&String::from_utf8_lossy(&info.name)))
        .map(|info| info.deviceid)
        .collect()
}

fn run_reader(
    conn: RustConnection,
    mut synthetic: HashSet<xinput::DeviceId>,
    event_tx: crossbeam_channel::Sender<ReaderEvent>,
    health: Arc<ListenerHealth>,
) {
    loop {
        let event = match conn.wait_for_event() {
            Ok(e) => e,
            Err(e) => {
                // The X server went away — a logout, or the session ending. The
                // connection cannot be revived, and anything held at that moment
                // will never be seen coming back up.
                tracing::warn!("X11 hotkey connection lost: {e}");
                let _ = event_tx.send(ReaderEvent::SourceLost);
                health.set_backend_failed(format!("the X11 connection ended: {e}"));
                return;
            }
        };

        let (raw, down) = match event {
            Event::XinputRawKeyPress(ev) => (ev, true),
            Event::XinputRawKeyRelease(ev) => (ev, false),
            Event::XinputHierarchy(_) => {
                synthetic = synthetic_source_ids(&conn);
                continue;
            }
            _ => continue,
        };

        // Auto-repeat is not a new press.
        if down && raw.flags.contains(xinput::KeyEventFlags::KEY_REPEAT) {
            continue;
        }
        if synthetic.contains(&raw.sourceid) {
            continue;
        }
        let Some(name) = key_name(raw.detail) else {
            continue;
        };
        if event_tx.send(ReaderEvent::Key { name, down }).is_err() {
            // The coordinator is gone; nothing left to report to.
            return;
        }
    }
}

/// X11 keycode → the evdev key name the rest of VoxCtrl speaks.
///
/// Names come from the evdev crate rather than a table of our own, so the X11
/// and evdev backends cannot disagree about what a key is called and a binding
/// recorded under one backend keeps working under the other.
fn key_name(keycode: u32) -> Option<String> {
    let code = u16::try_from(keycode).ok()?;
    let code = code.checked_sub(u16::from(X11_KEYCODE_OFFSET))?;
    let name = format!("{:?}", evdev::Key::new(code));
    // Debug renders an unmapped code as "unknown key: N".
    if name.starts_with("KEY_") || name.starts_with("BTN_") {
        Some(name)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycodes_translate_to_the_same_names_evdev_reports() {
        // The offset is the whole reason a binding recorded on one backend
        // works on the other; an off-by-one here silently rebinds every key.
        assert_eq!(key_name(38).as_deref(), Some("KEY_A"));
        assert_eq!(key_name(65).as_deref(), Some("KEY_SPACE"));
        assert_eq!(key_name(133).as_deref(), Some("KEY_LEFTMETA"));
        assert_eq!(key_name(37).as_deref(), Some("KEY_LEFTCTRL"));
    }

    #[test]
    fn keycodes_below_the_offset_are_not_keys() {
        // X11 reserves 0-7; subtracting would wrap into a nonsense key.
        for keycode in 0..8 {
            assert_eq!(key_name(keycode), None, "keycode {keycode} is not a key");
        }
    }

    #[test]
    fn unmapped_keycodes_are_dropped_rather_than_named() {
        // evdev's Debug renders these as "unknown key: N", which must never
        // reach a binding as if it were a key name.
        assert_eq!(key_name(60000), None);
    }

    #[test]
    fn xinput_2_0_is_refused_because_its_raw_events_cannot_be_trusted() {
        // Under 2.0 a raw event reaches the root window or the grabbing client,
        // never both — so every hotkey pressed with a menu open is lost — and
        // `sourceid` is unset, so the XTEST filter matches nothing and VoxCtrl
        // reads back its own injected text. Accepting 2.0 would look like it
        // worked right up until either bit us.
        assert!(!supports_reliable_raw_events(2, 0));
        assert!(!supports_reliable_raw_events(1, 9));

        assert!(supports_reliable_raw_events(2, 1));
        assert!(supports_reliable_raw_events(2, 4));
        assert!(supports_reliable_raw_events(3, 0));
    }

    #[test]
    fn injected_keystrokes_come_from_devices_this_backend_ignores() {
        // Typing a transcription that contains the hotkey must not retrigger it.
        for name in ["Virtual core XTEST keyboard", "VoxCtrl Virtual Keyboard"] {
            assert!(is_synthetic_device_name(name), "{name} must be ignored");
        }
        assert!(!is_synthetic_device_name("AT Translated Set 2 keyboard"));
    }
}
