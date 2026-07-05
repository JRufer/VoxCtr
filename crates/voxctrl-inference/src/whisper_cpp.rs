use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use anyhow::{bail, Context, Result};
use tracing::info;
use voxctrl_config::WhisperCppConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

// ── GGUF model resolution ─────────────────────────────────────────────────────

static GGUF_MAP: &[(&str, &[&str])] = &[
    ("tiny",           &["ggml-tiny-q5_1.bin",           "ggml-tiny.bin"]),
    ("tiny.en",        &["ggml-tiny.en-q5_1.bin",        "ggml-tiny.en.bin"]),
    ("base",           &["ggml-base-q5_1.bin",           "ggml-base.bin"]),
    ("base.en",        &["ggml-base.en-q5_1.bin",        "ggml-base.en.bin"]),
    ("small",          &["ggml-small-q5_1.bin",          "ggml-small.bin"]),
    ("small.en",       &["ggml-small.en-q5_1.bin",       "ggml-small.en.bin"]),
    ("medium",         &["ggml-medium-q5_0.bin",         "ggml-medium.bin"]),
    ("medium.en",      &["ggml-medium.en-q5_0.bin",      "ggml-medium.en.bin"]),
    ("large-v2",       &["ggml-large-v2-q5_0.bin",       "ggml-large-v2.bin"]),
    ("large-v3",       &["ggml-large-v3-q5_0.bin",       "ggml-large-v3.bin"]),
    ("large-v3-turbo", &["ggml-large-v3-turbo-q5_0.bin", "ggml-large-v3-turbo.bin"]),
];

const GGUF_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

/// Model sizes small enough (~75MB or less) to auto-download silently in the
/// background at first launch, so the app transcribes out of the box with no
/// manual step. Anything larger stays an explicit, user-initiated download —
/// pulling hundreds of MB to a few GB without asking would be a bad surprise
/// on a metered or slow connection.
pub fn is_small_auto_downloadable(size: &str) -> bool {
    matches!(size, "tiny" | "tiny.en")
}

// ── Backend ───────────────────────────────────────────────────────────────────

pub struct WhisperCppBackend {
    cfg: WhisperCppConfig,
    model_path: Option<PathBuf>,
    loaded: bool,

    // Model context — kept alive so the state's Arc ref remains valid.
    ctx: Option<whisper_rs::WhisperContext>,

    // Inference state — KV cache + attention buffers, reused across calls.
    // WhisperState holds Arc<WhisperInnerContext> so it is self-contained; no
    // lifetime trickery needed.  We lock during each transcribe() call (the
    // inference worker is single-threaded so there is never real contention).
    state: Mutex<Option<whisper_rs::WhisperState>>,
}

impl WhisperCppBackend {
    pub fn new(cfg: WhisperCppConfig) -> Self {
        Self {
            cfg,
            model_path: None,
            loaded: false,
            ctx: None,
            state: Mutex::new(None),
        }
    }

    pub fn default_model_dir() -> PathBuf {
        crate::util::models_base_dir()
    }

    fn resolve_model_path(&self) -> Result<PathBuf> {
        let size = &self.cfg.model_size;

        // Absolute path or ends with .bin → use directly
        if size.ends_with(".bin") || Path::new(size).is_absolute() {
            let p = PathBuf::from(size);
            if p.exists() {
                return Ok(p);
            }
            bail!("Model file not found: {}", p.display());
        }

        let model_dir = if self.cfg.model_dir.is_empty() {
            Self::default_model_dir()
        } else {
            crate::util::expand_tilde(&self.cfg.model_dir)
        };

        let candidates = GGUF_MAP
            .iter()
            .find(|(name, _)| *name == size.as_str())
            .map(|(_, files)| *files)
            .ok_or_else(|| anyhow::anyhow!("Unknown model size '{size}'"))?;

        for filename in candidates {
            let path = model_dir.join(filename);
            if path.exists() {
                return Ok(path);
            }
        }

        bail!(
            "Whisper model '{size}' is not downloaded (looked in {}). \
             Open Settings → Engine and click Download next to the model.",
            model_dir.display()
        )
    }

    fn threads(&self) -> u32 {
        if self.cfg.threads == 0 {
            (num_cpus() / 2).max(1)
        } else {
            self.cfg.threads
        }
    }
}

impl TranscriptionBackend for WhisperCppBackend {
    fn name(&self) -> &str {
        "whisper-cpp"
    }

    fn load(&mut self) -> Result<()> {
        static LOGGING_INIT: std::sync::Once = std::sync::Once::new();
        LOGGING_INIT.call_once(|| {
            whisper_rs::install_logging_hooks();
        });

        let path = self.resolve_model_path()?;
        info!("Loading whisper.cpp model: {}", path.display());

        let mut params = whisper_rs::WhisperContextParameters::default();
        params.use_gpu = self.cfg.device.to_lowercase() != "cpu";

        let ctx = whisper_rs::WhisperContext::new_with_params(path.to_str().unwrap(), params)
            .context("whisper-rs load")?;

        // Create the inference state once. Allocates KV cache + attention buffers
        // up front so transcribe() never has to reallocate them.
        let state = ctx.create_state().context("whisper state init")?;

        *self.state.lock().unwrap() = Some(state);
        self.ctx = Some(ctx);
        self.model_path = Some(path);
        self.loaded = true;
        Ok(())
    }

    fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
        if !self.loaded {
            bail!("Model not loaded");
        }
        let mut guard = self.state.lock().unwrap();
        let state = guard.as_mut().context("whisper state not initialised")?;
        transcribe_with_state(state, req, self.threads())
    }

    fn unload(&mut self) {
        // Drop state first so its Arc ref to the inner context is released before ctx.
        *self.state.lock().unwrap() = None;
        self.ctx = None;
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }
}

// ── whisper-rs transcription (reuses pre-allocated state) ────────────────────

fn transcribe_with_state(
    state: &mut whisper_rs::WhisperState,
    req: &TranscribeRequest,
    threads: u32,
) -> Result<TranscriptionResult> {
    use whisper_rs::{FullParams, SamplingStrategy};

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(threads as i32);
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_timestamps(false);

    if let Some(lang) = &req.language {
        params.set_language(Some(lang));
    }
    if let Some(prompt) = &req.initial_prompt {
        params.set_initial_prompt(prompt);
    }

    let t0 = Instant::now();
    state.full(params, &req.audio).context("whisper full")?;
    let inference_ms = t0.elapsed().as_millis() as u32;

    let n = state.full_n_segments();
    let mut parts = Vec::new();
    for i in 0..n {
        if let Some(segment) = state.get_segment(i) {
            if let Ok(text) = segment.to_str() {
                parts.push(text.trim().to_string());
            }
        }
    }
    let text = parts.join(" ");
    let language = req.language.clone().unwrap_or_else(|| "en".into());

    Ok(TranscriptionResult {
        text,
        language,
        language_probability: 1.0,
        duration_ms: (req.audio.len() as u32) / 16,
        inference_ms,
        word_timestamps: None,
    })
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

pub fn is_model_downloaded(size: &str, model_dir: &str) -> bool {
    let candidates = match GGUF_MAP.iter().find(|(name, _)| *name == size) {
        Some((_, files)) => *files,
        None => return false,
    };
    let dir = if model_dir.is_empty() {
        WhisperCppBackend::default_model_dir()
    } else {
        crate::util::expand_tilde(model_dir)
    };
    candidates.iter().any(|filename| dir.join(filename).exists())
}

/// Serializes calls to `download_model`. Guards against two independent
/// triggers racing on the same file — e.g. the silent startup auto-download of
/// the default "tiny" model firing at the same time the user clicks "Download"
/// in Settings before it has finished.
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn download_model(size: &str, model_dir: &str) -> Result<()> {
    let candidates = GGUF_MAP
        .iter()
        .find(|(name, _)| *name == size)
        .map(|(_, files)| *files)
        .ok_or_else(|| anyhow::anyhow!("Unknown model size '{size}'"))?;

    let model_dir = if model_dir.is_empty() {
        WhisperCppBackend::default_model_dir()
    } else {
        crate::util::expand_tilde(model_dir)
    };
    tokio::fs::create_dir_all(&model_dir).await?;

    let filename = candidates[0];
    let path = model_dir.join(filename);

    if path.exists() {
        return Ok(());
    }

    let _guard = DOWNLOAD_LOCK.lock().await;
    // Re-check after acquiring the lock: another caller may have just
    // finished downloading this exact file while we were waiting.
    if path.exists() {
        return Ok(());
    }

    let url = format!("{}{}", GGUF_BASE_URL, filename);
    info!("Downloading Whisper model: {}", url);

    let response = reqwest::get(&url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    tokio::fs::write(&path, bytes)
        .await
        .context("save model file")?;

    info!("Whisper model downloaded successfully to: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxctrl_config::WhisperCppConfig;

    // ── is_small_auto_downloadable ────────────────────────────────────────────

    #[test]
    fn test_is_small_auto_downloadable_tiny_variants() {
        assert!(is_small_auto_downloadable("tiny"));
        assert!(is_small_auto_downloadable("tiny.en"));
    }

    #[test]
    fn test_is_small_auto_downloadable_rejects_larger_models() {
        for size in ["base", "base.en", "small", "medium", "large-v2", "large-v3", "large-v3-turbo"] {
            assert!(!is_small_auto_downloadable(size), "{size} must not auto-download silently");
        }
    }

    #[test]
    fn test_is_small_auto_downloadable_rejects_unknown_and_paths() {
        assert!(!is_small_auto_downloadable("unknown-size"));
        assert!(!is_small_auto_downloadable("/tmp/custom.bin"));
    }

    #[test]
    fn test_new_backend() {
        let cfg = WhisperCppConfig {
            model_dir: "/tmp".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 4,
        };
        let backend = WhisperCppBackend::new(cfg);
        assert_eq!(backend.name(), "whisper-cpp");
        assert!(!backend.is_loaded());
    }

    #[test]
    fn test_threads_calculation() {
        // Explicit threads count
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 5,
        };
        let backend = WhisperCppBackend::new(cfg);
        assert_eq!(backend.threads(), 5);

        // Auto threads count (0)
        let cfg_auto = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend_auto = WhisperCppBackend::new(cfg_auto);
        assert!(backend_auto.threads() >= 1);
    }

    #[test]
    fn test_resolve_model_path_absolute() {
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "/tmp/nonexistent.bin".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        // Should bail because path does not exist
        assert!(backend.resolve_model_path().is_err());
    }

    #[test]
    fn test_resolve_model_path_unknown_size() {
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "invalid_size".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        assert!(backend.resolve_model_path().is_err());
    }

    #[test]
    fn test_transcribe_unloaded_error() {
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        let req = crate::backend::TranscribeRequest {
            audio: vec![0.0; 16000],
            language: None,
            word_timestamps: false,
            initial_prompt: None,
        };
        assert!(backend.transcribe(&req).is_err());
    }

    // ── model_dir tests ───────────────────────────────────────────────────────

    #[test]
    fn test_is_model_downloaded_default_dir_not_present() {
        // With empty model_dir (default) and no models on disk, returns false.
        // We isolate HOME to ensure default directory is empty.
        let old_home = std::env::var_os("HOME");
        let temp_home = tempfile::tempdir().expect("create temp home");
        let home = temp_home.path().to_path_buf();
        std::env::set_var("HOME", &home);

        let result = is_model_downloaded("tiny", "");

        if let Some(old) = old_home {
            std::env::set_var("HOME", old);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(!result);
    }

    #[test]
    fn test_is_model_downloaded_custom_dir_with_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("ggml-tiny-q5_1.bin");
        std::fs::File::create(&model_path)
            .unwrap()
            .write_all(b"fake")
            .unwrap();

        assert!(is_model_downloaded(
            "tiny",
            dir.path().to_str().unwrap()
        ));
    }

    #[test]
    fn test_is_model_downloaded_nonexistent_path() {
        // A path that does not exist on disk: no models found there.
        assert!(!is_model_downloaded("tiny", "/nonexistent/path/that/does/not/exist"));
    }

    #[test]
    fn test_is_model_downloaded_unknown_size() {
        // Unknown model size always returns false regardless of dir.
        assert!(!is_model_downloaded("unknown-size", ""));
        assert!(!is_model_downloaded("unknown-size", "/tmp"));
    }

    #[test]
    fn test_resolve_model_path_uses_custom_dir() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let model_path = dir.path().join("ggml-tiny-q5_1.bin");
        std::fs::File::create(&model_path)
            .unwrap()
            .write_all(b"fake")
            .unwrap();

        let cfg = WhisperCppConfig {
            model_dir: dir.path().to_str().unwrap().to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        let resolved = backend.resolve_model_path().expect("should resolve");
        assert_eq!(resolved, model_path);
    }

    #[test]
    fn test_resolve_model_path_falls_back_to_default_when_dir_empty() {
        // When model_dir is empty, resolve_model_path uses the default dir.
        // The default dir almost certainly does not contain models in CI, so we
        // just verify the error message mentions the default path rather than a
        // custom one.
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        let default_dir = WhisperCppBackend::default_model_dir();
        match backend.resolve_model_path() {
            Err(e) => assert!(
                e.to_string().contains(default_dir.to_str().unwrap()),
                "error should mention default dir: {e}"
            ),
            Ok(p) => {
                // If there happens to be a model on this machine, just check it's under the default dir.
                assert!(p.starts_with(&default_dir));
            }
        }
    }

    // Tilde expansion itself is covered by `crate::util` tests; this exercises
    // it through the whisper model-dir resolution path.
    #[test]
    fn test_is_model_downloaded_tilde_path() {
        use std::io::Write;
        
        let old_home = std::env::var_os("HOME");
        let temp_home = tempfile::tempdir().expect("create temp home");
        let home = temp_home.path().to_path_buf();
        
        std::env::set_var("HOME", &home);

        let dir = tempfile::tempdir_in(&home).expect("tempdir in home");
        let model_path = dir.path().join("ggml-tiny-q5_1.bin");
        std::fs::File::create(&model_path)
            .unwrap()
            .write_all(b"fake")
            .unwrap();

        // Construct a ~/... path pointing at the temp dir
        let rel = dir.path().strip_prefix(&home).unwrap();
        let tilde_path = format!("~/{}", rel.display());

        let result = is_model_downloaded("tiny", &tilde_path);

        if let Some(old) = old_home {
            std::env::set_var("HOME", old);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(result);
    }
}

