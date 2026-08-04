use std::{
    collections::HashSet,
    sync::Arc,
    time::Duration,
};

use tracing::warn;
use voxctrl_routing::{GestureType, HotkeyBinding};

use crate::{
    gestures::{shadowed_by_longer, BindingState, GestureEvent, GestureKind},
    GestureSender, ListenerHealth,
};

/// How often the supervisor rescans `/dev/input` while it has no keyboard.
/// Short enough that finishing the permission setup feels instant.
const RESCAN_BLOCKED_INTERVAL: Duration = Duration::from_secs(2);
/// Rescan interval once at least one keyboard is being read — this only picks
/// up newly plugged-in keyboards, so it can be lazy.
const RESCAN_HEALTHY_INTERVAL: Duration = Duration::from_secs(10);

pub fn start(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    device_path: Option<String>,
    rx_reload: crate::ReloaderReceiver,
    health: Arc<ListenerHealth>,
) {
    health.set_supported(true);

    let rt_handle = tokio::runtime::Handle::try_current().ok();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<(String, i32)>();

    // Spawn coordinator thread
    let rt = rt_handle.clone();
    std::thread::Builder::new()
        .name("voxctrl-hotkey-coord".into())
        .spawn(move || {
            let _guard = rt.as_ref().map(|h| h.enter());
            run_coordinator(bindings, tx, rx_reload, event_rx);
        })
        .expect("failed to spawn coordinator thread");

    std::thread::Builder::new()
        .name("voxctrl-evdev-supervisor".into())
        .spawn(move || run_supervisor(device_path, event_tx, health))
        .expect("failed to spawn evdev supervisor thread");
}

/// Owns the reader threads for the lifetime of the process.
///
/// Rescanning (rather than opening devices once at startup) is what makes the
/// permission setup restart-free: a user who runs the setup while VoxCtrl is
/// already running gets working hotkeys within a couple of seconds instead of
/// being told to relaunch the app or log out of their desktop session.
fn run_supervisor(
    device_path: Option<String>,
    event_tx: crossbeam_channel::Sender<(String, i32)>,
    health: Arc<ListenerHealth>,
) {
    let mut running: HashSet<String> = HashSet::new();
    let (exit_tx, exit_rx) = crossbeam_channel::unbounded::<String>();
    let mut warned_blocked = false;

    loop {
        // Reap readers whose device went away so a re-plug reopens it.
        while let Ok(path) = exit_rx.try_recv() {
            running.remove(&path);
        }

        let scan = scan_input_devices(device_path.as_deref());
        health.record_scan(scan.total, scan.denied);

        for path in scan.keyboards {
            if !running.insert(path.clone()) {
                continue;
            }
            let tx_clone = event_tx.clone();
            let exit_clone = exit_tx.clone();
            let reader_path = path.clone();
            match std::thread::Builder::new()
                .name("voxctrl-evdev".into())
                .spawn(move || {
                    run_reader(reader_path.clone(), tx_clone);
                    let _ = exit_clone.send(reader_path);
                }) {
                Ok(_) => tracing::info!("Reading hotkeys from {path}"),
                Err(e) => {
                    warn!("Failed to spawn evdev reader for {path}: {e}");
                    running.remove(&path);
                }
            }
        }

        health.set_keyboards_open(running.len());

        if running.is_empty() {
            if !warned_blocked {
                warned_blocked = true;
                if scan.denied > 0 {
                    warn!(
                        "No keyboard is readable: {} of {} input devices are permission-denied. \
                         Global hotkeys are disabled until VoxCtrl's setup is finished.",
                        scan.denied, scan.total
                    );
                } else {
                    warn!("No suitable keyboard evdev device found; hotkeys disabled");
                }
            }
        } else if warned_blocked {
            warned_blocked = false;
            tracing::info!("Keyboard access recovered; global hotkeys are active again");
        }

        std::thread::sleep(if running.is_empty() {
            RESCAN_BLOCKED_INTERVAL
        } else {
            RESCAN_HEALTHY_INTERVAL
        });
    }
}

fn run_reader(device_path: String, event_tx: crossbeam_channel::Sender<(String, i32)>) {
    let mut device = match open_device(&Some(device_path.clone())) {
        Some(d) => d,
        None => return,
    };

    loop {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() != evdev::EventType::KEY {
                        continue;
                    }
                    let mut key_name = match ev.kind() {
                        evdev::InputEventKind::Key(key) => format!("{:?}", key),
                        _ => format!("{:?}", ev.code()),
                    };
                    if key_name.starts_with("Key(") && key_name.ends_with(')') {
                        key_name = key_name[4..key_name.len() - 1].to_string();
                    }
                    let value = ev.value();
                    if value == 1 || value == 0 {
                        if event_tx.send((key_name, value)).is_err() {
                            // Coordinator thread has shut down, exit
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                // An unplugged keyboard errors forever. Give the device back to
                // the supervisor so it is reopened if it ever returns, instead
                // of spinning on a dead file descriptor for the whole session.
                if !std::path::Path::new(&device_path).exists() {
                    tracing::info!("evdev device {device_path} disappeared; releasing it");
                    return;
                }
                warn!("evdev read error on {device_path}: {e}; retrying in 1s");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn run_coordinator(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: crate::ReloaderReceiver,
    event_rx: crossbeam_channel::Receiver<(String, i32)>,
) {
    let mut states: Vec<BindingState> =
        bindings.into_iter().map(BindingState::new).collect();
    let mut pressed: HashSet<String> = HashSet::new();

    loop {
        crossbeam_channel::select! {
            recv(rx_reload) -> new_bindings => {
                match new_bindings {
                    Ok(new_bindings) => {
                        tracing::info!("linux hotkey loop: reloading {} bindings", new_bindings.len());
                        states = new_bindings.into_iter().map(BindingState::new).collect();
                        pressed.clear();
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            recv(event_rx) -> event => {
                match event {
                    Ok((key_name, value)) => {
                        if value == 1 {
                            pressed.insert(key_name.clone());
                            handle_press(&key_name, &mut states, &pressed, &tx);
                        } else if value == 0 {
                            handle_release(&key_name, &mut states, &pressed, &tx);
                            pressed.remove(&key_name);
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Hotkey coordinator: event channel closed unexpectedly \
                             ({e}); all hotkeys are disabled until the app restarts."
                        );
                        break;
                    }
                }
            }
        }
    }
}

fn handle_press(
    key: &str,
    states: &mut Vec<BindingState>,
    pressed: &HashSet<String>,
    tx: &GestureSender,
) {
    let shadowed = shadowed_by_longer(pressed, states);

    for s in states.iter_mut() {
        if s.binding.disabled || shadowed.contains(&s.binding.id) {
            continue;
        }

        if s.binding.gesture == GestureType::Chord {
            if let Some(ref subkey) = s.binding.subkey {
                if key == subkey && s.binding.keys.iter().all(|k| pressed.contains(k)) {
                    if !s.chord_active {
                        s.chord_active = true;
                        let _ = tx.send(GestureEvent {
                            binding_id: s.binding.id.clone(),
                            binding_label: s.binding.label.clone(),
                            target_id: s.binding.target_ids_string(),
                            kind: GestureKind::Start,
                        });
                    }
                }
            }
            continue;
        }

        // Only handle if this key is part of the binding and all binding keys
        // are pressed.
        if !s.binding.keys.contains(&key.to_string()) {
            continue;
        }
        if !s.binding.keys.iter().all(|k| pressed.contains(k)) {
            continue;
        }

        match s.binding.gesture {
            GestureType::Hold => {
                if !s.hold_active.load(std::sync::atomic::Ordering::SeqCst) && s.hold_cancel.is_none() {
                    let cancel = tokio_util::sync::CancellationToken::new();
                    s.hold_cancel = Some(cancel.clone());
                    let hold_active = s.hold_active.clone();
                    let tx = tx.clone();
                    let binding_id = s.binding.id.clone();
                    let binding_label = s.binding.label.clone();
                    let target_id = s.binding.target_ids_string();
                    let threshold = Duration::from_millis(s.binding.hold_threshold_ms as u64);
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = tokio::time::sleep(threshold) => {
                                hold_active.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = tx.send(GestureEvent {
                                    binding_id,
                                    binding_label,
                                    target_id,
                                    kind: GestureKind::Start,
                                });
                            }
                            _ = cancel.cancelled() => {}
                        }
                    });
                }
            }
            GestureType::Toggle => {
                if !s.toggle_on {
                    s.toggle_on = true;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Start,
                    });
                } else {
                    s.toggle_on = false;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Stop,
                    });
                }
            }
            GestureType::DoubleTap => {
                // Only advance the state machine; the event fires on release, not
                // press.  The hold-timer logic here was wrong: it gated the toggle
                // on hold_threshold_ms (default 1 s) and prevented DoubleTap from
                // ever firing when the timer happened to complete before release.
                s.double_tap.on_press();
            }
            GestureType::DoubleTapHold => {
                let completed = s.double_tap.on_press();
                if completed {
                    let cancel = tokio_util::sync::CancellationToken::new();
                    s.double_tap_hold_cancel = Some(cancel.clone());
                    let active = s.double_tap_hold_active.clone();
                    active.store(false, std::sync::atomic::Ordering::SeqCst);
                    let tx = tx.clone();
                    let binding_id = s.binding.id.clone();
                    let binding_label = s.binding.label.clone();
                    let target_id = s.binding.target_ids_string();
                    let threshold = Duration::from_millis(s.binding.hold_threshold_ms as u64);
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = tokio::time::sleep(threshold) => {
                                active.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = tx.send(GestureEvent {
                                    binding_id: binding_id.clone(),
                                    binding_label: binding_label.clone(),
                                    target_id: target_id.clone(),
                                    kind: GestureKind::Start,
                                });
                                
                                // Spawn the 2-minute safety timeout
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_secs(120)) => {
                                        if active.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                            let _ = tx.send(GestureEvent {
                                                binding_id,
                                                binding_label,
                                                target_id,
                                                kind: GestureKind::Stop,
                                            });
                                        }
                                    }
                                    _ = cancel.cancelled() => {}
                                }
                            }
                            _ = cancel.cancelled() => {}
                        }
                    });
                }
            }
            GestureType::Chord => {
                let _ = tx.send(GestureEvent {
                    binding_id: s.binding.id.clone(),
                    binding_label: s.binding.label.clone(),
                    target_id: s.binding.target_ids_string(),
                    kind: GestureKind::Start,
                });
            }
        }
    }
}

fn handle_release(
    key: &str,
    states: &mut Vec<BindingState>,
    pressed: &HashSet<String>,
    tx: &GestureSender,
) {
    // Collect keys for any currently active DoubleTapHold gestures
    let active_dth_keys: Vec<Vec<String>> = states.iter()
        .filter(|s| s.binding.gesture == GestureType::DoubleTapHold
            && s.double_tap_hold_active.load(std::sync::atomic::Ordering::SeqCst))
        .map(|s| s.binding.keys.clone())
        .collect();

    for s in states.iter_mut() {
        if s.binding.disabled {
            continue;
        }
        if !s.binding.keys.contains(&key.to_string()) {
            continue;
        }

        match s.binding.gesture {
            GestureType::Hold => {
                // Is any *other* key of this binding still held? `pressed` still
                // contains the key currently being released (the caller removes it
                // only after this returns), so exclude it explicitly.
                let others_still_held = s
                    .binding
                    .keys
                    .iter()
                    .any(|k| k.as_str() != key && pressed.contains(k));

                if s.hold_active.load(std::sync::atomic::Ordering::SeqCst) {
                    // Recording is active. Stop only once the *whole* combo has been
                    // released. If we stopped on the first key-up, transcription could
                    // be injected while a hotkey modifier (e.g. Super/Ctrl) is still
                    // down — the Wayland compositor then interprets the synthetic
                    // keystrokes as shortcuts and swallows them, so the text never
                    // reaches the cursor. wtype cannot clear a physically-held
                    // modifier, so the only reliable fix is to wait for full release.
                    if !others_still_held {
                        s.hold_cancel.take();
                        s.hold_active.store(false, std::sync::atomic::Ordering::SeqCst);
                        let _ = tx.send(GestureEvent {
                            binding_id: s.binding.id.clone(),
                            binding_label: s.binding.label.clone(),
                            target_id: s.binding.target_ids_string(),
                            kind: GestureKind::Stop,
                        });
                    }
                } else if let Some(cancel) = s.hold_cancel.take() {
                    // Still within the hold threshold (Start not yet fired).
                    // Releasing any key breaks the pending combo, so cancel the timer.
                    cancel.cancel();
                }
            }
            GestureType::DoubleTap => {
                if s.double_tap.on_release() {
                    let suppressed = active_dth_keys.contains(&s.binding.keys);
                    if !suppressed {
                        if !s.toggle_on {
                            s.toggle_on = true;
                            let _ = tx.send(GestureEvent {
                                binding_id: s.binding.id.clone(),
                                binding_label: s.binding.label.clone(),
                                target_id: s.binding.target_ids_string(),
                                kind: GestureKind::Start,
                            });
                        } else {
                            s.toggle_on = false;
                            let _ = tx.send(GestureEvent {
                                binding_id: s.binding.id.clone(),
                                binding_label: s.binding.label.clone(),
                                target_id: s.binding.target_ids_string(),
                                kind: GestureKind::Stop,
                            });
                        }
                    }
                }
            }
            GestureType::DoubleTapHold => {
                if s.double_tap.on_release() {
                    if let Some(cancel) = s.double_tap_hold_cancel.take() {
                        cancel.cancel();
                    }
                    if s.double_tap_hold_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        let _ = tx.send(GestureEvent {
                            binding_id: s.binding.id.clone(),
                            binding_label: s.binding.label.clone(),
                            target_id: s.binding.target_ids_string(),
                            kind: GestureKind::Stop,
                        });
                    }
                }
            }
            GestureType::Chord => {
                if s.chord_active {
                    s.chord_active = false;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Stop,
                    });
                }
            }
            _ => {}
        }
    }
}

// ── Device selection ──────────────────────────────────────────────────────────

fn open_device(preferred: &Option<String>) -> Option<evdev::Device> {
    if let Some(path) = preferred {
        if let Ok(d) = evdev::Device::open(path) {
            return Some(d);
        }
        warn!("Saved evdev device {path} not accessible");
    }
    None
}

/// Result of one `/dev/input` sweep.
pub(crate) struct InputScan {
    /// Devices that are readable and look like a keyboard.
    pub keyboards: Vec<String>,
    /// Devices that exist but cannot be opened because of permissions.
    pub denied: usize,
    /// Every `event*` node found, readable or not.
    pub total: usize,
}

/// True for an evdev device VoxCtrl is willing to read hotkeys from.
///
/// Virtual/synthetic devices are skipped so VoxCtrl never reacts to the
/// keystrokes it (or another automation tool) injects itself.
pub(crate) fn is_eligible_keyboard(name: &str, has_key_a: bool) -> bool {
    let name = name.to_ascii_lowercase();
    if name.contains("virtual")
        || name.contains("uinput")
        || name.contains("xtest")
        || name.contains("passthrough")
    {
        return false;
    }
    has_key_a
}

/// Sweep `/dev/input` for keyboards, counting the devices that exist but are
/// unreadable.
///
/// `evdev::enumerate()` alone cannot answer "is this machine missing a
/// keyboard, or is VoxCtrl just not allowed to read it?" — it silently drops
/// every device it fails to open. Opening the nodes directly keeps that
/// distinction, which is the difference between a useful warning and the
/// current silence.
pub(crate) fn scan_input_devices(preferred: Option<&str>) -> InputScan {
    let mut scan = InputScan {
        keyboards: Vec::new(),
        denied: 0,
        total: 0,
    };

    // An explicitly configured device is the only one we consider.
    if let Some(path) = preferred {
        scan.total = 1;
        match std::fs::File::open(path) {
            // Opening as a real evdev device, not just a file: otherwise a
            // stale or misconfigured path is reported as a working keyboard and
            // the supervisor respawns a reader for it on every sweep.
            Ok(_) => match evdev::Device::open(path) {
                Ok(_) => scan.keyboards.push(path.to_string()),
                Err(e) => warn!("Configured evdev device {path} is not usable: {e}"),
            },
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => scan.denied = 1,
            Err(e) => warn!("Configured evdev device {path} is not usable: {e}"),
        }
        return scan;
    }

    let entries = match std::fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Cannot list /dev/input: {e}");
            return scan;
        }
    };

    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_str()
            .map(|n| n.starts_with("event"))
            .unwrap_or(false)
        {
            continue;
        }
        scan.total += 1;
        let path = entry.path();

        if let Err(e) = std::fs::File::open(&path) {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                scan.denied += 1;
            }
            continue;
        }

        let Ok(dev) = evdev::Device::open(&path) else {
            continue;
        };
        let name = dev.name().unwrap_or("").to_string();
        let has_key_a = dev
            .supported_keys()
            .map(|keys| keys.contains(evdev::Key::KEY_A))
            .unwrap_or(false);
        if is_eligible_keyboard(&name, has_key_a) {
            tracing::debug!("Eligible keyboard: {name} at {path:?}");
            scan.keyboards.push(path.to_string_lossy().to_string());
        }
    }

    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use voxctrl_routing::{HotkeyBinding, GestureType};

    #[test]
    fn synthetic_devices_are_never_used_as_hotkey_sources() {
        // Reading back our own injected keystrokes would retrigger the very
        // hotkey that produced them.
        for name in [
            "VoxCtrl Virtual Keyboard",
            "py-evdev-uinput",
            "XTEST pointer",
            "Some passthrough device",
        ] {
            assert!(!is_eligible_keyboard(name, true), "{name} must be skipped");
        }
    }

    #[test]
    fn real_keyboards_are_eligible_and_non_keyboards_are_not() {
        assert!(is_eligible_keyboard("AT Translated Set 2 keyboard", true));
        assert!(!is_eligible_keyboard("Logitech USB Mouse", false));
    }

    #[test]
    fn scanning_a_missing_configured_device_reports_it_as_unusable() {
        // Not a permission problem — the app must not tell the user to fix
        // permissions when their saved device simply no longer exists.
        let scan = scan_input_devices(Some("/dev/input/does-not-exist-voxctrl-test"));
        assert_eq!(scan.total, 1);
        assert_eq!(scan.denied, 0);
        assert!(scan.keyboards.is_empty());
    }

    #[test]
    fn scanning_reports_totals_consistently() {
        let scan = scan_input_devices(None);
        assert!(
            scan.denied <= scan.total,
            "denied ({}) cannot exceed total ({})",
            scan.denied,
            scan.total
        );
        assert!(scan.keyboards.len() <= scan.total);
    }

    fn make_test_binding(id: &str, gesture: GestureType, keys: Vec<&str>) -> BindingState {
        BindingState::new(HotkeyBinding {
            id: id.to_string(),
            label: "Test Label".to_string(),
            keys: keys.into_iter().map(String::from).collect(),
            gesture,
            target_id: "target".to_string(),
            target_ids: vec!["target".to_string()],
            tap_ms: 250,
            hold_threshold_ms: 100, // short threshold for testing
            subkey: None,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        })
    }

    fn make_test_binding_with_subkey(
        id: &str,
        gesture: GestureType,
        keys: Vec<&str>,
        subkey: Option<&str>,
    ) -> BindingState {
        BindingState::new(HotkeyBinding {
            id: id.to_string(),
            label: "Test Label".to_string(),
            keys: keys.into_iter().map(String::from).collect(),
            gesture,
            target_id: "target".to_string(),
            target_ids: vec!["target".to_string()],
            tap_ms: 250,
            hold_threshold_ms: 100,
            subkey: subkey.map(String::from),
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        })
    }

    #[tokio::test]
    async fn test_double_tap_fires_with_zero_hold_threshold() {
        // Regression: DoubleTap was gated on a hold timer using hold_threshold_ms.
        // When hold_threshold_ms=0 (or any low value) the timer fired before the key
        // was released, setting double_tap_hold_triggered=true and permanently
        // suppressing the toggle.  With the fix the timer is gone entirely.
        let (tx, mut rx) = crate::channel();
        let mut states = vec![BindingState::new(HotkeyBinding {
            id: "dt_zero_threshold".to_string(),
            label: "DT Zero".to_string(),
            keys: vec!["KEY_LEFTMETA".to_string()],
            gesture: GestureType::DoubleTap,
            target_id: "t".to_string(),
            target_ids: vec!["t".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 0, // the value that previously broke DoubleTap
            subkey: None,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        })];
        let mut pressed = std::collections::HashSet::new();

        // First tap
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        tokio::time::sleep(std::time::Duration::from_millis(60)).await;

        // Second tap — must emit Start despite hold_threshold_ms=0
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        let event = rx.recv().await.expect("DoubleTap must fire with zero hold_threshold_ms");
        assert_eq!(event.binding_id, "dt_zero_threshold");
        assert_eq!(event.kind, GestureKind::Start);
    }

    #[tokio::test]
    async fn test_double_tap_fires_with_large_hold_threshold() {
        // Regression: DoubleTap with a large hold_threshold_ms (e.g. default 1000 ms)
        // previously spawned a long-running timer.  A quick release (before the timer
        // fired) would NOT emit an event because the timer hadn't set triggered=true,
        // and the release handler required triggered=false to fire.  With the fix,
        // DoubleTap always fires on the second release regardless of hold_threshold_ms.
        let (tx, mut rx) = crate::channel();
        let mut states = vec![BindingState::new(HotkeyBinding {
            id: "dt_large_threshold".to_string(),
            label: "DT Large".to_string(),
            keys: vec!["KEY_LEFTMETA".to_string()],
            gesture: GestureType::DoubleTap,
            target_id: "t".to_string(),
            target_ids: vec!["t".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 1000, // default value from models.rs
            subkey: None,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        })];
        let mut pressed = std::collections::HashSet::new();

        // First tap
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        tokio::time::sleep(std::time::Duration::from_millis(60)).await;

        // Second tap (quick release, well before 1000 ms threshold)
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        let event = rx.recv().await.expect("DoubleTap must fire regardless of hold_threshold_ms");
        assert_eq!(event.binding_id, "dt_large_threshold");
        assert_eq!(event.kind, GestureKind::Start);
    }

    #[tokio::test]
    async fn test_toggle_gesture_flow() {
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding("test_toggle", GestureType::Toggle, vec!["KEY_LEFTCTRL"])];
        let mut pressed = HashSet::new();

        // 1. Press key
        pressed.insert("KEY_LEFTCTRL".to_string());
        handle_press("KEY_LEFTCTRL", &mut states, &pressed, &tx);
        
        // Assert we get GestureKind::Start
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_toggle");
        assert_eq!(event.kind, GestureKind::Start);

        // 2. Release key
        handle_release("KEY_LEFTCTRL", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTCTRL");
        // Toggle release does nothing, assert channel is empty
        assert!(rx.try_recv().is_err());

        // 3. Press key again
        pressed.insert("KEY_LEFTCTRL".to_string());
        handle_press("KEY_LEFTCTRL", &mut states, &pressed, &tx);

        // Assert we get GestureKind::Stop
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_toggle");
        assert_eq!(event.kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn test_hold_gesture_flow() {
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding("test_hold", GestureType::Hold, vec!["KEY_LEFTALT"])];
        let mut pressed = HashSet::new();

        // 1. Press key
        pressed.insert("KEY_LEFTALT".to_string());
        handle_press("KEY_LEFTALT", &mut states, &pressed, &tx);

        // Hold has a threshold (100ms), so it shouldn't send anything immediately
        assert!(rx.try_recv().is_err());

        // Wait for threshold
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Assert we get GestureKind::Start
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_hold");
        assert_eq!(event.kind, GestureKind::Start);

        // 2. Release key
        handle_release("KEY_LEFTALT", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTALT");

        // Assert we get GestureKind::Stop immediately on release
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_hold");
        assert_eq!(event.kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn test_hold_multikey_stops_only_on_full_release() {
        // Regression: a Hold combo (Super+Space) must not emit Stop until *every*
        // key is released. Stopping on the first key-up let transcription be
        // injected while Super was still held, which Wayland compositors swallow
        // as a shortcut — so the dictated text never reached the cursor.
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding(
            "test_hold_combo",
            GestureType::Hold,
            vec!["KEY_LEFTMETA", "KEY_SPACE"],
        )];
        let mut pressed = HashSet::new();

        // Press the full combo.
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.insert("KEY_SPACE".to_string());
        handle_press("KEY_SPACE", &mut states, &pressed, &tx);

        // Wait past the hold threshold to receive Start.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.kind, GestureKind::Start);

        // Release SPACE first while LEFTMETA is still held — must NOT stop yet.
        handle_release("KEY_SPACE", &mut states, &pressed, &tx);
        pressed.remove("KEY_SPACE");
        assert!(
            rx.try_recv().is_err(),
            "Stop fired while a hotkey modifier was still held"
        );

        // Release the last key — now Stop should fire.
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_hold_combo");
        assert_eq!(event.kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn test_double_tap_gesture_flow() {
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding("test_dt", GestureType::DoubleTap, vec!["KEY_LEFTMETA"])];
        let mut pressed = HashSet::new();

        // --- First Double Tap (Starts) ---
        // 1. First Press
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 2. First Release
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        assert!(rx.try_recv().is_err());

        // Sleep to avoid debouncing (50ms)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 3. Second Press (Spawns hold timer, does not trigger on press)
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 4. Second Release (Quickly released, triggers toggle Start)
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_dt");
        assert_eq!(event.kind, GestureKind::Start);

        // Sleep to avoid debouncing for the next double-tap
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // --- Second Double Tap (Stops) ---
        // 5. Third Press
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 6. Third Release
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        assert!(rx.try_recv().is_err());

        // Sleep to avoid debouncing (50ms)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 7. Fourth Press (Spawns hold timer)
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 8. Fourth Release (Quickly released, triggers toggle Stop)
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_dt");
        assert_eq!(event.kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn test_double_tap_hold_gesture_flow() {
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding("test_dth", GestureType::DoubleTapHold, vec!["KEY_LEFTMETA"])];
        let mut pressed = HashSet::new();

        // 1. First Press
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 2. First Release
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        assert!(rx.try_recv().is_err());

        // Sleep to avoid debouncing
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 3. Second Press (Spawns timer)
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // Wait for hold threshold
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Assert we get GestureKind::Start
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_dth");
        assert_eq!(event.kind, GestureKind::Start);

        // 4. Release
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        // Assert we get GestureKind::Stop immediately on release
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_dth");
        assert_eq!(event.kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn test_double_tap_coexistence_flow() {
        let (tx, mut rx) = crate::channel();
        // Coexisting bindings on the same key: DoubleTap (Toggle) and DoubleTapHold
        let mut states = vec![
            make_test_binding("dt_toggle", GestureType::DoubleTap, vec!["KEY_LEFTMETA"]),
            make_test_binding("dt_hold", GestureType::DoubleTapHold, vec!["KEY_LEFTMETA"]),
        ];
        let mut pressed = HashSet::new();

        // Scenario A: Quick release double tap (should trigger dt_toggle)
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        // Release quickly (before 100ms threshold)
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        // dt_toggle should trigger Start
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "dt_toggle");
        assert_eq!(event.kind, GestureKind::Start);
        // dt_hold should not trigger
        assert!(rx.try_recv().is_err());

        // Sleep to avoid debouncing for the next test
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Reset toggled state for dt_toggle
        states[0].toggle_on = false;

        // Scenario B: Held double tap (should trigger dt_hold)
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        // Wait beyond 100ms threshold
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // dt_hold should trigger Start
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "dt_hold");
        assert_eq!(event.kind, GestureKind::Start);
        // dt_toggle should not trigger
        assert!(rx.try_recv().is_err());

        // Now release
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");

        // dt_hold should trigger Stop
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "dt_hold");
        assert_eq!(event.kind, GestureKind::Stop);
        // dt_toggle should not trigger on release either
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn test_double_tap_hold_safety_timeout() {
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding("test_dth", GestureType::DoubleTapHold, vec!["KEY_LEFTMETA"])];
        let mut pressed = HashSet::new();

        // Double tap sequence
        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        
        // Wait 100ms (virtual time)
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        pressed.insert("KEY_LEFTMETA".to_string());
        handle_press("KEY_LEFTMETA", &mut states, &pressed, &tx);

        // Wait beyond threshold (100ms virtual time)
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Start event should fire
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_dth");
        assert_eq!(event.kind, GestureKind::Start);

        // Advance virtual time by 121 seconds (safety timeout is 120s)
        tokio::time::sleep(std::time::Duration::from_secs(121)).await;

        // Stop event should fire automatically due to safety timeout
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_dth");
        assert_eq!(event.kind, GestureKind::Stop);

        // When the user eventually releases, no further stop event should fire
        handle_release("KEY_LEFTMETA", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTMETA");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_chord_gesture_flow() {
        let (tx, mut rx) = crate::channel();
        let mut states = vec![make_test_binding_with_subkey(
            "test_chord",
            GestureType::Chord,
            vec!["KEY_LEFTCTRL", "KEY_LEFTALT"],
            Some("KEY_C"),
        )];
        let mut pressed = HashSet::new();

        // 1. Press and hold base key 1
        pressed.insert("KEY_LEFTCTRL".to_string());
        handle_press("KEY_LEFTCTRL", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 2. Press and hold base key 2
        pressed.insert("KEY_LEFTALT".to_string());
        handle_press("KEY_LEFTALT", &mut states, &pressed, &tx);
        assert!(rx.try_recv().is_err());

        // 3. Press subkey (should trigger chord Start)
        pressed.insert("KEY_C".to_string());
        handle_press("KEY_C", &mut states, &pressed, &tx);
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_chord");
        assert_eq!(event.kind, GestureKind::Start);

        // 4. Release subkey (Chord does not emit Stop, assert channel is empty)
        handle_release("KEY_C", &mut states, &pressed, &tx);
        pressed.remove("KEY_C");
        assert!(rx.try_recv().is_err());

        // 5. Release one of the base keys (should trigger Stop since it's a chord release)
        handle_release("KEY_LEFTALT", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTALT");
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_chord");
        assert_eq!(event.kind, GestureKind::Stop);

        // 6. Press base keys and subkey again to verify we can trigger it a second time
        pressed.insert("KEY_LEFTALT".to_string());
        handle_press("KEY_LEFTALT", &mut states, &pressed, &tx);
        
        pressed.insert("KEY_C".to_string());
        handle_press("KEY_C", &mut states, &pressed, &tx);
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_chord");
        assert_eq!(event.kind, GestureKind::Start);

        // 7. Release one of the base keys again to stop it
        handle_release("KEY_LEFTCTRL", &mut states, &pressed, &tx);
        pressed.remove("KEY_LEFTCTRL");
        let event = rx.recv().await.unwrap();
        assert_eq!(event.binding_id, "test_chord");
        assert_eq!(event.kind, GestureKind::Stop);
    }
}

