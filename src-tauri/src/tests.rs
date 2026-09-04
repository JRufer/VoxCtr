use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};
use tokio::sync::Mutex;
use voxctrl_config::Config;
use voxctrl_routing::OutputTargetRouter;
use crate::state::AppState;
use crate::window::setup_blocker;

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
        
        // 4. OpenAI payload error (should be blocked by privacy filters)
        tracing::error!("OpenAI request payload: test prompt");
        
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
    assert!(!content.contains("OpenAI"));
}

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
        hotkeys_inhibited: Arc::new(AtomicBool::new(false)),
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
        audio_tx,
        overlay_tx,
        tts_handle: Arc::new(Mutex::new(None)),
        active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        hotkey_reloader: Arc::new(Mutex::new(None)),
        hotkey_gesture_tx: Arc::new(Mutex::new(None)),
        hotkey_health: Arc::new(voxctrl_hotkeys::ListenerHealth::default()),
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
        file_timestamp_format: voxctrl_routing::default_file_timestamp_format(),
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
        chat_url: None,
        chat_model: None,
        chat_api_key: None,
        chat_system_prompt: None,
        chat_max_history: 20,
        chat_timeout_secs: 120,
        chat_reply_mode: "speak".into(),
        chat_reset_phrase: None,
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
        file_timestamp_format: voxctrl_routing::default_file_timestamp_format(),
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
        chat_url: None,
        chat_model: None,
        chat_api_key: None,
        chat_system_prompt: None,
        chat_max_history: 20,
        chat_timeout_secs: 120,
        chat_reply_mode: "speak".into(),
        chat_reset_phrase: None,
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

#[test]
fn bare_modifier_shortcuts_are_refused_on_the_portal() {
    // The case the settings recorder has to catch. A lone Super reads as a
    // perfectly good hotkey to a user and no desktop can bind it, so the
    // rejection has to name the rule and say what to press instead.
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let check = crate::commands::check_hotkey_keys_with(
        &["KEY_LEFTMETA".to_string()],
        &health,
    );
    assert!(!check.accepted);
    assert!(check.enforced);
    assert_eq!(check.problem.as_deref(), Some("modifiers_only"));
    let message = check.message.expect("a rejection must explain itself");
    assert!(message.contains("regular key"), "{message}");
    assert!(
        message.contains("Super+Space") || message.contains("Ctrl+Alt"),
        "the user needs an example of what does work: {message}"
    );
}

#[test]
fn a_valid_combination_reports_what_the_desktop_will_bind() {
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let check = crate::commands::check_hotkey_keys_with(
        &["KEY_LEFTMETA".to_string(), "KEY_SPACE".to_string()],
        &health,
    );
    assert!(check.accepted);
    assert_eq!(check.accelerator.as_deref(), Some("LOGO+space"));
    assert!(check.problem.is_none());
}

#[test]
fn bare_modifiers_are_allowed_where_voxctrl_watches_the_keyboard() {
    // On the evdev fallback a lone Super genuinely works. Refusing it there
    // would break a working setup to satisfy a constraint that does not
    // apply — but it is still worth telling the user it is fragile.
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Evdev);
    health.set_keyboards_open(1);

    let check = crate::commands::check_hotkey_keys_with(
        &["KEY_LEFTMETA".to_string()],
        &health,
    );
    assert!(check.accepted, "this combination works on this machine");
    assert!(!check.enforced);
    assert_eq!(check.problem.as_deref(), Some("modifiers_only"));
    assert!(check
        .message
        .expect("an advisory still needs wording")
        .contains("stop working"));
}

#[test]
fn two_regular_keys_are_refused_with_their_own_reason() {
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let check = crate::commands::check_hotkey_keys_with(
        &["KEY_A".to_string(), "KEY_B".to_string()],
        &health,
    );
    assert!(!check.accepted);
    assert_eq!(check.problem.as_deref(), Some("multiple_keys"));
}

#[test]
fn validation_is_enforced_before_the_backend_has_answered() {
    // The portal is the default path, so an unfinished handshake must not
    // be a window in which an unbindable shortcut can be saved.
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    assert_eq!(health.backend(), voxctrl_hotkeys::Backend::Starting);

    let check = crate::commands::check_hotkey_keys_with(
        &["KEY_LEFTMETA".to_string()],
        &health,
    );
    assert!(!check.accepted);
    assert!(check.enforced);
}

#[test]
fn hotkey_status_reports_the_portal_as_active_and_private() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    // The state the app is built for: the compositor owns the keys and
    // VoxCtrl has no access to input devices at all.
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let res = crate::commands::hotkey_status(&health);
    assert_eq!(res.backend, "portal");
    assert!(res.is_active);
    assert!(res.is_private);
    assert!(!res.needs_attention);
    assert_eq!(
        res.devices_readable, 0,
        "the portal path must not probe input devices at all"
    );
    assert!(!res.detail.is_empty(), "every state needs an explanation");
}

#[test]
fn hotkey_status_marks_the_evdev_fallback_as_not_private() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    // It works, but every keystroke on the machine passes through VoxCtrl,
    // and the user is entitled to know that.
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_portal_error("no such interface".to_string());
    health.set_backend(voxctrl_hotkeys::Backend::Evdev);
    health.set_keyboards_open(1);

    let res = crate::commands::hotkey_status(&health);
    assert_eq!(res.backend, "evdev");
    assert!(res.is_active);
    assert!(!res.is_private);
    assert!(!res.needs_attention, "shortcuts do work in this state");
    assert!(res.portal_error.is_some());
}

#[test]
fn hotkey_status_flags_kde_for_the_manual_enable_bug() {
    // xdg-desktop-portal-kde registers shortcuts disabled and gives VoxCtrl
    // no way to see that — this is the standing warning that fills the gap,
    // scoped to the one desktop the bug is confirmed on (bugs.kde.org
    // #483639) so it does not cry wolf where binding really is instant.
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::set_var("XDG_CURRENT_DESKTOP", "KDE");

    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let res = crate::commands::hotkey_status(&health);
    assert!(res.needs_manual_enable);
    let hint = res.manual_enable_hint.expect("must explain the fix");
    assert!(hint.contains("Apply"), "{hint}");
    assert!(hint.contains("483639"), "{hint}");

    std::env::remove_var("XDG_CURRENT_DESKTOP");
}

#[test]
fn hotkey_status_does_not_flag_desktops_without_the_bug() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
    std::env::remove_var("KDE_FULL_SESSION");

    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let res = crate::commands::hotkey_status(&health);
    assert!(!res.needs_manual_enable);
    assert!(res.manual_enable_hint.is_none());

    std::env::remove_var("XDG_CURRENT_DESKTOP");
}

#[test]
fn hotkey_status_scopes_the_kde_warning_to_the_portal_backend() {
    // The bug is specifically in how xdg-desktop-portal-kde hands off
    // BindShortcuts; the evdev fallback never goes near it.
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::set_var("XDG_CURRENT_DESKTOP", "KDE");

    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_portal_error("no such interface".to_string());
    health.set_backend(voxctrl_hotkeys::Backend::Evdev);
    health.set_keyboards_open(1);

    let res = crate::commands::hotkey_status(&health);
    assert!(!res.needs_manual_enable, "the evdev path never hits this bug");

    std::env::remove_var("XDG_CURRENT_DESKTOP");
}

#[test]
fn hotkey_status_recognises_kde_via_the_legacy_full_session_variable() {
    // Some Plasma sessions do not populate XDG_CURRENT_DESKTOP as "KDE";
    // KDE_FULL_SESSION is the older, still-set fallback signal.
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::remove_var("XDG_CURRENT_DESKTOP");
    std::env::set_var("KDE_FULL_SESSION", "true");

    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_backend(voxctrl_hotkeys::Backend::Portal);

    let res = crate::commands::hotkey_status(&health);
    assert!(res.needs_manual_enable);

    std::env::remove_var("KDE_FULL_SESSION");
}

#[test]
fn hotkey_status_honours_the_kde_manual_enable_test_override() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);

    std::env::set_var("VOXCTRL_TEST_HOTKEY_STATUS", "kde_manual_enable");
    let res = crate::commands::hotkey_status(&health);
    assert!(res.needs_manual_enable);
    assert!(res.manual_enable_hint.is_some());

    std::env::remove_var("VOXCTRL_TEST_HOTKEY_STATUS");
}

#[tokio::test]
async fn open_shortcut_settings_prefers_the_kde_module_when_available() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::set_var("VOXCTRL_FAKE_COMMANDS", "kcmshell6,gnome-control-center");
    std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "1");

    let res = crate::commands::open_shortcut_settings().await;
    assert!(res.is_ok(), "{res:?}");

    std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
    std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
}

#[tokio::test]
async fn open_shortcut_settings_falls_back_down_the_candidate_list() {
    // Only the last-resort GNOME panel is "installed" — the command must
    // still succeed by walking past every unavailable candidate first,
    // not give up at the first miss.
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::set_var("VOXCTRL_FAKE_COMMANDS", "gnome-control-center");
    std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "1");

    let res = crate::commands::open_shortcut_settings().await;
    assert!(res.is_ok(), "{res:?}");

    std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
    std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
}

#[tokio::test]
async fn open_shortcut_settings_explains_itself_when_nothing_is_installed() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    std::env::set_var("VOXCTRL_FAKE_COMMANDS", "");
    std::env::set_var("VOXCTRL_INSTALLER_TEST_MOCK", "1");

    let err = crate::commands::open_shortcut_settings().await.unwrap_err();
    assert!(err.contains("System Settings"), "{err}");

    std::env::remove_var("VOXCTRL_FAKE_COMMANDS");
    std::env::remove_var("VOXCTRL_INSTALLER_TEST_MOCK");
}

#[test]
fn hotkey_status_asks_for_attention_when_nothing_can_deliver_shortcuts() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);
    health.set_portal_error("no such interface".to_string());
    health.set_backend(voxctrl_hotkeys::Backend::None);

    let res = crate::commands::hotkey_status(&health);
    assert_eq!(res.backend, "none");
    assert!(!res.is_active);
    assert!(res.needs_attention);
    assert!(!res.detail.is_empty());
    assert!(
        res.devices_readable <= res.devices_total,
        "readable ({}) cannot exceed total ({})",
        res.devices_readable,
        res.devices_total
    );
}

#[test]
fn hotkey_status_does_not_flash_a_failure_during_startup() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    // The portal handshake is async. Reporting "broken" for the few hundred
    // milliseconds before it answers would pop the setup window on every
    // launch of a perfectly working install.
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);

    let res = crate::commands::hotkey_status(&health);
    assert_eq!(res.backend, "starting");
    assert!(res.is_active);
    assert!(!res.needs_attention);
}

#[test]
fn hotkey_status_honours_the_test_override() {
    let _lock = crate::test_utils::get_env_lock().lock().unwrap();
    let health = voxctrl_hotkeys::ListenerHealth::default();
    health.set_supported(true);

    std::env::set_var("VOXCTRL_TEST_HOTKEY_STATUS", "none");
    let res = crate::commands::hotkey_status(&health);
    assert_eq!(res.backend, "none");
    assert!(res.needs_attention);

    std::env::set_var("VOXCTRL_TEST_HOTKEY_STATUS", "portal");
    let res = crate::commands::hotkey_status(&health);
    assert_eq!(res.backend, "portal");
    assert!(res.is_private);

    std::env::remove_var("VOXCTRL_TEST_HOTKEY_STATUS");
}

#[tokio::test]
async fn test_setup_blocker_flags_a_missing_model_at_keypress() {
    // The hotkey fired, so permissions are fine — but the model is not
    // downloaded and dictation will silently produce nothing. That is
    // exactly the moment the user must be told.
    let state = Arc::new(make_test_state());
    {
        let mut cfg = state.config.lock().await;
        cfg.data.engine.backend = voxctrl_config::BackendChoice::WhisperCpp;
        cfg.data.engine.whisper_cpp.model_size = "large-v3".to_string();
        cfg.data.engine.whisper_cpp.model_dir =
            tempfile::tempdir().unwrap().path().to_string_lossy().to_string();
    }

    // The injection-tool check runs first and depends on the host, so only
    // assert that *something* is reported and that it names a real cause.
    let blocker = setup_blocker(&state).await.expect("unfinished setup must report");
    assert!(
        blocker.contains("large-v3") || blocker.contains("wtype") || blocker.contains("xdotool"),
        "message must name the actual cause: {blocker}"
    );
}

#[tokio::test]
async fn test_setup_blocker_stays_quiet_for_the_auto_downloading_default() {
    // "tiny" downloads itself in the background with its own notifications;
    // a second "go to Settings" toast mid-download is pure noise.
    if crate::commands::missing_injection_tool().is_some() {
        return; // host lacks wtype/xdotool; that blocker legitimately wins
    }
    let state = Arc::new(make_test_state());
    {
        let mut cfg = state.config.lock().await;
        cfg.data.engine.backend = voxctrl_config::BackendChoice::WhisperCpp;
        cfg.data.engine.whisper_cpp.model_size = "tiny".to_string();
        cfg.data.engine.whisper_cpp.model_dir =
            tempfile::tempdir().unwrap().path().to_string_lossy().to_string();
    }
    assert!(setup_blocker(&state).await.is_none());
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

// ── Setup wizard re-entry ────────────────────────────────────────────────────

#[test]
fn test_setup_flag_recognised_in_its_common_spellings() {
    // The flag exists so someone whose app is already configured can see the
    // wizard again. Making them guess the exact word would defeat that, so
    // every spelling anyone would reasonably reach for is accepted.
    for flag in ["--setup", "--wizard", "--setup-wizard", "--first-run"] {
        let args = vec!["voxctrl".to_string(), flag.to_string()];
        assert!(
            crate::wants_setup_wizard(&args),
            "expected {flag} to open the setup wizard"
        );
    }
}

#[test]
fn test_setup_flag_found_after_other_arguments() {
    let args = vec![
        "voxctrl".to_string(),
        "--some-other-flag".to_string(),
        "--setup".to_string(),
    ];
    assert!(crate::wants_setup_wizard(&args));
}

#[test]
fn test_normal_launch_does_not_open_the_wizard() {
    // A plain launch, and the installer path, must both leave the wizard alone
    // — otherwise every start would reopen setup on a configured machine.
    assert!(!crate::wants_setup_wizard(&["voxctrl".to_string()]));
    assert!(!crate::wants_setup_wizard(&[
        "voxctrl".to_string(),
        "--install".to_string()
    ]));
    assert!(!crate::wants_setup_wizard(&[]));
}

#[test]
fn test_setup_flag_is_not_matched_by_lookalike_arguments() {
    // Substring matching here would turn unrelated flags into a surprise
    // wizard on startup.
    for flag in ["--setup-something", "setup", "--no-setup", "--wizardry"] {
        let args = vec!["voxctrl".to_string(), flag.to_string()];
        assert!(
            !crate::wants_setup_wizard(&args),
            "{flag} must not be mistaken for the setup flag"
        );
    }
}
