use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Gesture types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureType {
    Hold,
    Toggle,
    DoubleTap,
    DoubleTapHold,
    Chord,
}

// ── Hotkey binding ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub id: String,
    pub keys: Vec<String>,
    pub gesture: GestureType,
    pub target_id: String,
    #[serde(default)]
    pub target_ids: Vec<String>,
    #[serde(default = "default_tap_ms")]
    pub tap_ms: u32,
    #[serde(default = "default_hold_threshold_ms")]
    pub hold_threshold_ms: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub subkey: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, alias = "ollama_enabled")]
    pub openai_enabled: Option<bool>,
    #[serde(default, alias = "ollama_model")]
    pub openai_model: Option<String>,
    #[serde(default, alias = "ollama_mode")]
    pub openai_mode: Option<String>,
    #[serde(default, alias = "ollama_prompt")]
    pub openai_prompt: Option<String>,
}

impl HotkeyBinding {
    pub fn resolved_target_ids(&self) -> Vec<String> {
        if self.target_ids.is_empty() {
            vec![self.target_id.clone()]
        } else {
            self.target_ids.clone()
        }
    }

    pub fn target_ids_string(&self) -> String {
        self.resolved_target_ids().join(",")
    }
}

fn default_tap_ms() -> u32 {
    250
}
fn default_hold_threshold_ms() -> u32 {
    1000
}

// ── Delivery types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryType {
    Inject,
    Clipboard,
    Exec,
    Pipe,
    Socket,
    File,
    Dbus,
    Http,
    Webhook,
    Mcp,
    Speak,
}

// ── Per-target processing overrides ──────────────────────────────────────────

/// None = inherit global config; Some(x) = override for this target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetProcessingConfig {
    pub noise_suppression: Option<bool>,
    pub quiet_mode: Option<bool>,
    pub atspi_context: Option<bool>,
    pub remove_fillers: Option<bool>,
    pub spoken_punctuation: Option<bool>,
    pub auto_format_lists: Option<bool>,
    pub apply_snippets: Option<bool>,
    pub code_mode: Option<bool>,
}

impl TargetProcessingConfig {
    pub fn has_any(&self) -> bool {
        self.noise_suppression.is_some()
            || self.quiet_mode.is_some()
            || self.atspi_context.is_some()
            || self.remove_fillers.is_some()
            || self.spoken_punctuation.is_some()
            || self.auto_format_lists.is_some()
            || self.apply_snippets.is_some()
            || self.code_mode.is_some()
    }
}

// ── Output target ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTarget {
    pub id: String,
    pub label: String,
    pub delivery: DeliveryType,

    // Exec
    pub command: Option<String>,

    // Pipe
    pub pipe_path: Option<String>,

    // Socket
    pub socket_host: Option<String>,
    pub socket_port: Option<u16>,
    pub socket_unix: Option<String>,

    // File
    pub file_path: Option<String>,
    #[serde(default)]
    pub file_prefix: String,
    #[serde(default = "bool_true")]
    pub file_timestamp: bool,
    #[serde(default = "default_file_mode")]
    pub file_mode: String,

    // DBus
    pub dbus_signal: Option<String>,

    // HTTP
    pub http_url: Option<String>,
    #[serde(default = "default_http_method")]
    pub http_method: String,
    pub http_headers: Option<HashMap<String, String>>,
    pub http_json_template: Option<serde_json::Value>,

    // Webhook
    pub webhook_url: Option<String>,
    pub webhook_secret: Option<String>,
    pub webhook_json_template: Option<serde_json::Value>,

    // MCP
    pub mcp_path: Option<String>,
    pub mcp_tool: Option<String>,
    pub mcp_args: Option<serde_json::Value>,

    #[serde(default = "bool_true")]
    pub send_on_release: bool,
    #[serde(default = "bool_true")]
    pub append_newline: bool,
    #[serde(default)]
    pub strip_newlines: bool,
    pub initial_prompt: Option<String>,

    #[serde(default)]
    pub processing: TargetProcessingConfig,

    // TTS response loopback
    pub response_pipe: Option<String>,
}

fn bool_true() -> bool {
    true
}
fn default_http_method() -> String {
    "POST".into()
}
fn default_file_mode() -> String {
    "append".into()
}

impl OutputTarget {
    pub fn default_inject() -> Self {
        Self {
            id: "default".into(),
            label: "Focused Window".into(),
            delivery: DeliveryType::Inject,
            command: None,
            pipe_path: None,
            socket_host: None,
            socket_port: None,
            socket_unix: None,
            file_path: None,
            file_prefix: String::new(),
            file_timestamp: true,
            file_mode: "append".into(),
            dbus_signal: None,
            http_url: None,
            http_method: "POST".into(),
            http_headers: None,
            http_json_template: None,
            webhook_url: None,
            webhook_secret: None,
            webhook_json_template: None,
            mcp_path: None,
            mcp_tool: None,
            mcp_args: None,
            send_on_release: true,
            append_newline: false,
            strip_newlines: false,
            initial_prompt: None,
            processing: TargetProcessingConfig::default(),
            response_pipe: None,
        }
    }
}

// ── Results ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DeliveryResult {
    pub success: bool,
    pub error: Option<String>,
    pub delivered_text: Option<String>,
}

impl DeliveryResult {
    pub fn ok(text: String) -> Self {
        Self {
            success: true,
            error: None,
            delivered_text: Some(text),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(msg.into()),
            delivered_text: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub reachable: bool,
    pub detail: String,
}
