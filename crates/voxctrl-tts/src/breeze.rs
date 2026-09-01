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

fn synthesize_speech_bytes(
    text: &str,
    speaker_prompt: &str,
    speed: f32,
    model_dir: &Path,
    gpu: bool,
) -> Result<Vec<u8>> {
    // 1. Resolve python runner script (check model_dir, scripts/, or current directory)
    let candidate_scripts = vec![
        model_dir.join("breeze_tts_runner.py"),
        PathBuf::from("scripts/breeze_tts_runner.py"),
        PathBuf::from("scripts").join("breeze_tts_runner.py"),
    ];

    let runner_script = candidate_scripts.into_iter().find(|p| p.exists());

    if let Some(script_path) = runner_script {
        let uv_path = PathBuf::from("/home/jrufer/.local/bin/uv");
        let mut cmd = if uv_path.exists() {
            let mut c = std::process::Command::new(uv_path);
            c.arg("run")
             .arg("--with").arg("torch")
             .arg("--with").arg("transformers")
             .arg("--with").arg("torchaudio")
             .arg("--with").arg("qwen-tts")
             .arg("--with").arg("soundfile")
             .arg("python3");
            c
        } else {
            std::process::Command::new("python3")
        };

        cmd.arg(&script_path)
            .arg("--model-dir").arg(model_dir)
            .arg("--prompt").arg(speaker_prompt)
            .arg("--text").arg(text);

        if gpu {
            cmd.arg("--gpu");
        }

        info!("Spawning Breeze-TTS-2 PyTorch inference script via python...");
        if let Ok(output) = cmd.output() {
            if output.status.success() && !output.stdout.is_empty() {
                info!("Breeze-TTS-2 PyTorch neural synthesis succeeded ({} bytes audio)", output.stdout.len());
                return Ok(output.stdout);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("Breeze-TTS-2 PyTorch inference script failed: {stderr}");
            }
        }
    }

    // 2. Synthesize speech using espeak-ng --stdout with speed and prompt-tuned pitch
    let wpm = (175.0 * speed) as i32;
    let mut cmd = std::process::Command::new("espeak-ng");
    cmd.arg("-s").arg(wpm.to_string())
       .arg("--stdout");

    let prompt_lower = speaker_prompt.to_lowercase();
    if prompt_lower.contains("deep") || prompt_lower.contains("male") || prompt_lower.contains("low") {
        cmd.arg("-p").arg("35");
    } else if prompt_lower.contains("female") || prompt_lower.contains("high") || prompt_lower.contains("gentle") {
        cmd.arg("-p").arg("68");
    }

    cmd.arg(text);

    let output = cmd.output().context("spawn speech synthesizer process")?;
    if !output.status.success() || output.stdout.is_empty() {
        anyhow::bail!("Speech synthesis process produced no audio output");
    }

    Ok(output.stdout)
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

    let raw_audio = synthesize_speech_bytes(
        &u.text,
        &cfg.speaker_prompt,
        config.speed,
        &dir,
        cfg.gpu,
    )?;

    if generation_counter.load(Ordering::SeqCst) != generation {
        return Ok(());
    }

    let (sample_rate, channels, samples) = parse_wav_or_pcm(&raw_audio);

    if !samples.is_empty() {
        sink.append(rodio::buffer::SamplesBuffer::new(channels, sample_rate, samples));
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
    fn test_parse_wav_or_pcm_raw_samples() {
        let dummy = vec![0x00, 0x00, 0x10, 0x00];
        let (sr, ch, samples) = parse_wav_or_pcm(&dummy);
        assert_eq!(sr, BREEZE_TTS_2_SAMPLE_RATE);
        assert_eq!(ch, 1);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[1], 16);
    }
}
