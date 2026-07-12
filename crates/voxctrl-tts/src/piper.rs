//! Piper voice catalogue, path resolution, and standalone binary/voice
//! download + extraction. `TtsEngineWorker::speak_piper` (in `engine.rs`)
//! spawns the `piper` binary resolved here and calls back into
//! [`get_voice_path`] / [`sample_rate_for_voice`].

use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::{info, warn};

// ── Piper voice catalogue ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub quality: &'static str,
    pub sample_rate: u32,
    pub filename: &'static str,
}

pub static PIPER_VOICES: &[VoiceInfo] = &[
    VoiceInfo { name: "en-us-libritts-high",   quality: "high",   sample_rate: 22050, filename: "en_US-libritts-high.onnx" },
    VoiceInfo { name: "en-us-amy-low",         quality: "low",    sample_rate: 16000, filename: "en_US-amy-low.onnx" },
    VoiceInfo { name: "en-us-kathleen-low",    quality: "low",    sample_rate: 16000, filename: "en_US-kathleen-low.onnx" },
    VoiceInfo { name: "en-gb-southern_english_female-low", quality: "low", sample_rate: 16000, filename: "en_GB-southern_english_female-low.onnx" },
    VoiceInfo { name: "en-us-ryan-high",       quality: "high",   sample_rate: 22050, filename: "en_US-ryan-high.onnx" },
    VoiceInfo { name: "en-us-ryan-medium",     quality: "medium", sample_rate: 22050, filename: "en_US-ryan-medium.onnx" },
    VoiceInfo { name: "en-us-ryan-low",        quality: "low",    sample_rate: 16000, filename: "en_US-ryan-low.onnx" },
    VoiceInfo { name: "en-us-lessac-medium",   quality: "medium", sample_rate: 16000, filename: "en_US-lessac-medium.onnx" },
    VoiceInfo { name: "en-us-lessac-low",      quality: "low",    sample_rate: 16000, filename: "en_US-lessac-low.onnx" },
    VoiceInfo { name: "en-us-danny-low",       quality: "low",    sample_rate: 16000, filename: "en_US-danny-low.onnx" },
    VoiceInfo { name: "en-gb-alan-low",        quality: "low",    sample_rate: 16000, filename: "en_GB-alan-low.onnx" },
];

// ── Piper helpers ─────────────────────────────────────────────────────────────

pub fn piper_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("piper-voices")
}

/// Expands a leading `~` to the user's home directory. Shared with `pocket.rs`,
/// which applies the same expansion to its own voice-clip directory setting.
pub(crate) fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn resolve_voices_dir(voice_dir: &str) -> PathBuf {
    if voice_dir.is_empty() {
        piper_voices_dir()
    } else {
        expand_tilde(voice_dir)
    }
}

pub fn piper_binary() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "piper.exe" } else { "piper" };
    let local_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("piper");
    let local = local_dir.join(exe);
    // On unix the local install is voxctrl-managed: only use it when healthy
    // (binary + espeak-ng-data directory), so a broken install falls through to
    // a system-wide piper and gets repaired on the next voice download.
    let local_healthy = if cfg!(unix) {
        local.exists() && local_dir.join("espeak-ng-data").is_dir()
    } else {
        local.exists()
    };
    if local_healthy {
        return Some(local);
    }
    voxctrl_config::find_in_path("piper")
}

// ── Voice catalogue helpers ───────────────────────────────────────────────────

fn voice_name_to_filename(name: &str) -> Option<String> {
    PIPER_VOICES
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.filename.to_string())
}

/// Used by `TtsEngineWorker::speak_piper` to pick the correct playback sample rate.
pub(crate) fn sample_rate_for_voice(name: &str) -> u32 {
    PIPER_VOICES
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.sample_rate)
        .unwrap_or(22050)
}

pub fn is_voice_downloaded(voice_name: &str, voice_dir: &str) -> bool {
    get_voice_path(voice_name, voice_dir).is_some()
}

// ── Piper voice download ──────────────────────────────────────────────────────

const PIPER_RELEASE_BASE: &str =
    "https://github.com/rhasspy/piper/releases/download/v0.0.2/";

pub fn get_voice_path(voice_name: &str, voice_dir: &str) -> Option<PathBuf> {
    let filename = voice_name_to_filename(voice_name)
        .unwrap_or_else(|| format!("{voice_name}.onnx"));

    let voices_dir = resolve_voices_dir(voice_dir);

    let path_onnx = voices_dir.join(&filename);
    let path_json = voices_dir.join(format!("{filename}.json"));
    if path_onnx.exists() && path_json.exists() {
        return Some(path_onnx);
    }

    let filename_lower = filename.to_lowercase();
    let path_onnx_lower = voices_dir.join(&filename_lower);
    let path_json_lower = voices_dir.join(format!("{filename_lower}.json"));
    if path_onnx_lower.exists() && path_json_lower.exists() {
        return Some(path_onnx_lower);
    }

    let path_raw_lower = voices_dir.join(format!("{}.onnx", voice_name.to_lowercase()));
    let path_raw_json_lower =
        voices_dir.join(format!("{}.onnx.json", voice_name.to_lowercase()));
    if path_raw_lower.exists() && path_raw_json_lower.exists() {
        return Some(path_raw_lower);
    }

    None
}

/// Extracts the piper release tarball into `dest_dir`, preserving the archive's
/// directory structure with the leading `piper/` component stripped.
///
/// Preserving the tree matters: the tarball ships an `espeak-ng-data/`
/// directory that the piper binary needs for phonemization. An earlier version
/// of this code flattened every entry's file name into one directory, which
/// destroyed `espeak-ng-data/` — so the standalone piper binary failed on every
/// machine without a system-wide piper install (no TTS audio, only a
/// "piper process failed" log line).
#[cfg(unix)]
fn extract_piper_archive(bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let tar = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();

        // Strip the top-level "piper/" component; skip unsafe paths.
        let rel: PathBuf = path
            .components()
            .skip(1)
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect();
        if rel.as_os_str().is_empty() {
            continue;
        }

        let dest = dest_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // unpack() handles directories, files, and symlinks and preserves modes.
        entry.unpack(&dest)?;
    }

    // Belt and braces: the binary must be executable.
    use std::os::unix::fs::PermissionsExt;
    let exe = dest_dir.join("piper");
    if let Ok(metadata) = std::fs::metadata(&exe) {
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&exe, perms);
    }

    Ok(())
}

pub async fn download_piper_binary() -> Result<()> {
    #[cfg(unix)]
    {
        let local_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("piper");

        let dest_exe = local_dir.join("piper");
        // A correct install has the binary AND the espeak-ng-data directory.
        // Installs made by the old flattening extractor have the binary but a
        // bogus `espeak-ng-data` *file* — wipe and re-extract those too.
        if dest_exe.exists() && local_dir.join("espeak-ng-data").is_dir() {
            return Ok(());
        }
        if local_dir.exists() {
            info!("Repairing broken standalone Piper install at {}", local_dir.display());
            tokio::fs::remove_dir_all(&local_dir).await?;
        }
        tokio::fs::create_dir_all(&local_dir).await?;

        info!("Downloading standalone Piper binary...");
        let url =
            "https://github.com/rhasspy/piper/releases/download/v1.2.0/piper_amd64.tar.gz";

        let response = reqwest::get(url).await?.error_for_status()?;
        let bytes = response.bytes().await?;

        info!("Extracting Piper binary...");
        extract_piper_archive(&bytes, &local_dir)?;
        info!("Standalone Piper binary installed to {}", dest_exe.display());
    }
    Ok(())
}

pub async fn download_voice(voice_name: &str, voice_dir: &str) -> Result<()> {
    if piper_binary().is_none() {
        if let Err(e) = download_piper_binary().await {
            warn!("Failed to download standalone piper binary: {e}");
        }
    }

    let voices_dir = resolve_voices_dir(voice_dir);
    tokio::fs::create_dir_all(&voices_dir).await?;

    if get_voice_path(voice_name, voice_dir).is_some() {
        info!("Voice {} is already downloaded.", voice_name);
        return Ok(());
    }

    let tarball_url = format!("{PIPER_RELEASE_BASE}voice-{voice_name}.tar.gz");
    info!("Downloading voice tarball: {tarball_url}");

    let response = reqwest::get(&tarball_url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    info!("Extracting voice files...");
    let cursor = std::io::Cursor::new(bytes);
    let tar = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let file_name = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        if file_name.ends_with(".onnx") || file_name.ends_with(".onnx.json") {
            let dest_path = voices_dir.join(&file_name);
            let mut temp_file = tempfile::NamedTempFile::new_in(&voices_dir)?;
            std::io::copy(&mut entry, &mut temp_file)?;
            temp_file.persist(&dest_path)?;
            info!("Extracted: {}", dest_path.display());
        }
    }

    info!("Voice files successfully downloaded and extracted.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_fake_voice(dir: &std::path::Path, filename: &str) {
        fs::write(dir.join(filename), b"fake onnx model").unwrap();
        fs::write(dir.join(format!("{filename}.json")), b"{}").unwrap();
    }

    // ── resolve_voices_dir ────────────────────────────────────────────────────

    #[test]
    fn test_resolve_voices_dir_empty_uses_default() {
        let result = resolve_voices_dir("");
        assert_eq!(result, piper_voices_dir());
    }

    #[test]
    fn test_resolve_voices_dir_absolute_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let result = resolve_voices_dir(path);
        assert_eq!(result, dir.path());
    }

    #[test]
    fn test_resolve_voices_dir_tilde_expands() {
        let result = resolve_voices_dir("~/my-voices");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home.join("my-voices"));
    }

    #[test]
    fn test_resolve_voices_dir_tilde_alone_expands() {
        let result = resolve_voices_dir("~");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home);
    }

    // ── expand_tilde ──────────────────────────────────────────────────────────

    #[test]
    fn test_expand_tilde_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn test_expand_tilde_subdir() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/.piper-voices"), home.join(".piper-voices"));
    }

    #[test]
    fn test_expand_tilde_absolute_unchanged() {
        assert_eq!(expand_tilde("/usr/share/voices"), PathBuf::from("/usr/share/voices"));
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("relative/path"), PathBuf::from("relative/path"));
    }

    // ── is_voice_downloaded ───────────────────────────────────────────────────

    #[test]
    fn test_is_voice_downloaded_default_dir_not_present() {
        let _ = is_voice_downloaded("en-us-lessac-medium", "");
    }

    #[test]
    fn test_is_voice_downloaded_returns_true_when_files_exist() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        create_fake_voice(dir.path(), "en_US-amy-low.onnx");
        assert!(is_voice_downloaded("en-us-amy-low", path));
    }

    #[test]
    fn test_is_voice_downloaded_returns_false_when_files_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(!is_voice_downloaded("en-us-amy-low", path));
    }

    #[test]
    fn test_is_voice_downloaded_returns_false_for_nonexistent_dir() {
        assert!(!is_voice_downloaded("en-us-amy-low", "/nonexistent/path/xyz"));
    }

    #[test]
    fn test_is_voice_downloaded_tilde_path() {
        let _ = is_voice_downloaded("en-us-lessac-medium", "~/.local/share/voxctrl/piper-voices");
    }

    #[test]
    fn test_is_voice_downloaded_only_onnx_not_sufficient() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("en_US-amy-low.onnx"), b"fake").unwrap();
        assert!(!is_voice_downloaded("en-us-amy-low", path));
    }

    // ── get_voice_path ────────────────────────────────────────────────────────

    #[test]
    fn test_get_voice_path_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(get_voice_path("en-us-ryan-high", path).is_none());
    }

    #[test]
    fn test_get_voice_path_returns_some_when_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        create_fake_voice(dir.path(), "en_US-ryan-high.onnx");
        let result = get_voice_path("en-us-ryan-high", path);
        assert!(result.is_some());
        assert!(result.unwrap().exists());
    }

    #[test]
    fn test_get_voice_path_accepts_custom_dir() {
        let dir = tempdir().unwrap();
        let other_dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let other_path = other_dir.path().to_str().unwrap();
        create_fake_voice(other_dir.path(), "en_US-danny-low.onnx");
        assert!(get_voice_path("en-us-danny-low", path).is_none());
        assert!(get_voice_path("en-us-danny-low", other_path).is_some());
    }

    #[test]
    fn test_get_voice_path_lowercase_fallback() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let lc_name = "en_us-lessac-medium.onnx";
        fs::write(dir.path().join(lc_name), b"fake").unwrap();
        fs::write(dir.path().join(format!("{lc_name}.json")), b"{}").unwrap();
        assert!(get_voice_path("en-us-lessac-medium", path).is_some());
    }

    // ── Piper voice catalogue ─────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_not_empty() {
        assert!(!PIPER_VOICES.is_empty());
    }

    #[test]
    fn test_piper_voices_have_required_fields() {
        for v in PIPER_VOICES {
            assert!(!v.name.is_empty());
            assert!(!v.quality.is_empty());
            assert!(!v.filename.is_empty());
            assert!(v.sample_rate > 0);
        }
    }

    #[test]
    fn test_piper_voices_names_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in PIPER_VOICES {
            assert!(seen.insert(v.name), "duplicate piper voice name: {}", v.name);
        }
    }

    #[test]
    fn test_piper_voices_filenames_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in PIPER_VOICES {
            assert!(seen.insert(v.filename), "duplicate piper filename: {}", v.filename);
        }
    }

    #[test]
    fn test_piper_voices_quality_values_are_valid() {
        let valid = ["high", "medium", "low"];
        for v in PIPER_VOICES {
            assert!(valid.contains(&v.quality), "unexpected quality '{}' for {}", v.quality, v.name);
        }
    }

    #[test]
    fn test_piper_voices_sample_rates_are_valid() {
        let valid_rates = [16000u32, 22050u32];
        for v in PIPER_VOICES {
            assert!(valid_rates.contains(&v.sample_rate), "unexpected sample_rate {} for {}", v.sample_rate, v.name);
        }
    }

    #[test]
    fn test_piper_voices_filenames_end_with_onnx() {
        for v in PIPER_VOICES {
            assert!(v.filename.ends_with(".onnx"), "filename should end with .onnx: {}", v.filename);
        }
    }

    // ── sample_rate_for_voice ─────────────────────────────────────────────────

    #[test]
    fn test_sample_rate_for_known_high_quality_voice() {
        assert_eq!(sample_rate_for_voice("en-us-ryan-high"), 22050);
    }

    #[test]
    fn test_sample_rate_for_known_low_quality_voice() {
        assert_eq!(sample_rate_for_voice("en-us-amy-low"), 16000);
    }

    #[test]
    fn test_sample_rate_for_unknown_voice_defaults_to_22050() {
        assert_eq!(sample_rate_for_voice("xx-unknown-voice"), 22050);
    }

    // ── piper_binary ──────────────────────────────────────────────────────────

    #[test]
    fn test_piper_binary_returns_option_without_panicking() {
        let _ = piper_binary();
    }

    // ── extract_piper_archive ─────────────────────────────────────────────────

    #[cfg(unix)]
    fn build_fake_piper_tarball() -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let gz = GzEncoder::new(Vec::new(), Compression::fast());
        let mut builder = tar::Builder::new(gz);

        let mut add_file = |path: &str, content: &[u8], mode: u32| {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            builder.append_data(&mut header, path, content).unwrap();
        };

        add_file("piper/piper", b"#!/bin/sh\necho fake piper\n", 0o755);
        add_file("piper/libespeak-ng.so.1", b"fake lib", 0o644);
        // Nested data tree — the part the old flattening extractor destroyed.
        add_file("piper/espeak-ng-data/phondata", b"fake phondata", 0o644);
        add_file("piper/espeak-ng-data/lang/gmw/en-US", b"fake lang", 0o644);

        builder.into_inner().unwrap().finish().unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn test_extract_piper_archive_preserves_directory_structure() {
        let dir = tempdir().unwrap();
        let bytes = build_fake_piper_tarball();

        extract_piper_archive(&bytes, dir.path()).unwrap();

        // Regression: espeak-ng-data must survive as a real directory tree,
        // not a flattened pile of files (which broke piper phonemization on
        // every fresh install without a system-wide piper).
        assert!(dir.path().join("piper").is_file());
        assert!(dir.path().join("libespeak-ng.so.1").is_file());
        assert!(dir.path().join("espeak-ng-data").is_dir());
        assert!(dir.path().join("espeak-ng-data/phondata").is_file());
        assert!(dir.path().join("espeak-ng-data/lang/gmw/en-US").is_file());

        // Binary must be executable.
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.path().join("piper")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "piper binary must be executable");
    }

    #[cfg(unix)]
    #[test]
    fn test_extract_piper_archive_skips_unsafe_paths() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let gz = GzEncoder::new(Vec::new(), Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let content: &[u8] = b"evil";
        let mut header = tar::Header::new_gnu();
        // Write the malicious path straight into the header bytes —
        // Builder::append_data / Header::set_path refuse `..` themselves.
        {
            let name = b"piper/../../escape.txt";
            let gnu = header.as_gnu_mut().unwrap();
            gnu.name[..name.len()].copy_from_slice(name);
        }
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, content).unwrap();
        let bytes = builder.into_inner().unwrap().finish().unwrap();

        let dir = tempdir().unwrap();
        extract_piper_archive(&bytes, dir.path()).unwrap();

        // The ParentDir components must be stripped: nothing lands outside the
        // destination directory.
        assert!(!dir.path().parent().unwrap().join("escape.txt").exists());
        assert!(!dir.path().parent().unwrap().parent().unwrap().join("escape.txt").exists());
        // The sanitized remainder is extracted inside the destination instead.
        assert!(dir.path().join("escape.txt").exists());
    }

    // ── piper_voices_dir ──────────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_dir_not_empty() {
        let d = piper_voices_dir();
        assert!(d.components().count() > 0);
    }

    #[test]
    fn test_piper_voices_dir_ends_with_piper_voices() {
        let d = piper_voices_dir();
        assert!(d.ends_with("voxctrl/piper-voices"));
    }

    // ── voice_name_to_filename ────────────────────────────────────────────────

    #[test]
    fn test_voice_name_to_filename_known() {
        assert_eq!(
            voice_name_to_filename("en-us-lessac-medium"),
            Some("en_US-lessac-medium.onnx".to_string())
        );
    }

    #[test]
    fn test_voice_name_to_filename_unknown_returns_none() {
        assert_eq!(voice_name_to_filename("xx-unknown-voice"), None);
    }

    #[test]
    fn test_voice_name_to_filename_all_piper_voices_resolve() {
        for v in PIPER_VOICES {
            let result = voice_name_to_filename(v.name);
            assert!(result.is_some(), "voice_name_to_filename should resolve {}", v.name);
            assert_eq!(result.unwrap(), v.filename);
        }
    }
}
