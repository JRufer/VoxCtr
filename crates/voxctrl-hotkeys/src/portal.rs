//! Global shortcuts via `org.freedesktop.portal.GlobalShortcuts`.
//!
//! This is the backend VoxCtrl wants to run on. The compositor owns the key
//! grab and hands back nothing but "your shortcut fired" — VoxCtrl never reads
//! a keyboard device, never sees a keystroke it did not register, and needs no
//! permission setup at all. Compare the evdev backend, which can only work by
//! reading every key the user types, system-wide, into this process.
//!
//! Availability is the trade-off: the portal interface needs a compositor that
//! implements it (KDE Plasma, GNOME 48+, Hyprland). Where it is missing,
//! `start` reports why and the caller falls back.

use std::{collections::HashMap, sync::Arc, time::Duration};

use ashpd::desktop::{
    global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut},
    Session,
};
use futures_util::StreamExt;
use voxctrl_routing::HotkeyBinding;

use crate::{
    gestures::{GestureEngine, Transition},
    BoundShortcut, GestureSender, ListenerHealth, ReloaderReceiver,
};

/// Why the portal backend could not be used.
#[derive(Debug, Clone)]
pub enum PortalError {
    /// No `org.freedesktop.portal.GlobalShortcuts` on the bus.
    Unavailable(String),
    /// The portal is there, but the session or the binding request failed.
    Rejected(String),
}

impl std::fmt::Display for PortalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "{e}"),
            Self::Rejected(e) => write!(f, "{e}"),
        }
    }
}

/// One system shortcut, shared by every binding that listens on the same keys.
///
/// Registering per binding instead would ask the compositor to bind one
/// accelerator twice — which it is entitled to refuse — and would break the
/// `double_tap` / `double_tap_hold` pairing that depends on both gestures
/// seeing the same press.
struct ShortcutGroup {
    id: String,
    description: String,
    trigger: Option<String>,
    binding_ids: Vec<String>,
}

fn group_bindings(bindings: &[HotkeyBinding]) -> Vec<ShortcutGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, ShortcutGroup> = HashMap::new();

    for b in bindings.iter().filter(|b| !b.disabled && !b.keys.is_empty()) {
        let signature = b.trigger_signature();
        let entry = groups.entry(signature.clone()).or_insert_with(|| {
            order.push(signature.clone());
            ShortcutGroup {
                id: shortcut_id(&signature),
                description: String::new(),
                trigger: portal_trigger(&b.keys),
                binding_ids: Vec::new(),
            }
        });
        if !entry.description.is_empty() {
            entry.description.push_str(" / ");
        }
        entry.description.push_str(if b.label.is_empty() {
            "Dictate"
        } else {
            &b.label
        });
        entry.binding_ids.push(b.id.clone());
    }

    order
        .into_iter()
        .filter_map(|s| groups.remove(&s))
        .collect()
}

/// Portal shortcut ids must be stable across restarts — the compositor keys the
/// user's chosen binding off them — and may not contain the separators the
/// portal uses in object paths.
fn shortcut_id(signature: &str) -> String {
    let slug: String = signature
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("voxctrl_{slug}")
}

/// Translate evdev key names into the accelerator syntax of the XDG shortcuts
/// specification (`CTRL+SHIFT+a`).
///
/// Returns `None` when the combination cannot be expressed — most importantly
/// for a bare modifier such as a lone Super, which is not a valid accelerator.
/// The portal treats a missing preferred trigger as "ask the user", so those
/// bindings still work; the compositor just picks the keys instead of VoxCtrl.
pub fn portal_trigger(keys: &[String]) -> Option<String> {
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
                let named = keysym_name(k)?;
                if key.is_some() {
                    // Two non-modifier keys is not an accelerator.
                    return None;
                }
                key = Some(named);
            }
        }
    }

    let key = key?;
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
    Some(out)
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

/// Bring up a portal session and start dispatching shortcuts into `tx`.
///
/// Returns once the session is bound; the listening loop runs on a spawned
/// task for the life of the process.
pub async fn start(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    rx_reload: ReloaderReceiver,
    health: Arc<ListenerHealth>,
) -> Result<(), PortalError> {
    let portal = GlobalShortcuts::new()
        .await
        .map_err(|e| PortalError::Unavailable(format!("{e}")))?;

    let session = portal
        .create_session()
        .await
        .map_err(|e| PortalError::Unavailable(format!("{e}")))?;

    let groups = group_bindings(&bindings);
    let bound = bind_groups(&portal, &session, &groups).await?;
    health.set_bound_shortcuts(bound);
    // Claimed here rather than by the caller after the await: the listener task
    // below can fail immediately, and a late `set_backend(Portal)` racing that
    // failure would report a dead session as healthy.
    health.set_backend(crate::Backend::Portal);

    tracing::info!(
        "Global shortcuts registered through the desktop portal; VoxCtrl is not reading \
         any input device"
    );

    tokio::spawn(async move {
        run(portal, session, bindings, groups, tx, rx_reload, health).await;
    });

    Ok(())
}

async fn bind_groups(
    portal: &GlobalShortcuts<'_>,
    session: &Session<'_, GlobalShortcuts<'_>>,
    groups: &[ShortcutGroup],
) -> Result<Vec<BoundShortcut>, PortalError> {
    let shortcuts: Vec<NewShortcut> = groups
        .iter()
        .map(|g| {
            NewShortcut::new(g.id.clone(), g.description.clone())
                .preferred_trigger(g.trigger.as_deref())
        })
        .collect();

    if shortcuts.is_empty() {
        return Ok(Vec::new());
    }

    let request = portal
        .bind_shortcuts(session, &shortcuts, &Default::default())
        .await
        .map_err(|e| PortalError::Rejected(format!("{e}")))?;

    let response = request
        .response()
        .map_err(|e| PortalError::Rejected(format!("{e}")))?;

    Ok(describe(groups, response.shortcuts()))
}

/// What the compositor actually bound, so the UI can show the real shortcut
/// rather than the one VoxCtrl asked for.
fn describe(groups: &[ShortcutGroup], shortcuts: &[Shortcut]) -> Vec<BoundShortcut> {
    groups
        .iter()
        .map(|g| {
            let bound = shortcuts.iter().find(|s| s.id() == g.id);
            BoundShortcut {
                binding_ids: g.binding_ids.clone(),
                requested: g.trigger.clone(),
                trigger_description: bound
                    .map(|s| s.trigger_description().to_string())
                    .unwrap_or_default(),
                bound: bound.is_some(),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn run(
    portal: GlobalShortcuts<'_>,
    session: Session<'_, GlobalShortcuts<'_>>,
    bindings: Vec<HotkeyBinding>,
    mut groups: Vec<ShortcutGroup>,
    tx: GestureSender,
    rx_reload: ReloaderReceiver,
    health: Arc<ListenerHealth>,
) {
    let mut engine = GestureEngine::new(bindings);
    let mut by_shortcut: HashMap<String, Vec<String>> = groups
        .iter()
        .map(|g| (g.id.clone(), g.binding_ids.clone()))
        .collect();

    let (mut activated, mut deactivated, mut changed) = match futures_util::try_join!(
        portal.receive_activated(),
        portal.receive_deactivated(),
        portal.receive_shortcuts_changed(),
    ) {
        Ok(streams) => streams,
        Err(e) => {
            tracing::error!("Cannot listen for portal shortcuts: {e}");
            health.set_backend_failed(format!("portal signals unavailable: {e}"));
            return;
        }
    };

    loop {
        tokio::select! {
            // Matched as Option rather than `Some(..) = ..` on purpose: a
            // stream that ends means the portal session is gone, and a pattern
            // guard would silently disable the branch and leave the loop
            // spinning on the reload poll while reporting itself healthy.
            event = activated.next() => {
                let Some(event) = event else { break };
                if let Some(ids) = by_shortcut.get(event.shortcut_id()) {
                    for id in ids {
                        engine.apply(id, Transition::Activated, &tx);
                    }
                }
            }
            event = deactivated.next() => {
                let Some(event) = event else { break };
                if let Some(ids) = by_shortcut.get(event.shortcut_id()) {
                    // A portal shortcut is atomic: there is no partial release
                    // to distinguish, so the combo ending and every key being
                    // up are the same moment.
                    for id in ids {
                        engine.apply(id, Transition::Deactivated, &tx);
                        engine.apply(id, Transition::Released, &tx);
                    }
                }
            }
            event = changed.next() => {
                let Some(event) = event else { break };
                // The user re-assigned a shortcut in the desktop's settings.
                health.set_bound_shortcuts(describe(&groups, event.shortcuts()));
            }
            new_bindings = next_reload(&rx_reload) => {
                let Some(new_bindings) = new_bindings else { break };
                tracing::info!("portal hotkeys: reloading {} bindings", new_bindings.len());
                engine.reset(&tx);
                engine.reload(new_bindings.clone());
                groups = group_bindings(&new_bindings);
                by_shortcut = groups
                    .iter()
                    .map(|g| (g.id.clone(), g.binding_ids.clone()))
                    .collect();
                match bind_groups(&portal, &session, &groups).await {
                    Ok(bound) => health.set_bound_shortcuts(bound),
                    Err(e) => {
                        tracing::warn!("Re-binding portal shortcuts failed: {e}");
                        health.set_backend_failed(format!("re-binding shortcuts failed: {e}"));
                    }
                }
            }
        }
    }

    // Losing the session means no shortcut can arrive again, and anything held
    // at that moment would otherwise record until the safety timeout.
    engine.reset(&tx);
    health.set_backend_failed("the portal session ended".to_string());
    tracing::warn!("Portal shortcut session ended; global hotkeys are inactive");
}

/// Bridge the blocking reload channel into the async select loop.
async fn next_reload(rx: &ReloaderReceiver) -> Option<Vec<HotkeyBinding>> {
    loop {
        match rx.try_recv() {
            Ok(bindings) => return Some(bindings),
            Err(crossbeam_channel::TryRecvError::Disconnected) => return None,
            Err(crossbeam_channel::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxctrl_routing::GestureType;

    fn binding(id: &str, keys: &[&str], gesture: GestureType) -> HotkeyBinding {
        HotkeyBinding {
            id: id.to_string(),
            label: id.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
            gesture,
            target_id: "t".to_string(),
            target_ids: vec!["t".to_string()],
            tap_ms: 300,
            hold_threshold_ms: 200,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        }
    }

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn combos_translate_to_accelerator_syntax() {
        assert_eq!(
            portal_trigger(&keys(&["KEY_LEFTMETA", "KEY_SPACE"])).as_deref(),
            Some("LOGO+space")
        );
        assert_eq!(
            portal_trigger(&keys(&["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"])).as_deref(),
            Some("CTRL+ALT+d")
        );
        assert_eq!(
            portal_trigger(&keys(&["KEY_F5"])).as_deref(),
            Some("F5")
        );
    }

    #[test]
    fn modifier_order_does_not_change_the_trigger() {
        assert_eq!(
            portal_trigger(&keys(&["KEY_SPACE", "KEY_LEFTMETA"])),
            portal_trigger(&keys(&["KEY_LEFTMETA", "KEY_SPACE"])),
        );
    }

    #[test]
    fn left_and_right_modifiers_are_the_same_accelerator() {
        assert_eq!(
            portal_trigger(&keys(&["KEY_RIGHTCTRL", "KEY_A"])).as_deref(),
            Some("CTRL+a")
        );
    }

    #[test]
    fn a_bare_modifier_has_no_accelerator() {
        // Not a failure: the portal asks the user to choose instead, which is
        // the only way a lone Super can ever be a global shortcut.
        assert!(portal_trigger(&keys(&["KEY_LEFTMETA"])).is_none());
        assert!(portal_trigger(&keys(&["KEY_LEFTCTRL", "KEY_LEFTSHIFT"])).is_none());
    }

    #[test]
    fn two_non_modifier_keys_have_no_accelerator() {
        assert!(portal_trigger(&keys(&["KEY_A", "KEY_B"])).is_none());
    }

    #[test]
    fn bindings_sharing_keys_share_one_system_shortcut() {
        // Both gestures must see the same press, and the compositor must not be
        // asked to bind one accelerator twice.
        let groups = group_bindings(&[
            binding("tap", &["KEY_LEFTMETA"], GestureType::DoubleTap),
            binding("hold", &["KEY_LEFTMETA"], GestureType::DoubleTapHold),
            binding("other", &["KEY_LEFTCTRL", "KEY_D"], GestureType::Hold),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].binding_ids, vec!["tap", "hold"]);
        assert_eq!(groups[1].binding_ids, vec!["other"]);
        assert!(groups[0].description.contains("tap"));
        assert!(groups[0].description.contains("hold"));
    }

    #[test]
    fn disabled_bindings_are_not_registered_with_the_compositor() {
        let mut disabled = binding("off", &["KEY_LEFTCTRL", "KEY_D"], GestureType::Hold);
        disabled.disabled = true;
        assert!(group_bindings(&[disabled]).is_empty());
    }

    #[test]
    fn shortcut_ids_are_stable_and_path_safe() {
        let a = group_bindings(&[binding("x", &["KEY_LEFTMETA", "KEY_SPACE"], GestureType::Hold)]);
        let b = group_bindings(&[binding("y", &["KEY_SPACE", "KEY_LEFTMETA"], GestureType::Hold)]);
        assert_eq!(a[0].id, b[0].id, "the compositor keys the user's choice off this");
        assert!(a[0].id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
