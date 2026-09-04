// VoxCtrl Tauri Application Core
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};
use tauri::Manager;
use tokio::sync::Mutex;
use voxctrl_config::Config;
use voxctrl_routing::{config_dir, load_bindings, load_targets, OutputTargetRouter};

use crate::commands::*;
use crate::state::AppState;

mod commands;
mod installer;
mod host_env;
mod mint_shortcuts;
mod overlay_sidecar;
mod pipeline;
mod services;
mod startup_log;
mod state;
mod tray;
mod window;

#[cfg(test)]
mod tests;

pub use overlay_sidecar::get_overlay_path;
pub use window::{
    get_app_handle, set_app_handle, setup_blocker, show_and_focus_window, show_setup_window,
    SETUP_WINDOW,
};

pub fn run_cli_installer() -> Result<(), String> {
    crate::installer::run_cli_installer()
}

#[cfg(test)]
pub mod test_utils {
    use std::sync::{Mutex, OnceLock};
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub fn get_env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}

// ── Tauri app entry point ─────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Whether these command-line arguments ask for the setup wizard.
///
/// Accepted in a few spellings because this is the flag someone reaches for
/// when the app is already configured and they want to see setup again; making
/// them guess the exact word would defeat the point.
pub fn wants_setup_wizard(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--setup" | "--wizard" | "--setup-wizard" | "--first-run"
        )
    })
}

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
                    log_func: Option<
                        unsafe extern "C" fn(
                            *const std::os::raw::c_char,
                            std::os::raw::c_int,
                            *const std::os::raw::c_char,
                            *mut std::os::raw::c_void,
                        ),
                    >,
                    user_data: *mut std::os::raw::c_void,
                ) -> std::os::raw::c_uint;
            }

            unsafe extern "C" fn dummy_log_handler(
                _log_domain: *const std::os::raw::c_char,
                _log_levels: std::os::raw::c_int,
                _message: *const std::os::raw::c_char,
                _user_data: *mut std::os::raw::c_void,
            ) {
            }

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
    tracing::info!("Whisper model size: {}", config.data.engine.whisper_cpp.model_size);
    tracing::info!("Whisper device: {}", config.data.engine.whisper_cpp.device);
    tracing::info!("Whisper threads: {}", config.data.engine.whisper_cpp.threads);
    tracing::info!("Moonshine model size: {}", config.data.engine.moonshine.model_size);
    tracing::info!("Moonshine language: {}", config.data.engine.moonshine.language);
    tracing::info!("VAD threshold: {}", config.data.audio.vad_threshold);
    tracing::info!("Noise suppression: {}", config.data.audio.noise_suppression);
    tracing::info!("Input device index: {:?}", config.data.audio.input_device_index);
    tracing::info!("Gain: {}", config.data.audio.gain);
    tracing::info!("Dynamic stream: {}", config.data.audio.dynamic_stream);
    tracing::info!("TTS enabled: {}", config.data.tts.enabled);
    tracing::info!("TTS engine: {:?}", config.data.tts.engine);
    tracing::info!("TTS voice: {}", config.data.tts.voice);
    tracing::info!("TTS speed: {}", config.data.tts.speed);
    tracing::info!("TTS GPU: {}", config.data.tts.gpu);
    tracing::info!("Pocket-TTS voice: {}", config.data.tts.pocket_tts.voice);
    tracing::info!("Pocket-TTS prewarm: {}", config.data.tts.pocket_tts.prewarm);
    tracing::info!("Pocket-TTS HF token set: {}", config.data.tts.pocket_tts.hf_token.is_some());
    tracing::info!("MCP enabled: {}", config.data.mcp.server_enabled);
    tracing::info!("MCP record timeout: {}", config.data.mcp.record_timeout);
    tracing::info!("=============================");

    let cfg_data = Arc::new(config.data.clone());
    let config = Arc::new(Mutex::new(config));

    let cdir = config_dir();
    let targets = load_targets(&cdir).unwrap_or_default();
    let bindings = load_bindings(&cdir).unwrap_or_default();

    let router = Arc::new(OutputTargetRouter::new(targets.clone()));

    // ── Audio & Inference pipelines ──────────────────────────────────────────
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<voxctrl_audio::AudioChunk>(64);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<voxctrl_inference::InferenceOutput>(32);
    let (inference_tx, inference_rx) =
        crossbeam_channel::bounded::<voxctrl_inference::InferenceRequest>(4);
    let (overlay_tx, overlay_rx) = crossbeam_channel::unbounded::<String>();

    let hotkey_health = Arc::new(voxctrl_hotkeys::ListenerHealth::default());

    let app_state = Arc::new(AppState {
        config: config.clone(),
        router: router.clone(),
        recording: Arc::new(AtomicBool::new(false)),
        processing: Arc::new(AtomicBool::new(false)),
        speaking: Arc::new(AtomicBool::new(false)),
        overlay_enabled: Arc::new(AtomicBool::new(cfg_data.ui.show_overlay)),
        mcp_recording: Arc::new(AtomicBool::new(false)),
        hotkeys_inhibited: Arc::new(AtomicBool::new(false)),
        audio_ready: Arc::new(AtomicBool::new(false)),
        dynamic_stream: Arc::new(AtomicBool::new(cfg_data.audio.dynamic_stream)),
        monitoring: Arc::new(AtomicBool::new(false)),
        input_device_index: Arc::new(AtomicU32::new(
            cfg_data.audio.input_device_index.unwrap_or(u32::MAX),
        )),
        gain: Arc::new(AtomicU32::new(cfg_data.audio.gain.to_bits())),
        noise_suppression: Arc::new(AtomicBool::new(cfg_data.audio.noise_suppression)),
        word_count: Arc::new(AtomicU32::new(0)),
        last_text: Arc::new(Mutex::new(String::new())),
        last_text_version: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        active_target: Arc::new(Mutex::new("default".to_string())),
        active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
        active_binding_id: Arc::new(Mutex::new(String::new())),
        targets: Arc::new(Mutex::new(targets.clone())),
        audio_tx: audio_tx.clone(),
        tts_handle: Arc::new(Mutex::new(None)),
        active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        hotkey_reloader: Arc::new(Mutex::new(None)),
        hotkey_gesture_tx: Arc::new(Mutex::new(None)),
        hotkey_health: hotkey_health.clone(),
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
            app_state.noise_suppression.clone(),
        );
        let _ = recorder.run(
            audio_tx,
            Some(audio_level_tx),
            Some(app_state.audio_ready.clone()),
        );
    }

    // Audio chunk coordinator thread
    pipeline::spawn_audio_coordinator(app_state.clone(), audio_rx, inference_tx);

    // Inference worker
    voxctrl_inference::run_worker(cfg_data.clone(), inference_rx, text_tx.clone());

    // TTS initial worker
    let _tts_handle = if cfg_data.tts.enabled {
        Some(voxctrl_tts::TtsEngineWorker::start(
            cfg_data.tts.clone(),
            cfg_data.features.custom_vocabulary.clone(),
            None,
            None,
            None,
        ))
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

    // Setup desktop integration (launcher and icon) before initializing hotkey listeners,
    // so xdg-desktop-portal can resolve the `ai.voxctrl.app` AppID against an installed .desktop file.
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = crate::installer::setup_desktop_integration() {
            tracing::warn!("Failed to setup desktop integration at startup: {e}");
        }
    }

    // Hotkey listener & bindings
    let mut all_bindings = bindings;
    if !cfg_data.tts.stop_key.is_empty() {
        use voxctrl_routing::{GestureType, HotkeyBinding};
        all_bindings.push(HotkeyBinding {
            id: "__tts_stop__".to_string(),
            label: "TTS Stop Key".to_string(),
            keys: cfg_data.tts.stop_key.clone(),
            gesture: GestureType::Hold,
            target_id: String::new(),
            target_ids: vec![],
            tap_ms: 250,
            hold_threshold_ms: 0,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        });
    }

    let (gesture_tx, gesture_rx) = voxctrl_hotkeys::channel();
    let listener = voxctrl_hotkeys::start_listener(
        all_bindings,
        gesture_tx.clone(),
        cfg_data.audio.evdev_device.clone(),
        hotkey_health.clone(),
    );

    let state_for_gesture = app_state.clone();
    let gesture_tx_for_state = gesture_tx.clone();
    tokio::spawn(async move {
        let mut reloader = state_for_gesture.hotkey_reloader.lock().await;
        *reloader = Some(listener.reloader_tx);
        let mut gtx = state_for_gesture.hotkey_gesture_tx.lock().await;
        *gtx = Some(gesture_tx_for_state);
    });

    pipeline::spawn_hotkey_gesture_handler(app_state.clone(), gesture_rx);

    // Text delivery worker
    let rt_handle = tokio::runtime::Handle::current();
    pipeline::spawn_text_delivery_worker(app_state.clone(), text_rx, rt_handle);

    // DBus service
    #[cfg(target_os = "linux")]
    services::start_dbus_service(app_state.clone());

    // MCP Server
    if cfg_data.mcp.server_enabled {
        services::start_mcp_server(app_state.clone());
    }

    // ── Build Tauri app ───────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            tracing::info!("Single instance trigger: argv={:?}, cwd={:?}", argv, cwd);
            // `voxctrl --setup` while the app is already running is a request
            // to see the wizard, not to raise Settings. Without this the flag
            // would appear to do nothing at all on the second launch, which is
            // the launch a user testing it is most likely to make.
            if wants_setup_wizard(&argv) {
                if let Err(e) = crate::window::open_wizard_window(app) {
                    tracing::error!("Could not open the setup wizard: {e}");
                }
                return;
            }
            if let Err(e) = crate::window::open_settings_window(app) {
                tracing::error!("Could not open Settings: {e}");
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .setup(move |app| {
            set_app_handle(app.handle().clone());

            // Set window icon programmatically on Linux/Wayland
            #[cfg(target_os = "linux")]
            {
                let _ = crate::installer::setup_desktop_integration();
                let icon_bytes = include_bytes!("../icons/128x128.png");
                if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
                    for window in app.webview_windows().values() {
                        let _ = window.set_icon(icon.clone());
                    }
                }
            }

            // Re-initialize TTS worker with event emitter callbacks
            services::setup_tts_and_fifos(&app.handle(), app_state.clone());

            // Register Speak target callback
            services::register_speak_target(&app.handle());

            // Register Command trigger target callback
            services::register_command_trigger_target(&app.handle());

            // Setup watcher for hotkey permissions
            #[cfg(target_os = "linux")]
            pipeline::spawn_setup_watcher(app.handle().clone(), app_state.hotkey_health.clone());

            // Forward audio levels to settings window and Slint overlay
            pipeline::spawn_audio_level_forwarder(
                app.handle().clone(),
                app_state.clone(),
                audio_level_rx,
            );

            // Setup system tray
            let _tray = tray::create_tray(app)?;

            let record_on_icon =
                tauri::image::Image::from_bytes(include_bytes!("../../assets/record_on.png"))
                    .expect("Failed to load record_on icon");
            let record_off_icon =
                tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
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

            // Spawn the Slint overlay helper process
            overlay_sidecar::spawn_overlay_process(overlay_rx);

            // Auto download speech model if needed. Skipped entirely when the
            // wizard was asked for: it is about to ask which model the user
            // wants, and fetching a different one behind its back would waste
            // the download and confuse the step.
            let forced_wizard = wants_setup_wizard(&std::env::args().collect::<Vec<_>>());
            if forced_wizard {
                if let Err(e) = crate::window::open_wizard_window(&app.handle().clone()) {
                    tracing::error!("Could not open the setup wizard: {e}");
                }
            } else {
                services::auto_download_speech_model_if_needed(app, &cfg_data);
            }

            // Emit periodic status updates to all windows and animate tray
            tray::spawn_status_ticker(
                app.handle().clone(),
                app_state.clone(),
                record_on_icon,
                record_off_icon,
                processing_frames,
            );

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
            speak_text,
            show_overlay,
            hide_overlay,
            get_custom_overlays,
            list_audio_devices,
            start_monitoring_audio,
            stop_monitoring_audio,
            check_voice_downloaded,
            download_voice,
            check_breeze_tts_2_ready,
            download_breeze_tts_2,
            check_pocket_tts_ready,
            download_pocket_tts,
            list_pocket_tts_voices,
            inflect_micro_available,
            check_inflect_micro_downloaded,
            download_inflect_micro,
            inflect_micro_inspect,
            check_model_downloaded,
            download_model,
            moonshine_available,
            check_moonshine_downloaded,
            download_moonshine_model,
            check_directory_exists,
            test_openai,
            cuda_enabled,
            check_hotkey_status,
            check_hotkey_keys,
            retry_portal_shortcuts,
            open_shortcut_settings,
            register_mint_shortcut,
            approve_shortcuts,
            install_system_integration,
            get_setup_status,
            finish_setup_wizard,
            open_setup_wizard,
            download_configured_model,
            open_settings_tab,
            stop_tts,
            reset_chat_conversation,
            test_chat_target,
            set_hotkeys_inhibited,
        ])
        .build(tauri::generate_context!())
        .expect("error building Tauri application")
        .run(|_app, event| {
            // Windows are ordinary windows: the close button closes them, and
            // every entry point rebuilds one when it is gone. That makes the
            // last window closing look to Tauri like the app should exit, which
            // for a tray app it must not — dictation carries on with nothing on
            // screen. An explicit quit carries an exit code, so the tray's Quit
            // item still works.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
