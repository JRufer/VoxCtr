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
use crate::pocket::{load_pocket_tts_model_on_gpu, POCKET_TTS_VARIANT};

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
/// The loaded model, plus the GPU setting it was loaded under — flipping that
/// setting has to reload, since the device is fixed when the weights are placed.
pub struct LoadedBreezeModel {
    gpu: bool,
    model: pocket_tts::TTSModel,
}

pub type BreezeModelSlot = Option<LoadedBreezeModel>;

fn prompt_contains_word(prompt_lower: &str, target_words: &[&str]) -> bool {
    let words: Vec<&str> = prompt_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    target_words.iter().any(|tw| words.contains(tw))
}

fn read_voice_transcript_file(wav_path_str: &str) -> Option<String> {
    let path = std::path::Path::new(wav_path_str);
    if !path.exists() {
        return None;
    }
    let txt_path = path.with_extension("txt");
    if txt_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&txt_path) {
            let trimmed = content.trim().to_string();
            if !trimmed.is_empty() {
                info!("Loaded voice transcript for {:?}: '{}'", txt_path.file_name(), trimmed);
                return Some(trimmed);
            }
        }
    }
    None
}

/// Called from TtsEngineWorker::run when config.engine == TtsEngine::BreezeTts2
#[allow(clippy::too_many_arguments)]
pub(crate) fn speak_breeze_tts_2(
    config: &TtsConfig,
    u: &Utterance,
    model: &mut BreezeModelSlot,
    voice_states: &mut std::collections::HashMap<String, pocket_tts::ModelState>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
) -> Result<()> {
    let cfg = &config.breeze_tts_2;
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");

    if let Some(ref tok) = config.hf_token {
        if !tok.trim().is_empty() {
            unsafe { std::env::set_var("HF_TOKEN", tok.trim()) };
        }
    }

    if model.as_ref().is_none_or(|m| m.gpu != cfg.gpu) {
        info!("Loading Breeze-TTS-2 neural speech model session in pure Rust...");
        *model = Some(LoadedBreezeModel {
            gpu: cfg.gpu,
            model: load_pocket_tts_model_on_gpu(POCKET_TTS_VARIANT, cfg.gpu)
                .context("load Breeze-TTS-2 neural model")?,
        });
    }
    let tts_model = &model.as_ref().unwrap().model;

    // Determine reference audio clip and transcript based on voice_mode
    let is_clone_mode = cfg.voice_mode == "clone" || (!cfg.cloned_voice.trim().is_empty() && cfg.voice_mode != "prompt");

    let (ref_clip_string, _transcript) = if is_clone_mode {
        let voice_id = if cfg.cloned_voice.trim().is_empty() { "alba" } else { cfg.cloned_voice.trim() };
        let resolved = crate::pocket::resolve_pocket_tts_voice_clip(voice_id, &cfg.voice_dir);
        let wav_path = resolved.unwrap_or_else(|| "hf://kyutai/tts-voices/alba-mackenna/casual.wav".to_string());
        let transcript = read_voice_transcript_file(&wav_path);
        (wav_path, transcript)
    } else {
        // Map natural language speaker_prompt (Voice Design) to neural voice reference clip
        let prompt_lower = cfg.speaker_prompt.to_lowercase();
        let prompt_trim = cfg.speaker_prompt.trim();

        let female_pool = [
            "hf://kyutai/tts-voices/vctk/p228_023_enhanced.wav", // Anna (Clear/High)
            "hf://kyutai/tts-voices/vctk/p229_023_enhanced.wav", // Vera (Soft/Warm)
            "hf://kyutai/tts-voices/vctk/p230_023_enhanced.wav", // Tense/Intense
            "hf://kyutai/tts-voices/vctk/p231_023_enhanced.wav", // Bright/Young
            "hf://kyutai/tts-voices/vctk/p234_023_enhanced.wav", // Expressive
            "hf://kyutai/tts-voices/vctk/p236_023_enhanced.wav", // Resonant
            "hf://kyutai/tts-voices/alba-mackenna/casual.wav",   // Alba (Casual)
        ];

        let male_pool = [
            "hf://kyutai/tts-voices/vctk/p254_023_enhanced.wav", // Charles (Standard Male)
            "hf://kyutai/tts-voices/vctk/p360_023_enhanced.wav", // Michael (Deep/Bass Male)
            "hf://kyutai/tts-voices/vctk/p226_023_enhanced.wav", // Clear Male
            "hf://kyutai/tts-voices/vctk/p227_023_enhanced.wav", // Warm Male
            "hf://kyutai/tts-voices/vctk/p232_023_enhanced.wav", // Rich Male
        ];

        let female_keywords = [
            "female", "woman", "women", "girl", "lady", "she", "her", "alba", "anna", "vera", 
            "high", "soft", "gentle", "sweet", "cute", "bright", "cheerful", "calm", "smooth", 
            "friendly", "young", "female1", "female2"
        ];

        let is_female = prompt_contains_word(&prompt_lower, &female_keywords)
            || prompt_lower.contains("female")
            || prompt_lower.contains("woman")
            || prompt_lower.contains("girl")
            || prompt_lower.contains("lady")
            || prompt_lower.contains("soft")
            || prompt_lower.contains("gentle")
            || prompt_lower.contains("sweet");

        let is_male = !is_female && (
            prompt_contains_word(&prompt_lower, &["male", "man", "men", "guy", "boy", "he", "him", "his", "deep", "baritone", "bass", "bold", "narrator", "radio", "announcer", "strong", "charles", "michael"])
            || prompt_lower.contains("male")
            || prompt_lower.contains("man")
            || prompt_lower.contains("deep")
            || prompt_lower.contains("narrator")
        );

        let prompt_hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            prompt_trim.hash(&mut h);
            h.finish() as usize
        };

        let clip = if prompt_trim.starts_with("hf://") || prompt_trim.ends_with(".wav") || prompt_trim.contains('/') {
            prompt_trim.to_string()
        } else if is_female {
            if prompt_lower.contains("anna") {
                female_pool[0].to_string()
            } else if prompt_lower.contains("vera") {
                female_pool[1].to_string()
            } else if prompt_lower.contains("alba") {
                female_pool[6].to_string()
            } else {
                female_pool[prompt_hash % female_pool.len()].to_string()
            }
        } else if is_male {
            if prompt_lower.contains("michael") || prompt_lower.contains("deep") || prompt_lower.contains("bass") {
                male_pool[1].to_string()
            } else if prompt_lower.contains("charles") {
                male_pool[0].to_string()
            } else {
                male_pool[prompt_hash % male_pool.len()].to_string()
            }
        } else {
            female_pool[prompt_hash % female_pool.len()].to_string()
        };
        (clip, None)
    };
    let ref_clip = &ref_clip_string;

    if !voice_states.contains_key(ref_clip) {
        info!("Computing Breeze-TTS-2 voice state embedding for {ref_clip}...");
        let clip_path = pocket_tts::weights::download_if_necessary(ref_clip)
            .context("resolve Breeze-TTS-2 reference voice clip")?;
        let state = tts_model
            .get_voice_state(&clip_path)
            .context("compute Breeze-TTS-2 neural voice state")?;
        voice_states.insert(ref_clip.to_string(), state);
    }
    let voice_state = voice_states.get(ref_clip).unwrap();

    if is_prewarm {
        info!("Breeze-TTS-2 model and voice state prewarmed instantly.");
        return Ok(());
    }

    info!(
        "Synthesizing text with Breeze-TTS-2 in pure Rust (prompt='{}', ref_clip='{}', speed={:.2})...",
        cfg.speaker_prompt.trim(), ref_clip, config.speed
    );

    let mut callback_fired = false;
    for chunk in tts_model.generate_stream(&u.text, voice_state) {
        if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != generation {
            break;
        }
        let chunk = chunk.context("breeze generate (stream)")?;
        let chunk = chunk.squeeze(0).context("squeeze breeze audio chunk")?;
        let bytes = pocket_tts::audio::pcm_i16_le_bytes(&chunk).context("encode breeze audio chunk")?;

        let samples: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        if !samples.is_empty() {
            sink.append(rodio::buffer::SamplesBuffer::new(1, BREEZE_TTS_2_SAMPLE_RATE, samples));

            if !callback_fired {
                callback_fired = true;
                sink.play();
                if let Some(ref cb) = on_playback_start {
                    cb();
                }
            }
        }
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
}
