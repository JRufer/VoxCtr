//! Windows hotkey backend: a Win32 low-level keyboard hook via `rdev`.
//!
//! Gesture recognition is the shared engine, so `hold`, `toggle`, `double_tap`
//! and `double_tap_hold` behave exactly as they do on Linux.

use std::sync::{Arc, Mutex};

use tracing::{info, warn};
use voxctrl_routing::HotkeyBinding;

use crate::{
    gestures::GestureEngine, keys::KeyMatcher, Backend, GestureSender, ListenerHealth,
};

pub fn start(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: crate::ReloaderReceiver,
    health: Arc<ListenerHealth>,
) {
    // The Windows hook needs no device permissions, so the listener is healthy
    // as soon as the thread is up.
    health.set_supported(true);
    health.record_scan(1, 0);
    health.set_keyboards_open(1);
    health.set_backend(Backend::WindowsHook);

    let failed = health.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("voxctrl-rdev".into())
        .spawn(move || run(bindings, tx, rx_reload))
    {
        failed.set_backend_failed(format!("cannot start the keyboard hook: {e}"));
    }
}

fn run(bindings: Vec<HotkeyBinding>, tx: GestureSender, rx_reload: crate::ReloaderReceiver) {
    info!("rdev hotkey listener active (Windows)");

    let engine = Arc::new(Mutex::new(GestureEngine::new(bindings.clone())));
    let matcher = Arc::new(Mutex::new(KeyMatcher::new(bindings)));
    let tx = Arc::new(tx);

    let cb = {
        let engine = engine.clone();
        let matcher = matcher.clone();
        let tx = tx.clone();
        move |event: rdev::Event| {
            if let Ok(new_bindings) = rx_reload.try_recv() {
                tracing::info!(
                    "windows hotkey loop: reloading {} bindings",
                    new_bindings.len()
                );
                let mut engine = engine.lock().unwrap();
                engine.reset(&tx);
                engine.reload(new_bindings.clone());
                matcher.lock().unwrap().reload(new_bindings);
            }

            let (key, down) = match &event.event_type {
                rdev::EventType::KeyPress(k) => (key_name(k), true),
                rdev::EventType::KeyRelease(k) => (key_name(k), false),
                _ => return,
            };

            let transitions = matcher.lock().unwrap().on_key(&key, down);
            if transitions.is_empty() {
                return;
            }
            let mut engine = engine.lock().unwrap();
            for (id, transition) in transitions {
                engine.apply(&id, transition, &tx);
            }
        }
    };

    if let Err(e) = rdev::listen(cb) {
        warn!("rdev listener error: {e:?}");
    }

    // The hook is gone; nothing can release a gesture that is still running.
    let mut engine = engine.lock().unwrap();
    engine.reset(&tx);
}

/// rdev key → the evdev-style names bindings are stored with.
fn key_name(key: &rdev::Key) -> String {
    format!("KEY_{key:?}").to_ascii_uppercase()
}
