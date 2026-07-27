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
             parse as a phoneme table. The vocabulary maps eSpeak IPA to the model's \
             phoneme ids and synthesis cannot be correct without it.\n\
             Files published there: {}\n\
             If the table is one of these, place it in {} and report the name so it \
             can be recognised automatically.",
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
    fn is_auxiliary(&self) -> bool {
        if self.size > MAX_AUX_FILE_BYTES {
            return false;
        }
        let lower = self.name.to_ascii_lowercase();
        if lower.ends_with(".onnx") || lower.ends_with(".onnx_data") {
            return false;
        }
        [".json", ".txt", ".csv", ".tsv"].iter().any(|e| lower.ends_with(e))
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
