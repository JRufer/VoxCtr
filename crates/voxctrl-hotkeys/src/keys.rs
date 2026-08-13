//! Turns a stream of raw key presses and releases into per-binding trigger
//! transitions.
//!
//! Only the backends that see individual keys need this — evdev on Linux and
//! the Win32 hook on Windows. The portal backend is handed whole shortcuts by
//! the compositor and talks to the gesture engine directly.

use std::collections::HashSet;

use voxctrl_routing::HotkeyBinding;

use crate::gestures::{shadowed_by_longer, Transition};

/// Tracks which keys are down and which bindings that satisfies.
pub struct KeyMatcher {
    bindings: Vec<HotkeyBinding>,
    pressed: HashSet<String>,
    active: HashSet<String>,
}

impl KeyMatcher {
    pub fn new(bindings: Vec<HotkeyBinding>) -> Self {
        Self {
            bindings,
            pressed: HashSet::new(),
            active: HashSet::new(),
        }
    }

    /// Swap in new bindings. The caller is expected to have reset the gesture
    /// engine, so no trigger is considered active afterwards.
    pub fn reload(&mut self, bindings: Vec<HotkeyBinding>) {
        self.bindings = bindings;
        self.pressed.clear();
        self.active.clear();
    }

    /// Every trigger that is currently active, as `Released` transitions.
    ///
    /// Used when the key source disappears: whatever is held will never be seen
    /// coming back up.
    pub fn clear(&mut self) -> Vec<(String, Transition)> {
        let mut out = Vec::new();
        for id in std::mem::take(&mut self.active) {
            out.push((id.clone(), Transition::Deactivated));
            out.push((id, Transition::Released));
        }
        self.pressed.clear();
        out
    }

    pub fn on_key(&mut self, key: &str, down: bool) -> Vec<(String, Transition)> {
        if down {
            self.pressed.insert(key.to_string());
            self.on_press(key)
        } else {
            let out = self.on_release(key);
            self.pressed.remove(key);
            out
        }
    }

    fn on_press(&mut self, key: &str) -> Vec<(String, Transition)> {
        // Shadowing is resolved at press time only. Deliberately: if it were
        // re-evaluated on release, letting go of Ctrl during Ctrl+Super+Space
        // would "un-shadow" Super+Space and start a second recording out of a
        // gesture the user was in the middle of ending.
        let shadowed = shadowed_by_longer(&self.pressed, &self.bindings);

        let mut out = Vec::new();
        for b in &self.bindings {
            if b.disabled || b.keys.is_empty() || self.active.contains(&b.id) {
                continue;
            }
            if shadowed.contains(&b.id) {
                continue;
            }
            // The key just pressed has to be part of the combo — otherwise a
            // binding would activate on an unrelated key merely because its own
            // keys happened to still be down.
            if !b.keys.iter().any(|k| k == key) {
                continue;
            }
            if !b.keys.iter().all(|k| self.pressed.contains(k)) {
                continue;
            }
            self.active.insert(b.id.clone());
            out.push((b.id.clone(), Transition::Activated));
        }
        out
    }

    fn on_release(&mut self, key: &str) -> Vec<(String, Transition)> {
        let mut out = Vec::new();
        let mut ended = Vec::new();

        for b in &self.bindings {
            if !self.active.contains(&b.id) {
                continue;
            }
            if !b.keys.iter().any(|k| k == key) {
                continue;
            }
            out.push((b.id.clone(), Transition::Deactivated));

            // `pressed` still holds the key being released — the caller removes
            // it once this returns — so it has to be excluded explicitly.
            let others_held = b
                .keys
                .iter()
                .any(|k| k.as_str() != key && self.pressed.contains(k));
            if !others_held {
                out.push((b.id.clone(), Transition::Released));
                ended.push(b.id.clone());
            }
        }

        for id in ended {
            self.active.remove(&id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxctrl_routing::GestureType;

    fn binding(id: &str, keys: &[&str]) -> HotkeyBinding {
        HotkeyBinding {
            id: id.to_string(),
            label: id.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
            gesture: GestureType::Hold,
            target_id: "t".to_string(),
            target_ids: vec!["t".to_string()],
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

    #[test]
    fn a_combo_activates_only_once_every_key_is_down() {
        let mut m = KeyMatcher::new(vec![binding("b", &["KEY_LEFTMETA", "KEY_SPACE"])]);

        assert!(m.on_key("KEY_LEFTMETA", true).is_empty());
        assert_eq!(
            m.on_key("KEY_SPACE", true),
            vec![("b".to_string(), Transition::Activated)]
        );
    }

    #[test]
    fn a_combo_reports_release_only_when_the_last_key_comes_up() {
        let mut m = KeyMatcher::new(vec![binding("b", &["KEY_LEFTMETA", "KEY_SPACE"])]);
        m.on_key("KEY_LEFTMETA", true);
        m.on_key("KEY_SPACE", true);

        assert_eq!(
            m.on_key("KEY_SPACE", false),
            vec![("b".to_string(), Transition::Deactivated)]
        );
        assert_eq!(
            m.on_key("KEY_LEFTMETA", false),
            vec![
                ("b".to_string(), Transition::Deactivated),
                ("b".to_string(), Transition::Released),
            ]
        );
    }

    #[test]
    fn an_unrelated_key_does_not_activate_a_held_combo() {
        // Super is down for other reasons; pressing an unrelated key must not
        // count as completing a binding that only needs Super.
        let mut m = KeyMatcher::new(vec![binding("b", &["KEY_LEFTMETA"])]);
        m.on_key("KEY_LEFTMETA", true);
        assert!(m.on_key("KEY_T", true).is_empty());
    }

    #[test]
    fn a_longer_combo_shadows_a_shorter_one() {
        let mut m = KeyMatcher::new(vec![
            binding("short", &["KEY_LEFTMETA", "KEY_SPACE"]),
            binding("long", &["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]),
        ]);

        m.on_key("KEY_LEFTCTRL", true);
        m.on_key("KEY_LEFTMETA", true);
        let out = m.on_key("KEY_SPACE", true);
        assert_eq!(out, vec![("long".to_string(), Transition::Activated)]);
    }

    #[test]
    fn releasing_a_shadowing_key_does_not_start_the_shorter_combo() {
        // Regression guard: re-deriving shadowing on release would activate
        // Super+Space here, in the middle of ending Ctrl+Super+Space.
        let mut m = KeyMatcher::new(vec![
            binding("short", &["KEY_LEFTMETA", "KEY_SPACE"]),
            binding("long", &["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]),
        ]);
        m.on_key("KEY_LEFTCTRL", true);
        m.on_key("KEY_LEFTMETA", true);
        m.on_key("KEY_SPACE", true);

        let out = m.on_key("KEY_LEFTCTRL", false);
        assert_eq!(out, vec![("long".to_string(), Transition::Deactivated)]);
    }

    #[test]
    fn clearing_releases_everything_still_held() {
        let mut m = KeyMatcher::new(vec![binding("b", &["KEY_LEFTALT"])]);
        m.on_key("KEY_LEFTALT", true);

        assert_eq!(
            m.clear(),
            vec![
                ("b".to_string(), Transition::Deactivated),
                ("b".to_string(), Transition::Released),
            ]
        );
        assert!(m.clear().is_empty());
    }

    #[test]
    fn a_repeated_press_does_not_reactivate_an_active_binding() {
        let mut m = KeyMatcher::new(vec![binding("b", &["KEY_LEFTALT"])]);
        assert_eq!(m.on_key("KEY_LEFTALT", true).len(), 1);
        assert!(m.on_key("KEY_LEFTALT", true).is_empty());
    }
}
