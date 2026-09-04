use std::sync::Arc;
use tauri::{Emitter, Manager};
use voxctrl_config::AppConfig;
use voxctrl_mcp::McpCallbacks;

use crate::state::AppState;

impl McpCallbacks for AppState {
    fn transcribe_voice(
        &self,
        timeout_secs: f64,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
        async move {
            use std::sync::atomic::Ordering;
            use tokio::time::{sleep, Duration};

            // Snapshot the current version counter BEFORE starting. The delivery
            // thread increments it each time a new result is written to last_text.
            // Polling for version > baseline_version guarantees we only accept a
            // result from THIS recording session, never a stale prior-session value.
            let baseline_version = self.last_text_version.load(Ordering::SeqCst);

            self.set_mcp_recording(true);

            // Start recording.
            self.set_recording(true);

            // Spawn a timer to automatically stop recording after timeout_secs.
            let recording = self.recording.clone();
            let audio_tx = self.audio_tx.clone();
            tokio::spawn(async move {
                sleep(Duration::from_secs_f64(timeout_secs)).await;
                recording.store(false, Ordering::SeqCst);
                let _ = audio_tx.send(Vec::new());
            });

            // Wait until recording stops (timer or manual stop).
            while self.is_recording() {
                sleep(Duration::from_millis(50)).await;
            }

            self.set_mcp_recording(false);

            // Wait for inference + delivery to produce a new last_text.
            // last_text is now written BEFORE delivery targets run, so this poll
            // completes as soon as inference finishes rather than waiting for slow
            // delivery targets.  3 s budget is kept as a safety net.
            let poll_limit = 60; // 60 × 50 ms = 3.0 s
            let mut text = String::new();
            for _ in 0..poll_limit {
                sleep(Duration::from_millis(50)).await;
                if self.last_text_version.load(Ordering::SeqCst) > baseline_version {
                    text = self.last_text.lock().await.clone();
                    break;
                }
            }

            if text.is_empty() {
                Ok("(no speech detected)".to_string())
            } else {
                Ok(text)
            }
        }
    }

    fn speak_text(
        &self,
        text: String,
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        async move {
            let handle = self.tts_handle.lock().await;
            if let Some(ref tts) = *handle {
                tts.speak(text);
            }
            Ok(())
        }
    }

    fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send {
        async move { (self.is_recording(), self.is_speaking()) }
    }
}

pub fn start_mcp_server(callbacks: Arc<AppState>) {
    tokio::spawn(async move {
        tracing::info!("Starting MCP server...");
        if let Err(e) = voxctrl_mcp::run_server(callbacks).await {
            tracing::error!("MCP server error: {:?}", e);
        }
    });
}

#[cfg(target_os = "linux")]
pub fn start_dbus_service(app_state: Arc<AppState>) {
    let dbus_state = Arc::new(tokio::sync::Mutex::new(voxctrl_dbus::AppState::default()));
    let (start_tx, mut start_rx) = tokio::sync::mpsc::channel::<String>(4);
    let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(4);
    let app_state_dbus = app_state.clone();
    let dbus_state_clone = dbus_state.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                v = start_rx.recv() => {
                    let Some(binding_id) = v else { break };
                    // A desktop shortcut names the binding it stands for, so
                    // the transcription reaches the same targets it would have
                    // if VoxCtrl had seen the keys itself. Without this every
                    // shortcut would dictate into the default target no matter
                    // which one the user pressed.
                    apply_dbus_binding(&app_state_dbus, &binding_id).await;
                    app_state_dbus.set_recording(true);
                    let mut st = dbus_state_clone.lock().await;
                    st.status = voxctrl_dbus::DictationStatus::Recording;
                }
                v = stop_rx.recv() => {
                    if v.is_some() {
                        app_state_dbus.set_recording(false);
                        let mut st = dbus_state_clone.lock().await;
                        st.status = voxctrl_dbus::DictationStatus::Idle;
                    } else {
                        break;
                    }
                }
            }
        }
    });
    tokio::spawn(async move {
        if let Err(e) = voxctrl_dbus::start_service(dbus_state, start_tx, stop_tx).await {
            tracing::error!("DBus service error: {e}");
        }
    });
}

/// Point the pipeline at the binding a D-Bus caller named, so its targets and
/// its label are the ones used for this dictation.
///
/// An unknown or empty id leaves whatever the app already had, which is the
/// "Focused Window" default on a fresh start.
async fn apply_dbus_binding(state: &Arc<AppState>, binding_id: &str) {
    if binding_id.is_empty() {
        return;
    }
    let bindings =
        voxctrl_routing::load_bindings(&voxctrl_routing::config_dir()).unwrap_or_default();
    let Some(binding) = bindings.into_iter().find(|b| b.id == binding_id) else {
        tracing::warn!("D-Bus asked for unknown binding '{binding_id}'; using the current target");
        return;
    };

    *state.active_target.lock().await = binding.target_id.clone();
    *state.active_binding_id.lock().await = binding.id.clone();
    *state.active_binding_label.lock().await = binding.label.clone();
}

pub fn setup_tts_and_fifos(app_handle: &tauri::AppHandle, state: Arc<AppState>) {
    let cfg_opt = if let Ok(config_guard) = state.config.try_lock() {
        Some(config_guard.data.clone())
    } else {
        None
    };

    if let Some(cfg) = cfg_opt {
        if cfg.tts.enabled {
            if let Ok(mut handle) = state.tts_handle.try_lock() {
                if let Some(ref tts) = *handle {
                    tts.shutdown();
                }
                let app_handle_clone = app_handle.clone();
                let app_handle_clone_end = app_handle.clone();
                let app_handle_clone_err = app_handle.clone();
                let state_clone = state.clone();
                let state_clone_end = state.clone();
                let new_tts = voxctrl_tts::TtsEngineWorker::start(
                    cfg.tts.clone(),
                    cfg.features.custom_vocabulary.clone(),
                    Some(std::sync::Arc::new(move || {
                        state_clone.set_speaking(true);
                        let _ = app_handle_clone.emit("tts-playback-start", ());
                    })),
                    Some(std::sync::Arc::new(move || {
                        state_clone_end.set_speaking(false);
                        let _ = app_handle_clone_end.emit("tts-playback-end", ());
                    })),
                    Some(std::sync::Arc::new(move |msg: String| {
                        let _ = app_handle_clone_err.emit("tts-error", msg);
                    })),
                );
                *handle = Some(new_tts.clone());
                let state_for_fifos = state.clone();
                let tts_for_fifos = new_tts.clone();
                tauri::async_runtime::spawn(async move {
                    state_for_fifos.spawn_fifo_responders(tts_for_fifos).await;
                });
            }
        }
    }
}

pub fn register_speak_target(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<Arc<AppState>>().inner().clone();
    voxctrl_routing::targets::set_speak_callback(std::sync::Arc::new(move |text| {
        let state = state.clone();
        let text_str = text.to_string();
        tauri::async_runtime::spawn(async move {
            let handle = state.tts_handle.lock().await;
            if let Some(ref tts) = *handle {
                tts.speak(text_str);
            } else {
                tracing::warn!("Speak target triggered but TTS is disabled or not initialized");
            }
        });
    }));
}

pub fn register_command_trigger_target(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<Arc<AppState>>().inner().clone();
    let app_handle_clone = app_handle.clone();
    voxctrl_routing::targets::set_command_trigger_callback(std::sync::Arc::new(
        move |command_name, text_summary| {
            let (show_overlay, duration_secs) = if let Ok(cfg) = state.config.try_lock() {
                (
                    cfg.data.ui.show_command_overlay,
                    cfg.data.ui.command_overlay_duration_secs,
                )
            } else {
                (true, 3)
            };

            if show_overlay {
                let cmd_name = command_name.to_string();
                let summary = text_summary.to_string();
                let _ = app_handle_clone.emit(
                    "command-executed",
                    serde_json::json!({
                        "command": cmd_name,
                        "summary": summary,
                        "duration_secs": duration_secs,
                    }),
                );

                let overlay_msg = serde_json::json!({
                    "type": "command",
                    "command": cmd_name,
                    "summary": summary,
                    "duration_secs": duration_secs,
                });
                if let Ok(json_str) = serde_json::to_string(&overlay_msg) {
                    let _ = state.overlay_tx.send(json_str);
                }
            }
        },
    ));
}

pub fn auto_download_speech_model_if_needed(
    app: &tauri::App,
    cfg_data: &Arc<AppConfig>,
) {
    // A machine that has never run the wizard gets the wizard and nothing else.
    // Every decision this function would make for the user — which model to
    // fetch, whether to open Settings — is a step the wizard asks about, so
    // making them here first would download the wrong model and bury the
    // wizard behind a window the user did not ask for.
    if !cfg_data.ui.setup_completed {
        if app.get_webview_window(crate::window::WIZARD_WINDOW).is_some() {
            if let Err(e) = crate::window::open_wizard_window(&app.handle().clone()) {
                tracing::error!("Could not open the setup wizard: {e}");
            }
            return;
        }
        // No wizard window in this build: fall through to the old behaviour
        // rather than leaving a new install with no visible setup at all.
    }

    let mut show_settings = cfg_data.ui.auto_show_settings;
    // Only the whisper-cpp path needs a GGUF model on disk. A Moonshine
    // selection uses whisper-cpp (and thus its model) unless the
    // Moonshine backend is actually compiled into this build.
    let uses_whisper_model = cfg_data.engine.backend != voxctrl_config::BackendChoice::Moonshine
        || !voxctrl_inference::MOONSHINE_COMPILED;
    if uses_whisper_model {
        let model_size = cfg_data.engine.whisper_cpp.model_size.clone();
        let model_dir = cfg_data.engine.whisper_cpp.model_dir.clone();
        if !voxctrl_inference::whisper_cpp::is_model_downloaded(&model_size, &model_dir) {
            if voxctrl_inference::whisper_cpp::is_small_auto_downloadable(&model_size) {
                // The inference worker independently retries loading
                // the model on every dictation request (see
                // voxctrl-inference::run_worker), so transcription
                // starts working the moment this finishes — no app
                // restart needed.
                tauri::async_runtime::spawn(async move {
                    voxctrl_inject::show_notification(
                        "VoxCtrl",
                        &format!("Downloading the default speech model ({model_size})..."),
                    );
                    match voxctrl_inference::whisper_cpp::download_model(&model_size, &model_dir)
                        .await
                    {
                        Ok(()) => {
                            voxctrl_inject::show_notification(
                                "VoxCtrl",
                                "Speech model ready — dictation is now available.",
                            );
                        }
                        Err(e) => {
                            tracing::error!("Auto-download of default speech model failed: {e:#}");
                            voxctrl_inject::show_notification(
                                "VoxCtrl",
                                &format!(
                                    "Could not download the default speech model: {e:#}. Open Settings → Engine to retry."
                                ),
                            );
                        }
                    }
                });
            } else {
                show_settings = true;
            }
        }
    }

    if show_settings {
        if let Err(e) = crate::window::open_settings_window(&app.handle().clone()) {
            tracing::error!("Could not open Settings at startup: {e}");
        }
    }
}
