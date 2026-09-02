//! The utterance queue, worker thread, and Piper/eSpeak synthesis. Pocket-TTS
//! synthesis lives in `pocket.rs` (called from [`TtsEngineWorker::run`]) since
//! it needs no access to this module's private fields.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use tracing::{debug, info, warn};
use voxctrl_config::{TtsConfig, TtsEngine};
use voxctrl_text::{correct_custom_vocabulary, expand_snippets};

use crate::breeze::{speak_breeze_tts_2, BreezeModelSlot};
use crate::inflect::speak_inflect_micro;
use crate::piper::{get_voice_path, piper_binary, sample_rate_for_voice};
use crate::pocket::speak_pocket_tts;
use crate::voxcpm::{speak_voxcpm2, VoxCpmModelSlot};

/// The worker's cached Inflect-Micro-v2 sessions. Without the `inflect-micro`
/// feature there is no model type to cache, so the slot degenerates to `()` and
/// `speak_inflect_micro` reports that the engine wasn't compiled in.
#[cfg(feature = "inflect-micro")]
type InflectModelSlot = Option<crate::inflect::model::InflectModel>;
#[cfg(not(feature = "inflect-micro"))]
type InflectModelSlot = Option<()>;

// ── Utterance queue ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,
    pub source_label: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TtsCommand {
    Play {
        utterance: Utterance,
        generation: u32,
    },
    UpdateConfig(TtsConfig),
    Shutdown,
}

static ACTIVE_SINK: std::sync::Mutex<Option<std::sync::Arc<rodio::Sink>>> = std::sync::Mutex::new(None);

pub fn stop_current_playback() {
    let mut guard = ACTIVE_SINK.lock().unwrap();
    if let Some(ref sink) = *guard {
        let _ = sink.stop();
    }
    *guard = None;
}

#[derive(Clone)]
pub struct TtsEngineHandle {
    tx: Sender<TtsCommand>,
    generation: Arc<std::sync::atomic::AtomicU32>,
}

impl TtsEngineHandle {
    pub fn speak(&self, text: impl Into<String>) {
        let gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(TtsCommand::Play {
            utterance: Utterance {
                text: text.into(),
                voice: None,
                source_label: None,
            },
            generation: gen,
        });
    }

    pub fn speak_utterance(&self, u: Utterance) {
        let gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(TtsCommand::Play {
            utterance: u,
            generation: gen,
        });
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        stop_current_playback();
    }

    pub fn update_config(&self, config: TtsConfig) {
        let _ = self.tx.send(TtsCommand::UpdateConfig(config));
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(TtsCommand::Shutdown);
    }
}

// ── TTS engine worker ─────────────────────────────────────────────────────────

pub type PlaybackCallback = Arc<dyn Fn() + Send + Sync + 'static>;
/// Called with a human-readable message whenever an utterance fails to play
/// (engine missing, voice not downloaded, audio device unavailable, ...).
pub type ErrorCallback = Arc<dyn Fn(String) + Send + Sync + 'static>;

pub struct TtsEngineWorker {
    config: TtsConfig,
    custom_vocabulary: Vec<String>,
    rx: Receiver<TtsCommand>,
    generation: Arc<std::sync::atomic::AtomicU32>,
    on_playback_start: Option<PlaybackCallback>,
    on_playback_end: Option<PlaybackCallback>,
    on_error: Option<ErrorCallback>,
}

impl TtsEngineWorker {
    pub fn start(
        config: TtsConfig,
        custom_vocabulary: Vec<String>,
        on_playback_start: Option<PlaybackCallback>,
        on_playback_end: Option<PlaybackCallback>,
        on_error: Option<ErrorCallback>,
    ) -> TtsEngineHandle {
        let (tx, rx) = bounded(32);
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handle = TtsEngineHandle { tx, generation: generation.clone() };

        let prewarm = match config.engine {
            TtsEngine::PocketTts => config.pocket_tts.prewarm,
            TtsEngine::InflectMicro => config.inflect_micro.prewarm,
            TtsEngine::BreezeTts2 => config.breeze_tts_2.prewarm,
            TtsEngine::VoxCpm2 => config.voxcpm2.prewarm,
            _ => false,
        };
        if prewarm {
            let _ = handle.tx.send(TtsCommand::Play {
                utterance: Utterance {
                    text: " ".into(),
                    voice: None,
                    source_label: Some("prewarm".into()),
                },
                generation: 0,
            });
        }

        let worker = Self {
            config,
            custom_vocabulary,
            rx,
            generation,
            on_playback_start,
            on_playback_end,
            on_error,
        };
        std::thread::Builder::new()
            .name("voxctrl-tts".into())
            .spawn(move || worker.run())
            .expect("spawn tts thread");

        handle
    }

    fn run(self) {
        info!("TTS engine started (engine={:?})", self.config.engine);
        let mut current_config = self.config.clone();

        // pocket-tts model + per-voice cloned voice state, cached for the lifetime of this worker thread.
        let mut pocket_tts_model: Option<pocket_tts::TTSModel> = None;
        let mut pocket_tts_voice_states: HashMap<String, pocket_tts::ModelState> = HashMap::new();
        // Inflect-Micro-v2 ONNX sessions, cached for the same lifetime.
        let mut inflect_model: InflectModelSlot = None;
        // Breeze-TTS-2 session + per-voice cloned state, cached for the same lifetime.
        let mut breeze_tts_2_model: BreezeModelSlot = None;
        let mut breeze_tts_2_voice_states: HashMap<String, pocket_tts::ModelState> = HashMap::new();
        // VoxCPM2 session (model + decoded reference-clip cache), cached for the
        // same lifetime. Loading the checkpoint costs 20-25 s, so it happens at
        // most once per worker thread.
        let mut voxcpm2_session: VoxCpmModelSlot = None;

        // Persistent Rodio Output Stream - kept alive for the lifetime of this thread!
        let mut audio_context: Option<(rodio::OutputStream, rodio::OutputStreamHandle, Arc<rodio::Sink>)> = None;

        let init_audio = |ctx: &mut Option<(rodio::OutputStream, rodio::OutputStreamHandle, Arc<rodio::Sink>)>| -> Result<Arc<rodio::Sink>> {
            if let Some((_, _, ref sink)) = ctx {
                return Ok(sink.clone());
            }
            let (stream, handle) = rodio::OutputStream::try_default()
                .map_err(|e| anyhow::anyhow!("audio output device: {e}"))?;
            let sink = Arc::new(rodio::Sink::try_new(&handle)
                .map_err(|e| anyhow::anyhow!("audio sink: {e}"))?);
            *ctx = Some((stream, handle, sink.clone()));
            Ok(sink)
        };

        while let Ok(cmd) = self.rx.recv() {
            match cmd {
                TtsCommand::UpdateConfig(new_cfg) => {
                    info!("TTS worker config dynamically updated (engine={:?})", new_cfg.engine);
                    current_config = new_cfg;
                }
                TtsCommand::Play { mut utterance, generation } => {
                    let current_gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
                    if generation < current_gen {
                        debug!("Discarding stale utterance: generation={generation} (current={current_gen})");
                        continue;
                    }

                    let is_prewarm = utterance.source_label.as_deref() == Some("prewarm");

                    if !is_prewarm {
                        let mut snips = current_config.snippets.clone();
                        // Guarantee explicit subword phonetic boundaries so Kyutai models (Pocket-TTS & Breeze-TTS-2)
                        // never omit the 'Con' syllable or slur into 'crol'.
                        snips.entry("VoxCtrl".to_string()).or_insert_with(|| "Voks Con-trol".to_string());
                        snips.entry("voxctrl".to_string()).or_insert_with(|| "Voks Con-trol".to_string());
                        snips.entry("Vox Control".to_string()).or_insert_with(|| "Voks Con-trol".to_string());
                        snips.entry("vox control".to_string()).or_insert_with(|| "Voks Con-trol".to_string());

                        utterance.text = expand_snippets(&utterance.text, &snips);
                        if !self.custom_vocabulary.is_empty() {
                            utterance.text = correct_custom_vocabulary(&utterance.text, &self.custom_vocabulary);
                        }
                    }

                    let sink_res = init_audio(&mut audio_context);
                    if let Err(e) = sink_res {
                        warn!("TTS audio init error: {e}");
                        if !is_prewarm {
                            if let Some(ref cb) = self.on_error {
                                cb(format!("Audio output unavailable: {e}"));
                            }
                        }
                        continue;
                    }
                    let sink = sink_res.unwrap();
                    let speed = if current_config.speed <= 0.0 { 1.0 } else { current_config.speed };
                    sink.set_speed(speed.clamp(0.5, 2.5));

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = Some(sink.clone());
                    }

                    // Caught rather than allowed to unwind: a panic here kills the
                    // worker thread outright, and because the channel sender lives on
                    // in the handle, every later `speak()` succeeds silently. The UI
                    // then waits forever for callbacks that can never fire, which
                    // presents as the app hanging rather than as a failure. ONNX
                    // Runtime can panic during session creation when its shared
                    // library cannot be resolved, so this path is reachable.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match current_config.engine {
                        TtsEngine::Piper => self.speak_piper(&utterance, &sink),
                        TtsEngine::Espeak => self.speak_espeak(&utterance),
                        TtsEngine::PocketTts => speak_pocket_tts(
                            &current_config,
                            &utterance,
                            &mut pocket_tts_model,
                            &mut pocket_tts_voice_states,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        TtsEngine::InflectMicro => speak_inflect_micro(
                            &current_config,
                            &utterance,
                            &mut inflect_model,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        TtsEngine::BreezeTts2 => speak_breeze_tts_2(
                            &current_config,
                            &utterance,
                            &mut breeze_tts_2_model,
                            &mut breeze_tts_2_voice_states,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        TtsEngine::VoxCpm2 => speak_voxcpm2(
                            &current_config,
                            &utterance,
                            &mut voxcpm2_session,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        }
                    }))
                    .unwrap_or_else(|payload| {
                        let detail = payload
                            .downcast_ref::<&str>()
                            .map(|s| (*s).to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "unknown panic".into());
                        Err(anyhow::anyhow!(
                            "TTS engine panicked: {detail}. For the Inflect-Micro-v2 \
                             engine this usually means ONNX Runtime could not be \
                             loaded — this build resolves libonnxruntime at runtime."
                        ))
                    });

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = None;
                    }

                    if let Err(e) = result {
                        warn!("TTS speak error: {e:#}");
                        if !is_prewarm {
                            if let Some(ref cb) = self.on_error {
                                cb(format!("{e:#}"));
                            }
                        }
                    }
                    if !is_prewarm {
                        if let Some(ref cb) = self.on_playback_end {
                            cb();
                        }
                    }
                }
                TtsCommand::Shutdown => {
                    debug!("TTS shutdown signal received");
                    stop_current_playback();
                    break;
                }
            }
        }
    }

    fn speak_piper(&self, u: &Utterance, sink: &rodio::Sink) -> Result<()> {
        let binary = piper_binary().ok_or_else(|| {
            anyhow::anyhow!(
                "Piper binary not found. Download a voice from TTS settings (this \
                 also installs the standalone Piper engine), or install piper \
                 system-wide."
            )
        })?;
        let voice_name = u.voice.as_deref().unwrap_or(&self.config.voice);

        let voice_path =
            get_voice_path(voice_name, &self.config.voice_dir).ok_or_else(|| {
                anyhow::anyhow!("Piper voice files not found for: {}", voice_name)
            })?;

        let length_scale = 1.0 / self.config.speed;
        let mut cmd = std::process::Command::new(&binary);
        cmd.arg("--model")
            .arg(&voice_path)
            .arg("--length-scale")
            .arg(length_scale.to_string())
            .arg("--output-raw");

        if self.config.gpu {
            cmd.arg("--cuda");
        }

        let mut piper = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn piper")?;

        use std::io::Write;
        piper
            .stdin
            .as_mut()
            .unwrap()
            .write_all(u.text.as_bytes())
            .context("write to piper stdin")?;

        let output = piper.wait_with_output().context("wait piper")?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "piper process failed with exit code {:?}: {}",
                output.status.code(),
                err_msg.trim()
            );
        }

        if output.stdout.is_empty() {
            anyhow::bail!("piper produced empty stdout");
        }

        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        play_raw_audio(sink, &output.stdout, sample_rate_for_voice(voice_name))?;
        Ok(())
    }

    fn speak_espeak(&self, u: &Utterance) -> Result<()> {
        if voxctrl_config::find_in_path("espeak-ng").is_none() {
            anyhow::bail!(
                "espeak-ng is not installed on this system. Install it with your \
                 package manager (e.g. `sudo pacman -S espeak-ng` or `sudo apt \
                 install espeak-ng`) or switch to another TTS engine."
            );
        }

        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        let wpm = (175.0 * self.config.speed) as i32;
        let status = std::process::Command::new("espeak-ng")
            .arg("-s")
            .arg(wpm.to_string())
            .arg(&u.text)
            .status()
            .context("spawn espeak-ng")?;
        if !status.success() {
            anyhow::bail!("espeak-ng exited with status {:?}", status.code());
        }
        Ok(())
    }
}

// ── Audio playback ────────────────────────────────────────────────────────────

fn play_raw_audio(sink: &rodio::Sink, raw: &[u8], sample_rate: u32) -> Result<()> {
    let samples: Vec<i16> = raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── stop() must bump the generation counter ──────────────────────────────
    //
    // Regression test for a bug where the global stop-key hotkey called the raw
    // `stop_current_playback()` free function instead of `TtsEngineHandle::stop()`.
    // That stopped the Rodio sink but left the generation counter unchanged, so
    // Pocket-TTS's frame-by-frame streaming loop (which only checks the counter
    // between frames) kept appending new audio — and `Sink::append()` resets the
    // sink's `stopped` flag, so playback silently resumed after the "stop".

    #[test]
    fn test_handle_stop_increments_generation_counter() {
        let (tx, _rx) = bounded(32);
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handle = TtsEngineHandle { tx, generation: generation.clone() };

        assert_eq!(generation.load(std::sync::atomic::Ordering::SeqCst), 0);
        handle.stop();
        assert_eq!(generation.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_raw_stop_current_playback_does_not_bump_generation() {
        // Documents the exact gap that caused the regression: calling the free
        // function alone never advances any generation counter, since it has no
        // knowledge of one. Callers MUST go through TtsEngineHandle::stop().
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        stop_current_playback();
        assert_eq!(generation.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn test_streaming_loop_cancellation_check_breaks_on_stale_generation() {
        // Mirrors the per-frame check inside speak_pocket_tts: once stop() bumps
        // the live counter past the snapshotted generation, the loop must stop
        // appending further frames instead of running to completion.
        let generation_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let snapshotted_generation = 0u32;

        let mut frames_processed = 0;
        for _ in 0..5 {
            if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != snapshotted_generation {
                break;
            }
            frames_processed += 1;
            if frames_processed == 2 {
                // Simulate stop() firing mid-stream.
                generation_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        assert_eq!(frames_processed, 2, "loop must abandon remaining frames after stop()");
    }
}
