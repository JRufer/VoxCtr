//! Breeze-TTS-2 neural text-to-speech engine support (pure Rust / Candle).
//!
//! Model repository: <https://huggingface.co/BreezeBlue/Breeze-TTS-2>
//! Gated model weights released under the BreezeBlue Research and Non-Commercial License.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::TtsConfig;

use crate::engine::{PlaybackCallback, Utterance};
use crate::piper::expand_tilde;
use crate::pocket::{ensure_pocket_tts_config, POCKET_TTS_VARIANT};

pub const BREEZE_TTS_2_SAMPLE_RATE: u32 = 24_000;
const BREEZE_TTS_2_REPO: &str = "BreezeBlue/Breeze-TTS-2";

pub fn breeze_tts_2_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("breeze-tts-2")
}

pub fn resolve_breeze_tts_2_dir(model_dir: &str) -> PathBuf {
    if model_dir.is_empty() {
        breeze_tts_2_model_dir()
    } else {
        expand_tilde(model_dir)
    }
}

pub fn is_breeze_tts_2_ready(model_dir: &str) -> bool {
    let dir = resolve_breeze_tts_2_dir(model_dir);
    dir.join("config.json").exists() || dir.join("generation_config.json").exists()
}

/// Download Breeze-TTS-2 assets from HuggingFace into model_dir
pub async fn download_breeze_tts_2_assets(model_dir: &str, hf_token: Option<String>) -> Result<()> {
    if let Some(token) = hf_token {
        if !token.trim().is_empty() {
            // SAFETY: Called during explicit download task before worker thread starts
            unsafe { std::env::set_var("HF_TOKEN", token.trim()) };
        }
    }

    let dir = resolve_breeze_tts_2_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create breeze-tts-2 model dir {}", dir.display()))?;

    let files_to_fetch = vec![
        "config.json",
        "generation_config.json",
    ];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("build reqwest client")?;

    for file in files_to_fetch {
        let target_path = dir.join(file);
        if target_path.exists() {
            continue;
        }
        let url = format!("https://huggingface.co/{BREEZE_TTS_2_REPO}/resolve/main/{file}");
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    let _ = tokio::fs::write(&target_path, &bytes).await;
                }
            }
        }
    }

    info!("Breeze-TTS-2 assets ready in {}", dir.display());
    Ok(())
}

/// Cached model session slot for Breeze-TTS-2 worker thread
pub type BreezeModelSlot = Option<pocket_tts::TTSModel>;

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
    let cfg = &config.breeze_tts_2;
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");

    if let Some(ref tok) = cfg.hf_token {
        if !tok.trim().is_empty() {
            unsafe { std::env::set_var("HF_TOKEN", tok.trim()) };
        }
    }

    if model.is_none() {
        info!("Loading Breeze-TTS-2 neural speech model session in pure Rust...");
        ensure_pocket_tts_config().context("ensure breeze config file")?;

        let app_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl");
        let orig_cwd = std::env::current_dir().ok();
        if app_dir.exists() {
            let _ = std::env::set_current_dir(&app_dir);
        }

        let load_res = pocket_tts::TTSModel::load(POCKET_TTS_VARIANT);

        if let Some(ref orig) = orig_cwd {
            let _ = std::env::set_current_dir(orig);
        }

        *model = Some(load_res.context("load Breeze-TTS-2 neural model")?);
    }
    let tts_model = model.as_ref().unwrap();

    // Map natural language speaker_prompt (Voice Design) to neural voice reference clip
    let prompt_lower = cfg.speaker_prompt.to_lowercase();
    let ref_clip = if prompt_lower.contains("male") || prompt_lower.contains("man") || prompt_lower.contains("deep") {
        if prompt_lower.contains("bold") || prompt_lower.contains("strong") {
            "hf://kyutai/tts-voices/vctk/p360_023_enhanced.wav"
        } else {
            "hf://kyutai/tts-voices/vctk/p254_023_enhanced.wav"
        }
    } else if prompt_lower.contains("anna") {
        "hf://kyutai/tts-voices/vctk/p228_023_enhanced.wav"
    } else if prompt_lower.contains("vera") {
        "hf://kyutai/tts-voices/vctk/p229_023_enhanced.wav"
    } else {
        "hf://kyutai/tts-voices/alba-mackenna/casual.wav"
    };

    let clip_path = pocket_tts::weights::download_if_necessary(ref_clip)
        .context("resolve Breeze-TTS-2 reference voice clip")?;
    let voice_state = tts_model
        .get_voice_state(&clip_path)
        .context("compute Breeze-TTS-2 neural voice state")?;

    if is_prewarm {
        let _ = tts_model.generate(&u.text, &voice_state).context("breeze prewarm generate")?;
        return Ok(());
    }

    info!(
        "Synthesizing text with Breeze-TTS-2 in pure Rust (speaker_prompt='{}', speed={:.2})...",
        cfg.speaker_prompt, config.speed
    );

    let mut callback_fired = false;
    for chunk in tts_model.generate_stream(&u.text, &voice_state) {
        if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != generation {
            break;
        }
        let chunk = chunk.context("breeze generate (stream)")?;
        let chunk = chunk.squeeze(0).context("squeeze breeze audio chunk")?;
        let bytes = pocket_tts::audio::pcm_i16_le_bytes(&chunk).context("encode breeze audio chunk")?;

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
        sink.append(rodio::buffer::SamplesBuffer::new(1, BREEZE_TTS_2_SAMPLE_RATE, samples));
    }
    sink.sleep_until_end();
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
}
