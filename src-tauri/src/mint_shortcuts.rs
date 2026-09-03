//! Linux Mint (Cinnamon / MATE) native shortcut integration.
//!
//! Cinnamon and MATE serve no XDG `GlobalShortcuts` portal and have no plans
//! to. On an X11 session that costs nothing — VoxCtrl reads raw XInput2 key
//! events and every gesture works. This module is for the case where even that
//! is unavailable (a Wayland Cinnamon session, or an X server without XInput2):
//! it registers VoxCtrl's D-Bus interface as a *native* custom shortcut through
//! `gsettings`, so the desktop itself owns the key grab.
//!
//! What that mechanism can express is strictly limited, and the limit is worth
//! stating plainly because the rest of the app has to reflect it: a custom
//! keybinding runs a command on key-**press** and reports nothing on release.
//! There is no way to hold a key, and no way to tell a tap from a hold. Only
//! `toggle` survives the trip, which is why `Backend::MintDbus` advertises that
//! one gesture and the settings UI offers no others while it is running.

use std::process::Command;

use tracing::info;
use voxctrl_routing::{GestureType, HotkeyBinding};

use crate::host_env::host_command;

/// D-Bus address of the running instance. `zbus` publishes methods under their
/// PascalCase names, so `toggle_binding` is `ToggleBinding` on the bus — a
/// command naming the Rust spelling silently invokes nothing.
const DBUS_DEST: &str = "ai.voxctrl.Dictation";
const DBUS_PATH: &str = "/ai/voxctrl/Dictation";

/// Marks a custom keybinding as VoxCtrl's, so a sync can tell its own entries
/// from the user's without depending on what they are named.
const OWNED_COMMAND_MARKER: &str = "ai.voxctrl.Dictation";

/// The shortcut registered when there is no usable binding to mirror.
const FALLBACK_ACCEL: &str = "<Primary><Alt>space";

/// One shortcut as it was handed to the desktop.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NativeShortcut {
    /// The VoxCtrl binding this fires, empty for the fallback toggle.
    pub binding_id: String,
    /// GTK accelerator, e.g. `<Super>space`.
    pub accel: String,
    /// gsettings id of the custom keybinding, e.g. `custom3`.
    pub slot: String,
}

/// Check if the current session is running Linux Mint's Cinnamon or MATE desktop environment.
pub fn is_mint_desktop() -> bool {
    #[cfg(test)]
    {
        if let Ok(mock) = std::env::var("VOXCTRL_TEST_DESKTOP_MOCK") {
            return mock == "cinnamon" || mock == "mate" || mock == "mint";
        }
    }

    let check_var = |var: &str| {
        std::env::var(var)
            .unwrap_or_default()
            .to_lowercase()
    };

    let desktop = check_var("XDG_CURRENT_DESKTOP");
    let session = check_var("DESKTOP_SESSION");

    desktop.contains("cinnamon")
        || desktop.contains("mate")
        || session.contains("cinnamon")
        || session.contains("mate")
        || session.contains("mint")
}

/// Run `gsettings` with the host's environment rather than the bundle's.
///
/// Inside the AppImage a plain `Command::new("gsettings")` inherits
/// `GSETTINGS_SCHEMA_DIR` pointing into the AppDir, so it cannot see the
/// Cinnamon schemas and reports that this desktop has no keybinding support —
/// which reads exactly like running on a desktop that genuinely has none.
fn gsettings() -> Command {
    host_command("gsettings")
}

/// Detect whether Cinnamon or MATE gsettings schemas are available and valid on this system.
pub fn detect_mint_schema() -> Option<&'static str> {
    #[cfg(test)]
    {
        if let Ok(mock) = std::env::var("VOXCTRL_TEST_SCHEMA_MOCK") {
            if mock == "cinnamon" {
                return Some("org.cinnamon.desktop.keybindings");
            } else if mock == "mate" {
                return Some("org.mate.SettingsDaemon.plugins.media-keys");
            } else if mock == "none" {
                return None;
            }
        }
    }

    if !is_mint_desktop() {
        return None;
    }

    let check_schema = |schema: &str, key: &str| -> bool {
        gsettings()
            .args(["get", schema, key])
            .output()
            .map(|out| out.status.success() && !String::from_utf8_lossy(&out.stderr).contains("No such"))
            .unwrap_or(false)
    };

    if check_schema("org.cinnamon.desktop.keybindings", "custom-list")
        || check_schema("org.cinnamon.desktop.keybindings", "custom-keybindings")
    {
        Some("org.cinnamon.desktop.keybindings")
    } else if check_schema("org.mate.SettingsDaemon.plugins.media-keys", "custom-keybindings") {
        Some("org.mate.SettingsDaemon.plugins.media-keys")
    } else {
        None
    }
}

/// Detect the key name for custom keybindings list ('custom-list' or 'custom-keybindings').
pub fn detect_mint_key_name(schema: &str) -> &'static str {
    if schema.contains("cinnamon") {
        let test = gsettings().args(["get", schema, "custom-list"]).output();
        if let Ok(out) = test {
            if out.status.success() && !String::from_utf8_lossy(&out.stderr).contains("No such") {
                return "custom-list";
            }
        }
        "custom-keybindings"
    } else {
        "custom-keybindings"
    }
}

/// Parse a gsettings array string like "['custom0', 'voxctrl-toggle']" or "@as []".
pub fn parse_custom_keybindings_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "@as []" || trimmed == "[]" {
        return Vec::new();
    }

    // Extract items between quotes inside brackets
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;

    for ch in trimmed.chars() {
        if ch == '\'' || ch == '"' {
            if in_quote {
                items.push(current.clone());
                current.clear();
                in_quote = false;
            } else {
                in_quote = true;
            }
        } else if in_quote {
            current.push(ch);
        }
    }

    items
}

/// Format a list of keybinding IDs back into a gsettings string array representation, e.g. "['custom0', 'voxctrl-toggle']".
pub fn format_custom_keybindings_list(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".to_string();
    }
    let formatted_items: Vec<String> = items.iter().map(|item| format!("'{}'", item)).collect();
    format!("[{}]", formatted_items.join(", "))
}

/// Pick gsettings slots for `needed` shortcuts, reusing VoxCtrl's own and never
/// touching anyone else's.
///
/// Cinnamon's Keyboard applet enumerates custom keybindings as `custom0`,
/// `custom1`, … and does not list an entry named anything else, so a shortcut
/// VoxCtrl registered under its own name would be invisible — and unfixable —
/// in System Settings. Numbered slots are also why this has to allocate rather
/// than hardcode: writing `custom0` blind would overwrite whatever custom
/// shortcut the user already had there.
pub fn allocate_slots(existing: &[String], owned: &[String], needed: usize) -> Vec<String> {
    let mut slots: Vec<String> = owned.iter().take(needed).cloned().collect();

    let mut n = 0;
    while slots.len() < needed {
        let candidate = format!("custom{n}");
        n += 1;
        if existing.contains(&candidate) && !owned.contains(&candidate) {
            continue;
        }
        if slots.contains(&candidate) {
            continue;
        }
        slots.push(candidate);
    }
    slots
}

/// The gesture styles this backend can serve, as a predicate over bindings.
///
/// A custom keybinding fires a command on key-press and says nothing on
/// release, so a hold has no end and a double-tap cannot be told from two
/// separate presses. Registering those anyway would give the user a shortcut
/// that starts a recording nothing ever stops.
fn is_mirrorable(binding: &HotkeyBinding) -> bool {
    !binding.disabled && !binding.keys.is_empty() && binding.gesture == GestureType::Toggle
}

/// The `dbus-send` invocation that fires one binding.
fn toggle_command(binding_id: &str) -> String {
    format!(
        "dbus-send --session --dest={DBUS_DEST} --type=method_call {DBUS_PATH} \
         {DBUS_DEST}.ToggleBinding string:'{binding_id}'"
    )
}

/// Read the `command` of one custom keybinding slot.
fn slot_command(schema: &str, slot: &str) -> Option<String> {
    let (child_schema, path) = child_schema_and_path(schema, slot);
    let out = gsettings()
        .args(["get", &format!("{child_schema}:{path}"), "command"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Read the `binding` of one custom keybinding slot, as the raw gsettings value.
fn slot_binding(schema: &str, slot: &str) -> Option<String> {
    let (child_schema, path) = child_schema_and_path(schema, slot);
    let out = gsettings()
        .args(["get", &format!("{child_schema}:{path}"), "binding"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when a gsettings `binding` value names an actual key.
///
/// Cinnamon stores an unbound shortcut as `['']` or `@as []` and MATE as `''`.
/// Treating those as bound is what let a half-written registration report
/// itself as working.
pub fn binding_value_is_set(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "@as []" || trimmed == "[]" || trimmed == "''" {
        return false;
    }
    parse_custom_keybindings_list(trimmed)
        .iter()
        .any(|v| !v.trim().is_empty())
}

fn child_schema_and_path(schema: &str, slot: &str) -> (&'static str, String) {
    if schema.contains("cinnamon") {
        (
            "org.cinnamon.desktop.keybindings.custom-keybinding",
            format!("/org/cinnamon/desktop/keybindings/custom-keybindings/{slot}/"),
        )
    } else {
        (
            "org.mate.SettingsDaemon.plugins.media-keys.custom-keybinding",
            format!("/org/mate/settings-daemon/plugins/media-keys/custom-keybindings/{slot}/"),
        )
    }
}

/// Slots that hold a VoxCtrl shortcut, in list order.
fn owned_slots(schema: &str, existing: &[String]) -> Vec<String> {
    existing
        .iter()
        .filter(|slot| {
            slot_command(schema, slot)
                .map(|c| c.contains(OWNED_COMMAND_MARKER))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// Check if VoxCtrl's native shortcut is registered *and* actually bound to a key.
pub fn is_mint_shortcut_registered() -> bool {
    let Some(schema) = detect_mint_schema() else {
        return false;
    };
    let key_name = detect_mint_key_name(schema);

    let output = gsettings().args(["get", schema, key_name]).output();

    let Ok(out) = output else { return false };
    if !out.status.success() {
        return false;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let existing = parse_custom_keybindings_list(&raw);

    // A slot in the list whose command is ours but whose `binding` was never
    // written fires on no key at all. Reporting that as registered is what made
    // the setup window say "VoxCtrl is ready" on a machine where no shortcut
    // could work.
    owned_slots(schema, &existing).iter().any(|slot| {
        slot_binding(schema, slot)
            .map(|b| binding_value_is_set(&b))
            .unwrap_or(false)
    })
}

/// Mirror the user's toggle bindings into Cinnamon/MATE system shortcuts.
///
/// Returns what was registered, so the caller can tell the user which of their
/// bindings this desktop is actually serving.
pub fn sync_mint_shortcuts(bindings: &[HotkeyBinding]) -> Result<Vec<NativeShortcut>, String> {
    write_shortcuts(mirrorable_shortcuts(bindings))
}

/// The `(binding id, GTK accelerator)` pairs this desktop can actually serve.
///
/// Falls back to a single toggle on `FALLBACK_ACCEL` when the user has no
/// binding that survives the trip, so a fresh install still has *some* working
/// shortcut rather than none.
fn mirrorable_shortcuts(bindings: &[HotkeyBinding]) -> Vec<(String, String)> {
    let mut wanted: Vec<(String, String)> = Vec::new();
    for b in bindings.iter().filter(|b| is_mirrorable(b)) {
        let Ok(portal_accel) = voxctrl_hotkeys::trigger::accelerator(&b.keys) else {
            // A bare modifier or a two-key combo has no accelerator, so the
            // desktop cannot bind it. Skipping is right: the settings UI has
            // already told the user this combination needs a regular key.
            continue;
        };
        wanted.push((b.id.clone(), convert_to_gtk_accelerator(&portal_accel)));
    }
    if wanted.is_empty() {
        wanted.push((String::new(), FALLBACK_ACCEL.to_string()));
    }
    wanted
}

fn write_shortcuts(wanted: Vec<(String, String)>) -> Result<Vec<NativeShortcut>, String> {
    let schema = detect_mint_schema().ok_or_else(|| {
        "Neither Cinnamon nor MATE keybinding schema was found via gsettings.".to_string()
    })?;
    let key_name = detect_mint_key_name(schema);

    let get_out = gsettings()
        .args(["get", schema, key_name])
        .output()
        .map_err(|e| format!("Failed to execute gsettings get: {}", e))?;
    if !get_out.status.success() {
        return Err(format!(
            "gsettings get failed: {}",
            String::from_utf8_lossy(&get_out.stderr)
        ));
    }
    let raw = String::from_utf8_lossy(&get_out.stdout);
    let existing = parse_custom_keybindings_list(&raw);
    let owned = owned_slots(schema, &existing);
    let slots = allocate_slots(&existing, &owned, wanted.len());

    // Write every child key *before* the slot joins the list. The settings
    // daemon reacts to the list changing by reading the entry it names, and an
    // entry it reads first and finds empty stays unbound: nothing re-notifies
    // it for a slot it has already seen.
    let mut registered = Vec::new();
    for (slot, (binding_id, accel)) in slots.iter().zip(wanted.iter()) {
        write_slot(schema, slot, binding_id, accel)?;
        registered.push(NativeShortcut {
            binding_id: binding_id.clone(),
            accel: accel.clone(),
            slot: slot.clone(),
        });
    }

    // A slot we used to own but no longer need is cleared as well as delisted.
    // Leaving a stale accelerator in dconf means a shortcut the user cannot see
    // in System Settings would come back the moment anything re-listed it.
    for slot in owned.iter().filter(|s| !slots.contains(s)) {
        let (child_schema, path) = child_schema_and_path(schema, slot);
        let target = format!("{child_schema}:{path}");
        for key in ["binding", "command", "name"] {
            let _ = gsettings().args(["reset", &target, key]).output();
        }
    }

    // Our stale slots are dropped from the list; everyone else's are kept in
    // their original order.
    let mut new_list: Vec<String> = existing
        .iter()
        .filter(|s| !owned.contains(s) || slots.contains(s))
        .cloned()
        .collect();
    for slot in slots {
        if !new_list.contains(&slot) {
            new_list.push(slot);
        }
    }

    if new_list != existing {
        let set_out = gsettings()
            .args(["set", schema, key_name, &format_custom_keybindings_list(&new_list)])
            .output()
            .map_err(|e| format!("Failed to set {} list: {}", key_name, e))?;
        if !set_out.status.success() {
            return Err(format!(
                "Failed to update {} list: {}",
                key_name,
                String::from_utf8_lossy(&set_out.stderr)
            ));
        }
    }

    info!(
        "Registered {} Linux Mint native shortcut(s) via gsettings",
        registered.len()
    );
    Ok(registered)
}

/// Write one custom keybinding's name, command and key.
fn write_slot(schema: &str, slot: &str, binding_id: &str, accel: &str) -> Result<(), String> {
    let (child_schema, path) = child_schema_and_path(schema, slot);
    let target = format!("{child_schema}:{path}");
    let is_cinnamon = schema.contains("cinnamon");

    let name = if binding_id.is_empty() {
        "VoxCtrl Dictation Toggle".to_string()
    } else {
        format!("VoxCtrl: {binding_id}")
    };

    let _ = gsettings().args(["set", &target, "name", &name]).output();
    let _ = gsettings()
        .args(["set", &target, "command", &toggle_command(binding_id)])
        .output();

    // Cinnamon's `binding` is an array of accelerators; MATE's is a single one.
    let binding_val = if is_cinnamon {
        format!("['{accel}']")
    } else {
        format!("'{accel}'")
    };
    let bind_out = gsettings()
        .args(["set", &target, "binding", &binding_val])
        .output()
        .map_err(|e| format!("Failed to set shortcut binding: {}", e))?;
    if !bind_out.status.success() {
        return Err(format!(
            "Failed to set keybinding value: {}",
            String::from_utf8_lossy(&bind_out.stderr)
        ));
    }
    Ok(())
}

/// Register VoxCtrl's native shortcut into Cinnamon or MATE system settings.
///
/// Kept for the callers that have no bindings to hand — the first-run installer
/// and the setup window's "Approve Shortcuts" button. Where the user's own
/// bindings are available, `sync_mint_shortcuts` mirrors those instead of
/// inventing a shortcut they did not choose.
pub fn register_mint_shortcut(preferred_binding: Option<&str>) -> Result<String, String> {
    let wanted = match preferred_binding {
        Some(accel) => vec![(String::new(), accel.to_string())],
        None => mirrorable_shortcuts(
            &voxctrl_routing::load_bindings(&voxctrl_routing::config_dir()).unwrap_or_default(),
        ),
    };

    let registered = write_shortcuts(wanted)?;
    let accels: Vec<String> = registered.iter().map(|s| s.accel.clone()).collect();
    Ok(format!(
        "Registered {} in Linux Mint System Settings",
        accels.join(", ")
    ))
}

/// Convert an XDG portal accelerator string (e.g. "CTRL+ALT+space") into GTK accelerator format (e.g. "<Primary><Alt>space").
pub fn convert_to_gtk_accelerator(portal_accel: &str) -> String {
    let tokens: Vec<&str> = portal_accel.split('+').collect();
    if tokens.is_empty() {
        return portal_accel.to_string();
    }
    let key = tokens.last().unwrap_or(&"");
    let mut result = String::new();
    for &m in &tokens[..tokens.len() - 1] {
        match m {
            "CTRL" => result.push_str("<Primary>"),
            "ALT" => result.push_str("<Alt>"),
            "SHIFT" => result.push_str("<Shift>"),
            "LOGO" => result.push_str("<Super>"),
            _ => {}
        }
    }
    result.push_str(key);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(id: &str, gesture: GestureType, keys: &[&str]) -> HotkeyBinding {
        HotkeyBinding {
            id: id.to_string(),
            label: id.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
            gesture,
            target_id: String::new(),
            target_ids: Vec::new(),
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

    #[test]
    fn test_convert_to_gtk_accelerator() {
        assert_eq!(convert_to_gtk_accelerator("CTRL+ALT+space"), "<Primary><Alt>space");
        assert_eq!(convert_to_gtk_accelerator("LOGO+d"), "<Super>d");
        assert_eq!(convert_to_gtk_accelerator("CTRL+SHIFT+Return"), "<Primary><Shift>Return");
        assert_eq!(convert_to_gtk_accelerator("space"), "space");
    }

    #[test]
    fn test_parse_custom_keybindings_list() {
        assert!(parse_custom_keybindings_list("@as []").is_empty());
        assert!(parse_custom_keybindings_list("[]").is_empty());
        assert_eq!(
            parse_custom_keybindings_list("['custom0', 'voxctrl-toggle']"),
            vec!["custom0", "voxctrl-toggle"]
        );
        assert_eq!(
            parse_custom_keybindings_list("[\"custom1\"]"),
            vec!["custom1"]
        );
    }

    #[test]
    fn test_format_custom_keybindings_list() {
        assert_eq!(format_custom_keybindings_list(&[]), "@as []");
        assert_eq!(
            format_custom_keybindings_list(&["custom0".to_string(), "voxctrl-toggle".to_string()]),
            "['custom0', 'voxctrl-toggle']"
        );
    }

    #[test]
    fn test_is_mint_desktop_mock() {
        let _lock = crate::test_utils::get_env_lock().lock().unwrap();
        std::env::set_var("VOXCTRL_TEST_DESKTOP_MOCK", "cinnamon");
        assert!(is_mint_desktop());

        std::env::set_var("VOXCTRL_TEST_DESKTOP_MOCK", "gnome");
        assert!(!is_mint_desktop());

        std::env::remove_var("VOXCTRL_TEST_DESKTOP_MOCK");
    }

    #[test]
    fn the_dbus_command_names_the_method_zbus_actually_publishes() {
        // zbus exposes `toggle_binding` as `ToggleBinding`. A command naming the
        // Rust spelling invokes nothing at all, which is exactly how the old
        // `toggle_recording` command failed: registration looked fine and the
        // shortcut did nothing.
        let cmd = toggle_command("dictate");
        assert!(cmd.contains("ai.voxctrl.Dictation.ToggleBinding"), "{cmd}");
        assert!(cmd.contains("string:'dictate'"), "{cmd}");
    }

    #[test]
    fn slots_never_overwrite_a_shortcut_the_user_created() {
        // custom0 and custom1 belong to the user; VoxCtrl has to go around them.
        let existing = vec!["custom0".to_string(), "custom1".to_string()];
        let slots = allocate_slots(&existing, &[], 2);
        assert_eq!(slots, vec!["custom2", "custom3"]);
    }

    #[test]
    fn a_resync_reuses_the_slots_voxctrl_already_owns() {
        // Otherwise every save leaks a new custom keybinding into the user's
        // System Settings.
        let existing = vec!["custom0".to_string(), "custom1".to_string()];
        let owned = vec!["custom1".to_string()];
        assert_eq!(allocate_slots(&existing, &owned, 1), vec!["custom1"]);
        assert_eq!(allocate_slots(&existing, &owned, 2), vec!["custom1", "custom2"]);
    }

    #[test]
    fn slots_are_numbered_so_cinnamons_settings_panel_lists_them() {
        // Cinnamon's Keyboard applet enumerates `customN` and shows nothing
        // else, so a name of our own would be invisible and unfixable there.
        for slot in allocate_slots(&[], &[], 3) {
            assert!(slot.starts_with("custom"), "{slot}");
            assert!(slot["custom".len()..].chars().all(|c| c.is_ascii_digit()), "{slot}");
        }
    }

    #[test]
    fn an_unwritten_binding_value_does_not_count_as_registered() {
        // The half-written state that used to report "VoxCtrl is ready".
        assert!(!binding_value_is_set("@as []"));
        assert!(!binding_value_is_set("[]"));
        assert!(!binding_value_is_set("['']"));
        assert!(!binding_value_is_set("''"));
        assert!(!binding_value_is_set("   "));

        assert!(binding_value_is_set("['<Super>space']"));
        assert!(binding_value_is_set("'<Primary><Alt>space'"));
    }

    #[test]
    fn only_toggle_bindings_are_mirrored_to_the_desktop() {
        // A custom keybinding reports no key release, so a hold registered here
        // would start a recording that nothing ever ends.
        assert!(is_mirrorable(&binding("t", GestureType::Toggle, &["KEY_LEFTCTRL", "KEY_D"])));
        for gesture in [
            GestureType::Hold,
            GestureType::DoubleTap,
            GestureType::DoubleTapHold,
        ] {
            assert!(
                !is_mirrorable(&binding("x", gesture, &["KEY_LEFTCTRL", "KEY_D"])),
                "{gesture:?} cannot survive a press-only shortcut"
            );
        }
    }

    #[test]
    fn disabled_and_keyless_bindings_are_never_mirrored() {
        let mut disabled = binding("t", GestureType::Toggle, &["KEY_LEFTCTRL", "KEY_D"]);
        disabled.disabled = true;
        assert!(!is_mirrorable(&disabled));
        assert!(!is_mirrorable(&binding("t", GestureType::Toggle, &[])));
    }
}
