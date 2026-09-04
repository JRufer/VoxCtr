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

/// Label of the first-launch setup wizard window.
pub const WIZARD_WINDOW: &str = "wizard";

/// The wizard's screens are laid out on a 16:9 stage — two engine cards side by
/// side, eight overlay thumbnails in a row, five voice cards in a row. Below
/// roughly 1280 logical pixels those rows wrap and the layout stops reading as
/// designed, so the window opens at 16:9 and refuses to be resized under a 16:9
/// floor. These must stay in step with the `wizard` entry in tauri.conf.json,
/// which is what a fresh install's first launch uses.
pub const WIZARD_WIDTH: f64 = 1600.0;
pub const WIZARD_HEIGHT: f64 = 900.0;
pub const WIZARD_MIN_WIDTH: f64 = 1280.0;
pub const WIZARD_MIN_HEIGHT: f64 = 720.0;

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

/// Show the Settings window, building it if the user has closed it.
///
/// Closing a window destroys it, so every entry point into Settings — the tray,
/// a second launch, the wizard's "Open Settings" — has to be able to make a new
/// one. Geometry is kept in step with the `settings` entry in tauri.conf.json.
pub fn open_settings_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(existing) = app.get_webview_window("settings") {
        show_and_focus_window(&existing);
        return Ok(existing);
    }

    tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("/settings".into()))
        .title("VoxCtrl Settings")
        .inner_size(720.0, 640.0)
        .min_inner_size(600.0, 450.0)
        .center()
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Could not open Settings: {e}"))
}

/// Show the first-run wizard, building its window if it is no longer there.
///
/// The wizard closes itself when the user finishes, and a closed Tauri window
/// cannot be shown again — so re-opening it has to construct a new one. That is
/// the right behaviour anyway: a re-run should start at step one with a fresh
/// webview, not resume on whatever screen the last run ended on.
///
/// Geometry is kept in step with the `wizard` entry in `tauri.conf.json`, which
/// is what the first launch of a fresh install uses.
pub fn open_wizard_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(WIZARD_WINDOW) {
        show_and_focus_window(&existing);
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        app,
        WIZARD_WINDOW,
        tauri::WebviewUrl::App("/wizard".into()),
    )
    .title("VoxCtrl — First-Run Setup")
    .inner_size(WIZARD_WIDTH, WIZARD_HEIGHT)
    .min_inner_size(WIZARD_MIN_WIDTH, WIZARD_MIN_HEIGHT)
    .center()
    .resizable(true)
    .decorations(true)
    .build()
    .map_err(|e| format!("Could not open the setup wizard: {e}"))?;

    Ok(())
}

/// Bring the setup window to the front, building it if it has been closed.
pub fn show_setup_window() {
    let Some(handle) = APP_HANDLE.get() else {
        return;
    };
    if let Some(w) = handle.get_webview_window(SETUP_WINDOW) {
        raise_window(&w, true);
        return;
    }

    let built = tauri::WebviewWindowBuilder::new(
        handle,
        SETUP_WINDOW,
        tauri::WebviewUrl::App("/udev-warning".into()),
    )
    .title("VoxCtrl Setup")
    .inner_size(580.0, 600.0)
    .min_inner_size(480.0, 420.0)
    .center()
    .always_on_top(true)
    .resizable(true)
    .build();
    if let Err(e) = built {
        tracing::error!("Could not open the setup window: {e}");
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

/// Helper to robustly show, unminimize, and focus a window
pub fn show_and_focus_window(window: &tauri::WebviewWindow) {
    raise_window(window, false);
}

/// Bring a window to the front and give it keyboard focus.
///
/// `set_focus` on its own is not enough on Linux. Every mainstream desktop
/// implements focus-stealing prevention: a process that does not already own
/// the focused window has its focus requests demoted to "urgent" — the taskbar
/// entry blinks and the window stays exactly where it was, behind whatever the
/// user is looking at. A tray click is precisely that case, since the tray is
/// not the app's own window.
///
/// Pinning the window above others is honoured where a bare focus request is
/// not, so the raise is done by pinning, focusing, then unpinning a moment
/// later — long enough for the compositor to restack, short enough that the
/// window does not linger above everything else.
///
/// `keep_on_top` leaves the pin in place, for windows that are meant to stay
/// above other applications.
pub fn raise_window(window: &tauri::WebviewWindow, keep_on_top: bool) {
    // Deliberately no `center()`: this is also the path for a window that is
    // already open, and moving a window the user has placed is not what
    // "bring it to the front" means.
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    if !keep_on_top {
        let w = window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let _ = w.set_always_on_top(false);
        });
    }
}
