//! Moonshine speech-to-text backend (ONNX Runtime).
//!
//! Moonshine (<https://github.com/moonshine-ai/moonshine>) is a compact
//! encoder–decoder ASR model designed for on-device use. Unlike Whisper it
//! consumes the raw 16 kHz waveform directly (no 30-second padding), which makes
//! it fast and low-latency on the short utterances typical of push-to-talk
//! dictation.
//!
//! This mirrors the upstream `useful-moonshine-onnx` pipeline, which uses two
//! ONNX graphs:
//!
//! 1. `encoder_model`         — raw audio `[1, samples]` → hidden states `[1, seq, dim]`
//! 2. `decoder_model_merged`  — a KV-cached decoder with a `use_cache_branch`
//!    toggle: given the running token, the encoder hidden states, and the
//!    accumulated `past_key_values`, it emits the next-token logits plus updated
//!    cache tensors.
//!
//! Decoding is greedy autoregression from the start-of-transcript token (id 1)
//! until the end-of-transcript token (id 2), capped at [`MAX_TOKENS`]. On the
//! first step `use_cache_branch` is false and empty caches are supplied; every
//! later step reuses the decoder self-attention cache while the encoder
//! cross-attention cache stays frozen (it depends only on the audio). Token ids
//! are turned back into text with the bundled `tokenizer.json`.
//!
//! Model sizes have fixed layer/head geometry (`tiny`: 6 layers, `base`: 8),
//! which determines the number and shape of the cache tensors.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use anyhow::{anyhow, bail, Context, Result};
use ort::{
    memory::Allocator,
    session::{
        builder::{GraphOptimizationLevel, SessionBuilder},
        Session, SessionInputValue,
    },
    value::{Shape, Tensor},
};
use tokenizers::Tokenizer;
use tracing::info;
use voxctrl_config::MoonshineConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

// ── Model constants ───────────────────────────────────────────────────────────

/// Start-of-transcript token id (first token fed to the decoder).
const SOT_TOKEN: i64 = 1;
/// End-of-transcript token id (decoding stops once this is produced).
const EOT_TOKEN: i64 = 2;
/// Moonshine operates on 16 kHz mono audio.
const SAMPLE_RATE: usize = 16_000;
/// Upper bound on decoded tokens per call. Matches upstream; end-of-transcript
/// normally stops decoding well before this.
const MAX_TOKENS: usize = 192;

/// The two ONNX graph files that make up a Moonshine model.
const ENCODER_FILE: &str = "encoder_model.onnx";
const DECODER_FILE: &str = "decoder_model_merged.onnx";
const MODEL_FILES: [&str; 2] = [ENCODER_FILE, DECODER_FILE];

/// The tokenizer is identical across model sizes and is shipped with the upstream
/// package rather than hosted per-model, so it is embedded in the binary.
const TOKENIZER_JSON: &[u8] = include_bytes!("../assets/moonshine_tokenizer.json");

/// Base URL for the upstream ONNX weights on the Hugging Face hub. The float
/// (non-quantized) graphs live under `onnx/merged/<size>/float/`. Kept as a
/// single constant so a hosting change is a one-line edit; users can also point
/// at a local copy to bypass downloading entirely.
const HF_BASE_URL: &str = "https://huggingface.co/UsefulSensors/moonshine/resolve/main/onnx/merged";

/// Per-size decoder geometry: `(num_layers, num_key_value_heads, head_dim)`. These
/// determine the shape of the KV-cache tensors. Values from the upstream model.
fn model_dims(size: &str) -> Option<(usize, usize, usize)> {
    match size {
        "tiny" => Some((6, 8, 36)),
        "base" => Some((8, 8, 52)),
        _ => None,
    }
}

fn valid_model_size(size: &str) -> bool {
    model_dims(size).is_some()
}

// ── Filesystem layout ─────────────────────────────────────────────────────────

/// Default parent directory for all Moonshine models: the shared models base
/// with a `moonshine` subfolder so the two backends never collide.
pub fn default_model_dir() -> PathBuf {
    crate::util::models_base_dir().join("moonshine")
}

/// Directory holding one model's files: `<model_dir>/<size>/`.
fn model_size_dir(model_dir: &str, size: &str) -> PathBuf {
    let base = if model_dir.is_empty() {
        default_model_dir()
    } else {
        crate::util::expand_tilde(model_dir)
    };
    base.join(size)
}

/// True when both ONNX graphs for `size` are present on disk. The tokenizer is
/// embedded, so it is not part of this check.
pub fn is_model_downloaded(size: &str, model_dir: &str) -> bool {
    if !valid_model_size(size) {
        return false;
    }
    let dir = model_size_dir(model_dir, size);
    MODEL_FILES.iter().all(|f| dir.join(f).exists())
}

// ── Download ──────────────────────────────────────────────────────────────────

/// Serializes downloads so two triggers (e.g. a Settings click and an on-demand
/// load) can't fight over the same files.
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Fetch both ONNX graphs for `size` into `<model_dir>/<size>/`. Files already
/// present are skipped, so this is safe to call repeatedly.
pub async fn download_model(size: &str, model_dir: &str) -> Result<()> {
    if !valid_model_size(size) {
        bail!("Unknown Moonshine model size '{size}' (expected 'tiny' or 'base')");
    }

    let dir = model_size_dir(model_dir, size);
    tokio::fs::create_dir_all(&dir).await?;

    let _guard = DOWNLOAD_LOCK.lock().await;

    for file in MODEL_FILES {
        let path = dir.join(file);
        if path.exists() {
            continue;
        }
        let url = format!("{HF_BASE_URL}/{size}/float/{file}");
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

/// What feeds one decoder input, resolved once at load time so the hot decode
/// loop binds inputs by position (no per-step name matching or map lookups).
/// `Cache(i)` refers to the `i`-th KV-cache tensor in emit order.
enum DecoderInput {
    InputIds,
    Hidden,
    UseCache,
    EncoderMask,
    Cache(usize),
}

struct Loaded {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
    /// One entry per decoder input, in the graph's declared order, so values can
    /// be bound positionally.
    decoder_plan: Vec<DecoderInput>,
    /// Number of KV-cache tensors (`num_layers * 4`).
    num_caches: usize,
    /// For each cache index, whether it is a decoder self-attention cache (which
    /// grows every step) versus an encoder cross-attention cache (fixed after the
    /// first step).
    cache_is_decoder: Vec<bool>,
    /// Shape of an empty (pre-first-step) cache tensor: `[0, kv_heads, 1, head_dim]`.
    empty_cache_shape: [i64; 4],
    /// Whether the encoder graph takes an `attention_mask` input.
    encoder_has_attention_mask: bool,
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
        let builder = Session::builder()
            .map_err(|e| anyhow!("ort session builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("set optimization level: {e}"))?
            .with_intra_threads(threads())
            .map_err(|e| anyhow!("set intra threads: {e}"))?;

        let mut builder = Self::with_gpu(builder);

        builder
            .commit_from_file(path)
            .with_context(|| format!("load ONNX graph {}", path.display()))
    }

    /// Register this build's GPU execution provider, if it has one.
    ///
    /// Registration is asked to `error_on_failure` rather than ORT's default of
    /// logging and silently continuing on the CPU. Silently is precisely how
    /// this backend spent its whole life running on the CPU while the Engine tab
    /// showed a Device setting: the fallback is fine, being told about it is the
    /// point. On failure the builder is recovered from the error and returned
    /// unchanged, so a machine with no usable CUDA still loads the model.
    fn with_gpu(builder: SessionBuilder) -> SessionBuilder {
        #[cfg(any(feature = "moonshine-cuda", feature = "moonshine-coreml"))]
        {
            #[cfg(feature = "moonshine-cuda")]
            let (name, ep) = ("CUDA", ort::ep::CUDA::default().build().error_on_failure());
            #[cfg(all(feature = "moonshine-coreml", not(feature = "moonshine-cuda")))]
            let (name, ep) = ("CoreML", ort::ep::CoreML::default().build().error_on_failure());

            match builder.with_execution_providers([ep]) {
                Ok(with_ep) => {
                    // Per graph, so twice per load; `load` already says this
                    // once at INFO. The failure below stays at WARN.
                    tracing::debug!("Moonshine: {name} execution provider registered");
                    return with_ep;
                }
                Err(e) => {
                    tracing::warn!(
                        "Moonshine: {name} execution provider unavailable ({e}); \
                         running on the CPU, which holds the fp32 weights in RAM"
                    );
                    // The error carries the builder back, so the load goes on.
                    return e.recover();
                }
            }
        }

        #[cfg(not(any(feature = "moonshine-cuda", feature = "moonshine-coreml")))]
        builder
    }
}

impl TranscriptionBackend for MoonshineBackend {
    fn name(&self) -> &str {
        "moonshine"
    }

    fn load(&mut self) -> Result<()> {
        let size = &self.cfg.model_size;
        let (num_layers, kv_heads, head_dim) = model_dims(size)
            .ok_or_else(|| anyhow!("Unknown Moonshine model size '{size}' (expected 'tiny' or 'base')"))?;

        let dir = model_size_dir("", size);
        if !is_model_downloaded(size, "") {
            bail!(
                "Moonshine '{size}' model is not downloaded (expected {} in {}). \
                 Open Settings → Engine and download it, or place the files there manually.",
                MODEL_FILES.join(" and "),
                dir.display()
            );
        }

        info!("Loading Moonshine '{size}' model from {}", dir.display());
        match crate::moonshine_gpu_backend() {
            Some(backend) => info!("Moonshine acceleration: {backend}"),
            // Said at INFO on every load because the alternative — saying
            // nothing — is what let a CPU-only backend look like a GPU one for
            // as long as the Engine tab showed a Device setting beside it.
            None => info!(
                "Moonshine acceleration: none (CPU); this build has no ONNX Runtime \
                 GPU provider, so the fp32 weights stay in RAM"
            ),
        }

        let encoder = Self::build_session(&dir.join(ENCODER_FILE))?;
        let decoder = Self::build_session(&dir.join(DECODER_FILE))?;

        let tokenizer = Tokenizer::from_bytes(TOKENIZER_JSON)
            .map_err(|e| anyhow!("load bundled Moonshine tokenizer: {e}"))?;

        // Cache tensor names, in the layer/decoder-encoder/key-value order the
        // decoder emits its `present.*` outputs. Used only to map named decoder
        // cache inputs to their emit-order index while building the plan.
        let mut cache_names = Vec::with_capacity(num_layers * 4);
        let mut cache_is_decoder = Vec::with_capacity(num_layers * 4);
        for i in 0..num_layers {
            for a in ["decoder", "encoder"] {
                for b in ["key", "value"] {
                    cache_names.push(format!("past_key_values.{i}.{a}.{b}"));
                    cache_is_decoder.push(a == "decoder");
                }
            }
        }
        let num_caches = cache_names.len();

        let encoder_has_attention_mask =
            encoder.inputs().iter().any(|i| i.name() == "attention_mask");

        // Resolve every decoder input to its source once. Binding positionally at
        // run time then needs no name lookups.
        let mut decoder_plan = Vec::with_capacity(decoder.inputs().len());
        for input in decoder.inputs() {
            let name = input.name();
            let slot = match name {
                "input_ids" => DecoderInput::InputIds,
                "encoder_hidden_states" => DecoderInput::Hidden,
                "use_cache_branch" => DecoderInput::UseCache,
                "encoder_attention_mask" => DecoderInput::EncoderMask,
                n if n.starts_with("past_key_values.") => {
                    let idx = cache_names
                        .iter()
                        .position(|c| c == n)
                        .ok_or_else(|| anyhow!("unrecognized decoder cache input '{n}'"))?;
                    DecoderInput::Cache(idx)
                }
                other => bail!("unexpected Moonshine decoder input '{other}'"),
            };
            decoder_plan.push(slot);
        }

        // Fail at load (not mid-decode) if the model doesn't expose the caches
        // this size expects.
        let declared_caches = decoder_plan
            .iter()
            .filter(|s| matches!(s, DecoderInput::Cache(_)))
            .count();
        if declared_caches != num_caches {
            bail!(
                "Moonshine '{size}' decoder declares {declared_caches} cache inputs, expected \
                 {num_caches} for this size — the model files may not match the selected size."
            );
        }

        info!("Moonshine geometry: {num_layers} layers, {kv_heads} kv-heads, head_dim {head_dim}");

        *self.state.lock().unwrap() = Some(Loaded {
            encoder,
            decoder,
            tokenizer,
            decoder_plan,
            num_caches,
            cache_is_decoder,
            empty_cache_shape: [0, kv_heads as i64, 1, head_dim as i64],
            encoder_has_attention_mask,
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

/// An owned cache tensor as `(shape, data)`. An empty (0-element) `data` denotes
/// the pre-first-step placeholder cache.
type CacheVal = (Vec<i64>, Vec<f32>);

fn run_inference(state: &mut Loaded, audio: &[f32]) -> Result<String> {
    let n_samples = audio.len();
    let needs_mask = state.encoder_has_attention_mask
        || state.decoder_plan.iter().any(|s| matches!(s, DecoderInput::EncoderMask));
    // `attention_mask` (when a graph takes it) is all-ones over the samples.
    let attention_mask: Vec<i64> = if needs_mask { vec![1; n_samples] } else { Vec::new() };

    // ── Encoder: raw waveform → hidden states ─────────────────────────────────
    let hidden: CacheVal = {
        let audio_tensor = Tensor::from_array(([1_usize, n_samples], audio.to_vec()))
            .context("build audio tensor")?;
        let mut feed: Vec<(&str, SessionInputValue)> = vec![("input_values", audio_tensor.into())];
        if state.encoder_has_attention_mask {
            let mask = Tensor::from_array(([1_usize, n_samples], attention_mask.clone()))
                .context("build encoder attention_mask")?;
            feed.push(("attention_mask", mask.into()));
        }
        let outputs = state.encoder.run(feed).context("encoder run")?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("extract encoder hidden states")?;
        (shape.to_vec(), data.to_vec())
    };

    // Hidden states and the encoder attention mask are constant across decoder
    // steps; build them once and feed by reference each iteration.
    let hidden_tensor =
        Tensor::from_array((dims(&hidden.0), hidden.1)).context("build hidden-states tensor")?;
    let enc_mask_tensor = if state.decoder_plan.iter().any(|s| matches!(s, DecoderInput::EncoderMask)) {
        Some(
            Tensor::from_array(([1_usize, n_samples], attention_mask))
                .context("build decoder encoder_attention_mask")?,
        )
    } else {
        None
    };

    // ── Decoder: greedy autoregressive loop ───────────────────────────────────
    let allocator = Allocator::default();

    // Caches in emit order; each starts empty (0-element, rank-4).
    let mut caches: Vec<CacheVal> =
        vec![(state.empty_cache_shape.to_vec(), Vec::new()); state.num_caches];

    let mut tokens: Vec<i64> = vec![SOT_TOKEN];
    let mut next_input: i64 = SOT_TOKEN;

    for step in 0..MAX_TOKENS {
        let use_cache = step > 0;

        // Per-step tensors. Cache tensors are rebuilt from the accumulated data;
        // input_ids/use_cache change each step. All are bound positionally below.
        let input_ids = Tensor::from_array(([1_usize, 1], vec![next_input])).context("build input_ids")?;
        let use_cache_t = Tensor::from_array(([1_usize], vec![use_cache])).context("build use_cache_branch")?;
        let mut cache_tensors: Vec<Option<Tensor<f32>>> = caches
            .iter()
            .map(|(shape, data)| build_cache_tensor(&allocator, shape, data).map(Some))
            .collect::<Result<_>>()?;

        // Bind inputs positionally, in the decoder's declared order.
        let mut input_ids = Some(input_ids);
        let mut use_cache_t = Some(use_cache_t);
        let mut values: Vec<SessionInputValue> = Vec::with_capacity(state.decoder_plan.len());
        for slot in &state.decoder_plan {
            let val: SessionInputValue = match slot {
                DecoderInput::InputIds => input_ids.take().unwrap().into(),
                DecoderInput::Hidden => (&hidden_tensor).into(),
                DecoderInput::UseCache => use_cache_t.take().unwrap().into(),
                DecoderInput::EncoderMask => enc_mask_tensor
                    .as_ref()
                    .expect("EncoderMask slot implies the mask was built")
                    .into(),
                DecoderInput::Cache(i) => cache_tensors[*i].take().unwrap().into(),
            };
            values.push(val);
        }

        let outputs = state.decoder.run(values.as_slice()).context("decoder run")?;
        if outputs.len() < 1 + state.num_caches {
            bail!(
                "Moonshine decoder returned {} outputs, expected at least {} (logits + {} caches)",
                outputs.len(),
                1 + state.num_caches,
                state.num_caches
            );
        }

        // logits: [1, 1, vocab] → arg-max of the final row.
        let (lshape, ldata) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("extract decoder logits")?;
        let next_token = argmax_last_row(lshape, ldata);

        tokens.push(next_token);
        if next_token == EOT_TOKEN {
            break;
        }

        // Update caches from the `present.*` outputs (which follow logits in emit
        // order). On the first (uncached) step every cache is refreshed; later
        // only the decoder self-attention caches grow — the encoder
        // cross-attention caches depend solely on the audio and stay fixed.
        for i in 0..state.num_caches {
            if !use_cache || state.cache_is_decoder[i] {
                let (shape, data) = outputs[i + 1]
                    .try_extract_tensor::<f32>()
                    .with_context(|| format!("extract present cache {i}"))?;
                caches[i] = (shape.to_vec(), data.to_vec());
            }
        }
        next_input = next_token;
    }

    // ── Detokenize (special tokens stripped) ─────────────────────────────────
    let ids_u32: Vec<u32> = tokens.iter().map(|&t| t as u32).collect();
    state
        .tokenizer
        .decode(&ids_u32, true)
        .map_err(|e| anyhow!("tokenizer decode: {e}"))
}

/// Build a cache tensor. Empty (`data.is_empty()`) caches use the allocator path
/// because `from_array` rejects the 0-sized dimension of the placeholder shape.
fn build_cache_tensor(allocator: &Allocator, shape: &[i64], data: &[f32]) -> Result<Tensor<f32>> {
    if data.is_empty() {
        Tensor::<f32>::new(allocator, Shape::new(shape.iter().copied()))
            .map_err(|e| anyhow!("allocate empty cache tensor: {e}"))
    } else {
        Tensor::from_array((dims(shape), data.to_vec())).context("build cache tensor")
    }
}

/// Arg-max over the final timestep of a `[1, seq, vocab]` (or `[1, vocab]`)
/// logits tensor, returning the winning token id.
fn argmax_last_row(shape: &[i64], data: &[f32]) -> i64 {
    let vocab = *shape.last().unwrap_or(&1) as usize;
    if vocab == 0 || data.is_empty() {
        return EOT_TOKEN;
    }
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
    best as i64
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
    fn test_model_dims() {
        assert_eq!(model_dims("tiny"), Some((6, 8, 36)));
        assert_eq!(model_dims("base"), Some((8, 8, 52)));
        assert_eq!(model_dims("nope"), None);
    }

    #[test]
    fn test_default_model_dir_has_moonshine_segment() {
        assert!(default_model_dir().ends_with("moonshine"));
    }

    #[test]
    fn test_model_size_dir_layout() {
        assert_eq!(model_size_dir("/tmp/models", "base"), PathBuf::from("/tmp/models/base"));
    }

    #[test]
    fn test_is_model_downloaded_unknown_size() {
        assert!(!is_model_downloaded("nonexistent", ""));
        assert!(!is_model_downloaded("nonexistent", "/tmp"));
    }

    #[test]
    fn test_is_model_downloaded_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_model_downloaded("base", dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_is_model_downloaded_complete_set() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let size_dir = root.path().join("base");
        std::fs::create_dir_all(&size_dir).unwrap();
        for f in MODEL_FILES {
            std::fs::File::create(size_dir.join(f)).unwrap().write_all(b"x").unwrap();
        }
        assert!(is_model_downloaded("base", root.path().to_str().unwrap()));

        std::fs::remove_file(size_dir.join(DECODER_FILE)).unwrap();
        assert!(!is_model_downloaded("base", root.path().to_str().unwrap()));
    }

    #[test]
    fn test_bundled_tokenizer_loads_and_strips_specials() {
        let tok = Tokenizer::from_bytes(TOKENIZER_JSON).expect("bundled tokenizer must parse");
        // Decoding just the special SOT/EOT ids with skip_special_tokens yields
        // empty text — the exact behavior the decode loop relies on.
        let out = tok.decode(&[SOT_TOKEN as u32, EOT_TOKEN as u32], true).unwrap();
        assert_eq!(out.trim(), "");
    }

    #[test]
    fn test_argmax_last_row_multistep() {
        let shape = [1_i64, 2, 4];
        let data = [
            0.1, 0.9, 0.2, 0.3, // step 0 (ignored)
            0.5, 0.4, 0.8, 0.1, // step 1 → index 2 wins
        ];
        assert_eq!(argmax_last_row(&shape, &data), 2);
    }

    #[test]
    fn test_argmax_last_row_single_step() {
        assert_eq!(argmax_last_row(&[1, 5], &[0.0, 0.0, 0.0, 7.0, 1.0]), 3);
    }

    #[test]
    fn test_argmax_last_row_empty_is_eot() {
        assert_eq!(argmax_last_row(&[1, 0], &[]), EOT_TOKEN);
    }

    #[test]
    fn test_dims_conversion() {
        assert_eq!(dims(&[1, 8, 0, 52]), vec![1_usize, 8, 0, 52]);
    }

    #[test]
    fn test_new_backend_reports_name_and_unloaded() {
        let cfg = MoonshineConfig { model_size: "base".into(), language: "en".into() };
        let b = MoonshineBackend::new(cfg);
        assert_eq!(b.name(), "moonshine");
        assert!(!b.is_loaded());
    }

    #[test]
    fn test_transcribe_before_load_errors() {
        let cfg = MoonshineConfig { model_size: "base".into(), language: "en".into() };
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
