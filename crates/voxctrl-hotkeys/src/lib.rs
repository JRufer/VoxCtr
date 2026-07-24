pub mod gestures;

#[cfg(target_os = "linux")]
mod linux;
// rdev-based listener shared by Windows and macOS (both lack a raw evdev
// equivalent; rdev taps the OS input event stream on each).
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod rdev_backend;

use tokio::sync::mpsc;
use voxctrl_routing::HotkeyBinding;

pub use gestures::{GestureEvent, GestureKind};

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
pub fn start_listener(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    device_path: Option<String>,
) -> ListenerHandle {
    let (reloader_tx, reloader_rx) = crossbeam_channel::unbounded();

    #[cfg(target_os = "linux")]
    {
        linux::start(bindings, tx, device_path, reloader_rx);
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        rdev_backend::start(bindings, tx, reloader_rx);
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        tracing::warn!("Hotkey listener not supported on this platform");
    }

    ListenerHandle { reloader_tx }
}

/// Opaque handle; drop to stop the listener.
pub struct ListenerHandle {
    pub reloader_tx: ReloaderSender,
}
