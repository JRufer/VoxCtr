use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter,
};
use crate::state::AppState;

/// The tray entry that doubles as the setup indicator.
static SETUP_MENU_ITEM: OnceLock<tauri::menu::MenuItem<tauri::Wry>> = OnceLock::new();

pub const TRAY_SETUP_OK: &str = "🩺  Setup & Diagnostics";
#[cfg(target_os = "linux")]
pub const TRAY_SETUP_BROKEN: &str = "⚠️  Global shortcuts unavailable";

/// Reflect setup state in the tray, which is the one piece of VoxCtrl UI that
/// is always on screen.
#[cfg(target_os = "linux")]
pub fn update_tray_for_setup(app: &tauri::AppHandle, ok: bool) {
    let app = app.clone();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(item) = SETUP_MENU_ITEM.get() {
            let _ = item.set_text(if ok { TRAY_SETUP_OK } else { TRAY_SETUP_BROKEN });
        }
        if let Some(tray) = handle.tray_by_id("main-tray") {
            let _ = tray.set_tooltip(Some(if ok {
                "VoxCtrl"
            } else {
                "VoxCtrl — global shortcuts are unavailable"
            }));
        }
    });
}

pub fn create_tray(app: &tauri::App) -> Result<tauri::tray::TrayIcon, tauri::Error> {
    let record_off_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
        .expect("Failed to load record_off icon");
    let tray_icon = record_off_icon.clone();

    let settings_i = tauri::menu::MenuItem::with_id(app, "settings", "⚙  Settings", true, None::<&str>)?;
    let setup_i = tauri::menu::MenuItem::with_id(app, "setup", TRAY_SETUP_OK, true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Quit VoxCtrl", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(
        app,
        &[&settings_i, &setup_i, &separator, &quit_i],
    )?;
    let _ = SETUP_MENU_ITEM.set(setup_i);

    TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon)
        .tooltip("VoxCtrl")
        .menu(&menu)
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "settings" => {
                    if let Err(e) = crate::window::open_settings_window(app) {
                        tracing::error!("Could not open Settings: {e}");
                    }
                }
                "setup" => {
                    crate::window::show_setup_window();
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                if let Err(e) = crate::window::open_settings_window(tray.app_handle()) {
                    tracing::error!("Could not open Settings: {e}");
                }
            }
        })
        .build(app)
}

pub fn spawn_status_ticker(
    handle: tauri::AppHandle,
    state_for_ticker: Arc<AppState>,
    record_on_icon: tauri::image::Image<'static>,
    record_off_icon: tauri::image::Image<'static>,
    processing_frames: [tauri::image::Image<'static>; 6],
) {
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
                // Send the anchor + monitor; the overlay computes pixel
                // coordinates itself using its own display scale.
                let pos_msg = serde_json::json!({
                    "type": "position",
                    "position": overlay_position,
                    "monitor": overlay_monitor,
                });
                if let Ok(json_str) = serde_json::to_string(&pos_msg) {
                    let _ = state_for_ticker.overlay_tx.send(json_str);
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
}
