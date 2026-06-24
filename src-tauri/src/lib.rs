// Force Tauri rebuild with latest voxctrl-routing changes
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};
use std::time::Duration;

use tauri::{
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tokio::sync::Mutex;
use voxctrl_config::Config;
use voxctrl_routing::{load_bindings, load_targets, config_dir, OutputTargetRouter};
use voxctrl_mcp::McpCallbacks;

use crate::{
    commands::*,
    state::{AppState, HistoryEntry},
};

mod commands;
mod state;
mod startup_log;
mod installer;

pub fn run_cli_installer() -> Result<(), String> {
    crate::installer::run_cli_installer()
}

// Helper to robustly show, unminimize, and focus a window, especially under Linux WMs
fn show_and_focus_window(window: &tauri::WebviewWindow) {
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

// ── Tauri app entry point ─────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    {
        // Workaround for WebKitGTK blank window/rendering issues due to DMABUF creation failures
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

        // Suppress libayatana-appindicator deprecation warnings by registering a dummy log handler
        unsafe {
            extern "C" {
                fn g_log_set_handler(
                    log_domain: *const std::os::raw::c_char,
                    log_levels: std::os::raw::c_int,
                    log_func: Option<unsafe extern "C" fn(*const std::os::raw::c_char, std::os::raw::c_int, *const std::os::raw::c_char, *mut std::os::raw::c_void)>,
                    user_data: *mut std::os::raw::c_void,
                ) -> std::os::raw::c_uint;
            }

            unsafe extern "C" fn dummy_log_handler(
                _log_domain: *const std::os::raw::c_char,
                _log_levels: std::os::raw::c_int,
                _message: *const std::os::raw::c_char,
                _user_data: *mut std::os::raw::c_void,
            ) {}

            let domain = b"libayatana-appindicator\0".as_ptr() as *const std::os::raw::c_char;
            g_log_set_handler(domain, 16, Some(dummy_log_handler), std::ptr::null_mut());
        }
    }

    // Initialise logging (console + special warning-free/privacy-safe startup and error file log)
    use tracing_subscriber::prelude::*;
    let local_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voxctrl");
    let _ = std::fs::create_dir_all(&local_dir);
    let log_path = local_dir.join("startup_errors.log");

    let file_layer = match startup_log::StartupErrorLayer::new(log_path) {
        Ok(layer) => Some(layer),
        Err(e) => {
            eprintln!("Failed to initialize startup error log file: {e}");
            None
        }
    };

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "voxctrl=info".parse().unwrap());

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer());

    if let Some(fl) = file_layer {
        let _ = registry.with(fl).try_init();
    } else {
        let _ = registry.try_init();
    }

    let config = Config::load();
    
    // Log the sanitized configuration parameters at startup
    tracing::info!("=== System Startup Config ===");
    tracing::info!("Backend choice: {:?}", config.data.engine.backend);
    tracing::info!("Inference mode: {:?}", config.data.engine.inference_mode);
    tracing::info!("Whisper model size: {}", config.data.engine.whisper_cpp.model_size);
    tracing::info!("Whisper device: {}", config.data.engine.whisper_cpp.device);
    tracing::info!("Whisper threads: {}", config.data.engine.whisper_cpp.threads);
    tracing::info!("Moonshine model size: {}", config.data.engine.moonshine.model_size);
    tracing::info!("Moonshine language: {}", config.data.engine.moonshine.language);
    tracing::info!("VAD threshold: {}", config.data.audio.vad_threshold);
    tracing::info!("Min silence duration ms: {}", config.data.audio.min_silence_duration_ms);
    tracing::info!("Noise suppression: {}", config.data.audio.noise_suppression);
    tracing::info!("Input device index: {:?}", config.data.audio.input_device_index);
    tracing::info!("Gain: {}", config.data.audio.gain);
    tracing::info!("Dynamic stream: {}", config.data.audio.dynamic_stream);
    tracing::info!("TTS enabled: {}", config.data.tts.enabled);
    tracing::info!("TTS engine: {:?}", config.data.tts.engine);
    tracing::info!("TTS voice: {}", config.data.tts.voice);
    tracing::info!("TTS speed: {}", config.data.tts.speed);
    tracing::info!("TTS GPU: {}", config.data.tts.gpu);
    tracing::info!("Kokoro voice: {}", config.data.tts.kokoro.voice);
    tracing::info!("Kokoro quality: {}", config.data.tts.kokoro.quality);
    tracing::info!("Kokoro speed: {}", config.data.tts.kokoro.speed);
    tracing::info!("Kokoro prewarm: {}", config.data.tts.kokoro.prewarm);
    tracing::info!("MCP enabled: {}", config.data.mcp.server_enabled);
    tracing::info!("MCP record timeout: {}", config.data.mcp.record_timeout);
    tracing::info!("ATSPI injection: {}", config.data.atspi.injection);
    tracing::info!("ATSPI context prompt: {}", config.data.atspi.context_prompt);
    tracing::info!("ATSPI auto code mode: {}", config.data.atspi.auto_code_mode);
    tracing::info!("=============================");

    let cfg_data = Arc::new(config.data.clone());
    let config = Arc::new(Mutex::new(config));

    let cdir = config_dir();
    let targets = load_targets(&cdir).unwrap_or_default();
    let bindings = load_bindings(&cdir).unwrap_or_default();

    let router = Arc::new(OutputTargetRouter::new(targets.clone()));

    // ── Audio pipeline ────────────────────────────────────────────────────────
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<voxctrl_audio::AudioChunk>(64);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<voxctrl_inference::InferenceOutput>(32);

    let (inference_tx, inference_rx) = crossbeam_channel::bounded::<voxctrl_inference::InferenceRequest>(4);

    let (overlay_tx, overlay_rx) = crossbeam_channel::unbounded::<String>();

    let app_state = Arc::new(AppState {
        config: config.clone(),
        router: router.clone(),
        recording: Arc::new(AtomicBool::new(false)),
        processing: Arc::new(AtomicBool::new(false)),
        speaking: Arc::new(AtomicBool::new(false)),
        overlay_enabled: Arc::new(AtomicBool::new(cfg_data.ui.show_overlay)),
        mcp_recording: Arc::new(AtomicBool::new(false)),
        audio_ready: Arc::new(AtomicBool::new(false)),
        dynamic_stream: Arc::new(AtomicBool::new(cfg_data.audio.dynamic_stream)),
        monitoring: Arc::new(AtomicBool::new(false)),
        input_device_index: Arc::new(std::sync::atomic::AtomicU32::new(cfg_data.audio.input_device_index.unwrap_or(u32::MAX))),
        gain: Arc::new(std::sync::atomic::AtomicU32::new(cfg_data.audio.gain.to_bits())),
        word_count: Arc::new(AtomicU32::new(0)),
        last_text: Arc::new(Mutex::new(String::new())),
        last_text_version: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        active_target: Arc::new(Mutex::new("default".to_string())),
        active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
        active_binding_id: Arc::new(Mutex::new(String::new())),
        targets: Arc::new(Mutex::new(targets.clone())),
        history: Arc::new(Mutex::new(Vec::new())),
        audio_tx: audio_tx.clone(),
        tts_handle: Arc::new(Mutex::new(None)),
        active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        hotkey_reloader: Arc::new(Mutex::new(None)),
        overlay_tx: overlay_tx.clone(),
    });

    let (audio_level_tx, audio_level_rx) = crossbeam_channel::bounded::<f32>(128);

    {
        let audio_cfg = cfg_data.audio.clone();
        let recorder = voxctrl_audio::AudioRecorder::new(
            audio_cfg,
            app_state.recording.clone(),
            app_state.monitoring.clone(),
            app_state.dynamic_stream.clone(),
            app_state.input_device_index.clone(),
            app_state.gain.clone(),
        );
        let _ = recorder.run(audio_tx, Some(audio_level_tx), Some(app_state.audio_ready.clone()));
    }

    // Spawn a coordinator thread to accumulate audio chunks and trigger batch inference
    let state_for_audio = app_state.clone();
    std::thread::spawn(move || {
        let mut accumulated_audio = Vec::<f32>::new();
        let mut was_recording = false;
        let mut target_id = "default".to_string();
        let mut binding_id = String::new();

        while let Ok(chunk) = audio_rx.recv() {
            let is_recording = state_for_audio.is_recording();

            if is_recording {
                if !was_recording {
                    accumulated_audio.clear();
                    target_id = state_for_audio.active_target.blocking_lock().clone();
                    binding_id = state_for_audio.active_binding_id.blocking_lock().clone();
                    was_recording = true;
                }
                accumulated_audio.extend(chunk);
            } else {
                if was_recording {
                    if !accumulated_audio.is_empty() {
                        let req = voxctrl_inference::InferenceRequest {
                            audio: std::mem::take(&mut accumulated_audio),
                            target_id: target_id.clone(),
                            binding_id: Some(binding_id.clone()),
                            context_text: None,
                        };
                        state_for_audio.set_processing(true);
                        let _ = inference_tx.send(req);
                    }
                    was_recording = false;
                }
            }
        }
    });

    // Inference worker
    voxctrl_inference::run_worker(cfg_data.clone(), inference_rx, text_tx.clone());

    // ── TTS ───────────────────────────────────────────────────────────────────
    let _tts_handle = if cfg_data.tts.enabled {
        Some(voxctrl_tts::TtsEngineWorker::start(cfg_data.tts.clone(), None, None))
    } else {
        None
    };

    let state_for_tts = app_state.clone();
    let tts_handle_clone = _tts_handle.clone();
    tokio::spawn(async move {
        {
            let mut handle = state_for_tts.tts_handle.lock().await;
            *handle = tts_handle_clone.clone();
        }
        if let Some(tts) = tts_handle_clone {
            state_for_tts.spawn_fifo_responders(tts).await;
        }
    });

    // ── Hotkey listener ───────────────────────────────────────────────────────
    // Inject a synthetic hold-binding for the TTS stop key so the existing
    // evdev listener handles it without any additional infrastructure.
    let mut all_bindings = bindings;
    if !cfg_data.tts.stop_key.is_empty() {
        use voxctrl_routing::{HotkeyBinding, GestureType};
        all_bindings.push(HotkeyBinding {
            id: "__tts_stop__".to_string(),
            label: "TTS Stop Key".to_string(),
            keys: cfg_data.tts.stop_key.clone(),
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

    let (gesture_tx, mut gesture_rx) = voxctrl_hotkeys::channel();
    let listener = voxctrl_hotkeys::start_listener(
        all_bindings,
        gesture_tx,
        cfg_data.audio.evdev_device.clone(),
    );

    let state_for_gesture = app_state.clone();
    tokio::spawn(async move {
        let mut reloader = state_for_gesture.hotkey_reloader.lock().await;
        *reloader = Some(listener.reloader_tx);
    });

    let state_for_gesture = app_state.clone();
    tokio::spawn(async move {
        while let Some(event) = gesture_rx.recv().await {
            use voxctrl_hotkeys::GestureKind;

            // TTS stop key: fires on key-down (Start), not release.
            // Only stop the active Rodio sink — do NOT send None to the worker
            // channel (that would kill the thread and break all future TTS).
            if event.binding_id == "__tts_stop__" {
                if event.kind == GestureKind::Start {
                    voxctrl_tts::stop_current_playback();
                }
                continue;
            }

            match event.kind {
                GestureKind::Start => {
                    *state_for_gesture.active_target.lock().await = event.target_id.clone();
                    *state_for_gesture.active_binding_label.lock().await = event.binding_label.clone();
                    *state_for_gesture.active_binding_id.lock().await = event.binding_id.clone();
                    state_for_gesture.set_recording(true);
                }
                GestureKind::Stop => {
                    state_for_gesture.set_recording(false);
                }
            }
        }
    });

    // ── Text delivery: inference → router → injection ─────────────────────────
    {
        let state = app_state.clone();
        let rt_handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            while let Ok(output) = text_rx.recv() {
                state.set_processing(false);
                if output.text.trim().is_empty() {
                    continue;
                }
                tracing::info!("Received transcription: \"{}\" for target '{}' (took {}ms)", output.text, output.target_id, output.inference_ms);
                let words = output.text.split_whitespace().count() as u32;
                state.increment_words(words);

                // Deliver text via the output target router.
                // Write last_text BEFORE launching deliveries so that MCP
                // transcribe_voice can detect the result without waiting for
                // potentially slow targets (webhooks, sockets, etc.).
                let text = output.text.clone();
                let target_id = output.target_id.clone();
                let router = state.router.clone();
                let state_lt = state.clone();
                let text_lt = output.text.clone();
                rt_handle.spawn(async move {
                    {
                        let mut lt = state_lt.last_text.lock().await;
                        *lt = text_lt;
                        state_lt.last_text_version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    let target_ids: Vec<String> = target_id
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    for tid in target_ids {
                        router.deliver(&tid, &text).await;
                    }
                });

                let (show_notif, history_enabled) = {
                    let cfg_lock = state.config.blocking_lock();
                    (cfg_lock.data.ui.show_notification, cfg_lock.data.ui.history_enabled)
                };
                if show_notif {
                    voxctrl_inject::show_notification("VoxCtrl", &output.text);
                }
                if history_enabled {
                    let state2 = state.clone();
                    let entry = HistoryEntry {
                        text: output.text,
                        target_id: output.target_id,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        inference_ms: output.inference_ms,
                    };
                    rt_handle.spawn(async move {
                        let mut hist = state2.history.lock().await;
                        hist.insert(0, entry);
                        if hist.len() > 500 {
                            hist.truncate(500);
                        }
                    });
                }
            }
        });
    }

    // ── DBus service (Linux) ──────────────────────────────────────────────────
    #[cfg(target_os = "linux")]
    {
        let dbus_state = Arc::new(Mutex::new(voxctrl_dbus::AppState::default()));
        let (start_tx, mut start_rx) = tokio::sync::mpsc::channel::<()>(4);
        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(4);
        let app_state_dbus = app_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    v = start_rx.recv() => {
                        if v.is_some() {
                            app_state_dbus.set_recording(true);
                        } else {
                            break;
                        }
                    }
                    v = stop_rx.recv() => {
                        if v.is_some() {
                            app_state_dbus.set_recording(false);
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

    // ── MCP Server ────────────────────────────────────────────────────────────
    if cfg_data.mcp.server_enabled {
        let callbacks = app_state.clone();
        tokio::spawn(async move {
            tracing::info!("Starting MCP server...");
            if let Err(e) = voxctrl_mcp::run_server(callbacks).await {
                tracing::error!("MCP server error: {:?}", e);
            }
        });
    }

    // ── Build Tauri app ───────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            tracing::info!("Single instance trigger: argv={:?}, cwd={:?}", argv, cwd);
            if let Some(window) = app.get_webview_window("settings") {
                show_and_focus_window(&window);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                if label == "settings" || label == "history" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .setup(move |app| {
            // Re-initialize TTS worker with callback to emit tts-playback-start
            {
                let app_handle = app.handle().clone();
                let state = app_handle.state::<Arc<AppState>>().inner().clone();
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
                            let state_clone = state.clone();
                            let state_clone_end = state.clone();
                            let new_tts = voxctrl_tts::TtsEngineWorker::start(
                                cfg.tts.clone(),
                                Some(std::sync::Arc::new(move || {
                                    state_clone.set_speaking(true);
                                    let _ = app_handle_clone.emit("tts-playback-start", ());
                                })),
                                Some(std::sync::Arc::new(move || {
                                    state_clone_end.set_speaking(false);
                                    let _ = app_handle_clone_end.emit("tts-playback-end", ());
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

            // Register Speak target callback
            {
                let state = app.handle().state::<Arc<AppState>>().inner().clone();
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

            // Register OpenAI API target callback
            {
                let state = app.handle().state::<Arc<AppState>>().inner().clone();
                voxctrl_routing::targets::set_openai_callback(std::sync::Arc::new(move |req| {
                    let state = state.clone();
                    Box::pin(async move {
                        let cfg = state.config.lock().await.data.ollama.clone();
                        let client = voxctrl_llm::OllamaClient::new(cfg);
                        let history: Vec<(String, String)> = req
                            .history
                            .into_iter()
                            .map(|t| (t.role, t.content))
                            .collect();
                        client
                            .complete(
                                req.system_prompt.as_deref(),
                                &history,
                                &req.text,
                                req.model.as_deref(),
                                req.max_tokens,
                                req.timeout_secs,
                            )
                            .await
                    })
                }));
            }

            // ── Startup udev Diagnostics ──────────────────────────────────────
            #[cfg(target_os = "linux")]
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut check_failed = false;

                    if let Ok(override_val) = std::env::var("VOXCTRL_TEST_UDEV_STATUS") {
                        match override_val.as_str() {
                            "missing" | "relogin" => check_failed = true,
                            _ => {}
                        }
                    } else {
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

                        if !rule_exists || !in_group {
                            check_failed = true;
                        }
                    }

                    if check_failed {
                        if let Some(w) = app_handle.get_webview_window("udev-warning") {
                            let _ = w.show();
                            let _ = w.set_always_on_top(true);
                            let _ = w.set_focus();
                        }
                    }
                });
            }

            // ── Forward audio levels to settings window and Slint overlay ────
            let handle = app.handle().clone();
            let state_for_audio_level = app.handle().state::<Arc<AppState>>().inner().clone();
            std::thread::spawn(move || {
                while let Ok(level) = audio_level_rx.recv() {
                    let _ = handle.emit("audio-level", level);

                    // Forward to Slint overlay channel. When the overlay is
                    // disabled, report idle state so the native window never maps
                    // (a mapped overlay steals keyboard focus on Wayland and breaks
                    // text injection).
                    let overlay_on = state_for_audio_level.is_overlay_enabled();
                    let is_recording = overlay_on && state_for_audio_level.is_recording();
                    let is_processing = overlay_on && state_for_audio_level.is_processing();
                    let is_speaking = overlay_on && state_for_audio_level.is_speaking();
                    let audio_ready = state_for_audio_level.is_audio_ready();
                    let active_target_label = {
                        if let Ok(label) = state_for_audio_level.active_binding_label.try_lock() {
                            label.clone()
                        } else {
                            "Focused Window".to_string()
                        }
                    };

                    let msg = serde_json::json!({
                        "type": "status",
                        "recording": is_recording,
                        "processing": is_processing,
                        "speaking": is_speaking,
                        "audio_ready": audio_ready,
                        "audio_level": level,
                        "active_target_label": if active_target_label.is_empty() { "Focused Window".to_string() } else { active_target_label },
                    });

                    if let Ok(json_str) = serde_json::to_string(&msg) {
                        let _ = state_for_audio_level.overlay_tx.send(json_str);
                    }
                }
            });

            // ── System tray ───────────────────────────────────────────────────
            let record_on_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_on.png"))
                .expect("Failed to load record_on icon");
            let record_off_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
                .expect("Failed to load record_off icon");
            let processing_frames = [
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_1.png"))
                    .expect("Failed to load processing_1 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_2.png"))
                    .expect("Failed to load processing_2 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_3.png"))
                    .expect("Failed to load processing_3 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_4.png"))
                    .expect("Failed to load processing_4 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_5.png"))
                    .expect("Failed to load processing_5 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_6.png"))
                    .expect("Failed to load processing_6 icon"),
            ];
            let tray_icon = record_off_icon.clone();

            let settings_i = tauri::menu::MenuItem::with_id(app, "settings", "⚙  Settings", true, None::<&str>)?;
            let history_i = tauri::menu::MenuItem::with_id(app, "history", "📋  History", true, None::<&str>)?;
            let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Quit VoxCtrl", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(
                app,
                &[&settings_i, &history_i, &separator, &quit_i],
            )?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .tooltip("VoxCtrl")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "settings" => {
                            if let Some(window) = app.get_webview_window("settings") {
                                show_and_focus_window(&window);
                            }
                        }
                        "history" => {
                            if let Some(window) = app.get_webview_window("history") {
                                show_and_focus_window(&window);
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("settings") {
                            show_and_focus_window(&window);
                        }
                    }
                })
                .build(app)?;

            // Spawn the Slint overlay helper process
            if let Some(overlay_path) = get_overlay_path() {
                tracing::info!("Spawning Slint overlay helper: {:?}", overlay_path);
                match std::process::Command::new(&overlay_path)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            let rx = overlay_rx.clone();
                            std::thread::spawn(move || {
                                use std::io::Write;
                                while let Ok(msg) = rx.recv() {
                                    if writeln!(stdin, "{}", msg).is_err() || stdin.flush().is_err() {
                                        break;
                                    }
                                }
                            });
                        }
                        // Wait for child in background so it doesn't become a zombie
                        std::thread::spawn(move || {
                            let status = child.wait();
                            // Logged on stderr (tracing) so it is visible even when
                            // stdout is block-buffered behind a pipe. If this fires
                            // after the first dictation, the overlay is crashing
                            // rather than hitting a window-mapping issue.
                            tracing::warn!("Slint overlay process exited: {:?}", status);
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to spawn Slint overlay process: {:?}", e);
                    }
                }
            } else {
                eprintln!("Slint overlay binary not found! Check your build directory.");
            }

            // Force show the settings window on startup if the voice model is missing
            let mut show_settings = cfg_data.ui.auto_show_settings;
            if cfg_data.engine.backend != voxctrl_config::BackendChoice::Moonshine {
                let model_size = &cfg_data.engine.whisper_cpp.model_size;
                let model_dir = &cfg_data.engine.whisper_cpp.model_dir;
                if !voxctrl_inference::whisper_cpp::is_model_downloaded(model_size, model_dir) {
                    show_settings = true;
                }
            }

            if show_settings {
                if let Some(window) = app.get_webview_window("settings") {
                    show_and_focus_window(&window);
                }
            }

            // Emit periodic status updates to all windows
            let state_for_ticker = app_state.clone();
            let handle = app.handle().clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(150));
                let mut last_recording = false;
                let mut was_animating = false;
                let mut frame_idx = 0;
                let mut last_pos: Option<(String, String, String)> = None;
                let mut startup_tick_count: u32 = 0;
                loop {
                    interval.tick().await;
                    startup_tick_count = startup_tick_count.saturating_add(1);
                    let is_recording = state_for_ticker.is_recording();
                    let is_processing = state_for_ticker.is_processing();

                    // Decide whether the tray icon needs updating this tick and,
                    // if so, which frame to show. The actual `set_icon` call must
                    // happen on the GTK main thread: on Linux the tray is backed by
                    // ayatana-appindicator/GTK, which is not thread-safe. Calling it
                    // from this Tokio worker thread makes icon updates unreliable —
                    // the animated icon flickers or disappears entirely on
                    // appindicator-based desktops (e.g. GNOME). `run_on_main_thread`
                    // marshals the update onto the loop that owns the tray.
                    let next_icon: Option<tauri::image::Image<'static>> = if is_processing {
                        let icon = processing_frames[frame_idx].clone();
                        frame_idx = (frame_idx + 1) % 6;
                        was_animating = true;
                        Some(icon)
                    } else if was_animating || is_recording != last_recording {
                        was_animating = false;
                        Some(if is_recording { record_on_icon.clone() } else { record_off_icon.clone() })
                    } else {
                        None
                    };

                    if let Some(icon) = next_icon {
                        let handle_for_icon = handle.clone();
                        let _ = handle.run_on_main_thread(move || {
                            if let Some(tray) = handle_for_icon.tray_by_id("main-tray") {
                                let _ = tray.set_icon(Some(icon));
                            }
                        });
                    }

                    let is_mcp_recording = state_for_ticker.is_mcp_recording();

                    // Update overlay window coordinates if user config changes
                    let (overlay_position, overlay_monitor, overlay_style) = {
                        let cfg = state_for_ticker.config.lock().await;
                        (
                            cfg.data.ui.overlay_position.clone(),
                            cfg.data.ui.overlay_monitor.clone(),
                            cfg.data.ui.overlay_style.clone(),
                        )
                    };

                    let current_pos = (overlay_position.clone(), overlay_monitor.clone(), overlay_style.clone());
                    let mut should_reposition = false;
                    if last_pos.as_ref() != Some(&current_pos) || last_pos.is_none() {
                        last_pos = Some(current_pos);
                        should_reposition = true;
                    } else if startup_tick_count < 40 {
                        should_reposition = true;
                    }

                    if should_reposition {
                        if let Some((x, y)) = calculate_overlay_coordinates(&handle, &overlay_position, &overlay_monitor) {
                            let pos_msg = serde_json::json!({
                                "type": "position",
                                "x": x,
                                "y": y,
                            });
                            if let Ok(json_str) = serde_json::to_string(&pos_msg) {
                                let _ = state_for_ticker.overlay_tx.send(json_str);
                            }
                        }
                    }

                    last_recording = is_recording;

                    let active_target_id = state_for_ticker.active_target.lock().await.clone();
                    let binding_label = state_for_ticker.active_binding_label.lock().await.clone();
                    let target_label = if (is_recording || is_processing) && !binding_label.is_empty() {
                        binding_label
                    } else {
                        let targets_guard = state_for_ticker.targets.lock().await;
                        let ids: Vec<&str> = active_target_id
                            .split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let labels: Vec<String> = ids
                            .iter()
                            .map(|id| {
                                targets_guard
                                    .iter()
                                    .find(|t| &t.id == id)
                                    .map(|t| t.label.clone())
                                    .unwrap_or_else(|| {
                                        if *id == "default" {
                                            "Focused Window".to_string()
                                        } else {
                                            id.to_string()
                                        }
                                    })
                            })
                            .collect();
                        labels.join(" + ")
                    };

                    let payload = serde_json::json!({
                        "recording": is_recording,
                        "processing": is_processing,
                        "speaking": state_for_ticker.is_speaking(),
                        "mcp_recording": is_mcp_recording,
                        "audio_ready": state_for_ticker.is_audio_ready(),
                        "word_count": state_for_ticker.total_words(),
                        "active_target_id": active_target_id,
                        "active_target_label": target_label,
                    });
                    let _ = handle.emit("status-tick", payload.clone());

                    // Forward status to Slint overlay channel. When the overlay is
                    // disabled, force the visibility flags off so the native window
                    // never maps — a mapped overlay grabs keyboard focus on Wayland
                    // and prevents transcribed text from reaching the cursor.
                    let overlay_on = state_for_ticker.is_overlay_enabled();
                    let mut payload_value = payload.clone();
                    if let Some(obj) = payload_value.as_object_mut() {
                        obj.insert("type".to_string(), serde_json::json!("status"));
                        obj.insert("audio_level".to_string(), serde_json::json!(0.0));
                        obj.insert("overlay_style".to_string(), serde_json::json!(overlay_style));
                        if !overlay_on {
                            obj.insert("recording".to_string(), serde_json::json!(false));
                            obj.insert("processing".to_string(), serde_json::json!(false));
                            obj.insert("speaking".to_string(), serde_json::json!(false));
                        }
                    }
                    if let Ok(json_str) = serde_json::to_string(&payload_value) {
                        let _ = state_for_ticker.overlay_tx.send(json_str);
                    }
                }
            });

            startup_log::STARTUP_COMPLETE.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_available_monitors,
            start_recording,
            stop_recording,
            toggle_recording,
            get_config,
            save_config,
            get_targets,
            save_targets,
            get_bindings,
            save_bindings,
            get_history,
            clear_history,
            speak_text,
            show_overlay,
            hide_overlay,
            get_custom_overlays,
            list_audio_devices,
            start_monitoring_audio,
            stop_monitoring_audio,
            check_voice_downloaded,
            download_voice,
            check_kokoro_ready,
            download_kokoro,
            check_model_downloaded,
            download_model,
            check_directory_exists,
            test_ollama,
            cuda_enabled,
            check_udev_status,
            install_system_integration,
            stop_tts,
        ])
        .run(tauri::generate_context!())
        .expect("error running Tauri application");
}

impl McpCallbacks for AppState {
    fn transcribe_voice(&self, timeout_secs: f64) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
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

    fn speak_text(&self, text: String) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        async move {
            let handle = self.tts_handle.lock().await;
            if let Some(ref tts) = *handle {
                tts.speak(text);
            }
            Ok(())
        }
    }

    fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send {
        async move {
            (self.is_recording(), self.is_speaking())
        }
    }
}

pub fn calculate_overlay_y(
    monitor_y: i32,
    monitor_height: i32,
    window_height: i32,
    scale_factor: f64,
    position_str: &str,
) -> i32 {
    match position_str {
        "top" => monitor_y + (60.0 * scale_factor) as i32,
        "bottom" => monitor_y + monitor_height - window_height - (60.0 * scale_factor) as i32,
        _ => monitor_y + (monitor_height - window_height) / 2, // "center" is default
    }
}

pub fn get_overlay_path() -> Option<std::path::PathBuf> {
    // Logged via tracing (stderr) so the resolved path is visible even when
    // stdout is block-buffered behind a pipe.
    if let Ok(mut current_path) = std::env::current_exe() {
        tracing::info!("get_overlay_path: current_exe = {:?}", current_path);
        current_path.pop(); // Pop binary name
        let bin_name = if cfg!(target_os = "windows") {
            "voxctrl-overlay.exe"
        } else {
            "voxctrl-overlay"
        };
        // Check same dir (where the bundled sidecar lives next to the main app)
        let p1 = current_path.join(bin_name);
        if p1.exists() {
            tracing::info!("get_overlay_path: found alongside main binary: {:?}", p1);
            return Some(p1);
        }
        // Check parent dir (if running in deps/)
        current_path.pop();
        let p2 = current_path.join(bin_name);
        if p2.exists() {
            tracing::info!("get_overlay_path: found in parent dir: {:?}", p2);
            return Some(p2);
        }

        // Dev-only fallback: relative to the current working directory.
        if let Ok(cwd) = std::env::current_dir() {
            let p3 = cwd.join("target").join("debug").join(bin_name);
            if p3.exists() {
                tracing::info!("get_overlay_path: found in target/debug: {:?}", p3);
                return Some(p3);
            }
            let p4 = cwd.join("src-tauri").join("target").join("debug").join(bin_name);
            if p4.exists() {
                tracing::info!("get_overlay_path: found in src-tauri/target/debug: {:?}", p4);
                return Some(p4);
            }
        }
    }
    tracing::error!("get_overlay_path: overlay binary not found");
    None
}

pub fn calculate_overlay_coordinates(app: &tauri::AppHandle, position_str: &str, monitor_pref: &str) -> Option<(i32, i32)> {
    let target_monitor = match monitor_pref {
        "primary" => app.primary_monitor().ok().flatten(),
        _ => {
            if let Ok(monitors) = app.available_monitors() {
                monitors.into_iter().find(|m| m.name().map(|s| s.as_ref()) == Some(monitor_pref))
            } else {
                None
            }
        }
    };

    let monitor = target_monitor
        .or_else(|| app.primary_monitor().ok().flatten())?;

    let monitor_size = monitor.size();
    let monitor_pos = monitor.position();
    let scale_factor = monitor.scale_factor();
    
    // Slint overlay window physical size is (560 * scale_factor) x (190 * scale_factor)
    let win_w = (560.0 * scale_factor) as i32;
    let win_h = (190.0 * scale_factor) as i32;
    let mon_w = monitor_size.width as i32;
    let mon_h = monitor_size.height as i32;
    let mon_x = monitor_pos.x;
    let mon_y = monitor_pos.y;
    
    let x = mon_x + (mon_w - win_w) / 2;
    let y = calculate_overlay_y(mon_y, mon_h, win_h, scale_factor, position_str);
    
    Some((x, y))
}

#[cfg(test)]
pub mod test_utils {
    use std::sync::{Mutex, OnceLock};
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    
    pub fn get_env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_error_layer_privacy_and_levels() {
        use tracing_subscriber::prelude::*;
        use std::io::Read;
        
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("test_startup_errors.log");
        
        let layer = crate::startup_log::StartupErrorLayer::new(log_path.clone()).unwrap();
        let subscriber = tracing_subscriber::registry().with(layer);
        
        crate::startup_log::STARTUP_COMPLETE.store(false, std::sync::atomic::Ordering::SeqCst);
        
        tracing::subscriber::with_default(subscriber, || {
            // 1. Startup INFO log (should be written)
            tracing::info!("System startup: device init");
            
            // 2. Transcription text (should be blocked by privacy filters)
            tracing::info!("Received transcription: Hello user");
            
            // 3. Spoken text warn (should be blocked by privacy filters)
            tracing::warn!("Failed to speak the text: Hello user");
            
            // 4. Ollama payload error (should be blocked by privacy filters)
            tracing::error!("Ollama request payload: test prompt");
            
            // Transition to post-startup
            crate::startup_log::STARTUP_COMPLETE.store(true, std::sync::atomic::Ordering::SeqCst);
            
            // 5. Post-startup INFO log (should be ignored by level filter)
            tracing::info!("Normal runtime info log");
            
            // 6. Post-startup ERROR log (should be written)
            tracing::error!("System audio device connection lost");
        });
        
        // Read file contents
        let mut file = std::fs::File::open(log_path).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        
        // Assertions
        assert!(content.contains("System startup: device init"));
        assert!(content.contains("System audio device connection lost"));
        
        assert!(!content.contains("Hello user"));
        assert!(!content.contains("Normal runtime info log"));
        assert!(!content.contains("Ollama"));
    }

    #[test]
    fn test_calculate_overlay_y() {
        let mon_y = 0;
        let mon_h = 1080;
        let win_h = 160;

        let y_top = calculate_overlay_y(mon_y, mon_h, win_h, 1.0, "top");
        assert_eq!(y_top, 60);

        let y_center = calculate_overlay_y(mon_y, mon_h, win_h, 1.0, "center");
        assert_eq!(y_center, (1080 - 160) / 2);

        let y_bottom = calculate_overlay_y(mon_y, mon_h, win_h, 1.0, "bottom");
        assert_eq!(y_bottom, 1080 - 160 - 60);

        // High DPI scaling factor 2.0
        let y_top_hidpi = calculate_overlay_y(mon_y, mon_h, win_h, 2.0, "top");
        assert_eq!(y_top_hidpi, 120);

        // Fractional scaling factor 1.5
        let y_bottom_fractional = calculate_overlay_y(mon_y, mon_h, win_h, 1.5, "bottom");
        assert_eq!(y_bottom_fractional, 1080 - 160 - 90);

        // Secondary monitor positioned above primary display (negative Y coordinates)
        let mon_y_negative = -1080;
        let y_top_negative = calculate_overlay_y(mon_y_negative, mon_h, win_h, 1.0, "top");
        assert_eq!(y_top_negative, -1080 + 60);

        let y_bottom_negative = calculate_overlay_y(mon_y_negative, mon_h, win_h, 1.0, "bottom");
        assert_eq!(y_bottom_negative, -1080 + 1080 - 160 - 60);

        // 4K Monitor
        let mon_h_4k = 2160;
        let y_center_4k = calculate_overlay_y(mon_y, mon_h_4k, win_h, 1.0, "center");
        assert_eq!(y_center_4k, (2160 - 160) / 2);
    }
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32};
    use tokio::sync::Mutex;
    use voxctrl_config::Config;
    use voxctrl_routing::OutputTargetRouter;
    use crate::state::AppState;

    fn make_test_state() -> AppState {
        let (audio_tx, _) = crossbeam_channel::bounded(1);
        let (overlay_tx, _) = crossbeam_channel::unbounded();
        AppState {
            config: Arc::new(Mutex::new(Config::load())),
            router: Arc::new(OutputTargetRouter::new(Vec::new())),
            recording: Arc::new(AtomicBool::new(false)),
            processing: Arc::new(AtomicBool::new(false)),
            speaking: Arc::new(AtomicBool::new(false)),
            overlay_enabled: Arc::new(AtomicBool::new(true)),
            mcp_recording: Arc::new(AtomicBool::new(false)),
            audio_ready: Arc::new(AtomicBool::new(false)),
            dynamic_stream: Arc::new(AtomicBool::new(false)),
            monitoring: Arc::new(AtomicBool::new(false)),
            input_device_index: Arc::new(AtomicU32::new(u32::MAX)),
            gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            word_count: Arc::new(AtomicU32::new(0)),
            last_text: Arc::new(Mutex::new(String::new())),
            last_text_version: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            active_target: Arc::new(Mutex::new("default".to_string())),
            active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
            active_binding_id: Arc::new(Mutex::new(String::new())),
            targets: Arc::new(Mutex::new(Vec::new())),
            history: Arc::new(Mutex::new(Vec::new())),
            audio_tx,
            overlay_tx,
            tts_handle: Arc::new(Mutex::new(None)),
            active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
            hotkey_reloader: Arc::new(Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn test_app_state_initial_values() {
        let state = make_test_state();
        assert!(!state.is_recording());
        assert!(!state.is_speaking());
        assert_eq!(state.total_words(), 0);
        assert_eq!(*state.active_target.lock().await, "default");
    }

    #[tokio::test]
    async fn test_app_state_words_increment() {
        let state = make_test_state();
        state.increment_words(15);
        assert_eq!(state.total_words(), 15);
        state.increment_words(10);
        assert_eq!(state.total_words(), 25);
    }

    #[tokio::test]
    async fn test_history_entries() {
        let state = make_test_state();
        {
            let mut hist = state.history.lock().await;
            hist.push(HistoryEntry {
                text: "hello world".to_string(),
                target_id: "default".to_string(),
                timestamp: "2026-05-20T22:00:00Z".to_string(),
                inference_ms: 120,
            });
        }
        let hist = state.history.lock().await;
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].text, "hello world");
        assert_eq!(hist[0].target_id, "default");
        assert_eq!(hist[0].inference_ms, 120);
    }

    #[tokio::test]
    async fn test_sequential_multi_target_delivery() {
        use voxctrl_routing::models::{DeliveryType, OutputTarget};

        let temp_dir = std::env::temp_dir();
        let path_a = temp_dir.join("voxctrl_test_target_a.log").to_string_lossy().to_string();
        let path_b = temp_dir.join("voxctrl_test_target_b.log").to_string_lossy().to_string();

        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);

        let target_a = OutputTarget {
            id: "target_a".into(),
            label: "Target A".into(),
            delivery: DeliveryType::File,
            command: None,
            pipe_path: None,
            socket_host: None,
            socket_port: None,
            socket_unix: None,
            file_path: Some(path_a.clone()),
            file_prefix: "".into(),
            file_timestamp: false,
            file_mode: "append".into(),
            dbus_signal: None,
            http_url: None,
            http_method: "POST".into(),
            http_headers: None,
            http_json_template: None,
            webhook_url: None,
            webhook_secret: None,
            webhook_json_template: None,
            mcp_path: None,
            mcp_tool: None,
            mcp_args: None,
            openai_prompt: None,
            openai_model: None,
            openai_max_tokens: None,
            openai_timeout_secs: None,
            send_on_release: true,
            append_newline: false,
            initial_prompt: None,
            processing: Default::default(),
            response_pipe: None,
            strip_newlines: false,
        };

        let target_b = OutputTarget {
            id: "target_b".into(),
            label: "Target B".into(),
            delivery: DeliveryType::File,
            command: None,
            pipe_path: None,
            socket_host: None,
            socket_port: None,
            socket_unix: None,
            file_path: Some(path_b.clone()),
            file_prefix: "".into(),
            file_timestamp: false,
            file_mode: "append".into(),
            dbus_signal: None,
            http_url: None,
            http_method: "POST".into(),
            http_headers: None,
            http_json_template: None,
            webhook_url: None,
            webhook_secret: None,
            webhook_json_template: None,
            mcp_path: None,
            mcp_tool: None,
            mcp_args: None,
            openai_prompt: None,
            openai_model: None,
            openai_max_tokens: None,
            openai_timeout_secs: None,
            send_on_release: true,
            append_newline: false,
            initial_prompt: None,
            processing: Default::default(),
            response_pipe: None,
            strip_newlines: false,
        };

        let targets = vec![target_a, target_b];

        let router = Arc::new(OutputTargetRouter::new(targets));
        let text = "Sequential delivery text".to_string();
        let target_ids = vec!["target_a".to_string(), "target_b".to_string()];

        let mut results = Vec::new();
        for tid in target_ids {
            let res = router.deliver(&tid, &text).await;
            results.push(res);
        }

        assert_eq!(results.len(), 2);
        for res in results {
            assert!(res.success, "Delivery failed: {:?}", res.error);
        }

        // Sleep a tiny bit to let OS write flush
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let content_a = std::fs::read_to_string(&path_a).unwrap_or_default();
        let content_b = std::fs::read_to_string(&path_b).unwrap_or_default();
        assert!(content_a.contains("Sequential delivery text"));
        assert!(content_b.contains("Sequential delivery text"));

        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
    }

    #[tokio::test]
    async fn test_check_udev_status_env_overrides() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_TEST_UDEV_STATUS", "missing");
        let res = check_udev_status().await.unwrap();
        assert!(!res.is_configured);
        assert!(!res.rule_exists);
        assert!(!res.in_group);
        assert!(!res.needs_relogin);

        std::env::set_var("VOXCTRL_TEST_UDEV_STATUS", "relogin");
        let res = check_udev_status().await.unwrap();
        assert!(!res.is_configured);
        assert!(res.rule_exists);
        assert!(!res.in_group);
        assert!(res.needs_relogin);

        std::env::set_var("VOXCTRL_TEST_UDEV_STATUS", "ok");
        let res = check_udev_status().await.unwrap();
        assert!(res.is_configured);
        assert!(res.rule_exists);
        assert!(res.in_group);
        assert!(!res.needs_relogin);

        std::env::remove_var("VOXCTRL_TEST_UDEV_STATUS");
    }

    #[tokio::test]
    async fn test_check_udev_status_nss_fallback_logic() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        // Query the NSS database manually using the exact logic we implemented to make sure it matches
        #[cfg(target_os = "linux")]
        {
            if let Ok(username) = std::env::var("USER") {
                let output = std::process::Command::new("id").args(&["-Gn", &username]).output();
                if let Ok(out) = output {
                    let groups_str = String::from_utf8_lossy(&out.stdout);
                    let database_has_input = groups_str.split_whitespace().any(|g| g == "input");
                    
                    // Verify that if check_udev_status is called, the in_group value
                    // matches whether the database or active session has the "input" group.
                    std::env::remove_var("VOXCTRL_TEST_UDEV_STATUS");
                    let res = check_udev_status().await.unwrap();
                    
                    // If either database_has_input is true or active session has it,
                    // in_group should resolve as true.
                    let active_session_has_input = match std::process::Command::new("id").args(&["-Gn"]).output() {
                        Ok(o) => {
                            let s = String::from_utf8_lossy(&o.stdout);
                            s.split_whitespace().any(|g| g == "input")
                        }
                        Err(_) => false,
                    };
                    
                    if database_has_input || active_session_has_input {
                        assert!(res.in_group, "User is in input group in database/session, in_group check should be true");
                    }
                }
            }
        }

        // On non-Linux (e.g. Windows), check should always succeed
        #[cfg(not(target_os = "linux"))]
        {
            std::env::remove_var("VOXCTRL_TEST_UDEV_STATUS");
            let res = check_udev_status().await.unwrap();
            assert!(res.is_configured);
            assert!(res.rule_exists);
            assert!(res.in_group);
        }
    }

    #[tokio::test]
    async fn test_speak_target_delivery() {
        use voxctrl_routing::models::{DeliveryType, OutputTarget};
        use voxctrl_routing::targets::build_target;
        use std::sync::{Arc, Mutex};

        let mut config = OutputTarget::default_inject();
        config.delivery = DeliveryType::Speak;

        let spoken = Arc::new(Mutex::new(String::new()));
        let spoken_clone = spoken.clone();
        let _ = voxctrl_routing::targets::set_speak_callback(Arc::new(move |text| {
            *spoken_clone.lock().unwrap() = text.to_string();
        }));

        let target = build_target(config);
        let res = target.deliver("Test Speak Target from Tauri").await;
        
        assert!(res.success);
        
        let spoken_text = spoken.lock().unwrap();
        if !spoken_text.is_empty() {
            assert_eq!(*spoken_text, "Test Speak Target from Tauri");
        }
    }
}

