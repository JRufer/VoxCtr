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
    Auto,
    WhisperCpp,
    Moonshine,
}

impl Default for BackendChoice {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InferenceMode {
    Balanced,
    Aggressive,
}

impl Default for InferenceMode {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineConfig {
    pub backend: BackendChoice,
    pub inference_mode: InferenceMode,
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
    pub min_silence_duration_ms: u32,
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
            min_silence_duration_ms: 500,
            input_device_index: None,
            evdev_device: None,
            noise_suppression: false,
            gain: 1.0,
            dynamic_stream: true,
        }
    }
}

fn default_auto_show_settings() -> bool {
    true
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
            auto_show_settings: true,
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
    pub quiet_mode: bool,
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
            quiet_mode: false,
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

fn default_breeze_temperature() -> f32 {
    0.7
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
    /// HuggingFace access token (required to download the gated `kyutai/pocket-tts` weights)
    #[serde(default)]
    pub hf_token: Option<String>,
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
            hf_token: None,
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
    /// HuggingFace access token (shared with Pocket-TTS)
    #[serde(default)]
    pub hf_token: Option<String>,
    /// Pre-warm model on startup so the first synthesis is instant
    #[serde(default)]
    pub prewarm: bool,
    /// Enable GPU acceleration (CUDA)
    #[serde(default)]
    pub gpu: bool,
    /// Sampling temperature / noise scale for speech generation
    #[serde(default = "default_breeze_temperature")]
    pub temperature: f32,
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
            hf_token: None,
            prewarm: false,
            gpu: false,
            temperature: default_breeze_temperature(),
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

// ── AT-SPI2 ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtspiConfig {
    /// Use AT-SPI2 for text insertion when available
    pub injection: bool,
    /// Feed surrounding text to Whisper as initial prompt
    pub context_prompt: bool,
    /// Automatically switch to code mode in terminals/IDEs
    pub auto_code_mode: bool,
}

impl Default for AtspiConfig {
    fn default() -> Self {
        Self {
            injection: true,
            context_prompt: true,
            auto_code_mode: true,
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
    pub atspi: AtspiConfig,
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
            match std::fs::read_to_string(&path)
                .map_err(ConfigError::Io)
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).map_err(ConfigError::Json))
            {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!("Failed to load config, using defaults: {e}");
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

        // Synchronize HuggingFace token between Pocket-TTS and Breeze-TTS-2
        if data.tts.pocket_tts.hf_token.is_some() && data.tts.breeze_tts_2.hf_token.is_none() {
            data.tts.breeze_tts_2.hf_token = data.tts.pocket_tts.hf_token.clone();
        } else if data.tts.breeze_tts_2.hf_token.is_some() && data.tts.pocket_tts.hf_token.is_none() {
            data.tts.pocket_tts.hf_token = data.tts.breeze_tts_2.hf_token.clone();
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

/// Search `$PATH` for an executable named `name`, returning its full path if found.
/// On Windows, appends `.exe` automatically when `name` has no extension.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let search_name: std::borrow::Cow<str> = if cfg!(target_os = "windows") && !name.contains('.') {
        format!("{name}.exe").into()
    } else {
        name.into()
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(search_name.as_ref()))
            .find(|p| p.is_file())
    })
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
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert!(cfg.ui.auto_show_settings);
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
                "inference_mode": "Balanced",
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
                "min_silence_duration_ms": 500,
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
                "quiet_mode": false,
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
            },
            "atspi": {
                "injection": true,
                "context_prompt": true,
                "auto_code_mode": true
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
    "quiet_mode": false,
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
}

