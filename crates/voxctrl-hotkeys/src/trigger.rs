//! Translating VoxCtrl key combinations into desktop shortcut accelerators.
//!
//! This is the single definition of what VoxCtrl can ask a desktop to bind,
//! following the accelerator syntax of the XDG shortcuts specification
//! (`CTRL+SHIFT+a`). The portal backend uses it to register shortcuts and the
//! settings UI validates against it over IPC, so what the key recorder accepts
//! and what a compositor can actually bind cannot drift apart.
//!
//! Compiled on every platform, not just Linux: Windows has no such restriction
//! at runtime, but the rules still describe what a binding must look like to
//! survive a move to a portal desktop.

/// Why a key combination cannot be a portal shortcut.
///
/// Carried rather than collapsed into `None` so the settings UI can tell the
/// user *which* rule they hit, and what to press instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerProblem {
    /// Nothing was captured.
    Empty,
    /// Modifiers only. A lone Super, or Ctrl+Shift with no other key, is not a
    /// valid accelerator: the compositor has nothing to bind.
    ModifiersOnly,
    /// More than one non-modifier key. An accelerator is modifiers plus exactly
    /// one key.
    MultipleKeys,
    /// A key with no keysym equivalent in the shortcuts specification.
    UnsupportedKey(String),
}

impl std::fmt::Display for TriggerProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "no keys were captured"),
            Self::ModifiersOnly => write!(
                f,
                "a shortcut needs at least one regular key — modifiers on their own \
                 (Super, Ctrl, Alt, Shift) cannot be registered with your desktop"
            ),
            Self::MultipleKeys => write!(
                f,
                "a shortcut is any number of modifiers plus exactly one regular key"
            ),
            Self::UnsupportedKey(k) => write!(
                f,
                "{} has no equivalent in the desktop shortcut specification",
                k.trim_start_matches("KEY_")
            ),
        }
    }
}

/// Translate evdev key names into the accelerator syntax of the XDG shortcuts
/// specification (`CTRL+SHIFT+a`).
///
/// This is the single definition of what VoxCtrl can ask a desktop to bind. The
/// settings UI validates against it through an IPC command rather than
/// reimplementing the rules, so what the recorder accepts and what the portal
/// can register cannot drift apart.
pub fn accelerator(keys: &[String]) -> Result<String, TriggerProblem> {
    if keys.is_empty() {
        return Err(TriggerProblem::Empty);
    }

    let mut modifiers: Vec<&str> = Vec::new();
    let mut key: Option<String> = None;

    for k in keys {
        match modifier_name(k) {
            Some(m) => {
                if !modifiers.contains(&m) {
                    modifiers.push(m);
                }
            }
            None => {
                let named =
                    keysym_name(k).ok_or_else(|| TriggerProblem::UnsupportedKey(k.clone()))?;
                if key.is_some() {
                    return Err(TriggerProblem::MultipleKeys);
                }
                key = Some(named);
            }
        }
    }

    let key = key.ok_or(TriggerProblem::ModifiersOnly)?;
    // Canonical order, so the same combo always produces the same string.
    let mut parts: Vec<&str> = Vec::new();
    for m in ["CTRL", "ALT", "SHIFT", "LOGO"] {
        if modifiers.contains(&m) {
            parts.push(m);
        }
    }
    let mut out = parts.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&key);
    Ok(out)
}

/// The preferred trigger to hand the portal, or `None` when the combination
/// cannot be expressed.
///
/// `None` is not fatal at registration time: the portal reads a missing
/// preferred trigger as "ask the user", which is what keeps a binding saved by
/// an older VoxCtrl working instead of vanishing.
pub fn portal_trigger(keys: &[String]) -> Option<String> {
    accelerator(keys).ok()
}

fn modifier_name(key: &str) -> Option<&'static str> {
    match key {
        "KEY_LEFTCTRL" | "KEY_RIGHTCTRL" => Some("CTRL"),
        "KEY_LEFTALT" | "KEY_RIGHTALT" => Some("ALT"),
        "KEY_LEFTSHIFT" | "KEY_RIGHTSHIFT" => Some("SHIFT"),
        "KEY_LEFTMETA" | "KEY_RIGHTMETA" => Some("LOGO"),
        _ => None,
    }
}

/// evdev name → XKB keysym name, as the shortcuts specification expects.
fn keysym_name(key: &str) -> Option<String> {
    let name = key.strip_prefix("KEY_")?;
    let sym = match name {
        "SPACE" => "space".to_string(),
        "ENTER" | "KPENTER" => "Return".to_string(),
        "TAB" => "Tab".to_string(),
        "ESC" | "ESCAPE" => "Escape".to_string(),
        "BACKSPACE" => "BackSpace".to_string(),
        "DELETE" => "Delete".to_string(),
        "INSERT" => "Insert".to_string(),
        "HOME" => "Home".to_string(),
        "END" => "End".to_string(),
        "PAGEUP" => "Prior".to_string(),
        "PAGEDOWN" => "Next".to_string(),
        "UP" => "Up".to_string(),
        "DOWN" => "Down".to_string(),
        "LEFT" => "Left".to_string(),
        "RIGHT" => "Right".to_string(),
        "MINUS" => "minus".to_string(),
        "EQUAL" => "equal".to_string(),
        "COMMA" => "comma".to_string(),
        "DOT" => "period".to_string(),
        "SLASH" => "slash".to_string(),
        "SEMICOLON" => "semicolon".to_string(),
        "APOSTROPHE" => "apostrophe".to_string(),
        "GRAVE" => "grave".to_string(),
        "BACKSLASH" => "backslash".to_string(),
        "LEFTBRACE" => "bracketleft".to_string(),
        "RIGHTBRACE" => "bracketright".to_string(),
        "CAPSLOCK" => "Caps_Lock".to_string(),
        _ => {
            if let Some(n) = name.strip_prefix('F') {
                if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                    return Some(format!("F{n}"));
                }
            }
            if name.len() == 1 {
                let c = name.chars().next()?;
                if c.is_ascii_alphabetic() {
                    return Some(c.to_ascii_lowercase().to_string());
                }
                if c.is_ascii_digit() {
                    return Some(c.to_string());
                }
            }
            return None;
        }
    };
    Some(sym)
}

/// Keys VoxCtrl will not hold a *standing* grab on, in every spelling a saved
/// config might use.
///
/// Registering a global shortcut — through the XDG shortcuts portal, or through
/// Cinnamon's own keybinding settings — asks the desktop for an *exclusive*
/// grab. The compositor then routes that key to VoxCtrl and to nothing else:
/// the menu that was open does not close, the dialog does not cancel, the app
/// underneath is never told the key was pressed. For a combination the user
/// deliberately reserved for VoxCtrl (Super+Space, Ctrl+Alt+D) that is exactly
/// what they asked for. For Escape it never is — Escape is how every program on
/// the machine says "never mind", and it is VoxCtrl's default TTS stop key, so
/// users who never picked it would lose it desktop-wide.
///
/// The app answers this by holding the grab only for the seconds it is actually
/// speaking (`stop_key::StopKeyGrab::WhileSpeaking`), so Escape interrupts
/// playback and belongs to the rest of the desktop the rest of the time. This
/// list is what marks a key as needing that treatment.
///
/// Both spellings are here on purpose: the Rust config canonicalises to
/// `KEY_ESC`, but a config written by an older build — or by the frontend's own
/// default — can still say `KEY_ESCAPE`.
pub const RESERVED_KEYS: &[&str] = &["KEY_ESC", "KEY_ESCAPE"];

/// Would a standing grab on these keys take something the rest of the desktop
/// needs?
///
/// Only bare, unmodified combinations count. `Ctrl+Escape` grabs a combination
/// nothing else is listening for, so it is an ordinary shortcut and is
/// registered for the life of the process like any other.
pub fn is_reserved_for_the_desktop(keys: &[String]) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|k| RESERVED_KEYS.contains(&k.to_ascii_uppercase().as_str()))
}

/// True for a key that only ever acts as a modifier, so it cannot be the one
/// regular key an accelerator needs.
pub fn is_modifier(key: &str) -> bool {
    modifier_name(key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn combos_translate_to_accelerator_syntax() {
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTMETA", "KEY_SPACE"])).unwrap(),
            "LOGO+space"
        );
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"])).unwrap(),
            "CTRL+ALT+d"
        );
        assert_eq!(accelerator(&keys(&["KEY_F5"])).unwrap(), "F5");
    }

    #[test]
    fn modifier_order_does_not_change_the_trigger() {
        assert_eq!(
            accelerator(&keys(&["KEY_SPACE", "KEY_LEFTMETA"])),
            accelerator(&keys(&["KEY_LEFTMETA", "KEY_SPACE"])),
        );
    }

    #[test]
    fn left_and_right_modifiers_are_the_same_accelerator() {
        assert_eq!(
            accelerator(&keys(&["KEY_RIGHTCTRL", "KEY_A"])).unwrap(),
            "CTRL+a"
        );
    }

    #[test]
    fn a_bare_modifier_is_rejected_with_a_reason() {
        // The case the settings UI has to catch: a lone Super looks like a
        // perfectly good hotkey to a user, and no desktop can bind it.
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTMETA"])),
            Err(TriggerProblem::ModifiersOnly)
        );
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTCTRL", "KEY_LEFTSHIFT"])),
            Err(TriggerProblem::ModifiersOnly)
        );
        assert_eq!(
            accelerator(&keys(&["KEY_RIGHTMETA"])),
            Err(TriggerProblem::ModifiersOnly)
        );
    }

    #[test]
    fn bare_escape_is_a_valid_accelerator_that_may_only_be_held_transiently() {
        // Escape *is* something a desktop can bind, and VoxCtrl does bind it —
        // that is how the stop key interrupts playback. What it must not do is
        // hold the grab while it is not speaking, because the grab is exclusive
        // and an open menu would stop closing anywhere on the machine.
        assert_eq!(accelerator(&keys(&["KEY_ESC"])).unwrap(), "Escape");
        assert!(is_reserved_for_the_desktop(&keys(&["KEY_ESC"])));
        assert!(is_reserved_for_the_desktop(&keys(&["KEY_ESCAPE"])));
    }

    #[test]
    fn escape_with_a_modifier_is_an_ordinary_shortcut() {
        // Ctrl+Escape takes nothing from the desktop: no app is listening for
        // the combination, so it is registered for the life of the process and
        // needs none of the arming machinery.
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTCTRL", "KEY_ESC"])).unwrap(),
            "CTRL+Escape"
        );
        let ctrl_escape = keys(&["KEY_LEFTCTRL", "KEY_ESC"]);
        assert!(!is_reserved_for_the_desktop(&ctrl_escape));
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTMETA", "KEY_ESCAPE"])).unwrap(),
            "LOGO+Escape"
        );
    }

    #[test]
    fn only_escape_is_reserved() {
        // The rule is deliberately one key wide. A bare F5 or a bare Space is a
        // combination the user picked on purpose, and taking it back between
        // utterances would break setups that work today.
        for k in ["KEY_F5", "KEY_SPACE", "KEY_A", "KEY_TAB", "KEY_ENTER"] {
            assert!(
                !is_reserved_for_the_desktop(&keys(&[k])),
                "{k} must stay an ordinary standing shortcut"
            );
        }
        assert!(!is_reserved_for_the_desktop(&[]));
    }

    #[test]
    fn two_regular_keys_are_rejected() {
        assert_eq!(
            accelerator(&keys(&["KEY_A", "KEY_B"])),
            Err(TriggerProblem::MultipleKeys)
        );
        assert_eq!(
            accelerator(&keys(&["KEY_LEFTCTRL", "KEY_A", "KEY_B"])),
            Err(TriggerProblem::MultipleKeys)
        );
    }

    #[test]
    fn an_empty_capture_is_rejected() {
        assert_eq!(accelerator(&[]), Err(TriggerProblem::Empty));
    }

    #[test]
    fn an_unmappable_key_names_itself() {
        let err = accelerator(&keys(&["KEY_FN_F1"])).unwrap_err();
        assert_eq!(err, TriggerProblem::UnsupportedKey("KEY_FN_F1".into()));
        // The message must name the key without the evdev prefix, which means
        // nothing to a user reading a settings dialog.
        assert!(err.to_string().contains("FN_F1"));
        assert!(!err.to_string().contains("KEY_FN_F1"));
    }

    #[test]
    fn every_problem_explains_itself_without_jargon() {
        for problem in [
            TriggerProblem::Empty,
            TriggerProblem::ModifiersOnly,
            TriggerProblem::MultipleKeys,
            TriggerProblem::UnsupportedKey("KEY_ZZZ".into()),
        ] {
            let msg = problem.to_string();
            assert!(!msg.is_empty());
            assert!(
                !msg.contains("accelerator") || matches!(problem, TriggerProblem::MultipleKeys),
                "user-facing text should avoid spec jargon: {msg}"
            );
        }
    }

    #[test]
    fn modifiers_are_recognised_on_both_sides_of_the_keyboard() {
        for k in [
            "KEY_LEFTCTRL", "KEY_RIGHTCTRL", "KEY_LEFTALT", "KEY_RIGHTALT",
            "KEY_LEFTSHIFT", "KEY_RIGHTSHIFT", "KEY_LEFTMETA", "KEY_RIGHTMETA",
        ] {
            assert!(is_modifier(k), "{k} must count as a modifier");
        }
        assert!(!is_modifier("KEY_SPACE"));
        assert!(!is_modifier("KEY_CAPSLOCK"), "Caps Lock is bindable as a key");
    }

    #[test]
    fn the_bindings_voxctrl_ships_with_are_all_bindable() {
        // A default the settings UI would refuse to save is a default that
        // cannot work on the backend VoxCtrl prefers.
        for binding in voxctrl_routing::loader::default_bindings() {
            assert!(
                accelerator(&binding.keys).is_ok(),
                "default binding `{}` is not a valid shortcut: {:?}",
                binding.id,
                accelerator(&binding.keys)
            );
        }
    }

    #[test]
    fn portal_trigger_collapses_problems_to_none() {
        assert_eq!(portal_trigger(&keys(&["KEY_LEFTMETA"])), None);
        assert_eq!(
            portal_trigger(&keys(&["KEY_LEFTMETA", "KEY_SPACE"])).as_deref(),
            Some("LOGO+space")
        );
    }
}
