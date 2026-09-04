use std::path::{Path, PathBuf};

use chrono::Utc;
use thiserror::Error;

use crate::models::{
    DeliveryType, GestureType, HotkeyBinding, OutputTarget, TargetProcessingConfig,
};

const FORMAT_VERSION: &str = "1.1";
const KEEP_BACKUPS: usize = 20;

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
}

// ── TOML round-trip via serde ────────────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct TargetsFile {
    format_version: String,
    #[serde(default, rename = "target")]
    targets: Vec<RawTarget>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct BindingsFile {
    format_version: String,
    #[serde(default, rename = "binding")]
    bindings: Vec<RawBinding>,
}

// We use intermediate "raw" structs so every field can be Option<> with a
// serde default, avoiding breakage when new keys are added in the future.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RawTarget {
    id: String,
    label: String,
    delivery: String,
    command: Option<String>,
    pipe_path: Option<String>,
    socket_host: Option<String>,
    socket_port: Option<u16>,
    socket_unix: Option<String>,
    file_path: Option<String>,
    #[serde(default)]
    file_prefix: String,
    #[serde(default = "bool_true")]
    file_timestamp: bool,
    #[serde(default = "default_file_mode")]
    file_mode: String,
    dbus_signal: Option<String>,
    http_url: Option<String>,
    #[serde(default = "default_post")]
    http_method: String,
    http_headers: Option<std::collections::HashMap<String, String>>,
    http_json_template: Option<toml::Value>,
    webhook_url: Option<String>,
    webhook_secret: Option<String>,
    webhook_json_template: Option<toml::Value>,
    mcp_path: Option<String>,
    mcp_tool: Option<String>,
    mcp_args: Option<toml::Value>,
    chat_url: Option<String>,
    chat_model: Option<String>,
    chat_api_key: Option<String>,
    chat_system_prompt: Option<String>,
    #[serde(default = "default_chat_max_history")]
    chat_max_history: u32,
    #[serde(default = "default_chat_timeout_secs")]
    chat_timeout_secs: u64,
    #[serde(default = "default_chat_reply_mode")]
    chat_reply_mode: String,
    chat_reset_phrase: Option<String>,
    #[serde(default)]
    strip_newlines: bool,
    #[serde(default)]
    pub processing: RawProcessing,
    response_pipe: Option<String>,
    // Legacy field kept for migration
    post_processing: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RawProcessing {
    remove_fillers: Option<bool>,
    spoken_punctuation: Option<bool>,
    auto_format_lists: Option<bool>,
    code_mode: Option<bool>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct RawBinding {
    id: String,
    label: String,
    keys: Vec<String>,
    gesture: String,
    target_id: String,
    #[serde(default)]
    target_ids: Option<Vec<String>>,
    #[serde(default = "default_tap_ms")]
    tap_ms: u32,
    #[serde(default = "default_hold_ms")]
    hold_threshold_ms: u32,
    #[serde(default)]
    disabled: bool,
    #[serde(default, alias = "ollama_enabled")]
    openai_enabled: Option<bool>,
    #[serde(default, alias = "ollama_model")]
    openai_model: Option<String>,
    #[serde(default, alias = "ollama_mode")]
    openai_mode: Option<String>,
    #[serde(default, alias = "ollama_prompt")]
    openai_prompt: Option<String>,
    #[serde(default, alias = "ollama_system_prompt")]
    openai_system_prompt: Option<String>,
}

fn bool_true() -> bool {
    true
}
fn default_post() -> String {
    "POST".into()
}
fn default_file_mode() -> String {
    "append".into()
}
use crate::models::{default_chat_max_history, default_chat_reply_mode, default_chat_timeout_secs};
fn default_tap_ms() -> u32 {
    // Keep in sync with models::default_tap_ms.
    300
}
fn default_hold_ms() -> u32 {
    // Keep in sync with models::default_hold_threshold_ms — 200ms debounces
    // accidental taps without making a normal hotkey press feel dead.
    200
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn raw_to_target(r: RawTarget) -> OutputTarget {
    let delivery = match r.delivery.as_str() {
        "clipboard" => DeliveryType::Clipboard,
        "exec" => DeliveryType::Exec,
        "pipe" => DeliveryType::Pipe,
        "socket" => DeliveryType::Socket,
        "file" => DeliveryType::File,
        "dbus" => DeliveryType::Dbus,
        "http" => DeliveryType::Http,
        "webhook" => DeliveryType::Webhook,
        "mcp" => DeliveryType::Mcp,
        "speak" => DeliveryType::Speak,
        "chat" => DeliveryType::Chat,
        "command" => DeliveryType::Command,
        _ => DeliveryType::Inject,
    };

    let has_any_override = r.processing.remove_fillers.is_some()
        || r.processing.spoken_punctuation.is_some()
        || r.processing.auto_format_lists.is_some()
        || r.processing.code_mode.is_some();

    // Migrate legacy post_processing string to processing overrides
    let processing = if !has_any_override {
        migrate_legacy_pp(r.post_processing.as_deref().unwrap_or("default"))
    } else {
        TargetProcessingConfig {
            remove_fillers: r.processing.remove_fillers,
            spoken_punctuation: r.processing.spoken_punctuation,
            auto_format_lists: r.processing.auto_format_lists,
            code_mode: r.processing.code_mode,
        }
    };

    // Convert toml::Value http/webhook templates to serde_json::Value
    let http_json_template = r
        .http_json_template
        .and_then(|v| serde_json::to_value(v).ok());
    let webhook_json_template = r
        .webhook_json_template
        .and_then(|v| serde_json::to_value(v).ok());
    let mcp_args = r
        .mcp_args
        .and_then(|v| serde_json::to_value(v).ok());

    OutputTarget {
        id: r.id,
        label: r.label,
        delivery,
        command: r.command,
        pipe_path: r.pipe_path,
        socket_host: r.socket_host,
        socket_port: r.socket_port,
        socket_unix: r.socket_unix,
        file_path: r.file_path,
        file_prefix: r.file_prefix,
        file_timestamp: r.file_timestamp,
        file_mode: r.file_mode,
        dbus_signal: r.dbus_signal,
        http_url: r.http_url,
        http_method: r.http_method,
        http_headers: r.http_headers,
        http_json_template,
        webhook_url: r.webhook_url,
        webhook_secret: r.webhook_secret,
        webhook_json_template,
        mcp_path: r.mcp_path,
        mcp_tool: r.mcp_tool,
        mcp_args,
        chat_url: r.chat_url,
        chat_model: r.chat_model,
        chat_api_key: r.chat_api_key,
        chat_system_prompt: r.chat_system_prompt,
        chat_max_history: r.chat_max_history,
        chat_timeout_secs: r.chat_timeout_secs,
        chat_reply_mode: r.chat_reply_mode,
        chat_reset_phrase: r.chat_reset_phrase,
        strip_newlines: r.strip_newlines,
        processing,
        response_pipe: r.response_pipe,
    }
}

fn migrate_legacy_pp(pp: &str) -> TargetProcessingConfig {
    match pp {
        "none" => TargetProcessingConfig {
            remove_fillers: Some(false),
            spoken_punctuation: Some(false),
            auto_format_lists: Some(false),
            ..Default::default()
        },
        "strip_fillers" => TargetProcessingConfig {
            remove_fillers: Some(true),
            spoken_punctuation: Some(false),
            auto_format_lists: Some(false),
            ..Default::default()
        },
        "snippets_only" => TargetProcessingConfig {
            remove_fillers: Some(false),
            spoken_punctuation: Some(false),
            auto_format_lists: Some(false),
            ..Default::default()
        },
        "openai_only" | "ollama_only" => TargetProcessingConfig {
            remove_fillers: Some(false),
            spoken_punctuation: Some(false),
            auto_format_lists: Some(false),
            ..Default::default()
        },
        _ => TargetProcessingConfig::default(),
    }
}

fn target_to_raw(t: &OutputTarget) -> RawTarget {
    let p = &t.processing;
    RawTarget {
        id: t.id.clone(),
        label: t.label.clone(),
        delivery: format!("{:?}", t.delivery).to_lowercase(),
        command: t.command.clone(),
        pipe_path: t.pipe_path.clone(),
        socket_host: t.socket_host.clone(),
        socket_port: t.socket_port,
        socket_unix: t.socket_unix.clone(),
        file_path: t.file_path.clone(),
        file_prefix: t.file_prefix.clone(),
        file_timestamp: t.file_timestamp,
        file_mode: t.file_mode.clone(),
        dbus_signal: t.dbus_signal.clone(),
        http_url: t.http_url.clone(),
        http_method: t.http_method.clone(),
        http_headers: t.http_headers.clone(),
        http_json_template: t
            .http_json_template
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        webhook_url: t.webhook_url.clone(),
        webhook_secret: t.webhook_secret.clone(),
        webhook_json_template: t
            .webhook_json_template
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        mcp_path: t.mcp_path.clone(),
        mcp_tool: t.mcp_tool.clone(),
        mcp_args: t
            .mcp_args
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        chat_url: t.chat_url.clone(),
        chat_model: t.chat_model.clone(),
        chat_api_key: t.chat_api_key.clone(),
        chat_system_prompt: t.chat_system_prompt.clone(),
        chat_max_history: t.chat_max_history,
        chat_timeout_secs: t.chat_timeout_secs,
        chat_reply_mode: t.chat_reply_mode.clone(),
        chat_reset_phrase: t.chat_reset_phrase.clone(),
        strip_newlines: t.strip_newlines,
        processing: RawProcessing {
            remove_fillers: p.remove_fillers,
            spoken_punctuation: p.spoken_punctuation,
            auto_format_lists: p.auto_format_lists,
            code_mode: p.code_mode,
        },
        response_pipe: t.response_pipe.clone(),
        post_processing: None,
    }
}

fn raw_to_binding(r: RawBinding) -> HotkeyBinding {
    let gesture = match r.gesture.as_str() {
        "toggle" => GestureType::Toggle,
        "double_tap" => GestureType::DoubleTap,
        "double_tap_hold" => GestureType::DoubleTapHold,
        // `chord` was removed. Its base keys already live in `keys`, and its
        // start/stop semantics (hold the base combo, release to stop) are what
        // `hold` does — so an existing binding keeps working instead of
        // vanishing from the user's config. The `subkey` field is simply
        // dropped: serde ignores it on read and it is no longer written back.
        "chord" => {
            tracing::info!(
                binding = %r.id,
                "The `chord` gesture was removed; migrating this binding to `hold`"
            );
            GestureType::Hold
        }
        _ => GestureType::Hold,
    };
    let target_ids = if let Some(ref ids) = r.target_ids {
        if ids.is_empty() {
            vec![r.target_id.clone()]
        } else {
            ids.clone()
        }
    } else {
        vec![r.target_id.clone()]
    };
    HotkeyBinding {
        id: r.id,
        label: r.label,
        keys: r.keys,
        gesture,
        target_id: r.target_id,
        target_ids,
        tap_ms: r.tap_ms,
        hold_threshold_ms: r.hold_threshold_ms,
        disabled: r.disabled,
        openai_enabled: r.openai_enabled,
        openai_model: r.openai_model,
        openai_mode: r.openai_mode,
        openai_prompt: r.openai_prompt,
        openai_system_prompt: r.openai_system_prompt,
    }
}

fn binding_to_raw(b: &HotkeyBinding) -> RawBinding {
    let gesture = match b.gesture {
        GestureType::Toggle => "toggle",
        GestureType::DoubleTap => "double_tap",
        GestureType::DoubleTapHold => "double_tap_hold",
        GestureType::Hold => "hold",
    };
    let target_id = b.target_ids.first().cloned().unwrap_or_else(|| b.target_id.clone());
    RawBinding {
        id: b.id.clone(),
        label: b.label.clone(),
        keys: b.keys.clone(),
        gesture: gesture.into(),
        target_id,
        target_ids: Some(b.target_ids.clone()),
        tap_ms: b.tap_ms,
        hold_threshold_ms: b.hold_threshold_ms,
        disabled: b.disabled,
        openai_enabled: b.openai_enabled,
        openai_model: b.openai_model.clone(),
        openai_mode: b.openai_mode.clone(),
        openai_prompt: b.openai_prompt.clone(),
        openai_system_prompt: b.openai_system_prompt.clone(),
    }
}

// ── Default values ────────────────────────────────────────────────────────────

pub fn default_targets() -> Vec<OutputTarget> {
    vec![OutputTarget::default_inject()]
}

pub fn default_bindings() -> Vec<HotkeyBinding> {
    vec![
        HotkeyBinding {
            id: "default_hold".into(),
            label: "Dictate (Hold)".into(),
            keys: vec!["KEY_LEFTMETA".into(), "KEY_SPACE".into()],
            gesture: GestureType::Hold,
            target_id: "default".into(),
            target_ids: vec!["default".into()],
            tap_ms: 300,
            hold_threshold_ms: 200,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        },
        HotkeyBinding {
            id: "default_toggle".into(),
            label: "Dictate (Toggle)".into(),
            keys: vec![
                "KEY_LEFTCTRL".into(),
                "KEY_LEFTMETA".into(),
                "KEY_SPACE".into(),
            ],
            gesture: GestureType::Toggle,
            target_id: "default".into(),
            target_ids: vec!["default".into()],
            tap_ms: 300,
            hold_threshold_ms: 200,
            disabled: false,
            openai_enabled: Some(false),
            openai_model: None,
            openai_mode: None,
            openai_prompt: None,
            openai_system_prompt: None,
        },
    ]
}

// ── Backup ────────────────────────────────────────────────────────────────────

fn backup(filename: &str, config_dir: &Path) -> std::io::Result<()> {
    let src = config_dir.join(filename);
    if !src.exists() {
        return Ok(());
    }
    // NOTE: colons are illegal in Windows filenames, so the timestamp uses
    // hyphens instead of the RFC 3339 `:` separators for the time component.
    let ts = Utc::now().format("%Y-%m-%dT%H-%M-%SZ");
    let backup_dir = config_dir.join("backups");
    std::fs::create_dir_all(&backup_dir)?;
    let dst = backup_dir.join(format!("{filename}.{ts}"));
    std::fs::copy(&src, &dst)?;
    prune_backups(filename, config_dir);
    Ok(())
}

fn prune_backups(filename: &str, config_dir: &Path) {
    let backup_dir = config_dir.join("backups");
    if !backup_dir.exists() {
        return;
    }
    let pattern = format!("{filename}.");
    let mut entries: Vec<_> = std::fs::read_dir(&backup_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&pattern)
        })
        .map(|e| e.path())
        .collect();
    entries.sort();
    for old in entries.iter().take(entries.len().saturating_sub(KEEP_BACKUPS)) {
        let _ = std::fs::remove_file(old);
    }
}

// ── Private file write ────────────────────────────────────────────────────────

fn write_private(path: impl AsRef<std::path::Path>, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)?;
    }
    Ok(())
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn load_targets(config_dir: &Path) -> Result<Vec<OutputTarget>, LoaderError> {
    let path = config_dir.join("targets.toml");
    if !path.exists() {
        return Ok(default_targets());
    }
    let text = std::fs::read_to_string(&path)?;
    let file: TargetsFile = toml::from_str(&text)?;
    let targets: Vec<_> = file.targets.into_iter().map(raw_to_target).collect();
    Ok(if targets.is_empty() {
        default_targets()
    } else {
        targets
    })
}

pub fn load_bindings(config_dir: &Path) -> Result<Vec<HotkeyBinding>, LoaderError> {
    let path = config_dir.join("bindings.toml");
    if !path.exists() {
        return Ok(default_bindings());
    }
    let text = std::fs::read_to_string(&path)?;
    let file: BindingsFile = toml::from_str(&text)?;
    let bindings: Vec<_> = file.bindings.into_iter().map(raw_to_binding).collect();
    Ok(if bindings.is_empty() {
        default_bindings()
    } else {
        bindings
    })
}

pub fn save_targets(targets: &[OutputTarget], config_dir: &Path) -> Result<(), LoaderError> {
    std::fs::create_dir_all(config_dir)?;
    backup("targets.toml", config_dir)?;
    let file = TargetsFile {
        format_version: FORMAT_VERSION.into(),
        targets: targets.iter().map(target_to_raw).collect(),
    };
    let text = toml::to_string_pretty(&file)?;
    write_private(config_dir.join("targets.toml"), &text)?;
    Ok(())
}

pub fn save_bindings(bindings: &[HotkeyBinding], config_dir: &Path) -> Result<(), LoaderError> {
    std::fs::create_dir_all(config_dir)?;
    backup("bindings.toml", config_dir)?;
    let file = BindingsFile {
        format_version: FORMAT_VERSION.into(),
        bindings: bindings.iter().map(binding_to_raw).collect(),
    };
    let text = toml::to_string_pretty(&file)?;
    write_private(config_dir.join("bindings.toml"), &text)?;
    Ok(())
}
