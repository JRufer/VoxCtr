//! VoxCPM2 neural text-to-speech — pure Rust, in process, no Python.
//!
//! Upstream model: <https://github.com/OpenBMB/VoxCPM> (Apache-2.0), a 2B
//! tokenizer-free diffusion-autoregressive speech model. The reference
//! implementation is PyTorch, but VoxCtrl runs it through
//! [`voxcpm-rs`](https://crates.io/crates/voxcpm-rs), a pure-Rust port on the
//! [Burn](https://burn.dev) framework — the same shape of dependency as the
//! `pocket-tts` crate behind Pocket-TTS and Breeze-TTS-2. There is no
//! subprocess, no ONNX Runtime and no Python interpreter anywhere in this path.
//!
//! ## Two ways to choose a voice
//!
//! Both are the ones Breeze-TTS-2 offers, and the settings UI mirrors it:
//!
//! - **Voice design** — a natural-language description of the speaker. VoxCPM2
//!   takes this as a `(description)` prefix on the text itself, so no reference
//!   audio is needed at all.
//! - **Voice cloning** — a reference `.wav` clip, read from the same shared
//!   voice folder Pocket-TTS and Breeze-TTS-2 use, so a clip dropped in once is
//!   available to every engine. A description can be layered on top to control
//!   delivery without changing the cloned identity.
//!
//! ## Where the latency budget goes
//!
//! The target is first audio in under a second. Four things get us there, in
//! descending order of how much they matter:
//!
//! 1. **The model stays resident.** Loading a 4 GB checkpoint costs 20-25 s, so
//!    it happens once per worker thread and never again — the same caching the
//!    other neural engines do. With `prewarm` on it happens at startup, before
//!    anyone asks for speech.
//! 2. **Generation is streamed, not awaited.** [`VoxCPM::generate_stream`]
//!    yields audio while the rest is still being generated, so what the user
//!    waits for is the first chunk, not the whole utterance.
//! 3. **`chunk_patches` sets the size of that first chunk.** Each patch is
//!    ~80 ms of audio and costs one autoregressive step, so this is the direct
//!    time-to-first-audio knob. VoxCtrl defaults to 2 rather than the crate's 5.
//! 4. **`inference_timesteps` sets the cost of every step.** Diffusion steps are
//!    linear in wall time; VoxCtrl defaults to 6, the floor of the range that
//!    keeps quality intact.
//!
//! On top of that, reference clips are decoded once and cached as raw PCM, so
//! cloning does not re-read and re-resample a file per utterance.
//!
//! Without the `voxcpm2` cargo feature this module still compiles: the
//! catalogue, readiness checks and downloader all work (so the UI behaves), and
//! only synthesis reports that the engine was not built in.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::info;

use crate::piper::expand_tilde;

/// Default HuggingFace repository for the weights. Apache-2.0 and ungated, so
/// unlike Breeze-TTS-2 and Pocket-TTS no access token is needed.
pub const VOXCPM2_DEFAULT_REPO: &str = "openbmb/VoxCPM2";

/// Whether this build can actually synthesize with VoxCPM2.
///
/// The model files download and the settings UI works either way; this reports
/// whether the inference crate was compiled in, so the UI can say up front that
/// the engine will not speak rather than letting the first utterance fail.
pub const VOXCPM2_COMPILED: bool = cfg!(feature = "voxcpm2");

/// Human-readable name of the compiled-in compute backend, for the settings UI.
pub fn voxcpm2_backend_name() -> &'static str {
    if cfg!(feature = "voxcpm2-wgpu") {
        "GPU (wgpu: Vulkan / Metal / DX12)"
    } else if cfg!(feature = "voxcpm2") {
        "CPU (ndarray)"
    } else {
        "not compiled in"
    }
}

pub fn voxcpm2_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("voxcpm2")
}

pub fn resolve_voxcpm2_dir(model_dir: &str) -> PathBuf {
    if model_dir.trim().is_empty() {
        voxcpm2_model_dir()
    } else {
        expand_tilde(model_dir)
    }
}

/// The checkpoint files `VoxCPM::from_local` needs, each as a list of accepted
/// filenames. The upstream repo ships `model.safetensors` + `audiovae.pth`;
/// both of those and their alternates load without a conversion step.
const REQUIRED_FILES: &[&[&str]] = &[
    &["config.json"],
    &["tokenizer.json"],
    &["model.safetensors", "model.pth", "model.pt"],
    &["audiovae.safetensors", "audiovae.pth"],
];

/// Files to fetch for a fresh install, in the order they are downloaded. The
/// two small JSON files come first so an interrupted download leaves the
/// cheap-to-refetch parts done.
const DOWNLOAD_FILES: &[&str] = &[
    "config.json",
    "tokenizer.json",
    "audiovae.pth",
    "model.safetensors",
];

/// Network-free check for a usable local checkpoint.
pub fn is_voxcpm2_ready(model_dir: &str) -> bool {
    voxcpm2_missing_files(model_dir).is_empty()
}

/// Which required files are absent, named as the UI should show them. Returning
/// the list rather than a bare bool means a half-finished download can say what
/// is still missing instead of just reporting "not ready".
pub fn voxcpm2_missing_files(model_dir: &str) -> Vec<String> {
    let dir = resolve_voxcpm2_dir(model_dir);
    REQUIRED_FILES
        .iter()
        .filter(|alternates| !alternates.iter().any(|name| dir.join(name).exists()))
        .map(|alternates| alternates.join(" or "))
        .collect()
}

/// Download the checkpoint into `model_dir`.
///
/// The main weights file is several gigabytes, so it is streamed to disk rather
/// than buffered in memory, and written to a `.part` file that is renamed only
/// on success — an interrupted download then leaves no half-file that the
/// readiness check would mistake for a complete one.
pub async fn download_voxcpm2_assets(
    model_dir: &str,
    repo: &str,
    hf_token: Option<String>,
) -> Result<()> {
    let repo = if repo.trim().is_empty() {
        VOXCPM2_DEFAULT_REPO
    } else {
        repo.trim()
    };
    let dir = resolve_voxcpm2_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create voxcpm2 model dir {}", dir.display()))?;

    let client = reqwest::Client::builder()
        // No total-request timeout: the weights file is multi-gigabyte and a
        // wall-clock cap would abort a perfectly healthy slow download. A
        // connect timeout still catches an unreachable host quickly.
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build reqwest client")?;

    for file in DOWNLOAD_FILES {
        let target = dir.join(file);
        if target.exists() {
            info!("VoxCPM2 asset already present, skipping: {file}");
            continue;
        }

        let url = format!("https://huggingface.co/{repo}/resolve/main/{file}");
        let mut request = client.get(&url);
        if let Some(ref token) = hf_token {
            if !token.trim().is_empty() {
                request = request.bearer_auth(token.trim());
            }
        }

        info!("Downloading VoxCPM2 asset {file} from {repo}...");
        let resp = request
            .send()
            .await
            .with_context(|| format!("request VoxCPM2 asset {file}"))?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "downloading {file} from {repo} failed with HTTP {}",
                resp.status()
            );
        }

        let part = target.with_extension("part");
        stream_to_file(resp, &part, file).await?;
        tokio::fs::rename(&part, &target)
            .await
            .with_context(|| format!("finalize {}", target.display()))?;
    }

    let missing = voxcpm2_missing_files(model_dir);
    if !missing.is_empty() {
        anyhow::bail!(
            "VoxCPM2 download finished but these files are still missing: {}",
            missing.join(", ")
        );
    }

    info!("VoxCPM2 checkpoint ready in {}", dir.display());
    Ok(())
}

async fn stream_to_file(
    mut resp: reqwest::Response,
    path: &std::path::Path,
    label: &str,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let total = resp.content_length();
    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("create {}", path.display()))?;
    let mut written: u64 = 0;
    let mut next_log_at: u64 = 256 * 1024 * 1024;

    // `Response::chunk()` rather than `bytes_stream()`: it needs no extra
    // futures crate, and the loop reads the same way.
    while let Some(chunk) = resp
        .chunk()
        .await
        .with_context(|| format!("read {label} response body"))?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", path.display()))?;
        written += chunk.len() as u64;
        if written >= next_log_at {
            match total {
                Some(total) if total > 0 => info!(
                    "VoxCPM2 {label}: {} MiB of {} MiB",
                    written / (1024 * 1024),
                    total / (1024 * 1024)
                ),
                _ => info!("VoxCPM2 {label}: {} MiB", written / (1024 * 1024)),
            }
            next_log_at += 256 * 1024 * 1024;
        }
    }
    file.flush().await.context("flush download")?;
    Ok(())
}

/// Compose the text VoxCPM2 actually sees.
///
/// Voice design and style-controlled cloning share one surface in this model: a
/// parenthetical description in front of the text. Kept separate from synthesis
/// so it can be tested without a checkpoint.
#[cfg_attr(not(feature = "voxcpm2"), allow(dead_code))]
pub(crate) fn compose_prompted_text(description: &str, text: &str) -> String {
    let description = description.trim().trim_matches(['(', ')']).trim();
    let text = text.trim();
    if description.is_empty() {
        return text.to_string();
    }
    format!("({description}){text}")
}

/// The description to apply for a given config, or empty for none.
///
/// In design mode the description *is* the voice. In clone mode the clip
/// carries the voice and the optional style prompt only shapes delivery.
#[cfg_attr(not(feature = "voxcpm2"), allow(dead_code))]
pub(crate) fn description_for(cfg: &voxctrl_config::VoxCpm2Config) -> &str {
    if cfg.voice_mode == "clone" {
        cfg.style_prompt.as_str()
    } else {
        cfg.design_prompt.as_str()
    }
}

/// Read the transcript sitting next to a reference clip, if there is one.
///
/// A transcript upgrades cloning from "imitate this speaker" to "continue this
/// recording", which tracks the reference voice noticeably more closely. It is
/// optional, so a missing or empty file is not an error.
#[cfg_attr(not(feature = "voxcpm2"), allow(dead_code))]
pub(crate) fn read_clip_transcript(clip_path: &str) -> Option<String> {
    let txt_path = std::path::Path::new(clip_path).with_extension("txt");
    let content = std::fs::read_to_string(&txt_path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    info!("Using transcript {} for VoxCPM2 cloning", txt_path.display());
    Some(trimmed)
}

// ── Synthesis ─────────────────────────────────────────────────────────────────

#[cfg(feature = "voxcpm2")]
mod inference {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use anyhow::{Context, Result};
    use tracing::{info, warn};
    use voxcpm_rs::{CancelToken, GenerateOptions, Prompt, PromptAudio, VoxCPM};

    use crate::engine::{PlaybackCallback, Utterance};

    // One backend is compiled in. wgpu covers Vulkan, Metal and DX12 and is the
    // only configuration that reaches the sub-second target on a 2B model; the
    // ndarray CPU path is the portable fallback and is far slower than realtime.
    #[cfg(feature = "voxcpm2-wgpu")]
    pub type Backend = burn::backend::Wgpu<f32, i32>;
    #[cfg(not(feature = "voxcpm2-wgpu"))]
    pub type Backend = burn::backend::NdArray<f32>;

    /// A loaded model plus the caches that keep repeat utterances fast.
    pub struct VoxCpmSession {
        model: VoxCPM<Backend>,
        sample_rate: u32,
        /// Reference clips decoded to mono PCM, keyed by resolved clip path.
        /// Decoding and resampling a wav costs tens of milliseconds that would
        /// otherwise be paid on every single cloned utterance.
        reference_cache: HashMap<String, (Vec<f32>, u32)>,
    }

    pub fn load_session(model_dir: &str) -> Result<VoxCpmSession> {
        let dir = super::resolve_voxcpm2_dir(model_dir);
        let missing = super::voxcpm2_missing_files(model_dir);
        if !missing.is_empty() {
            anyhow::bail!(
                "VoxCPM2 checkpoint incomplete in {} — missing: {}. Download it from TTS settings.",
                dir.display(),
                missing.join(", ")
            );
        }

        info!(
            "Loading VoxCPM2 checkpoint from {} on {} (first load takes 20-25 s)...",
            dir.display(),
            super::voxcpm2_backend_name()
        );
        let started = std::time::Instant::now();
        let device = <Backend as burn::tensor::backend::Backend>::Device::default();
        let model = VoxCPM::<Backend>::from_local(&dir, &device)
            .map_err(|e| anyhow::anyhow!("load VoxCPM2 checkpoint: {e}"))?;
        let sample_rate = model.sample_rate();
        info!(
            "VoxCPM2 loaded in {:.1} s ({} Hz output)",
            started.elapsed().as_secs_f32(),
            sample_rate
        );

        Ok(VoxCpmSession {
            model,
            sample_rate,
            reference_cache: HashMap::new(),
        })
    }

    /// Resolve the configured voice to a reference clip path, or `None` in
    /// design mode.
    ///
    /// Clone mode reads from the voice folder shared with Pocket-TTS and
    /// Breeze-TTS-2, so a clip added for one engine works in all of them.
    fn resolve_reference_clip(cfg: &voxctrl_config::VoxCpm2Config) -> Result<Option<String>> {
        if cfg.voice_mode != "clone" {
            return Ok(None);
        }
        let voice_id = cfg.cloned_voice.trim();
        if voice_id.is_empty() {
            anyhow::bail!(
                "VoxCPM2 is set to voice cloning but no reference clip is selected. \
                 Pick one in TTS settings, or switch to voice design."
            );
        }

        let resolved = crate::pocket::resolve_pocket_tts_voice_clip(voice_id, &cfg.voice_dir)
            .ok_or_else(|| {
                anyhow::anyhow!("unknown VoxCPM2 reference voice: {voice_id}")
            })?;

        // Built-in catalogue entries are `hf://` URIs; resolve them to a real
        // file (cached after the first fetch). A user clip is already a path.
        if resolved.starts_with("hf://") {
            let path = pocket_tts::weights::download_if_necessary(&resolved)
                .context("resolve VoxCPM2 reference voice clip")?;
            Ok(Some(path.to_string_lossy().into_owned()))
        } else {
            Ok(Some(resolved))
        }
    }

    fn prompt_for(session: &mut VoxCpmSession, clip: Option<String>) -> Result<Prompt> {
        let Some(clip) = clip else {
            return Ok(Prompt::None);
        };

        if !session.reference_cache.contains_key(&clip) {
            info!("Decoding VoxCPM2 reference clip {clip} (cached for later utterances)");
            let decoded = voxcpm_rs::audio::load_audio(&clip)
                .map_err(|e| anyhow::anyhow!("decode reference clip {clip}: {e}"))?;
            session.reference_cache.insert(clip.clone(), decoded);
        }
        let (samples, sample_rate) = session.reference_cache.get(&clip).unwrap().clone();
        let audio = PromptAudio::Pcm {
            samples,
            sample_rate,
        };

        // With a transcript the model continues the reference recording rather
        // than merely imitating it, which holds the speaker identity better.
        Ok(match super::read_clip_transcript(&clip) {
            Some(prompt_text) => Prompt::Combined {
                reference_audio: audio.clone(),
                prompt_audio: audio,
                prompt_text,
            },
            None => Prompt::Reference { audio },
        })
    }

    fn build_options(
        cfg: &voxctrl_config::VoxCpm2Config,
        prompt: Prompt,
        cancel: CancelToken,
    ) -> GenerateOptions {
        GenerateOptions::builder()
            .cfg(cfg.cfg_value.clamp(1.0, 5.0))
            // Below ~6 steps quality degrades audibly; above 10 costs latency
            // for no gain on short utterances.
            .timesteps((cfg.inference_timesteps.clamp(4, 32)) as usize)
            // Each patch is ~80 ms of audio and one autoregressive step, so this
            // is what the user actually waits for before the first sound.
            .chunk_patches((cfg.chunk_patches.clamp(1, 32)) as usize)
            // Bounds a runaway generation: without it a pathological input can
            // keep the stop head from ever firing and speak indefinitely.
            .max_len((cfg.max_len.max(1)) as usize)
            .prompt(prompt)
            .cancel(cancel)
            .build()
    }

    /// Cancel `token` as soon as the worker's generation counter moves past
    /// `generation`, and stop watching when the returned guard is dropped.
    ///
    /// The crate polls the token between diffusion steps, which is finer-grained
    /// than our own between-chunk check — pressing stop cuts the current step
    /// short instead of waiting out the whole chunk. That needs a second thread,
    /// because the generating thread is busy inside the model.
    struct CancelWatcher {
        done: Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl CancelWatcher {
        fn spawn(
            token: CancelToken,
            generation_counter: Arc<AtomicU32>,
            generation: u32,
        ) -> Self {
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let thread_done = done.clone();
            let handle = std::thread::Builder::new()
                .name("voxctrl-voxcpm-cancel".into())
                .spawn(move || {
                    while !thread_done.load(Ordering::SeqCst) {
                        if generation_counter.load(Ordering::SeqCst) != generation {
                            token.cancel();
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                })
                .ok();
            Self { done, handle }
        }
    }

    impl Drop for CancelWatcher {
        fn drop(&mut self) {
            self.done.store(true, Ordering::SeqCst);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn speak(
        config: &voxctrl_config::TtsConfig,
        u: &Utterance,
        session_slot: &mut Option<VoxCpmSession>,
        on_playback_start: &Option<PlaybackCallback>,
        sink: &rodio::Sink,
        generation_counter: &Arc<AtomicU32>,
        generation: u32,
    ) -> Result<()> {
        let cfg = &config.voxcpm2;
        let is_prewarm = u.source_label.as_deref() == Some("prewarm");

        if session_slot.is_none() {
            *session_slot = Some(load_session(&cfg.model_dir)?);
        }

        let clip = resolve_reference_clip(cfg)?;
        let session = session_slot.as_mut().unwrap();
        let sample_rate = session.sample_rate;
        let prompt = prompt_for(session, clip)?;

        // A prewarm pass runs a real (short) generation rather than only loading
        // the weights: the first generation also pays for shader compilation and
        // allocator growth on the GPU backends, and doing that here is what makes
        // the first *spoken* utterance fast instead of merely the second one.
        let text = if is_prewarm {
            super::compose_prompted_text(super::description_for(cfg), "Ready.")
        } else {
            super::compose_prompted_text(super::description_for(cfg), &u.text)
        };
        if text.is_empty() {
            return Ok(());
        }

        let cancel = CancelToken::new();
        let _watcher = CancelWatcher::spawn(
            cancel.clone(),
            generation_counter.clone(),
            generation,
        );
        let mut opts = build_options(cfg, prompt, cancel);
        if is_prewarm {
            // Warm the pipeline, not the whole sentence: one chunk of patches is
            // enough to compile every shader the real path uses.
            opts.max_len = opts.chunk_patches;
        }

        if !is_prewarm {
            info!(
                "Synthesizing with VoxCPM2 (mode={}, timesteps={}, chunk_patches={}, cfg={:.2})",
                cfg.voice_mode, opts.inference_timesteps, opts.chunk_patches, opts.cfg_value
            );
        }

        let started = std::time::Instant::now();
        let stream = session
            .model
            .generate_stream(&text, opts)
            .map_err(|e| anyhow::anyhow!("VoxCPM2 generation failed to start: {e}"))?;

        let mut first_chunk_logged = false;
        let mut callback_fired = false;
        for chunk in stream {
            if generation_counter.load(Ordering::SeqCst) != generation {
                break; // stop() was called — abandon the rest of the generation
            }
            let chunk = match chunk {
                Ok(chunk) => chunk,
                // Cancellation is the expected end of a stopped utterance, not a
                // failure to report to the user.
                Err(voxcpm_rs::Error::Cancelled) => break,
                Err(e) => return Err(anyhow::anyhow!("VoxCPM2 generation failed: {e}")),
            };
            if chunk.is_empty() {
                continue;
            }

            if !first_chunk_logged {
                first_chunk_logged = true;
                if !is_prewarm {
                    info!(
                        "VoxCPM2 first audio chunk after {} ms",
                        started.elapsed().as_millis()
                    );
                }
            }

            if is_prewarm {
                continue; // warmed, but nothing to play
            }

            // f32 in [-1, 1] to the i16 samples rodio's buffer takes.
            let samples: Vec<i16> = chunk
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect();
            sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));

            if !callback_fired {
                callback_fired = true;
                sink.play();
                if let Some(ref cb) = on_playback_start {
                    cb();
                }
            }
        }

        if is_prewarm {
            info!(
                "VoxCPM2 prewarmed in {:.1} s — the first utterance skips model load and shader compilation",
                started.elapsed().as_secs_f32()
            );
            return Ok(());
        }

        if !callback_fired {
            warn!("VoxCPM2 produced no audio for this utterance");
            return Ok(());
        }

        // Generation finishes well before playback does whenever the model runs
        // faster than realtime, which is the whole point of the GPU backend.
        // Returning here would fire the playback-end callback while audio is
        // still coming out of the speakers, so the UI would clear "Speaking..."
        // early and the stop key would have nothing left to stop.
        if generation_counter.load(Ordering::SeqCst) == generation {
            sink.sleep_until_end();
        }
        Ok(())
    }
}

#[cfg(feature = "voxcpm2")]
pub(crate) use inference::speak as speak_voxcpm2_impl;

#[cfg(feature = "voxcpm2")]
pub(crate) type VoxCpmModelSlot = Option<inference::VoxCpmSession>;
#[cfg(not(feature = "voxcpm2"))]
pub(crate) type VoxCpmModelSlot = Option<()>;

/// Called from `TtsEngineWorker::run` when `config.engine == TtsEngine::VoxCpm2`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn speak_voxcpm2(
    config: &voxctrl_config::TtsConfig,
    u: &crate::engine::Utterance,
    model: &mut VoxCpmModelSlot,
    on_playback_start: &Option<crate::engine::PlaybackCallback>,
    sink: &rodio::Sink,
    generation_counter: &std::sync::Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
) -> Result<()> {
    #[cfg(feature = "voxcpm2")]
    {
        speak_voxcpm2_impl(
            config,
            u,
            model,
            on_playback_start,
            sink,
            generation_counter,
            generation,
        )
    }
    #[cfg(not(feature = "voxcpm2"))]
    {
        let _ = (config, u, model, on_playback_start, sink, generation_counter, generation);
        anyhow::bail!(
            "This build was compiled without the `voxcpm2` feature, so the VoxCPM2 \
             engine cannot synthesize. Rebuild with `--features voxcpm2-wgpu` (GPU) \
             or `--features voxcpm2-cpu`, or choose another TTS engine."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use voxctrl_config::VoxCpm2Config;

    #[test]
    fn test_voxcpm2_model_dir_ends_with_engine_name() {
        assert!(voxcpm2_model_dir().ends_with("voxcpm2"));
    }

    #[test]
    fn test_resolve_voxcpm2_dir() {
        assert_eq!(resolve_voxcpm2_dir(""), voxcpm2_model_dir());
        assert_eq!(resolve_voxcpm2_dir("   "), voxcpm2_model_dir());
        assert_eq!(
            resolve_voxcpm2_dir("/tmp/voxcpm2"),
            PathBuf::from("/tmp/voxcpm2")
        );
    }

    #[test]
    fn test_not_ready_when_dir_is_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(!is_voxcpm2_ready(path));
        // Every required file is reported, so the UI can say what is missing.
        assert_eq!(voxcpm2_missing_files(path).len(), REQUIRED_FILES.len());
    }

    #[test]
    fn test_ready_only_once_every_required_file_exists() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();

        std::fs::write(dir.path().join("config.json"), b"{}").unwrap();
        std::fs::write(dir.path().join("tokenizer.json"), b"{}").unwrap();
        assert!(!is_voxcpm2_ready(path), "weights are still missing");

        std::fs::write(dir.path().join("model.safetensors"), b"w").unwrap();
        assert!(!is_voxcpm2_ready(path), "audiovae is still missing");

        std::fs::write(dir.path().join("audiovae.pth"), b"v").unwrap();
        assert!(is_voxcpm2_ready(path));
        assert!(voxcpm2_missing_files(path).is_empty());
    }

    #[test]
    fn test_weight_file_alternates_are_accepted() {
        // The upstream repo ships model.safetensors + audiovae.pth, but the
        // loader takes .pth/.pt for the backbone and .safetensors for the VAE
        // too. A checkpoint in either shape must count as ready.
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        for name in ["config.json", "tokenizer.json", "model.pth", "audiovae.safetensors"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        assert!(is_voxcpm2_ready(path));
    }

    #[test]
    fn test_partial_download_is_not_ready() {
        // A `.part` file is an in-flight download and must never satisfy the
        // readiness check, or the app would try to load a truncated checkpoint.
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        for name in ["config.json", "tokenizer.json", "audiovae.pth"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::write(dir.path().join("model.safetensors.part"), b"half").unwrap();
        assert!(!is_voxcpm2_ready(path));
        assert_eq!(
            voxcpm2_missing_files(path),
            vec!["model.safetensors or model.pth or model.pt".to_string()]
        );
    }

    // ── Voice design / style prompt composition ──────────────────────────────

    #[test]
    fn test_compose_prompted_text_prefixes_description() {
        assert_eq!(
            compose_prompted_text("A calm female voice", "Hello there."),
            "(A calm female voice)Hello there."
        );
    }

    #[test]
    fn test_compose_prompted_text_without_description_is_plain_text() {
        assert_eq!(compose_prompted_text("", "Hello there."), "Hello there.");
        assert_eq!(compose_prompted_text("   ", "Hello there."), "Hello there.");
    }

    #[test]
    fn test_compose_prompted_text_does_not_double_wrap_parentheses() {
        // Users copying an example from the model card often include the
        // brackets themselves; wrapping again would put them in the speech.
        assert_eq!(
            compose_prompted_text("(A deep narrator)", "Hi."),
            "(A deep narrator)Hi."
        );
    }

    #[test]
    fn test_description_for_design_mode_uses_design_prompt() {
        let cfg = VoxCpm2Config {
            voice_mode: "design".into(),
            design_prompt: "A bright young voice".into(),
            style_prompt: "ignored in design mode".into(),
            ..Default::default()
        };
        assert_eq!(description_for(&cfg), "A bright young voice");
    }

    #[test]
    fn test_description_for_clone_mode_uses_style_prompt() {
        // In clone mode the clip carries the identity, so the design prompt must
        // not leak in and fight it — only the style prompt applies.
        let cfg = VoxCpm2Config {
            voice_mode: "clone".into(),
            design_prompt: "A bright young voice".into(),
            style_prompt: "cheerful, slightly faster".into(),
            ..Default::default()
        };
        assert_eq!(description_for(&cfg), "cheerful, slightly faster");
    }

    #[test]
    fn test_clone_mode_without_style_prompt_has_no_description() {
        let cfg = VoxCpm2Config {
            voice_mode: "clone".into(),
            ..Default::default()
        };
        assert_eq!(description_for(&cfg), "");
    }

    // ── Reference clip transcripts ───────────────────────────────────────────

    #[test]
    fn test_read_clip_transcript_reads_sibling_txt() {
        let dir = tempdir().unwrap();
        let clip = dir.path().join("myvoice.wav");
        std::fs::write(&clip, b"fake audio").unwrap();
        std::fs::write(dir.path().join("myvoice.txt"), "  This is the transcript.  ").unwrap();
        assert_eq!(
            read_clip_transcript(clip.to_str().unwrap()).as_deref(),
            Some("This is the transcript.")
        );
    }

    #[test]
    fn test_read_clip_transcript_absent_or_empty_is_none() {
        let dir = tempdir().unwrap();
        let clip = dir.path().join("myvoice.wav");
        std::fs::write(&clip, b"fake audio").unwrap();
        assert!(read_clip_transcript(clip.to_str().unwrap()).is_none());

        std::fs::write(dir.path().join("myvoice.txt"), "   \n  ").unwrap();
        assert!(read_clip_transcript(clip.to_str().unwrap()).is_none());
    }

    #[test]
    fn test_backend_name_reports_whether_engine_is_compiled_in() {
        let name = voxcpm2_backend_name();
        assert_eq!(name == "not compiled in", !VOXCPM2_COMPILED);
    }
}
