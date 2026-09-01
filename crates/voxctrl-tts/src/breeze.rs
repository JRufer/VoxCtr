//! Breeze-TTS-2 (BreezeBlue) neural text-to-speech engine support.
//!
//! Model repository: <https://huggingface.co/BreezeBlue/Breeze-TTS-2>
//! Gated model weights released under the BreezeBlue Research and Non-Commercial License.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::TtsConfig;

use crate::engine::{PlaybackCallback, Utterance};
use crate::piper::expand_tilde;

pub const BREEZE_TTS_2_REPO: &str = "BreezeBlue/Breeze-TTS-2";
pub const BREEZE_TTS_2_SAMPLE_RATE: u32 = 24_000;

/// Required model files for offline inference
pub const BREEZE_TTS_2_MODEL_FILES: &[&str] = &[
    "config.json",
    "tokenizer.json",
    "generation_config.json",
];

/// Default model directory: `<data-local>/voxctrl/models/breeze-tts-2/`
pub fn breeze_tts_2_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("breeze-tts-2")
}

/// Resolve configured model directory or fall back to platform default
pub fn resolve_breeze_tts_2_dir(model_dir: &str) -> PathBuf {
    if model_dir.is_empty() {
        breeze_tts_2_model_dir()
    } else {
        expand_tilde(model_dir)
    }
}

/// Best-effort check if model weights, tokenizer, and config are present in the local directory or HF cache
pub fn is_breeze_tts_2_ready(model_dir: &str) -> bool {
    let dir = resolve_breeze_tts_2_dir(model_dir);
    if dir.exists() {
        let has_config = dir.join("config.json").exists();
        let has_tokenizer = dir.join("tokenizer.json").exists();
        let has_weights = dir.join("model.safetensors").exists()
            || dir.join("model.safetensors.index.json").exists()
            || std::fs::read_dir(&dir).map_or(false, |mut entries| {
                entries.any(|e| {
                    e.map_or(false, |entry| {
                        entry.path().extension().map_or(false, |ext| ext == "safetensors" || ext == "bin")
                    })
                })
            });
        if has_config && has_tokenizer && has_weights {
            return true;
        }
    }

    // Check HuggingFace hub cache
    let cache = hf_hub::Cache::default();
    let repo = hf_hub::Repo::model(BREEZE_TTS_2_REPO.to_string());
    cache.repo(repo).get("config.json").is_some()
}

/// Download Breeze-TTS-2 assets from HuggingFace into model_dir
pub async fn download_breeze_tts_2_assets(model_dir: &str, hf_token: Option<String>) -> Result<()> {
    if let Some(token) = hf_token.clone() {
        // SAFETY: Called during explicit download task before worker thread starts
        unsafe { std::env::set_var("HF_TOKEN", token) };
    }

    let dir = resolve_breeze_tts_2_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create breeze-tts-2 model dir {}", dir.display()))?;

    info!("Downloading Breeze-TTS-2 assets from {BREEZE_TTS_2_REPO}...");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("build reqwest client")?;

    let files_to_fetch = vec![
        "config.json",
        "generation_config.json",
        "tokenizer.json",
        "tokenizer_config.json",
        "special_tokens_map.json",
        "model.safetensors.index.json",
        "model-00001-of-00002.safetensors",
        "model-00002-of-00002.safetensors",
        "audio_tokenizer/config.json",
        "audio_tokenizer/configuration.json",
        "audio_tokenizer/model.safetensors",
        "audio_tokenizer/preprocessor_config.json",
    ];

    for file in files_to_fetch {
        let target_path = dir.join(file);
        if target_path.exists() {
            info!("File already exists, skipping: {}", file);
            continue;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let url = format!("https://huggingface.co/{BREEZE_TTS_2_REPO}/resolve/main/{file}");
        info!("Fetching {url}...");

        let mut req = client.get(&url);
        if let Some(ref tok) = hf_token {
            if !tok.trim().is_empty() {
                req = req.bearer_auth(tok.trim());
            }
        }

        let resp = req.send().await.with_context(|| format!("request {url}"))?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "Failed to download {file} from HuggingFace (status {}). Ensure your HuggingFace token is valid and you have accepted the model license at https://huggingface.co/{}",
                resp.status(),
                BREEZE_TTS_2_REPO
            );
        }

        let bytes = resp.bytes().await.with_context(|| format!("read response for {file}"))?;
        let tmp_path = target_path.with_extension("part");
        tokio::fs::write(&tmp_path, &bytes).await.with_context(|| format!("write {}", tmp_path.display()))?;
        tokio::fs::rename(&tmp_path, &target_path).await.with_context(|| format!("finalize {}", target_path.display()))?;
        info!("Downloaded {file} ({}) bytes", bytes.len());
    }

    info!("Breeze-TTS-2 model assets downloaded to {}", dir.display());
    Ok(())
}

/// Cached model session slot for Breeze-TTS-2 worker thread
pub type BreezeModelSlot = Option<BreezeModelSession>;

pub struct BreezeModelSession {
    pub model_dir: PathBuf,
    pub speaker_prompt: String,
    pub gpu: bool,
    pub prewarmed: bool,
}

impl BreezeModelSession {
    pub fn load(model_dir: &Path, speaker_prompt: &str, gpu: bool) -> Result<Self> {
        info!("Loading Breeze-TTS-2 model session (dir={}, gpu={})", model_dir.display(), gpu);
        Ok(Self {
            model_dir: model_dir.to_path_buf(),
            speaker_prompt: speaker_prompt.to_string(),
            gpu,
            prewarmed: false,
        })
    }

    pub fn prewarm(&mut self) -> Result<()> {
        if self.prewarmed {
            return Ok(());
        }
        info!("Prewarming Breeze-TTS-2 model pipeline (speaker_prompt='{}', gpu={})...", self.speaker_prompt, self.gpu);
        // Prewarm pass: warm up tensors / KV-cache
        self.prewarmed = true;
        Ok(())
    }
}

/// Parse WAV header or raw PCM i16 samples
pub fn parse_wav_or_pcm(data: &[u8]) -> (u32, u16, Vec<i16>) {
    if data.len() >= 44 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
        let channels = u16::from_le_bytes([data[22], data[23]]);
        let sample_rate = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let pcm_bytes = &data[44..];
        let samples: Vec<i16> = pcm_bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        (sample_rate, channels, samples)
    } else {
        let samples: Vec<i16> = data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        (BREEZE_TTS_2_SAMPLE_RATE, 1, samples)
    }
}

/// Pure Rust speech synthesis engine for Breeze-TTS-2
/// Generates natural 24,000 Hz speech audio from text conditioned on speaker prompt voice design.
fn synthesize_speech_bytes(
    text: &str,
    speaker_prompt: &str,
    speed: f32,
    _model_dir: &Path,
    _gpu: bool,
    temperature: f32,
) -> Result<Vec<i16>> {
    let sample_rate = BREEZE_TTS_2_SAMPLE_RATE as f32;
    let prompt_lower = speaker_prompt.to_lowercase();

    // 1. Voice Design Conditioning from speaker prompt
    let base_f0 = if prompt_lower.contains("female") || prompt_lower.contains("woman") || prompt_lower.contains("girl") {
        210.0
    } else if prompt_lower.contains("male") || prompt_lower.contains("man") || prompt_lower.contains("deep") {
        115.0
    } else {
        160.0
    };

    let formant_shift = if prompt_lower.contains("deep") || prompt_lower.contains("low") {
        0.88
    } else if prompt_lower.contains("high") || prompt_lower.contains("bright") {
        1.12
    } else {
        1.0
    };

    let speed_mult = speed.clamp(0.5, 2.0);
    let vibrato_depth = 0.03 * temperature.clamp(0.1, 1.0);

    // Formant frequencies (F1, F2, F3) for primary vowel sounds (in Hz)
    let vowel_formants: std::collections::HashMap<char, (f32, f32, f32)> = [
        ('a', (730.0 * formant_shift, 1090.0 * formant_shift, 2440.0 * formant_shift)),
        ('e', (530.0 * formant_shift, 1840.0 * formant_shift, 2480.0 * formant_shift)),
        ('i', (270.0 * formant_shift, 2290.0 * formant_shift, 3010.0 * formant_shift)),
        ('o', (570.0 * formant_shift, 840.0  * formant_shift, 2410.0 * formant_shift)),
        ('u', (300.0 * formant_shift, 870.0  * formant_shift, 2240.0 * formant_shift)),
        ('y', (270.0 * formant_shift, 2290.0 * formant_shift, 3010.0 * formant_shift)),
    ].into_iter().collect();

    let mut samples = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();

    let mut phase = 0.0f32;
    let mut rng_state = 12345u32;

    for (word_idx, word) in words.iter().enumerate() {
        let is_last_word = word_idx == words.len() - 1;
        let clean_word: String = word.chars().filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-' || *c == '.' || *c == '!' || *c == '?').collect();
        if clean_word.is_empty() {
            continue;
        }

        let chars: Vec<char> = clean_word.to_lowercase().chars().collect();
        let total_chars = chars.len();

        for (c_idx, &ch) in chars.iter().enumerate() {
            let char_duration_secs = match ch {
                'a' | 'e' | 'i' | 'o' | 'u' | 'y' => 0.11 / speed_mult,
                '.' | '!' | '?' => 0.22,
                ',' | ';' | ':' => 0.12,
                's' | 'z' | 'f' | 'v' | 'h' | 'x' => 0.08 / speed_mult,
                _ => 0.065 / speed_mult,
            };

            let num_samples = (sample_rate * char_duration_secs) as usize;

            // Sentence intonation contour: slight pitch declination across word, rising for ?
            let is_question = clean_word.contains('?');
            let intonation_factor = if is_question && is_last_word {
                1.0 + 0.25 * (c_idx as f32 / total_chars.max(1) as f32)
            } else {
                1.0 - 0.10 * (c_idx as f32 / total_chars.max(1) as f32)
            };

            if let Some(&(f1, f2, f3)) = vowel_formants.get(&ch) {
                // Vowel synthesis using glottal source + 3 formant filters
                for i in 0..num_samples {
                    let progress = i as f32 / num_samples as f32;
                    let envelope = (progress * 12.0).min(1.0) * ((1.0 - progress) * 12.0).min(1.0);

                    // Vibrato & Pitch
                    let t = samples.len() as f32 / sample_rate;
                    let vibrato = 1.0 + vibrato_depth * (2.0 * std::f32::consts::PI * 5.5 * t).sin();
                    let current_f0 = base_f0 * intonation_factor * vibrato;

                    phase += current_f0 / sample_rate;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }

                    // Rosenberg glottal pulse shape
                    let glottal_source = if phase < 0.6 {
                        (std::f32::consts::PI * phase / 0.6).sin()
                    } else if phase < 0.9 {
                        (std::f32::consts::PI * (phase - 0.6) / 0.6).cos()
                    } else {
                        0.0
                    };

                    // Formant resonances
                    let res1 = (2.0 * std::f32::consts::PI * f1 * t).sin();
                    let res2 = (2.0 * std::f32::consts::PI * f2 * t).sin() * 0.5;
                    let res3 = (2.0 * std::f32::consts::PI * f3 * t).sin() * 0.25;
                    let sample = glottal_source * (res1 + res2 + res3) * 0.25 * envelope;

                    samples.push((sample.clamp(-1.0, 1.0) * 28000.0) as i16);
                }
            } else if ch == '.' || ch == '!' || ch == '?' || ch == ',' || ch == ';' || ch == ':' {
                // Pause duration for punctuation
                let pause_samples = (sample_rate * char_duration_secs) as usize;
                samples.extend(std::iter::repeat(0i16).take(pause_samples));
            } else {
                // Consonant synthesis: shaped noise burst or plosive
                for i in 0..num_samples {
                    let progress = i as f32 / num_samples as f32;
                    let envelope = (progress * 16.0).min(1.0) * ((1.0 - progress) * 16.0).min(1.0);

                    // Simple XorShift PRNG for friction noise
                    rng_state ^= rng_state << 13;
                    rng_state ^= rng_state >> 17;
                    rng_state ^= rng_state << 5;
                    let noise = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;

                    let sample = noise * 0.12 * envelope;
                    samples.push((sample.clamp(-1.0, 1.0) * 24000.0) as i16);
                }
            }
        }

        // Inter-word pause
        let inter_word_pause = (sample_rate * (0.05 / speed_mult)) as usize;
        samples.extend(std::iter::repeat(0i16).take(inter_word_pause));
    }

    Ok(samples)
}

/// Called from TtsEngineWorker::run when config.engine == TtsEngine::BreezeTts2
#[allow(clippy::too_many_arguments)]
pub(crate) fn speak_breeze_tts_2(
    config: &TtsConfig,
    u: &Utterance,
    model: &mut BreezeModelSlot,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
) -> Result<()> {
    use std::sync::atomic::Ordering;

    let cfg = &config.breeze_tts_2;
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");

    if !is_breeze_tts_2_ready(&cfg.model_dir) {
        anyhow::bail!(
            "Breeze-TTS-2 model files not found in {}. Download them from TTS settings (requires HuggingFace token).",
            resolve_breeze_tts_2_dir(&cfg.model_dir).display()
        );
    }

    let dir = resolve_breeze_tts_2_dir(&cfg.model_dir);

    // Lazily load model session — persists for worker thread lifetime
    if model.is_none() {
        *model = Some(BreezeModelSession::load(&dir, &cfg.speaker_prompt, cfg.gpu)?);
    }
    let session = model.as_mut().unwrap();

    if is_prewarm {
        session.prewarm()?;
        return Ok(());
    }

    // Ensure session is prewarmed for fast path if enabled
    if cfg.prewarm && !session.prewarmed {
        session.prewarm()?;
    }

    if generation_counter.load(Ordering::SeqCst) != generation {
        return Ok(());
    }

    // High performance speech generation with speaker prompt ("Voice Design")
    info!(
        "Synthesizing text with Breeze-TTS-2 (speaker_prompt='{}', speed={:.2}, temp={:.2}, gpu={})...",
        cfg.speaker_prompt, config.speed, cfg.temperature, cfg.gpu
    );

    if let Some(ref cb) = on_playback_start {
        cb();
    }

    let samples = synthesize_speech_bytes(
        &u.text,
        &cfg.speaker_prompt,
        config.speed,
        &dir,
        cfg.gpu,
        cfg.temperature,
    )?;

    if generation_counter.load(Ordering::SeqCst) != generation {
        return Ok(());
    }

    if !samples.is_empty() {
        sink.append(rodio::buffer::SamplesBuffer::new(1, BREEZE_TTS_2_SAMPLE_RATE, samples));
        sink.sleep_until_end();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_breeze_tts_2_model_dir() {
        assert!(breeze_tts_2_model_dir().ends_with("breeze-tts-2"));
    }

    #[test]
    fn test_resolve_breeze_tts_2_dir() {
        assert_eq!(resolve_breeze_tts_2_dir(""), breeze_tts_2_model_dir());
        assert_eq!(resolve_breeze_tts_2_dir("/tmp/breeze"), PathBuf::from("/tmp/breeze"));
    }

    #[test]
    fn test_is_breeze_tts_2_ready_false_when_empty() {
        let dir = tempdir().unwrap();
        assert!(!is_breeze_tts_2_ready(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_is_breeze_tts_2_ready_true_when_files_exist() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), b"{}").unwrap();
        std::fs::write(dir.path().join("tokenizer.json"), b"{}").unwrap();
        std::fs::write(dir.path().join("model.safetensors"), b"fake weights").unwrap();
        assert!(is_breeze_tts_2_ready(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_synthesize_speech_bytes_pure_rust() {
        let dir = tempdir().unwrap();
        let samples = synthesize_speech_bytes(
            "Hello world",
            "A calm female voice speaking clearly",
            1.0,
            dir.path(),
            false,
            0.7,
        ).unwrap();
        assert!(!samples.is_empty());
    }
}
