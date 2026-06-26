use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use tracing::{debug, info, warn};
use voxctrl_config::{TtsConfig, TtsEngine};

// ── Piper voice catalogue ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub quality: &'static str,
    pub sample_rate: u32,
    pub filename: &'static str,
}

pub static PIPER_VOICES: &[VoiceInfo] = &[
    VoiceInfo { name: "en-us-libritts-high",   quality: "high",   sample_rate: 22050, filename: "en_US-libritts-high.onnx" },
    VoiceInfo { name: "en-us-amy-low",         quality: "low",    sample_rate: 16000, filename: "en_US-amy-low.onnx" },
    VoiceInfo { name: "en-us-kathleen-low",    quality: "low",    sample_rate: 16000, filename: "en_US-kathleen-low.onnx" },
    VoiceInfo { name: "en-gb-southern_english_female-low", quality: "low", sample_rate: 16000, filename: "en_GB-southern_english_female-low.onnx" },
    VoiceInfo { name: "en-us-ryan-high",       quality: "high",   sample_rate: 22050, filename: "en_US-ryan-high.onnx" },
    VoiceInfo { name: "en-us-ryan-medium",     quality: "medium", sample_rate: 22050, filename: "en_US-ryan-medium.onnx" },
    VoiceInfo { name: "en-us-ryan-low",        quality: "low",    sample_rate: 16000, filename: "en_US-ryan-low.onnx" },
    VoiceInfo { name: "en-us-lessac-medium",   quality: "medium", sample_rate: 16000, filename: "en_US-lessac-medium.onnx" },
    VoiceInfo { name: "en-us-lessac-low",      quality: "low",    sample_rate: 16000, filename: "en_US-lessac-low.onnx" },
    VoiceInfo { name: "en-us-danny-low",       quality: "low",    sample_rate: 16000, filename: "en_US-danny-low.onnx" },
    VoiceInfo { name: "en-gb-alan-low",        quality: "low",    sample_rate: 16000, filename: "en_GB-alan-low.onnx" },
];

// ── Pocket-TTS voice catalogue ────────────────────────────────────────────────
//
// pocket-tts clones a voice from a short reference clip rather than using a
// trained named-voice embedding table. We curate a small set of clips from
// the public `kyutai/tts-voices` HuggingFace dataset so VoxCtrl can still
// offer a familiar voice-picker UX. Clips are downloaded on first use (and
// cached by `hf-hub`) — see `pocket_tts_ready()` / `download_pocket_tts_assets()`.

#[derive(Debug, Clone)]
pub struct PocketTtsVoiceInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// `hf://` reference clip path consumed by `pocket_tts::weights::download_if_necessary`.
    pub reference_clip: &'static str,
}

pub static POCKET_TTS_VOICES: &[PocketTtsVoiceInfo] = &[
    PocketTtsVoiceInfo { id: "alba",    label: "Alba (Female)",   reference_clip: "hf://kyutai/tts-voices/alba-mackenna/casual.wav" },
    PocketTtsVoiceInfo { id: "anna",    label: "Anna (Female)",   reference_clip: "hf://kyutai/tts-voices/vctk/p228_023_enhanced.wav" },
    PocketTtsVoiceInfo { id: "vera",    label: "Vera (Female)",   reference_clip: "hf://kyutai/tts-voices/vctk/p229_023_enhanced.wav" },
    PocketTtsVoiceInfo { id: "charles", label: "Charles (Male)",  reference_clip: "hf://kyutai/tts-voices/vctk/p254_023_enhanced.wav" },
    PocketTtsVoiceInfo { id: "michael", label: "Michael (Male)",  reference_clip: "hf://kyutai/tts-voices/vctk/p360_023_enhanced.wav" },
];

pub fn pocket_tts_voice(id: &str) -> Option<&'static PocketTtsVoiceInfo> {
    POCKET_TTS_VOICES.iter().find(|v| v.id == id)
}

/// Default directory scanned for user-supplied Pocket-TTS voice clips.
pub fn pocket_tts_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("pocket-tts-voices")
}

fn resolve_pocket_tts_voices_dir(voice_dir: &str) -> PathBuf {
    if voice_dir.is_empty() {
        pocket_tts_voices_dir()
    } else {
        expand_tilde(voice_dir)
    }
}

/// Scans `voice_dir` for `<id>.wav` files, returning `(id, path)` pairs. A file named
/// after a built-in voice (e.g. `alba.wav`) overrides that voice's bundled reference clip.
fn scan_custom_pocket_tts_voices(voice_dir: &str) -> Vec<(String, PathBuf)> {
    let dir = resolve_pocket_tts_voices_dir(voice_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("wav")) != Some(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        found.push((stem.to_string(), path));
    }
    found
}

fn prettify_voice_label(id: &str) -> String {
    id.replace(['_', '-'], " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PocketTtsVoiceOption {
    pub id: String,
    pub label: String,
}

/// Built-in voices merged with any custom clips found in `voice_dir`, for the voice picker.
pub fn pocket_tts_voice_catalogue(voice_dir: &str) -> Vec<PocketTtsVoiceOption> {
    let custom = scan_custom_pocket_tts_voices(voice_dir);
    let mut options: Vec<PocketTtsVoiceOption> = POCKET_TTS_VOICES
        .iter()
        .map(|v| PocketTtsVoiceOption { id: v.id.to_string(), label: v.label.to_string() })
        .collect();

    for (id, _) in &custom {
        if let Some(existing) = options.iter_mut().find(|o| &o.id == id) {
            existing.label = format!("{} (Custom)", prettify_voice_label(id));
        } else {
            options.push(PocketTtsVoiceOption {
                id: id.clone(),
                label: format!("{} (Custom)", prettify_voice_label(id)),
            });
        }
    }
    options
}

/// Resolves a voice id to its reference clip source: either a built-in `hf://` URI or
/// a local path to a custom clip dropped into `voice_dir`. Custom clips take priority.
fn resolve_pocket_tts_voice_clip(id: &str, voice_dir: &str) -> Option<String> {
    let custom = scan_custom_pocket_tts_voices(voice_dir);
    if let Some((_, path)) = custom.iter().find(|(custom_id, _)| custom_id == id) {
        return Some(path.to_string_lossy().into_owned());
    }
    pocket_tts_voice(id).map(|v| v.reference_clip.to_string())
}

// ── Pocket-TTS model variant / sample rate ────────────────────────────────────

const POCKET_TTS_VARIANT: &str = "b6369a24";
const POCKET_TTS_SAMPLE_RATE: u32 = 24000;
// Gated weights repo; tokenizer + non-cloning fallback live in the ungated sibling repo.
const POCKET_TTS_WEIGHTS_REPO: &str = "kyutai/pocket-tts";
const POCKET_TTS_WEIGHTS_REVISION: &str = "427e3d61b276ed69fdd03de0d185fa8a8d97fc5b";
const POCKET_TTS_WEIGHTS_FILE: &str = "tts_b6369a24.safetensors";
const POCKET_TTS_TOKENIZER_REPO: &str = "kyutai/pocket-tts-without-voice-cloning";
const POCKET_TTS_TOKENIZER_REVISION: &str = "d4fdd22ae8c8e1cb3634e150ebeff1dab2d16df3";
const POCKET_TTS_TOKENIZER_FILE: &str = "tokenizer.model";

/// Best-effort, network-free check for whether the model weights, tokenizer, and the
/// selected voice's reference clip are already present in the local HuggingFace cache.
pub fn is_pocket_tts_ready(voice: &str, voice_dir: &str) -> bool {
    let cache = hf_hub::Cache::default();

    let weights_present = cache
        .repo(hf_hub::Repo::with_revision(
            POCKET_TTS_WEIGHTS_REPO.to_string(),
            hf_hub::RepoType::Model,
            POCKET_TTS_WEIGHTS_REVISION.to_string(),
        ))
        .get(POCKET_TTS_WEIGHTS_FILE)
        .is_some();

    let tokenizer_present = cache
        .repo(hf_hub::Repo::with_revision(
            POCKET_TTS_TOKENIZER_REPO.to_string(),
            hf_hub::RepoType::Model,
            POCKET_TTS_TOKENIZER_REVISION.to_string(),
        ))
        .get(POCKET_TTS_TOKENIZER_FILE)
        .is_some();

    let voice_present = match resolve_pocket_tts_voice_clip(voice, voice_dir) {
        Some(clip) => hf_cache_file_present(&clip),
        None => false,
    };

    weights_present && tokenizer_present && voice_present
}

fn hf_cache_file_present(hf_path: &str) -> bool {
    let Some(rest) = hf_path.strip_prefix("hf://") else { return Path::new(hf_path).exists() };
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        return false;
    }
    let repo_id = format!("{}/{}", parts[0], parts[1]);
    let filename_with_revision = parts[2..].join("/");
    let (filename, revision) = match filename_with_revision.rfind('@') {
        Some(at) => (filename_with_revision[..at].to_string(), Some(filename_with_revision[at + 1..].to_string())),
        None => (filename_with_revision, None),
    };

    let cache = hf_hub::Cache::default();
    let repo = match revision {
        Some(rev) => hf_hub::Repo::with_revision(repo_id, hf_hub::RepoType::Model, rev),
        None => hf_hub::Repo::model(repo_id),
    };
    cache.repo(repo).get(&filename).is_some()
}

/// Download the pocket-tts model weights, tokenizer, and the selected voice's reference
/// clip into the local HuggingFace cache. Requires `HF_TOKEN` to be set (the default
/// weights repo is gated and requires accepting the model license on huggingface.co).
pub async fn download_pocket_tts_assets(voice: &str, voice_dir: &str, hf_token: Option<String>) -> Result<()> {
    if let Some(token) = hf_token {
        // SAFETY: single-threaded at startup/download time; no concurrent env access.
        unsafe { std::env::set_var("HF_TOKEN", token) };
    }

    let reference_clip = resolve_pocket_tts_voice_clip(voice, voice_dir)
        .ok_or_else(|| anyhow::anyhow!("unknown pocket-tts voice: {voice}"))?;

    tokio::task::spawn_blocking(move || -> Result<()> {
        info!("Downloading pocket-tts model weights ({POCKET_TTS_VARIANT})...");
        pocket_tts::weights::download_if_necessary(&format!(
            "hf://{POCKET_TTS_WEIGHTS_REPO}/{POCKET_TTS_WEIGHTS_FILE}@{POCKET_TTS_WEIGHTS_REVISION}"
        ))
        .context("download pocket-tts model weights")?;

        info!("Downloading pocket-tts tokenizer...");
        pocket_tts::weights::download_if_necessary(&format!(
            "hf://{POCKET_TTS_TOKENIZER_REPO}/{POCKET_TTS_TOKENIZER_FILE}@{POCKET_TTS_TOKENIZER_REVISION}"
        ))
        .context("download pocket-tts tokenizer")?;

        info!("Downloading pocket-tts reference voice clip: {reference_clip}");
        pocket_tts::weights::download_if_necessary(&reference_clip)
            .context("download pocket-tts reference voice clip")?;

        Ok(())
    })
    .await
    .context("download_pocket_tts_assets task join")??;

    info!("pocket-tts assets ready for voice '{voice}'");
    Ok(())
}

// ── Piper helpers ─────────────────────────────────────────────────────────────

pub fn piper_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("piper-voices")
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn resolve_voices_dir(voice_dir: &str) -> PathBuf {
    if voice_dir.is_empty() {
        piper_voices_dir()
    } else {
        expand_tilde(voice_dir)
    }
}

pub fn piper_binary() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "piper.exe" } else { "piper" };
    let local = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("piper")
        .join(exe);
    if local.exists() {
        return Some(local);
    }
    voxctrl_config::find_in_path("piper")
}

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

    pub fn shutdown(&self) {
        let _ = self.tx.send(TtsCommand::Shutdown);
    }
}

// ── TTS engine worker ─────────────────────────────────────────────────────────

pub type PlaybackCallback = Arc<dyn Fn() + Send + Sync + 'static>;

pub struct TtsEngineWorker {
    config: TtsConfig,
    rx: Receiver<TtsCommand>,
    generation: Arc<std::sync::atomic::AtomicU32>,
    on_playback_start: Option<PlaybackCallback>,
    on_playback_end: Option<PlaybackCallback>,
}

impl TtsEngineWorker {
    pub fn start(
        config: TtsConfig,
        on_playback_start: Option<PlaybackCallback>,
        on_playback_end: Option<PlaybackCallback>,
    ) -> TtsEngineHandle {
        let (tx, rx) = bounded(32);
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handle = TtsEngineHandle { tx, generation: generation.clone() };

        if config.engine == TtsEngine::PocketTts && config.pocket_tts.prewarm {
            let _ = handle.tx.send(TtsCommand::Play {
                utterance: Utterance {
                    text: " ".into(),
                    voice: None,
                    source_label: Some("prewarm".into()),
                },
                generation: 0,
            });
        }

        let worker = Self { config, rx, generation, on_playback_start, on_playback_end };
        std::thread::Builder::new()
            .name("voxctrl-tts".into())
            .spawn(move || worker.run())
            .expect("spawn tts thread");

        handle
    }

    fn run(self) {
        info!("TTS engine started (engine={:?})", self.config.engine);
        // pocket-tts model + per-voice cloned voice state, cached for the lifetime of this worker thread.
        let mut pocket_tts_model: Option<pocket_tts::TTSModel> = None;
        let mut pocket_tts_voice_states: HashMap<String, pocket_tts::ModelState> = HashMap::new();

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
                TtsCommand::Play { utterance, generation } => {
                    let current_gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
                    if generation < current_gen {
                        debug!("Discarding stale utterance: generation={generation} (current={current_gen})");
                        continue;
                    }

                    let is_prewarm = utterance.source_label.as_deref() == Some("prewarm");

                    let sink_res = init_audio(&mut audio_context);
                    if let Err(e) = sink_res {
                        warn!("TTS audio init error: {e}");
                        continue;
                    }
                    let sink = sink_res.unwrap();

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = Some(sink.clone());
                    }

                    let result = match self.config.engine {
                        TtsEngine::Piper => self.speak_piper(&utterance, &sink),
                        TtsEngine::Espeak => self.speak_espeak(&utterance),
                        TtsEngine::PocketTts => speak_pocket_tts(
                            &self.config,
                            &utterance,
                            &mut pocket_tts_model,
                            &mut pocket_tts_voice_states,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                    };

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = None;
                    }

                    if let Err(e) = result {
                        warn!("TTS speak error: {e}");
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
        let binary = piper_binary().context("piper binary not found")?;
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
        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        let wpm = (175.0 * self.config.speed) as i32;
        std::process::Command::new("espeak-ng")
            .arg("-s")
            .arg(wpm.to_string())
            .arg(&u.text)
            .status()
            .context("espeak-ng")?;
        Ok(())
    }
}

// ── pocket-tts synthesis (pure Rust / Candle) ─────────────────────────────────

fn speak_pocket_tts(
    config: &TtsConfig,
    u: &Utterance,
    model: &mut Option<pocket_tts::TTSModel>,
    voice_states: &mut HashMap<String, pocket_tts::ModelState>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    generation: u32,
) -> Result<()> {
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let voice = u.voice.as_deref().unwrap_or(&config.pocket_tts.voice);

    if !is_pocket_tts_ready(voice, &config.pocket_tts.voice_dir) {
        anyhow::bail!("pocket-tts assets for voice '{voice}' not found. Download them from TTS settings.");
    }

    // Lazily load the model — stays alive for the worker thread lifetime.
    if model.is_none() {
        info!("Loading pocket-tts model (variant={POCKET_TTS_VARIANT})");
        *model = Some(
            pocket_tts::TTSModel::load(POCKET_TTS_VARIANT).context("load pocket-tts model")?,
        );
    }
    let model = model.as_ref().unwrap();

    if !voice_states.contains_key(voice) {
        let reference_clip = resolve_pocket_tts_voice_clip(voice, &config.pocket_tts.voice_dir)
            .ok_or_else(|| anyhow::anyhow!("unknown pocket-tts voice: {voice}"))?;
        let clip_path = pocket_tts::weights::download_if_necessary(&reference_clip)
            .context("resolve pocket-tts reference voice clip")?;
        let state = model
            .get_voice_state(&clip_path)
            .context("compute pocket-tts voice state")?;
        voice_states.insert(voice.to_string(), state);
    }
    let voice_state = voice_states.get(voice).unwrap();

    if is_prewarm {
        // Run generation once to warm the model and caches; nothing is played.
        let _ = model.generate(&u.text, voice_state).context("pocket-tts generate")?;
        return Ok(());
    }

    // Stream audio frame-by-frame instead of waiting for the whole utterance to
    // finish generating: each frame is queued onto the sink as soon as it's ready,
    // so playback of the first frame overlaps with generation of the rest. This cuts
    // perceived latency from "time to generate the whole sentence" down to roughly
    // "time to generate the first frame".
    let mut callback_fired = false;
    for chunk in model.generate_stream(&u.text, voice_state) {
        if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != generation {
            break; // stop() was called — abandon the rest of the generation
        }
        let chunk = chunk.context("pocket-tts generate (stream)")?;
        let chunk = chunk.squeeze(0).context("squeeze pocket-tts audio chunk")?;
        let bytes =
            pocket_tts::audio::pcm_i16_le_bytes(&chunk).context("encode pocket-tts audio chunk")?;

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
        sink.append(rodio::buffer::SamplesBuffer::new(1, POCKET_TTS_SAMPLE_RATE, samples));
    }
    sink.sleep_until_end();
    Ok(())
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

// ── Voice catalogue helpers ───────────────────────────────────────────────────

fn voice_name_to_filename(name: &str) -> Option<String> {
    PIPER_VOICES
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.filename.to_string())
}

fn sample_rate_for_voice(name: &str) -> u32 {
    PIPER_VOICES
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.sample_rate)
        .unwrap_or(22050)
}

pub fn is_voice_downloaded(voice_name: &str, voice_dir: &str) -> bool {
    get_voice_path(voice_name, voice_dir).is_some()
}

// ── Piper voice download ──────────────────────────────────────────────────────

const PIPER_RELEASE_BASE: &str =
    "https://github.com/rhasspy/piper/releases/download/v0.0.2/";

pub fn get_voice_path(voice_name: &str, voice_dir: &str) -> Option<PathBuf> {
    let filename = voice_name_to_filename(voice_name)
        .unwrap_or_else(|| format!("{voice_name}.onnx"));

    let voices_dir = resolve_voices_dir(voice_dir);

    let path_onnx = voices_dir.join(&filename);
    let path_json = voices_dir.join(format!("{filename}.json"));
    if path_onnx.exists() && path_json.exists() {
        return Some(path_onnx);
    }

    let filename_lower = filename.to_lowercase();
    let path_onnx_lower = voices_dir.join(&filename_lower);
    let path_json_lower = voices_dir.join(format!("{filename_lower}.json"));
    if path_onnx_lower.exists() && path_json_lower.exists() {
        return Some(path_onnx_lower);
    }

    let path_raw_lower = voices_dir.join(format!("{}.onnx", voice_name.to_lowercase()));
    let path_raw_json_lower =
        voices_dir.join(format!("{}.onnx.json", voice_name.to_lowercase()));
    if path_raw_lower.exists() && path_raw_json_lower.exists() {
        return Some(path_raw_lower);
    }

    None
}

pub async fn download_piper_binary() -> Result<()> {
    #[cfg(unix)]
    {
        let local_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("piper");

        tokio::fs::create_dir_all(&local_dir).await?;

        let dest_exe = local_dir.join("piper");
        if dest_exe.exists() {
            return Ok(());
        }

        info!("Downloading standalone Piper binary...");
        let url =
            "https://github.com/rhasspy/piper/releases/download/v1.2.0/piper_amd64.tar.gz";

        let response = reqwest::get(url).await?.error_for_status()?;
        let bytes = response.bytes().await?;

        info!("Extracting Piper binary...");
        let cursor = std::io::Cursor::new(bytes);
        let tar = flate2::read::GzDecoder::new(cursor);
        let mut archive = tar::Archive::new(tar);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            if let Some(file_name) = path.file_name() {
                let dest = local_dir.join(file_name);
                let mut outfile = std::fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut outfile)?;

                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = std::fs::metadata(&dest) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&dest, perms);
                }
            }
        }
        info!("Standalone Piper binary installed to {}", dest_exe.display());
    }
    Ok(())
}

pub async fn download_voice(voice_name: &str, voice_dir: &str) -> Result<()> {
    if piper_binary().is_none() {
        if let Err(e) = download_piper_binary().await {
            warn!("Failed to download standalone piper binary: {e}");
        }
    }

    let voices_dir = resolve_voices_dir(voice_dir);
    tokio::fs::create_dir_all(&voices_dir).await?;

    if get_voice_path(voice_name, voice_dir).is_some() {
        info!("Voice {} is already downloaded.", voice_name);
        return Ok(());
    }

    let tarball_url = format!("{PIPER_RELEASE_BASE}voice-{voice_name}.tar.gz");
    info!("Downloading voice tarball: {tarball_url}");

    let response = reqwest::get(&tarball_url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    info!("Extracting voice files...");
    let cursor = std::io::Cursor::new(bytes);
    let tar = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let file_name = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        if file_name.ends_with(".onnx") || file_name.ends_with(".onnx.json") {
            let dest_path = voices_dir.join(&file_name);
            let mut temp_file = tempfile::NamedTempFile::new_in(&voices_dir)?;
            std::io::copy(&mut entry, &mut temp_file)?;
            temp_file.persist(&dest_path)?;
            info!("Extracted: {}", dest_path.display());
        }
    }

    info!("Voice files successfully downloaded and extracted.");
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_fake_voice(dir: &std::path::Path, filename: &str) {
        fs::write(dir.join(filename), b"fake onnx model").unwrap();
        fs::write(dir.join(format!("{filename}.json")), b"{}").unwrap();
    }

    // ── resolve_voices_dir ────────────────────────────────────────────────────

    #[test]
    fn test_resolve_voices_dir_empty_uses_default() {
        let result = resolve_voices_dir("");
        assert_eq!(result, piper_voices_dir());
    }

    #[test]
    fn test_resolve_voices_dir_absolute_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let result = resolve_voices_dir(path);
        assert_eq!(result, dir.path());
    }

    #[test]
    fn test_resolve_voices_dir_tilde_expands() {
        let result = resolve_voices_dir("~/my-voices");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home.join("my-voices"));
    }

    #[test]
    fn test_resolve_voices_dir_tilde_alone_expands() {
        let result = resolve_voices_dir("~");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home);
    }

    // ── expand_tilde ──────────────────────────────────────────────────────────

    #[test]
    fn test_expand_tilde_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn test_expand_tilde_subdir() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/.piper-voices"), home.join(".piper-voices"));
    }

    #[test]
    fn test_expand_tilde_absolute_unchanged() {
        assert_eq!(expand_tilde("/usr/share/voices"), PathBuf::from("/usr/share/voices"));
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("relative/path"), PathBuf::from("relative/path"));
    }

    // ── is_voice_downloaded ───────────────────────────────────────────────────

    #[test]
    fn test_is_voice_downloaded_default_dir_not_present() {
        let _ = is_voice_downloaded("en-us-lessac-medium", "");
    }

    #[test]
    fn test_is_voice_downloaded_returns_true_when_files_exist() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        create_fake_voice(dir.path(), "en_US-amy-low.onnx");
        assert!(is_voice_downloaded("en-us-amy-low", path));
    }

    #[test]
    fn test_is_voice_downloaded_returns_false_when_files_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(!is_voice_downloaded("en-us-amy-low", path));
    }

    #[test]
    fn test_is_voice_downloaded_returns_false_for_nonexistent_dir() {
        assert!(!is_voice_downloaded("en-us-amy-low", "/nonexistent/path/xyz"));
    }

    #[test]
    fn test_is_voice_downloaded_tilde_path() {
        let _ = is_voice_downloaded("en-us-lessac-medium", "~/.local/share/voxctrl/piper-voices");
    }

    #[test]
    fn test_is_voice_downloaded_only_onnx_not_sufficient() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("en_US-amy-low.onnx"), b"fake").unwrap();
        assert!(!is_voice_downloaded("en-us-amy-low", path));
    }

    // ── get_voice_path ────────────────────────────────────────────────────────

    #[test]
    fn test_get_voice_path_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(get_voice_path("en-us-ryan-high", path).is_none());
    }

    #[test]
    fn test_get_voice_path_returns_some_when_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        create_fake_voice(dir.path(), "en_US-ryan-high.onnx");
        let result = get_voice_path("en-us-ryan-high", path);
        assert!(result.is_some());
        assert!(result.unwrap().exists());
    }

    #[test]
    fn test_get_voice_path_accepts_custom_dir() {
        let dir = tempdir().unwrap();
        let other_dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let other_path = other_dir.path().to_str().unwrap();
        create_fake_voice(other_dir.path(), "en_US-danny-low.onnx");
        assert!(get_voice_path("en-us-danny-low", path).is_none());
        assert!(get_voice_path("en-us-danny-low", other_path).is_some());
    }

    #[test]
    fn test_get_voice_path_lowercase_fallback() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let lc_name = "en_us-lessac-medium.onnx";
        fs::write(dir.path().join(lc_name), b"fake").unwrap();
        fs::write(dir.path().join(format!("{lc_name}.json")), b"{}").unwrap();
        assert!(get_voice_path("en-us-lessac-medium", path).is_some());
    }

    // ── Piper voice catalogue ─────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_not_empty() {
        assert!(!PIPER_VOICES.is_empty());
    }

    #[test]
    fn test_piper_voices_have_required_fields() {
        for v in PIPER_VOICES {
            assert!(!v.name.is_empty());
            assert!(!v.quality.is_empty());
            assert!(!v.filename.is_empty());
            assert!(v.sample_rate > 0);
        }
    }

    #[test]
    fn test_piper_voices_names_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in PIPER_VOICES {
            assert!(seen.insert(v.name), "duplicate piper voice name: {}", v.name);
        }
    }

    #[test]
    fn test_piper_voices_filenames_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in PIPER_VOICES {
            assert!(seen.insert(v.filename), "duplicate piper filename: {}", v.filename);
        }
    }

    #[test]
    fn test_piper_voices_quality_values_are_valid() {
        let valid = ["high", "medium", "low"];
        for v in PIPER_VOICES {
            assert!(valid.contains(&v.quality), "unexpected quality '{}' for {}", v.quality, v.name);
        }
    }

    #[test]
    fn test_piper_voices_sample_rates_are_valid() {
        let valid_rates = [16000u32, 22050u32];
        for v in PIPER_VOICES {
            assert!(valid_rates.contains(&v.sample_rate), "unexpected sample_rate {} for {}", v.sample_rate, v.name);
        }
    }

    #[test]
    fn test_piper_voices_filenames_end_with_onnx() {
        for v in PIPER_VOICES {
            assert!(v.filename.ends_with(".onnx"), "filename should end with .onnx: {}", v.filename);
        }
    }

    // ── sample_rate_for_voice ─────────────────────────────────────────────────

    #[test]
    fn test_sample_rate_for_known_high_quality_voice() {
        assert_eq!(sample_rate_for_voice("en-us-ryan-high"), 22050);
    }

    #[test]
    fn test_sample_rate_for_known_low_quality_voice() {
        assert_eq!(sample_rate_for_voice("en-us-amy-low"), 16000);
    }

    #[test]
    fn test_sample_rate_for_unknown_voice_defaults_to_22050() {
        assert_eq!(sample_rate_for_voice("xx-unknown-voice"), 22050);
    }

    // ── piper_binary ──────────────────────────────────────────────────────────

    #[test]
    fn test_piper_binary_returns_option_without_panicking() {
        let _ = piper_binary();
    }

    // ── piper_voices_dir ──────────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_dir_not_empty() {
        let d = piper_voices_dir();
        assert!(d.components().count() > 0);
    }

    #[test]
    fn test_piper_voices_dir_ends_with_piper_voices() {
        let d = piper_voices_dir();
        assert!(d.ends_with("voxctrl/piper-voices"));
    }

    // ── voice_name_to_filename ────────────────────────────────────────────────

    #[test]
    fn test_voice_name_to_filename_known() {
        assert_eq!(
            voice_name_to_filename("en-us-lessac-medium"),
            Some("en_US-lessac-medium.onnx".to_string())
        );
    }

    #[test]
    fn test_voice_name_to_filename_unknown_returns_none() {
        assert_eq!(voice_name_to_filename("xx-unknown-voice"), None);
    }

    #[test]
    fn test_voice_name_to_filename_all_piper_voices_resolve() {
        for v in PIPER_VOICES {
            let result = voice_name_to_filename(v.name);
            assert!(result.is_some(), "voice_name_to_filename should resolve {}", v.name);
            assert_eq!(result.unwrap(), v.filename);
        }
    }

    // ── Pocket-TTS voice catalogue ──────────────────────────────────────────────

    #[test]
    fn test_pocket_tts_voices_not_empty() {
        assert!(!POCKET_TTS_VOICES.is_empty());
    }

    #[test]
    fn test_pocket_tts_voices_have_required_fields() {
        for v in POCKET_TTS_VOICES {
            assert!(!v.id.is_empty());
            assert!(!v.label.is_empty());
            assert!(v.reference_clip.starts_with("hf://"));
        }
    }

    #[test]
    fn test_pocket_tts_voices_ids_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in POCKET_TTS_VOICES {
            assert!(seen.insert(v.id), "duplicate voice id: {}", v.id);
        }
    }

    #[test]
    fn test_pocket_tts_voice_lookup_known() {
        assert!(pocket_tts_voice("alba").is_some());
        assert!(pocket_tts_voice("michael").is_some());
    }

    #[test]
    fn test_pocket_tts_voice_lookup_unknown_returns_none() {
        assert!(pocket_tts_voice("not-a-real-voice").is_none());
    }

    // ── hf_cache_file_present ────────────────────────────────────────────────

    #[test]
    fn test_hf_cache_file_present_missing_returns_false() {
        assert!(!hf_cache_file_present("hf://kyutai/tts-voices/does-not-exist.wav"));
    }

    #[test]
    fn test_hf_cache_file_present_non_hf_path_checks_filesystem() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("clip.wav");
        fs::write(&file, b"fake audio").unwrap();
        assert!(hf_cache_file_present(file.to_str().unwrap()));
    }

    // ── is_pocket_tts_ready ──────────────────────────────────────────────────

    #[test]
    fn test_is_pocket_tts_ready_false_for_unknown_voice() {
        assert!(!is_pocket_tts_ready("not-a-real-voice", ""));
    }

    // ── custom voice directory ───────────────────────────────────────────────

    #[test]
    fn test_scan_custom_pocket_tts_voices_finds_wav_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("myvoice.wav"), b"fake audio").unwrap();
        fs::write(dir.path().join("notes.txt"), b"ignore me").unwrap();
        let found = scan_custom_pocket_tts_voices(dir.path().to_str().unwrap());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "myvoice");
    }

    #[test]
    fn test_pocket_tts_voice_catalogue_merges_custom_voices() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("myvoice.wav"), b"fake audio").unwrap();
        let catalogue = pocket_tts_voice_catalogue(dir.path().to_str().unwrap());
        assert!(catalogue.iter().any(|v| v.id == "myvoice"));
        assert!(catalogue.iter().any(|v| v.id == "alba"));
    }

    #[test]
    fn test_pocket_tts_voice_catalogue_custom_overrides_builtin_label() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alba.wav"), b"fake audio").unwrap();
        let catalogue = pocket_tts_voice_catalogue(dir.path().to_str().unwrap());
        let alba = catalogue.iter().find(|v| v.id == "alba").unwrap();
        assert!(alba.label.contains("Custom"));
    }

    #[test]
    fn test_resolve_pocket_tts_voice_clip_prefers_custom() {
        let dir = tempdir().unwrap();
        let clip = dir.path().join("alba.wav");
        fs::write(&clip, b"fake audio").unwrap();
        let resolved = resolve_pocket_tts_voice_clip("alba", dir.path().to_str().unwrap()).unwrap();
        assert_eq!(resolved, clip.to_string_lossy());
    }

    #[test]
    fn test_resolve_pocket_tts_voice_clip_falls_back_to_builtin() {
        let dir = tempdir().unwrap();
        let resolved = resolve_pocket_tts_voice_clip("alba", dir.path().to_str().unwrap()).unwrap();
        assert!(resolved.starts_with("hf://"));
    }

    #[test]
    fn test_resolve_pocket_tts_voice_clip_unknown_returns_none() {
        let dir = tempdir().unwrap();
        assert!(resolve_pocket_tts_voice_clip("not-a-real-voice", dir.path().to_str().unwrap()).is_none());
    }

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

// ── FIFO response listener ────────────────────────────────────────────────────

pub async fn run_fifo_responder(fifo_path: String, tts: TtsEngineHandle) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    info!("FIFO responder watching {fifo_path}");
    loop {
        while !std::path::Path::new(&fifo_path).exists() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        match tokio::fs::File::open(&fifo_path).await {
            Ok(file) => {
                let mut lines = BufReader::new(file).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        tts.speak(line);
                    }
                }
            }
            Err(e) => {
                warn!("FIFO open error {fifo_path}: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
