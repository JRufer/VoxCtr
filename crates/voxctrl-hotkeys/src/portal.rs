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
    trigger::portal_trigger,
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
        // A portal shortcut is an exclusive grab. Asking for one on a key the
        // whole desktop depends on — bare Escape — would mean menus, dialogs
        // and every other app stop seeing it for as long as VoxCtrl runs, which
        // is not a trade any shortcut is worth. Leaving the group out here also
        // makes `sync_kde_shortcuts` prune the registration from KGlobalAccel,
        // so an install that already grabbed Escape releases it on next start.
        if crate::trigger::is_reserved_for_the_desktop(&b.keys) {
            tracing::warn!(
                "Not registering `{}` ({}) with the desktop: a portal shortcut is an \
                 exclusive grab, and this key has to stay available to every other app. \
                 Add a modifier — Ctrl+Escape — to use it on this backend.",
                if b.label.is_empty() { &b.id } else { &b.label },
                b.keys.join("+"),
            );
            continue;
        }
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

/// The application id VoxCtrl declares to the desktop.
///
/// Matches the Tauri bundle identifier, the `StartupWMClass` in the desktop
/// entry, and the installed `ai.voxctrl.app.desktop` file — which is how the
/// desktop's own shortcut settings find a human-readable name and icon for the
/// shortcuts registered below, instead of showing a bare D-Bus address.
pub const APP_ID: &str = "ai.voxctrl.app";

/// Tell xdg-desktop-portal who we are, before asking it for anything.
///
/// A sandboxed app gets an application id from its sandbox. A normal app on the
/// host has none, and since xdg-desktop-portal 1.20 it is expected to declare
/// one through `org.freedesktop.host.portal.Registry`. From 1.21 the
/// GlobalShortcuts portal refuses a session outright without one — that is
/// exactly the `org.freedesktop.portal.Error.NotAllowed: An app id is required`
/// a current KDE session reports.
///
/// Order matters, and is the whole reason this runs before the shortcuts proxy
/// is built: registration is allowed **once per D-Bus connection** and only
/// **before the first portal call** on it. ashpd shares one connection across
/// every portal it opens, so anything that touches a portal first can spend
/// that one chance. `register_host_app` no-ops inside a sandbox, where the id
/// already comes from elsewhere.
///
/// Returns what happened, in the user's words, so a failure is visible in the
/// setup window instead of only in a log nobody reads.
async fn register_host_app_id() -> Result<(), String> {
    let app_id = match ashpd::AppID::try_from(APP_ID) {
        Ok(id) => id,
        Err(e) => return Err(format!("`{APP_ID}` is not a usable application id: {e}")),
    };

    match ashpd::register_host_app(app_id).await {
        Ok(()) => {
            tracing::debug!("Declared `{APP_ID}` to the desktop portal");
            Ok(())
        }
        // A portal older than 1.20 does not serve this interface, and does not
        // need it: it derives the id from the process's systemd scope. The
        // registry is documented as something that may be removed again, so a
        // missing interface has to stay non-fatal.
        Err(ashpd::Error::PortalNotFound(_)) => {
            tracing::debug!(
                "This xdg-desktop-portal has no host app registry; it predates the app-id \
                 requirement, so there is nothing to declare"
            );
            Ok(())
        }
        Err(e) => Err(format!("{e}")),
    }
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
    // Before the first portal call of any kind — see `register_host_app_id`.
    let registration = register_host_app_id().await;

    let portal = GlobalShortcuts::new()
        .await
        .map_err(|e| PortalError::Unavailable(format!("{e}")))?;

    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|e| session_error(e, &registration))?;

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

/// Turn a refused session into something the user can act on.
///
/// The app-id case is worth separating: it is not "your desktop has no portal",
/// and whether VoxCtrl managed to declare an id decides whether the next step is
/// "update xdg-desktop-portal" or "here is why the declaration failed".
fn session_error(e: ashpd::Error, registration: &Result<(), String>) -> PortalError {
    let message = format!("{e}");
    if !message.contains("app id") {
        return PortalError::Unavailable(message);
    }
    PortalError::Rejected(match registration {
        Err(why) => format!(
            "{message}. VoxCtrl could not declare an application id to this desktop: {why}"
        ),
        Ok(()) => format!(
            "{message}. VoxCtrl declared itself as `{APP_ID}` and the desktop accepted the \
             declaration, then still refused the session — which points at a bug or a \
             version mismatch in xdg-desktop-portal rather than anything you configured."
        ),
    })
}

async fn bind_groups(
    portal: &GlobalShortcuts,
    session: &Session<GlobalShortcuts>,
    groups: &[ShortcutGroup],
) -> Result<Vec<BoundShortcut>, PortalError> {
    // Sync shortcut names in KDE settings and unregister any deleted shortcuts.
    sync_kde_shortcuts(groups).await;

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
        .bind_shortcuts(session, &shortcuts, None, Default::default())
        .await
        .map_err(|e| PortalError::Rejected(format!("{e}")))?;

    let response = request
        .response()
        .map_err(|e| PortalError::Rejected(format!("{e}")))?;

    Ok(describe(groups, response.shortcuts()))
}

/// Query KDE's KGlobalAccel over D-Bus to unregister deleted shortcuts, and
/// synchronize display names in `~/.config/kglobalshortcutsrc` for renamed shortcuts.
///
/// xdg-desktop-portal-kde persists registered global shortcuts in KDE's
/// KGlobalAccel (and `~/.config/kglobalshortcutsrc`), but does not clean them
/// up or update their display names when an application stops requesting or renames
/// them. Pruning deleted shortcuts and syncing descriptions ensures that KDE's shortcut
/// settings stay in sync with VoxCtrl's configured bindings, labels, and names.
async fn sync_kde_shortcuts(groups: &[ShortcutGroup]) {
    let group_details: std::collections::HashMap<&str, (&str, Option<&str>)> = groups
        .iter()
        .map(|g| (g.id.as_str(), (g.description.as_str(), g.trigger.as_deref())))
        .collect();

    // 1. Unregister deleted shortcuts over D-Bus from KGlobalAccel
    if let Ok(connection) = zbus::Connection::session().await {
        if let Ok(proxy) = zbus::Proxy::new(
            &connection,
            "org.kde.kglobalaccel",
            "/kglobalaccel",
            "org.kde.KGlobalAccel",
        )
        .await
        {
            let components = [
                "ai.voxctrl.app",
                "ai.voxctrl.app.desktop",
                "voxctrl",
                "voxctrl.desktop",
            ];

            for component in components {
                let actions: Vec<Vec<String>> = match proxy
                    .call("allActionsForComponent", &([component].as_slice(),))
                    .await
                {
                    Ok(a) => a,
                    Err(_) => continue,
                };

                for action in actions {
                    if action.len() >= 2 {
                        let shortcut_id = &action[1];
                        let is_stale = !group_details.contains_key(shortcut_id.as_str());

                        if is_stale {
                            let unregister_res: Result<bool, zbus::Error> = proxy
                                .call("unregister", &(component, shortcut_id.as_str()))
                                .await;
                            match unregister_res {
                                Ok(unregistered) => {
                                    tracing::info!(
                                        "Pruned stale KDE shortcut `{}` for component `{}` (success: {})",
                                        shortcut_id,
                                        component,
                                        unregistered
                                    );
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "Failed to unregister KDE shortcut `{}`: {e}",
                                        shortcut_id
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Synchronize descriptions in ~/.config/kglobalshortcutsrc
    update_kglobalshortcutsrc(&group_details).await;
}

async fn update_kglobalshortcutsrc(
    group_details: &std::collections::HashMap<&str, (&str, Option<&str>)>,
) {
    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => return,
    };
    let config_path = home.join(".config").join("kglobalshortcutsrc");
    if !config_path.exists() {
        return;
    }

    let content = match tokio::fs::read_to_string(&config_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Could not read kglobalshortcutsrc: {e}");
            return;
        }
    };

    let mut new_lines = Vec::new();
    let mut in_voxctrl_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = &trimmed[1..trimmed.len() - 1];
            in_voxctrl_section = matches!(
                section,
                "ai.voxctrl.app" | "ai.voxctrl.app.desktop" | "voxctrl" | "voxctrl.desktop"
            );
            new_lines.push(line.to_string());
            continue;
        }

        if in_voxctrl_section {
            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                if key.starts_with("_k_") {
                    new_lines.push(line.to_string());
                    continue;
                }
                if let Some(&(expected_desc, trigger_opt)) = group_details.get(key) {
                    let default_key = trigger_opt.unwrap_or("none");
                    let parts: Vec<&str> = val.split(',').collect();
                    if parts.len() >= 3 {
                        let cur_key = if parts[0] == "none" || parts[0].is_empty() {
                            default_key
                        } else {
                            parts[0]
                        };
                        let def_key = if parts[1] == "none" || parts[1].is_empty() {
                            default_key
                        } else {
                            parts[1]
                        };
                        new_lines.push(format!("{key}={cur_key},{def_key},{expected_desc}"));
                    } else if parts.len() == 2 {
                        let cur_key = if parts[0] == "none" || parts[0].is_empty() {
                            default_key
                        } else {
                            parts[0]
                        };
                        let def_key = if parts[1] == "none" || parts[1].is_empty() {
                            default_key
                        } else {
                            parts[1]
                        };
                        new_lines.push(format!("{key}={cur_key},{def_key},{expected_desc}"));
                    } else {
                        new_lines.push(format!("{key}={val},{expected_desc}"));
                    }
                } else {
                    // Stale or deleted shortcut - omit from file
                    tracing::debug!("Removing stale shortcut `{key}` from kglobalshortcutsrc");
                }
                continue;
            }
        }

        new_lines.push(line.to_string());
    }

    let updated_content = new_lines.join("\n") + "\n";
    if updated_content != content {
        if let Err(e) = tokio::fs::write(&config_path, updated_content).await {
            tracing::debug!("Failed to write updated kglobalshortcutsrc: {e}");
        }
    }
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
    portal: GlobalShortcuts,
    mut session: Session<GlobalShortcuts>,
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
                let new_groups = group_bindings(&new_bindings);
                let same_groups = new_groups.len() == groups.len() && new_groups.iter().zip(groups.iter()).all(|(a, b)| {
                    a.id == b.id && a.description == b.description && a.trigger == b.trigger
                });
                engine.reset(&tx);
                engine.reload(new_bindings.clone());
                groups = new_groups;
                by_shortcut = groups
                    .iter()
                    .map(|g| (g.id.clone(), g.binding_ids.clone()))
                    .collect();

                if same_groups {
                    tracing::info!("portal hotkeys: shortcut triggers unchanged; preserving active portal session");
                    continue;
                }

                // Re-creating the portal session on reload is required because
                // GlobalShortcuts portal sessions allow bind_shortcuts to be called only once per session.
                match portal.create_session(Default::default()).await {
                    Ok(new_session) => {
                        match bind_groups(&portal, &new_session, &groups).await {
                            Ok(bound) => {
                                let _ = session.close().await;
                                session = new_session;
                                health.set_bound_shortcuts(bound);
                            }
                            Err(e) => {
                                let _ = new_session.close().await;
                                tracing::warn!("Re-binding portal shortcuts failed: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create new portal session for reload: {e}");
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
    fn bare_escape_is_never_registered_with_the_compositor() {
        // The compositor grants an *exclusive* grab, so a registered Escape is
        // an Escape no other application ever receives. Dropping the group here
        // is what leaves the key alone; on KDE it also makes `sync_kde_shortcuts`
        // prune a registration an older VoxCtrl already made.
        let stop = binding("__tts_stop__", &["KEY_ESC"], GestureType::Hold);
        assert!(group_bindings(&[stop]).is_empty());
    }

    #[test]
    fn a_reserved_key_does_not_take_the_rest_of_the_bindings_with_it() {
        let groups = group_bindings(&[
            binding("__tts_stop__", &["KEY_ESC"], GestureType::Hold),
            binding("dictate", &["KEY_LEFTMETA", "KEY_SPACE"], GestureType::Hold),
            binding("stop", &["KEY_LEFTCTRL", "KEY_ESC"], GestureType::Hold),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].binding_ids, vec!["dictate"]);
        assert_eq!(groups[1].binding_ids, vec!["stop"]);
        assert_eq!(groups[1].trigger.as_deref(), Some("CTRL+Escape"));
    }

    #[test]
    fn shortcut_ids_are_stable_and_path_safe() {
        let a = group_bindings(&[binding("x", &["KEY_LEFTMETA", "KEY_SPACE"], GestureType::Hold)]);
        let b = group_bindings(&[binding("y", &["KEY_SPACE", "KEY_LEFTMETA"], GestureType::Hold)]);
        assert_eq!(a[0].id, b[0].id, "the compositor keys the user's choice off this");
        assert!(a[0].id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
