//! The two ONNX graphs behind Inflect-Micro-v2, and the synthesis pipeline.
//!
//! The export splits the learned text-to-waveform path in two:
//!
//! 1. `duration.onnx` — phoneme ids → the aligned latent sequence, applying the
//!    stochastic duration predictor and monotonic alignment.
//! 2. `decode.onnx`   — that latent sequence → a 24 kHz mono waveform, via the
//!    residual coupling flow and the alias-reduced neural vocoder.
//!
//! # Binding to the graphs
//!
//! Inputs are resolved **by name at load time** against [`DURATION_ALIASES`] and
//! friends, exactly as `voxctrl-inference`'s Moonshine backend resolves its
//! decoder plan. Anything that cannot be resolved is a hard error at load,
//! reported together with the full discovered signature, so a naming mismatch
//! surfaces as an actionable message rather than as garbled audio.
//!
//! The alias tables cover conventional VITS export naming. They were written
//! without access to the published export (see the module docs in `mod.rs`), so
//! if Inflect names its tensors differently, [`InflectModel::load`] will say so
//! and list what it found — run the `inflect_micro_inspect` Tauri command to get
//! the same report without loading a voice.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use ort::{
    session::{builder::GraphOptimizationLevel, Session, SessionInputValue},
    value::Tensor,
};
use tracing::{info, warn};

use super::phonemes::{self, PhonemeVocab};
use super::{DECODE_FILE, DURATION_FILE, SAMPLE_RATE};

// ── Tensor naming ─────────────────────────────────────────────────────────────

/// Accepted names for the phoneme-id input of `duration.onnx`.
const PHONEME_IDS_ALIASES: [&str; 5] =
    ["input", "input_ids", "phoneme_ids", "text", "x"];
/// Accepted names for the phoneme-sequence-length input.
const PHONEME_LENGTHS_ALIASES: [&str; 4] =
    ["input_lengths", "phoneme_lengths", "x_lengths", "text_lengths"];
/// Accepted names for the packed `[noise_scale, length_scale, noise_scale_w]`
/// input used by conventional VITS exports.
const SCALES_ALIASES: [&str; 2] = ["scales", "scale"];
/// Accepted names when the scales are exposed as separate scalar inputs instead.
const NOISE_SCALE_ALIASES: [&str; 2] = ["noise_scale", "noise"];
const LENGTH_SCALE_ALIASES: [&str; 3] = ["length_scale", "speed", "duration_scale"];
const NOISE_SCALE_W_ALIASES: [&str; 2] = ["noise_scale_w", "noise_w"];
/// Accepted names for an explicit sampling-seed input, when the graph takes one.
const SEED_ALIASES: [&str; 2] = ["seed", "rng_seed"];

/// Accepted names for the latent sequence `decode.onnx` consumes. Matched against
/// `duration.onnx`'s outputs to connect the two stages.
const LATENT_ALIASES: [&str; 6] = ["z", "latent", "latents", "z_p", "input", "x"];
/// Accepted names for an optional latent-length input on `decode.onnx`.
const LATENT_LENGTHS_ALIASES: [&str; 3] = ["z_lengths", "y_lengths", "latent_lengths"];

/// What supplies one `duration.onnx` input, resolved once at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DurationInput {
    PhonemeIds,
    PhonemeLengths,
    /// Packed `[noise_scale, length_scale, noise_scale_w]`.
    Scales,
    NoiseScale,
    LengthScale,
    NoiseScaleW,
    Seed,
}

/// What supplies one `decode.onnx` input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeInput {
    /// The i-th output of `duration.onnx`.
    FromDuration(usize),
    LatentLengths,
    NoiseScale,
    Seed,
}

fn matches(name: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
}

// ── Discovered signature ──────────────────────────────────────────────────────

/// One graph's input or output names, as declared by the ONNX file.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphSignature {
    pub file: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

/// The full discovered signature of both graphs. Returned by
/// [`inspect`] and surfaced through the `inflect_micro_inspect` Tauri command so
/// the real tensor contract can be read off a downloaded model.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelSignature {
    pub duration: GraphSignature,
    pub decode: GraphSignature,
    /// Phoneme-vocabulary file found in the model dir, if any.
    pub vocab_file: Option<String>,
    pub vocab_size: Option<usize>,
}

fn signature_of(session: &Session, file: &str) -> GraphSignature {
    GraphSignature {
        file: file.to_string(),
        inputs: session.inputs().iter().map(|i| i.name().to_string()).collect(),
        outputs: session.outputs().iter().map(|o| o.name().to_string()).collect(),
    }
}

/// Load both graphs purely to report their tensor names, without binding them to
/// a synthesis plan. Works even when the names don't match the alias tables —
/// that is precisely the case it exists to diagnose.
pub fn inspect(dir: &Path) -> Result<ModelSignature> {
    let duration = build_session(&dir.join(DURATION_FILE))?;
    let decode = build_session(&dir.join(DECODE_FILE))?;

    let (vocab_file, vocab_size) = match PhonemeVocab::load(dir)? {
        Some(v) => (
            phonemes::VOCAB_FILES
                .iter()
                .find(|f| dir.join(f).exists())
                .map(|f| (*f).to_string()),
            Some(v.len()),
        ),
        None => (None, None),
    };

    Ok(ModelSignature {
        duration: signature_of(&duration, DURATION_FILE),
        decode: signature_of(&decode, DECODE_FILE),
        vocab_file,
        vocab_size,
    })
}

/// Render a signature as a human-readable block for error messages.
fn describe(sig: &GraphSignature) -> String {
    format!(
        "  {}:\n    inputs:  {}\n    outputs: {}",
        sig.file,
        sig.inputs.join(", "),
        sig.outputs.join(", ")
    )
}

// ── Session construction ──────────────────────────────────────────────────────

fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .unwrap_or(1)
}

fn build_session(path: &Path) -> Result<Session> {
    if !path.exists() {
        bail!(
            "Inflect-Micro-v2 graph {} is missing. Download the model from \
             Settings → Text-to-Speech, or place the files there manually.",
            path.display()
        );
    }
    // Builder methods return `ort::Error<SessionBuilder>`, which carries the
    // builder back for recovery and so isn't a `Send + Sync` std error that
    // anyhow can absorb — format via Display instead. This mirrors the same
    // dance in `voxctrl-inference::moonshine::build_session`.
    Session::builder()
        .map_err(|e| anyhow!("ort session builder: {e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow!("set optimization level: {e}"))?
        .with_intra_threads(threads())
        .map_err(|e| anyhow!("set intra threads: {e}"))?
        .commit_from_file(path)
        .with_context(|| format!("load ONNX graph {}", path.display()))
}

// ── Loaded model ──────────────────────────────────────────────────────────────

pub struct InflectModel {
    duration: Session,
    decode: Session,
    vocab: PhonemeVocab,
    /// One entry per `duration.onnx` input, in declared order.
    duration_plan: Vec<DurationInput>,
    /// One entry per `decode.onnx` input, in declared order.
    decode_plan: Vec<DecodeInput>,
    signature: ModelSignature,
    dir: PathBuf,
}

impl InflectModel {
    /// Load both graphs and resolve their inputs into a binding plan.
    ///
    /// Fails when the phoneme vocabulary is absent or when any input cannot be
    /// matched to a known role, rather than guessing — a wrong binding produces
    /// audio that sounds broken in ways that are hard to trace back here.
    pub fn load(dir: &Path) -> Result<Self> {
        info!("Loading Inflect-Micro-v2 from {}", dir.display());

        let duration = build_session(&dir.join(DURATION_FILE))?;
        let decode = build_session(&dir.join(DECODE_FILE))?;

        let duration_sig = signature_of(&duration, DURATION_FILE);
        let decode_sig = signature_of(&decode, DECODE_FILE);

        let vocab = PhonemeVocab::load(dir)?.ok_or_else(|| {
            anyhow!(
                "No phoneme vocabulary found in {}. Expected one of: {}. The \
                 vocabulary ships with the ONNX export and maps eSpeak IPA to the \
                 model's phoneme ids; synthesis cannot be correct without it.",
                dir.display(),
                phonemes::VOCAB_FILES.join(", ")
            )
        })?;

        let duration_plan = plan_duration(&duration_sig, &decode_sig)?;
        let decode_plan = plan_decode(&duration_sig, &decode_sig)?;

        let vocab_len = vocab.len();
        info!(
            "Inflect-Micro-v2 ready: {} phonemes in vocabulary, {} duration inputs, {} decode inputs",
            vocab_len,
            duration_plan.len(),
            decode_plan.len()
        );

        Ok(Self {
            duration,
            decode,
            vocab,
            duration_plan,
            decode_plan,
            signature: ModelSignature {
                duration: duration_sig,
                decode: decode_sig,
                vocab_file: phonemes::VOCAB_FILES
                    .iter()
                    .find(|f| dir.join(f).exists())
                    .map(|f| (*f).to_string()),
                vocab_size: Some(vocab_len),
            },
            dir: dir.to_path_buf(),
        })
    }

    pub fn signature(&self) -> &ModelSignature {
        &self.signature
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Synthesize `text` to a 24 kHz mono waveform in `[-1.0, 1.0]`.
    ///
    /// `speed` is the shared TTS speed multiplier; VITS expresses rate as
    /// `length_scale`, which is its reciprocal (a longer sequence is slower).
    pub fn synthesize(
        &mut self,
        text: &str,
        cfg: &voxctrl_config::InflectMicroConfig,
        speed: f32,
    ) -> Result<Vec<f32>> {
        // Destructured so the sessions can be borrowed mutably for `run` while the
        // plans and the input names (which live in `signature`) stay borrowed
        // immutably — `Session::run` needs `&mut`, and the graph's own
        // `inputs()` borrow would otherwise conflict with it.
        let Self {
            duration,
            decode,
            vocab,
            duration_plan,
            decode_plan,
            signature,
            ..
        } = self;

        let ipa = phonemes::phonemize(text)?;
        if ipa.trim().is_empty() {
            return Ok(Vec::new());
        }

        let encoded = vocab.encode(&ipa);
        if !encoded.skipped.is_empty() {
            warn!(
                "Inflect-Micro-v2: {} IPA symbol(s) absent from the phoneme vocabulary and skipped: {}",
                encoded.skipped.len(),
                encoded.skipped.join(" ")
            );
        }
        if encoded.ids.is_empty() {
            return Ok(Vec::new());
        }

        let length_scale = if speed > 0.0 { 1.0 / speed } else { 1.0 };
        let n = encoded.ids.len();

        // ── Stage 1: phonemes → aligned latents ───────────────────────────────
        let duration_outputs = {
            let mut feed: Vec<(&str, SessionInputValue)> =
                Vec::with_capacity(duration_plan.len());
            // Tensors must outlive the `run` call, so they are built up front and
            // referenced by the feed rather than constructed inline.
            let ids_t = Tensor::from_array(([1_usize, n], encoded.ids.clone()))
                .context("build phoneme id tensor")?;
            let lengths_t = Tensor::from_array(([1_usize], vec![n as i64]))
                .context("build phoneme length tensor")?;
            let scales_t = Tensor::from_array((
                [3_usize],
                vec![cfg.noise_scale, length_scale, cfg.noise_scale_w],
            ))
            .context("build scales tensor")?;
            let noise_t = Tensor::from_array(([1_usize], vec![cfg.noise_scale]))
                .context("build noise_scale tensor")?;
            let length_t = Tensor::from_array(([1_usize], vec![length_scale]))
                .context("build length_scale tensor")?;
            let noise_w_t = Tensor::from_array(([1_usize], vec![cfg.noise_scale_w]))
                .context("build noise_scale_w tensor")?;
            let seed_t = Tensor::from_array(([1_usize], vec![cfg.seed as i64]))
                .context("build seed tensor")?;

            for (slot, name) in duration_plan.iter().zip(signature.duration.inputs.iter()) {
                let value: SessionInputValue = match slot {
                    DurationInput::PhonemeIds => (&ids_t).into(),
                    DurationInput::PhonemeLengths => (&lengths_t).into(),
                    DurationInput::Scales => (&scales_t).into(),
                    DurationInput::NoiseScale => (&noise_t).into(),
                    DurationInput::LengthScale => (&length_t).into(),
                    DurationInput::NoiseScaleW => (&noise_w_t).into(),
                    DurationInput::Seed => (&seed_t).into(),
                };
                feed.push((name.as_str(), value));
            }

            let outputs = duration.run(feed).context("duration graph run")?;
            let mut owned: Vec<(Vec<i64>, Vec<f32>)> = Vec::with_capacity(outputs.len());
            for i in 0..outputs.len() {
                let (shape, data) = outputs[i]
                    .try_extract_tensor::<f32>()
                    .with_context(|| format!("extract duration output {i}"))?;
                owned.push((shape.to_vec(), data.to_vec()));
            }
            owned
        };

        // ── Stage 2: latents → waveform ───────────────────────────────────────
        let mut feed: Vec<(&str, SessionInputValue)> = Vec::with_capacity(decode_plan.len());

        let stage1_tensors: Vec<Tensor<f32>> = duration_outputs
            .iter()
            .map(|(shape, data)| {
                Tensor::from_array((shape.clone(), data.clone()))
                    .context("rebuild duration output as decode input")
            })
            .collect::<Result<_>>()?;

        // Latent length is the time dimension of the first stage-1 output.
        let latent_len = duration_outputs
            .first()
            .and_then(|(shape, _)| shape.last().copied())
            .unwrap_or(0);
        let latent_lengths_t = Tensor::from_array(([1_usize], vec![latent_len]))
            .context("build latent length tensor")?;
        let noise_t = Tensor::from_array(([1_usize], vec![cfg.noise_scale]))
            .context("build decode noise_scale tensor")?;
        let seed_t = Tensor::from_array(([1_usize], vec![cfg.seed as i64]))
            .context("build decode seed tensor")?;

        for (slot, name) in decode_plan.iter().zip(signature.decode.inputs.iter()) {
            let value: SessionInputValue = match slot {
                DecodeInput::FromDuration(i) => (&stage1_tensors[*i]).into(),
                DecodeInput::LatentLengths => (&latent_lengths_t).into(),
                DecodeInput::NoiseScale => (&noise_t).into(),
                DecodeInput::Seed => (&seed_t).into(),
            };
            feed.push((name.as_str(), value));
        }

        let outputs = decode.run(feed).context("decode graph run")?;
        if outputs.len() == 0 {
            bail!("{DECODE_FILE} produced no outputs");
        }
        // The waveform is taken as the graph's first output — a vocoder graph
        // emits it first, and any additional outputs are diagnostics. `inspect`
        // reports the real output list if this ever needs revisiting.
        let (_, audio) = outputs[0]
            .try_extract_tensor::<f32>()
            .context("extract decoded waveform")?;

        Ok(audio.to_vec())
    }

    pub const fn sample_rate() -> u32 {
        SAMPLE_RATE
    }
}

// ── Input planning ────────────────────────────────────────────────────────────

fn unresolved(
    graph: &str,
    name: &str,
    duration: &GraphSignature,
    decode: &GraphSignature,
) -> anyhow::Error {
    anyhow!(
        "Inflect-Micro-v2: could not map input '{name}' of {graph} to a known role.\n\
         Discovered signature:\n{}\n{}\n\
         The tensor names this build expects follow the conventional VITS export; \
         if the published export differs, the alias tables in \
         crates/voxctrl-tts/src/inflect/model.rs need the real names.",
        describe(duration),
        describe(decode)
    )
}

fn plan_duration(duration: &GraphSignature, decode: &GraphSignature) -> Result<Vec<DurationInput>> {
    let mut plan = Vec::with_capacity(duration.inputs.len());
    for name in &duration.inputs {
        let slot = if matches(name, &PHONEME_IDS_ALIASES) {
            DurationInput::PhonemeIds
        } else if matches(name, &PHONEME_LENGTHS_ALIASES) {
            DurationInput::PhonemeLengths
        } else if matches(name, &SCALES_ALIASES) {
            DurationInput::Scales
        } else if matches(name, &NOISE_SCALE_ALIASES) {
            DurationInput::NoiseScale
        } else if matches(name, &LENGTH_SCALE_ALIASES) {
            DurationInput::LengthScale
        } else if matches(name, &NOISE_SCALE_W_ALIASES) {
            DurationInput::NoiseScaleW
        } else if matches(name, &SEED_ALIASES) {
            DurationInput::Seed
        } else {
            return Err(unresolved(DURATION_FILE, name, duration, decode));
        };
        plan.push(slot);
    }

    if !plan.contains(&DurationInput::PhonemeIds) {
        bail!(
            "Inflect-Micro-v2: {DURATION_FILE} declares no phoneme-id input.\n\
             Discovered signature:\n{}\n{}",
            describe(duration),
            describe(decode)
        );
    }
    Ok(plan)
}

fn plan_decode(duration: &GraphSignature, decode: &GraphSignature) -> Result<Vec<DecodeInput>> {
    let mut plan = Vec::with_capacity(decode.inputs.len());
    let mut next_stage1 = 0usize;

    for name in &decode.inputs {
        // Prefer connecting by matching name against a stage-1 output: that is an
        // unambiguous link between the graphs.
        if let Some(idx) = duration.outputs.iter().position(|o| o.eq_ignore_ascii_case(name)) {
            plan.push(DecodeInput::FromDuration(idx));
            next_stage1 = next_stage1.max(idx + 1);
            continue;
        }

        let slot = if matches(name, &LATENT_ALIASES) {
            // A conventionally-named latent input that didn't match by name; take
            // stage-1 outputs in declared order.
            let idx = next_stage1;
            if idx >= duration.outputs.len() {
                return Err(unresolved(DECODE_FILE, name, duration, decode));
            }
            next_stage1 += 1;
            DecodeInput::FromDuration(idx)
        } else if matches(name, &LATENT_LENGTHS_ALIASES) {
            DecodeInput::LatentLengths
        } else if matches(name, &NOISE_SCALE_ALIASES) {
            DecodeInput::NoiseScale
        } else if matches(name, &SEED_ALIASES) {
            DecodeInput::Seed
        } else {
            return Err(unresolved(DECODE_FILE, name, duration, decode));
        };
        plan.push(slot);
    }

    if !plan.iter().any(|s| matches!(s, DecodeInput::FromDuration(_))) {
        bail!(
            "Inflect-Micro-v2: {DECODE_FILE} takes nothing from {DURATION_FILE}, so the two \
             stages cannot be connected.\nDiscovered signature:\n{}\n{}",
            describe(duration),
            describe(decode)
        );
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(file: &str, inputs: &[&str], outputs: &[&str]) -> GraphSignature {
        GraphSignature {
            file: file.to_string(),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ── Alias matching ───────────────────────────────────────────────────────

    #[test]
    fn test_matches_is_case_insensitive() {
        assert!(matches("INPUT_IDS", &PHONEME_IDS_ALIASES));
        assert!(!matches("totally_unknown", &PHONEME_IDS_ALIASES));
    }

    // ── Duration planning ────────────────────────────────────────────────────

    #[test]
    fn test_plan_duration_conventional_vits_signature() {
        // The shape a standard VITS ONNX export (e.g. Piper's) declares.
        let d = sig(DURATION_FILE, &["input", "input_lengths", "scales"], &["z"]);
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        let plan = plan_duration(&d, &c).unwrap();
        assert_eq!(
            plan,
            vec![
                DurationInput::PhonemeIds,
                DurationInput::PhonemeLengths,
                DurationInput::Scales
            ]
        );
    }

    #[test]
    fn test_plan_duration_separate_scale_inputs() {
        let d = sig(
            DURATION_FILE,
            &["phoneme_ids", "phoneme_lengths", "noise_scale", "length_scale", "noise_scale_w"],
            &["z"],
        );
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        let plan = plan_duration(&d, &c).unwrap();
        assert!(plan.contains(&DurationInput::NoiseScale));
        assert!(plan.contains(&DurationInput::LengthScale));
        assert!(plan.contains(&DurationInput::NoiseScaleW));
    }

    #[test]
    fn test_plan_duration_accepts_seed_input() {
        let d = sig(DURATION_FILE, &["input", "seed"], &["z"]);
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        assert!(plan_duration(&d, &c).unwrap().contains(&DurationInput::Seed));
    }

    #[test]
    fn test_plan_duration_rejects_unknown_input_with_signature_in_message() {
        let d = sig(DURATION_FILE, &["input", "mystery_tensor"], &["z"]);
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        let err = plan_duration(&d, &c).unwrap_err().to_string();
        assert!(err.contains("mystery_tensor"), "names the offending input");
        assert!(err.contains("Discovered signature"), "includes the report");
        assert!(err.contains("duration.onnx"));
    }

    #[test]
    fn test_plan_duration_requires_phoneme_ids() {
        let d = sig(DURATION_FILE, &["input_lengths"], &["z"]);
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        let err = plan_duration(&d, &c).unwrap_err().to_string();
        assert!(err.contains("no phoneme-id input"));
    }

    // ── Decode planning ──────────────────────────────────────────────────────

    #[test]
    fn test_plan_decode_links_stages_by_matching_name() {
        let d = sig(DURATION_FILE, &["input"], &["z", "y_mask"]);
        let c = sig(DECODE_FILE, &["y_mask", "z"], &["audio"]);
        let plan = plan_decode(&d, &c).unwrap();
        // Bound by name, so order in `decode` doesn't matter.
        assert_eq!(plan, vec![DecodeInput::FromDuration(1), DecodeInput::FromDuration(0)]);
    }

    #[test]
    fn test_plan_decode_falls_back_to_declared_order_for_latent_alias() {
        let d = sig(DURATION_FILE, &["input"], &["hidden"]);
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        let plan = plan_decode(&d, &c).unwrap();
        assert_eq!(plan, vec![DecodeInput::FromDuration(0)]);
    }

    #[test]
    fn test_plan_decode_accepts_latent_lengths_and_noise() {
        let d = sig(DURATION_FILE, &["input"], &["z"]);
        let c = sig(DECODE_FILE, &["z", "z_lengths", "noise_scale"], &["audio"]);
        let plan = plan_decode(&d, &c).unwrap();
        assert!(plan.contains(&DecodeInput::LatentLengths));
        assert!(plan.contains(&DecodeInput::NoiseScale));
    }

    #[test]
    fn test_plan_decode_rejects_unknown_input() {
        let d = sig(DURATION_FILE, &["input"], &["z"]);
        let c = sig(DECODE_FILE, &["z", "wat"], &["audio"]);
        let err = plan_decode(&d, &c).unwrap_err().to_string();
        assert!(err.contains("wat"));
    }

    #[test]
    fn test_plan_decode_requires_a_link_to_stage_one() {
        let d = sig(DURATION_FILE, &["input"], &["z"]);
        let c = sig(DECODE_FILE, &["noise_scale"], &["audio"]);
        let err = plan_decode(&d, &c).unwrap_err().to_string();
        assert!(err.contains("cannot be connected"));
    }

    #[test]
    fn test_plan_decode_latent_alias_beyond_stage_one_outputs_errors() {
        let d = sig(DURATION_FILE, &["input"], &[]);
        let c = sig(DECODE_FILE, &["z"], &["audio"]);
        assert!(plan_decode(&d, &c).is_err());
    }

    // ── Reporting ────────────────────────────────────────────────────────────

    #[test]
    fn test_describe_lists_inputs_and_outputs() {
        let s = sig("duration.onnx", &["a", "b"], &["c"]);
        let text = describe(&s);
        assert!(text.contains("duration.onnx"));
        assert!(text.contains("a, b"));
        assert!(text.contains("c"));
    }

    #[test]
    fn test_threads_is_at_least_one() {
        assert!(threads() >= 1);
    }
}
