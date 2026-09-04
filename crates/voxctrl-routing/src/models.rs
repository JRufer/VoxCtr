use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Gesture types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureType {
    Hold,
    Toggle,
    DoubleTap,
    DoubleTapHold,
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
    pub disabled: bool,
    #[serde(default, alias = "ollama_enabled")]
    pub openai_enabled: Option<bool>,
    #[serde(default, alias = "ollama_model")]
    pub openai_model: Option<String>,
    #[serde(default, alias = "ollama_mode")]
    pub openai_mode: Option<String>,
    #[serde(default, alias = "ollama_prompt")]
    pub openai_prompt: Option<String>,
    #[serde(default, alias = "ollama_system_prompt")]
    pub openai_system_prompt: Option<String>,
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

    /// Stable identity of the key combination this binding listens for,
    /// independent of the order the keys were captured in.
    ///
    /// Two bindings that share a signature share a physical trigger, which is
    /// what lets `double_tap` and `double_tap_hold` coexist on one key and what
    /// lets the portal backend register a single system shortcut for both.
    pub fn trigger_signature(&self) -> String {
        let mut keys = self.keys.clone();
        keys.sort();
        keys.dedup();
        keys.join("+")
    }
}

fn default_tap_ms() -> u32 {
    // Gap allowed between releasing the first tap and pressing the second.
    // 250ms was tight enough that a deliberate but unhurried double-tap missed;
    // desktop double-click windows are typically 400-500ms, so this stays on
    // the responsive side of convention without punishing a slower tap.
    300
}
fn default_hold_threshold_ms() -> u32 {
    // Minimum press duration before a Hold gesture starts recording. Long
    // enough to debounce accidental taps, short enough that pressing the
    // hotkey gives near-immediate feedback — the previous 1000ms made a normal
    // press look completely dead (no overlay, no recording) on fresh installs.
    200
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
    Chat,
    Command,
}

// ── Per-target processing overrides ──────────────────────────────────────────

/// None = inherit global config; Some(x) = override for this target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetProcessingConfig {
    pub remove_fillers: Option<bool>,
    pub spoken_punctuation: Option<bool>,
    pub auto_format_lists: Option<bool>,
    pub code_mode: Option<bool>,
}

impl TargetProcessingConfig {
    pub fn has_any(&self) -> bool {
        self.remove_fillers.is_some()
            || self.spoken_punctuation.is_some()
            || self.auto_format_lists.is_some()
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

    // Chat (OpenAI-compatible /v1/chat/completions, with conversation history)
    /// Base URL of the OpenAI-compatible server, with or without a `/v1` suffix.
    pub chat_url: Option<String>,
    pub chat_model: Option<String>,
    /// Optional bearer token. Local servers usually don't need one.
    pub chat_api_key: Option<String>,
    /// System message prepended to every request. Empty/None = no system message.
    pub chat_system_prompt: Option<String>,
    /// How many non-system messages to keep. 0 = unlimited.
    #[serde(default = "default_chat_max_history")]
    pub chat_max_history: u32,
    #[serde(default = "default_chat_timeout_secs")]
    pub chat_timeout_secs: u64,
    /// What to do with the assistant's reply: `speak`, `inject`, `clipboard` or `none`.
    #[serde(default = "default_chat_reply_mode")]
    pub chat_reply_mode: String,
    /// Spoken phrase that clears this target's conversation history instead of
    /// being sent to the model. Case- and punctuation-insensitive.
    pub chat_reset_phrase: Option<String>,

    /// strftime pattern for the timestamp the `file` target writes when
    /// `file_timestamp` is on. An unusable pattern falls back to the default
    /// rather than failing the delivery.
    #[serde(default = "crate::timestamp::default_file_timestamp_format")]
    pub file_timestamp_format: String,

    /// Flatten the transcript onto one line before delivering it. Honored by
    /// the `inject` and `command` targets.
    #[serde(default)]
    pub strip_newlines: bool,

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
pub(crate) fn default_chat_max_history() -> u32 {
    20
}
pub(crate) fn default_chat_timeout_secs() -> u64 {
    // Local models on modest hardware routinely take tens of seconds for a
    // first token, so this is far more generous than the 5s used for the
    // fire-and-forget HTTP target.
    120
}
pub(crate) fn default_chat_reply_mode() -> String {
    "speak".into()
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
            chat_url: None,
            chat_model: None,
            chat_api_key: None,
            chat_system_prompt: None,
            chat_max_history: default_chat_max_history(),
            chat_timeout_secs: default_chat_timeout_secs(),
            chat_reply_mode: default_chat_reply_mode(),
            chat_reset_phrase: None,
            file_timestamp_format: crate::timestamp::default_file_timestamp_format(),
            strip_newlines: false,
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
