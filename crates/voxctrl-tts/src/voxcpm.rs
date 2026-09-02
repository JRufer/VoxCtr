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
//! ## Latency, and why playback does not start on the first chunk
//!
//! The target is first audio in under a second *and* no gaps once it starts.
//! Those pull against each other: the audio device consumes sound in real time
//! and does not wait, so if the next chunk is not ready when the last one runs
//! out, rodio splices silence into the middle of the sentence. Playing every
//! chunk the moment it appears is therefore only smooth when generation is
//! comfortably faster than playback, and choppy everywhere else.
//!
//! So the engine banks a lead first, sized from a measured generation rate:
//!
//! 1. **The model stays resident.** Loading a 4 GB checkpoint costs 20-25 s, so
//!    it happens once per worker thread and never again. With `prewarm` on it
//!    happens at startup, before anyone asks for speech.
//! 2. **A lead buffer, not the first chunk.** [`required_lead_secs`] turns the
//!    observed real-time factor into the audio that must be banked: the
//!    configured minimum when generation outruns playback, and enough to cover
//!    `(rtf - 1) * remaining` when it does not.
//! 3. **One continuous source per utterance.** [`StreamingPcm`] feeds the sink a
//!    single stream instead of a buffer per chunk, so there is no per-chunk
//!    queue boundary to hear and a starved sink degrades to brief silence
//!    rather than a queue transition.
//! 4. **`inference_timesteps` and `chunk_patches` set the generation rate.**
//!    Diffusion steps are linear in wall time; larger chunks cut the AudioVAE's
//!    repeated decode work, which goes as `O(N^2 / chunk_patches)`. Both make
//!    the real-time factor smaller, which is what lets the lead stay short.
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

/// Rough spoken duration of `text`, in seconds.
///
/// Used to size the lead buffer when generation is slower than realtime, where
/// the shortfall to cover is proportional to how much audio is still to come.
/// English runs about 14 characters a second; the estimate only has to be the
/// right order of magnitude, and erring long only costs a little extra lead.
#[cfg_attr(not(feature = "voxcpm2"), allow(dead_code))]
pub(crate) fn estimate_speech_secs(text: &str) -> f32 {
    const CHARS_PER_SEC: f32 = 14.0;
    (text.chars().count() as f32 / CHARS_PER_SEC).max(1.0)
}

/// How much audio must be buffered before playback can start without the sink
/// running dry later in the utterance.
///
/// `rtf` is the real-time factor of generation: seconds of compute per second
/// of audio. Below 1.0 the buffer grows on its own once playback starts, so any
/// small lead is safe and we keep the configured minimum. At or above 1.0
/// playback drains the buffer faster than generation refills it, and the lead
/// has to cover the whole shortfall over the rest of the utterance —
/// `(rtf - 1) * remaining` — or the sink will starve and rodio will splice
/// silence into the middle of the speech.
///
/// The safety factor absorbs the jitter an average RTF hides: a single slow
/// diffusion step costs more than the mean.
#[cfg_attr(not(feature = "voxcpm2"), allow(dead_code))]
pub(crate) fn required_lead_secs(rtf: f32, remaining_secs: f32, min_lead_secs: f32) -> f32 {
    const SAFETY: f32 = 1.25;
    if !rtf.is_finite() || rtf <= STREAM_RTF_MAX {
        return min_lead_secs;
    }
    let shortfall = (rtf - 1.0).max(0.0) * remaining_secs.max(0.0) * SAFETY;
    (min_lead_secs + shortfall).max(min_lead_secs)
}

/// Generation this much faster than realtime needs no lead beyond the minimum:
/// the buffer only grows from the moment playback starts. Kept below 1.0 by a
/// margin so a model that is *just* keeping up is treated as too slow to stream.
#[cfg_attr(not(feature = "voxcpm2"), allow(dead_code))]
pub(crate) const STREAM_RTF_MAX: f32 = 0.8;

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

    /// A single continuous rodio source fed by the generator thread.
    ///
    /// Feeding rodio one `SamplesBuffer` per generated chunk is what made
    /// playback choppy. Each appended buffer is a separate entry in the sink's
    /// queue, and when the queue runs dry — which it does whenever the next
    /// chunk is not finished yet — rodio splices in a filler block of silence
    /// and carries on. The result is speech that stalls and resumes repeatedly,
    /// and the bigger the chunk the longer it plays before the next gap.
    ///
    /// One source for the whole utterance removes the per-chunk queue boundary
    /// entirely. Starvation degrades to a moment of silence inside a continuous
    /// stream instead of a queue transition, and the lead buffer in [`speak`]
    /// is what stops it happening at all.
    struct StreamingPcm {
        rx: crossbeam_channel::Receiver<Vec<i16>>,
        buf: Vec<i16>,
        pos: usize,
        sample_rate: u32,
    }

    impl Iterator for StreamingPcm {
        type Item = i16;

        fn next(&mut self) -> Option<i16> {
            loop {
                if self.pos < self.buf.len() {
                    let sample = self.buf[self.pos];
                    self.pos += 1;
                    return Some(sample);
                }
                match self.rx.try_recv() {
                    Ok(next) => {
                        self.buf = next;
                        self.pos = 0;
                    }
                    // Never block: this runs on the audio thread, and blocking
                    // it would stall every other sound on the device. Silence
                    // is the only safe answer to a chunk that is not ready.
                    Err(crossbeam_channel::TryRecvError::Empty) => return Some(0),
                    // The generator dropped its sender: the utterance is over.
                    // This is what lets `sleep_until_end` ever return.
                    Err(crossbeam_channel::TryRecvError::Disconnected) => return None,
                }
            }
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (self.buf.len() - self.pos, None)
        }
    }

    impl rodio::Source for StreamingPcm {
        /// `None` means "length unknown, parameters constant", which keeps
        /// rodio's sample-rate converter running across the whole utterance
        /// rather than being rebuilt at a frame boundary.
        fn current_frame_len(&self) -> Option<usize> {
            None
        }

        fn channels(&self) -> u16 {
            1
        }

        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<std::time::Duration> {
            None
        }
    }

    /// f32 in [-1, 1] to the i16 samples rodio takes.
    fn to_pcm16(chunk: &[f32]) -> Vec<i16> {
        chunk
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect()
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

        let min_lead_secs = (cfg.prebuffer_ms as f32 / 1000.0).clamp(0.05, 30.0);
        let estimated_total_secs = super::estimate_speech_secs(&u.text);

        let started = std::time::Instant::now();
        let stream = session
            .model
            .generate_stream(&text, opts)
            .map_err(|e| anyhow::anyhow!("VoxCPM2 generation failed to start: {e}"))?;

        // Audio is held here until enough of a lead exists to play without the
        // sink running dry; after that it goes straight to the audio thread.
        let mut lead: Vec<i16> = Vec::new();
        let mut playing: Option<crossbeam_channel::Sender<Vec<i16>>> = None;

        let mut generated_samples: usize = 0;
        // Measured from the *first* chunk rather than from the call, so the
        // one-off prefill cost is excluded and this reflects the steady-state
        // generation rate — the thing that decides whether playback can keep up.
        let mut steady_start: Option<std::time::Instant> = None;
        let mut steady_samples: usize = 0;
        let mut required_lead_secs = min_lead_secs;
        let mut first_chunk_logged = false;

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
                steady_start = Some(std::time::Instant::now());
                if !is_prewarm {
                    info!(
                        "VoxCPM2 first audio chunk after {} ms",
                        started.elapsed().as_millis()
                    );
                }
            } else {
                steady_samples += chunk.len();
            }
            generated_samples += chunk.len();

            if is_prewarm {
                continue; // warmed, but nothing to play
            }

            let samples = to_pcm16(&chunk);
            if let Some(ref tx) = playing {
                // Already playing: hand the audio thread the new audio. A
                // disconnected receiver means playback was stopped underneath us.
                if tx.send(samples).is_err() {
                    break;
                }
                continue;
            }

            lead.extend_from_slice(&samples);

            // Re-measure on every chunk while still buffering: the required lead
            // depends on a generation rate we can only observe as we go.
            if let Some(steady_start) = steady_start {
                let steady_secs = steady_samples as f32 / sample_rate as f32;
                if steady_secs > 0.0 {
                    let rtf = steady_start.elapsed().as_secs_f32() / steady_secs;
                    // Nothing has played yet, so what still has to be covered is
                    // the estimated total minus what is already in hand.
                    let remaining = (estimated_total_secs
                        - lead.len() as f32 / sample_rate as f32)
                        .max(0.0);
                    required_lead_secs = super::required_lead_secs(rtf, remaining, min_lead_secs);
                }
            }

            let lead_secs = lead.len() as f32 / sample_rate as f32;
            if lead_secs >= required_lead_secs {
                let (tx, rx) = crossbeam_channel::unbounded::<Vec<i16>>();
                sink.append(StreamingPcm {
                    rx,
                    buf: std::mem::take(&mut lead),
                    pos: 0,
                    sample_rate,
                });
                sink.play();
                playing = Some(tx);
                if let Some(ref cb) = on_playback_start {
                    cb();
                }
                info!(
                    "VoxCPM2 playback started after {} ms with a {:.0} ms lead buffer",
                    started.elapsed().as_millis(),
                    lead_secs * 1000.0
                );
            }
        }

        if is_prewarm {
            info!(
                "VoxCPM2 prewarmed in {:.1} s — the first utterance skips model load and shader compilation",
                started.elapsed().as_secs_f32()
            );
            return Ok(());
        }

        if generated_samples == 0 {
            warn!("VoxCPM2 produced no audio for this utterance");
            return Ok(());
        }

        // Generation finished before the lead was ever reached — the model is
        // slower than realtime, or the utterance is shorter than the lead. Either
        // way the whole thing is in hand, so play it as one uninterrupted buffer.
        if playing.is_none() {
            if generation_counter.load(Ordering::SeqCst) != generation {
                return Ok(()); // stopped while buffering; nothing to play
            }
            let audio_secs = lead.len() as f32 / sample_rate as f32;
            info!(
                "VoxCPM2 generated {:.1} s of audio in {:.1} s before reaching the {:.0} ms lead; \
                 playing it complete rather than risking gaps mid-sentence",
                audio_secs,
                started.elapsed().as_secs_f32(),
                required_lead_secs * 1000.0
            );
            sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, lead));
            sink.play();
            if let Some(ref cb) = on_playback_start {
                cb();
            }
        } else {
            // Dropping the sender is what ends the source, and so what lets
            // `sleep_until_end` below return once the tail has played out.
            drop(playing);
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

    // ── Lead buffer sizing ───────────────────────────────────────────────────
    //
    // Regression tests for choppy playback. Feeding rodio a buffer per generated
    // chunk let the sink run dry between chunks, and rodio fills a dry queue with
    // silence rather than waiting — speech that stalled and resumed over and over,
    // lasting longer between gaps the bigger the chunk was.

    #[test]
    fn test_faster_than_realtime_generation_keeps_the_minimum_lead() {
        // Below 1.0 the buffer grows on its own once playback starts, so there is
        // nothing to cover and no reason to delay speaking.
        assert_eq!(required_lead_secs(0.3, 10.0, 0.4), 0.4);
        assert_eq!(required_lead_secs(STREAM_RTF_MAX, 10.0, 0.4), 0.4);
    }

    #[test]
    fn test_slower_than_realtime_generation_demands_a_bigger_lead() {
        // At RTF 1.5 every second of audio costs 1.5 s to make, so 10 s of speech
        // needs about 5 s of lead (plus the safety factor) or the sink starves.
        let lead = required_lead_secs(1.5, 10.0, 0.4);
        assert!(lead > 5.0, "lead {lead} must cover the shortfall");
        assert!(lead < 8.0, "lead {lead} should not be wildly over-provisioned");
    }

    #[test]
    fn test_lead_grows_with_both_slowness_and_remaining_audio() {
        // Monotonic in each input: a slower model or a longer sentence both mean
        // more audio has to be banked up front.
        assert!(required_lead_secs(2.0, 10.0, 0.4) > required_lead_secs(1.2, 10.0, 0.4));
        assert!(required_lead_secs(1.5, 20.0, 0.4) > required_lead_secs(1.5, 5.0, 0.4));
    }

    #[test]
    fn test_lead_never_drops_below_the_configured_minimum() {
        for rtf in [0.0, 0.1, 0.9, 1.0, 3.0] {
            assert!(
                required_lead_secs(rtf, 0.0, 0.4) >= 0.4,
                "rtf {rtf} must still honour the configured minimum"
            );
        }
    }

    #[test]
    fn test_lead_survives_a_degenerate_rate_measurement() {
        // A zero-length measurement window yields inf or NaN; that must fall back
        // to the minimum rather than poisoning the comparison and never playing.
        assert_eq!(required_lead_secs(f32::NAN, 10.0, 0.4), 0.4);
        assert_eq!(required_lead_secs(f32::INFINITY, 10.0, 0.4), 0.4);
    }

    #[test]
    fn test_speech_estimate_scales_with_text_length() {
        let short = estimate_speech_secs("Hello there.");
        let long = estimate_speech_secs(&"Hello there. ".repeat(40));
        assert!(long > short);
        // A sentence of ~140 characters is roughly ten seconds of speech.
        let ten_seconds = estimate_speech_secs(&"a".repeat(140));
        assert!((ten_seconds - 10.0).abs() < 1.0, "got {ten_seconds}");
    }

    #[test]
    fn test_speech_estimate_has_a_floor_for_tiny_text() {
        // Never zero: it multiplies the shortfall, and zero would collapse the
        // lead to the minimum for a slow model.
        assert!(estimate_speech_secs("") >= 1.0);
        assert!(estimate_speech_secs("Hi") >= 1.0);
    }

    #[test]
    fn test_backend_name_reports_whether_engine_is_compiled_in() {
        let name = voxcpm2_backend_name();
        assert_eq!(name == "not compiled in", !VOXCPM2_COMPILED);
    }
}
