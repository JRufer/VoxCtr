//! Inflect-Micro-v2 (<https://huggingface.co/owensong/Inflect-Micro-v2>) — a
//! ~9.4M-parameter VITS-family text-to-waveform model, released under Apache 2.0,
//! producing 24 kHz mono audio from a single fixed English voice.
//!
//! Unlike Piper (which shells out to a separate binary) and Pocket-TTS (which
//! clones a voice from a reference clip), Inflect runs in-process through ONNX
//! Runtime and has no voice to select — so its settings are the sampling seed and
//! the latent sampling temperature rather than a voice picker.
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
//! # Tensor contract
//!
//! The graph signatures, tokenization, chunking, boundary pauses and edge fade
//! all follow the export's own `inference_onnx.py`. The one deliberate
//! divergence is the latent-noise RNG: the reference draws it with NumPy, which
//! this cannot reproduce stream-for-stream, so a given seed does not select the
//! same sample here as it does there. See `model::StandardNormal`.

pub mod phonemes;

#[cfg(feature = "inflect-micro")]
pub mod model;

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{info, warn};

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

/// A repository plus the subdirectory the export lives in.
#[derive(Debug, Clone, Copy)]
struct Layout {
    repo: &'static str,
    /// Subdirectory within the repo; empty for the repo root.
    subdir: &'static str,
}

/// Candidate upstream locations, tried in order. The publisher ships the
/// verified FP32 export in a separate `-ONNX` repository alongside the PyTorch
/// release, keeping the graphs under `onnx/`.
const CANDIDATE_LAYOUTS: [Layout; 4] = [
    Layout { repo: "owensong/Inflect-Micro-v2-ONNX", subdir: "onnx" },
    Layout { repo: "owensong/Inflect-Micro-v2-ONNX", subdir: "" },
    Layout { repo: "owensong/Inflect-Micro-v2", subdir: "onnx" },
    Layout { repo: "owensong/Inflect-Micro-v2", subdir: "" },
];

impl Layout {
    /// The hub API endpoint listing this layout's files.
    fn tree_url(&self) -> String {
        if self.subdir.is_empty() {
            format!("https://huggingface.co/api/models/{}/tree/main", self.repo)
        } else {
            format!("https://huggingface.co/api/models/{}/tree/main/{}", self.repo, self.subdir)
        }
    }

    /// The download URL for one file in this layout.
    fn file_url(&self, file: &str) -> String {
        if self.subdir.is_empty() {
            format!("https://huggingface.co/{}/resolve/main/{file}", self.repo)
        } else {
            format!("https://huggingface.co/{}/resolve/main/{}/{file}", self.repo, self.subdir)
        }
    }
}

/// Upper bound on an auxiliary file fetched alongside the graphs. The phoneme
/// table is a few kilobytes; this only exists to stop a stray large asset from
/// being pulled in by the "fetch every small text file" rule.
const MAX_AUX_FILE_BYTES: u64 = 4 * 1024 * 1024;

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
    if !MODEL_FILES.iter().all(|f| dir.join(f).exists()) {
        return false;
    }
    // Detected by parsing rather than by filename, so this agrees with what
    // `InflectModel::load` will actually accept.
    matches!(phonemes::PhonemeVocab::load(&dir), Ok(Some(_)))
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

    let (layout, listing) = resolve_layout().await?;

    for file in MODEL_FILES {
        let path = dir.join(file);
        if path.exists() {
            continue;
        }
        let url = layout.file_url(file);
        info!("Downloading Inflect-Micro-v2 graph: {url}");
        download_to(&url, &path)
            .await
            .with_context(|| format!("download {file}"))?;
    }

    // Fetch every small text-ish file alongside the graphs rather than guessing
    // what the phoneme table is called. Which one actually *is* the table is
    // decided by parsing it — see `PhonemeVocab::load`.
    for entry in listing.iter().filter(|e| e.is_auxiliary()) {
        let path = dir.join(&entry.name);
        if path.exists() {
            continue;
        }
        let url = layout.file_url(&entry.name);
        match download_to(&url, &path).await {
            Ok(()) => info!("Downloaded Inflect-Micro-v2 auxiliary file: {}", entry.name),
            // Auxiliary files are best-effort; the vocabulary check below is
            // what decides whether the download actually succeeded.
            Err(e) => warn!("Could not fetch {}: {e:#}", entry.name),
        }
    }

    if phonemes::PhonemeVocab::load(&dir)?.is_none() {
        let names: Vec<&str> = listing.iter().map(|e| e.name.as_str()).collect();
        anyhow::bail!(
            "Downloaded the ONNX graphs from {}, but none of the accompanying files \
             parse as a phoneme table. The table maps eSpeak IPA to the model's \
             phoneme ids and synthesis cannot be correct without it.\n\
             Files published there: {}\n\
             This export ships the symbol list inside its reference script rather \
             than as a standalone file; the scripts are downloaded to {} so the \
             table can be recovered from them.",
            layout.file_url("").trim_end_matches('/'),
            if names.is_empty() { "(none listed)".to_string() } else { names.join(", ") },
            dir.display()
        );
    }

    info!("Inflect-Micro-v2 ready in {}", dir.display());
    Ok(())
}

/// One file entry from the hub's tree listing.
#[derive(Debug, Clone)]
struct RepoFile {
    name: String,
    size: u64,
}

impl RepoFile {
    /// Whether this is a small non-graph file worth fetching alongside the
    /// graphs — the phoneme table is one of these, whatever it is called.
    ///
    /// `.py` is included because this export ships no standalone table: the
    /// symbol list lives inside the reference script (`inference_onnx.py`).
    /// Those files are only ever read as data — nothing here executes them.
    fn is_auxiliary(&self) -> bool {
        if self.size > MAX_AUX_FILE_BYTES {
            return false;
        }
        let lower = self.name.to_ascii_lowercase();
        if lower.ends_with(".onnx") || lower.ends_with(".onnx_data") {
            return false;
        }
        [".json", ".txt", ".csv", ".tsv", ".py"]
            .iter()
            .any(|e| lower.ends_with(e))
    }
}

/// Find which candidate layout actually hosts the export by listing each through
/// the hub API and looking for [`DURATION_FILE`].
///
/// Listing rather than probing a guessed filename means the phoneme table is
/// discovered instead of assumed. On failure the error names every endpoint
/// tried and what it returned.
async fn resolve_layout() -> Result<(Layout, Vec<RepoFile>)> {
    let client = reqwest::Client::new();
    let mut attempts = Vec::with_capacity(CANDIDATE_LAYOUTS.len());

    for layout in CANDIDATE_LAYOUTS {
        let url = layout.tree_url();
        match fetch_listing(&client, &url).await {
            Ok(files) => {
                if files.iter().any(|f| f.name == DURATION_FILE) {
                    info!("Inflect-Micro-v2 export found in {} ({} files)", url, files.len());
                    return Ok((layout, files));
                }
                attempts.push(format!("  {url} → listed, but no {DURATION_FILE}"));
            }
            Err(e) => attempts.push(format!("  {url} → {e}")),
        }
    }

    anyhow::bail!(
        "Could not find the Inflect-Micro-v2 ONNX export at any known location.\n\
         Tried:\n{}\n\
         If the model has moved, download {} plus its phoneme table by hand and \
         point the model directory at them in TTS settings.",
        attempts.join("\n"),
        MODEL_FILES.join(" + ")
    )
}

/// Fetch and parse one hub tree listing into its file entries (directories and
/// anything without a usable path are skipped).
async fn fetch_listing(client: &reqwest::Client, url: &str) -> Result<Vec<RepoFile>> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("fetch {url}"))?;
    let body = response.text().await.with_context(|| format!("read {url}"))?;
    parse_listing(&body).with_context(|| format!("parse listing from {url}"))
}

/// Parse the hub's tree JSON: an array of `{type, path, size}` objects, where
/// `path` is repo-relative and so carries the subdirectory prefix.
fn parse_listing(body: &str) -> Result<Vec<RepoFile>> {
    let value: serde_json::Value = serde_json::from_str(body).context("invalid JSON")?;
    let array = value.as_array().context("expected a JSON array")?;

    let mut files = Vec::with_capacity(array.len());
    for entry in array {
        if entry.get("type").and_then(|t| t.as_str()) != Some("file") {
            continue;
        }
        let Some(path) = entry.get("path").and_then(|p| p.as_str()) else { continue };
        // Keep only the basename; downloads are addressed relative to the subdir.
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        if name.is_empty() {
            continue;
        }
        files.push(RepoFile {
            name,
            size: entry.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
        });
    }
    Ok(files)
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

/// Maximum characters per synthesis call, matching `split_text`'s default in the
/// export's reference script.
pub const CHUNK_LIMIT_CHARS: usize = 280;

/// Length of the taper applied to each end of a chunk, matching `edge_fade`.
pub const EDGE_FADE_MS: f32 = 5.0;

/// Silence inserted after a chunk, keyed by the punctuation that ended it.
/// Values are the reference script's `boundary_pause_seconds`.
pub fn boundary_pause_seconds(chunk: &str) -> f32 {
    match chunk.trim_end().chars().last() {
        Some('?') => 0.28,
        Some('!') => 0.24,
        Some('.') => 0.22,
        Some(';') => 0.16,
        Some(':') => 0.13,
        Some(',') => 0.09,
        _ => 0.08,
    }
}

/// Taper the first and last 5 ms of a chunk so concatenation doesn't click.
/// Mirrors the reference script's `edge_fade`.
pub fn edge_fade(waveform: &mut [f32], sample_rate: u32, milliseconds: f32) {
    let frames = ((sample_rate as f32 * milliseconds / 1000.0).round() as usize)
        .min(waveform.len() / 2);
    if frames == 0 {
        return;
    }
    let last = frames.saturating_sub(1).max(1) as f32;
    for i in 0..frames {
        let ramp = i as f32 / last;
        waveform[i] *= ramp;
        let end = waveform.len() - 1 - i;
        waveform[end] *= ramp;
    }
}

/// Split `text` into synthesis chunks, following the reference `split_text`.
///
/// Whitespace is normalised, the text is broken after sentence-ending
/// punctuation, and any sentence still over [`CHUNK_LIMIT_CHARS`] is split again
/// at the last comma/semicolon/colon in range (or the last space, or hard at the
/// limit). Sentences are *not* packed together — each is its own chunk, which is
/// what makes the per-chunk boundary pauses land in the right places.
pub fn chunk_text(text: &str) -> Vec<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    for sentence in split_sentences(&normalized) {
        let mut sentence = sentence;
        // `.len()` is bytes; the reference counts characters, so work in chars.
        while sentence.chars().count() > CHUNK_LIMIT_CHARS {
            let split_at = pick_split_point(&sentence, CHUNK_LIMIT_CHARS);
            let head: String = sentence.chars().take(split_at).collect();
            let tail: String = sentence.chars().skip(split_at).collect();
            let head = head.trim().to_string();
            if !head.is_empty() {
                chunks.push(head);
            }
            sentence = tail.trim().to_string();
            if sentence.is_empty() {
                break;
            }
        }
        if !sentence.is_empty() {
            chunks.push(sentence);
        }
    }
    chunks
}

/// Choose where to cut an over-long sentence: the last `,`/`;`/`:` within the
/// limit if it is past the halfway point, else the last space, else hard.
fn pick_split_point(sentence: &str, limit: usize) -> usize {
    let chars: Vec<char> = sentence.chars().collect();
    let search_end = (limit + 1).min(chars.len());
    let window = &chars[..search_end];

    let punctuation = window
        .iter()
        .rposition(|c| matches!(c, ',' | ';' | ':'))
        .map(|i| i + 1);

    match punctuation {
        Some(p) if p >= limit / 2 => p,
        _ => match window.iter().rposition(|c| *c == ' ') {
            Some(space) if space >= limit / 2 => space,
            _ => limit,
        },
    }
}

/// Break after `.`/`!`/`?`/`;`/`:` followed by whitespace. The `regex` crate has
/// no lookbehind, and this is the whole of what the reference pattern does.
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut i = 0;

    while i < chars.len() {
        current.push(chars[i]);
        let is_terminator = matches!(chars[i], '.' | '!' | '?' | ';' | ':');
        let next_is_space = chars.get(i + 1).is_some_and(|c| c.is_whitespace());
        if is_terminator && next_is_space {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
            current.clear();
            i += 1; // consume the separating space
        }
        i += 1;
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
        // One short synthesis to warm the sessions; nothing is played.
        let _ = model.synthesize("warm up", cfg, config.speed, cfg.seed)?;
        return Ok(());
    }

    let chunks = chunk_text(&u.text);
    let mut callback_fired = false;

    for (index, chunk) in chunks.iter().enumerate() {
        if generation_counter.load(Ordering::SeqCst) != generation {
            break; // stop() was called — abandon the rest of the utterance
        }

        // The reference advances the seed per chunk so consecutive sentences
        // don't draw identical latent noise.
        let seed = cfg.seed.wrapping_add(index as u64);
        let mut audio = model.synthesize(chunk, cfg, config.speed, seed)?;
        if audio.is_empty() {
            continue;
        }
        edge_fade(&mut audio, SAMPLE_RATE, EDGE_FADE_MS);

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

        // Silence between chunks, sized by how the previous one ended.
        if index > 0 {
            let pause = boundary_pause_seconds(&chunks[index - 1]);
            let frames = (SAMPLE_RATE as f32 * pause).round() as usize;
            if frames > 0 {
                sink.append(rodio::buffer::SamplesBuffer::new(
                    1,
                    SAMPLE_RATE,
                    vec![0.0f32; frames],
                ));
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
        write_vocab(dir.path(), "phonemes.json");
        assert!(!is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_downloaded_when_graphs_and_vocab_present() {
        let dir = tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        write_vocab(dir.path(), "tokens.txt");
        assert!(is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_downloaded_recognises_vocab_under_an_unexpected_name() {
        // The export's table is not reliably named; readiness must agree with
        // what `PhonemeVocab::load` accepts, which is decided by parsing.
        let dir = tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        write_vocab(dir.path(), "inflect_symbols.txt");
        assert!(is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_not_downloaded_when_only_a_hyperparameter_config_is_present() {
        // A config.json of model hyperparameters must not be mistaken for a table.
        let dir = tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        std::fs::write(
            dir.path().join("config.json"),
            br#"{"sample_rate": 24000, "hidden_channels": 192, "n_layers": 6}"#,
        )
        .unwrap();
        assert!(!is_inflect_micro_downloaded(dir.path().to_str().unwrap()));
    }

    /// Write a table with enough short symbols to pass the plausibility check.
    fn write_vocab(dir: &std::path::Path, name: &str) {
        let mut body = String::new();
        for (i, c) in "_^$abdefhijklmnopqrstuvwxyzəɪˈː".chars().enumerate() {
            body.push_str(&format!("{c} {i}\n"));
        }
        if name.ends_with(".json") {
            let entries: Vec<String> = body
                .lines()
                .map(|l| {
                    let (s, i) = l.rsplit_once(' ').unwrap();
                    format!("{}: {i}", serde_json::to_string(s).unwrap())
                })
                .collect();
            std::fs::write(dir.join(name), format!("{{{}}}", entries.join(","))).unwrap();
        } else {
            std::fs::write(dir.join(name), body).unwrap();
        }
    }

    // ── Repository listing ───────────────────────────────────────────────────

    #[test]
    fn test_parse_listing_extracts_basenames_and_sizes() {
        let body = r#"[
            {"type":"file","path":"onnx/duration.onnx","size":1234},
            {"type":"file","path":"onnx/tokens.txt","size":56},
            {"type":"directory","path":"onnx/nested"}
        ]"#;
        let files = parse_listing(body).unwrap();
        assert_eq!(files.len(), 2, "directories are skipped");
        assert_eq!(files[0].name, DURATION_FILE, "subdir prefix is stripped");
        assert_eq!(files[0].size, 1234);
    }

    #[test]
    fn test_parse_listing_rejects_non_array() {
        assert!(parse_listing(r#"{"error":"not found"}"#).is_err());
    }

    #[test]
    fn test_parse_listing_tolerates_missing_size() {
        let files = parse_listing(r#"[{"type":"file","path":"tokens.txt"}]"#).unwrap();
        assert_eq!(files[0].size, 0);
    }

    // ── Auxiliary file selection ─────────────────────────────────────────────

    #[test]
    fn test_auxiliary_selects_small_text_files() {
        for name in ["tokens.txt", "phonemes.json", "symbols.csv"] {
            let f = RepoFile { name: name.into(), size: 4096 };
            assert!(f.is_auxiliary(), "{name} should be fetched");
        }
    }

    #[test]
    fn test_auxiliary_includes_reference_scripts() {
        // This export ships no standalone table — the symbol list is inside
        // inference_onnx.py, so the scripts have to come down with the graphs.
        for name in ["inference_onnx.py", "export_onnx.py"] {
            let f = RepoFile { name: name.into(), size: 8192 };
            assert!(f.is_auxiliary(), "{name} should be fetched");
        }
    }

    #[test]
    fn test_auxiliary_skips_graphs_and_large_files() {
        assert!(!RepoFile { name: "decode.onnx".into(), size: 100 }.is_auxiliary());
        assert!(!RepoFile { name: "model.onnx_data".into(), size: 100 }.is_auxiliary());
        assert!(
            !RepoFile { name: "huge.json".into(), size: MAX_AUX_FILE_BYTES + 1 }.is_auxiliary(),
            "oversized files are not auxiliary"
        );
        assert!(!RepoFile { name: "README.md".into(), size: 100 }.is_auxiliary());
    }

    // ── Layout URLs ──────────────────────────────────────────────────────────

    #[test]
    fn test_layout_urls_with_subdir() {
        let l = Layout { repo: "owner/repo", subdir: "onnx" };
        assert_eq!(l.tree_url(), "https://huggingface.co/api/models/owner/repo/tree/main/onnx");
        assert_eq!(l.file_url("a.onnx"), "https://huggingface.co/owner/repo/resolve/main/onnx/a.onnx");
    }

    #[test]
    fn test_layout_urls_at_repo_root() {
        let l = Layout { repo: "owner/repo", subdir: "" };
        assert_eq!(l.tree_url(), "https://huggingface.co/api/models/owner/repo/tree/main");
        assert_eq!(l.file_url("a.onnx"), "https://huggingface.co/owner/repo/resolve/main/a.onnx");
    }

    #[test]
    fn test_known_good_layout_is_tried_first() {
        // Confirmed working against the published export.
        assert_eq!(CANDIDATE_LAYOUTS[0].repo, "owensong/Inflect-Micro-v2-ONNX");
        assert_eq!(CANDIDATE_LAYOUTS[0].subdir, "onnx");
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
    fn test_split_sentences_requires_whitespace_after_terminator() {
        // "3.5" must not split — the period is not followed by whitespace.
        assert_eq!(split_sentences("pi is 3.14 exactly"), vec!["pi is 3.14 exactly"]);
    }

    // ── Boundary pauses and fade ─────────────────────────────────────────────

    #[test]
    fn test_boundary_pause_varies_by_terminator() {
        assert!(boundary_pause_seconds("what?") > boundary_pause_seconds("stop."));
        assert!(boundary_pause_seconds("stop.") > boundary_pause_seconds("and,"));
    }

    #[test]
    fn test_boundary_pause_defaults_for_unpunctuated_text() {
        assert_eq!(boundary_pause_seconds("no terminator"), 0.08);
        assert_eq!(boundary_pause_seconds(""), 0.08);
    }

    #[test]
    fn test_edge_fade_tapers_both_ends() {
        let mut wave = vec![1.0f32; 4800];
        edge_fade(&mut wave, SAMPLE_RATE, EDGE_FADE_MS);
        assert_eq!(wave[0], 0.0, "starts silent");
        assert_eq!(*wave.last().unwrap(), 0.0, "ends silent");
        assert_eq!(wave[2400], 1.0, "middle is untouched");
    }

    #[test]
    fn test_edge_fade_handles_short_buffers() {
        let mut wave = vec![1.0f32; 3];
        edge_fade(&mut wave, SAMPLE_RATE, EDGE_FADE_MS);
        assert!(wave.iter().all(|s| s.is_finite()));
        let mut empty: Vec<f32> = Vec::new();
        edge_fade(&mut empty, SAMPLE_RATE, EDGE_FADE_MS);
    }

    #[test]
    fn test_split_sentences_empty_input() {
        assert!(split_sentences("").is_empty());
        assert!(split_sentences("   \n  ").is_empty());
    }

    // ── Chunking ─────────────────────────────────────────────────────────────

    #[test]
    fn test_chunk_text_gives_each_sentence_its_own_chunk() {
        // Sentences are not packed together — one chunk each is what makes the
        // per-chunk boundary pauses land in the right places.
        let chunks = chunk_text("One. Two. Three.");
        assert_eq!(chunks, vec!["One.", "Two.", "Three."]);
    }

    #[test]
    fn test_chunk_text_normalises_whitespace() {
        let chunks = chunk_text("  One.\n\n  Two.  ");
        assert_eq!(chunks, vec!["One.", "Two."]);
    }

    #[test]
    fn test_chunk_text_splits_sentence_over_the_limit() {
        let long = "word ".repeat(100);
        let chunks = chunk_text(&format!("{long}end."));
        assert!(chunks.len() > 1, "a 500-char sentence must be split");
        for c in &chunks {
            assert!(
                c.chars().count() <= CHUNK_LIMIT_CHARS,
                "chunk of {} chars exceeds the limit",
                c.chars().count()
            );
        }
    }

    #[test]
    fn test_chunk_text_splits_on_inner_punctuation_when_available() {
        // A comma past the halfway point is preferred over a hard cut.
        let head = "a".repeat(200);
        let tail = "b".repeat(200);
        let chunks = chunk_text(&format!("{head}, {tail}"));
        assert!(chunks[0].ends_with(','), "should break at the comma: {:?}", chunks[0]);
    }

    #[test]
    fn test_chunk_text_breaks_on_semicolon_and_colon() {
        assert_eq!(chunk_text("one; two: three"), vec!["one;", "two:", "three"]);
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
