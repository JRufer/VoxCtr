//! Linux Mint (Cinnamon / MATE) native shortcut integration.
//!
//! Linux Mint's desktop environments (Cinnamon, MATE) do not yet serve the XDG
//! `GlobalShortcuts` portal (`org.freedesktop.portal.GlobalShortcuts`), but they
//! manage native custom shortcuts via `gsettings` under `org.cinnamon.desktop.keybindings`
//! or `org.mate.SettingsDaemon.plugins.media-keys`.
//!
//! This module provides detection and automatic registration of VoxCtrl's
//! session D-Bus interface (`ai.voxctrl.Dictation.toggle_recording`) directly
//! into Mint's native system shortcut manager.

use std::process::Command;
use tracing::info;

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
        Command::new("gsettings")
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
        let test = Command::new("gsettings")
            .args(["get", schema, "custom-list"])
            .output();
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

/// Check if VoxCtrl's native shortcut is registered in gsettings.
pub fn is_mint_shortcut_registered() -> bool {
    let Some(schema) = detect_mint_schema() else {
        return false;
    };
    let key_name = detect_mint_key_name(schema);

    let output = Command::new("gsettings")
        .args(["get", schema, key_name])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout);
            let items = parse_custom_keybindings_list(&raw);
            items.iter().any(|item| item.contains("voxctrl"))
        }
        _ => false,
    }
}

/// Register VoxCtrl's native shortcut into Cinnamon or MATE system settings.
pub fn register_mint_shortcut(preferred_binding: Option<&str>) -> Result<String, String> {
    let schema = detect_mint_schema().ok_or_else(|| {
        "Neither Cinnamon nor MATE keybinding schema was found via gsettings.".to_string()
    })?;
    let key_name = detect_mint_key_name(schema);

    let binding = preferred_binding.unwrap_or("<Primary><Alt>space");
    let keybinding_id = "voxctrl-toggle";

    // 1. Read existing custom keybindings list
    let get_out = Command::new("gsettings")
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
    let mut items = parse_custom_keybindings_list(&raw);

    if !items.iter().any(|i| i == keybinding_id) {
        items.push(keybinding_id.to_string());
        let new_list_str = format_custom_keybindings_list(&items);

        let set_out = Command::new("gsettings")
            .args(["set", schema, key_name, &new_list_str])
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

    // 2. Configure the custom keybinding child schema properties
    let is_cinnamon = schema.contains("cinnamon");
    let path = if is_cinnamon {
        format!("/org/cinnamon/desktop/keybindings/custom-keybindings/{}/", keybinding_id)
    } else {
        format!("/org/mate/settings-daemon/plugins/media-keys/custom-keybindings/{}/", keybinding_id)
    };

    let child_schema = if is_cinnamon {
        "org.cinnamon.desktop.keybindings.custom-keybinding"
    } else {
        "org.mate.SettingsDaemon.plugins.media-keys.custom-keybinding"
    };

    let dbus_command = "dbus-send --session --dest=ai.voxctrl.Dictation --type=method_call /ai/voxctrl/Dictation ai.voxctrl.Dictation.toggle_recording";

    // Set name
    let _ = Command::new("gsettings")
        .args(["set", &format!("{}:{}", child_schema, path), "name", "VoxCtrl Dictation Toggle"])
        .output();

    // Set command
    let _ = Command::new("gsettings")
        .args(["set", &format!("{}:{}", child_schema, path), "command", dbus_command])
        .output();

    // Set binding (Cinnamon expects array of strings like "['<Primary><Alt>space']", MATE expects string)
    let binding_val = if is_cinnamon {
        format!("['{}']", binding)
    } else {
        format!("'{}'", binding)
    };

    let bind_out = Command::new("gsettings")
        .args(["set", &format!("{}:{}", child_schema, path), "binding", &binding_val])
        .output()
        .map_err(|e| format!("Failed to set shortcut binding: {}", e))?;

    if !bind_out.status.success() {
        return Err(format!(
            "Failed to set keybinding value: {}",
            String::from_utf8_lossy(&bind_out.stderr)
        ));
    }

    info!("Successfully registered Linux Mint native shortcut {} via gsettings", binding);
    Ok(format!("Registered shortcut {} in Linux Mint System Settings", binding))
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
}
