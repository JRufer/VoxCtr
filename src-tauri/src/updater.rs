//! The app's side of updating: when to check, what to show, and how to hand
//! over to the new build.
//!
//! The decisions that can be got wrong without a running desktop — is this
//! version newer, which file replaces this installation, is the download the
//! one GitHub published — live in `voxctrl-update` and are tested there. What
//! is here is the part that needs Tauri: the launch check, the window, the
//! progress events, and the restart.

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager, State};

use crate::state::AppState;
use voxctrl_update::{CheckOutcome, PendingUpdate, Progress, UpdateInfo};

/// The version this build reports — the same string the About tab shows, and
/// the one every release tag is compared against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long after launch the check runs.
///
/// Startup is already busy: the model is loading, the portal handshake is in
/// flight, the tray is being built. A network round-trip thrown into that
/// competes with the things the user is waiting for, and an update dialog that
/// appears before the app has finished appearing reads as a fault.
const LAUNCH_CHECK_DELAY: Duration = Duration::from_secs(10);

/// What the frontend gets back from a check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheckPayload {
    /// The version running now.
    pub current_version: String,
    /// The update that was found, or `None` when this is the latest release.
    pub update: Option<UpdateInfo>,
    /// True when the user had previously chosen to skip this exact version, so
    /// a manual check can still show it while the launch check stays quiet.
    pub skipped: bool,
}

/// Run a check and remember the result. Returns what to show the user.
pub async fn check(state: &Arc<AppState>) -> Result<UpdateCheckPayload, String> {
    // Which release artifact this binary *is*, so the update keeps the variant
    // the user installed. On Windows the installer writes identical paths for
    // the CPU and GPU builds, so nothing on disk can answer this — only the
    // features the binary was compiled with.
    let gpu_build = voxctrl_inference::moonshine_gpu_backend().is_some()
        || voxctrl_inference::whisper_gpu_backend().is_some();

    let outcome = voxctrl_update::check(CURRENT_VERSION, gpu_build)
        .await
        .map_err(|e| e.to_string())?;

    let skipped_version = {
        let cfg = state.config.lock().await;
        cfg.data.updates.skipped_version.clone()
    };

    match outcome {
        CheckOutcome::UpToDate { current } => {
            *state.pending_update.lock().await = None;
            Ok(UpdateCheckPayload { current_version: current, update: None, skipped: false })
        }
        CheckOutcome::Available(pending) => {
            let info = pending.info.clone();
            let skipped = !voxctrl_update::should_prompt(&info, skipped_version.as_deref());
            *state.pending_update.lock().await = Some(*pending);
            Ok(UpdateCheckPayload {
                current_version: CURRENT_VERSION.to_string(),
                update: Some(info),
                skipped,
            })
        }
    }
}

/// Check once, shortly after launch, and raise the update window if there is
/// something to say.
///
/// Every failure here is silent by design. The user did not ask for this check;
/// a laptop that woke up on a train and could not reach GitHub has nothing to
/// apologise for, and a modal error about it would be worse than the missing
/// update. Failures go to the log, and Settings → General → "Check now" reports
/// them properly, because there someone is waiting for an answer.
pub fn spawn_launch_check(app: tauri::AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        {
            let cfg = state.config.lock().await;
            if !cfg.data.updates.auto_check {
                tracing::info!("Update check on launch is disabled in settings");
                return;
            }
        }

        tokio::time::sleep(LAUNCH_CHECK_DELAY).await;

        match check(&state).await {
            Ok(payload) => match payload.update {
                Some(info) if payload.skipped => {
                    tracing::info!("Update {} is available but was skipped by the user", info.version);
                }
                Some(info) => {
                    tracing::info!("Update available: {} → {}", info.current_version, info.version);
                    if let Err(e) = crate::window::open_update_window(&app) {
                        tracing::error!("Could not open the update window: {e}");
                    }
                }
                None => tracing::info!("VoxCtrl {CURRENT_VERSION} is up to date"),
            },
            Err(e) => tracing::warn!("Update check failed: {e}"),
        }
    });
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Check GitHub now, on the user's explicit say-so.
#[tauri::command]
pub async fn check_for_update(state: State<'_, Arc<AppState>>) -> Result<UpdateCheckPayload, String> {
    let state = state.inner().clone();
    check(&state).await
}

/// What the last check found, without asking GitHub again. This is what the
/// update window renders from, so opening it costs nothing.
#[tauri::command]
pub async fn get_pending_update(
    state: State<'_, Arc<AppState>>,
) -> Result<UpdateCheckPayload, String> {
    let pending = state.pending_update.lock().await;
    Ok(UpdateCheckPayload {
        current_version: CURRENT_VERSION.to_string(),
        update: pending.as_ref().map(|p| p.info.clone()),
        skipped: false,
    })
}

/// Stop offering this particular version. A newer one still gets raised.
#[tauri::command]
pub async fn skip_update_version(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    version: String,
) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().await;
        cfg.data.updates.skipped_version = Some(version.clone());
        cfg.save().map_err(|e| e.to_string())?;
        let _ = app.emit("config-changed", cfg.data.clone());
    }
    tracing::info!("Skipping update {version} at the user's request");
    Ok(())
}

/// Turn the launch check on or off from the update window, so the answer to
/// "stop telling me about this" does not require finding a settings tab.
#[tauri::command]
pub async fn set_update_auto_check(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> Result<(), String> {
    let mut cfg = state.config.lock().await;
    cfg.data.updates.auto_check = enabled;
    cfg.save().map_err(|e| e.to_string())?;
    let _ = app.emit("config-changed", cfg.data.clone());
    Ok(())
}

/// Download the pending update, install it, and restart into it.
///
/// Progress is emitted as `update-progress`; the app exits shortly after
/// `update-installed`. If anything fails the running installation is untouched
/// — nothing is replaced until a complete, checksum-verified file is sitting on
/// the same filesystem as the one it replaces.
#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let state = state.inner().clone();

    let pending: PendingUpdate = {
        let guard = state.pending_update.lock().await;
        guard
            .clone()
            .ok_or_else(|| "There is no update to install. Check for updates first.".to_string())?
    };

    if !pending.info.can_self_update {
        return Err(pending
            .info
            .unsupported_reason
            .clone()
            .unwrap_or_else(|| "This installation cannot update itself.".to_string()));
    }

    if !state.begin_update() {
        return Err("An update is already being installed.".to_string());
    }

    let emitter = app.clone();
    let result = voxctrl_update::install(&pending, move |p: Progress| {
        let _ = emitter.emit("update-progress", p);
    })
    .await;

    let launch_path = match result {
        Ok(path) => path,
        Err(e) => {
            state.end_update();
            let message = e.to_string();
            tracing::error!("Update failed: {message}");
            let _ = app.emit("update-failed", message.clone());
            return Err(message);
        }
    };

    tracing::info!("Update installed; restarting into {}", launch_path.display());
    let _ = app.emit("update-installed", pending.info.version.clone());

    // A version the user has just installed is not one to keep skipping.
    {
        let mut cfg = state.config.lock().await;
        if cfg.data.updates.skipped_version.is_some() {
            cfg.data.updates.skipped_version = None;
            let _ = cfg.save();
        }
    }

    voxctrl_update::apply::spawn_relaunch(&launch_path).map_err(|e| {
        state.end_update();
        e.to_string()
    })?;

    // Long enough for the window to paint "restarting…", short enough that the
    // relaunch helper is not left polling for a process that will not die.
    // The helper waits for this process to exit before starting the new one,
    // so the single-instance guard cannot bounce the new copy off the old.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        app.exit(0);
    });

    Ok(())
}

/// Close the update window without doing anything. The same update is raised
/// again on the next launch, which is what "Not now" should mean.
#[tauri::command]
pub async fn dismiss_update(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(crate::window::UPDATE_WINDOW) {
        let _ = window.close();
    }
    Ok(())
}

/// Open the update window from Settings, once a check has found something.
#[tauri::command]
pub async fn open_update_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::window::open_update_window(&app)
}
