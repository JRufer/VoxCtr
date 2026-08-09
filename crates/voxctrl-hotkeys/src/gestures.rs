//! Gesture recognition, independent of where the key events came from.
//!
//! Every backend — evdev on Linux, the XDG `GlobalShortcuts` portal, the
//! Win32 hook on Windows — reduces its input to the same two facts about a
//! binding's trigger: it became active, or it stopped being active. Everything
//! that decides whether that means "start recording" lives here, so the
//! gestures behave identically no matter which backend is running and can be
//! tested without a keyboard.

use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::time::Instant;

use tokio_util::sync::CancellationToken;
use voxctrl_routing::{GestureType, HotkeyBinding};

use crate::GestureSender;

/// Event emitted when a gesture is fully recognized.
#[derive(Debug, Clone)]
pub struct GestureEvent {
    pub binding_id: String,
    pub binding_label: String,
    pub target_id: String,
    pub kind: GestureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    /// Hold started
    Start,
    /// Hold released / toggle activated
    Stop,
}

/// A `double_tap_hold` that is never released — because the backend dropped the
/// release, or the key is physically stuck — would record forever. Recording
/// stops itself after this long.
const DOUBLE_TAP_HOLD_MAX: Duration = Duration::from_secs(120);

/// Shortest gap between releasing the first tap and pressing the second that
/// still counts as two deliberate taps.
///
/// This exists only to reject duplicated events from a misbehaving source; real
/// key bounce is filtered by the keyboard firmware and the kernel long before
/// VoxCtrl sees it, and evdev auto-repeat is dropped at the reader. The old
/// value was 50ms, which sits *inside* the range of a genuinely fast human
/// double-tap and so swallowed exactly the taps it was supposed to catch.
const MIN_TAP_GAP: Duration = Duration::from_millis(15);

/// Longest a first tap may be held down and still count as a tap.
///
/// Without this, using a bound modifier normally — holding Super for a second
/// while doing something else — leaves the machine primed, and the next quick
/// press registers as a double-tap the user never made.
const MAX_TAP_HOLD: Duration = Duration::from_millis(600);

// ── Trigger transitions ───────────────────────────────────────────────────────

/// What a backend reports about one binding's trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// The trigger just became fully satisfied.
    Activated,
    /// The trigger is no longer satisfied, but part of it may still be held.
    Deactivated,
    /// Every key of the trigger is up.
    ///
    /// Distinct from `Deactivated` because a combo must not finish while one of
    /// its keys is still physically down: transcription injected at that moment
    /// arrives at the compositor with, say, Super still held, and is swallowed
    /// as a shortcut instead of reaching the cursor. Backends whose triggers are
    /// atomic (the portal, which only ever reports one shortcut as a whole)
    /// report `Deactivated` and `Released` together.
    Released,
}

// ── Gesture engine ────────────────────────────────────────────────────────────

/// Owns the per-binding state machines and turns trigger transitions into
/// `GestureEvent`s.
pub struct GestureEngine {
    states: Vec<BindingState>,
    /// Binding id → index into `states`.
    index: HashMap<String, usize>,
}

impl GestureEngine {
    pub fn new(bindings: Vec<HotkeyBinding>) -> Self {
        let mut engine = Self {
            states: Vec::new(),
            index: HashMap::new(),
        };
        engine.reload(bindings);
        engine
    }

    /// Replace the bindings, abandoning any gesture in flight.
    ///
    /// Callers that may have a recording open should `reset` first; this is the
    /// plain swap used when the listener is (re)started.
    pub fn reload(&mut self, bindings: Vec<HotkeyBinding>) {
        // A `double_tap` fires as early as it possibly can — on the second
        // press — because that is the moment the gesture is unambiguous and
        // every millisecond after it is latency the user feels. The one case
        // where it cannot is when a `double_tap_hold` shares the same keys:
        // there, both gestures look identical until the second press is either
        // released quickly (a tap) or kept down (a hold), so the tap has to
        // wait for the release to tell them apart.
        let mut hold_signatures: HashSet<String> = HashSet::new();
        for b in &bindings {
            if !b.disabled && b.gesture == GestureType::DoubleTapHold {
                hold_signatures.insert(b.trigger_signature());
            }
        }

        self.states = bindings
            .into_iter()
            .map(|b| {
                let contended = hold_signatures.contains(&b.trigger_signature());
                BindingState::new(b, contended)
            })
            .collect();
        self.index = self
            .states
            .iter()
            .enumerate()
            .map(|(i, s)| (s.binding.id.clone(), i))
            .collect();
    }

    /// Bindings currently loaded, in configuration order.
    pub fn bindings(&self) -> impl Iterator<Item = &HotkeyBinding> {
        self.states.iter().map(|s| &s.binding)
    }

    pub fn apply(&mut self, binding_id: &str, transition: Transition, tx: &GestureSender) {
        self.apply_at(binding_id, transition, Instant::now(), tx);
    }

    /// `at` is the moment the transition happened, which is what the tap
    /// windows are measured against.
    pub fn apply_at(
        &mut self,
        binding_id: &str,
        transition: Transition,
        at: Instant,
        tx: &GestureSender,
    ) {
        let Some(&i) = self.index.get(binding_id) else {
            return;
        };
        // Whether a sibling `double_tap_hold` on the same keys has already
        // claimed this press has to be read before the mutable borrow.
        let signature = self.states[i].signature.clone();
        let sibling_hold_active = self.states.iter().enumerate().any(|(j, s)| {
            j != i
                && s.signature == signature
                && s.binding.gesture == GestureType::DoubleTapHold
                && s.double_tap_hold_active.load(std::sync::atomic::Ordering::SeqCst)
        });

        let state = &mut self.states[i];
        if state.binding.disabled {
            return;
        }
        match transition {
            Transition::Activated => state.on_activate(at, tx),
            Transition::Deactivated => state.on_deactivate(tx),
            Transition::Released => state.on_release(at, sibling_hold_active, tx),
        }
    }

    /// Abandon every gesture in flight, stopping any recording they started.
    ///
    /// Used when the source of key events goes away mid-gesture — a keyboard is
    /// unplugged while held, the portal session dies — because the release that
    /// would have stopped the recording is never going to arrive.
    pub fn reset(&mut self, tx: &GestureSender) {
        for state in &mut self.states {
            state.abort(tx);
        }
    }
}

// ── Per-binding state ─────────────────────────────────────────────────────────

pub struct BindingState {
    pub binding: HotkeyBinding,
    signature: String,
    /// A `double_tap_hold` shares this trigger, so `double_tap` cannot resolve
    /// on the second press.
    contended: bool,
    // Hold
    pub hold_active: Arc<AtomicBool>,
    pub hold_cancel: Option<CancellationToken>,
    // Toggle / double-tap toggle
    pub toggle_on: bool,
    // Double-tap
    pub double_tap: DoubleTapMachine,
    // Double-tap hold
    pub double_tap_hold_active: Arc<AtomicBool>,
    pub double_tap_hold_cancel: Option<CancellationToken>,
}

impl BindingState {
    pub fn new(binding: HotkeyBinding, contended: bool) -> Self {
        let tap_ms = binding.tap_ms;
        let signature = binding.trigger_signature();
        Self {
            binding,
            signature,
            contended,
            hold_active: Arc::new(AtomicBool::new(false)),
            hold_cancel: None,
            toggle_on: false,
            double_tap: DoubleTapMachine::new(Duration::from_millis(tap_ms as u64)),
            double_tap_hold_active: Arc::new(AtomicBool::new(false)),
            double_tap_hold_cancel: None,
        }
    }

    fn emit(&self, kind: GestureKind, tx: &GestureSender) {
        let _ = tx.send(GestureEvent {
            binding_id: self.binding.id.clone(),
            binding_label: self.binding.label.clone(),
            target_id: self.binding.target_ids_string(),
            kind,
        });
    }

    fn on_activate(&mut self, at: Instant, tx: &GestureSender) {
        match self.binding.gesture {
            GestureType::Hold => self.start_hold_timer(tx),
            GestureType::Toggle => {
                self.toggle_on = !self.toggle_on;
                self.emit(
                    if self.toggle_on {
                        GestureKind::Start
                    } else {
                        GestureKind::Stop
                    },
                    tx,
                );
            }
            GestureType::DoubleTap => {
                if self.double_tap.on_press(at) == TapOutcome::Completed && !self.contended {
                    self.toggle_on = !self.toggle_on;
                    self.emit(
                        if self.toggle_on {
                            GestureKind::Start
                        } else {
                            GestureKind::Stop
                        },
                        tx,
                    );
                }
            }
            GestureType::DoubleTapHold => {
                if self.double_tap.on_press(at) == TapOutcome::Completed {
                    self.start_double_tap_hold_timer(tx);
                }
            }
        }
    }

    /// The combo was broken but keys may still be down.
    fn on_deactivate(&mut self, _tx: &GestureSender) {
        // Only cancels timers that have not fired yet; a gesture that already
        // started recording ends on `Released`, once nothing is held.
        if !self.hold_active.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(cancel) = self.hold_cancel.take() {
                cancel.cancel();
            }
        }
        if !self
            .double_tap_hold_active
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            if let Some(cancel) = self.double_tap_hold_cancel.take() {
                cancel.cancel();
            }
        }
    }

    /// Every key of the trigger is up.
    fn on_release(&mut self, at: Instant, sibling_hold_active: bool, tx: &GestureSender) {
        match self.binding.gesture {
            GestureType::Hold => {
                if self.hold_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    self.hold_cancel.take();
                    self.emit(GestureKind::Stop, tx);
                } else if let Some(cancel) = self.hold_cancel.take() {
                    // Released inside the hold threshold — Start never fired.
                    cancel.cancel();
                }
            }
            GestureType::Toggle => {}
            GestureType::DoubleTap => {
                let completed = self.double_tap.on_release(at) == TapOutcome::Completed;
                // When a `double_tap_hold` shares these keys the tap resolves
                // here instead of on the press — unless the hold got there
                // first, in which case this release belongs to the hold.
                if completed && self.contended && !sibling_hold_active {
                    self.toggle_on = !self.toggle_on;
                    self.emit(
                        if self.toggle_on {
                            GestureKind::Start
                        } else {
                            GestureKind::Stop
                        },
                        tx,
                    );
                }
            }
            GestureType::DoubleTapHold => {
                self.double_tap.on_release(at);
                if let Some(cancel) = self.double_tap_hold_cancel.take() {
                    cancel.cancel();
                }
                // Deliberately not conditional on the tap machine's state. If
                // recording is running, letting go stops it — full stop.
                // Gating this on the state machine is how a desynchronised
                // machine used to leave the microphone open for two minutes.
                if self
                    .double_tap_hold_active
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    self.emit(GestureKind::Stop, tx);
                }
            }
        }
    }

    /// Tear down without waiting for a release that is never coming.
    fn abort(&mut self, tx: &GestureSender) {
        if let Some(cancel) = self.hold_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.double_tap_hold_cancel.take() {
            cancel.cancel();
        }
        // Both swaps must run — `||` would short-circuit past the second and
        // leave a double-tap-hold flagged as recording forever.
        let was_holding = self
            .hold_active
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        let was_tap_holding = self
            .double_tap_hold_active
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        let was_toggled = matches!(
            self.binding.gesture,
            GestureType::Toggle | GestureType::DoubleTap
        ) && self.toggle_on;
        let was_recording = was_holding || was_tap_holding || was_toggled;
        self.toggle_on = false;
        self.double_tap.reset();
        if was_recording {
            self.emit(GestureKind::Stop, tx);
        }
    }

    fn start_hold_timer(&mut self, tx: &GestureSender) {
        if self.hold_active.load(std::sync::atomic::Ordering::SeqCst)
            || self.hold_cancel.is_some()
        {
            return;
        }
        let cancel = CancellationToken::new();
        self.hold_cancel = Some(cancel.clone());
        let active = self.hold_active.clone();
        let event = self.pending_event(GestureKind::Start);
        let tx = tx.clone();
        let threshold = Duration::from_millis(self.binding.hold_threshold_ms as u64);
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(threshold) => {
                    active.store(true, std::sync::atomic::Ordering::SeqCst);
                    let _ = tx.send(event);
                }
                _ = cancel.cancelled() => {}
            }
        });
    }

    fn start_double_tap_hold_timer(&mut self, tx: &GestureSender) {
        // A second press while a previous cycle is still running means the
        // release went missing. Close the old cycle before opening a new one so
        // the recording state can never drift from what the user is doing.
        if let Some(cancel) = self.double_tap_hold_cancel.take() {
            cancel.cancel();
        }
        if self
            .double_tap_hold_active
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.emit(GestureKind::Stop, tx);
        }

        let cancel = CancellationToken::new();
        self.double_tap_hold_cancel = Some(cancel.clone());
        let active = self.double_tap_hold_active.clone();
        let start = self.pending_event(GestureKind::Start);
        let stop = self.pending_event(GestureKind::Stop);
        let tx = tx.clone();
        let threshold = Duration::from_millis(self.binding.hold_threshold_ms as u64);
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(threshold) => {}
                _ = cancel.cancelled() => return,
            }
            active.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = tx.send(start);

            tokio::select! {
                _ = tokio::time::sleep(DOUBLE_TAP_HOLD_MAX) => {
                    if active.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        let _ = tx.send(stop);
                    }
                }
                _ = cancel.cancelled() => {}
            }
        });
    }

    fn pending_event(&self, kind: GestureKind) -> GestureEvent {
        GestureEvent {
            binding_id: self.binding.id.clone(),
            binding_label: self.binding.label.clone(),
            target_id: self.binding.target_ids_string(),
            kind,
        }
    }
}

impl Drop for BindingState {
    fn drop(&mut self) {
        if let Some(cancel) = self.hold_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.double_tap_hold_cancel.take() {
            cancel.cancel();
        }
    }
}

// ── Double-tap state machine ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtState {
    Idle,
    FirstDown,
    FirstUp,
    SecondDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapOutcome {
    None,
    /// The second press landed inside the window (from `on_press`), or the
    /// second press was released (from `on_release`).
    Completed,
}

/// Recognises "two taps in quick succession".
///
/// Every rejection path deliberately leaves the machine in a state consistent
/// with the key actually being down or up. The previous version could bail out
/// of a press while staying in `FirstUp`, after which the physical release
/// matched no arm at all — so the machine believed a key was up that was down,
/// and the next tap behaved like the second half of a gesture the user never
/// started.
pub struct DoubleTapMachine {
    pub state: DtState,
    window: Duration,
    last_release: Option<Instant>,
    first_press: Option<Instant>,
}

impl DoubleTapMachine {
    pub fn new(window: Duration) -> Self {
        Self {
            state: DtState::Idle,
            window,
            last_release: None,
            first_press: None,
        }
    }

    pub fn on_press(&mut self, now: Instant) -> TapOutcome {
        match self.state {
            DtState::Idle => {
                self.state = DtState::FirstDown;
                self.first_press = Some(now);
                TapOutcome::None
            }
            DtState::FirstUp => {
                let gap = self
                    .last_release
                    .map(|r| now.saturating_duration_since(r))
                    .unwrap_or(Duration::MAX);
                if gap >= MIN_TAP_GAP && gap <= self.window {
                    self.state = DtState::SecondDown;
                    TapOutcome::Completed
                } else {
                    // Too fast to be real, or too slow to be a pair: this press
                    // becomes the first tap of a new gesture rather than being
                    // dropped on the floor.
                    self.state = DtState::FirstDown;
                    self.first_press = Some(now);
                    self.last_release = None;
                    TapOutcome::None
                }
            }
            // A press with no intervening release. The source repeated itself;
            // the key is already down, so nothing changes.
            DtState::FirstDown | DtState::SecondDown => TapOutcome::None,
        }
    }

    pub fn on_release(&mut self, now: Instant) -> TapOutcome {
        match self.state {
            DtState::FirstDown => {
                let held = self
                    .first_press
                    .map(|p| now.saturating_duration_since(p))
                    .unwrap_or_default();
                if held > MAX_TAP_HOLD {
                    // That was a hold, not a tap. Starting fresh stops a later
                    // quick press from completing a "double-tap" the user never
                    // began.
                    self.reset();
                } else {
                    self.state = DtState::FirstUp;
                    self.last_release = Some(now);
                }
                TapOutcome::None
            }
            DtState::SecondDown => {
                self.reset();
                TapOutcome::Completed
            }
            DtState::Idle | DtState::FirstUp => TapOutcome::None,
        }
    }

    pub fn reset(&mut self) {
        self.state = DtState::Idle;
        self.last_release = None;
        self.first_press = None;
    }
}

// ── Superset shadowing ────────────────────────────────────────────────────────

/// Given a set of currently-pressed keys and a list of bindings, return the ids
/// of bindings whose key set is a proper subset of a longer binding that is
/// also fully pressed. This prevents Meta+Space firing when Ctrl+Meta+Space is
/// held.
pub fn shadowed_by_longer(pressed: &HashSet<String>, bindings: &[HotkeyBinding]) -> HashSet<String> {
    let active: Vec<&HotkeyBinding> = bindings
        .iter()
        .filter(|b| !b.disabled)
        .filter(|b| !b.keys.is_empty())
        .filter(|b| b.keys.iter().all(|k| pressed.contains(k)))
        .collect();

    let mut shadowed = HashSet::new();
    for b in &active {
        for other in &active {
            if b.id != other.id
                && other.keys.len() > b.keys.len()
                && b.keys.iter().all(|k| other.keys.contains(k))
            {
                shadowed.insert(b.id.clone());
            }
        }
    }
    shadowed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: &str, gesture: GestureType, keys: &[&str]) -> HotkeyBinding {
        HotkeyBinding {
            id: id.to_string(),
            label: format!("{id} label"),
            keys: keys.iter().map(|k| k.to_string()).collect(),
            gesture,
            target_id: "target".to_string(),
            target_ids: vec!["target".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 100,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        }
    }

    /// Press-and-release of an atomic trigger, the way the portal reports it.
    fn tap(engine: &mut GestureEngine, id: &str, tx: &GestureSender) {
        engine.apply(id, Transition::Activated, tx);
        engine.apply(id, Transition::Deactivated, tx);
        engine.apply(id, Transition::Released, tx);
    }

    async fn sleep(ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    #[tokio::test]
    async fn double_tap_starts_on_the_second_press() {
        // The gesture is unambiguous the moment the second press lands, and
        // waiting for the release just adds latency the user feels.
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dt", &tx);
        assert!(rx.try_recv().is_err(), "one tap must not start anything");
        sleep(60).await;
        engine.apply("dt", Transition::Activated, &tx);

        let event = rx.try_recv().expect("second press starts recording");
        assert_eq!(event.binding_id, "dt");
        assert_eq!(event.kind, GestureKind::Start);
    }

    #[tokio::test]
    async fn double_tap_toggles_off_on_the_next_double_tap() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dt", &tx);
        sleep(60).await;
        tap(&mut engine, "dt", &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        sleep(60).await;
        tap(&mut engine, "dt", &tx);
        sleep(60).await;
        tap(&mut engine, "dt", &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn a_very_fast_double_tap_still_fires() {
        // Regression: the old 50ms floor rejected the second press of a fast
        // double-tap *and* left the machine mid-gesture, so the tap was lost
        // and the one after it behaved unpredictably.
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dt", &tx);
        sleep(25).await; // well under the old 50ms floor, well within human range
        engine.apply("dt", Transition::Activated, &tx);

        let event = rx.try_recv().expect("a 25ms gap is a double-tap, not bounce");
        assert_eq!(event.kind, GestureKind::Start);
    }

    #[tokio::test]
    async fn a_duplicated_press_does_not_desync_the_machine() {
        // A source that repeats an event must not leave the machine believing a
        // key is up while it is down.
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dt", &tx);
        // Duplicate of the release the backend already sent.
        engine.apply("dt", Transition::Released, &tx);
        sleep(60).await;
        engine.apply("dt", Transition::Activated, &tx);

        assert_eq!(
            rx.try_recv().expect("gesture survives a duplicated release").kind,
            GestureKind::Start
        );
    }

    #[tokio::test]
    async fn a_slow_second_tap_becomes_a_new_first_tap() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dt", &tx);
        sleep(400).await; // beyond tap_ms
        tap(&mut engine, "dt", &tx);
        assert!(rx.try_recv().is_err(), "two slow taps are not a double-tap");

        // ...but that second tap is now the first of a fresh pair.
        sleep(60).await;
        engine.apply("dt", Transition::Activated, &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);
    }

    #[tokio::test]
    async fn holding_the_key_normally_does_not_prime_a_double_tap() {
        // Super is a real modifier. Holding it for a second and then pressing it
        // again shortly after is ordinary use, not a double-tap.
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"])]);

        engine.apply("dt", Transition::Activated, &tx);
        sleep(700).await; // longer than MAX_TAP_HOLD
        engine.apply("dt", Transition::Deactivated, &tx);
        engine.apply("dt", Transition::Released, &tx);

        sleep(60).await;
        engine.apply("dt", Transition::Activated, &tx);
        assert!(
            rx.try_recv().is_err(),
            "a long hold followed by a press is not a double-tap"
        );
    }

    #[tokio::test]
    async fn double_tap_hold_records_while_held_and_stops_on_release() {
        let (tx, mut rx) = crate::channel();
        let mut engine =
            GestureEngine::new(vec![binding("dth", GestureType::DoubleTapHold, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dth", Transition::Activated, &tx);
        assert!(rx.try_recv().is_err(), "not until the hold threshold passes");

        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        engine.apply("dth", Transition::Deactivated, &tx);
        engine.apply("dth", Transition::Released, &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn double_tap_hold_stops_even_if_the_tap_machine_lost_track() {
        // The failure that leaves the microphone open: recording is running, but
        // the state machine no longer agrees a gesture is in progress. Releasing
        // must still stop it, because the user let go.
        let (tx, mut rx) = crate::channel();
        let mut engine =
            GestureEngine::new(vec![binding("dth", GestureType::DoubleTapHold, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dth", Transition::Activated, &tx);
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        // Desync the tap machine behind the engine's back.
        engine.states[0].double_tap.reset();

        engine.apply("dth", Transition::Released, &tx);
        assert_eq!(
            rx.try_recv().expect("release must always stop recording").kind,
            GestureKind::Stop
        );
    }

    #[tokio::test]
    async fn double_tap_hold_does_not_leak_a_second_recording() {
        // If the release goes missing entirely, the next double-tap-and-hold
        // must close the old recording rather than stacking a second one.
        let (tx, mut rx) = crate::channel();
        let mut engine =
            GestureEngine::new(vec![binding("dth", GestureType::DoubleTapHold, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dth", Transition::Activated, &tx);
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        // The release never arrives; the user double-taps and holds again.
        engine.states[0].double_tap.reset();
        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dth", Transition::Activated, &tx);

        assert_eq!(
            rx.try_recv().expect("stale recording is closed first").kind,
            GestureKind::Stop
        );
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);
    }

    #[tokio::test(start_paused = true)]
    async fn double_tap_hold_stops_itself_after_the_safety_timeout() {
        let (tx, mut rx) = crate::channel();
        let mut engine =
            GestureEngine::new(vec![binding("dth", GestureType::DoubleTapHold, &["KEY_LEFTMETA"])]);

        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dth", Transition::Activated, &tx);
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        sleep(121_000).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);

        engine.apply("dth", Transition::Released, &tx);
        assert!(rx.try_recv().is_err(), "no duplicate stop on the real release");
    }

    #[tokio::test]
    async fn double_tap_and_double_tap_hold_coexist_on_one_key() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![
            binding("dt", GestureType::DoubleTap, &["KEY_LEFTMETA"]),
            binding("dth", GestureType::DoubleTapHold, &["KEY_LEFTMETA"]),
        ]);

        // Quick double-tap → the tap wins, resolving on the release.
        tap(&mut engine, "dt", &tx);
        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dt", Transition::Activated, &tx);
        engine.apply("dth", Transition::Activated, &tx);
        assert!(rx.try_recv().is_err(), "the tap must wait for the release here");
        engine.apply("dt", Transition::Released, &tx);
        engine.apply("dth", Transition::Released, &tx);

        let event = rx.try_recv().unwrap();
        assert_eq!(event.binding_id, "dt");
        assert_eq!(event.kind, GestureKind::Start);
        assert!(rx.try_recv().is_err(), "the hold must not also fire");

        // Reset the tap's toggle so the next gesture starts from idle.
        engine.states[0].toggle_on = false;
        sleep(200).await;

        // Double-tap and keep it down → the hold wins.
        tap(&mut engine, "dt", &tx);
        tap(&mut engine, "dth", &tx);
        sleep(60).await;
        engine.apply("dt", Transition::Activated, &tx);
        engine.apply("dth", Transition::Activated, &tx);
        sleep(150).await;

        let event = rx.try_recv().unwrap();
        assert_eq!(event.binding_id, "dth");
        assert_eq!(event.kind, GestureKind::Start);

        engine.apply("dt", Transition::Released, &tx);
        engine.apply("dth", Transition::Released, &tx);
        let event = rx.try_recv().unwrap();
        assert_eq!(event.binding_id, "dth");
        assert_eq!(event.kind, GestureKind::Stop);
        assert!(rx.try_recv().is_err(), "the tap must stay out of the hold's way");
    }

    #[tokio::test]
    async fn hold_waits_for_the_threshold_and_stops_on_release() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("h", GestureType::Hold, &["KEY_LEFTALT"])]);

        engine.apply("h", Transition::Activated, &tx);
        assert!(rx.try_recv().is_err());
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        engine.apply("h", Transition::Released, &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn hold_released_inside_the_threshold_never_starts() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("h", GestureType::Hold, &["KEY_LEFTALT"])]);

        engine.apply("h", Transition::Activated, &tx);
        engine.apply("h", Transition::Deactivated, &tx);
        engine.apply("h", Transition::Released, &tx);
        sleep(200).await;
        assert!(rx.try_recv().is_err(), "the pending start must be cancelled");
    }

    #[tokio::test]
    async fn hold_ignores_a_partial_release_of_a_combo() {
        // Stopping while a modifier is still down gets the transcription eaten
        // by the compositor as a shortcut.
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding(
            "h",
            GestureType::Hold,
            &["KEY_LEFTMETA", "KEY_SPACE"],
        )]);

        engine.apply("h", Transition::Activated, &tx);
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        engine.apply("h", Transition::Deactivated, &tx); // Space up, Super still down
        assert!(rx.try_recv().is_err(), "must not stop while a key is held");

        engine.apply("h", Transition::Released, &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn toggle_alternates_on_each_activation() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("t", GestureType::Toggle, &["KEY_LEFTCTRL"])]);

        engine.apply("t", Transition::Activated, &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);
        engine.apply("t", Transition::Released, &tx);
        assert!(rx.try_recv().is_err());
        engine.apply("t", Transition::Activated, &tx);
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn reset_stops_a_recording_whose_release_can_never_arrive() {
        let (tx, mut rx) = crate::channel();
        let mut engine = GestureEngine::new(vec![binding("h", GestureType::Hold, &["KEY_LEFTALT"])]);

        engine.apply("h", Transition::Activated, &tx);
        sleep(150).await;
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Start);

        engine.reset(&tx); // keyboard unplugged mid-hold
        assert_eq!(rx.try_recv().unwrap().kind, GestureKind::Stop);
    }

    #[tokio::test]
    async fn disabled_bindings_never_fire() {
        let (tx, mut rx) = crate::channel();
        let mut b = binding("t", GestureType::Toggle, &["KEY_LEFTCTRL"]);
        b.disabled = true;
        let mut engine = GestureEngine::new(vec![b]);

        engine.apply("t", Transition::Activated, &tx);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn shadowing_suppresses_the_shorter_combo() {
        let bindings = vec![
            binding("short", GestureType::Hold, &["KEY_LEFTMETA", "KEY_SPACE"]),
            binding(
                "long",
                GestureType::Hold,
                &["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"],
            ),
        ];
        let pressed: HashSet<String> = ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let shadowed = shadowed_by_longer(&pressed, &bindings);
        assert!(shadowed.contains("short"));
        assert!(!shadowed.contains("long"));
    }
}
