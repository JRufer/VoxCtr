use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::Manager;
use crate::commands;
use crate::state::AppState;

/// Set once the Tauri app is built, so background tasks started before it (the
/// hotkey gesture loop) can raise windows.
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Label of the first-run setup window.
pub const SETUP_WINDOW: &str = "udev-warning";

/// Minimum gap between "finish the setup" notifications, so holding a
/// push-to-talk key does not produce a wall of toasts.
pub const SETUP_NOTICE_INTERVAL: Duration = Duration::from_secs(60);

/// How often the setup watcher re-checks the listener. Short enough that a
/// change made elsewhere — the portal coming up, a shortcut reassigned in the
/// desktop's settings — visibly flips the app to working within a couple of
/// seconds.
pub const SETUP_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Re-alert cadence while nothing can deliver shortcuts at all. In that state
/// no shortcut can reach the app, so this is the only way the user hears about
/// it while they are pressing keys and getting nothing.
pub const BLIND_ALERT_INTERVAL: Duration = Duration::from_secs(300);

pub fn set_app_handle(handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

pub fn get_app_handle() -> Option<tauri::AppHandle> {
    APP_HANDLE.get().cloned()
}

/// Bring the setup window to the front.
pub fn show_setup_window() {
    if let Some(handle) = APP_HANDLE.get() {
        if let Some(w) = handle.get_webview_window(SETUP_WINDOW) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_always_on_top(true);
            let _ = w.set_focus();
        }
    }
}

/// What is stopping dictation from working end to end, as a message to show the
/// user. `None` means the app is fully set up.
///
/// Called on every hotkey activation: pressing the shortcut and getting silence
/// is the moment the user needs to be told the install is unfinished, and it is
/// the only moment we know for certain they are trying to dictate.
pub async fn setup_blocker(state: &Arc<AppState>) -> Option<String> {
    if let Some(tool) = commands::missing_injection_tool() {
        return Some(format!(
            "VoxCtrl cannot type text into other windows: '{tool}' is not installed. \
             Finish the setup in VoxCtrl → Settings to install it."
        ));
    }

    let cfg = state.config.lock().await;
    let eng = &cfg.data.engine;
    // Moonshine only bypasses the Whisper-model check when it is actually
    // compiled in; otherwise the app silently falls back to whisper-cpp and
    // still needs the model.
    let uses_whisper_model = eng.backend != voxctrl_config::BackendChoice::Moonshine
        || !voxctrl_inference::MOONSHINE_COMPILED;
    if !uses_whisper_model {
        return None;
    }
    // Skip small models the startup hook auto-downloads in the background —
    // that flow has its own "downloading…/ready/failed" notifications, so this
    // would only add a confusing "go to Settings" message mid-download.
    if voxctrl_inference::whisper_cpp::is_small_auto_downloadable(&eng.whisper_cpp.model_size) {
        return None;
    }
    if voxctrl_inference::whisper_cpp::is_model_downloaded(
        &eng.whisper_cpp.model_size,
        &eng.whisper_cpp.model_dir,
    ) {
        return None;
    }

    Some(format!(
        "Speech model '{}' is not downloaded — dictation cannot produce text. \
         Open Settings → Engine and download it.",
        eng.whisper_cpp.model_size
    ))
}

/// Helper to robustly show, unminimize, and focus a window, especially under Linux WMs
pub fn show_and_focus_window(window: &tauri::WebviewWindow) {
    let w = window.clone();
    tauri::async_runtime::spawn(async move {
        let mut pos: Option<tauri::PhysicalPosition<i32>> = None;
        #[cfg(target_os = "linux")]
        {
            // If the window is already open/visible, we must hide it first and wait a short period
            // (150ms) to allow the Linux window manager (GNOME/Mutter) to fully unmap it.
            // Showing it again triggers a brand new window mapping event, which bypasses Wayland/GNOME's
            // Focus Stealing Prevention, robustly bringing it to the foreground with active keyboard focus.
            if w.is_visible().unwrap_or(false) {
                pos = w.outer_position().ok();
                let _ = w.hide();
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }

        let _ = w.unminimize();
        let _ = w.show();
        #[cfg(target_os = "linux")]
        {
            if let Some(p) = pos {
                let _ = w.set_position(p);
            }
        }
        let _ = w.set_focus();
    });
}
