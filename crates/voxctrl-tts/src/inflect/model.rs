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
//! The tensor names come from the export's own `inference_onnx.py`, so they are
//! the graphs' real signature rather than an inference from convention. Both are
//! verified at load time and a missing input is a hard error reporting what the
//! graph actually declares, so a revision mismatch surfaces as an actionable
//! message rather than as garbled audio. The `inflect_micro_inspect` Tauri
//! command reports the same signature without loading a voice.
//!
//! `zp_noise` is drawn host-side rather than sampled inside the graph — see
//! [`StandardNormal`] for what that means for seed reproducibility.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use ort::{
    session::{builder::GraphOptimizationLevel, Session, SessionInputValue},
    value::Tensor,
};
use tracing::{info, warn};

use super::phonemes::{self, PhonemeVocab};
use super::{DECODE_FILE, DURATION_FILE, SAMPLE_RATE};

// ── Tensor contract ───────────────────────────────────────────────────────────
//
// Taken from the export's own `inference_onnx.py`, so these are the graphs'
// actual input and output names rather than an inference from convention.

/// `duration.onnx` inputs.
const DURATION_INPUTS: [&str; 3] = ["tokens", "lengths", "length_scale"];
/// `duration.onnx` outputs, in the order the reference script requests them.
const DURATION_OUTPUTS: [&str; 3] = ["m_p_exp", "logs_p_exp", "y_mask"];

/// `decode.onnx` inputs. `zp_noise` is sampled host-side (see [`StandardNormal`]).
const DECODE_INPUTS: [&str; 5] =
    ["m_p_exp", "logs_p_exp", "y_mask", "zp_noise", "noise_scale"];
/// `decode.onnx` output.
const DECODE_OUTPUT: &str = "waveform";

// ── Discovered signature ──────────────────────────────────────────────────────

/// One graph's declared inputs and outputs.
///
/// `inputs`/`outputs` hold bare names, which is what the contract check compares
/// against. `input_details`/`output_details` carry ONNX Runtime's full
/// description of each (element type and shape), which is what a diagnostic
/// report needs — a name alone can't tell you a dtype or rank is wrong.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphSignature {
    pub file: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub input_details: Vec<String>,
    pub output_details: Vec<String>,
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
        input_details: session.inputs().iter().map(|i| format!("{i:?}")).collect(),
        output_details: session.outputs().iter().map(|o| format!("{o:?}")).collect(),
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
        "  {}:\n    inputs:\n      {}\n    outputs:\n      {}",
        sig.file,
        sig.input_details.join("\n      "),
        sig.output_details.join("\n      ")
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

// ── Latent noise ──────────────────────────────────────────────────────────────

/// Seeded standard-normal sampler for the `zp_noise` input.
///
/// The reference script draws this host-side with NumPy
/// (`np.random.default_rng(seed).standard_normal(...)`). Reproducing NumPy's
/// PCG64 stream and ziggurat sampler exactly would be the only way to match its
/// output sample-for-sample, so this uses its own PCG64 with Box–Muller instead:
/// output is fully deterministic for a given seed within VoxCtrl, but a given
/// seed does not correspond to the same voice as the same seed in the Python
/// reference. Any correctly-distributed noise yields valid audio — the seed only
/// controls which sample from the distribution you get.
struct StandardNormal {
    state: u128,
    /// Box–Muller produces two deviates per pass; this holds the spare.
    spare: Option<f32>,
}

impl StandardNormal {
    /// PCG64 multiplier and increment, as specified by the reference implementation.
    const MULTIPLIER: u128 = 47026247687942121848144207491837523525;
    const INCREMENT: u128 = 117397592171526113268558934119004209487;

    fn new(seed: u64) -> Self {
        let mut rng = Self { state: 0, spare: None };
        rng.state = rng
            .state
            .wrapping_add(Self::INCREMENT)
            .wrapping_add(seed as u128);
        rng.step();
        rng
    }

    fn step(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(Self::MULTIPLIER)
            .wrapping_add(Self::INCREMENT);
        // XSL-RR output: xor the halves, then rotate by the top 6 bits.
        let xored = ((self.state >> 64) ^ self.state) as u64;
        let rot = (self.state >> 122) as u32;
        xored.rotate_right(rot)
    }

    /// Uniform in (0, 1] — zero is excluded so `ln` stays finite.
    fn next_open_unit(&mut self) -> f64 {
        let bits = self.step() >> 11; // 53 significant bits
        (bits as f64 + 1.0) / (9007199254740992.0 + 1.0)
    }

    fn next(&mut self) -> f32 {
        if let Some(spare) = self.spare.take() {
            return spare;
        }
        let u1 = self.next_open_unit();
        let u2 = self.next_open_unit();
        let radius = (-2.0 * u1.ln()).sqrt();
        let theta = std::f64::consts::TAU * u2;
        self.spare = Some((radius * theta.sin()) as f32);
        (radius * theta.cos()) as f32
    }

    fn fill(&mut self, n: usize) -> Vec<f32> {
        (0..n).map(|_| self.next()).collect()
    }
}

// ── Loaded model ──────────────────────────────────────────────────────────────

pub struct InflectModel {
    duration: Session,
    decode: Session,
    vocab: PhonemeVocab,
    signature: ModelSignature,
    dir: PathBuf,
}

/// Fail with the discovered signature when a graph doesn't declare what the
/// reference contract says it should.
fn require_inputs(sig: &GraphSignature, required: &[&str], other: &GraphSignature) -> Result<()> {
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|r| !sig.inputs.iter().any(|i| i == r))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "Inflect-Micro-v2: {} does not declare the expected input(s): {}.\n\
         Discovered signature:\n{}\n{}\n\
         The contract this build targets comes from the export's own \
         inference_onnx.py; a mismatch means the downloaded graphs are a \
         different revision.",
        sig.file,
        missing.join(", "),
        describe(sig),
        describe(other)
    )
}

impl InflectModel {
    /// Load both graphs and verify they declare the expected tensor contract.
    ///
    /// Fails when the phoneme table is absent or a graph's signature doesn't
    /// match, rather than proceeding — a wrong binding produces audio that
    /// sounds broken in ways that are hard to trace back here.
    pub fn load(dir: &Path) -> Result<Self> {
        info!("Loading Inflect-Micro-v2 from {}", dir.display());

        let duration = build_session(&dir.join(DURATION_FILE))?;
        let decode = build_session(&dir.join(DECODE_FILE))?;

        let duration_sig = signature_of(&duration, DURATION_FILE);
        let decode_sig = signature_of(&decode, DECODE_FILE);

        let vocab = PhonemeVocab::load(dir)?.ok_or_else(|| {
            anyhow!(
                "No phoneme table found in {}. The export derives phoneme ids from \
                 the ordered `symbols` list in its text frontend, so that list must \
                 be present; synthesis cannot be correct without it. A symbols.py, \
                 a JSON array, or a symbol->id table are all accepted.",
                dir.display()
            )
        })?;

        // Logged once per load: if synthesis then fails inside ONNX Runtime, this
        // is the record of what the graphs actually declared.
        info!("Inflect-Micro-v2 graph signatures:\n{}\n{}", describe(&duration_sig), describe(&decode_sig));

        require_inputs(&duration_sig, &DURATION_INPUTS, &decode_sig)?;
        require_inputs(&decode_sig, &DECODE_INPUTS, &duration_sig)?;

        let vocab_len = vocab.len();
        let vocab_file = vocab
            .source
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        info!(
            "Inflect-Micro-v2 phoneme table: {} entries from {}",
            vocab_len,
            vocab
                .source
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "built-in fallback".into())
        );

        Ok(Self {
            duration,
            decode,
            vocab,
            signature: ModelSignature {
                duration: duration_sig,
                decode: decode_sig,
                vocab_file,
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

    /// Synthesize one chunk to a 24 kHz mono waveform in `[-1.0, 1.0]`.
    ///
    /// Mirrors `InflectONNX._synthesize_chunk`: phonemize, tokenize with blanks,
    /// run the duration graph for the expanded latent statistics, draw the latent
    /// noise host-side, then decode. `speed` is the shared TTS multiplier; VITS
    /// expresses rate as `length_scale`, its reciprocal.
    pub fn synthesize(
        &mut self,
        text: &str,
        cfg: &voxctrl_config::InflectMicroConfig,
        speed: f32,
        seed: u64,
    ) -> Result<Vec<f32>> {
        let Self { duration, decode, vocab, signature, .. } = self;

        let ipa = phonemes::phonemize(text)?;
        if ipa.trim().is_empty() {
            return Ok(Vec::new());
        }

        let encoded = vocab.encode(&ipa);
        if !encoded.skipped.is_empty() {
            warn!(
                "Inflect-Micro-v2: {} IPA symbol(s) absent from the phoneme table and skipped: {}",
                encoded.skipped.len(),
                encoded.skipped.join(" ")
            );
        }
        if encoded.ids.is_empty() {
            return Ok(Vec::new());
        }

        let n = encoded.ids.len();
        let length_scale = if speed > 0.0 { 1.0 / speed } else { 1.0 };

        // ── Stage 1: tokens → expanded latent statistics ──────────────────────
        let (m_p_exp, logs_p_exp, y_mask) = {
            let tokens = Tensor::from_array(([1_usize, n], encoded.ids.clone()))
                .context("build tokens tensor")?;
            let lengths = Tensor::from_array(([1_usize], vec![n as i64]))
                .context("build lengths tensor")?;
            // Scalar (rank-0), matching `np.asarray(value, dtype=np.float32)`.
            let length_scale_t = Tensor::from_array(([0_usize; 0], vec![length_scale]))
                .context("build length_scale tensor")?;

            let outputs = duration
                .run(vec![
                    ("tokens", SessionInputValue::from(&tokens)),
                    ("lengths", SessionInputValue::from(&lengths)),
                    ("length_scale", SessionInputValue::from(&length_scale_t)),
                ])
                .map_err(|e| {
                    anyhow!(
                        "{DURATION_FILE} run failed: {e}\n\
                         Fed: tokens i64[1,{n}], lengths i64[1], length_scale f32 scalar.\n\
                         Graph declares:\n{}",
                        describe(&signature.duration)
                    )
                })?;

            let mut extracted = Vec::with_capacity(DURATION_OUTPUTS.len());
            for name in DURATION_OUTPUTS {
                let (shape, data) = outputs[name]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| {
                        anyhow!(
                            "duration output '{name}' is not float32: {e}\n\
                             Graph declares:\n{}",
                            describe(&signature.duration)
                        )
                    })?;
                extracted.push((shape.to_vec(), data.to_vec()));
            }
            let mut it = extracted.into_iter();
            (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
        };

        // ── Stage 2: latents + noise → waveform ───────────────────────────────
        let noise_values = StandardNormal::new(seed).fill(m_p_exp.1.len());

        let m_shape = m_p_exp.0.clone();
        let m_p_exp_t = Tensor::from_array((m_p_exp.0.clone(), m_p_exp.1))
            .context("build m_p_exp tensor")?;
        let logs_p_exp_t = Tensor::from_array((logs_p_exp.0, logs_p_exp.1))
            .context("build logs_p_exp tensor")?;
        let y_mask_t =
            Tensor::from_array((y_mask.0, y_mask.1)).context("build y_mask tensor")?;
        // zp_noise matches m_p_exp's shape exactly.
        let zp_noise_t = Tensor::from_array((m_p_exp.0, noise_values))
            .context("build zp_noise tensor")?;
        let noise_scale_t = Tensor::from_array(([0_usize; 0], vec![cfg.noise_scale]))
            .context("build noise_scale tensor")?;

        let outputs = decode
            .run(vec![
                ("m_p_exp", SessionInputValue::from(&m_p_exp_t)),
                ("logs_p_exp", SessionInputValue::from(&logs_p_exp_t)),
                ("y_mask", SessionInputValue::from(&y_mask_t)),
                ("zp_noise", SessionInputValue::from(&zp_noise_t)),
                ("noise_scale", SessionInputValue::from(&noise_scale_t)),
            ])
            .map_err(|e| {
                anyhow!(
                    "{DECODE_FILE} run failed: {e}\n\
                     Fed: m_p_exp f32{m_shape:?}, logs_p_exp f32, y_mask f32, \
                     zp_noise f32{m_shape:?}, noise_scale f32 scalar.\n\
                     Graph declares:\n{}",
                    describe(&signature.decode)
                )
            })?;

        let (_, audio) = outputs[DECODE_OUTPUT]
            .try_extract_tensor::<f32>()
            .context("extract decoded waveform")?;

        Ok(audio.iter().map(|s| s.clamp(-1.0, 1.0)).collect())
    }

    pub const fn sample_rate() -> u32 {
        SAMPLE_RATE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(file: &str, inputs: &[&str], outputs: &[&str]) -> GraphSignature {
        GraphSignature {
            file: file.to_string(),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
            input_details: inputs.iter().map(|s| s.to_string()).collect(),
            output_details: outputs.iter().map(|s| s.to_string()).collect(),
        }
    }

    // ── Contract validation ──────────────────────────────────────────────────

    #[test]
    fn test_require_inputs_accepts_the_reference_contract() {
        let d = sig(DURATION_FILE, &DURATION_INPUTS, &DURATION_OUTPUTS);
        let c = sig(DECODE_FILE, &DECODE_INPUTS, &[DECODE_OUTPUT]);
        assert!(require_inputs(&d, &DURATION_INPUTS, &c).is_ok());
        assert!(require_inputs(&c, &DECODE_INPUTS, &d).is_ok());
    }

    #[test]
    fn test_require_inputs_ignores_extra_inputs() {
        // A graph declaring more than we feed is fine; only absence is fatal.
        let d = sig(DURATION_FILE, &["tokens", "lengths", "length_scale", "extra"], &[]);
        let c = sig(DECODE_FILE, &[], &[]);
        assert!(require_inputs(&d, &DURATION_INPUTS, &c).is_ok());
    }

    #[test]
    fn test_require_inputs_names_every_missing_tensor() {
        let d = sig(DURATION_FILE, &["tokens"], &[]);
        let c = sig(DECODE_FILE, &[], &[]);
        let err = require_inputs(&d, &DURATION_INPUTS, &c).unwrap_err().to_string();
        assert!(err.contains("lengths"), "names the missing inputs: {err}");
        assert!(err.contains("length_scale"));
        assert!(!err.contains(" tokens,"), "does not name inputs that are present");
    }

    #[test]
    fn test_require_inputs_reports_both_signatures() {
        let d = sig(DURATION_FILE, &[], &[]);
        let c = sig(DECODE_FILE, &["m_p_exp"], &["waveform"]);
        let err = require_inputs(&d, &DURATION_INPUTS, &c).unwrap_err().to_string();
        assert!(err.contains("Discovered signature"));
        assert!(err.contains(DURATION_FILE));
        assert!(err.contains(DECODE_FILE));
    }

    #[test]
    fn test_zp_noise_is_a_decode_input() {
        // The latent noise is drawn host-side and fed in; forgetting it makes
        // synthesis fail at run time rather than at load.
        assert!(DECODE_INPUTS.contains(&"zp_noise"));
    }

    #[test]
    fn test_duration_outputs_feed_decode_inputs() {
        for name in DURATION_OUTPUTS {
            assert!(
                DECODE_INPUTS.contains(&name),
                "{name} should carry from the duration graph into decode"
            );
        }
    }

    // ── Reporting ────────────────────────────────────────────────────────────

    #[test]
    fn test_describe_lists_inputs_and_outputs() {
        let s = sig("duration.onnx", &["a", "b"], &["c"]);
        let text = describe(&s);
        assert!(text.contains("duration.onnx"));
        // The report shows per-tensor detail lines, not a comma-joined name list.
        assert!(text.contains("a"));
        assert!(text.contains("b"));
        assert!(text.contains("c"));
    }

    #[test]
    fn test_threads_is_at_least_one() {
        assert!(threads() >= 1);
    }

    // ── Latent noise sampler ─────────────────────────────────────────────────

    #[test]
    fn test_standard_normal_is_deterministic_for_a_seed() {
        let a = StandardNormal::new(42).fill(64);
        let b = StandardNormal::new(42).fill(64);
        assert_eq!(a, b, "same seed must reproduce the same noise");
    }

    #[test]
    fn test_standard_normal_differs_between_seeds() {
        let a = StandardNormal::new(1).fill(64);
        let b = StandardNormal::new(2).fill(64);
        assert_ne!(a, b);
    }

    #[test]
    fn test_standard_normal_is_roughly_unit_gaussian() {
        let samples = StandardNormal::new(7).fill(20_000);
        let mean: f64 = samples.iter().map(|s| *s as f64).sum::<f64>() / samples.len() as f64;
        let variance: f64 = samples
            .iter()
            .map(|s| (*s as f64 - mean).powi(2))
            .sum::<f64>()
            / samples.len() as f64;
        assert!(mean.abs() < 0.05, "mean {mean} should be near 0");
        assert!((variance - 1.0).abs() < 0.1, "variance {variance} should be near 1");
    }

    #[test]
    fn test_standard_normal_produces_both_signs() {
        let samples = StandardNormal::new(3).fill(256);
        assert!(samples.iter().any(|s| *s > 0.0));
        assert!(samples.iter().any(|s| *s < 0.0));
    }

    #[test]
    fn test_standard_normal_values_are_finite() {
        assert!(StandardNormal::new(11).fill(4096).iter().all(|s| s.is_finite()));
    }

    #[test]
    fn test_standard_normal_fill_length() {
        assert_eq!(StandardNormal::new(0).fill(0).len(), 0);
        assert_eq!(StandardNormal::new(0).fill(101).len(), 101);
    }
}
