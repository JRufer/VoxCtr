//! Inflect-Micro-v2 (<https://huggingface.co/owensong/Inflect-Micro-v2>) — a
//! ~9.4M-parameter VITS-family text-to-waveform model, released under Apache 2.0,
//! producing 24 kHz mono audio from a single fixed English voice.
//!
//! Unlike Piper (which shells out to a separate binary) and Pocket-TTS (which
//! clones a voice from a reference clip), Inflect runs in-process through ONNX
//! Runtime and has no voice to select — so its settings are the sampling seed and
//! the two VITS noise scales rather than a voice picker.
//!
//! Split by concern:
//! - [`phonemes`] — eSpeak-NG IPA frontend and the phoneme→id vocabulary
//! - [`model`]    — the `duration.onnx` / `decode.onnx` pair and the synthesis path
//!
//! # Build feature
//!
//! The ONNX half sits behind the `inflect-micro` cargo feature so the default
//! build doesn't pull in ONNX Runtime. [`INFLECT_MICRO_COMPILED`] reports whether
//! it was built in, which the settings UI surfaces so choosing the engine in a
//! build without it fails loudly rather than silently doing nothing.
//!
//! # Status of the tensor contract
//!
//! The tensor names in [`model`] were written without access to the published
//! export — `huggingface.co` was blocked by network policy at the time — so they
//! follow conventional VITS export naming. Inputs are bound **by name** at load
//! time and any unresolved name is a hard error listing the real signature, so a
//! mismatch shows up as an actionable message instead of broken audio. The
//! `inflect_micro_inspect` Tauri command prints that signature for a downloaded
//! model; correcting the alias tables in `model.rs` is then a one-place edit.

pub mod phonemes;

#[cfg(feature = "inflect-micro")]
pub mod model;

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::info;

use crate::piper::expand_tilde;

// ── Model constants ───────────────────────────────────────────────────────────

/// Inflect-Micro-v2 emits 24 kHz mono audio.
pub const SAMPLE_RATE: u32 = 24_000;

/// Stage 1: phoneme ids → aligned latent sequence.
pub const DURATION_FILE: &str = "duration.onnx";
/// Stage 2: latent sequence → waveform.
pub const DECODE_FILE: &str = "decode.onnx";

/// The graphs that must be on disk before the engine can load.
pub const MODEL_FILES: [&str; 2] = [DURATION_FILE, DECODE_FILE];

/// Whether this build includes the ONNX Runtime half of the engine.
pub const INFLECT_MICRO_COMPILED: bool = cfg!(feature = "inflect-micro");

/// Upstream weights on the Hugging Face hub. Kept as one constant so a hosting
/// change is a single edit; users can also point `model_dir` at a local copy and
/// never download anything.
const HF_BASE_URL: &str = "https://huggingface.co/owensong/Inflect-Micro-v2/resolve/main/onnx";

// ── Filesystem layout ─────────────────────────────────────────────────────────

/// Default model directory: `<data-local>/voxctrl/models/inflect-micro/`, keeping
/// the same `models/<engine>` layout the STT backends use.
pub fn inflect_micro_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("inflect-micro")
}

/// Resolve the configured directory, falling back to the platform default.
pub fn resolve_model_dir(model_dir: &str) -> PathBuf {
    if model_dir.is_empty() {
        inflect_micro_model_dir()
    } else {
        expand_tilde(model_dir)
    }
}

/// True when both ONNX graphs and a phoneme vocabulary are present.
///
/// The vocabulary is part of the check because synthesis cannot be correct
/// without it — see [`phonemes::PhonemeVocab::load`].
pub fn is_inflect_micro_downloaded(model_dir: &str) -> bool {
    let dir = resolve_model_dir(model_dir);
    let graphs = MODEL_FILES.iter().all(|f| dir.join(f).exists());
    let vocab = phonemes::VOCAB_FILES.iter().any(|f| dir.join(f).exists());
    graphs && vocab
}

// ── Download ──────────────────────────────────────────────────────────────────

/// Serializes downloads so a Settings click and an on-demand load can't fight
/// over the same files.
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Fetch the model into `model_dir` (or the platform default). Files already
/// present are skipped, so this is safe to call repeatedly.
///
/// The vocabulary file name isn't fixed across VITS exports, so each candidate in
/// [`phonemes::VOCAB_FILES`] is tried and the first that exists upstream is kept.
pub async fn download_inflect_micro_assets(model_dir: &str) -> Result<()> {
    let dir = resolve_model_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create model dir {}", dir.display()))?;

    let _guard = DOWNLOAD_LOCK.lock().await;

    for file in MODEL_FILES {
        let path = dir.join(file);
        if path.exists() {
            continue;
        }
        let url = format!("{HF_BASE_URL}/{file}");
        info!("Downloading Inflect-Micro-v2 graph: {url}");
        download_to(&url, &path)
            .await
            .with_context(|| format!("download {file}"))?;
    }

    if !phonemes::VOCAB_FILES.iter().any(|f| dir.join(f).exists()) {
        let mut fetched = false;
        for candidate in phonemes::VOCAB_FILES {
            let url = format!("{HF_BASE_URL}/{candidate}");
            match download_to(&url, &dir.join(candidate)).await {
                Ok(()) => {
                    info!("Downloaded Inflect-Micro-v2 phoneme vocabulary: {candidate}");
                    fetched = true;
                    break;
                }
                // A candidate that isn't published is expected; try the next.
                Err(_) => continue,
            }
        }
        if !fetched {
            anyhow::bail!(
                "Downloaded the ONNX graphs but found no phoneme vocabulary upstream \
                 (tried {}). Place the model's phoneme table in {} manually.",
                phonemes::VOCAB_FILES.join(", "),
                dir.display()
            );
        }
    }

    info!("Inflect-Micro-v2 ready in {}", dir.display());
    Ok(())
}

/// Download one URL to `path`, writing via a `.part` temp file so an interrupted
/// transfer never leaves a truncated file that later looks "present".
async fn download_to(url: &str, path: &std::path::Path) -> Result<()> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("fetch {url}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("read {url}"))?;

    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, &bytes)
        .await
        .with_context(|| format!("write {}", tmp.display()))?;
    tokio::fs::rename(&tmp, path)
        .await
        .with_context(|| format!("finalize {}", path.display()))?;
    Ok(())
}

// ── Text chunking ─────────────────────────────────────────────────────────────

/// Roughly how many characters to synthesize per ONNX call.
///
/// The graphs produce a whole utterance in one shot with no streaming API, so
/// long text is split and played chunk by chunk: playback of the first chunk
/// overlaps generation of the rest, which cuts perceived latency from "generate
/// the whole response" to "generate the first sentence". It also bounds peak
/// memory, since VITS activations scale with sequence length.
pub const CHUNK_TARGET_CHARS: usize = 220;

/// Split `text` into synthesis chunks at sentence boundaries, packing sentences
/// up to [`CHUNK_TARGET_CHARS`]. A single sentence longer than the target is left
/// whole rather than cut mid-phrase — prosody matters more than the bound.
pub fn chunk_text(text: &str) -> Vec<String> {
    let sentences = split_sentences(text);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for sentence in sentences {
        if current.is_empty() {
            current = sentence;
        } else if current.len() + 1 + sentence.len() <= CHUNK_TARGET_CHARS {
            current.push(' ');
            current.push_str(&sentence);
        } else {
            chunks.push(std::mem::take(&mut current));
            current = sentence;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Split into sentences, keeping terminal punctuation attached.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for c in text.chars() {
        current.push(c);
        if matches!(c, '.' | '!' | '?' | '\n') {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    out
}

// ── Synthesis ─────────────────────────────────────────────────────────────────

/// Called from `TtsEngineWorker::run` when `config.engine == TtsEngine::InflectMicro`.
///
/// Takes the worker's model cache by mutable reference so the loaded ONNX
/// sessions persist for the worker thread's lifetime, matching how
/// `speak_pocket_tts` caches its model.
#[cfg(feature = "inflect-micro")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn speak_inflect_micro(
    config: &voxctrl_config::TtsConfig,
    u: &crate::engine::Utterance,
    model: &mut Option<model::InflectModel>,
    on_playback_start: &Option<crate::engine::PlaybackCallback>,
    sink: &rodio::Sink,
    generation_counter: &std::sync::Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
) -> Result<()> {
    use std::sync::atomic::Ordering;

    let cfg = &config.inflect_micro;
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");

    if !is_inflect_micro_downloaded(&cfg.model_dir) {
        anyhow::bail!(
            "Inflect-Micro-v2 model files not found in {}. Download them from TTS settings.",
            resolve_model_dir(&cfg.model_dir).display()
        );
    }

    // Lazily load — the sessions stay alive for the worker thread's lifetime.
    if model.is_none() {
        let dir = resolve_model_dir(&cfg.model_dir);
        *model = Some(model::InflectModel::load(&dir)?);
    }
    let model = model.as_mut().unwrap();

    if is_prewarm {
        // Run one short synthesis to warm the sessions; nothing is played.
        let _ = model.synthesize("warm up", cfg, config.speed)?;
        return Ok(());
    }

    let mut callback_fired = false;
    for chunk in chunk_text(&u.text) {
        if generation_counter.load(Ordering::SeqCst) != generation {
            break; // stop() was called — abandon the rest of the utterance
        }

        let audio = model.synthesize(&chunk, cfg, config.speed)?;
        if audio.is_empty() {
            continue;
        }

        // Re-check after synthesis: a chunk can take long enough that stop()
        // lands mid-generation, and appending would restart a stopped sink.
        if generation_counter.load(Ordering::SeqCst) != generation {
            break;
        }

        if !callback_fired {
            callback_fired = true;
            if let Some(ref cb) = on_playback_start {
                cb();
            }
        }

        sink.append(rodio::buffer::SamplesBuffer::new(1, SAMPLE_RATE, audio));
    }

    sink.sleep_until_end();
    Ok(())
}

/// Stand-in used when the crate is built without the `inflect-micro` feature, so
/// selecting the engine reports why it can't run instead of failing obscurely.
#[cfg(not(feature = "inflect-micro"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn speak_inflect_micro(
    _config: &voxctrl_config::TtsConfig,
    _u: &crate::engine::Utterance,
    _model: &mut Option<()>,
    _on_playback_start: &Option<crate::engine::PlaybackCallback>,
    _sink: &rodio::Sink,
    _generation_counter: &std::sync::Arc<std::sync::atomic::AtomicU32>,
    _generation: u32,
) -> Result<()> {
    anyhow::bail!(
        "This build of VoxCtrl was compiled without the `inflect-micro` feature, so \
         the Inflect-Micro-v2 engine is unavailable. Rebuild with \
         `--features inflect-micro`, or pick another TTS engine in settings."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── Filesystem layout ────────────────────────────────────────────────────

    #[test]
    fn test_model_dir_has_inflect_micro_segment() {
        assert!(inflect_micro_model_dir().ends_with("inflect-micro"));
    }

    #[test]
    fn test_resolve_model_dir_empty_uses_default() {
        assert_eq!(resolve_model_dir(""), inflect_micro_model_dir());
    }

    #[test]
    fn test_resolve_model_dir_honours_explicit_path() {
        assert_eq!(resolve_model_dir("/opt/models"), PathBuf::from("/opt/models"));
    }

    // ── Readiness ────────────────────────────────────────────────────────────

    #[test]
    fn test_not_downloaded_when_dir_empty() {
        let dir = tempdir().unwrap();
        assert!(!is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_not_downloaded_when_vocab_missing() {
        let dir = tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), b"not a real graph").unwrap();
        }
        assert!(
            !is_inflect_micro_downloaded(dir.path().to_str().unwrap()),
            "graphs alone are not enough — the phoneme table is required"
        );
    }

    #[test]
    fn test_not_downloaded_when_one_graph_missing() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(DURATION_FILE), b"x").unwrap();
        std::fs::write(dir.path().join("phonemes.json"), b"{}").unwrap();
        assert!(!is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_downloaded_when_graphs_and_vocab_present() {
        let dir = tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        std::fs::write(dir.path().join("tokens.txt"), b"_ 0\n").unwrap();
        assert!(is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    // ── Sentence splitting ───────────────────────────────────────────────────

    #[test]
    fn test_split_sentences_keeps_terminal_punctuation() {
        let s = split_sentences("One. Two! Three?");
        assert_eq!(s, vec!["One.", "Two!", "Three?"]);
    }

    #[test]
    fn test_split_sentences_handles_trailing_fragment() {
        let s = split_sentences("Complete. Incomplete");
        assert_eq!(s, vec!["Complete.", "Incomplete"]);
    }

    #[test]
    fn test_split_sentences_breaks_on_newline() {
        let s = split_sentences("Line one\nLine two");
        assert_eq!(s, vec!["Line one", "Line two"]);
    }

    #[test]
    fn test_split_sentences_empty_input() {
        assert!(split_sentences("").is_empty());
        assert!(split_sentences("   \n  ").is_empty());
    }

    // ── Chunking ─────────────────────────────────────────────────────────────

    #[test]
    fn test_chunk_text_packs_short_sentences_together() {
        let chunks = chunk_text("One. Two. Three.");
        assert_eq!(chunks.len(), 1, "short sentences share a chunk");
        assert_eq!(chunks[0], "One. Two. Three.");
    }

    #[test]
    fn test_chunk_text_splits_once_over_target() {
        let sentence = "a".repeat(150) + ".";
        let text = format!("{sentence} {sentence}");
        let chunks = chunk_text(&text);
        assert_eq!(chunks.len(), 2, "two 151-char sentences exceed the 220 target");
    }

    #[test]
    fn test_chunk_text_keeps_overlong_sentence_whole() {
        // Splitting mid-phrase would wreck prosody, so an overlong sentence stays intact.
        let long = "b".repeat(CHUNK_TARGET_CHARS * 2) + ".";
        let chunks = chunk_text(&long);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), long.len());
    }

    #[test]
    fn test_chunk_text_empty_input_yields_no_chunks() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   ").is_empty());
    }

    #[test]
    fn test_chunk_text_preserves_all_content() {
        let text = "First sentence. Second sentence! Third one? And a fragment";
        let rejoined = chunk_text(text).join(" ");
        for word in ["First", "Second", "Third", "fragment"] {
            assert!(rejoined.contains(word), "lost {word:?}");
        }
    }

    // ── Build gating ─────────────────────────────────────────────────────────

    #[test]
    fn test_compiled_flag_tracks_feature() {
        assert_eq!(INFLECT_MICRO_COMPILED, cfg!(feature = "inflect-micro"));
    }
}
