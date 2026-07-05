//! Moonshine speech-to-text backend (ONNX Runtime).
//!
//! Moonshine (<https://github.com/moonshine-ai/moonshine>) is a compact
//! encoder–decoder ASR model designed for on-device use. Unlike Whisper it
//! consumes the raw 16 kHz waveform directly (no 30-second padding), which makes
//! it fast and low-latency on short utterances — a good fit for push-to-talk
//! dictation.
//!
//! The upstream ONNX release splits the model into four graphs that we run in
//! sequence:
//!
//! 1. `preprocess`      — raw audio `[1, samples]` → features `[1, frames, dim]`
//! 2. `encode`          — features → encoder context `[1, frames, dim]`
//! 3. `uncached_decode` — first decoder step: start token + context → logits + KV cache
//! 4. `cached_decode`   — subsequent steps: one token + context + KV cache → logits + new cache
//!
//! Decoding is greedy autoregression: start from the start-of-transcript token,
//! take the arg-max of each step's logits, feed it back until the
//! end-of-transcript token appears (or a length cap derived from the audio
//! duration is hit). Token ids are turned back into text with the model's
//! `tokenizer.json`.
//!
//! To stay robust against small differences between exported model revisions,
//! tensors are bound to graph inputs **by position** using each session's
//! declared input list rather than by hard-coded names, and the presence of an
//! optional `seq_len` input is detected from the graph arity instead of assumed.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use anyhow::{anyhow, bail, Context, Result};
use ort::{
    session::{builder::GraphOptimizationLevel, Session, SessionInputValue, SessionOutputs},
    value::Tensor,
};
use tokenizers::Tokenizer;
use tracing::info;
use voxctrl_config::MoonshineConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

// ── Model constants ───────────────────────────────────────────────────────────

/// Start-of-transcript token id (first token fed to the decoder).
const SOT_TOKEN: i32 = 1;
/// End-of-transcript token id (decoding stops once this is produced).
const EOT_TOKEN: i32 = 2;
/// Moonshine operates on 16 kHz mono audio.
const SAMPLE_RATE: usize = 16_000;
/// Upper bound on decoded tokens, scaled by audio length. Six tokens per second
/// comfortably exceeds natural speech rates while capping runaway generation if
/// the model never emits the end token.
const MAX_TOKENS_PER_SECOND: usize = 6;
/// Never fewer than this many tokens, so very short clips still get a few steps.
const MIN_MAX_TOKENS: usize = 8;

/// The ONNX graph files that make up a Moonshine model, in load order.
const MODEL_FILES: [&str; 4] = [
    "preprocess.onnx",
    "encode.onnx",
    "uncached_decode.onnx",
    "cached_decode.onnx",
];
const TOKENIZER_FILE: &str = "tokenizer.json";

/// Base URL for the upstream ONNX weights on the Hugging Face hub. The float
/// (non-quantized) graphs live under `onnx/merged/{size}/float/`. Kept as a
/// single constant so a hosting change is a one-line edit; users can also point
/// `model_dir` at a local copy to bypass downloading entirely.
const HF_BASE_URL: &str = "https://huggingface.co/UsefulSensors/moonshine/resolve/main/onnx/merged";

fn valid_model_size(size: &str) -> bool {
    matches!(size, "tiny" | "base")
}

// ── Filesystem layout ─────────────────────────────────────────────────────────

/// Default parent directory for all Moonshine models. Mirrors the whisper
/// backend's `~/.local/share/voxctrl/models` convention with a `moonshine`
/// subfolder so the two backends never collide.
pub fn default_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("moonshine")
}

fn expand_tilde(path: &str) -> PathBuf {
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

/// Directory holding one model's files: `<model_dir>/<size>/`.
fn model_size_dir(model_dir: &str, size: &str) -> PathBuf {
    let base = if model_dir.is_empty() {
        default_model_dir()
    } else {
        expand_tilde(model_dir)
    };
    base.join(size)
}

/// True when every ONNX graph and the tokenizer for `size` are present on disk.
pub fn is_model_downloaded(size: &str, model_dir: &str) -> bool {
    if !valid_model_size(size) {
        return false;
    }
    let dir = model_size_dir(model_dir, size);
    MODEL_FILES.iter().all(|f| dir.join(f).exists()) && dir.join(TOKENIZER_FILE).exists()
}

// ── Download ──────────────────────────────────────────────────────────────────

/// Serializes downloads so two triggers (e.g. a Settings click and an on-demand
/// load) can't fight over the same files.
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Fetch every model file for `size` into `<model_dir>/<size>/`. Files already
/// present are skipped, so this is safe to call repeatedly.
pub async fn download_model(size: &str, model_dir: &str) -> Result<()> {
    if !valid_model_size(size) {
        bail!("Unknown Moonshine model size '{size}' (expected 'tiny' or 'base')");
    }

    let dir = model_size_dir(model_dir, size);
    tokio::fs::create_dir_all(&dir).await?;

    let _guard = DOWNLOAD_LOCK.lock().await;

    // ONNX graphs live under `<size>/float/<file>`; the tokenizer is shared at
    // `<size>/tokenizer.json`.
    let mut jobs: Vec<(String, PathBuf)> = MODEL_FILES
        .iter()
        .map(|f| {
            (
                format!("{HF_BASE_URL}/{size}/float/{f}"),
                dir.join(f),
            )
        })
        .collect();
    jobs.push((
        format!("{HF_BASE_URL}/{size}/{TOKENIZER_FILE}"),
        dir.join(TOKENIZER_FILE),
    ));

    for (url, path) in jobs {
        if path.exists() {
            continue;
        }
        info!("Downloading Moonshine file: {url}");
        let response = reqwest::get(&url)
            .await
            .with_context(|| format!("request {url}"))?
            .error_for_status()
            .with_context(|| format!("fetch {url}"))?;
        let bytes = response.bytes().await.with_context(|| format!("read {url}"))?;
        // Write to a temp path then rename so an interrupted download never
        // leaves a half-written file that later looks "present".
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, &bytes)
            .await
            .with_context(|| format!("write {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .with_context(|| format!("finalize {}", path.display()))?;
    }

    info!("Moonshine '{size}' model ready in {}", dir.display());
    Ok(())
}

// ── Loaded state ──────────────────────────────────────────────────────────────

/// One Moonshine session (a graph) plus the number of KV-cache tensors it
/// produces, discovered from the graph, so we bind cache in/out without relying
/// on tensor names.
struct Loaded {
    preprocess: Session,
    encode: Session,
    uncached_decode: Session,
    cached_decode: Session,
    tokenizer: Tokenizer,
    /// Cache tensors emitted by `uncached_decode` (outputs after the logits).
    num_cache: usize,
    /// Non-cache inputs the decoders take: 2 = `[tokens, context]`,
    /// 3 = `[tokens, context, seq_len]`.
    decode_fixed_inputs: usize,
    /// Whether `encode` takes a trailing `seq_len` input in addition to features.
    encode_takes_seq_len: bool,
}

// ── Backend ───────────────────────────────────────────────────────────────────

pub struct MoonshineBackend {
    cfg: MoonshineConfig,
    /// Serialized behind a mutex: `Session::run` needs `&mut`, and the inference
    /// worker is single-threaded so there is never real contention.
    state: Mutex<Option<Loaded>>,
    loaded: bool,
}

impl MoonshineBackend {
    pub fn new(cfg: MoonshineConfig) -> Self {
        Self {
            cfg,
            state: Mutex::new(None),
            loaded: false,
        }
    }

    fn build_session(path: &Path) -> Result<Session> {
        // The builder methods return `ort::Error<SessionBuilder>` (the error
        // carries the builder back for recovery), which is not a `Send + Sync`
        // `std::error::Error`, so it can't flow through `anyhow::Context`. Format
        // it via Display instead. `commit_from_file` returns a plain `ort::Error`
        // that anyhow can absorb directly.
        Session::builder()
            .map_err(|e| anyhow!("ort session builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("set optimization level: {e}"))?
            .with_intra_threads(threads())
            .map_err(|e| anyhow!("set intra threads: {e}"))?
            .commit_from_file(path)
            .with_context(|| format!("load ONNX graph {}", path.display()))
    }
}

impl TranscriptionBackend for MoonshineBackend {
    fn name(&self) -> &str {
        "moonshine"
    }

    fn load(&mut self) -> Result<()> {
        let size = &self.cfg.model_size;
        if !valid_model_size(size) {
            bail!("Unknown Moonshine model size '{size}' (expected 'tiny' or 'base')");
        }

        let dir = model_size_dir("", size);
        // model_dir override: MoonshineConfig has no dir field, so honor an
        // explicit path only through the default location. If files are missing,
        // point the user at exactly where to put them.
        if !is_model_downloaded(size, "") {
            bail!(
                "Moonshine '{size}' model is not downloaded (expected {} plus {} in {}). \
                 Open Settings → Engine and download it, or place the files there manually.",
                MODEL_FILES.join(", "),
                TOKENIZER_FILE,
                dir.display()
            );
        }

        info!("Loading Moonshine '{size}' model from {}", dir.display());

        let preprocess = Self::build_session(&dir.join("preprocess.onnx"))?;
        let encode = Self::build_session(&dir.join("encode.onnx"))?;
        let uncached_decode = Self::build_session(&dir.join("uncached_decode.onnx"))?;
        let cached_decode = Self::build_session(&dir.join("cached_decode.onnx"))?;

        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER_FILE))
            .map_err(|e| anyhow!("load tokenizer.json: {e}"))?;

        // Discover the KV-cache shape of this export.
        //   uncached_decode outputs = [logits, cache_0, cache_1, ...]
        //   cached_decode  inputs   = [tokens, context, (seq_len), cache_0, ...]
        let num_cache = uncached_decode
            .outputs()
            .len()
            .checked_sub(1)
            .ok_or_else(|| anyhow!("uncached_decode has no outputs"))?;
        let cached_inputs = cached_decode.inputs().len();
        let decode_fixed_inputs = cached_inputs
            .checked_sub(num_cache)
            .ok_or_else(|| anyhow!("cached_decode has fewer inputs ({cached_inputs}) than cache tensors ({num_cache})"))?;
        if !(2..=3).contains(&decode_fixed_inputs) {
            bail!(
                "Unexpected Moonshine decoder signature: {decode_fixed_inputs} non-cache inputs \
                 (expected 2 or 3). The model export may be incompatible."
            );
        }
        let encode_takes_seq_len = encode.inputs().len() >= 2;

        info!(
            "Moonshine graph shape: {num_cache} cache tensors, {decode_fixed_inputs} fixed decoder inputs, \
             encode_seq_len={encode_takes_seq_len}"
        );

        *self.state.lock().unwrap() = Some(Loaded {
            preprocess,
            encode,
            uncached_decode,
            cached_decode,
            tokenizer,
            num_cache,
            decode_fixed_inputs,
            encode_takes_seq_len,
        });
        self.loaded = true;
        Ok(())
    }

    fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
        if !self.loaded {
            bail!("Model not loaded");
        }
        let mut guard = self.state.lock().unwrap();
        let state = guard.as_mut().context("Moonshine state not initialised")?;

        let n_samples = req.audio.len();
        if n_samples == 0 {
            return Ok(empty_result(&self.cfg.language));
        }

        let t0 = Instant::now();
        let text = run_inference(state, &req.audio)?;
        let inference_ms = t0.elapsed().as_millis() as u32;

        Ok(TranscriptionResult {
            text: text.trim().to_string(),
            language: self.cfg.language.clone(),
            language_probability: 1.0,
            duration_ms: (n_samples / (SAMPLE_RATE / 1000)) as u32,
            inference_ms,
            word_timestamps: None,
        })
    }

    fn unload(&mut self) {
        *self.state.lock().unwrap() = None;
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }
}

// ── Inference pipeline ────────────────────────────────────────────────────────

fn run_inference(state: &mut Loaded, audio: &[f32]) -> Result<String> {
    let n_samples = audio.len();

    // 1. Preprocess: raw waveform [1, samples] → features [1, frames, dim].
    let audio_tensor = Tensor::from_array(([1_usize, n_samples], audio.to_vec()))
        .context("build audio tensor")?;
    let (feat_shape, feat_data) = {
        let outputs = run_positional(&mut state.preprocess, vec![audio_tensor.into()])?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("extract preprocess output")?;
        (shape.to_vec(), data.to_vec())
    };

    // Number of encoder frames = second-to-last dimension of the features.
    let frames = *feat_shape
        .iter()
        .rev()
        .nth(1)
        .ok_or_else(|| anyhow!("preprocess output has too few dimensions"))? as i32;

    // 2. Encode: features → context [1, frames, dim].
    let (ctx_shape, ctx_data) = {
        let feat_tensor = Tensor::from_array((dims(&feat_shape), feat_data))
            .context("build features tensor")?;
        let mut inputs: Vec<SessionInputValue> = vec![feat_tensor.into()];
        if state.encode_takes_seq_len {
            let seq = Tensor::from_array(([1_usize], vec![frames])).context("build encode seq_len")?;
            inputs.push(seq.into());
        }
        let outputs = run_positional(&mut state.encode, inputs)?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("extract encode output")?;
        (shape.to_vec(), data.to_vec())
    };

    // Context is fed unchanged into every decoder step; build it once and pass by
    // reference so it is not re-copied each iteration.
    let context = Tensor::from_array((dims(&ctx_shape), ctx_data)).context("build context tensor")?;

    // 3. First decoder step (no cache yet): start token → logits + KV cache.
    let max_tokens = ((n_samples / SAMPLE_RATE) * MAX_TOKENS_PER_SECOND).max(MIN_MAX_TOKENS);
    let mut tokens: Vec<i32> = Vec::with_capacity(max_tokens + 1);

    let mut token_count: i32 = 1;
    let first_ids = Tensor::from_array(([1_usize, 1], vec![SOT_TOKEN])).context("build start token")?;
    let (mut logits, mut cache) = {
        let inputs = decoder_fixed_inputs(state, first_ids, &context, token_count)?;
        let outputs = run_positional(&mut state.uncached_decode, inputs)?;
        extract_logits_and_cache(&outputs, state.num_cache)?
    };

    // 4. Greedy autoregressive loop over cached_decode.
    for _ in 0..max_tokens {
        let next = argmax_last_row(&logits.0, &logits.1);
        if next == EOT_TOKEN {
            break;
        }
        tokens.push(next);
        token_count += 1;

        let ids = Tensor::from_array(([1_usize, 1], vec![next])).context("build next token")?;
        let mut inputs = decoder_fixed_inputs(state, ids, &context, token_count)?;
        for (shape, data) in cache.drain(..) {
            let t = Tensor::from_array((dims(&shape), data)).context("build cache tensor")?;
            inputs.push(t.into());
        }
        let outputs = run_positional(&mut state.cached_decode, inputs)?;
        let (new_logits, new_cache) = extract_logits_and_cache(&outputs, state.num_cache)?;
        logits = new_logits;
        cache = new_cache;
    }

    // 5. Decode token ids to text, dropping the special tokens.
    let ids_u32: Vec<u32> = tokens.iter().map(|&t| t as u32).collect();
    let text = state
        .tokenizer
        .decode(&ids_u32, true)
        .map_err(|e| anyhow!("tokenizer decode: {e}"))?;
    Ok(text)
}

/// Build the fixed (non-cache) decoder inputs: `[tokens, context]` and, when the
/// export declares it, a trailing `seq_len` scalar holding the running token
/// count.
fn decoder_fixed_inputs<'a>(
    state: &Loaded,
    ids: Tensor<i32>,
    context: &'a Tensor<f32>,
    token_count: i32,
) -> Result<Vec<SessionInputValue<'a>>> {
    let mut inputs: Vec<SessionInputValue> = Vec::with_capacity(state.decode_fixed_inputs + state.num_cache);
    inputs.push(ids.into());
    inputs.push(context.into());
    if state.decode_fixed_inputs == 3 {
        let seq = Tensor::from_array(([1_usize], vec![token_count])).context("build decode seq_len")?;
        inputs.push(seq.into());
    }
    Ok(inputs)
}

/// Run a session binding `values` to its inputs positionally (input `i` gets
/// `values[i]`), which avoids depending on the export's tensor names.
fn run_positional<'s>(
    session: &'s mut Session,
    values: Vec<SessionInputValue<'_>>,
) -> Result<SessionOutputs<'s>> {
    let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
    if names.len() != values.len() {
        bail!(
            "graph expects {} inputs but {} were supplied",
            names.len(),
            values.len()
        );
    }
    let feed: Vec<(String, SessionInputValue)> = names.into_iter().zip(values).collect();
    session.run(feed).context("ort session run")
}

/// Split a decoder step's outputs into `(logits shape, logits data)` and the
/// owned KV-cache tensors, copying out of the borrowed session outputs so the
/// session can be run again.
#[allow(clippy::type_complexity)]
fn extract_logits_and_cache(
    outputs: &SessionOutputs,
    num_cache: usize,
) -> Result<((Vec<i64>, Vec<f32>), Vec<(Vec<i64>, Vec<f32>)>)> {
    let (lshape, ldata) = outputs[0]
        .try_extract_tensor::<f32>()
        .context("extract decoder logits")?;
    let logits = (lshape.to_vec(), ldata.to_vec());

    let mut cache = Vec::with_capacity(num_cache);
    for i in 0..num_cache {
        let (shape, data) = outputs[i + 1]
            .try_extract_tensor::<f32>()
            .with_context(|| format!("extract KV cache tensor {i}"))?;
        cache.push((shape.to_vec(), data.to_vec()));
    }
    Ok((logits, cache))
}

/// Arg-max over the final timestep of a `[1, seq, vocab]` (or `[1, vocab]`)
/// logits tensor, returning the winning token id.
fn argmax_last_row(shape: &[i64], data: &[f32]) -> i32 {
    let vocab = *shape.last().unwrap_or(&1) as usize;
    if vocab == 0 || data.is_empty() {
        return EOT_TOKEN;
    }
    // The last `vocab` values are the distribution for the most recent timestep.
    let start = data.len().saturating_sub(vocab);
    let row = &data[start..];
    let mut best = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (i, &v) in row.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best = i;
        }
    }
    best as i32
}

/// Convert an `i64` ONNX shape into the `usize` dims `Tensor::from_array` wants.
fn dims(shape: &[i64]) -> Vec<usize> {
    shape.iter().map(|&d| d.max(0) as usize).collect()
}

fn empty_result(language: &str) -> TranscriptionResult {
    TranscriptionResult {
        text: String::new(),
        language: language.to_string(),
        language_probability: 1.0,
        duration_ms: 0,
        inference_ms: 0,
        word_timestamps: None,
    }
}

fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(1))
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_model_size() {
        assert!(valid_model_size("tiny"));
        assert!(valid_model_size("base"));
        assert!(!valid_model_size("small"));
        assert!(!valid_model_size(""));
    }

    #[test]
    fn test_default_model_dir_has_moonshine_segment() {
        let dir = default_model_dir();
        assert!(dir.ends_with("moonshine"));
    }

    #[test]
    fn test_model_size_dir_layout() {
        let dir = model_size_dir("/tmp/models", "base");
        assert_eq!(dir, PathBuf::from("/tmp/models/base"));
    }

    #[test]
    fn test_is_model_downloaded_unknown_size() {
        assert!(!is_model_downloaded("nonexistent", ""));
        assert!(!is_model_downloaded("nonexistent", "/tmp"));
    }

    #[test]
    fn test_is_model_downloaded_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        // Empty directory: nothing downloaded.
        assert!(!is_model_downloaded("base", dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_is_model_downloaded_complete_set() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let size_dir = root.path().join("base");
        std::fs::create_dir_all(&size_dir).unwrap();
        for f in MODEL_FILES.iter().chain([&TOKENIZER_FILE]) {
            std::fs::File::create(size_dir.join(f))
                .unwrap()
                .write_all(b"x")
                .unwrap();
        }
        assert!(is_model_downloaded("base", root.path().to_str().unwrap()));

        // Remove one graph → no longer considered downloaded.
        std::fs::remove_file(size_dir.join("encode.onnx")).unwrap();
        assert!(!is_model_downloaded("base", root.path().to_str().unwrap()));
    }

    #[test]
    fn test_argmax_last_row_multistep() {
        // Two timesteps, vocab of 4. Arg-max must come from the LAST row.
        let shape = [1_i64, 2, 4];
        let data = [
            0.1, 0.9, 0.2, 0.3, // step 0 (ignored)
            0.5, 0.4, 0.8, 0.1, // step 1 → index 2 wins
        ];
        assert_eq!(argmax_last_row(&shape, &data), 2);
    }

    #[test]
    fn test_argmax_last_row_single_step() {
        let shape = [1_i64, 5];
        let data = [0.0, 0.0, 0.0, 7.0, 1.0];
        assert_eq!(argmax_last_row(&shape, &data), 3);
    }

    #[test]
    fn test_argmax_last_row_empty_is_eot() {
        assert_eq!(argmax_last_row(&[1, 0], &[]), EOT_TOKEN);
    }

    #[test]
    fn test_dims_conversion() {
        assert_eq!(dims(&[1, 40, 288]), vec![1_usize, 40, 288]);
    }

    #[test]
    fn test_new_backend_reports_name_and_unloaded() {
        let cfg = MoonshineConfig {
            model_size: "base".into(),
            language: "en".into(),
        };
        let b = MoonshineBackend::new(cfg);
        assert_eq!(b.name(), "moonshine");
        assert!(!b.is_loaded());
    }

    #[test]
    fn test_transcribe_before_load_errors() {
        let cfg = MoonshineConfig {
            model_size: "base".into(),
            language: "en".into(),
        };
        let b = MoonshineBackend::new(cfg);
        let req = TranscribeRequest {
            audio: vec![0.0; 1600],
            language: None,
            word_timestamps: false,
            initial_prompt: None,
        };
        assert!(b.transcribe(&req).is_err());
    }
}
