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
    // Unregister any stale shortcuts or shortcuts whose description (label) has changed
    // so KDE KGlobalAccel updates the action description when re-binding.
    prune_stale_kde_shortcuts(groups).await;

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

/// Query KDE's KGlobalAccel over D-Bus and unregister any VoxCtrl shortcuts
/// that are no longer part of the active shortcut groups or whose label/description
/// has changed.
///
/// xdg-desktop-portal-kde persists registered global shortcuts in KDE's
/// KGlobalAccel (and `~/.config/kglobalshortcutsrc`), but does not clean them
/// up or update their display names when an application stops requesting or renames
/// them. Pruning stale and renamed shortcuts here ensures that KDE's shortcut
/// settings stay in sync with VoxCtrl's configured bindings, labels, and names.
async fn prune_stale_kde_shortcuts(groups: &[ShortcutGroup]) {
    let group_descriptions: std::collections::HashMap<&str, &str> = groups
        .iter()
        .map(|g| (g.id.as_str(), g.description.as_str()))
        .collect();

    let connection = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Could not connect to session D-Bus to check KDE shortcuts: {e}");
            return;
        }
    };

    let proxy = match zbus::Proxy::new(
        &connection,
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel",
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!("KGlobalAccel proxy unavailable (not running on KDE?): {e}");
            return;
        }
    };

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
                let should_remove = match group_descriptions.get(shortcut_id.as_str()) {
                    None => true, // Shortcut was deleted or disabled in the app
                    Some(expected_desc) => {
                        // If description in KDE differs from current group description (renamed),
                        // unregister it so portal re-registers it with the new name.
                        if action.len() >= 4 {
                            let current_desc = &action[3];
                            current_desc != *expected_desc
                        } else {
                            false
                        }
                    }
                };

                if should_remove {
                    let unregister_res: Result<bool, zbus::Error> = proxy
                        .call("unregister", &(component, shortcut_id.as_str()))
                        .await;
                    match unregister_res {
                        Ok(unregistered) => {
                            tracing::info!(
                                "Pruned stale or renamed KDE shortcut `{}` for component `{}` (success: {})",
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
    session: Session<GlobalShortcuts>,
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
