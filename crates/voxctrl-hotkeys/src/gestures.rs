use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::time::Instant;

use tokio_util::sync::CancellationToken;
use voxctrl_routing::HotkeyBinding;

/// Event emitted when a gesture is fully recognized.
#[derive(Debug, Clone)]
pub struct GestureEvent {
    pub binding_id: String,
    pub binding_label: String,
    pub target_id: String,
    pub kind: GestureKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestureKind {
    /// Hold started
    Start,
    /// Hold released / toggle activated
    Stop,
}

// ── State machines ────────────────────────────────────────────────────────────

/// Per-binding mutable state.
pub struct BindingState {
    pub binding: HotkeyBinding,
    // Hold
    pub hold_active: Arc<AtomicBool>,
    pub hold_cancel: Option<CancellationToken>,
    // Toggle
    pub toggle_on: bool,
    // Double-tap
    pub double_tap: DoubleTapMachine,
    // Double-tap hold
    pub double_tap_hold_active: Arc<AtomicBool>,
    pub double_tap_hold_cancel: Option<CancellationToken>,
    pub double_tap_hold_triggered: Arc<AtomicBool>,
    // Chord
    pub chord_active: bool,
}

impl BindingState {
    pub fn new(binding: HotkeyBinding) -> Self {
        let tap_ms = binding.tap_ms;
        Self {
            binding,
            hold_active: Arc::new(AtomicBool::new(false)),
            hold_cancel: None,
            toggle_on: false,
            double_tap: DoubleTapMachine::new(Duration::from_millis(tap_ms as u64)),
            double_tap_hold_active: Arc::new(AtomicBool::new(false)),
            double_tap_hold_cancel: None,
            double_tap_hold_triggered: Arc::new(AtomicBool::new(false)),
            chord_active: false,
        }
    }
}

impl Drop for BindingState {
    fn drop(&mut self) {
        if let Some(cancel) = self.hold_cancel.take() {
            cancel.cancel();
        }
    }
}

// ── Double-tap state machine ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DtState {
    Idle,
    FirstDown,
    FirstUp,
    SecondDown,
}

pub struct DoubleTapMachine {
    pub state: DtState,
    deadline: Duration,
    last_up: Option<Instant>,
}

impl DoubleTapMachine {
    pub fn new(deadline: Duration) -> Self {
        Self {
            state: DtState::Idle,
            deadline,
            last_up: None,
        }
    }

    /// Returns true when a valid double-tap is completed.
    pub fn on_press(&mut self) -> bool {
        match self.state {
            DtState::Idle => {
                self.state = DtState::FirstDown;
                false
            }
            DtState::FirstUp => {
                if let Some(up) = self.last_up {
                    let elapsed = up.elapsed();
                    if elapsed < Duration::from_millis(50) {
                        // Ignore key bounce / simulated repeat press events
                        return false;
                    }
                    if elapsed <= self.deadline {
                        self.state = DtState::SecondDown;
                        return true;
                    }
                }
                self.state = DtState::FirstDown;
                false
            }
            _ => false,
        }
    }

    pub fn on_release(&mut self) -> bool {
        match self.state {
            DtState::FirstDown => {
                self.state = DtState::FirstUp;
                self.last_up = Some(Instant::now());
                false
            }
            DtState::SecondDown => {
                self.state = DtState::Idle;
                true
            }
            _ => false,
        }
    }

    pub fn reset(&mut self) {
        self.state = DtState::Idle;
        self.last_up = None;
    }
}

// ── Superset shadowing ────────────────────────────────────────────────────────

/// Given a set of currently-pressed keys and a list of sorted bindings,
/// filter out bindings whose key set is a proper subset of a longer binding
/// that is also active. This prevents Meta+Space firing when Ctrl+Meta+Space
/// is held.
pub fn shadowed_by_longer(
    pressed: &HashSet<String>,
    bindings: &[BindingState],
) -> HashSet<String> {
    let active: Vec<&BindingState> = bindings
        .iter()
        .filter(|b| !b.binding.disabled)
        .filter(|b| b.binding.keys.iter().all(|k| pressed.contains(k)))
        .collect();

    let mut shadowed = HashSet::new();
    for b in &active {
        for other in &active {
            if b.binding.id != other.binding.id
                && other.binding.keys.len() > b.binding.keys.len()
                && b.binding.keys.iter().all(|k| other.binding.keys.contains(k))
            {
                shadowed.insert(b.binding.id.clone());
            }
        }
    }
    shadowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxctrl_routing::GestureType;

    #[test]
    fn test_binding_state_hold_init() {
        let binding = HotkeyBinding {
            id: "test".to_string(),
            label: "Test".to_string(),
            keys: vec!["KEY_LEFTMETA".to_string(), "KEY_SPACE".to_string()],
            gesture: GestureType::Hold,
            target_id: "target".to_string(),
            target_ids: vec!["target".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 200,
            subkey: None,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        };

        let state = BindingState::new(binding);
        assert!(!state.hold_active.load(std::sync::atomic::Ordering::SeqCst));
        assert!(state.hold_cancel.is_none());
    }

    #[tokio::test]
    async fn test_cancellation_on_drop() {
        let binding = HotkeyBinding {
            id: "test".to_string(),
            label: "Test".to_string(),
            keys: vec!["KEY_LEFTMETA".to_string(), "KEY_SPACE".to_string()],
            gesture: GestureType::Hold,
            target_id: "target".to_string(),
            target_ids: vec!["target".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 200,
            subkey: None,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        };

        let cancel = CancellationToken::new();
        {
            let mut state = BindingState::new(binding);
            state.hold_cancel = Some(cancel.clone());
            assert!(!cancel.is_cancelled());
        }
        // State dropped, cancel should be triggered
        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn test_double_tap_debounce() {
        let mut machine = DoubleTapMachine::new(Duration::from_millis(300));
        
        // First tap: press and release
        assert!(!machine.on_press());
        assert!(!machine.on_release());
        
        // Immediate second press (e.g. bounce, 10ms later) should be debounced/ignored
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!machine.on_press());
        
        // If we wait longer (e.g. 100ms), it should successfully trigger
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(machine.on_press());
    }

    #[tokio::test]
    async fn test_double_tap_toggle_state() {
        let binding = HotkeyBinding {
            id: "test".to_string(),
            label: "Test".to_string(),
            keys: vec!["KEY_LEFTCTRL".to_string()],
            gesture: GestureType::DoubleTap,
            target_id: "target".to_string(),
            target_ids: vec!["target".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 200,
            subkey: None,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        };

        let mut state = BindingState::new(binding);
        assert!(!state.toggle_on);
        
        // Simulating the flow of double-tap press and release
        // First press:
        assert!(!state.double_tap.on_press());
        assert!(!state.toggle_on);
        
        // First release:
        assert!(!state.double_tap.on_release());
        assert!(!state.toggle_on);
        
        // Sleep to avoid debouncing (needs > 50ms)
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Second press (completed double tap):
        assert!(state.double_tap.on_press());
        // Since it's completed, in handle_press we would toggle:
        state.toggle_on = !state.toggle_on;
        assert!(state.toggle_on);
        
        // Second release:
        assert!(state.double_tap.on_release());
        // State machine resets to Idle, but toggle_on remains true
        assert!(state.toggle_on);
        
        // Sleep to avoid debouncing for the next double-tap sequence
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Next double tap:
        // First press:
        assert!(!state.double_tap.on_press());
        // First release:
        assert!(!state.double_tap.on_release());

        // Sleep to avoid debouncing
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Second press (completed double tap):
        assert!(state.double_tap.on_press());
        state.toggle_on = !state.toggle_on;
        assert!(!state.toggle_on); // Toggled off!
    }
}
