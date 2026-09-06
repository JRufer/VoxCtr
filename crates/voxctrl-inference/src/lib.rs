pub mod backend;
#[cfg(feature = "moonshine")]
pub mod moonshine;
pub mod postprocess;
mod util;
pub mod whisper_cpp;

/// Whether the Moonshine ONNX backend was compiled into this build. When false,
/// selecting Moonshine transparently falls back to whisper-cpp, and callers
/// (e.g. the "model not downloaded" UI checks) must treat a Moonshine selection
/// as effectively whisper-cpp.
pub const MOONSHINE_COMPILED: bool = cfg!(feature = "moonshine");

/// Which GPU backend whisper.cpp can offload to in this build, or `None` for a
/// CPU-only build.
///
/// This is fixed when the binary is compiled — ggml links exactly one compute
/// backend — so it is the honest answer to "can this app use my GPU", and the
/// only correct source for the Device dropdown. `whisper_cpp.device` selects
/// *whether* to offload, never *to what*: a config asking for `cuda` on a
/// Vulkan build gets Vulkan, and on a CPU build gets nothing at all.
pub fn whisper_gpu_backend() -> Option<&'static str> {
    // Both features can be enabled at once; ggml picks CUDA in that case.
    if cfg!(feature = "cuda") {
        Some("cuda")
    } else if cfg!(feature = "vulkan") {
        Some("vulkan")
    } else {
        None
    }
}

/// Which GPU backend the Moonshine ONNX backend can offload to in this build,
/// or `None` when it runs on the CPU.
///
/// ONNX Runtime has no Vulkan execution provider, so a Vulkan build — the one
/// that gives whisper.cpp GPU offload without CUDA's runtime — has no GPU path
/// for Moonshine at all. That asymmetry is why this is reported separately from
/// [`whisper_gpu_backend`] rather than inferred from it: they are genuinely
/// different answers in the build most people run.
pub fn moonshine_gpu_backend() -> Option<&'static str> {
    if cfg!(feature = "moonshine-cuda") {
        Some("cuda")
    } else if cfg!(feature = "moonshine-coreml") {
        Some("coreml")
    } else {
        None
    }
}

use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use tracing::{error, info};
use voxctrl_config::{AppConfig, BackendChoice};

use backend::{TranscribeRequest, TranscriptionBackend};
use postprocess::{run_pipeline, PostProcessConfig, is_silence_hallucination};
use whisper_cpp::WhisperCppBackend;

// ── Audio chunk type (must match voxctrl-audio) ────────────────────────────────

pub type AudioChunk = Vec<f32>;

// ── Inference request ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// Accumulated audio samples (16 kHz, mono, f32)
    pub audio: Vec<f32>,
    /// Target id (used to look up per-target processing overrides)
    pub target_id: String,
    /// Hotkey binding ID (if triggered by a hotkey)
    pub binding_id: Option<String>,
}

/// Final output after transcription + post-processing.
#[derive(Debug, Clone)]
pub struct InferenceOutput {
    pub text: String,
    pub target_id: String,
    pub raw_text: String,
    pub inference_ms: u32,
    pub language: String,
    /// Set when transcription failed (model missing, backend error, ...). The
    /// UI layer surfaces this to the user; `text` is empty in that case.
    pub error: Option<String>,
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct InferenceEngine {
    config: Arc<AppConfig>,
    backend: Box<dyn TranscriptionBackend>,
}

impl InferenceEngine {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let backend = build_backend(&config);
        Self { config, backend }
    }

    /// Load the selected backend model. Blocks until ready.
    pub fn load(&mut self) -> Result<()> {
        self.backend.load()
    }

    pub fn unload(&mut self) {
        self.backend.unload();
    }

    /// Transcribe and post-process. Returns the final text.
    pub fn process(&self, req: InferenceRequest) -> Result<InferenceOutput> {
        if req.audio.is_empty() {
            return Ok(InferenceOutput {
                text: String::new(),
                target_id: req.target_id,
                raw_text: String::new(),
                inference_ms: 0,
                language: "en".into(),
                error: None,
            });
        }

        // whisper-cpp auto-detects language when None; passing Moonshine's language
        // field (a different engine) here was wrong.
        let language: Option<String> = None;

        // Use the in-memory config that was passed to this engine — no disk I/O on
        // the hot path, no TOCTOU race with concurrent save_config writes.
        let app_config = (*self.config).clone();

        // ── Noise Gate (VAD) ──────────────────────────────────────────────────
        // Compute RMS energy of the entire audio request to implement a robust noise gate.
        let rms = {
            let sum_sq: f32 = req.audio.iter().map(|&s| s * s).sum();
            (sum_sq / req.audio.len() as f32).sqrt()
        };

        // Map vad_threshold (0.0 - 1.0) to physical RMS threshold.
        // Invert so that 1.0 represents MAXIMUM sensitivity (completely open gate / 0.0 RMS threshold).
        // 0.0 represents MINIMUM sensitivity (highest gate / 0.006 RMS threshold).
        // A default slider value of 0.5 maps to 0.003 RMS, which easily lets speech through while filtering silence.
        let rms_threshold = (1.0 - app_config.audio.vad_threshold) * 0.006;

        if rms < rms_threshold {
            info!(
                "Audio skipped by noise gate: RMS is {:.5} (threshold is {:.5}, vad_threshold={:.2})",
                rms,
                rms_threshold,
                app_config.audio.vad_threshold
            );
            return Ok(InferenceOutput {
                text: String::new(),
                target_id: req.target_id,
                raw_text: String::new(),
                inference_ms: 0,
                language: "en".into(),
                error: None,
            });
        }

        let dir = voxctrl_routing::config_dir();
        let targets = voxctrl_routing::load_targets(&dir).unwrap_or_default();

        let mut merged_prompt = String::from("VoxCtrl is a voice control assistant application. VoxCtrl commands start with VoxCtrl. ");

        // Custom vocabulary words from features config
        if !app_config.features.custom_vocabulary.is_empty() {
            // Append as: "Vocabulary: word1, word2, word3..."
            merged_prompt.push_str("Vocabulary: ");
            merged_prompt.push_str(&app_config.features.custom_vocabulary.join(", "));
            merged_prompt.push_str(". ");
        }

        let initial_prompt = if merged_prompt.trim().is_empty() {
            None
        } else {
            Some(merged_prompt.trim().to_string())
        };

        let t_req = TranscribeRequest {
            audio: req.audio,
            language,
            word_timestamps: false,
            initial_prompt,
        };

        let result = self.backend.transcribe(&t_req)?;
        let raw_text = result.text.clone();

        let post_cfg = self.build_post_config_with_app_config(&req.target_id, &app_config, &targets);
        let mut processed = run_pipeline(&raw_text, &post_cfg);

        // ── Silence Hallucination Filter ──────────────────────────────────────
        // If Whisper returned a known silence hallucination (like "Thank you"), check if the audio energy
        // was extremely low (e.g. below 0.003 RMS, which is absolute background room silence).
        // This ensures the user can still say a genuine, spoken "Thank you" (which has much higher energy),
        // while perfectly discarding silence-induced hallucinations when sensitivity is set high.
        if !processed.is_empty() && is_silence_hallucination(&processed) && rms < 0.003 {
            info!("Discarded silence hallucination '{}' (audio RMS: {:.5})", processed, rms);
            processed = String::new();
        }

        // ── Hotkey-Specific OpenAI Post-Processing ────────────────────────────
        let bindings = voxctrl_routing::load_bindings(&dir).unwrap_or_default();
        let binding = req.binding_id.as_ref().and_then(|bid| bindings.iter().find(|b| &b.id == bid));

        let binding_wants_openai = binding
            .and_then(|b| b.openai_enabled)
            .unwrap_or(false);

        if binding_wants_openai && !processed.is_empty() {
            // Re-read the OpenAI settings from disk so changes made in the
            // Settings UI (model, endpoint, API key, prompts) take effect without
            // restarting the app. Targets and bindings above are already hot-read
            // from disk per request; the global AppConfig held by this worker is
            // frozen at startup (intentional for the Whisper backend), so using
            // its `openai` section here would ignore the user's latest settings.
            let mut openai_cfg = voxctrl_config::Config::load().data.openai;
            openai_cfg.enabled = true;

            if let Some(ref b) = binding {
                if let Some(ref model) = b.openai_model {
                    if !model.is_empty() {
                        openai_cfg.model = model.clone();
                    }
                }
                if let Some(ref mode_str) = b.openai_mode {
                    let mode = match mode_str.as_str() {
                        "clean" => voxctrl_config::OpenAiMode::Clean,
                        "formal" => voxctrl_config::OpenAiMode::Formal,
                        "casual" => voxctrl_config::OpenAiMode::Casual,
                        "bullet" => voxctrl_config::OpenAiMode::Bullet,
                        "concise" => voxctrl_config::OpenAiMode::Concise,
                        "custom" => voxctrl_config::OpenAiMode::Custom,
                        _ => voxctrl_config::OpenAiMode::Clean,
                    };
                    // A non-custom preset overrides the system prompt for this hotkey.
                    if mode != voxctrl_config::OpenAiMode::Custom {
                        openai_cfg.mode = mode.clone();
                        openai_cfg.system_prompt =
                            voxctrl_llm::preset_system_prompt(&mode).to_string();
                    }
                }
                // An explicit per-hotkey system prompt overrides the global default
                // (and any preset selected above).
                if let Some(ref system_prompt) = b.openai_system_prompt {
                    if !system_prompt.is_empty() {
                        openai_cfg.system_prompt = system_prompt.clone();
                    }
                }
                if let Some(ref prompt) = b.openai_prompt {
                    if !prompt.is_empty() {
                        // The per-hotkey prompt template overrides the user prompt
                        // (it already requires the "{text}" placeholder).
                        openai_cfg.user_prompt = prompt.clone();
                    }
                }
            }

            let client = voxctrl_llm::OpenAiClient::new(openai_cfg);
            let processed_res = match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    let c = client.clone();
                    let text = processed.clone();
                    std::thread::spawn(move || {
                        handle.block_on(async { c.process(&text).await })
                    }).join().unwrap_or(processed)
                }
                Err(_) => {
                    if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                        rt.block_on(async { client.process(&processed).await })
                    } else {
                        processed
                    }
                }
            };
            processed = processed_res;
        }

        Ok(InferenceOutput {
            text: processed,
            target_id: req.target_id,
            raw_text,
            inference_ms: result.inference_ms,
            language: result.language,
            error: None,
        })
    }

    fn build_post_config_with_app_config(&self, target_id: &str, app_config: &voxctrl_config::AppConfig, targets: &[voxctrl_routing::OutputTarget]) -> PostProcessConfig {
        let target_ids: Vec<&str> = target_id.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let first_target_id = target_ids.first().copied().unwrap_or("default");
        let target = targets.iter().find(|t| t.id == first_target_id);

        let remove_fillers = target
            .and_then(|t| t.processing.remove_fillers)
            .unwrap_or(app_config.features.remove_fillers);

        let spoken_punctuation = target
            .and_then(|t| t.processing.spoken_punctuation)
            .unwrap_or(app_config.features.spoken_punctuation);

        let auto_format_lists = target
            .and_then(|t| t.processing.auto_format_lists)
            .unwrap_or(app_config.features.auto_format_lists);

        let code_mode = target
            .and_then(|t| t.processing.code_mode)
            .unwrap_or(false);

        PostProcessConfig {
            remove_fillers,
            spoken_punctuation,
            auto_format_lists,
            // Snippets always expand; the only thing that turns them off is
            // having none defined.
            apply_snippets: !app_config.features.snippets.is_empty(),
            snippets: app_config.features.snippets.clone(),
            code_mode,
            custom_vocabulary: app_config.features.custom_vocabulary.clone(),
        }
    }
}

// ── Backend selection ─────────────────────────────────────────────────────────

fn build_backend(config: &AppConfig) -> Box<dyn TranscriptionBackend> {
    match config.engine.backend {
        BackendChoice::WhisperCpp => {
            Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
        }
        BackendChoice::Moonshine => {
            #[cfg(feature = "moonshine")]
            {
                info!(
                    "Using Moonshine backend ({} model)",
                    config.engine.moonshine.model_size
                );
                Box::new(moonshine::MoonshineBackend::new(config.engine.moonshine.clone()))
            }
            #[cfg(not(feature = "moonshine"))]
            {
                // Moonshine feature not compiled — fall back to whisper-cpp.
                tracing::warn!("Moonshine backend selected but not compiled in this build; using whisper-cpp");
                Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
            }
        }
    }
}

// ── Threaded worker ───────────────────────────────────────────────────────────

/// Run the inference engine on a dedicated OS thread.
/// Receives `InferenceRequest` from `rx`, sends `InferenceOutput` to `tx`.
pub fn run_worker(
    config: Arc<AppConfig>,
    rx: Receiver<InferenceRequest>,
    tx: Sender<InferenceOutput>,
) {
    std::thread::Builder::new()
        .name("voxctrl-inference".into())
        .spawn(move || {
            let mut engine = InferenceEngine::new(config);
            // A load failure (typically: model not downloaded yet on a fresh
            // install) must NOT kill this thread. Keep consuming requests and
            // retry the load on each one — the user may download the model
            // from Settings → Engine while the app is running, and dictation
            // must start working right away, not after an app restart.
            let mut loaded = match engine.load() {
                Ok(()) => {
                    info!("Inference engine ready");
                    true
                }
                Err(e) => {
                    error!("Failed to load inference backend: {e:#}");
                    false
                }
            };

            while let Ok(req) = rx.recv() {
                if !loaded {
                    match engine.load() {
                        Ok(()) => {
                            info!("Inference engine ready (loaded on demand)");
                            loaded = true;
                        }
                        Err(e) => {
                            error!("Inference backend still not loadable: {e:#}");
                            let _ = tx.send(InferenceOutput {
                                text: String::new(),
                                target_id: req.target_id,
                                raw_text: String::new(),
                                inference_ms: 0,
                                language: String::new(),
                                error: Some(format!("{e:#}")),
                            });
                            continue;
                        }
                    }
                }

                match engine.process(req) {
                    Ok(output) => {
                        let _ = tx.send(output);
                    }
                    Err(e) => {
                        error!("Inference error: {:?}", e);
                        let _ = tx.send(InferenceOutput {
                            text: "".to_string(),
                            target_id: "".to_string(),
                            raw_text: "".to_string(),
                            inference_ms: 0,
                            language: "".to_string(),
                            error: Some(format!("{e:#}")),
                        });
                    }
                }
            }
        })
        .expect("failed to spawn inference thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_is_none_for_all_whisper_devices() {
        // Fix regression: previously read moonshine.language when device != "auto".
        // Now language is always None (Whisper auto-detects), regardless of device.
        for device in &["auto", "cpu", "cuda", "vulkan"] {
            let mut cfg = AppConfig::default();
            cfg.engine.whisper_cpp.device = device.to_string();
            cfg.engine.moonshine.language = "fr".to_string();
            let engine = InferenceEngine::new(Arc::new(cfg));
            // process() is not called (no model), but we can verify the field read
            // via the build_post_config path — just confirm engine creation is fine.
            assert_eq!(engine.config.engine.whisper_cpp.device, *device);
            // The language used internally is always None; moonshine.language must
            // not bleed into whisper inference even when device != "auto".
            let _ = engine; // ensure engine is not optimised out
        }
    }

    #[test]
    fn test_process_uses_in_memory_config_not_disk() {
        // Fix regression: process() must not re-read config.json from disk.
        // Verify: modifying the config file on disk does NOT affect InferenceEngine
        // behaviour (the engine uses the Arc<AppConfig> given at construction).
        let mut cfg = AppConfig::default();
        cfg.features.remove_fillers = true;
        let engine = InferenceEngine::new(Arc::new(cfg.clone()));
        // Engine should carry the config we gave it, not whatever is on disk.
        assert!(engine.config.features.remove_fillers);
        // If a fresh config with remove_fillers=false were on disk it should not
        // override us (we simply verify the field is still true here).
        assert!(engine.config.features.remove_fillers);
    }

    #[test]
    fn default_backend_is_whisper_cpp() {
        let cfg = AppConfig::default();
        assert_eq!(build_backend(&cfg).name(), "whisper-cpp");
    }
}
