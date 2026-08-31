use regex::Regex;

/// Attempts to intercept a command from the transcription.
///
/// Patterns:
/// 1. "VoxCtrl, [command], [text]"
/// 2. "[text]. VoxCtrl, [command]"
///
/// Returns `Some((target_id, payload))` if a command is successfully parsed,
/// otherwise `None`.
pub fn parse_command_routing(text: &str) -> Option<(String, String)> {
    let trigger = "voxctrl";
    let text_lower = text.to_lowercase();

    if !text_lower.contains(trigger) {
        return None;
    }

    // Pattern 1: "VoxCtrl, [command], [text]" (or "VoxCtrl [command] [text]")
    // Regex: (?i)voxctrl[,\s]+([a-z0-9_-]+)[,\s]+(.*)
    let re_prefix = Regex::new(r"(?i)voxctrl[,\s]+([a-z0-9_-]+)[,\s]+(.*)").ok()?;
    if let Some(caps) = re_prefix.captures(text) {
        let cmd = caps.get(1)?.as_str().to_string();
        let payload = caps.get(2)?.as_str().trim().to_string();
        return Some((cmd, payload));
    }

    // Pattern 2: "[text]. VoxCtrl, [command]"
    // Regex: (.*)[.,\s]+(?i)voxctrl[,\s]+([a-z0-9_-]+)
    let re_suffix = Regex::new(r"(.*)[.,\s]+(?i)voxctrl[,\s]+([a-z0-9_-]+)").ok()?;
    if let Some(caps) = re_suffix.captures(text) {
        let payload = caps.get(1)?.as_str().trim().to_string();
        let cmd = caps.get(2)?.as_str().to_string();
        return Some((cmd, payload));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_pattern() {
        let input = "VoxCtrl, notes, Hello there!";
        let result = parse_command_routing(input);
        assert!(result.is_some());
        let (cmd, payload) = result.unwrap();
        assert_eq!(cmd, "notes");
        assert_eq!(payload, "Hello there!");
    }

    #[test]
    fn test_prefix_no_comma() {
        let input = "voxctrl notes hello there";
        let result = parse_command_routing(input);
        assert!(result.is_some());
        let (cmd, payload) = result.unwrap();
        assert_eq!(cmd, "notes");
        assert_eq!(payload, "hello there");
    }

    #[test]
    fn test_suffix_pattern() {
        let input = "Hello there. VoxCtrl, notes";
        let result = parse_command_routing(input);
        assert!(result.is_some());
        let (cmd, payload) = result.unwrap();
        assert_eq!(cmd, "notes");
        assert_eq!(payload, "Hello there");
    }

    #[test]
    fn test_no_trigger() {
        let input = "Just a normal sentence.";
        let result = parse_command_routing(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_command_format() {
        let input = "VoxCtrl , , text";
        let result = parse_command_routing(input);
        // The regex might match but cmd would be empty if we aren't careful.
        // [a-z0-9_-]+ requires at least one character.
        assert!(result.is_none());
    }
}
