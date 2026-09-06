use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

// ── Engine sub-configs ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperCppConfig {
    /// Directory containing GGUF model files. Empty = platform default.
    pub model_dir: String,
    /// Model size name: "tiny", "base", "small", "medium", "large-v3", etc.
    pub model_size: String,
    /// "auto" | "cuda" | "vulkan" | "cpu"
    pub device: String,
    /// 0 = auto-detect (half of logical cores)
    pub threads: u32,
}

impl Default for WhisperCppConfig {
    fn default() -> Self {
        Self {
            model_dir: String::new(),
            // "tiny" (~75MB) is small enough to auto-download silently at
            // first launch (see src-tauri/src/lib.rs startup hook) so the app
            // transcribes out of the box with no manual download step. Users
            // who want more accuracy can pick a larger model in Settings.
            model_size: "tiny".into(),
            device: "auto".into(),
            threads: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoonshineConfig {
    /// "base" or "tiny"
    pub model_size: String,
    /// BCP-47 language code, e.g. "en"
    pub language: String,
}

impl Default for MoonshineConfig {
    fn default() -> Self {
        Self {
            model_size: "base".into(),
            language: "en".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BackendChoice {
    /// `alias = "auto"` migrates configs written when the backend could be
    /// left unset: auto-selection always resolved to whisper.cpp anyway, so
    /// those installs keep the backend they were already running.
    #[serde(alias = "auto")]
    WhisperCpp,
    Moonshine,
}

impl Default for BackendChoice {
    fn default() -> Self {
        Self::WhisperCpp
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineConfig {
    pub backend: BackendChoice,
    pub whisper_cpp: WhisperCppConfig,
    pub moonshine: MoonshineConfig,
}

// ── Audio ─────────────────────────────────────────────────────────────────────

fn default_gain() -> f32 {
    1.0
}

fn default_dynamic_stream() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub vad_threshold: f32,
    /// None = use default system device
    pub input_device_index: Option<u32>,
    /// Saved evdev device path, e.g. "/dev/input/event4" (Linux only)
    pub evdev_device: Option<String>,
    pub noise_suppression: bool,
    /// Linear gain multiplier applied before sending to inference (1.0 = unity)
    #[serde(default = "default_gain")]
    pub gain: f32,
    #[serde(default = "default_dynamic_stream")]
    pub dynamic_stream: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            vad_threshold: 0.5,
            input_device_index: None,
            evdev_device: None,
            noise_suppression: false,
            gain: 1.0,
            dynamic_stream: true,
        }
    }
}

fn default_auto_show_settings() -> bool {
    false
}

fn default_setup_completed() -> bool {
    true
}

fn default_show_notification() -> bool {
    false
}

fn default_overlay_position() -> String {
    "center".into()
}

fn default_overlay_monitor() -> String {
    "primary".into()
}

fn default_show_command_overlay() -> bool {
    true
}

fn default_command_overlay_duration_secs() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub show_overlay: bool,
    pub overlay_style: String,
    #[serde(default = "default_overlay_position")]
    pub overlay_position: String,
    #[serde(default = "default_overlay_monitor")]
    pub overlay_monitor: String,
    #[serde(default = "default_auto_show_settings")]
    pub auto_show_settings: bool,
    #[serde(default = "default_show_notification")]
    pub show_notification: bool,
    #[serde(default = "default_show_command_overlay")]
    pub show_command_overlay: bool,
    #[serde(default = "default_command_overlay_duration_secs")]
    pub command_overlay_duration_secs: u32,
    /// Whether the first-run setup wizard has been finished.
    ///
    /// The serde default is `true` on purpose: a config file written by an
    /// earlier VoxCtrl has no such field, and its owner has plainly already
    /// set the app up by hand. Only `UiConfig::default()` — reached when no
    /// config file exists at all — starts this at `false`, so the wizard runs
    /// exactly once, on a genuinely new machine.
    #[serde(default = "default_setup_completed")]
    pub setup_completed: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_overlay: true,
            overlay_style: "mono_bars".into(),
            overlay_position: "center".into(),
            overlay_monitor: "primary".into(),
            auto_show_settings: false,
            show_notification: false,
            show_command_overlay: true,
            command_overlay_duration_secs: 3,
            setup_completed: false,
        }
    }
}

// ── Features ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    pub remove_fillers: bool,
    pub custom_vocabulary: Vec<String>,
    pub spoken_punctuation: bool,
    pub auto_format_lists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_notification: Option<bool>,
    /// Map of trigger → expansion, e.g. {"addr" → "123 Main St"}
    pub snippets: std::collections::HashMap<String, String>,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            remove_fillers: true,
            custom_vocabulary: vec!["VoxCtrl".into()],
            spoken_punctuation: true,
            auto_format_lists: true,
            show_notification: None,
            snippets: std::collections::HashMap::new(),
        }
    }
}

// ── OpenAI API ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiMode {
    Clean,
    Formal,
    Casual,
    Bullet,
    Concise,
    Custom,
}

impl Default for OpenAiMode {
    fn default() -> Self {
        Self::Clean
    }
}

fn default_system_prompt() -> String {
    "Fix grammar and punctuation only. Return only the corrected text, no commentary.".into()
}

fn default_user_prompt() -> String {
    "{text}".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub enabled: bool,
    pub model: String,
    /// Preset that last populated the system prompt. Kept for UI convenience and
    /// backward compatibility; generation is driven by `system_prompt`/`user_prompt`.
    #[serde(default)]
    pub mode: OpenAiMode,
    /// Legacy single-prompt template (mode == Custom). Migrated into `user_prompt`.
    #[serde(default)]
    pub custom_prompt: Option<String>,
    /// System message sent to the model. Empty = no system message.
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// User message template. Must contain "{text}", which is replaced with the
    /// dictated text before being sent to the model.
    #[serde(default = "default_user_prompt")]
    pub user_prompt: String,
    /// Base URL of the OpenAI-compatible API server (a local server or a remote
    /// provider). May optionally include a `/v1` suffix.
    pub endpoint: String,
    /// Optional API key sent as a `Bearer` token. Required by most remote
    /// providers; usually unnecessary for a local server.
    #[serde(default)]
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "llama3.2:1b".into(),
            mode: OpenAiMode::Clean,
            custom_prompt: None,
            system_prompt: default_system_prompt(),
            user_prompt: default_user_prompt(),
            endpoint: "http://localhost:11434".into(),
            api_key: None,
            timeout_secs: 30,
        }
    }
}

// ── TTS ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsEngine {
    Piper,
    Espeak,
    PocketTts,
    InflectMicro,
    #[serde(rename = "breeze_tts_2", alias = "breeze_tts2")]
    BreezeTts2,
}

impl Default for TtsEngine {
    fn default() -> Self {
        Self::Piper
    }
}

fn default_pocket_tts_voice() -> String {
    "alba".into()
}

fn default_breeze_tts_2_speaker_prompt() -> String {
    "A calm and clear female voice speaking at a natural pace".into()
}

fn default_tts_speed() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PocketTtsConfig {
    /// Bundled reference voice name, e.g. "alba", "anna", "vera", "charles", "michael",
    /// or the filename stem of a custom clip dropped into `voice_dir`.
    #[serde(default = "default_pocket_tts_voice")]
    pub voice: String,
    /// Pre-warm model on startup so the first synthesis is instant
    #[serde(default)]
    pub prewarm: bool,
    /// Where this engine's token used to live, kept only so a config written
    /// before `tts.hf_token` existed can be migrated. Cleared once lifted, and
    /// never written back.
    #[serde(default, rename = "hf_token", skip_serializing_if = "Option::is_none")]
    pub legacy_hf_token: Option<String>,
    /// Directory scanned for custom voice clips (`<id>.wav`). Empty = platform default
    /// (`~/.local/share/voxctrl/pocket-tts-voices/`).
    #[serde(default)]
    pub voice_dir: String,
}

impl Default for PocketTtsConfig {
    fn default() -> Self {
        Self {
            voice: default_pocket_tts_voice(),
            prewarm: false,
            legacy_hf_token: None,
            voice_dir: String::new(),
        }
    }
}

/// Breeze-TTS-2 (BreezeBlue) — bilingual neural text-to-speech with natural-language
/// voice design speaker prompts. Model weights are gated on HuggingFace under a
/// non-commercial research license.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreezeTts2Config {
    /// Voice selection mode: "prompt" (Voice Design) or "clone" (Cloned Voice Clip)
    #[serde(default = "default_breeze_voice_mode")]
    pub voice_mode: String,
    /// Selected cloned voice ID from the shared voice folder (e.g. "alba", "my_voice")
    #[serde(default)]
    pub cloned_voice: String,
    /// Shared voice directory for custom clips (empty = platform default `~/.local/share/voxctrl/pocket-tts-voices/`)
    #[serde(default)]
    pub voice_dir: String,
    /// Text prompt describing the voice of the speaker (Voice Design)
    #[serde(default = "default_breeze_tts_2_speaker_prompt")]
    pub speaker_prompt: String,
    /// Directory containing model weights & tokenizer. Empty = platform default
    /// (`~/.local/share/voxctrl/models/breeze-tts-2/`).
    #[serde(default)]
    pub model_dir: String,
    /// Where this engine's token used to live; see
    /// [`PocketTtsConfig::legacy_hf_token`].
    #[serde(default, rename = "hf_token", skip_serializing_if = "Option::is_none")]
    pub legacy_hf_token: Option<String>,
    /// Pre-warm model on startup so the first synthesis is instant
    #[serde(default)]
    pub prewarm: bool,
    /// Enable GPU acceleration (CUDA)
    #[serde(default)]
    pub gpu: bool,
}

fn default_breeze_voice_mode() -> String {
    "prompt".into()
}

impl Default for BreezeTts2Config {
    fn default() -> Self {
        Self {
            voice_mode: default_breeze_voice_mode(),
            cloned_voice: String::new(),
            voice_dir: String::new(),
            speaker_prompt: default_breeze_tts_2_speaker_prompt(),
            model_dir: String::new(),
            legacy_hf_token: None,
            prewarm: false,
            gpu: false,
        }
    }
}

fn default_inflect_micro_seed() -> u64 {
    0
}

fn default_inflect_micro_noise_scale() -> f32 {
    0.667
}

/// Inflect-Micro-v2 (<https://huggingface.co/owensong/Inflect-Micro-v2>) — a
/// ~9.4M-parameter VITS-family model with a single fixed English voice, so
/// unlike Piper and Pocket-TTS there is no voice to pick. Speaking rate comes
/// from the shared [`TtsConfig::speed`]; what remains model-specific is the
/// sampling seed and the two VITS noise scales.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflectMicroConfig {
    /// Directory holding the ONNX graphs and phoneme vocabulary. Empty = platform
    /// default (`~/.local/share/voxctrl/models/inflect-micro/`).
    #[serde(default)]
    pub model_dir: String,
    /// Seed for the stochastic duration predictor and latent sampling. The model
    /// is deterministic for a fixed seed, so a stable value keeps repeated
    /// synthesis of the same text identical.
    #[serde(default = "default_inflect_micro_seed")]
    pub seed: u64,
    /// Latent sampling temperature, fed to `decode.onnx` as `noise_scale`.
    /// Higher is more varied, lower is flatter. Valid range is 0.0 to 1.0.
    #[serde(default = "default_inflect_micro_noise_scale")]
    pub noise_scale: f32,
    /// Pre-warm the ONNX sessions on startup so the first synthesis is instant.
    #[serde(default)]
    pub prewarm: bool,
}

impl Default for InflectMicroConfig {
    fn default() -> Self {
        Self {
            model_dir: String::new(),
            seed: default_inflect_micro_seed(),
            noise_scale: default_inflect_micro_noise_scale(),
            prewarm: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub enabled: bool,
    pub engine: TtsEngine,
    /// Voice name for Piper, e.g. "en-us-lessac-medium"
    pub voice: String,
    /// Directory containing Piper voice files. Empty = platform default.
    #[serde(default)]
    pub voice_dir: String,
    /// Key(s) that stop TTS playback, e.g. ["KEY_ESC"]
    pub stop_key: Vec<String>,
    pub response_overlay: bool,
    #[serde(default = "default_tts_speed")]
    pub speed: f32,
    /// Enable GPU acceleration for Piper
    #[serde(default)]
    pub gpu: bool,
    /// The single HuggingFace access token for every gated model VoxCtrl
    /// downloads (Pocket-TTS and Breeze-TTS-2 today). One token, one place —
    /// entering it in Settings or in the setup wizard writes here.
    #[serde(default)]
    pub hf_token: Option<String>,
    #[serde(default)]
    pub pocket_tts: PocketTtsConfig,
    #[serde(default)]
    pub inflect_micro: InflectMicroConfig,
    #[serde(default)]
    pub breeze_tts_2: BreezeTts2Config,
    #[serde(default)]
    pub snippets: std::collections::HashMap<String, String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // eSpeak-NG is a system package installed by the setup flow
            // (installer.rs) — it works immediately with no model download,
            // unlike Piper (needs a voice download) or Pocket-TTS (needs a
            // multi-hundred-MB gated model download).
            engine: TtsEngine::Espeak,
            voice: "en-us-lessac-medium".into(),
            voice_dir: String::new(),
            stop_key: vec!["KEY_ESC".into()],
            response_overlay: true,
            speed: 1.0,
            gpu: false,
            hf_token: None,
            pocket_tts: PocketTtsConfig::default(),
            inflect_micro: InflectMicroConfig::default(),
            breeze_tts_2: BreezeTts2Config::default(),
            snippets: {
                let mut map = std::collections::HashMap::new();
                map.insert("VoxCtrl".into(), "Voks Con-trol".into());
                map.insert("voxctrl".into(), "Voks Con-trol".into());
                map.insert("Vox Control".into(), "Voks Con-trol".into());
                map
            },
        }
    }
}

// ── MCP ───────────────────────────────────────────────────────────────────────

fn default_visual_feedback() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub server_enabled: bool,
    pub record_timeout: f64,
    #[serde(default = "default_visual_feedback")]
    pub visual_feedback: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            server_enabled: false,
            record_timeout: 15.0,
            visual_feedback: true,
        }
    }
}

// ── Updates ───────────────────────────────────────────────────────────────────

fn default_auto_check() -> bool {
    true
}

/// Automatic update checking.
///
/// This is the only thing in VoxCtrl that reaches the network without being
/// asked to: on launch it fetches the public GitHub releases API to see whether
/// a newer version has been published. The request carries no identifier of any
/// kind, and `auto_check = false` stops it entirely — see `docs/privacy.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Check GitHub for a newer release when the app starts.
    #[serde(default = "default_auto_check")]
    pub auto_check: bool,
    /// A version the user chose to skip, so the same release is not raised
    /// with them at every launch. A newer one still is.
    #[serde(default)]
    pub skipped_version: Option<String>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_check: true,
            skipped_version: None,
        }
    }
}

// ── Root config ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub engine: EngineConfig,
    pub audio: AudioConfig,
    pub ui: UiConfig,
    pub features: FeaturesConfig,
    /// `alias = "ollama"` keeps configs written before the OpenAI-API rename loading.
    #[serde(alias = "ollama")]
    pub openai: OpenAiConfig,
    pub tts: TtsConfig,
    pub mcp: McpConfig,
    /// Added after 0.3.10, so it must default rather than fail configs written
    /// before it existed — every one of which would otherwise fall back to
    /// defaults wholesale and lose the user's settings.
    #[serde(default)]
    pub updates: UpdateConfig,
}

/// Lift a HuggingFace token stored per engine onto the single `tts.hf_token`,
/// clearing the old copies. Returns whether anything moved, so the caller
/// knows to rewrite the file.
///
/// Pocket-TTS and Breeze-TTS-2 each used to hold their own copy, synchronized
/// on load; they now share one key, and the same token downloads both.
fn migrate_hf_token(data: &mut AppConfig) -> bool {
    let legacy = data
        .tts
        .pocket_tts
        .legacy_hf_token
        .take()
        .or_else(|| data.tts.breeze_tts_2.legacy_hf_token.take());
    data.tts.breeze_tts_2.legacy_hf_token = None;

    let Some(token) = legacy else { return false };
    if data.tts.hf_token.is_none() {
        data.tts.hf_token = Some(token);
    }
    true
}

/// Read a config file, keeping every section that parses.
///
/// serde is all-or-nothing: one unreadable value anywhere in the file — a
/// backend spelled `whisper_cpp` instead of `whisper-cpp`, a hand-edit, a
/// section written by a newer VoxCtrl with a stricter type — used to fail the
/// whole deserialize, and the app started on wholesale defaults. Every unrelated
/// setting in the file silently reverted for that run, and the next save wrote
/// the defaults back over the user's file for good.
///
/// So a failure is retried section by section: each top-level key is applied to
/// the defaults on its own, and one that will not deserialize is dropped with a
/// warning naming it. A bad `engine` block costs the engine settings and nothing
/// else. Only a file that is not a JSON object at all falls back entirely.
fn parse_tolerant(text: &str) -> AppConfig {
    let whole_file_error = match serde_json::from_str::<AppConfig>(text) {
        Ok(cfg) => return cfg,
        Err(e) => e,
    };

    let Ok(serde_json::Value::Object(file)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        tracing::warn!("Failed to load config, using defaults: {whole_file_error}");
        return AppConfig::default();
    };

    tracing::warn!(
        "Config did not load as a whole ({whole_file_error}); \
         recovering it section by section"
    );

    let Ok(serde_json::Value::Object(mut merged)) = serde_json::to_value(AppConfig::default())
    else {
        return AppConfig::default();
    };

    for (key, value) in file {
        // A key with no counterpart in the defaults is one serde would ignore
        // anyway — a setting from a newer or older VoxCtrl. Leave it be.
        if !merged.contains_key(&key) {
            continue;
        }
        let mut candidate = merged.clone();
        candidate.insert(key.clone(), value);
        match serde_json::from_value::<AppConfig>(serde_json::Value::Object(candidate.clone())) {
            Ok(_) => merged = candidate,
            Err(e) => tracing::warn!(
                "Config section '{key}' could not be read ({e}); it falls back to \
                 defaults, and the rest of the file is kept"
            ),
        }
    }

    serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default()
}

// ── Config manager ────────────────────────────────────────────────────────────

pub struct Config {
    pub data: AppConfig,
    path: PathBuf,
}

impl Config {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let mut data = if path.exists() {
            match std::fs::read_to_string(&path).map_err(ConfigError::Io) {
                Ok(text) => parse_tolerant(&text),
                Err(e) => {
                    tracing::warn!("Failed to read config, using defaults: {e}");
                    AppConfig::default()
                }
            }
        } else {
            AppConfig::default()
        };

        // Migrate show_notification from legacy features to ui struct if present
        if let Some(legacy_notif) = data.features.show_notification {
            data.ui.show_notification = legacy_notif;
            data.features.show_notification = None;
            // Instantly persist the migrated clean configuration to clean up the JSON
            let clean_config = Self { data: data.clone(), path: path.clone() };
            if let Err(e) = clean_config.save() {
                tracing::error!("Failed to save clean migrated config: {e}");
            }
        }

        // Migrate legacy "KEY_ESCAPE" → "KEY_ESC" (evdev crate uses KEY_ESC as the
        // canonical debug name via stringify!(KEY_ESC)).
        let needs_escape_fix = data.tts.stop_key.iter().any(|k| k == "KEY_ESCAPE");
        if needs_escape_fix {
            data.tts.stop_key = data.tts.stop_key
                .into_iter()
                .map(|k| if k == "KEY_ESCAPE" { "KEY_ESC".to_string() } else { k })
                .collect();
            let clean_config = Self { data: data.clone(), path: path.clone() };
            if let Err(e) = clean_config.save() {
                tracing::error!("Failed to save migrated stop_key: {e}");
            }
        }

        // Migrate legacy default OpenAI timeout (8s) to the new default (30s) to prevent timeouts
        if data.openai.timeout_secs == 8 {
            data.openai.timeout_secs = 30;
            let clean_config = Self { data: data.clone(), path: path.clone() };
            if let Err(e) = clean_config.save() {
                tracing::error!("Failed to save migrated OpenAI timeout: {e}");
            }
        }

        // Migrate the legacy single `custom_prompt` (used when mode == Custom) into the
        // new `user_prompt` field, then clear it so this runs only once.
        if let Some(legacy_prompt) = data.openai.custom_prompt.take() {
            if !legacy_prompt.trim().is_empty() {
                data.openai.user_prompt = legacy_prompt;
                // The legacy custom prompt carried the full instruction, so drop the
                // default grammar-fix system prompt to preserve the old behavior.
                data.openai.system_prompt = String::new();
            }
            let clean_config = Self { data: data.clone(), path: path.clone() };
            if let Err(e) = clean_config.save() {
                tracing::error!("Failed to save migrated OpenAI custom prompt: {e}");
            }
        }

        // One token now serves every gated model; older configs carry a copy
        // per engine. Rewrite the file so the duplicates go away for good.
        if migrate_hf_token(&mut data) {
            let migrated = Self { data: data.clone(), path: path.clone() };
            if let Err(e) = migrated.save() {
                tracing::error!("Failed to save migrated HuggingFace token: {e}");
            }
        }

        Self { data, path }
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.path)?;
            f.write_all(json.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn reload(&mut self) {
        *self = Self::load();
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::load()
    }
}

// ── Path utilities ────────────────────────────────────────────────────────────

/// Find the executable `name` the same way spawning it will.
///
/// This exists to answer one question — will `Command::new(name)` work? — so it
/// has to search what `Command` searches. On Unix that is `$PATH`.
///
/// On Windows it is not. `std`'s resolver appends `.exe` and then tries, in
/// order: the directory of the running executable, the system directory
/// (`System32`), the Windows directory, and only then `PATH`. Searching `PATH`
/// alone reported "not found" for anything living in the first three — every
/// tool in System32 among them — while spawning it worked. An Exec target
/// whose Test button contradicted the target actually running is how that
/// surfaced.
///
/// Note `PATHEXT` is deliberately *not* consulted: `CreateProcessW` appends
/// `.exe` and nothing else when searching, so a `.bat` or `.cmd` found by name
/// alone could not be spawned anyway, and reporting it as reachable would trade
/// one wrong answer for its mirror image.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    // "If the file name does not contain an extension, .exe is appended."
    let search_name: std::borrow::Cow<str> = if cfg!(target_os = "windows") && !name.contains('.') {
        format!("{name}.exe").into()
    } else {
        name.into()
    };

    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                dirs.push(parent.to_path_buf());
            }
        }
        // GetSystemDirectoryW / GetWindowsDirectoryW, without taking a Win32
        // dependency for two paths that are derived from one another.
        if let Some(root) = std::env::var_os("SystemRoot") {
            let root = PathBuf::from(root);
            dirs.push(root.join("System32"));
            dirs.push(root);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }

    dirs.into_iter()
        .map(|dir| dir.join(search_name.as_ref()))
        .find(|p| p.is_file())
}

// ── Validation ────────────────────────────────────────────────────────────────

static VALID_MODEL_SIZES: &[&str] = &[
    "tiny", "tiny.en", "base", "base.en", "small", "small.en",
    "medium", "medium.en", "large-v2", "large-v3", "large-v3-turbo",
];

pub fn validate(cfg: &AppConfig) -> Vec<String> {
    let mut errors = Vec::new();

    if !VALID_MODEL_SIZES.contains(&cfg.engine.whisper_cpp.model_size.as_str())
        && !cfg.engine.whisper_cpp.model_size.ends_with(".bin")
        && !std::path::Path::new(&cfg.engine.whisper_cpp.model_size).is_absolute()
    {
        errors.push(format!(
            "Unknown whisper_cpp model_size '{}'. Valid: {:?}",
            cfg.engine.whisper_cpp.model_size, VALID_MODEL_SIZES
        ));
    }

    if !["auto", "cuda", "vulkan", "cpu"]
        .contains(&cfg.engine.whisper_cpp.device.as_str())
    {
        errors.push(format!(
            "Invalid whisper_cpp device '{}'. Use: auto, cuda, vulkan, cpu",
            cfg.engine.whisper_cpp.device
        ));
    }

    if cfg.audio.vad_threshold < 0.0 || cfg.audio.vad_threshold > 1.0 {
        errors.push(format!(
            "vad_threshold {} out of range [0.0, 1.0]",
            cfg.audio.vad_threshold
        ));
    }

    errors
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_name_on_path_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let extension = if cfg!(target_os = "windows") { ".exe" } else { "" };
        let name = format!("voxctrl_path_probe{extension}");
        std::fs::write(dir.path().join(&name), b"").unwrap();

        let _guard = PathGuard::prepending(dir.path());
        assert_eq!(
            find_in_path("voxctrl_path_probe").as_deref(),
            Some(dir.path().join(&name).as_path())
        );
    }

    #[test]
    fn a_name_that_is_on_no_searched_directory_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = PathGuard::prepending(dir.path());
        assert_eq!(find_in_path("voxctrl_definitely_not_here_9f3a"), None);
    }

    /// The directories Windows searches before `PATH` are exactly the gap this
    /// function used to have: `Command::new` finds a System32 tool, and looking
    /// only at `PATH` said it did not exist.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_system_directory_tool_is_found_even_when_path_is_empty() {
        let _guard = PathGuard::replacing_with_nothing();
        let found = find_in_path("where").expect("where.exe lives in System32");
        assert!(found.is_file());
    }

    /// Puts a directory at the front of `PATH` and restores it on drop.
    struct PathGuard(Option<std::ffi::OsString>);

    impl PathGuard {
        #[cfg(target_os = "windows")]
        fn replacing_with_nothing() -> Self {
            let previous = std::env::var_os("PATH");
            std::env::set_var("PATH", "");
            Self(previous)
        }

        fn prepending(dir: &std::path::Path) -> Self {
            let previous = std::env::var_os("PATH");
            let mut entries = vec![dir.to_path_buf()];
            if let Some(existing) = &previous {
                entries.extend(std::env::split_paths(existing));
            }
            std::env::set_var("PATH", std::env::join_paths(entries).unwrap());
            Self(previous)
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => std::env::set_var("PATH", previous),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    use super::*;

    // ── parse_tolerant ────────────────────────────────────────────────────────

    /// A file with nothing wrong with it must take the ordinary path and come
    /// back exactly as written.
    #[test]
    fn a_valid_config_parses_whole() {
        let cfg = parse_tolerant(
            r#"{"engine": {"backend": "moonshine",
                           "whisper_cpp": {"model_dir": "", "model_size": "small",
                                           "device": "auto", "threads": 0},
                           "moonshine": {"model_size": "base", "language": "en"}},
                "audio": {"vad_threshold": 0.65, "input_device_index": null,
                          "evdev_device": null, "noise_suppression": true,
                          "gain": 1.6, "dynamic_stream": true}}"#,
        );
        assert_eq!(cfg.engine.backend, BackendChoice::Moonshine);
        assert_eq!(cfg.engine.whisper_cpp.model_size, "small");
        assert_eq!(cfg.audio.gain, 1.6);
    }

    /// The failure this exists for: `whisper_cpp` where the enum spells it
    /// `whisper-cpp`. The engine section is lost, and every unrelated setting
    /// in the file survives — which is the opposite of what used to happen.
    #[test]
    fn one_bad_section_does_not_take_the_rest_of_the_file_with_it() {
        let cfg = parse_tolerant(
            r#"{"engine": {"backend": "whisper_cpp"},
                "audio": {"vad_threshold": 0.65, "input_device_index": null,
                          "evdev_device": null, "noise_suppression": true,
                          "gain": 1.6, "dynamic_stream": true},
                "ui": {"show_overlay": false, "overlay_style": "pulse",
                       "overlay_position": "top", "overlay_monitor": "primary",
                       "auto_show_settings": false, "show_notification": false,
                       "show_command_overlay": true, "command_overlay_duration_secs": 3,
                       "setup_completed": true}}"#,
        );

        // Kept.
        assert_eq!(cfg.audio.gain, 1.6);
        assert!(cfg.audio.noise_suppression);
        assert_eq!(cfg.ui.overlay_style, "pulse");
        assert!(!cfg.ui.show_overlay);

        // Lost, because it is the section that would not read.
        assert_eq!(cfg.engine.backend, BackendChoice::default());
    }

    /// A key the running build knows nothing about is left alone rather than
    /// counted as a failed section — it is how a config survives a downgrade.
    #[test]
    fn an_unknown_top_level_key_is_ignored() {
        let cfg = parse_tolerant(
            r#"{"engine": {"backend": "whisper_cpp"},
                "some_future_section": {"whatever": 1},
                "features": {"remove_fillers": false, "custom_vocabulary": [],
                             "spoken_punctuation": true, "auto_format_lists": true,
                             "snippets": {}}}"#,
        );
        assert!(!cfg.features.remove_fillers);
    }

    /// Not a JSON object at all — there are no sections to recover, so this is
    /// the one case that still falls back wholesale.
    #[test]
    fn a_file_that_is_not_an_object_falls_back_to_defaults() {
        let cfg = parse_tolerant("[1, 2, 3]");
        assert_eq!(cfg.engine.backend, BackendChoice::default());
        assert_eq!(cfg.audio.gain, AudioConfig::default().gain);
    }

    fn tts_json(body: &str) -> TtsConfig {
        serde_json::from_str(body).expect("tts config should parse")
    }

    /// A config written when each engine carried its own copy of the token
    /// must come back with one token on `tts`, and the file it writes next
    /// must hold that token exactly once.
    #[test]
    fn migrates_per_engine_hf_tokens_onto_one_key() {
        let mut data = AppConfig::default();
        data.tts = tts_json(
            r#"{"enabled": true, "engine": "pocket_tts", "voice": "v",
                "stop_key": ["KEY_ESC"], "response_overlay": true,
                "pocket_tts": {"voice": "alba", "hf_token": "hf_from_pocket"},
                "breeze_tts_2": {"hf_token": "hf_from_pocket"}}"#,
        );
        assert_eq!(
            data.tts.pocket_tts.legacy_hf_token.as_deref(),
            Some("hf_from_pocket"),
            "the old location must still parse"
        );

        assert!(migrate_hf_token(&mut data));

        assert_eq!(data.tts.hf_token.as_deref(), Some("hf_from_pocket"));
        assert!(data.tts.pocket_tts.legacy_hf_token.is_none());
        assert!(data.tts.breeze_tts_2.legacy_hf_token.is_none());

        let written = serde_json::to_string(&data.tts).unwrap();
        assert_eq!(
            written.matches("hf_token").count(),
            1,
            "the token must be stored once, not per engine: {written}"
        );
    }

    /// A token set only on Breeze is lifted too — either copy will do.
    #[test]
    fn migrates_a_breeze_only_token() {
        let mut data = AppConfig::default();
        data.tts = tts_json(
            r#"{"enabled": false, "engine": "espeak", "voice": "v", "stop_key": [],
                "response_overlay": true, "breeze_tts_2": {"hf_token": "hf_from_breeze"}}"#,
        );

        assert!(migrate_hf_token(&mut data));
        assert_eq!(data.tts.hf_token.as_deref(), Some("hf_from_breeze"));
    }

    /// A config that already has the single key keeps it, and needs no rewrite.
    #[test]
    fn a_config_with_one_token_is_left_alone() {
        let mut data = AppConfig::default();
        data.tts = tts_json(
            r#"{"enabled": false, "engine": "espeak", "voice": "v", "stop_key": [],
                "response_overlay": true, "hf_token": "hf_single"}"#,
        );

        assert!(!migrate_hf_token(&mut data), "nothing to migrate");
        assert_eq!(data.tts.hf_token.as_deref(), Some("hf_single"));
        assert_eq!(
            serde_json::to_string(&data.tts).unwrap().matches("hf_token").count(),
            1
        );
    }

    /// The single key wins over a stale per-engine copy rather than being
    /// overwritten by it.
    #[test]
    fn the_single_token_wins_over_a_legacy_copy() {
        let mut data = AppConfig::default();
        data.tts = tts_json(
            r#"{"enabled": false, "engine": "espeak", "voice": "v", "stop_key": [],
                "response_overlay": true, "hf_token": "hf_current",
                "pocket_tts": {"hf_token": "hf_stale"}}"#,
        );

        assert!(migrate_hf_token(&mut data));
        assert_eq!(data.tts.hf_token.as_deref(), Some("hf_current"));
        assert!(data.tts.pocket_tts.legacy_hf_token.is_none());
    }


    /// Configs written before the Backend dropdown lost its "Auto-detect"
    /// entry still say `"auto"`. They must keep loading, on whisper.cpp —
    /// which is what auto-selection resolved to in every case — rather than
    /// failing the whole config back to defaults.
    #[test]
    fn legacy_auto_backend_loads_as_whisper_cpp() {
        let parsed: BackendChoice = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(parsed, BackendChoice::WhisperCpp);
        assert_eq!(BackendChoice::default(), BackendChoice::WhisperCpp);
    }

    #[test]
    fn backend_choice_serializes_kebab_case() {
        assert_eq!(
            serde_json::to_string(&BackendChoice::WhisperCpp).unwrap(),
            r#""whisper-cpp""#
        );
        assert_eq!(
            serde_json::to_string(&BackendChoice::Moonshine).unwrap(),
            r#""moonshine""#
        );
    }

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert!(!cfg.ui.auto_show_settings);
        assert!(!cfg.ui.show_notification);
        assert_eq!(cfg.ui.overlay_style, "mono_bars");
        assert_eq!(cfg.ui.overlay_position, "center");
        assert_eq!(cfg.ui.overlay_monitor, "primary");
        assert!(cfg.features.show_notification.is_none());
    }

    #[test]
    fn test_legacy_notification_migration() {
        let legacy_json = r#"{
            "engine": {
                "backend": "auto",
                "whisper_cpp": {
                    "model_dir": "",
                    "model_size": "large-v3",
                    "device": "auto",
                    "threads": 0
                },
                "moonshine": {
                    "model_size": "base",
                    "language": "en"
                }
            },
            "audio": {
                "vad_threshold": 0.5,
                "input_device_index": null,
                "evdev_device": null,
                "noise_suppression": false,
                "gain": 1.0,
                "dynamic_stream": true
            },
            "ui": {
                "show_overlay": true,
                "overlay_style": "voice_card"
            },
            "features": {
                "remove_fillers": true,
                "custom_vocabulary": [],
                "spoken_punctuation": true,
                "auto_format_lists": true,
                "show_notification": true,
                "snippets": {}
            },
            "openai": {
                "enabled": false,
                "model": "llama3.2:1b",
                "mode": "clean",
                "custom_prompt": null,
                "endpoint": "http://localhost:11434",
                "timeout_secs": 8
            },
            "tts": {
                "enabled": false,
                "engine": "piper",
                "voice": "en-us-lessac-medium",
                "stop_key": ["KEY_ESC"],
                "response_overlay": true
            },
            "mcp": {
                "server_enabled": false,
                "record_timeout": 15.0
            }
        }"#;

        let parsed: AppConfig = serde_json::from_str(legacy_json).unwrap();
        assert!(parsed.features.show_notification.is_some());
        assert_eq!(parsed.features.show_notification, Some(true));

        // Create a temporary config path to test Config::load migration logic
        let temp_dir = tempfile::tempdir().unwrap();
        let config_file_path = temp_dir.path().join("config.json");
        std::fs::write(&config_file_path, legacy_json).unwrap();

        let config = Config {
            data: parsed,
            path: config_file_path.clone(),
        };

        // Trigger load which executes the migration
        let _migrated_config = Config::load();
        
        // Assertions on the loaded instance
        let mut custom_config = Config {
            data: config.data.clone(),
            path: config_file_path.clone(),
        };
        if let Some(legacy_notif) = custom_config.data.features.show_notification {
            custom_config.data.ui.show_notification = legacy_notif;
            custom_config.data.features.show_notification = None;
            custom_config.save().unwrap();
        }

        assert!(custom_config.data.ui.show_notification);
        assert!(custom_config.data.features.show_notification.is_none());

        // Re-read file to verify the JSON content no longer has features.show_notification
        let re_read_content = std::fs::read_to_string(&config_file_path).unwrap();
        assert!(re_read_content.contains(r#""show_notification": true"#));
        assert!(!re_read_content.contains(r#""features": {
    "remove_fillers": true,
    "custom_vocabulary": [],
    "spoken_punctuation": true,
    "auto_format_lists": true,
    "show_notification": true"#));
    }

    #[test]
    fn test_fresh_install_starts_with_the_wizard_pending() {
        // No config file on disk means a machine that has never run VoxCtrl,
        // so the first-run wizard has to be pending.
        let cfg = AppConfig::default();
        assert!(!cfg.ui.setup_completed);
    }

    #[test]
    fn test_existing_config_file_never_reopens_the_wizard() {
        // A config written by an earlier VoxCtrl has no `setup_completed` key.
        // Its owner has plainly already set the app up, so deserializing must
        // treat the missing field as "done" rather than ambushing them with a
        // setup wizard on an upgrade.
        let legacy_json = r#"{
            "show_overlay": true,
            "overlay_style": "waveform",
            "auto_show_settings": true,
            "show_notification": false
        }"#;

        let parsed: UiConfig = serde_json::from_str(legacy_json).unwrap();
        assert!(parsed.setup_completed);
    }

    #[test]
    fn test_setup_completed_round_trips() {
        let mut cfg = AppConfig::default();
        assert!(!cfg.ui.setup_completed);

        cfg.ui.setup_completed = true;
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(back.ui.setup_completed);

        cfg.ui.setup_completed = false;
        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(
            !back.ui.setup_completed,
            "an explicit false must survive a save/load cycle, or a user who \
             quits the wizard would never see it again"
        );
    }

    #[test]
    fn test_ui_config_position_monitor_defaults() {
        let partial_json = r#"{
            "show_overlay": true,
            "overlay_style": "waveform",
            "auto_show_settings": true,
            "show_notification": false
        }"#;

        let parsed: UiConfig = serde_json::from_str(partial_json).unwrap();
        assert_eq!(parsed.overlay_position, "center");
        assert_eq!(parsed.overlay_monitor, "primary");
    }

    #[test]
    fn test_openai_prompt_defaults_for_legacy_config() {
        // Legacy config without system_prompt / user_prompt keys must deserialize
        // with the new prompt defaults applied via serde defaults.
        let legacy_openai = r#"{
            "enabled": true,
            "model": "llama3.2:1b",
            "mode": "clean",
            "custom_prompt": null,
            "endpoint": "http://localhost:11434",
            "timeout_secs": 30
        }"#;

        let parsed: OpenAiConfig = serde_json::from_str(legacy_openai).unwrap();
        assert_eq!(parsed.user_prompt, "{text}");
        assert!(parsed.system_prompt.contains("Fix grammar"));
        assert_eq!(parsed.api_key, None);
    }

    #[test]
    fn test_openai_timeout_migration() {
        let mut default_cfg = AppConfig::default();
        default_cfg.openai.timeout_secs = 8;

        let legacy_json = serde_json::to_string(&default_cfg).unwrap();

        let parsed: AppConfig = serde_json::from_str(&legacy_json).unwrap();
        assert_eq!(parsed.openai.timeout_secs, 8);

        let temp_dir = tempfile::tempdir().unwrap();
        let config_file_path = temp_dir.path().join("config.json");
        std::fs::write(&config_file_path, &legacy_json).unwrap();

        let mut config = Config {
            data: parsed,
            path: config_file_path.clone(),
        };

        if config.data.openai.timeout_secs == 8 {
            config.data.openai.timeout_secs = 30;
            config.save().unwrap();
        }

        assert_eq!(config.data.openai.timeout_secs, 30);

        let re_read_content = std::fs::read_to_string(&config_file_path).unwrap();
        assert!(re_read_content.contains(r#""timeout_secs": 30"#));
    }

    #[test]
    fn test_breeze_tts_2_serde() {
        let engine = TtsEngine::BreezeTts2;
        let json = serde_json::to_string(&engine).unwrap();
        assert_eq!(json, r#""breeze_tts_2""#);

        let parsed1: TtsEngine = serde_json::from_str(r#""breeze_tts_2""#).unwrap();
        assert_eq!(parsed1, TtsEngine::BreezeTts2);

        let parsed2: TtsEngine = serde_json::from_str(r#""breeze_tts2""#).unwrap();
        assert_eq!(parsed2, TtsEngine::BreezeTts2);
    }

    #[test]
    fn update_checking_is_on_by_default_and_skips_nothing() {
        let cfg = AppConfig::default();
        assert!(cfg.updates.auto_check);
        assert!(cfg.updates.skipped_version.is_none());
    }

    /// A config written before the updates section existed must keep every
    /// setting in it. Without `#[serde(default)]` the whole file fails to parse
    /// and the user silently gets defaults for everything they ever chose.
    #[test]
    fn a_config_without_an_updates_section_still_loads() {
        let json = serde_json::to_string(&AppConfig::default()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("updates");
        let stripped = serde_json::to_string(&value).unwrap();

        let parsed: AppConfig = serde_json::from_str(&stripped).expect("older configs must load");
        assert!(parsed.updates.auto_check);
    }

    #[test]
    fn turning_auto_check_off_survives_a_round_trip() {
        let mut cfg = AppConfig::default();
        cfg.updates.auto_check = false;
        cfg.updates.skipped_version = Some("0.4.0".to_string());

        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(!parsed.updates.auto_check);
        assert_eq!(parsed.updates.skipped_version.as_deref(), Some("0.4.0"));
    }
}
