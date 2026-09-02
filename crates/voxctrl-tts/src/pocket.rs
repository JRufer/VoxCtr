//! Pocket-TTS (pure Rust / Candle) voice catalogue, model asset management,
//! and streaming synthesis. Named `pocket` rather than `pocket_tts` to avoid
//! colliding with the external `pocket_tts` crate this module wraps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use crate::engine::PlaybackCallback;
use crate::piper::expand_tilde;

// ── Pocket-TTS voice catalogue ────────────────────────────────────────────────
//
// pocket-tts clones a voice from a short reference clip rather than using a
// trained named-voice embedding table. We curate a small set of clips from
// the public `kyutai/tts-voices` HuggingFace dataset so VoxCtrl can still
// offer a familiar voice-picker UX. Clips are downloaded on first use (and
// cached by `hf-hub`) — see `is_pocket_tts_ready()` / `download_pocket_tts_assets()`.

#[derive(Debug, Clone)]
pub struct PocketTtsVoiceInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// `hf://` reference clip path consumed by `pocket_tts::weights::download_if_necessary`.
    pub reference_clip: &'static str,
}

pub static POCKET_TTS_VOICES: &[PocketTtsVoiceInfo] = &[
    PocketTtsVoiceInfo { id: "alba",    label: "Alba (Female)",   reference_clip: "hf://kyutai/tts-voices/alba-mackenna/casual.wav" },
    PocketTtsVoiceInfo { id: "anna",    label: "Anna (Female)",   reference_clip: "hf://kyutai/tts-voices/vctk/p228_023_enhanced.wav" },
    PocketTtsVoiceInfo { id: "vera",    label: "Vera (Female)",   reference_clip: "hf://kyutai/tts-voices/vctk/p229_023_enhanced.wav" },
    PocketTtsVoiceInfo { id: "charles", label: "Charles (Male)",  reference_clip: "hf://kyutai/tts-voices/vctk/p254_023_enhanced.wav" },
    PocketTtsVoiceInfo { id: "michael", label: "Michael (Male)",  reference_clip: "hf://kyutai/tts-voices/vctk/p360_023_enhanced.wav" },
];

pub fn pocket_tts_voice(id: &str) -> Option<&'static PocketTtsVoiceInfo> {
    POCKET_TTS_VOICES.iter().find(|v| v.id == id)
}

/// Default directory scanned for user-supplied Pocket-TTS voice clips.
pub fn pocket_tts_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("pocket-tts-voices")
}

fn resolve_pocket_tts_voices_dir(voice_dir: &str) -> PathBuf {
    if voice_dir.is_empty() {
        pocket_tts_voices_dir()
    } else {
        expand_tilde(voice_dir)
    }
}

/// Scans `voice_dir` for `<id>.wav` files, returning `(id, path)` pairs. A file named
/// after a built-in voice (e.g. `alba.wav`) overrides that voice's bundled reference clip.
fn scan_custom_pocket_tts_voices(voice_dir: &str) -> Vec<(String, PathBuf)> {
    let dir = resolve_pocket_tts_voices_dir(voice_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("wav")) != Some(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        found.push((stem.to_string(), path));
    }
    found
}

fn prettify_voice_label(id: &str) -> String {
    id.replace(['_', '-'], " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PocketTtsVoiceOption {
    pub id: String,
    pub label: String,
}

/// Built-in voices merged with any custom clips found in `voice_dir`, for the voice picker.
pub fn pocket_tts_voice_catalogue(voice_dir: &str) -> Vec<PocketTtsVoiceOption> {
    let custom = scan_custom_pocket_tts_voices(voice_dir);
    let mut options: Vec<PocketTtsVoiceOption> = POCKET_TTS_VOICES
        .iter()
        .map(|v| PocketTtsVoiceOption { id: v.id.to_string(), label: v.label.to_string() })
        .collect();

    for (id, _) in &custom {
        if let Some(existing) = options.iter_mut().find(|o| &o.id == id) {
            existing.label = format!("{} (Custom)", prettify_voice_label(id));
        } else {
            options.push(PocketTtsVoiceOption {
                id: id.clone(),
                label: format!("{} (Custom)", prettify_voice_label(id)),
            });
        }
    }
    options
}

/// Resolves a voice id to its reference clip source: either a built-in `hf://` URI or
/// a local path to a custom clip dropped into `voice_dir`. Custom clips take priority.
pub(crate) fn resolve_pocket_tts_voice_clip(id: &str, voice_dir: &str) -> Option<String> {
    let custom = scan_custom_pocket_tts_voices(voice_dir);
    if let Some((_, path)) = custom.iter().find(|(custom_id, _)| custom_id == id) {
        return Some(path.to_string_lossy().into_owned());
    }
    pocket_tts_voice(id).map(|v| v.reference_clip.to_string())
}

// ── Pocket-TTS model variant / sample rate ────────────────────────────────────

pub(crate) const POCKET_TTS_VARIANT: &str = "b6369a24";
const POCKET_TTS_SAMPLE_RATE: u32 = 24000;
// Gated weights repo; tokenizer + non-cloning fallback live in the ungated sibling repo.
const POCKET_TTS_WEIGHTS_REPO: &str = "kyutai/pocket-tts";
const POCKET_TTS_WEIGHTS_REVISION: &str = "427e3d61b276ed69fdd03de0d185fa8a8d97fc5b";
const POCKET_TTS_WEIGHTS_FILE: &str = "tts_b6369a24.safetensors";
const POCKET_TTS_TOKENIZER_REPO: &str = "kyutai/pocket-tts-without-voice-cloning";
const POCKET_TTS_TOKENIZER_REVISION: &str = "d4fdd22ae8c8e1cb3634e150ebeff1dab2d16df3";
const POCKET_TTS_TOKENIZER_FILE: &str = "tokenizer.model";

/// Architecture config for the pocket-tts model variant, shared with the AppImage
/// packaging (which bundles the same file at `usr/config/`, see
/// `load_pocket_tts_model`).
pub const POCKET_TTS_CONFIG_YAML: &str = include_str!("../config/b6369a24.yaml");

/// Ensures `config/<variant>.yaml` exists relative to the current working directory,
/// as well as in the user's local data directory (`~/.local/share/voxctrl/config/<variant>.yaml`),
/// so `pocket_tts::TTSModel::load` can find the required architecture configuration even in
/// read-only runtime environments like AppImages.
pub fn ensure_pocket_tts_config() -> Result<()> {
    // 1. Attempt to write to current working directory (ignore errors if CWD is read-only)
    let cwd_config_dir = Path::new("config");
    let cwd_config_path = cwd_config_dir.join(format!("{POCKET_TTS_VARIANT}.yaml"));
    if !cwd_config_path.exists() {
        if let Ok(()) = std::fs::create_dir_all(cwd_config_dir) {
            let _ = std::fs::write(&cwd_config_path, POCKET_TTS_CONFIG_YAML);
        }
    }

    // 2. Write to user's writable data directory (~/.local/share/voxctrl/config/<variant>.yaml)
    let app_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl");
    let user_config_dir = app_dir.join("config");
    let user_config_path = user_config_dir.join(format!("{POCKET_TTS_VARIANT}.yaml"));
    if !user_config_path.exists() {
        let _ = std::fs::create_dir_all(&user_config_dir);
        let _ = std::fs::write(&user_config_path, POCKET_TTS_CONFIG_YAML);
    }

    Ok(())
}

/// Loads the pocket-tts model for `variant` without leaving the process's working
/// directory changed.
///
/// `pocket_tts::TTSModel::load` only finds its architecture config at
/// `config/<variant>.yaml` relative to the current working directory. When that
/// file is already reachable from the cwd — the AppImage bundles it at `usr/config/`
/// and AppRun starts the app in `usr/`; a dev checkout gets one written by
/// `ensure_pocket_tts_config` — load directly. Only otherwise fall back to
/// temporarily switching into the user's data directory, where
/// `ensure_pocket_tts_config` also wrote a copy.
///
/// Avoiding the switch matters: the cwd is process-wide, and inside the AppImage
/// WebKitGTK locates its helper processes (WebKitNetworkProcess, ...) through a
/// path relative to the cwd. Swapping it from the TTS worker thread while the
/// webview was spawning a helper aborted the whole app at startup.
pub(crate) fn load_pocket_tts_model(variant: &str) -> Result<pocket_tts::TTSModel> {
    ensure_pocket_tts_config().context("ensure pocket-tts config file")?;

    let cwd_config = Path::new("config").join(format!("{variant}.yaml"));
    if cwd_config.exists() {
        return pocket_tts::TTSModel::load(variant);
    }

    let app_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl");
    let orig_cwd = std::env::current_dir().ok();
    if app_dir.exists() {
        let _ = std::env::set_current_dir(&app_dir);
    }
    let load_res = pocket_tts::TTSModel::load(variant);
    if let Some(ref orig) = orig_cwd {
        let _ = std::env::set_current_dir(orig);
    }
    load_res
}

/// Best-effort, network-free check for whether the model weights, tokenizer, and the
/// selected voice's reference clip are already present in the local HuggingFace cache.
pub fn is_pocket_tts_ready(voice: &str, voice_dir: &str) -> bool {
    let cache = hf_hub::Cache::default();

    let weights_present = cache
        .repo(hf_hub::Repo::with_revision(
            POCKET_TTS_WEIGHTS_REPO.to_string(),
            hf_hub::RepoType::Model,
            POCKET_TTS_WEIGHTS_REVISION.to_string(),
        ))
        .get(POCKET_TTS_WEIGHTS_FILE)
        .is_some();

    let tokenizer_present = cache
        .repo(hf_hub::Repo::with_revision(
            POCKET_TTS_TOKENIZER_REPO.to_string(),
            hf_hub::RepoType::Model,
            POCKET_TTS_TOKENIZER_REVISION.to_string(),
        ))
        .get(POCKET_TTS_TOKENIZER_FILE)
        .is_some();

    let voice_present = match resolve_pocket_tts_voice_clip(voice, voice_dir) {
        Some(clip) => hf_cache_file_present(&clip),
        None => false,
    };

    weights_present && tokenizer_present && voice_present
}

fn hf_cache_file_present(hf_path: &str) -> bool {
    let Some(rest) = hf_path.strip_prefix("hf://") else { return Path::new(hf_path).exists() };
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        return false;
    }
    let repo_id = format!("{}/{}", parts[0], parts[1]);
    let filename_with_revision = parts[2..].join("/");
    let (filename, revision) = match filename_with_revision.rfind('@') {
        Some(at) => (filename_with_revision[..at].to_string(), Some(filename_with_revision[at + 1..].to_string())),
        None => (filename_with_revision, None),
    };

    let cache = hf_hub::Cache::default();
    let repo = match revision {
        Some(rev) => hf_hub::Repo::with_revision(repo_id, hf_hub::RepoType::Model, rev),
        None => hf_hub::Repo::model(repo_id),
    };
    cache.repo(repo).get(&filename).is_some()
}

/// Download the pocket-tts model weights, tokenizer, and the selected voice's reference
/// clip into the local HuggingFace cache. Requires `HF_TOKEN` to be set (the default
/// weights repo is gated and requires accepting the model license on huggingface.co).
pub async fn download_pocket_tts_assets(voice: &str, voice_dir: &str, hf_token: Option<String>) -> Result<()> {
    if let Some(token) = hf_token {
        // SAFETY: single-threaded at startup/download time; no concurrent env access.
        unsafe { std::env::set_var("HF_TOKEN", token) };
    }

    let reference_clip = resolve_pocket_tts_voice_clip(voice, voice_dir)
        .ok_or_else(|| anyhow::anyhow!("unknown pocket-tts voice: {voice}"))?;

    tokio::task::spawn_blocking(move || -> Result<()> {
        ensure_pocket_tts_config().context("ensure pocket-tts config file")?;

        info!("Downloading pocket-tts model weights ({POCKET_TTS_VARIANT})...");
        pocket_tts::weights::download_if_necessary(&format!(
            "hf://{POCKET_TTS_WEIGHTS_REPO}/{POCKET_TTS_WEIGHTS_FILE}@{POCKET_TTS_WEIGHTS_REVISION}"
        ))
        .context("download pocket-tts model weights")?;

        info!("Downloading pocket-tts tokenizer...");
        pocket_tts::weights::download_if_necessary(&format!(
            "hf://{POCKET_TTS_TOKENIZER_REPO}/{POCKET_TTS_TOKENIZER_FILE}@{POCKET_TTS_TOKENIZER_REVISION}"
        ))
        .context("download pocket-tts tokenizer")?;

        info!("Downloading pocket-tts reference voice clip: {reference_clip}");
        pocket_tts::weights::download_if_necessary(&reference_clip)
            .context("download pocket-tts reference voice clip")?;

        Ok(())
    })
    .await
    .context("download_pocket_tts_assets task join")??;

    info!("pocket-tts assets ready for voice '{voice}'");
    Ok(())
}

// ── pocket-tts synthesis (pure Rust / Candle) ─────────────────────────────────

/// Called from `TtsEngineWorker::run` (in `engine.rs`) when `config.engine ==
/// TtsEngine::PocketTts`. Takes the worker's model/voice-state caches by
/// mutable reference so they persist across calls for the worker's lifetime.
pub(crate) fn speak_pocket_tts(
    config: &voxctrl_config::TtsConfig,
    u: &crate::engine::Utterance,
    model: &mut Option<pocket_tts::TTSModel>,
    voice_states: &mut HashMap<String, pocket_tts::ModelState>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
) -> Result<()> {
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let voice = u.voice.as_deref().unwrap_or(&config.pocket_tts.voice);

    if !is_pocket_tts_ready(voice, &config.pocket_tts.voice_dir) {
        anyhow::bail!("pocket-tts assets for voice '{voice}' not found. Download them from TTS settings.");
    }

    // Lazily load the model — stays alive for the worker thread lifetime.
    if model.is_none() {
        info!("Loading pocket-tts model (variant={POCKET_TTS_VARIANT})");
        *model =
            Some(load_pocket_tts_model(POCKET_TTS_VARIANT).context("load pocket-tts model")?);
    }
    let model = model.as_ref().unwrap();

    if !voice_states.contains_key(voice) {
        let reference_clip = resolve_pocket_tts_voice_clip(voice, &config.pocket_tts.voice_dir)
            .ok_or_else(|| anyhow::anyhow!("unknown pocket-tts voice: {voice}"))?;
        let clip_path = pocket_tts::weights::download_if_necessary(&reference_clip)
            .context("resolve pocket-tts reference voice clip")?;
        let state = model
            .get_voice_state(&clip_path)
            .context("compute pocket-tts voice state")?;
        voice_states.insert(voice.to_string(), state);
    }
    let voice_state = voice_states.get(voice).unwrap();

    if is_prewarm {
        // Run generation once to warm the model and caches; nothing is played.
        let _ = model.generate(&u.text, voice_state).context("pocket-tts generate")?;
        return Ok(());
    }

    // Stream audio frame-by-frame instead of waiting for the whole utterance to
    // finish generating: each frame is queued onto the sink as soon as it's ready,
    // so playback of the first frame overlaps with generation of the rest. This cuts
    // perceived latency from "time to generate the whole sentence" down to roughly
    // "time to generate the first frame".
    let mut callback_fired = false;
    for chunk in model.generate_stream(&u.text, voice_state) {
        if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != generation {
            break; // stop() was called — abandon the rest of the generation
        }
        let chunk = chunk.context("pocket-tts generate (stream)")?;
        let chunk = chunk.squeeze(0).context("squeeze pocket-tts audio chunk")?;
        let bytes =
            pocket_tts::audio::pcm_i16_le_bytes(&chunk).context("encode pocket-tts audio chunk")?;

        if !callback_fired {
            callback_fired = true;
            if let Some(ref cb) = on_playback_start {
                cb();
            }
        }

        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        sink.append(rodio::buffer::SamplesBuffer::new(1, POCKET_TTS_SAMPLE_RATE, samples));
    }
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── Pocket-TTS voice catalogue ──────────────────────────────────────────────

    #[test]
    fn test_pocket_tts_voices_not_empty() {
        assert!(!POCKET_TTS_VOICES.is_empty());
    }

    #[test]
    fn test_pocket_tts_voices_have_required_fields() {
        for v in POCKET_TTS_VOICES {
            assert!(!v.id.is_empty());
            assert!(!v.label.is_empty());
            assert!(v.reference_clip.starts_with("hf://"));
        }
    }

    #[test]
    fn test_pocket_tts_voices_ids_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in POCKET_TTS_VOICES {
            assert!(seen.insert(v.id), "duplicate voice id: {}", v.id);
        }
    }

    #[test]
    fn test_pocket_tts_voice_lookup_known() {
        assert!(pocket_tts_voice("alba").is_some());
        assert!(pocket_tts_voice("michael").is_some());
    }

    #[test]
    fn test_pocket_tts_voice_lookup_unknown_returns_none() {
        assert!(pocket_tts_voice("not-a-real-voice").is_none());
    }

    // ── hf_cache_file_present ────────────────────────────────────────────────

    #[test]
    fn test_hf_cache_file_present_missing_returns_false() {
        assert!(!hf_cache_file_present("hf://kyutai/tts-voices/does-not-exist.wav"));
    }

    #[test]
    fn test_hf_cache_file_present_non_hf_path_checks_filesystem() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("clip.wav");
        fs::write(&file, b"fake audio").unwrap();
        assert!(hf_cache_file_present(file.to_str().unwrap()));
    }

    // ── is_pocket_tts_ready ──────────────────────────────────────────────────

    #[test]
    fn test_is_pocket_tts_ready_false_for_unknown_voice() {
        assert!(!is_pocket_tts_ready("not-a-real-voice", ""));
    }

    // ── custom voice directory ───────────────────────────────────────────────

    #[test]
    fn test_scan_custom_pocket_tts_voices_finds_wav_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("myvoice.wav"), b"fake audio").unwrap();
        fs::write(dir.path().join("notes.txt"), b"ignore me").unwrap();
        let found = scan_custom_pocket_tts_voices(dir.path().to_str().unwrap());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "myvoice");
    }

    #[test]
    fn test_pocket_tts_voice_catalogue_merges_custom_voices() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("myvoice.wav"), b"fake audio").unwrap();
        let catalogue = pocket_tts_voice_catalogue(dir.path().to_str().unwrap());
        assert!(catalogue.iter().any(|v| v.id == "myvoice"));
        assert!(catalogue.iter().any(|v| v.id == "alba"));
    }

    #[test]
    fn test_pocket_tts_voice_catalogue_custom_overrides_builtin_label() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alba.wav"), b"fake audio").unwrap();
        let catalogue = pocket_tts_voice_catalogue(dir.path().to_str().unwrap());
        let alba = catalogue.iter().find(|v| v.id == "alba").unwrap();
        assert!(alba.label.contains("Custom"));
    }

    #[test]
    fn test_resolve_pocket_tts_voice_clip_prefers_custom() {
        let dir = tempdir().unwrap();
        let clip = dir.path().join("alba.wav");
        fs::write(&clip, b"fake audio").unwrap();
        let resolved = resolve_pocket_tts_voice_clip("alba", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(resolved, clip.to_string_lossy());
    }

    #[test]
    fn test_resolve_pocket_tts_voice_clip_falls_back_to_builtin() {
        let dir = tempdir().unwrap();
        let resolved = resolve_pocket_tts_voice_clip("alba", dir.path().to_str().unwrap()).unwrap();
        assert!(resolved.starts_with("hf://"));
    }

    #[test]
    fn test_resolve_pocket_tts_voice_clip_unknown_returns_none() {
        let dir = tempdir().unwrap();
        assert!(resolve_pocket_tts_voice_clip("not-a-real-voice", dir.path().to_str().unwrap()).is_none());
    }

    #[test]
    fn test_ensure_pocket_tts_config_creates_file() {
        ensure_pocket_tts_config().expect("ensure config");
        let path = Path::new("config").join(format!("{POCKET_TTS_VARIANT}.yaml"));
        assert!(path.exists());
    }
}
