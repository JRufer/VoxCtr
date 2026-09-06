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

/// Label of the "a new version is available" window.
pub const UPDATE_WINDOW: &str = "update";

/// The wizard's screens are laid out on a wide stage — two engine cards side by
/// side, eight overlay thumbnails in a row, five voice cards in a row. The
/// widest breakpoint in the step stylesheets is 1200px, and the tallest step
/// (the overlay grid over the position preview) needs a shade under 1000px of
/// height before its footer is pushed off the bottom. So the floor is the size
/// at which every step is known to render at its intended breakpoint, and the
/// window opens a little above it — enough slack for the layout to breathe
/// without the dead space a much bigger window leaves behind. These must stay in step with the `wizard`
/// entry in tauri.conf.json, which is what a fresh install's first launch uses.
pub const WIZARD_WIDTH: f64 = 1500.0;
pub const WIZARD_HEIGHT: f64 = 1040.0;
pub const WIZARD_MIN_WIDTH: f64 = 1374.0;
pub const WIZARD_MIN_HEIGHT: f64 = 1000.0;

/// Default and minimum geometry for the Settings window. Its sidebar plus the
/// widest tab body need the width, and the longest tab needs the height before
/// it starts scrolling on first open.
pub const SETTINGS_WIDTH: f64 = 880.0;
pub const SETTINGS_HEIGHT: f64 = 1000.0;
pub const SETTINGS_MIN_WIDTH: f64 = 720.0;
pub const SETTINGS_MIN_HEIGHT: f64 = 640.0;

/// Fraction of the display the window may occupy, leaving room for the title
/// bar and a desktop panel. Height is the tighter of the two: panels are
/// usually horizontal, and the title bar eats from the same axis.
const WIZARD_FIT_W: f64 = 0.94;
const WIZARD_FIT_H: f64 = 0.90;

/// The largest window in the wizard's design proportions that fits the space
/// available, capped at the design size and floored at the size below which the
/// layout stops fitting.
///
/// A fixed design size is only safe at 100% scaling: the same window on a 1080p
/// display at 125% is scaled up in physical pixels, wider and taller than the
/// screen, so the footer with the Continue button ends up past the bottom edge.
/// Sizes are logical pixels, which is what the compositor scales.
///
/// The floor wins over fitting on purpose. A user can move or scroll a window
/// that is slightly too big for their desktop; they cannot unwrap a layout that
/// has dropped to a narrower breakpoint, which is what a smaller window gives
/// them.
pub fn wizard_size_for(available_width: f64, available_height: f64) -> (f64, f64) {
    let aspect = WIZARD_WIDTH / WIZARD_HEIGHT;

    let w = available_width.min(WIZARD_WIDTH);
    let h = available_height.min(WIZARD_HEIGHT);

    // Shrink whichever axis is over-long, so the window keeps its proportions
    // rather than letterboxing the layout it was designed around.
    let (w, h) = if w / h > aspect { (h * aspect, h) } else { (w, w / aspect) };

    (w.max(WIZARD_MIN_WIDTH), h.max(WIZARD_MIN_HEIGHT))
}

/// Resize a window to fit the display it is on, and re-centre it.
fn fit_to_display(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let logical = monitor.size().to_logical::<f64>(monitor.scale_factor());
    let (w, h) = wizard_size_for(logical.width * WIZARD_FIT_W, logical.height * WIZARD_FIT_H);
    let _ = window.set_size(tauri::LogicalSize::new(w, h));
    let _ = window.center();
}

/// Minimum gap between "finish the setup" notifications, so holding a
/// push-to-talk key does not produce a wall of toasts.
pub const SETUP_NOTICE_INTERVAL: Duration = Duration::from_secs(60);

/// How often the setup watcher re-checks the listener. Short enough that a
/// change made elsewhere — the portal coming up, a shortcut reassigned in the
/// desktop's settings — visibly flips the app to working within a couple of
/// seconds.
#[cfg(target_os = "linux")]
pub const SETUP_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Re-alert cadence while nothing can deliver shortcuts at all. In that state
/// no shortcut can reach the app, so this is the only way the user hears about
/// it while they are pressing keys and getting nothing.
#[cfg(target_os = "linux")]
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
        .inner_size(SETTINGS_WIDTH, SETTINGS_HEIGHT)
        .min_inner_size(SETTINGS_MIN_WIDTH, SETTINGS_MIN_HEIGHT)
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
        fit_to_display(&existing);
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

    // Only measurable once the window exists and knows which display it is on.
    if let Some(window) = app.get_webview_window(WIZARD_WINDOW) {
        fit_to_display(&window);
    }

    Ok(())
}

/// Show the update offer, building its window if it is not already there.
///
/// This is the one window that appears without the user having asked for
/// anything, so it is a plain window rather than an always-on-top one: an
/// update is worth mentioning, not worth interrupting whatever is on screen.
/// VoxCtrl is a tray app that usually has nothing open at all, which is why the
/// news cannot simply go into Settings.
pub fn open_update_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(UPDATE_WINDOW) {
        show_and_focus_window(&existing);
        return Ok(());
    }

    let window = tauri::WebviewWindowBuilder::new(
        app,
        UPDATE_WINDOW,
        tauri::WebviewUrl::App("/update".into()),
    )
    .title("VoxCtrl — Update Available")
    .inner_size(560.0, 620.0)
    .min_inner_size(460.0, 420.0)
    .center()
    .resizable(true)
    .decorations(true)
    .build()
    .map_err(|e| format!("Could not open the update window: {e}"))?;

    show_and_focus_window(&window);
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


#[cfg(test)]
mod tests {
    use super::*;

    /// 1080p at 100% scaling is the common case: wide enough for a comfortable
    /// stage, and short enough that the height lands exactly on the floor.
    #[test]
    fn a_1080p_display_gets_a_usable_stage() {
        let (w, h) = wizard_size_for(1920.0 * WIZARD_FIT_W, 1080.0 * WIZARD_FIT_H);
        assert!((WIZARD_MIN_WIDTH..=WIZARD_WIDTH).contains(&w), "width {w}");
        assert_eq!(h, WIZARD_MIN_HEIGHT);
        assert!(h <= 1080.0, "a {h}px window does not fit a 1080p display");
    }

    /// 1080p at 125% — the desktop is only 1536x864 logical pixels, which is
    /// under the layout's floor. The floor wins: a window the user has to move
    /// beats a layout that has wrapped.
    #[test]
    fn a_scaled_1080p_display_gets_at_least_the_layout_minimum() {
        let (avail_w, avail_h) = (1536.0 * WIZARD_FIT_W, 864.0 * WIZARD_FIT_H);
        let (w, h) = wizard_size_for(avail_w, avail_h);
        assert!(w >= WIZARD_MIN_WIDTH && h >= WIZARD_MIN_HEIGHT);
    }

    #[test]
    fn the_window_keeps_its_proportions_when_it_shrinks() {
        // Wide enough to be capped by the design width, tall enough that the
        // height floor does not kick in.
        let (w, h) = wizard_size_for(2000.0, 3000.0);
        let aspect = WIZARD_WIDTH / WIZARD_HEIGHT;
        assert!(
            ((w / h) - aspect).abs() < 0.01,
            "expected {aspect}:1, got {w}x{h}"
        );
    }

    /// A display too small for the layout gets the minimum rather than a
    /// window whose contents wrap: the user can move a window, but cannot
    /// unwrap a layout.
    #[test]
    fn a_small_display_never_goes_below_the_layout_minimum() {
        let (w, h) = wizard_size_for(900.0, 500.0);
        assert_eq!((w, h), (WIZARD_MIN_WIDTH, WIZARD_MIN_HEIGHT));
    }

    /// The breakpoint the CSS actually cares about: every wizard step is
    /// designed for a stage wider than its widest `max-width` media query, and
    /// tall enough not to push the footer past the bottom edge.
    #[test]
    fn every_display_clears_the_widest_css_breakpoint() {
        for (avail_w, avail_h) in [
            (1920.0, 1080.0),
            (1536.0, 864.0),
            (1280.0, 720.0),
            (3840.0, 2160.0),
            (900.0, 500.0),
        ] {
            let (w, h) = wizard_size_for(avail_w * WIZARD_FIT_W, avail_h * WIZARD_FIT_H);
            assert!(w >= 1374.0, "{avail_w}x{avail_h} gave a {w}px-wide window");
            assert!(h >= 1000.0, "{avail_w}x{avail_h} gave a {h}px-tall window");
        }
    }

    #[test]
    fn a_large_display_is_capped_at_the_design_size() {
        let (w, h) = wizard_size_for(3840.0, 2160.0);
        assert_eq!((w, h), (WIZARD_WIDTH, WIZARD_HEIGHT));
    }
}
