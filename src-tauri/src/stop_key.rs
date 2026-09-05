//! When VoxCtrl is allowed to hold the TTS stop key.
//!
//! The stop key is a global shortcut like any other, with one difference: its
//! default is Escape, and on a desktop where the compositor owns the key grab
//! (the XDG `GlobalShortcuts` portal) a registered shortcut is an *exclusive*
//! grab. Holding Escape for the life of the process meant no other application
//! ever saw it — an open menu would not close, a dialog would not cancel — for
//! as long as VoxCtrl was running.
//!
//! Re-emitting the key afterwards does not solve it. A synthetic press enters
//! the same input pipeline the grab sits on, so the compositor intercepts it
//! again: the key never reaches the focused app and VoxCtrl's own shortcut
//! fires in a loop. Injecting *below* the compositor (uinput) needs the
//! keyboard access this app deliberately refuses to arrange, and Wayland has no
//! way for one client to deliver a key to another at all.
//!
//! So VoxCtrl holds the grab only while it is actually speaking. Escape
//! interrupts playback, which is the whole point of the key, and belongs to the
//! rest of the desktop the rest of the time.
//!
//! None of this applies where VoxCtrl watches the key stream itself — the X11
//! raw-event backend, evdev, and the Windows low-level hook all observe without
//! grabbing, so every app receives Escape regardless and the stop key is simply
//! registered for the whole session.

use std::sync::{atomic::Ordering, Arc};
use std::time::Duration;

use voxctrl_hotkeys::Backend;
use voxctrl_routing::{GestureType, HotkeyBinding};

use crate::state::AppState;

/// The synthetic binding id the gesture handler matches on. Defined in
/// `voxctrl-routing` because the portal backend needs it too: it tells the one
/// shortcut VoxCtrl may hold transiently from a binding the user chose.
pub use voxctrl_routing::TTS_STOP_BINDING_ID as STOP_BINDING_ID;

/// How long the grab is kept after playback stops.
///
/// Long enough that the gaps between the utterances of one spoken response do
/// not each cost a re-registration — that churn is a D-Bus round trip and, on
/// KDE, a rewrite of the user's shortcut store. Short enough that a user who
/// presses Escape at a menu a moment after VoxCtrl stops talking gets their
/// menu closed rather than a swallowed key.
const RELEASE_AFTER: Duration = Duration::from_millis(2000);

/// How often the arbiter re-reads the world.
///
/// It is not only speech that moves this: the hotkey backend is still being
/// negotiated at launch, and the user can change the stop key in Settings. Both
/// change the answer without any playback event to hang a decision on.
const TICK: Duration = Duration::from_millis(500);

/// How long VoxCtrl may hold the stop key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopKeyGrab {
    /// No stop key is configured; there is nothing to register.
    None,
    /// Registered for the whole session. Either VoxCtrl is watching the keys
    /// itself and grabs nothing, or the combination is one no other app is
    /// listening for.
    Always,
    /// Registered only while VoxCtrl speaks, because a standing grab would take
    /// the key from the rest of the desktop.
    WhileSpeaking,
}

/// How long VoxCtrl may hold `stop_key` on the backend that is running.
pub fn stop_key_grab(backend: Backend, stop_key: &[String]) -> StopKeyGrab {
    if stop_key.is_empty() {
        return StopKeyGrab::None;
    }
    if !voxctrl_hotkeys::is_reserved_for_the_desktop(stop_key) {
        return StopKeyGrab::Always;
    }
    // These backends observe the key stream without grabbing anything, so the
    // application under the cursor receives Escape either way.
    if backend.sees_raw_keys() {
        return StopKeyGrab::Always;
    }
    // A listener that has not finished choosing a backend. On Linux it may still
    // land on the portal, so stay conservative: registering first and relaxing
    // later would hold Escape from launch until it resolves, which is the state
    // this exists to avoid. Everywhere else the only backend is one that grabs
    // nothing, and waiting would cost the user a stop key that works.
    if backend == Backend::Starting && !cfg!(target_os = "linux") {
        return StopKeyGrab::Always;
    }

    // Portal, Mint, and — on Linux — a listener still deciding.
    StopKeyGrab::WhileSpeaking
}

/// Should the stop binding be in the set handed to the listener right now?
pub fn stop_key_wanted(grab: StopKeyGrab, speaking: bool) -> bool {
    match grab {
        StopKeyGrab::None => false,
        StopKeyGrab::Always => true,
        StopKeyGrab::WhileSpeaking => speaking,
    }
}

/// The synthetic binding that carries the stop key into the listener.
///
/// It has no target and never records: `pipeline` matches its id and stops
/// playback on key-down.
pub fn stop_binding(stop_key: Vec<String>) -> HotkeyBinding {
    HotkeyBinding {
        id: STOP_BINDING_ID.to_string(),
        label: "TTS Stop Key".to_string(),
        keys: stop_key,
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
    }
}

/// The saved bindings plus the stop key, when the stop key is currently held.
///
/// Every path that reloads the listener goes through this, so a save from the
/// settings window cannot accidentally drop a grab the arbiter is holding — or
/// leave one behind that it has already released.
pub async fn listener_bindings(state: &AppState, saved: Vec<HotkeyBinding>) -> Vec<HotkeyBinding> {
    let mut all = saved;
    if state.stop_key_held.load(Ordering::SeqCst) {
        let stop_key = state.config.lock().await.data.tts.stop_key.clone();
        if !stop_key.is_empty() {
            all.push(stop_binding(stop_key));
        }
    }
    all
}

/// The saved bindings, read from disk, plus the stop key when it is held.
pub async fn listener_bindings_from_disk(state: &AppState) -> Vec<HotkeyBinding> {
    let dir = voxctrl_routing::config_dir();
    let saved = voxctrl_routing::load_bindings(&dir).unwrap_or_default();
    listener_bindings(state, saved).await
}

/// Start the task that takes the stop key and gives it back.
///
/// Returns without spawning if one is already running.
pub fn spawn(state: Arc<AppState>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
    if state.speaking_tx.set(tx).is_err() {
        return;
    }

    tokio::spawn(async move {
        // What was last handed to the listener. The arbiter is the only writer
        // of `stop_key_held`, and every other reload path reads it, so the two
        // can never disagree about what the listener is holding.
        let mut held = state.stop_key_held.load(Ordering::SeqCst);
        let mut release_at: Option<tokio::time::Instant> = None;

        loop {
            let speaking = tokio::select! {
                msg = rx.recv() => match msg {
                    Some(speaking) => speaking,
                    // The app is shutting down.
                    None => return,
                },
                _ = tokio::time::sleep(TICK) => state.is_speaking(),
            };

            // Speech ending does not release the key immediately; the next
            // utterance of the same response is usually a moment away. There is
            // no window to serve when the key is not held: starting one would
            // arm the grab on the first idle tick, which is the opposite of the
            // point.
            release_at = if speaking || !held {
                None
            } else {
                Some(release_at.unwrap_or_else(|| tokio::time::Instant::now() + RELEASE_AFTER))
            };
            let wanted_now = speaking
                || release_at.is_some_and(|deadline| tokio::time::Instant::now() < deadline);

            let stop_key = match state.config.try_lock() {
                Ok(cfg) => cfg.data.tts.stop_key.clone(),
                // Someone is mid-save. Whatever they are writing, they reload
                // the listener themselves; this tick has nothing to add.
                Err(_) => continue,
            };
            let grab = stop_key_grab(state.hotkey_health.backend(), &stop_key);
            let wanted = stop_key_wanted(grab, wanted_now);
            if wanted == held {
                continue;
            }

            // Re-registering restarts the listener's gesture engine, which
            // cancels whatever it is tracking. Doing that under a held dictation
            // key would drop the user's recording mid-sentence, and nothing here
            // is urgent enough to be worth that — the next tick will take it.
            if state.is_recording() {
                continue;
            }

            let bindings = {
                state.stop_key_held.store(wanted, Ordering::SeqCst);
                listener_bindings_from_disk(&state).await
            };
            let reloader = state.hotkey_reloader.lock().await;
            let Some(reloader) = reloader.as_ref() else {
                // No listener yet. Leave `held` alone so this is retried.
                state.stop_key_held.store(held, Ordering::SeqCst);
                continue;
            };
            match reloader.send(bindings) {
                Ok(()) => {
                    held = wanted;
                    tracing::debug!(
                        "TTS stop key {} ({grab:?})",
                        if wanted { "registered" } else { "released" }
                    );
                }
                Err(e) => {
                    state.stop_key_held.store(held, Ordering::SeqCst);
                    tracing::warn!("Could not reload bindings for the stop key: {e}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn escape_is_held_only_while_speaking_where_the_desktop_owns_the_grab() {
        // The bug this exists for: a standing portal grab on Escape meant no
        // other app ever saw the key. Playback is the one stretch where taking
        // it is what the user wants.
        let grab = stop_key_grab(Backend::Portal, &keys(&["KEY_ESC"]));
        assert_eq!(grab, StopKeyGrab::WhileSpeaking);
        assert!(stop_key_wanted(grab, true));
        assert!(!stop_key_wanted(grab, false));
    }

    #[test]
    fn escape_is_held_throughout_where_voxctrl_watches_the_keyboard() {
        // X11 raw events, evdev and the Windows hook grab nothing: every app
        // receives Escape whatever VoxCtrl has registered, so there is nothing
        // to give back and no reason to churn the listener between utterances.
        for backend in [Backend::X11, Backend::Evdev, Backend::WindowsHook] {
            let grab = stop_key_grab(backend, &keys(&["KEY_ESC"]));
            assert_eq!(grab, StopKeyGrab::Always, "{backend:?}");
            assert!(stop_key_wanted(grab, false), "{backend:?}");
        }
    }

    #[test]
    fn a_stop_key_with_a_modifier_is_an_ordinary_standing_shortcut() {
        // Nothing else is listening for Ctrl+Escape, so holding it costs the
        // desktop nothing — and a user who set one gets a stop key that works
        // from the first millisecond of playback.
        let grab = stop_key_grab(Backend::Portal, &keys(&["KEY_LEFTCTRL", "KEY_ESC"]));
        assert_eq!(grab, StopKeyGrab::Always);
        assert!(stop_key_wanted(grab, false));
    }

    #[test]
    fn an_undecided_backend_does_not_get_the_benefit_of_the_doubt() {
        // The listener is still choosing a backend at launch. Registering now
        // and relaxing later would hold Escape from startup until it resolves,
        // which is the state this whole module exists to avoid.
        for backend in [Backend::None, Backend::MintDbus] {
            assert_eq!(
                stop_key_grab(backend, &keys(&["KEY_ESC"])),
                StopKeyGrab::WhileSpeaking,
                "{backend:?}"
            );
        }

        // `Starting` is only undecided where a grab-owning backend is on the
        // table. On Windows the hook is the only option and it grabs nothing,
        // so waiting would cost the user a stop key that works.
        let starting = stop_key_grab(Backend::Starting, &keys(&["KEY_ESC"]));
        if cfg!(target_os = "linux") {
            assert_eq!(starting, StopKeyGrab::WhileSpeaking);
        } else {
            assert_eq!(starting, StopKeyGrab::Always);
        }
    }

    #[test]
    fn no_stop_key_means_nothing_is_ever_registered() {
        let grab = stop_key_grab(Backend::Portal, &[]);
        assert_eq!(grab, StopKeyGrab::None);
        assert!(!stop_key_wanted(grab, true));
    }

    #[test]
    fn the_stop_binding_records_nothing_and_targets_nothing() {
        // It exists to be matched by id in the gesture handler. A target or a
        // hold threshold would make it start a dictation instead.
        let b = stop_binding(keys(&["KEY_ESC"]));
        assert_eq!(b.id, STOP_BINDING_ID);
        assert!(b.target_id.is_empty());
        assert!(b.target_ids.is_empty());
        assert_eq!(b.hold_threshold_ms, 0);
        assert!(!b.disabled);
    }
}
