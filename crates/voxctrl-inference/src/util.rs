//! Small filesystem helpers shared by the transcription backends.

use std::path::PathBuf;

/// Base directory for on-device models: `<data-local>/voxctrl/models`.
/// Each backend places its files in a subfolder of this (whisper-cpp directly,
/// Moonshine under `moonshine/`).
pub(crate) fn models_base_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
}

/// Expand a leading `~` / `~/` to the user's home directory. Any other path is
/// returned unchanged.
pub(crate) fn expand_tilde(path: &str) -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(dirs::home_dir);
    if path == "~" {
        return home.unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(h) = home {
            return h.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_home() {
        let home = dirs::home_dir().expect("home dir must be available");
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn test_expand_tilde_subdir() {
        let home = dirs::home_dir().expect("home dir must be available");
        assert_eq!(expand_tilde("~/.models"), home.join(".models"));
    }

    #[test]
    fn test_expand_tilde_absolute_unchanged() {
        assert_eq!(expand_tilde("/tmp/models"), PathBuf::from("/tmp/models"));
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("relative/path"), PathBuf::from("relative/path"));
    }

    #[test]
    fn test_models_base_dir_ends_with_models() {
        assert!(models_base_dir().ends_with("models"));
    }
}
