use std::sync::Arc;

use tauri::{Emitter, Manager, State};
use tracing::info;
use voxctrl_config::AppConfig;
use voxctrl_routing::{HotkeyBinding, OutputTarget};

use crate::state::AppState;

// ── Status ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusPayload, String> {
    let active_target_id = state.active_target.lock().await.clone();
    let target_label = {
        let targets_guard = state.targets.lock().await;
        targets_guard.iter()
            .find(|t| t.id == active_target_id)
            .map(|t| t.label.clone())
            .unwrap_or_else(|| {
                if active_target_id == "default" {
                    "Focused Window".to_string()
                } else {
                    active_target_id.clone()
                }
            })
    };

    Ok(StatusPayload {
        recording: state.is_recording(),
        processing: state.is_processing(),
        speaking: state.is_speaking(),
        mcp_recording: state.is_mcp_recording(),
        audio_ready: state.is_audio_ready(),
        word_count: state.total_words(),
        active_target_id,
        active_target_label: target_label,
    })
}

#[derive(serde::Serialize)]
pub struct StatusPayload {
    pub recording: bool,
    pub processing: bool,
    pub speaking: bool,
    pub mcp_recording: bool,
    pub audio_ready: bool,
    pub word_count: u32,
    pub active_target_id: String,
    pub active_target_label: String,
}

// ── Recording control ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_recording(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.active_binding_id.lock().await = String::new();
    state.begin_recording().await;
    info!("Recording started via command");
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_recording(false);
    info!("Recording stopped via command");
    Ok(())
}

#[tauri::command]
pub async fn toggle_recording(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    let was = state.is_recording();
    if !was {
        *state.active_binding_id.lock().await = String::new();
    }
    state.set_recording(!was);
    Ok(!was)
}

// ── Config ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, String> {
    let guard = state.config.lock().await;
    Ok(guard.data.clone())
}

#[tauri::command]
pub async fn save_config(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    new_config: AppConfig,
) -> Result<(), String> {
    // Update live dynamic stream state, input device index, and gain in AppState
    state.set_dynamic_stream(new_config.audio.dynamic_stream);
    state.set_input_device_index(new_config.audio.input_device_index);
    state.set_gain(new_config.audio.gain);
    state.set_noise_suppression(new_config.audio.noise_suppression);
    state.set_overlay_enabled(new_config.ui.show_overlay);

    // Dynamic TTS engine lifecycle management
    {
        let mut handle = state.tts_handle.lock().await;
        let mut need_restart = true;

        if let Some(ref tts) = *handle {
            tts.update_config(new_config.tts.clone());
            need_restart = false;
        }

        if need_restart {
            if new_config.tts.enabled {
                let app_handle = app.clone();
                let app_handle_end = app.clone();
                let app_handle_err = app.clone();
                let state_clone = state.inner().clone();
                let state_clone_end = state.inner().clone();
                let new_tts = voxctrl_tts::TtsEngineWorker::start(
                    new_config.tts.clone(),
                    new_config.features.custom_vocabulary.clone(),
                    Some(std::sync::Arc::new(move || {
                        state_clone.set_speaking(true);
                        let _ = app_handle.emit("tts-playback-start", ());
                    })),
                    Some(std::sync::Arc::new(move || {
                        state_clone_end.set_speaking(false);
                        let _ = app_handle_end.emit("tts-playback-end", ());
                    })),
                    Some(std::sync::Arc::new(move |msg: String| {
                        let _ = app_handle_err.emit("tts-error", msg);
                    })),
                );
                *handle = Some(new_tts.clone());
                state.spawn_fifo_responders(new_tts).await;
            } else {
                *handle = None;
            }
        }
    }

    let mut guard = state.config.lock().await;
    let stop_key_changed = guard.data.tts.stop_key != new_config.tts.stop_key;
    guard.data = new_config.clone();
    guard.save().map_err(|e| e.to_string())?;
    info!("Config saved");

    let (overlay_position, overlay_monitor) = (
        guard.data.ui.overlay_position.clone(),
        guard.data.ui.overlay_monitor.clone(),
    );

    // Hot-reload the stop key binding in the listener if tts.stop_key changed.
    // The config lock has to go first: assembling the set reads the stop key
    // back out of it.
    drop(guard);
    if stop_key_changed {
        // A set that could not be read is not worth reloading: the listener
        // keeps the shortcuts it already has instead of losing all of them.
        if let Some(bindings) = crate::stop_key::listener_bindings_from_disk(&state).await {
            let reloader_guard = state.hotkey_reloader.lock().await;
            if let Some(reloader) = &*reloader_guard {
                let _ = reloader.send(bindings);
            }
        }
    }

    // Emit config-changed event to all windows to enable instant reactivity
    let _ = app.emit("config-changed", new_config);

    let pos_msg = serde_json::json!({
        "type": "position",
        "position": overlay_position,
        "monitor": overlay_monitor,
    });
    if let Ok(json_str) = serde_json::to_string(&pos_msg) {
        let _ = state.overlay_tx.send(json_str);
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_tts(
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    info!("TTS stop requested via command");
    let handle = state.tts_handle.lock().await;
    if let Some(ref tts) = *handle {
        tts.stop();
    }
    Ok(())
}

// ── Build info ────────────────────────────────────────────────────────────────

/// Returns true when this binary was compiled with the `cuda` cargo feature.
/// The frontend uses this to show or hide the CUDA device option.
#[tauri::command]
pub fn cuda_enabled() -> bool {
    cfg!(feature = "cuda")
}

/// What this build can actually offload to a GPU, per engine.
///
/// Both fields are decided at compile time and neither can be inferred from the
/// other: the Vulkan build gives whisper.cpp the GPU and leaves Moonshine on the
/// CPU, because ONNX Runtime has no Vulkan execution provider. The Engine tab
/// asks for this so it can offer the device options the build can honour, and
/// name the engine that cannot use the GPU at all.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcceleratorSupport {
    /// `"cuda"`, `"vulkan"`, or `None` on a CPU-only build.
    pub whisper_gpu: Option<String>,
    /// `"cuda"`, `"coreml"`, or `None` — `None` in every build shipped today.
    pub moonshine_gpu: Option<String>,
}

#[tauri::command]
pub fn accelerator_support() -> AcceleratorSupport {
    AcceleratorSupport {
        whisper_gpu: voxctrl_inference::whisper_gpu_backend().map(str::to_string),
        moonshine_gpu: voxctrl_inference::moonshine_gpu_backend().map(str::to_string),
    }
}

// ── Routing ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_targets(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<OutputTarget>, String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::load_targets(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_targets(
    state: State<'_, Arc<AppState>>,
    targets: Vec<OutputTarget>,
) -> Result<(), String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::save_targets(&targets, &dir).map_err(|e| e.to_string())?;
    
    // Update the in-memory targets cache
    *state.targets.lock().await = targets.clone();

    // Hot-reload the router
    state.router.reload(targets).await;
    info!("Targets saved and router reloaded");

    // Dynamically spawn new FIFO response pipe listeners if TTS is active
    let tts_handle_opt = {
        let guard = state.tts_handle.lock().await;
        guard.clone()
    };
    if let Some(tts) = tts_handle_opt {
        state.spawn_fifo_responders(tts).await;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_bindings(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<HotkeyBinding>, String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::load_bindings(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_bindings(
    state: State<'_, Arc<AppState>>,
    bindings: Vec<HotkeyBinding>,
) -> Result<(), String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::save_bindings(&bindings, &dir).map_err(|e| e.to_string())?;
    info!("Bindings saved");
    
    // Hot reload the bindings in the active listener threads, re-injecting the
    // stop key when it is currently held — `stop_key`'s arbiter decides that,
    // and a save must not take a grab it released or drop one it is using.
    let all_bindings = crate::stop_key::listener_bindings(&state, bindings.clone()).await;
    let reloader_guard = state.hotkey_reloader.lock().await;
    if let Some(reloader) = &*reloader_guard {
        if let Err(e) = reloader.send(all_bindings) {
            tracing::warn!("Failed to hot-reload bindings: {e}");
        } else {
            info!("Hot-reload signal sent to listener");
        }
    }
    
    // Where the desktop owns the key grab, the saved bindings have to be pushed
    // to it — nothing else will notice they changed. Narrowly gated: the Mint
    // route is only right when it is already in use, or when nothing else
    // worked at all. Registering it alongside a backend that watches the keys
    // itself would fire every shortcut twice, and `Starting` has not finished
    // deciding yet.
    let backend = state.hotkey_health.backend();
    if backend == voxctrl_hotkeys::Backend::MintDbus
        || (backend == voxctrl_hotkeys::Backend::None
            && crate::mint_shortcuts::is_mint_desktop())
    {
        match crate::mint_shortcuts::sync_mint_shortcuts(&bindings) {
            Ok(registered) => {
                state
                    .hotkey_health
                    .set_backend(voxctrl_hotkeys::Backend::MintDbus);
                info!(
                    "Mirrored {} binding(s) into Linux Mint's own shortcut settings",
                    registered.len()
                );
            }
            Err(e) => tracing::warn!("Failed to sync Linux Mint native shortcut settings: {e}"),
        }
    }

    Ok(())
}

/// Forget a Chat target's conversation so the next dictation starts fresh.
#[tauri::command]
pub async fn reset_chat_conversation(
    _state: State<'_, Arc<AppState>>,
    target_id: String,
) -> Result<usize, String> {
    let dropped = voxctrl_routing::reset_chat_history(&target_id).await;
    info!("Chat conversation for '{target_id}' reset ({dropped} messages dropped)");
    Ok(dropped)
}

/// Probe a Chat target's endpoint and list the models it serves.
///
/// Takes an unsaved target so the settings UI can test edits before persisting
/// them. Routed through Rust rather than `fetch` in the webview because the
/// endpoint is a third-party server that need not send CORS headers.
#[tauri::command]
pub async fn test_chat_target(target: OutputTarget) -> Result<OpenAiTestResult, String> {
    use voxctrl_config::OpenAiConfig;

    let endpoint = target.chat_url.unwrap_or_default();
    if endpoint.trim().is_empty() {
        return Err("No server URL configured.".into());
    }
    let client = voxctrl_llm::OpenAiClient::new(OpenAiConfig {
        enabled: true,
        endpoint: endpoint.clone(),
        api_key: target.chat_api_key,
        model: String::new(),
        timeout_secs: target.chat_timeout_secs.clamp(1, 30),
        ..Default::default()
    });

    match client.list_models().await {
        Ok(models) if models.is_empty() => Ok(OpenAiTestResult {
            success: true,
            message: format!("Connected to {endpoint}, but it reported no models."),
            models,
        }),
        Ok(models) => Ok(OpenAiTestResult {
            success: true,
            message: format!("Connected to {endpoint} — {} model(s) available.", models.len()),
            models,
        }),
        Err(e) => Ok(OpenAiTestResult {
            success: false,
            message: format!("Could not reach {endpoint}: {e}"),
            models: Vec::new(),
        }),
    }
}

#[tauri::command]
pub async fn speak_text(
    state: State<'_, Arc<AppState>>,
    text: String,
    voice: Option<String>,
) -> Result<(), String> {
    info!("TTS speak_text via command: {text}");
    let handle = state.tts_handle.lock().await;
    if let Some(ref tts) = *handle {
        tts.speak_utterance(voxctrl_tts::Utterance {
            text,
            voice,
            source_label: None,
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn check_voice_downloaded(voice_name: String, voice_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_voice_downloaded(&voice_name, &voice_dir))
}

#[tauri::command]
pub async fn download_voice(voice_name: String, voice_dir: String) -> Result<(), String> {
    voxctrl_tts::download_voice(&voice_name, &voice_dir)
        .await
        .map_err(|e| e.to_string())
}

/// Render a file target's timestamp format so the Settings UI can preview it
/// and report a bad pattern before the target is saved.
///
/// chrono is the authority on what a `strftime` pattern means, so the preview
/// comes from the same code the file target writes with rather than from a
/// second implementation in TypeScript.
#[tauri::command]
pub async fn preview_timestamp_format(format: String) -> Result<String, String> {
    voxctrl_routing::render_timestamp(&format, chrono::Utc::now())
}

/// The HuggingFace token exported into the environment, if any.
///
/// The UI shows it in place of the configured one and stops editing, because
/// `HF_TOKEN` wins at download time — a value typed over it would be saved and
/// then ignored, which is worse than not offering the field.
#[tauri::command]
pub async fn hf_token_env() -> Option<String> {
    voxctrl_tts::hf_token_from_env()
}

#[tauri::command]
pub async fn check_breeze_tts_2_ready(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_breeze_tts_2_ready(&model_dir))
}

#[tauri::command]
pub async fn download_breeze_tts_2(model_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_breeze_tts_2_assets(&model_dir, hf_token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_pocket_tts_ready(voice: String, voice_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_pocket_tts_ready(&voice, &voice_dir))
}

#[tauri::command]
pub async fn download_pocket_tts(voice: String, voice_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_pocket_tts_assets(&voice, &voice_dir, hf_token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_pocket_tts_voices(voice_dir: String) -> Result<Vec<voxctrl_tts::PocketTtsVoiceOption>, String> {
    Ok(voxctrl_tts::pocket_tts_voice_catalogue(&voice_dir))
}

/// Whether the Inflect-Micro-v2 ONNX engine was compiled into this build. The UI
/// uses this to warn that selecting the engine in a build without it will fail,
/// rather than letting the failure surface only on the first utterance.
#[tauri::command]
pub fn inflect_micro_available() -> bool {
    voxctrl_tts::INFLECT_MICRO_COMPILED
}

#[tauri::command]
pub async fn check_inflect_micro_downloaded(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_inflect_micro_downloaded(&model_dir))
}

#[tauri::command]
pub async fn download_inflect_micro(model_dir: String) -> Result<(), String> {
    voxctrl_tts::download_inflect_micro_assets(&model_dir)
        .await
        .map_err(|e| e.to_string())
}

/// Report the tensor names a downloaded Inflect-Micro-v2 export actually
/// declares. This is the diagnostic path for a graph whose naming doesn't match
/// what `inflect::model` binds against: it loads the graphs without building a
/// synthesis plan, so it still returns a useful answer when loading for playback
/// would fail.
#[tauri::command]
pub async fn inflect_micro_inspect(model_dir: String) -> Result<serde_json::Value, String> {
    #[cfg(feature = "inflect-micro")]
    {
        let dir = voxctrl_tts::inflect::resolve_model_dir(&model_dir);
        let signature = tokio::task::spawn_blocking(move || voxctrl_tts::inflect::model::inspect(&dir))
            .await
            .map_err(|e| format!("inspect task join: {e}"))?
            .map_err(|e| format!("{e:#}"))?;
        serde_json::to_value(signature).map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "inflect-micro"))]
    {
        let _ = model_dir;
        Err("This build was compiled without the `inflect-micro` feature.".to_string())
    }
}

#[tauri::command]
pub async fn check_model_downloaded(model_size: String, model_dir: Option<String>) -> Result<bool, String> {
    let dir = model_dir.unwrap_or_default();
    Ok(voxctrl_inference::whisper_cpp::is_model_downloaded(&model_size, &dir))
}

#[tauri::command]
pub async fn download_model(model_size: String, model_dir: String) -> Result<(), String> {
    voxctrl_inference::whisper_cpp::download_model(&model_size, &model_dir)
        .await
        .map_err(|e| e.to_string())
}

/// Whether the Moonshine ONNX backend was compiled into this build. The UI uses
/// this to decide whether selecting Moonshine actually runs Moonshine (vs.
/// transparently falling back to whisper-cpp).
#[tauri::command]
pub fn moonshine_available() -> bool {
    voxctrl_inference::MOONSHINE_COMPILED
}

#[tauri::command]
pub async fn check_moonshine_downloaded(model_size: String) -> Result<bool, String> {
    #[cfg(feature = "moonshine")]
    {
        Ok(voxctrl_inference::moonshine::is_model_downloaded(&model_size, ""))
    }
    #[cfg(not(feature = "moonshine"))]
    {
        let _ = model_size;
        Ok(false)
    }
}

#[tauri::command]
pub async fn download_moonshine_model(model_size: String) -> Result<(), String> {
    #[cfg(feature = "moonshine")]
    {
        voxctrl_inference::moonshine::download_model(&model_size, "")
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "moonshine"))]
    {
        let _ = model_size;
        Err("This build was compiled without the Moonshine backend. Rebuild with `--features moonshine` to use it.".into())
    }
}

#[tauri::command]
pub async fn check_directory_exists(path: String) -> Result<bool, String> {
    if path.is_empty() {
        return Ok(true);
    }
    Ok(expand_tilde(&path).is_dir())
}

fn expand_tilde(path: &str) -> std::path::PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(path)
}

// ── Overlay window ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn show_overlay(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let (position, monitor_pref) = {
        let cfg = state.config.lock().await;
        (cfg.data.ui.overlay_position.clone(), cfg.data.ui.overlay_monitor.clone())
    };

    // The overlay computes pixel coordinates from the anchor itself.
    let pos_msg = serde_json::json!({
        "type": "position",
        "position": position,
        "monitor": monitor_pref,
    });
    let status_msg = serde_json::json!({
        "type": "status",
        "recording": true,
        "processing": false,
        "speaking": false,
        "audio_ready": true,
        "audio_level": 0.0,
        "active_target_label": "Overlay Test",
    });

    if let Ok(s) = serde_json::to_string(&pos_msg) {
        let _ = state.overlay_tx.send(s);
    }
    if let Ok(s) = serde_json::to_string(&status_msg) {
        let _ = state.overlay_tx.send(s);
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_overlay(
    _app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let status_msg = serde_json::json!({
        "type": "status",
        "recording": false,
        "processing": false,
        "speaking": false,
        "audio_ready": true,
        "audio_level": 0.0,
        "active_target_label": "Focused Window",
    });
    
    if let Ok(s) = serde_json::to_string(&status_msg) {
        let _ = state.overlay_tx.send(s);
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct CustomOverlayInfo {
    pub name: String,
    pub html: String,
    pub css: String,
}

#[tauri::command]
pub async fn get_custom_overlays() -> Result<Vec<CustomOverlayInfo>, String> {
    let overlays_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voxctrl")
        .join("overlays");

    if !overlays_dir.exists() {
        let _ = std::fs::create_dir_all(&overlays_dir);
    }

    let mut list = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&overlays_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let mut folder_name = entry.file_name().to_string_lossy().to_string();

                    // Filter out legacy gradient-wave
                    if folder_name.to_lowercase() == "gradient-wave" || folder_name.to_lowercase() == "gradient_wave" {
                        continue;
                    }

                    // Automatically resolve naming conflicts with built-in styles
                    let reserved = [
                        "waveform", "pulse", "blue_wave", "voice_card", "none",
                        "mono_bars", "spectrum", "terminal", "vinyl",
                    ];
                    if reserved.contains(&folder_name.to_lowercase().as_str()) {
                        folder_name = format!("{}_custom", folder_name);
                    }

                    let html_path = entry.path().join("index.html");
                    let css_path = entry.path().join("style.css");

                    let html = if html_path.exists() {
                        std::fs::read_to_string(&html_path).unwrap_or_default()
                    } else {
                        String::new()
                    };

                    let css = if css_path.exists() {
                        std::fs::read_to_string(&css_path).unwrap_or_default()
                    } else {
                        String::new()
                    };

                    list.push(CustomOverlayInfo {
                        name: folder_name,
                        html,
                        css,
                    });
                }
            }
        }
    }

    Ok(list)
}

// ── Audio devices ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(true);
    info!("Audio monitoring started");
    Ok(())
}

#[tauri::command]
pub async fn stop_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(false);
    info!("Audio monitoring stopped");
    Ok(())
}

/// Tell the gesture handler to silently drop any incoming hotkey events.
/// Called when the user opens the keybind recorder in Settings so they cannot
/// accidentally trigger dictation while pressing keys for a new binding.
#[tauri::command]
pub async fn set_hotkeys_inhibited(
    inhibited: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.set_hotkeys_inhibited(inhibited);
    info!("Hotkeys inhibited: {inhibited}");
    Ok(())
}

#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let devices = voxctrl_audio::list_input_devices();
    Ok(devices
        .into_iter()
        .map(|d| AudioDeviceInfo { index: d.index, name: d.name })
        .collect())
}

#[derive(serde::Serialize)]
pub struct AudioDeviceInfo {
    pub index: u32,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct OpenAiTestResult {
    pub success: bool,
    pub message: String,
    pub models: Vec<String>,
}

#[tauri::command]
pub async fn test_openai(
    endpoint: String,
    api_key: Option<String>,
    timeout_secs: u64,
) -> Result<OpenAiTestResult, String> {
    use voxctrl_config::OpenAiConfig;
    let cfg = OpenAiConfig {
        enabled: true,
        endpoint: endpoint.clone(),
        api_key,
        model: String::new(),
        timeout_secs,
        ..Default::default()
    };
    let client = voxctrl_llm::OpenAiClient::new(cfg);
    if client.is_available().await {
        match client.list_models().await {
            Ok(models) => {
                Ok(OpenAiTestResult {
                    success: true,
                    message: "Successfully connected to the OpenAI API server!".to_string(),
                    models,
                })
            }
            Err(e) => {
                Ok(OpenAiTestResult {
                    success: true,
                    message: format!("Successfully connected, but failed to fetch model list: {}", e),
                    models: Vec::new(),
                })
            }
        }
    } else {
        Ok(OpenAiTestResult {
            success: false,
            message: format!("Failed to connect to the OpenAI API server at '{}'. Check the URL and API key.", endpoint),
            models: Vec::new(),
        })
    }
}

#[derive(serde::Serialize, Clone)]
pub struct HotkeyStatusPayload {
    /// Global shortcuts can fire right now.
    pub is_active: bool,
    /// Which mechanism is delivering them: `portal`, `evdev`, `windows_hook`,
    /// `starting` or `none`.
    pub backend: String,
    /// VoxCtrl receives only its own shortcuts and can read no keystrokes.
    /// True on the portal and the Windows hook; false on the evdev fallback.
    pub is_private: bool,
    /// Why the desktop portal is not in use, when it is not.
    pub portal_error: Option<String>,
    /// The portal exists and refused VoxCtrl, rather than being absent. Needs
    /// different advice: switching desktops would not help.
    pub portal_refused: bool,
    /// What the compositor actually bound, which may differ from what VoxCtrl
    /// asked for — the user gets the final say in the portal's own dialog.
    pub shortcuts: Vec<voxctrl_hotkeys::BoundShortcut>,
    /// Gesture styles the running backend can actually deliver, as the same
    /// snake_case names the bindings file uses. The settings UI offers exactly
    /// these, so a user is never shown a gesture that silently does nothing.
    pub supported_gestures: Vec<voxctrl_routing::GestureType>,
    /// Why the X11 backend was not used, when it was not. Distinct from
    /// `portal_error`: a Wayland Cinnamon user needs both reasons to make sense
    /// of why neither worked.
    pub x11_error: Option<String>,
    /// `wayland`, `x11` or whatever `XDG_SESSION_TYPE` says.
    pub session_type: String,
    /// `/dev/input/event*` nodes present, and how many VoxCtrl could open.
    /// Only meaningful for the evdev fallback.
    pub devices_total: u32,
    pub devices_readable: u32,
    /// The user has to do something outside VoxCtrl for shortcuts to work.
    pub needs_attention: bool,
    /// One-line, human-readable explanation of the state above.
    pub detail: String,
    /// KDE registers portal shortcuts into System Settings in a *disabled*
    /// state — the user must open Shortcuts, tick each one, and click Apply
    /// before it fires. This is a confirmed upstream xdg-desktop-portal-kde
    /// bug (bugs.kde.org #483639), not something VoxCtrl's registration got
    /// wrong, and the portal protocol has no "enabled" bit for VoxCtrl to
    /// check — so this is a standing warning on KDE + portal, not a detected
    /// fact about any particular shortcut.
    pub needs_manual_enable: bool,
    /// What to tell the user about `needs_manual_enable`, and how to fix it.
    /// `None` when `needs_manual_enable` is false.
    pub manual_enable_hint: Option<String>,
    /// Running on Linux Mint's Cinnamon or MATE desktop environment.
    pub is_mint_desktop: bool,
    /// VoxCtrl's native D-Bus shortcut is registered in Mint's gsettings registry.
    pub mint_shortcut_registered: bool,
}

/// `XDG_CURRENT_DESKTOP` is a colon-separated list (e.g. `ubuntu:GNOME`); any
/// entry can match. `KDE_FULL_SESSION` is a legacy fallback Plasma still sets
/// when the desktop-name entry is missing or non-standard.
#[cfg(target_os = "linux")]
fn desktop_environment() -> Option<String> {
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if current.split(':').any(|d| d.eq_ignore_ascii_case("KDE")) {
        return Some("KDE".to_string());
    }
    if std::env::var_os("KDE_FULL_SESSION").is_some() {
        return Some("KDE".to_string());
    }
    if current.split(':').any(|d| d.eq_ignore_ascii_case("GNOME")) {
        return Some("GNOME".to_string());
    }
    current
        .split(':')
        .find(|d| !d.is_empty())
        .map(|d| d.to_string())
}

#[cfg(not(target_os = "linux"))]
fn desktop_environment() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn session_type() -> String {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "wayland".to_string()
    } else if std::env::var_os("DISPLAY").is_some() {
        "x11".to_string()
    } else {
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string())
    }
}

#[cfg(not(target_os = "linux"))]
fn session_type() -> String {
    std::env::consts::OS.to_string()
}

/// Returns `(total, readable)` counts of `/dev/input/event*` nodes.
///
/// Only used to explain the evdev fallback. On the portal path VoxCtrl opens
/// none of these.
#[cfg(target_os = "linux")]
fn count_input_devices() -> (u32, u32) {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return (0, 0);
    };
    let mut total = 0;
    let mut readable = 0;
    for entry in entries.flatten() {
        let is_event = entry
            .file_name()
            .to_str()
            .map(|n| n.starts_with("event"))
            .unwrap_or(false);
        if !is_event {
            continue;
        }
        total += 1;
        if std::fs::File::open(entry.path()).is_ok() {
            readable += 1;
        }
    }
    (total, readable)
}

#[cfg(not(target_os = "linux"))]
fn count_input_devices() -> (u32, u32) {
    (0, 0)
}

/// How VoxCtrl is receiving global shortcuts, and what — if anything — is
/// stopping it.
///
/// This replaced a udev/`input`-group audit. VoxCtrl no longer configures
/// keyboard access at all, so the question is no longer "did our setup run?"
/// but "does this desktop offer the shortcuts portal, and if not, can we fall
/// back to access the user has already granted themselves?".
pub fn hotkey_status(health: &voxctrl_hotkeys::ListenerHealth) -> HotkeyStatusPayload {
    if let Ok(override_val) = std::env::var("VOXCTRL_TEST_HOTKEY_STATUS") {
        if let Some(payload) = test_override(&override_val) {
            return payload;
        }
    }

    let backend = health.backend();
    let desktop = desktop_environment();
    let is_mint_desktop = crate::mint_shortcuts::is_mint_desktop();
    let mint_shortcut_registered = if is_mint_desktop {
        crate::mint_shortcuts::is_mint_shortcut_registered()
    } else {
        false
    };

    // A Mint shortcut that is registered and bound is a working backend even if
    // the listener never claimed one — the desktop, not VoxCtrl, is holding it.
    let effective_backend = if backend == voxctrl_hotkeys::Backend::None && mint_shortcut_registered
    {
        voxctrl_hotkeys::Backend::MintDbus
    } else {
        backend
    };

    let needs_manual_enable = backend == voxctrl_hotkeys::Backend::Portal
        && desktop.as_deref() == Some("KDE");
    let (devices_total, devices_readable) = match effective_backend {
        // None of these open an input device, so a device count would only
        // invite the user to fix a permission problem they do not have.
        voxctrl_hotkeys::Backend::Portal
        | voxctrl_hotkeys::Backend::WindowsHook
        | voxctrl_hotkeys::Backend::X11
        | voxctrl_hotkeys::Backend::MintDbus => (0, 0),
        _ => count_input_devices(),
    };
    let shortcuts = health.bound_shortcuts();

    let detail = match effective_backend {
        voxctrl_hotkeys::Backend::Portal => {
            let unbound = shortcuts.iter().filter(|s| !s.bound).count();
            if unbound > 0 {
                format!(
                    "Your desktop registered VoxCtrl's shortcuts, but {unbound} of {} could \
                     not be bound. Pick different keys for those, or set them in your \
                     desktop's own shortcut settings.",
                    shortcuts.len()
                )
            } else {
                "Your desktop is handling VoxCtrl's global shortcuts. VoxCtrl cannot read \
                 your keyboard — it is only told when its own shortcut fires."
                    .to_string()
            }
        }
        voxctrl_hotkeys::Backend::WindowsHook => (
            "VoxCtrl is receiving shortcuts through a Windows low-level keyboard hook.              Every gesture style works, including bare modifiers, and no permission setup              was needed — but in this mode every keystroke passes through VoxCtrl. Keys are              matched against your shortcuts and discarded; nothing is stored or sent              anywhere. Windows does not deliver keys to this hook while an elevated              application has focus, or on the secure desktop (the UAC prompt and the lock              screen), so shortcuts do not fire there."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::X11 => (
            "Your desktop has no global-shortcuts portal, so VoxCtrl is reading X11 key \
             events directly. Every gesture style works, including bare modifiers, and no \
             permission setup was needed — but in this mode every keystroke passes through \
             VoxCtrl."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::MintDbus => (
            "Your desktop is handling VoxCtrl's shortcuts through its own keyboard settings. \
             VoxCtrl cannot read your keyboard. This route only carries a press, never a \
             release, so it can serve tap-to-start/tap-to-stop bindings and no other gesture."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::Evdev => format!(
            "This desktop does not offer the global-shortcuts portal, so VoxCtrl is reading \
             input devices directly ({devices_readable} of {devices_total} readable). That \
             works, but it means every keystroke passes through VoxCtrl."
        ),
        voxctrl_hotkeys::Backend::Starting => "Starting the shortcut listener…".to_string(),
        voxctrl_hotkeys::Backend::None if health.portal_refused() => {
            "Global shortcuts require approval from your desktop. The system prompt was \
             closed or declined before shortcuts were registered — click Approve Shortcuts \
             below to display the prompt and confirm your keybinds."
                .to_string()
        }
        voxctrl_hotkeys::Backend::None => {
            if is_mint_desktop {
                "This desktop (Linux Mint) does not provide the XDG global-shortcuts portal, but supports registering native custom shortcuts via System Settings / D-Bus."
                    .to_string()
            } else if devices_total == 0 {
                "No global-shortcuts portal and no input devices were found, so VoxCtrl \
                 cannot receive shortcuts on this system."
                    .to_string()
            } else {
                format!(
                    "This desktop does not provide the XDG global-shortcuts portal, so \
                     VoxCtrl has no way to receive its shortcuts. VoxCtrl will not grant \
                     itself keyboard access to work around it: doing so would let every \
                     program you run read everything you type. None of the {devices_total} \
                     input devices on this system is readable."
                )
            }
        }
    };

    let is_active = health.is_active() || (is_mint_desktop && mint_shortcut_registered);

    HotkeyStatusPayload {
        is_active,
        backend: match effective_backend {
            voxctrl_hotkeys::Backend::Portal => "portal",
            voxctrl_hotkeys::Backend::X11 => "x11",
            voxctrl_hotkeys::Backend::MintDbus => "mint_dbus",
            voxctrl_hotkeys::Backend::Evdev => "evdev",
            voxctrl_hotkeys::Backend::WindowsHook => "windows_hook",
            voxctrl_hotkeys::Backend::Starting => "starting",
            voxctrl_hotkeys::Backend::None => "none",
        }
        .to_string(),
        is_private: health.is_private() || (is_mint_desktop && mint_shortcut_registered),
        portal_error: health.portal_error(),
        portal_refused: health.portal_refused(),
        shortcuts,
        supported_gestures: effective_backend.gestures().to_vec(),
        x11_error: health.x11_error(),
        session_type: session_type(),
        devices_total,
        devices_readable,
        needs_attention: !is_active,
        detail,
        needs_manual_enable,
        manual_enable_hint: needs_manual_enable.then(|| {
            "KDE registers VoxCtrl's shortcuts as disabled until you turn them on yourself: \
             open Shortcuts, find VoxCtrl, tick the box next to each shortcut, and click \
             Apply. This is a known KDE bug (xdg-desktop-portal-kde #483639), not something \
             VoxCtrl's setup missed — the portal gives VoxCtrl no way to tell whether a \
             shortcut is enabled, so this step cannot be automated or skipped."
                .to_string()
        }),
        is_mint_desktop,
        mint_shortcut_registered,
    }
}

/// Developer/test override so the setup window can be exercised in every state
/// without a desktop session to match.
fn test_override(value: &str) -> Option<HotkeyStatusPayload> {
    let base = HotkeyStatusPayload {
        is_active: true,
        backend: "portal".to_string(),
        is_private: true,
        portal_error: None,
        portal_refused: false,
        shortcuts: Vec::new(),
        supported_gestures: voxctrl_hotkeys::Backend::Portal.gestures().to_vec(),
        x11_error: None,
        session_type: "wayland".to_string(),
        devices_total: 0,
        devices_readable: 0,
        needs_attention: false,
        detail: "Your desktop is handling VoxCtrl's global shortcuts.".to_string(),
        needs_manual_enable: false,
        manual_enable_hint: None,
        is_mint_desktop: false,
        mint_shortcut_registered: false,
    };
    match value {
        "portal" => Some(base),
        "mint" => Some(HotkeyStatusPayload {
            is_active: false,
            backend: "none".to_string(),
            is_private: false,
            portal_error: Some("no such interface".to_string()),
            is_mint_desktop: true,
            mint_shortcut_registered: false,
            needs_attention: true,
            detail: "This desktop (Linux Mint) does not provide the XDG global-shortcuts portal, but supports registering native custom shortcuts via System Settings / D-Bus.".to_string(),
            ..base
        }),
        "mint_registered" => Some(HotkeyStatusPayload {
            is_active: true,
            backend: "mint_dbus".to_string(),
            is_private: true,
            portal_error: Some("no such interface".to_string()),
            is_mint_desktop: true,
            mint_shortcut_registered: true,
            needs_attention: false,
            detail: "Linux Mint native desktop shortcut (Ctrl+Alt+Space) is registered in System Settings and triggers VoxCtrl over D-Bus.".to_string(),
            ..base
        }),
        "kde_manual_enable" => Some(HotkeyStatusPayload {
            needs_manual_enable: true,
            manual_enable_hint: Some(
                "KDE registers VoxCtrl's shortcuts as disabled until you turn them on \
                 yourself: open Shortcuts, find VoxCtrl, tick the box next to each \
                 shortcut, and click Apply."
                    .to_string(),
            ),
            ..base
        }),
        "evdev" => Some(HotkeyStatusPayload {
            backend: "evdev".to_string(),
            is_private: false,
            portal_error: Some("no such interface".to_string()),
            devices_total: 6,
            devices_readable: 6,
            detail: "VoxCtrl is reading input devices directly.".to_string(),
            ..base
        }),
        "none" => Some(HotkeyStatusPayload {
            is_active: false,
            backend: "none".to_string(),
            is_private: false,
            portal_error: Some("no such interface".to_string()),
            devices_total: 6,
            devices_readable: 0,
            needs_attention: true,
            detail: "This desktop does not provide the XDG global-shortcuts portal.".to_string(),
            ..base
        }),
        "refused" => Some(HotkeyStatusPayload {
            is_active: false,
            backend: "none".to_string(),
            is_private: false,
            portal_error: Some(
                "org.freedesktop.portal.Error.NotAllowed: An app id is required".to_string(),
            ),
            portal_refused: true,
            needs_attention: true,
            detail: "Your desktop has a global-shortcuts portal but refused VoxCtrl's \
                     request for one."
                .to_string(),
            ..base
        }),
        _ => None,
    }
}

/// Launch the desktop's own global-shortcuts settings panel, best-effort.
///
/// Exists for the KDE `needs_manual_enable` case — there is no D-Bus verb that
/// flips a portal shortcut from disabled to enabled, so the fix genuinely is
/// "open this panel yourself". Tries the KDE System Settings module directly
/// first (skips the home screen the user would otherwise have to navigate
/// through by hand, which is the whole point of the button), then falls back
/// to whatever System Settings binary exists, then to GNOME's keyboard panel
/// on the off chance this is ever needed there too.
///
/// Errors only when nothing in the list is installed — a real signal the user
/// is not actually on the desktop this feature assumes, worth surfacing
/// rather than silently doing nothing.
#[tauri::command]
pub async fn open_shortcut_settings() -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        return Err("Opening system shortcut settings is only supported on Linux.".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        for (bin, args) in shortcut_settings_candidates() {
            if !crate::installer::command_exists(bin) {
                continue;
            }
            match spawn_shortcut_settings(bin, args) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("Found `{bin}` but failed to launch it: {e}");
                    continue;
                }
            }
        }

        Err("Could not find a way to open your desktop's shortcut settings automatically. \
             Open System Settings yourself and look for Shortcuts → VoxCtrl."
            .to_string())
    }
}

/// Request or retry global shortcut registration via the XDG desktop portal.
#[tauri::command]
pub async fn retry_portal_shortcuts(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let Some(bindings) = crate::stop_key::listener_bindings_from_disk(&state).await else {
            return Err("Could not read your saved shortcuts, so there is nothing to \
                        register. Check the log for what went wrong reading bindings.toml."
                .to_string());
        };

        let gesture_tx = {
            let gtx_guard = state.hotkey_gesture_tx.lock().await;
            gtx_guard.clone()
        };
        let Some(gesture_tx) = gesture_tx else {
            return Err("Hotkey gesture channel is not available.".to_string());
        };

        let (reloader_tx, reloader_rx) = crossbeam_channel::unbounded();
        {
            let mut reloader = state.hotkey_reloader.lock().await;
            *reloader = Some(reloader_tx);
        }

        voxctrl_hotkeys::retry_portal(
            bindings,
            gesture_tx,
            reloader_rx,
            state.hotkey_health.clone(),
        )
        .await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        Ok(())
    }
}

/// Tried in order: the KDE System Settings module directly (skips the home
/// screen the user would otherwise navigate through by hand — the whole point
/// of the button), then whichever System Settings binary exists, then GNOME's
/// keyboard panel on the off chance this is ever needed there too.
#[cfg(target_os = "linux")]
fn shortcut_settings_candidates() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("kcmshell6", &["kcm_keys"]),
        ("kcmshell5", &["kcm_keys"]),
        ("systemsettings6", &["kcm_keys"]),
        ("systemsettings", &["kcm_keys"]),
        ("gnome-control-center", &["keyboard"]),
    ]
}

/// Fire-and-forget: these are GUI apps meant to stay open long after this
/// command returns, so there is nothing useful to await here.
#[cfg(target_os = "linux")]
fn spawn_shortcut_settings(bin: &str, args: &[&str]) -> Result<(), String> {
    #[cfg(test)]
    {
        if std::env::var_os("VOXCTRL_INSTALLER_TEST_MOCK").is_some() {
            return Ok(());
        }
    }
    std::process::Command::new(bin)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Verdict on a key combination the user just recorded.
#[derive(serde::Serialize, Clone)]
pub struct HotkeyKeysCheck {
    /// The combination can be saved as-is.
    pub accepted: bool,
    /// True when a rejection is binding rather than advisory — i.e. shortcuts
    /// are delivered by the desktop portal, which cannot register this. On the
    /// evdev fallback and on Windows, VoxCtrl watches the keys itself and a
    /// bare modifier works fine, so the same combination is merely flagged.
    pub enforced: bool,
    /// The shortcut as the desktop will see it, e.g. `LOGO+space`.
    pub accelerator: Option<String>,
    /// Machine-readable problem: `modifiers_only`, `multiple_keys`,
    /// `unsupported_key` or `empty`.
    pub problem: Option<String>,
    /// What to tell the user, and what to press instead.
    pub message: Option<String>,
}

impl HotkeyKeysCheck {
    fn ok(accelerator: Option<String>) -> Self {
        Self {
            accepted: true,
            enforced: false,
            accelerator,
            problem: None,
            message: None,
        }
    }
}

/// A combination the desktop can bind, plus a warning when *holding* it would
/// cost the rest of the desktop the key.
///
/// A binding here is registered for as long as VoxCtrl runs, and where the
/// compositor owns the grab that registration is exclusive: bare Escape would
/// reach VoxCtrl and nothing else, so menus would stop closing everywhere. It
/// is still the user's call — they may genuinely want Escape to start dictation
/// — so this is advice, not a refusal. The TTS stop key is the case that must
/// not carry this cost silently, and it does not: `stop_key` holds it only
/// while VoxCtrl is speaking.
fn standing_grab_check(
    keys: &[String],
    accelerator: String,
    health: &voxctrl_hotkeys::ListenerHealth,
) -> HotkeyKeysCheck {
    let takes_the_key = voxctrl_hotkeys::is_reserved_for_the_desktop(keys)
        && !health.backend().sees_raw_keys();
    if !takes_the_key {
        return HotkeyKeysCheck::ok(Some(accelerator));
    }
    HotkeyKeysCheck {
        // It genuinely works. Refusing it would be a lie, and the cost is the
        // user's to weigh.
        accepted: true,
        enforced: false,
        accelerator: Some(accelerator),
        problem: Some("reserved_key".to_string()),
        message: Some(
            "Your desktop hands a registered shortcut to VoxCtrl alone, and a binding is \
             held for as long as VoxCtrl runs — so with Escape bound here, an open menu or \
             dialog would stop closing anywhere on your desktop. Add a modifier \
             (Ctrl+Escape) to avoid that. The TTS stop key is safe to leave on Escape: it \
             is held only while VoxCtrl is speaking."
                .to_string(),
        ),
    }
}

/// Can this key combination be registered as a global shortcut?
///
/// The settings UI calls this instead of reimplementing the rules, so the key
/// recorder and the portal registration can never disagree about what is
/// valid. `voxctrl_hotkeys::accelerator` is the single definition.
pub fn check_hotkey_keys_with(
    keys: &[String],
    health: &voxctrl_hotkeys::ListenerHealth,
) -> HotkeyKeysCheck {
    use voxctrl_hotkeys::TriggerProblem;

    let problem = match voxctrl_hotkeys::accelerator(keys) {
        Ok(accelerator) => return standing_grab_check(keys, accelerator, health),
        Err(problem) => problem,
    };

    // Only the backends that hand the grab to the desktop actually cannot
    // deliver these. Blocking them everywhere would break bare-modifier
    // shortcuts on the backends where VoxCtrl watches the keys itself — X11,
    // evdev and the Windows hook — where they work perfectly well.
    let enforced = !health.backend().sees_raw_keys();

    let hint = match problem {
        TriggerProblem::ModifiersOnly => Some(
            "Add a regular key to the combination — Super+Space and Ctrl+Alt+D both work.",
        ),
        TriggerProblem::MultipleKeys => {
            Some("Keep one regular key and use modifiers for the rest.")
        }
        TriggerProblem::UnsupportedKey(_) => Some("Try a letter, number, function or arrow key."),
        TriggerProblem::Empty => None,
    };

    let mut message = if enforced {
        format!("Your desktop cannot register this shortcut: {problem}.")
    } else {
        format!(
            "This works right now, because VoxCtrl is watching the keyboard itself rather \
             than using the desktop's shortcut service. It will stop working if that \
             changes: {problem}."
        )
    };
    if let Some(hint) = hint {
        message.push(' ');
        message.push_str(hint);
    }

    HotkeyKeysCheck {
        // Advisory-only rejections still save: the combination genuinely works
        // on this machine, and refusing it would be a lie.
        accepted: !enforced,
        enforced,
        accelerator: None,
        problem: Some(
            match problem {
                TriggerProblem::Empty => "empty",
                TriggerProblem::ModifiersOnly => "modifiers_only",
                TriggerProblem::MultipleKeys => "multiple_keys",
                TriggerProblem::UnsupportedKey(_) => "unsupported_key",
            }
            .to_string(),
        ),
        message: Some(message),
    }
}

#[tauri::command]
pub async fn check_hotkey_keys(
    keys: Vec<String>,
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyKeysCheck, String> {
    Ok(check_hotkey_keys_with(&keys, &state.hotkey_health))
}

#[tauri::command]
pub async fn check_hotkey_status(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyStatusPayload, String> {
    Ok(hotkey_status(&state.hotkey_health))
}

#[tauri::command]
pub async fn register_mint_shortcut(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<String, String> {
    let result = crate::mint_shortcuts::register_mint_shortcut(None)?;
    state
        .hotkey_health
        .set_backend(voxctrl_hotkeys::Backend::MintDbus);
    Ok(result)
}

#[tauri::command]
pub async fn approve_shortcuts(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyStatusPayload, String> {
    if crate::mint_shortcuts::is_mint_desktop()
        && state.hotkey_health.backend() == voxctrl_hotkeys::Backend::None
    {
        crate::mint_shortcuts::register_mint_shortcut(None)?;
        state
            .hotkey_health
            .set_backend(voxctrl_hotkeys::Backend::MintDbus);
    } else if state.hotkey_health.backend() == voxctrl_hotkeys::Backend::Portal {
        let _ = open_shortcut_settings().await;
    } else {
        let _ = retry_portal_shortcuts(state.clone()).await;
    }
    Ok(hotkey_status(&state.hotkey_health))
}

/// Install the host packages VoxCtrl needs to type text into other windows.
///
/// Never touches keyboard permissions — global shortcuts come from the desktop
/// portal, which needs none.
#[tauri::command]
pub async fn install_system_integration(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyStatusPayload, String> {
    crate::installer::run_gui_installer().await?;
    Ok(hotkey_status(&state.hotkey_health))
}

/// The keystroke-injection helper this session needs, if it is not installed.
///
/// The package step of the setup script is deliberately best-effort (a stale
/// mirror must not block the permission fix), so it really can leave a system
/// with working hotkeys and no way to type the transcription anywhere. That
/// failure is otherwise invisible: dictation appears to do nothing at all.
pub fn missing_injection_tool() -> Option<&'static str> {
    #[cfg(not(target_os = "linux"))]
    {
        None
    }

    #[cfg(target_os = "linux")]
    {
        let have = |name: &str| voxctrl_config::find_in_path(name).is_some();
        if have("wtype") || have("xdotool") {
            return None;
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Some("wtype")
        } else {
            Some("xdotool")
        }
    }
}

/// Everything first-run setup depends on, in one call: how global shortcuts are
/// being delivered, whether text can be typed anywhere, and whether a speech
/// model is on disk.
#[derive(serde::Serialize)]
pub struct SetupStatusPayload {
    pub hotkeys: HotkeyStatusPayload,
    /// Shortcuts can fire right now. Mirrors `hotkeys.is_active` so the UI can
    /// gate on it without reaching into the nested payload.
    pub hotkeys_active: bool,
    pub model_ready: bool,
    pub model_size: String,
    /// The configured model downloads itself in the background at first launch.
    pub model_auto_downloads: bool,
    /// Name of the missing keystroke-injection helper, if any.
    pub missing_injection_tool: Option<String>,
    /// Graphical privilege escalation is available for the one-click install.
    pub pkexec_available: bool,
    /// Commands that install the host packages by hand, for machines with no
    /// polkit agent. Empty when the distro is unknown.
    pub manual_package_commands: String,
    pub is_complete: bool,
}

#[tauri::command]
pub async fn get_setup_status(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<SetupStatusPayload, String> {
    let hotkeys = hotkey_status(&state.hotkey_health);
    let hotkeys_active = hotkeys.is_active;

    let (model_size, model_dir, uses_whisper_model) = {
        let cfg = state.config.lock().await;
        let eng = &cfg.data.engine;
        (
            eng.whisper_cpp.model_size.clone(),
            eng.whisper_cpp.model_dir.clone(),
            eng.backend != voxctrl_config::BackendChoice::Moonshine
                || !voxctrl_inference::MOONSHINE_COMPILED,
        )
    };

    let model_ready = !uses_whisper_model
        || voxctrl_inference::whisper_cpp::is_model_downloaded(&model_size, &model_dir);
    let model_auto_downloads =
        uses_whisper_model && voxctrl_inference::whisper_cpp::is_small_auto_downloadable(&model_size);

    let missing_tool = missing_injection_tool();

    Ok(SetupStatusPayload {
        is_complete: hotkeys_active && model_ready && missing_tool.is_none(),
        hotkeys,
        hotkeys_active,
        model_ready,
        model_size,
        model_auto_downloads,
        missing_injection_tool: missing_tool.map(str::to_string),
        pkexec_available: crate::installer::command_exists("pkexec"),
        manual_package_commands: crate::installer::manual_setup_commands(
            crate::installer::detect_pkg_manager(),
        ),
    })
}

/// Download whichever speech model the config currently selects.
///
/// The setup window has no business knowing the model directory layout, and
/// asking the user to pick a model before they have used the app once is the
/// step this whole flow is trying to remove.
#[tauri::command]
pub async fn download_configured_model(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<(), String> {
    let (model_size, model_dir) = {
        let cfg = state.config.lock().await;
        (
            cfg.data.engine.whisper_cpp.model_size.clone(),
            cfg.data.engine.whisper_cpp.model_dir.clone(),
        )
    };
    voxctrl_inference::whisper_cpp::download_model(&model_size, &model_dir)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Re-open the first-run wizard on demand.
///
/// Setup is not a one-time event in practice: a user changes microphone, moves
/// to a machine with a GPU, or wants to redo a hotkey they regret — and, more
/// immediately, whoever is developing the wizard needs to see it without
/// hand-editing `setup_completed` out of their config file.
#[tauri::command]
pub async fn open_setup_wizard(app: tauri::AppHandle) -> Result<(), String> {
    crate::window::open_wizard_window(&app)
}

/// Mark the first-run wizard finished and get out of the way.
///
/// The flag is written here rather than through `save_config` so that closing
/// the wizard is one atomic step: the config the wizard has been editing all
/// along is already persisted, and this only flips the bit that decides
/// whether the wizard opens again on the next launch.
#[tauri::command]
pub async fn finish_setup_wizard(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
    open_settings: bool,
) -> Result<(), String> {
    let updated = {
        let mut guard = state.config.lock().await;
        guard.data.ui.setup_completed = true;
        guard.save().map_err(|e| e.to_string())?;
        guard.data.clone()
    };
    let _ = app.emit("config-changed", updated);
    info!("First-run setup wizard completed");

    if let Some(window) = app.get_webview_window(crate::window::WIZARD_WINDOW) {
        let _ = window.hide();
        let _ = window.close();
    }
    if open_settings {
        if let Err(e) = crate::window::open_settings_window(&app) {
            tracing::error!("Could not open Settings after setup: {e}");
        }
    }
    Ok(())
}

/// Open the settings window on a specific tab. Used by the setup window so
/// "choose a different model" lands the user on the right screen instead of
/// making them find it.
#[tauri::command]
pub async fn open_settings_tab(app: tauri::AppHandle, tab: String) -> Result<(), String> {
    let existed = app.get_webview_window("settings").is_some();
    let window = crate::window::open_settings_window(&app)?;
    let _ = window.emit("focus-settings-tab", tab.clone());

    // A window built just now has no listener registered yet, so the first
    // emit lands before anything is listening. Repeating it once the frontend
    // has had a moment to mount costs nothing — selecting the same tab twice
    // is idempotent — and is the difference between landing on the right tab
    // and landing on the default one.
    if !existed {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            let _ = window.emit("focus-settings-tab", tab);
        });
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct MonitorInfo {
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[tauri::command]
pub async fn get_available_monitors(app: tauri::AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_handle = app.clone();
    let res = app.run_on_main_thread(move || {
        let mut list = Vec::new();
        if let Some(w) = app_handle.webview_windows().values().next() {
            if let Ok(monitors) = w.available_monitors() {
                let primary = w.primary_monitor().ok().flatten();
                let primary_name = primary.as_ref().and_then(|m| m.name());

                for m in monitors {
                    let name = m.name().map(|s| s.to_string());
                    let is_primary = primary_name.is_some() && name.as_deref() == primary_name.map(|s| s.as_ref());
                    let size = m.size();
                    list.push(MonitorInfo {
                        name,
                        width: size.width,
                        height: size.height,
                        is_primary,
                    });
                }
            }
        }
        let _ = tx.send(list);
    });

    if let Err(e) = res {
        return Err(format!("Failed to run monitor query on main thread: {}", e));
    }

    rx.await.map_err(|e| format!("Failed to receive monitors: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_custom_overlays_returns_list() {
        let result = get_custom_overlays().await;
        assert!(result.is_ok());
        if let Ok(list) = result {
            // Check that the list is serializable
            let json = serde_json::to_string(&list);
            assert!(json.is_ok());
        }
    }
}


