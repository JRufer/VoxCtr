pub mod gestures;
mod health;
mod keys;
pub mod trigger;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub mod portal;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use tokio::sync::mpsc;
use voxctrl_routing::HotkeyBinding;

pub use gestures::{GestureEvent, GestureKind};
pub use health::{Backend, BoundShortcut, ListenerHealth};
pub use trigger::{accelerator, is_modifier, TriggerProblem};

/// Callback channel: the listener sends GestureEvents to the app coordinator.
pub type GestureSender = mpsc::UnboundedSender<GestureEvent>;
pub type GestureReceiver = mpsc::UnboundedReceiver<GestureEvent>;

pub type ReloaderSender = crossbeam_channel::Sender<Vec<HotkeyBinding>>;
pub type ReloaderReceiver = crossbeam_channel::Receiver<Vec<HotkeyBinding>>;

pub fn channel() -> (GestureSender, GestureReceiver) {
    mpsc::unbounded_channel()
}

/// Start the global hotkey listener. Bindings can be updated at runtime through
/// the returned handle.
///
/// On Linux this prefers the XDG desktop portal, where the compositor owns the
/// key grab and VoxCtrl is told nothing except that its own shortcut fired. The
/// evdev fallback is only used when the portal is unavailable *and* the user
/// has already given this process access to input devices — VoxCtrl never asks
/// for that access, because granting it lets every program running as the user
/// read the keyboard, not just this one.
///
/// `ListenerHandle::health` reports which of those happened, so the app can say
/// so at launch instead of failing silently.
pub fn start_listener(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    device_path: Option<String>,
    health: Arc<ListenerHealth>,
) -> ListenerHandle {
    let (reloader_tx, reloader_rx) = crossbeam_channel::unbounded();

    #[cfg(target_os = "linux")]
    {
        linux::start(bindings, tx, device_path, reloader_rx, health.clone());
    }
    #[cfg(target_os = "windows")]
    {
        let _ = device_path;
        windows::start(bindings, tx, reloader_rx, health.clone());
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (bindings, tx, device_path, reloader_rx);
        health.set_supported(false);
        tracing::warn!("Hotkey listener not supported on this platform");
    }

    ListenerHandle { reloader_tx }
}

/// Opaque handle; drop to stop the listener.
pub struct ListenerHandle {
    pub reloader_tx: ReloaderSender,
}
