use std::sync::Arc;

use tauri::{Emitter, Manager, State};
use tracing::info;
use voxctrl_config::AppConfig;
use voxctrl_routing::{HotkeyBinding, OutputTarget};

use crate::state::{AppState, HistoryEntry};

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
    state.set_recording(true);
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

    // Dynamic TTS engine lifecycle management
    {
        let mut handle = state.tts_handle.lock().await;
        if let Some(ref tts) = *handle {
            tts.shutdown();
        }
        if new_config.tts.enabled {
            let app_handle = app.clone();
            let app_handle_end = app.clone();
            let state_clone = state.inner().clone();
            let state_clone_end = state.inner().clone();
            let new_tts = voxctrl_tts::TtsEngineWorker::start(
                new_config.tts.clone(),
                Some(std::sync::Arc::new(move || {
                    state_clone.set_speaking(true);
                    let _ = app_handle.emit("tts-playback-start", ());
                })),
                Some(std::sync::Arc::new(move || {
                    state_clone_end.set_speaking(false);
                    let _ = app_handle_end.emit("tts-playback-end", ());
                })),
            );
            *handle = Some(new_tts.clone());
            state.spawn_fifo_responders(new_tts).await;
        } else {
            *handle = None;
        }
    }

    let mut guard = state.config.lock().await;
    guard.data = new_config.clone();
    guard.save().map_err(|e| e.to_string())?;
    info!("Config saved");

    // Hot-reload the stop key binding in the evdev listener
    {
        let dir = voxctrl_routing::config_dir();
        let mut current_bindings = voxctrl_routing::load_bindings(&dir).unwrap_or_default();
        if !new_config.tts.stop_key.is_empty() {
            use voxctrl_routing::{HotkeyBinding, GestureType};
            current_bindings.push(HotkeyBinding {
                id: "__tts_stop__".to_string(),
                label: "TTS Stop Key".to_string(),
                keys: new_config.tts.stop_key.clone(),
                gesture: GestureType::Hold,
                target_id: String::new(),
                target_ids: vec![],
                tap_ms: 250,
                hold_threshold_ms: 0,
                subkey: None,
                disabled: false,
                ollama_enabled: Some(false),
                ollama_model: None,
                ollama_mode: None,
                ollama_prompt: None,
            });
        }
        let reloader_guard = state.hotkey_reloader.lock().await;
        if let Some(reloader) = &*reloader_guard {
            let _ = reloader.send(current_bindings);
        }
    }

    // Emit config-changed event to all windows to enable instant reactivity
    let _ = app.emit("config-changed", new_config);

    let (overlay_position, overlay_monitor) = (guard.data.ui.overlay_position.clone(), guard.data.ui.overlay_monitor.clone());
    if let Some((x, y)) = crate::calculate_overlay_coordinates(&app, &overlay_position, &overlay_monitor) {
        let pos_msg = serde_json::json!({
            "type": "position",
            "x": x,
            "y": y,
        });
        if let Ok(json_str) = serde_json::to_string(&pos_msg) {
            let _ = state.overlay_tx.send(json_str);
        }
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
    
    // Hot reload the bindings in the active listener threads, always re-injecting the stop key.
    let mut all_bindings = bindings;
    {
        let cfg_guard = state.config.lock().await;
        if !cfg_guard.data.tts.stop_key.is_empty() {
            use voxctrl_routing::GestureType;
            all_bindings.push(HotkeyBinding {
                id: "__tts_stop__".to_string(),
                label: "TTS Stop Key".to_string(),
                keys: cfg_guard.data.tts.stop_key.clone(),
                gesture: GestureType::Hold,
                target_id: String::new(),
                target_ids: vec![],
                tap_ms: 250,
                hold_threshold_ms: 0,
                subkey: None,
                disabled: false,
                ollama_enabled: Some(false),
                ollama_model: None,
                ollama_mode: None,
                ollama_prompt: None,
            });
        }
    }
    let reloader_guard = state.hotkey_reloader.lock().await;
    if let Some(reloader) = &*reloader_guard {
        if let Err(e) = reloader.send(all_bindings) {
            tracing::warn!("Failed to hot-reload bindings: {e}");
        } else {
            info!("Hot-reload signal sent to listener");
        }
    }
    
    Ok(())
}

// ── History ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_history(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HistoryEntry>, String> {
    let guard = state.history.lock().await;
    Ok(guard.clone())
}

#[tauri::command]
pub async fn clear_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.history.lock().await.clear();
    state.word_count.store(0, std::sync::atomic::Ordering::SeqCst);
    Ok(())
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

#[tauri::command]
pub async fn check_kokoro_ready(quality: String, data_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_kokoro_ready(&quality, &data_dir))
}

#[tauri::command]
pub async fn download_kokoro(quality: String, data_dir: String) -> Result<(), String> {
    voxctrl_tts::download_kokoro_assets(&quality, &data_dir)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_model_downloaded(model_size: String, model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_inference::whisper_cpp::is_model_downloaded(&model_size, &model_dir))
}

#[tauri::command]
pub async fn download_model(model_size: String, model_dir: String) -> Result<(), String> {
    voxctrl_inference::whisper_cpp::download_model(&model_size, &model_dir)
        .await
        .map_err(|e| e.to_string())
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
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let (position, monitor_pref) = {
        let cfg = state.config.lock().await;
        (cfg.data.ui.overlay_position.clone(), cfg.data.ui.overlay_monitor.clone())
    };
    
    if let Some((x, y)) = crate::calculate_overlay_coordinates(&app, &position, &monitor_pref) {
        let pos_msg = serde_json::json!({
            "type": "position",
            "x": x,
            "y": y,
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
    info!("Live audio monitoring started");
    Ok(())
}

#[tauri::command]
pub async fn stop_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(false);
    info!("Live audio monitoring stopped");
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
pub struct OllamaTestResult {
    pub success: bool,
    pub message: String,
    pub models: Vec<String>,
}

#[tauri::command]
pub async fn test_ollama(
    endpoint: String,
    timeout_secs: u64,
) -> Result<OllamaTestResult, String> {
    use voxctrl_config::{OllamaConfig, OllamaMode};
    let cfg = OllamaConfig {
        enabled: true,
        endpoint: endpoint.clone(),
        model: String::new(),
        mode: OllamaMode::Clean,
        custom_prompt: None,
        timeout_secs,
    };
    let client = voxctrl_llm::OllamaClient::new(cfg);
    if client.is_available().await {
        match client.list_models().await {
            Ok(models) => {
                Ok(OllamaTestResult {
                    success: true,
                    message: "Successfully connected to Ollama!".to_string(),
                    models,
                })
            }
            Err(e) => {
                Ok(OllamaTestResult {
                    success: true,
                    message: format!("Successfully connected, but failed to fetch model list: {}", e),
                    models: Vec::new(),
                })
            }
        }
    } else {
        Ok(OllamaTestResult {
            success: false,
            message: format!("Failed to connect to Ollama at '{}'. Make sure Ollama is running.", endpoint),
            models: Vec::new(),
        })
    }
}

#[derive(serde::Serialize)]
pub struct UdevStatusPayload {
    pub is_configured: bool,
    pub rule_exists: bool,
    pub in_group: bool,
    pub needs_relogin: bool,
}

#[tauri::command]
pub async fn check_udev_status() -> Result<UdevStatusPayload, String> {
    // 1. Check for developer override via environment variable
    if let Ok(override_val) = std::env::var("VOXCTRL_TEST_UDEV_STATUS") {
        match override_val.as_str() {
            "missing" => {
                return Ok(UdevStatusPayload {
                    is_configured: false,
                    rule_exists: false,
                    in_group: false,
                    needs_relogin: false,
                });
            }
            "relogin" => {
                return Ok(UdevStatusPayload {
                    is_configured: false,
                    rule_exists: true,
                    in_group: false,
                    needs_relogin: true,
                });
            }
            "ok" => {
                return Ok(UdevStatusPayload {
                    is_configured: true,
                    rule_exists: true,
                    in_group: true,
                    needs_relogin: false,
                });
            }
            _ => {}
        }
    }

    // 2. Platform-specific checks
    #[cfg(not(target_os = "linux"))]
    {
        Ok(UdevStatusPayload {
            is_configured: true,
            rule_exists: true,
            in_group: true,
            needs_relogin: false,
        })
    }

    #[cfg(target_os = "linux")]
    {
        // Check if udev rules exist (support new and legacy names)
        let rule_exists = std::path::Path::new("/etc/udev/rules.d/99-voxctrl.rules").exists()
            || std::path::Path::new("/etc/udev/rules.d/99-voxctl.rules").exists()
            || std::path::Path::new("/etc/udev/rules.d/99-voxctr.rules").exists();

        // Check if the current user session has the "input" group by running `id -Gn`
        let mut in_group = match std::process::Command::new("id").args(&["-Gn"]).output() {
            Ok(output) => {
                let groups_str = String::from_utf8_lossy(&output.stdout);
                groups_str.split_whitespace().any(|g| g == "input")
            }
            Err(_) => false,
        };

        // Fallback: If not in group in active session, check system group database (NSS /etc/group).
        // This is robust against sandboxes/containers where active process groups haven't refreshed.
        if !in_group {
            if let Ok(username) = std::env::var("USER") {
                if let Ok(output) = std::process::Command::new("id").args(&["-Gn", &username]).output() {
                    let groups_str = String::from_utf8_lossy(&output.stdout);
                    if groups_str.split_whitespace().any(|g| g == "input") {
                        in_group = true;
                    }
                }
            }
        }

        // If udev rules exist but user is not in group in active session/database, they need a relogin
        let needs_relogin = rule_exists && !in_group;
        let is_configured = rule_exists && in_group;

        Ok(UdevStatusPayload {
            is_configured,
            rule_exists,
            in_group,
            needs_relogin,
        })
    }
}

#[tauri::command]
pub async fn install_system_integration() -> Result<UdevStatusPayload, String> {
    crate::installer::run_gui_installer().await?;
    check_udev_status().await
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
    if let Some(w) = app.webview_windows().values().next() {
        if let Ok(monitors) = w.available_monitors() {
            let mut list = Vec::new();
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
            return Ok(list);
        }
    }
    Ok(Vec::new())
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


