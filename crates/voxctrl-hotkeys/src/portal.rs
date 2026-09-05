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
use voxctrl_routing::{HotkeyBinding, TTS_STOP_BINDING_ID};

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
#[derive(Clone)]
struct ShortcutGroup {
    id: String,
    description: String,
    trigger: Option<String>,
    binding_ids: Vec<String>,
    /// This shortcut is only registered for as long as VoxCtrl needs it — the
    /// TTS stop key on bare Escape. It lives in its own portal session, so
    /// taking it and giving it back never disturbs the session holding the
    /// dictation shortcuts.
    transient: bool,
}

fn group_bindings(bindings: &[HotkeyBinding]) -> Vec<ShortcutGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, ShortcutGroup> = HashMap::new();

    for b in bindings.iter().filter(|b| !b.disabled && !b.keys.is_empty()) {
        // Only the app's own stop-key binding may be held transiently. A user
        // who binds dictation to bare Escape shares this group with it, and
        // their binding is a standing one — so the group is too, and the stop
        // key rides along on a registration that is never given back.
        let transient =
            crate::trigger::is_reserved_for_the_desktop(&b.keys) && b.id == TTS_STOP_BINDING_ID;
        let signature = b.trigger_signature();
        let entry = groups.entry(signature.clone()).or_insert_with(|| {
            order.push(signature.clone());
            ShortcutGroup {
                id: shortcut_id(&signature),
                description: String::new(),
                trigger: portal_trigger(&b.keys),
                binding_ids: Vec::new(),
                transient,
            }
        });
        entry.transient = entry.transient && transient;
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

/// Shortcut ids VoxCtrl binds only while it is speaking.
///
/// A reserved key (bare Escape) is registered when playback starts and dropped
/// again a moment after it ends, so the rest of the desktop keeps the key. That
/// makes it *absent* from the registered set for most of the session, and
/// `sync_kde_shortcuts` must not read that absence as "the user deleted this
/// binding": KDE stores the user's enabled tick against the shortcut id, and
/// pruning the id would throw it away — so the next arm would register a fresh
/// shortcut, disabled by default (bugs.kde.org #483639), and the stop key would
/// never fire again.
fn transient_shortcut_ids() -> std::collections::HashSet<String> {
    crate::trigger::RESERVED_KEYS
        .iter()
        .map(|k| shortcut_id(&k.to_ascii_uppercase()))
        .collect()
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

    // The standing shortcuts — everything but a transiently-held stop key — are
    // what this session carries, and it is never rebuilt for the stop key's
    // sake. That separation is the point: re-registering these ids under a new
    // session, then closing the old one, is what left the compositor with no
    // working dictation shortcut at all.
    let (groups, transient_groups) = split_groups(group_bindings(&bindings));
    let known = [groups.clone(), transient_groups.clone()].concat();
    let bound = bind_groups(&portal, &session, &groups, Some(&known)).await?;
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
        run(
            portal,
            session,
            bindings,
            groups,
            transient_groups,
            tx,
            rx_reload,
            health,
        )
        .await;
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

/// Register `groups` on `session`.
///
/// `known` is every group VoxCtrl currently has across all of its sessions, and
/// only matters for the KDE housekeeping: pruning is keyed on "an id VoxCtrl no
/// longer uses", so it has to see the whole picture rather than the subset this
/// session happens to carry. Pass `None` to skip that housekeeping entirely —
/// arming the stop key changes no user-visible binding, and rewriting the user's
/// shortcut store every time VoxCtrl speaks would be pure churn.
async fn bind_groups(
    portal: &GlobalShortcuts,
    session: &Session<GlobalShortcuts>,
    groups: &[ShortcutGroup],
    known: Option<&[ShortcutGroup]>,
) -> Result<Vec<BoundShortcut>, PortalError> {
    // Sync shortcut names in KDE settings and unregister any deleted shortcuts.
    if let Some(known) = known {
        sync_kde_shortcuts(known).await;
    }

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
    let transient = transient_shortcut_ids();
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
                        let is_stale = !group_details.contains_key(shortcut_id.as_str())
                            && !transient.contains(shortcut_id.as_str());

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
    update_kglobalshortcutsrc(&group_details, &transient).await;
}

async fn update_kglobalshortcutsrc(
    group_details: &std::collections::HashMap<&str, (&str, Option<&str>)>,
    transient: &std::collections::HashSet<String>,
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
                } else if transient.contains(key) {
                    // Bound only while VoxCtrl speaks. Keep the line exactly as
                    // it is: it carries the key the user assigned and whether
                    // they enabled it, and the next arm needs both.
                    new_lines.push(line.to_string());
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

/// Split the registered groups into the ones held for the session and the ones
/// VoxCtrl only holds while it needs them.
fn split_groups(groups: Vec<ShortcutGroup>) -> (Vec<ShortcutGroup>, Vec<ShortcutGroup>) {
    groups.into_iter().partition(|g| !g.transient)
}

/// Do these two sets ask the compositor for the same thing? Ids, names and
/// triggers are what a registration carries; anything else changed in a binding
/// is the gesture engine's business, not the portal's.
fn same_groups(a: &[ShortcutGroup], b: &[ShortcutGroup]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(a, b)| {
            a.id == b.id && a.description == b.description && a.trigger == b.trigger
        })
}

/// Which bindings each shortcut id fires, across every session.
fn shortcut_map(
    standing: &[ShortcutGroup],
    transient: &[ShortcutGroup],
) -> HashMap<String, Vec<String>> {
    standing
        .iter()
        .chain(transient.iter())
        .map(|g| (g.id.clone(), g.binding_ids.clone()))
        .collect()
}

/// A fresh session carrying `groups`, or the reason there is not one.
///
/// The caller closes whatever the new session replaces — and, for a set of ids
/// that is moving between sessions, closes it *first*: two sessions registering
/// the same id under the same app id, one of them then closing, is how a
/// desktop ends up holding neither.
async fn rebind(
    portal: &GlobalShortcuts,
    groups: &[ShortcutGroup],
    known: Option<&[ShortcutGroup]>,
) -> Result<(Session<GlobalShortcuts>, Vec<BoundShortcut>), PortalError> {
    let session = portal
        .create_session(Default::default())
        .await
        .map_err(|e| PortalError::Rejected(format!("{e}")))?;
    match bind_groups(portal, &session, groups, known).await {
        Ok(bound) => Ok((session, bound)),
        Err(e) => {
            let _ = session.close().await;
            Err(e)
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
    mut transient_groups: Vec<ShortcutGroup>,
    tx: GestureSender,
    rx_reload: ReloaderReceiver,
    health: Arc<ListenerHealth>,
) {
    let mut engine = GestureEngine::new(bindings);
    // Transient shortcuts get their own session so arming and releasing them
    // leaves the standing one — the dictation shortcuts — untouched.
    let mut transient_session: Option<Session<GlobalShortcuts>> = None;
    if !transient_groups.is_empty() {
        // Only reachable when the listener starts while VoxCtrl is already
        // speaking, which the "Approve shortcuts" button can do.
        match rebind(&portal, &transient_groups, None).await {
            Ok((session, _)) => transient_session = Some(session),
            Err(e) => tracing::warn!("Could not register the stop key: {e}"),
        }
    }
    let mut by_shortcut = shortcut_map(&groups, &transient_groups);

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
                match by_shortcut.get(event.shortcut_id()) {
                    Some(ids) => {
                        tracing::debug!(
                            "portal hotkeys: `{}` fired ({})",
                            event.shortcut_id(),
                            ids.join(", ")
                        );
                        for id in ids {
                            engine.apply(id, Transition::Activated, &tx);
                        }
                    }
                    // The compositor is delivering a shortcut VoxCtrl no longer
                    // knows about — worth saying, because the mirror image
                    // (a shortcut that stops arriving) looks identical from here.
                    None => tracing::debug!(
                        "portal hotkeys: `{}` fired but matches no binding",
                        event.shortcut_id()
                    ),
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
                let known = [groups.clone(), transient_groups.clone()].concat();
                health.set_bound_shortcuts(describe(&known, event.shortcuts()));
            }
            new_bindings = next_reload(&rx_reload) => {
                let Some(new_bindings) = new_bindings else { break };
                tracing::info!("portal hotkeys: reloading {} bindings", new_bindings.len());
                if new_bindings.is_empty() {
                    // Nothing upstream should ever send this: it would ask the
                    // compositor to forget every shortcut VoxCtrl has, and only
                    // a restart would bring them back. Refusing costs nothing —
                    // a genuine "no shortcuts" state is indistinguishable from a
                    // failed read, and the second is the likely one.
                    tracing::warn!(
                        "portal hotkeys: ignoring an empty binding set; keeping the \
                         shortcuts already registered"
                    );
                    continue;
                }
                let (new_standing, new_transient) = split_groups(group_bindings(&new_bindings));
                let standing_changed = !same_groups(&new_standing, &groups);
                let transient_changed = !same_groups(&new_transient, &transient_groups);

                engine.reset(&tx);
                engine.reload(new_bindings.clone());
                groups = new_standing;
                transient_groups = new_transient;
                by_shortcut = shortcut_map(&groups, &transient_groups);

                // A stop key going up or down must not touch the session that
                // holds the dictation shortcuts. Re-registering those ids under
                // a second session and then closing the first is what left a
                // compositor with no working shortcut at all once playback had
                // been cancelled once.
                if standing_changed {
                    // Sessions allow `bind_shortcuts` exactly once, so a real
                    // change to the user's bindings needs a new one.
                    tracing::info!(
                        "portal hotkeys: the standing shortcuts changed ({} now), \
                         re-creating their session",
                        groups.len()
                    );
                    let known = [groups.clone(), transient_groups.clone()].concat();
                    match rebind(&portal, &groups, Some(&known)).await {
                        Ok((new_session, bound)) => {
                            let _ = session.close().await;
                            session = new_session;
                            for s in &bound {
                                tracing::info!(
                                    "portal hotkeys: `{}` bound={} as `{}`",
                                    s.binding_ids.join(", "),
                                    s.bound,
                                    s.trigger_description
                                );
                            }
                            health.set_bound_shortcuts(bound);
                        }
                        Err(e) => tracing::warn!("Re-binding portal shortcuts failed: {e}"),
                    }
                }

                if transient_changed {
                    // Close first: the old session and the new one would be
                    // registering the same shortcut id under the same app id,
                    // and closing the loser afterwards takes the winner's
                    // registration with it on at least one desktop.
                    if let Some(old) = transient_session.take() {
                        let _ = old.close().await;
                    }
                    if transient_groups.is_empty() {
                        tracing::info!(
                            "portal hotkeys: released the stop key; the standing session \
                             was not touched"
                        );
                    } else {
                        // No KDE housekeeping here: nothing the user configured
                        // has changed, and rewriting their shortcut store every
                        // time VoxCtrl speaks would be pure churn.
                        match rebind(&portal, &transient_groups, None).await {
                            Ok((session, bound)) => {
                                transient_session = Some(session);
                                tracing::info!(
                                    "portal hotkeys: took the stop key on its own session \
                                     ({} bound)",
                                    bound.iter().filter(|s| s.bound).count()
                                );
                            }
                            Err(e) => tracing::warn!("Could not register the stop key: {e}"),
                        }
                    }
                }

                if !standing_changed && !transient_changed {
                    tracing::info!("portal hotkeys: shortcut triggers unchanged; preserving active portal session");
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
    fn a_reserved_key_is_registered_like_any_other_when_it_is_handed_over() {
        // Arming is the app's call, not the portal's: when the stop binding is
        // in the list the compositor is asked for it, and when the app drops it
        // for the idle stretches the group simply is not there.
        let stop = binding("__tts_stop__", &["KEY_ESC"], GestureType::Hold);
        let groups = group_bindings(std::slice::from_ref(&stop));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].trigger.as_deref(), Some("Escape"));

        assert!(group_bindings(&[]).is_empty());
    }

    #[test]
    fn a_transiently_bound_shortcut_is_never_pruned_from_kdes_store() {
        // KDE keys the user's "enabled" tick to the shortcut id. Escape is
        // absent from the registered set for most of a session — it is bound
        // only while VoxCtrl speaks — and treating that absence as a deleted
        // binding would discard the tick, so the next arm would register a
        // fresh, disabled shortcut (bugs.kde.org #483639) that never fires.
        let ids = transient_shortcut_ids();
        for spelling in ["KEY_ESC", "KEY_ESCAPE"] {
            let id = group_bindings(&[binding("s", &[spelling], GestureType::Hold)])[0]
                .id
                .clone();
            assert!(
                ids.contains(&id),
                "{spelling} registers as `{id}`, which must be protected"
            );
        }

        // And the protection is exactly that wide: an ordinary shortcut the
        // user really did delete still gets cleaned up.
        let ordinary = group_bindings(&[binding(
            "d",
            &["KEY_LEFTMETA", "KEY_SPACE"],
            GestureType::Hold,
        )])[0]
            .id
            .clone();
        assert!(!ids.contains(&ordinary));
    }

    #[test]
    fn the_stop_key_is_split_away_from_the_shortcuts_that_stay_registered() {
        // The regression this exists for: arming and releasing the stop key
        // used to rebuild the one session that also held the dictation
        // shortcuts — re-registering their ids under a new session and closing
        // the old one — and after the first release the compositor fired none
        // of them. The two now live in separate sessions.
        let (standing, transient) = split_groups(group_bindings(&[
            binding("dictate", &["KEY_LEFTMETA", "KEY_SPACE"], GestureType::Hold),
            binding("__tts_stop__", &["KEY_ESC"], GestureType::Hold),
        ]));

        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].binding_ids, vec!["dictate"]);
        assert_eq!(transient.len(), 1);
        assert_eq!(transient[0].binding_ids, vec!["__tts_stop__"]);
        assert_eq!(transient[0].trigger.as_deref(), Some("Escape"));

        // Both still reach the gesture engine: whichever session delivers the
        // activation, it is looked up by shortcut id.
        let map = shortcut_map(&standing, &transient);
        assert_eq!(map.len(), 2);
        assert!(map.values().any(|ids| ids == &vec!["__tts_stop__".to_string()]));
    }

    #[test]
    fn arming_the_stop_key_leaves_the_standing_shortcuts_alone() {
        // What the reload compares. Adding or dropping the stop key must not
        // read as a change to the standing set, or the session holding the
        // user's dictation shortcuts would be rebuilt for it after all.
        let dictate = binding("dictate", &["KEY_LEFTMETA", "KEY_SPACE"], GestureType::Hold);
        let stop = binding("__tts_stop__", &["KEY_ESC"], GestureType::Hold);

        let (idle, idle_transient) = split_groups(group_bindings(std::slice::from_ref(&dictate)));
        let (armed, armed_transient) = split_groups(group_bindings(&[dictate, stop]));

        assert!(same_groups(&idle, &armed), "the standing set is untouched");
        assert!(idle_transient.is_empty());
        assert!(!same_groups(&idle_transient, &armed_transient));
    }

    #[test]
    fn a_dictation_binding_on_escape_keeps_its_standing_registration() {
        // Only VoxCtrl's own stop key is held transiently. A user who chose
        // Escape to start dictation means it, and a shortcut that worked only
        // while VoxCtrl happened to be speaking would be worse than the grab
        // they were warned about — so the shared group stays standing and the
        // stop key rides along on it.
        let mut mine = binding("__tts_stop__", &["KEY_ESC"], GestureType::Hold);
        mine.id = "__tts_stop__".to_string();
        let theirs = binding("dictate-on-escape", &["KEY_ESC"], GestureType::Hold);

        let (standing, transient) = split_groups(group_bindings(&[mine, theirs]));
        assert_eq!(standing.len(), 1, "one shortcut, one registration");
        assert!(transient.is_empty());
        assert_eq!(
            standing[0].binding_ids,
            vec!["__tts_stop__", "dictate-on-escape"]
        );
    }

    #[test]
    fn shortcut_ids_are_stable_and_path_safe() {
        let a = group_bindings(&[binding("x", &["KEY_LEFTMETA", "KEY_SPACE"], GestureType::Hold)]);
        let b = group_bindings(&[binding("y", &["KEY_SPACE", "KEY_LEFTMETA"], GestureType::Hold)]);
        assert_eq!(a[0].id, b[0].id, "the compositor keys the user's choice off this");
        assert!(a[0].id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
