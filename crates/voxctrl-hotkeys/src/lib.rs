pub mod gestures;
mod health;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use tokio::sync::mpsc;
use voxctrl_routing::HotkeyBinding;

pub use gestures::{GestureEvent, GestureKind};
pub use health::ListenerHealth;

/// Callback channel: the listener sends GestureEvents to the app coordinator.
pub type GestureSender = mpsc::UnboundedSender<GestureEvent>;
pub type GestureReceiver = mpsc::UnboundedReceiver<GestureEvent>;

pub type ReloaderSender = crossbeam_channel::Sender<Vec<HotkeyBinding>>;
pub type ReloaderReceiver = crossbeam_channel::Receiver<Vec<HotkeyBinding>>;

pub fn channel() -> (GestureSender, GestureReceiver) {
    mpsc::unbounded_channel()
}

/// Start the platform-specific hotkey listener on a dedicated OS thread.
/// Bindings can be updated at runtime via `reload_bindings`.
///
/// On Linux the listener supervises itself: it keeps rescanning `/dev/input`
/// in the background, so hotkeys start working the moment the user gains
/// access to the input devices (or plugs in a keyboard) — without restarting
/// the app. `ListenerHandle::health` reports whether that has happened yet.
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
        tracing::warn!("Hotkey listener not supported on this platform");
    }

    ListenerHandle { reloader_tx }
}

/// Opaque handle; drop to stop the listener.
pub struct ListenerHandle {
    pub reloader_tx: ReloaderSender,
}
