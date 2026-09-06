use anyhow::Context;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;

use crate::models::{DeliveryResult, DeliveryType, OutputTarget, TestResult};

use std::sync::Arc;
use std::sync::OnceLock;

pub type SpeakCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;
static SPEAK_CALLBACK: OnceLock<SpeakCallback> = OnceLock::new();

pub fn set_speak_callback(callback: SpeakCallback) {
    let _ = SPEAK_CALLBACK.set(callback);
}

pub type CommandTriggerCallback = Arc<dyn Fn(&str, &str) + Send + Sync + 'static>;
static COMMAND_TRIGGER_CALLBACK: OnceLock<CommandTriggerCallback> = OnceLock::new();

pub fn set_command_trigger_callback(callback: CommandTriggerCallback) {
    let _ = COMMAND_TRIGGER_CALLBACK.set(callback);
}

pub fn notify_command_trigger(command_name: &str, text_summary: &str) {
    if let Some(cb) = COMMAND_TRIGGER_CALLBACK.get() {
        cb(command_name, text_summary);
    }
}

// Shared HTTP client — built once, reused for connection pooling.
fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("build reqwest client")
    })
}

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait DeliveryTarget: Send + Sync {
    async fn deliver(&self, text: &str) -> DeliveryResult;
    async fn test(&self) -> TestResult;
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn build_target(config: OutputTarget) -> Box<dyn DeliveryTarget> {
    match config.delivery {
        DeliveryType::Inject    => Box::new(InjectTarget(config)),
        DeliveryType::Clipboard => Box::new(ClipboardTarget(config)),
        DeliveryType::Exec      => Box::new(ExecTarget(config)),
        DeliveryType::Pipe      => Box::new(PipeTarget(config)),
        DeliveryType::Socket    => Box::new(SocketTarget(config)),
        DeliveryType::File      => Box::new(FileTarget(config)),
        DeliveryType::Dbus      => Box::new(DbusTarget(config)),
        DeliveryType::Http      => Box::new(HttpTarget(config)),
        DeliveryType::Webhook   => Box::new(WebhookTarget(config)),
        DeliveryType::Mcp       => Box::new(McpTarget(config)),
        DeliveryType::Speak     => Box::new(SpeakTarget(config)),
        DeliveryType::Chat      => Box::new(ChatTarget(config)),
        DeliveryType::Command   => Box::new(CommandTarget(config)),
    }
}

/// End delivered text with exactly one space.
///
/// Dictation arrives one utterance at a time, so without this the next one
/// starts flush against the last word. Text that already ends in whitespace is
/// left as it is rather than accumulating spaces across repeated deliveries.
fn append_trailing_space(payload: &mut String) {
    if !payload.ends_with(char::is_whitespace) {
        payload.push(' ');
    }
}

// ── InjectTarget ──────────────────────────────────────────────────────────────

pub struct InjectTarget(pub OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for InjectTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let mut payload = if self.0.strip_newlines {
            let cleaned = text.replace('\r', "").replace('\n', " ");
            let mut result = String::new();
            let mut last_was_space = false;
            for c in cleaned.chars() {
                if c == ' ' {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(c);
                    last_was_space = false;
                }
            }
            result
        } else {
            text.to_string()
        };
        // A trailing space so back-to-back dictations do not run their last
        // and first words together.
        append_trailing_space(&mut payload);

        #[cfg(target_os = "linux")]
        {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
            if wayland && which("wtype") {
                let ok = tokio::process::Command::new("wtype")
                    .arg("--")
                    .arg(&payload)
                    .status()
                    .await
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return DeliveryResult::ok(payload);
                }
            }
            if which("xdotool") {
                let ok = tokio::process::Command::new("xdotool")
                    .args(["type", "--clearmodifiers", "--delay", "12", "--"])
                    .arg(&payload)
                    .status()
                    .await
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return DeliveryResult::ok(payload);
                }
            }
            return DeliveryResult::err("No injection method available (wtype / xdotool)");
        }

        #[cfg(target_os = "windows")]
        {
            // `SendInput` with KEYEVENTF_UNICODE, via voxctrl-winput.
            //
            // This used to shell out to PowerShell and call
            // `SendKeys::SendWait`. The payload was base64-encoded so no shell
            // metacharacter could escape the string — a real defence, and it
            // worked — but SendKeys then applied *its own* escaping to the
            // decoded text, in which `+ ^ % ~ ( ) { } [ ]` are syntax. So
            // "50% (a+b)" was typed as "50" plus two stray chords and
            // "array[0]" as "array0": every dictation containing ordinary
            // punctuation came out wrong. SendInput carries the character
            // itself, so there is no escaping layer left to misread it.
            let sent = tokio::task::spawn_blocking(move || {
                voxctrl_winput::deliver(&payload).map(|()| payload)
            })
            .await;
            return match sent {
                Ok(Ok(payload)) => DeliveryResult::ok(payload),
                Ok(Err(e)) => DeliveryResult::err(e.to_string()),
                Err(e) => DeliveryResult::err(format!("Injection task failed: {e}")),
            };
        }

        #[allow(unreachable_code)]
        DeliveryResult::err("Text injection not supported on this platform")
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        {
            if which("wtype") {
                return TestResult { reachable: true, detail: "wtype found on PATH".into() };
            }
            if which("xdotool") {
                return TestResult { reachable: true, detail: "xdotool found on PATH".into() };
            }
            return TestResult {
                reachable: false,
                detail: "Neither wtype nor xdotool found".into(),
            };
        }
        #[cfg(target_os = "windows")]
        return TestResult {
            reachable: true,
            detail: "SendInput available (types Unicode directly)".into(),
        };
        #[allow(unreachable_code)]
        TestResult { reachable: false, detail: "Platform not supported".into() }
    }
}

// ── ClipboardTarget ───────────────────────────────────────────────────────────

/// The clipboard delivery target. It keeps its `OutputTarget` so the chat
/// reply path and the router construct it the same way as every other target,
/// though the copy itself needs nothing from it.
pub struct ClipboardTarget(#[allow(dead_code)] OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for ClipboardTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let mut payload = text.to_string();
        append_trailing_space(&mut payload);

        #[cfg(target_os = "linux")]
        {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
            if wayland && which("wl-copy") {
                let mut child = match tokio::process::Command::new("wl-copy")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(e) => return DeliveryResult::err(format!("Failed to spawn wl-copy: {e}")),
                };
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(e) = stdin.write_all(payload.as_bytes()).await {
                        return DeliveryResult::err(format!("Failed to write to wl-copy stdin: {e}"));
                    }
                }
                match child.wait().await {
                    Ok(status) if status.success() => return DeliveryResult::ok(payload),
                    Ok(status) => return DeliveryResult::err(format!("wl-copy exited with status: {status}")),
                    Err(e) => return DeliveryResult::err(format!("wl-copy wait failed: {e}")),
                }
            }

            if which("xclip") {
                let mut child = match tokio::process::Command::new("xclip")
                    .args(["-selection", "clipboard"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(e) => return DeliveryResult::err(format!("Failed to spawn xclip: {e}")),
                };
                if let Some(mut stdin) = child.stdin.take() {
                    if let Err(e) = stdin.write_all(payload.as_bytes()).await {
                        return DeliveryResult::err(format!("Failed to write to xclip stdin: {e}"));
                    }
                }
                match child.wait().await {
                    Ok(status) if status.success() => return DeliveryResult::ok(payload),
                    Ok(status) => return DeliveryResult::err(format!("xclip exited with status: {status}")),
                    Err(e) => return DeliveryResult::err(format!("xclip wait failed: {e}")),
                }
            }
        }

        let p = payload.clone();
        match tokio::task::spawn_blocking(move || {
            arboard::Clipboard::new()
                .context("open clipboard")?
                .set_text(&p)
                .context("set text")
        })
        .await
        {
            Ok(Ok(_)) => DeliveryResult::ok(payload),
            Ok(Err(e)) => DeliveryResult::err(e.to_string()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
            if wayland && which("wl-copy") {
                return TestResult { reachable: true, detail: "wl-copy found on PATH (Wayland)".into() };
            }
            if which("xclip") {
                return TestResult { reachable: true, detail: "xclip found on PATH".into() };
            }
        }

        let ok = tokio::task::spawn_blocking(|| arboard::Clipboard::new().is_ok())
            .await
            .unwrap_or(false);
        if ok {
            TestResult { reachable: true, detail: "Clipboard accessible".into() }
        } else {
            TestResult { reachable: false, detail: "Cannot open clipboard".into() }
        }
    }
}

// ── ExecTarget ────────────────────────────────────────────────────────────────

pub struct ExecTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for ExecTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let template = match &self.0.command {
            Some(c) => c.clone(),
            None => {
                tracing::error!(target_id = %self.0.id, "Exec target failed: No command configured");
                return DeliveryResult::err("No command configured");
            }
        };
        let raw_parts: Vec<&str> = template.split_whitespace().collect();
        if raw_parts.is_empty() {
            tracing::error!(target_id = %self.0.id, "Exec target failed: Empty command");
            return DeliveryResult::err("Empty command");
        }
        let has_placeholder = template.contains("{TEXT}") || template.contains("{text}");
        let cmd_binary = raw_parts[0].replace("{TEXT}", text).replace("{text}", text);
        let mut cmd = tokio::process::Command::new(&cmd_binary);
        let mut substituted_args = Vec::new();
        for part in &raw_parts[1..] {
            let substituted = part.replace("{TEXT}", text).replace("{text}", text);
            substituted_args.push(substituted.clone());
            cmd.arg(substituted);
        }
        if !has_placeholder {
            substituted_args.push(text.to_string());
            cmd.arg(text);
        }
        tracing::info!(
            target_id = %self.0.id,
            command_template = %template,
            binary = %cmd_binary,
            args = ?substituted_args,
            text_payload = %text,
            "Activating Exec command target"
        );
        match cmd.spawn() {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => {
                tracing::error!(target_id = %self.0.id, error = %e, "Exec target failed to spawn command");
                DeliveryResult::err(e.to_string())
            }
        }
    }

    async fn test(&self) -> TestResult {
        let Some(cmd) = &self.0.command else {
            return TestResult { reachable: false, detail: "No command configured".into() };
        };
        let binary = cmd.split_whitespace().next().unwrap_or("");
        if which(binary) {
            TestResult { reachable: true, detail: format!("{binary} found on PATH") }
        } else {
            TestResult { reachable: false, detail: format!("{binary} not found on PATH") }
        }
    }
}

// ── PipeTarget ────────────────────────────────────────────────────────────────

pub struct PipeTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for PipeTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(path_str) = &self.0.pipe_path else {
            return DeliveryResult::err("No pipe_path configured");
        };
        let path = shellexpand_tilde(path_str);
        if !std::path::Path::new(&path).exists() {
            return DeliveryResult::err(format!("Pipe {path} does not exist"));
        }
        let payload = format!("{text}\n").into_bytes();
        // Open FIFO for writing via std (non-blocking open)
        match std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|mut f| { use std::io::Write; f.write_all(&payload) })
        {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        let Some(path_str) = &self.0.pipe_path else {
            return TestResult { reachable: false, detail: "No pipe_path configured".into() };
        };
        let path = shellexpand_tilde(path_str);
        let p = std::path::Path::new(&path);
        if p.exists() {
            TestResult { reachable: true, detail: format!("FIFO {path} exists") }
        } else {
            TestResult { reachable: false, detail: format!("FIFO {path} not found") }
        }
    }
}

// ── SocketTarget ──────────────────────────────────────────────────────────────

pub struct SocketTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for SocketTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let payload = format!("{text}\n").into_bytes();

        #[cfg(unix)]
        if let Some(unix) = &self.0.socket_unix {
            let mut s = match UnixStream::connect(unix).await {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(e.to_string()),
            };
            return match s.write_all(&payload).await {
                Ok(_) => DeliveryResult::ok(text.into()),
                Err(e) => DeliveryResult::err(e.to_string()),
            };
        }

        let host = self.0.socket_host.as_deref().unwrap_or("127.0.0.1");
        let port = self.0.socket_port.unwrap_or(9000);
        let mut s = match TcpStream::connect((host, port)).await {
            Ok(s) => s,
            Err(e) => return DeliveryResult::err(e.to_string()),
        };
        match s.write_all(&payload).await {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        #[cfg(unix)]
        if let Some(unix) = &self.0.socket_unix {
            return match UnixStream::connect(unix).await {
                Ok(_) => TestResult { reachable: true, detail: format!("Unix socket {unix} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            };
        }

        let host = self.0.socket_host.as_deref().unwrap_or("127.0.0.1");
        let port = self.0.socket_port.unwrap_or(9000);
        match TcpStream::connect((host, port)).await {
            Ok(_) => TestResult { reachable: true, detail: format!("TCP {host}:{port} reachable") },
            Err(e) => TestResult { reachable: false, detail: e.to_string() },
        }
    }
}

// ── FileTarget ────────────────────────────────────────────────────────────────

pub struct FileTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for FileTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(path_str) = &self.0.file_path else {
            return DeliveryResult::err("No file_path configured");
        };
        let path = shellexpand_tilde(path_str);
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return DeliveryResult::err(e.to_string());
            }
        }
        let mut line = String::new();
        if self.0.file_timestamp {
            let ts = crate::timestamp::format_now(&self.0.file_timestamp_format);
            line.push_str(&format!("[{ts}] "));
        }
        line.push_str(&self.0.file_prefix);
        line.push_str(text);
        line.push('\n');

        let is_prepend = self.0.file_mode == "prepend";
        let write_result = if is_prepend {
            let existing_content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            let mut new_content = line;
            new_content.push_str(&existing_content);
            tokio::fs::write(&path, new_content.as_bytes()).await
        } else {
            match tokio::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
                .await
            {
                Ok(mut f) => {
                    let res = f.write_all(line.as_bytes()).await;
                    let _ = f.flush().await;
                    res
                }
                Err(e) => Err(e),
            }
        };

        match write_result {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        let Some(path_str) = &self.0.file_path else {
            return TestResult { reachable: false, detail: "No file_path configured".into() };
        };
        let path = shellexpand_tilde(path_str);
        let p = std::path::Path::new(&path);
        let _parent = p.parent().unwrap_or(std::path::Path::new("."));
        TestResult {
            reachable: true,
            detail: format!(
                "{path}{}",
                if p.exists() { "" } else { " (will be created)" }
            ),
        }
    }
}

// ── DbusTarget ────────────────────────────────────────────────────────────────

/// D-Bus has no Windows counterpart, so off Linux this target only ever reports
/// that. It still exists there because a `targets.toml` written on Linux has to
/// load and round-trip on Windows rather than failing to parse — the config is
/// shared, only the delivery is not.
pub struct DbusTarget(#[cfg_attr(not(target_os = "linux"), allow(dead_code))] OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for DbusTarget {
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    async fn deliver(&self, text: &str) -> DeliveryResult {
        #[cfg(target_os = "linux")]
        {
            let signal = self
                .0
                .dbus_signal
                .as_deref()
                .unwrap_or("ai.voxctrl.Routing.TextRouted");
            match emit_dbus_signal(signal, text).await {
                Ok(_) => return DeliveryResult::ok(text.into()),
                Err(e) => return DeliveryResult::err(e.to_string()),
            }
        }
        #[cfg(not(target_os = "linux"))]
        DeliveryResult::err("DBus not available on this platform")
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        return TestResult { reachable: true, detail: "DBus available (Linux)".into() };
        #[cfg(not(target_os = "linux"))]
        TestResult { reachable: false, detail: "DBus not available on this platform".into() }
    }
}

#[cfg(target_os = "linux")]
fn is_valid_dbus_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

#[cfg(target_os = "linux")]
async fn emit_dbus_signal(signal_name: &str, text: &str) -> anyhow::Result<()> {
    use zbus::Connection;

    if !is_valid_dbus_name(signal_name) {
        anyhow::bail!("Invalid D-Bus signal name '{signal_name}': only [A-Za-z0-9_.] allowed");
    }

    let conn = Connection::session().await?;
    let parts: Vec<&str> = signal_name.rsplitn(2, '.').collect();
    let (member, iface) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        (signal_name, signal_name)
    };
    let obj_path = format!("/{}", iface.replace('.', "/"));
    conn.emit_signal(None::<&str>, obj_path.as_str(), iface, member, &(text,))
        .await?;
    Ok(())
}

// ── HttpTarget ────────────────────────────────────────────────────────────────

pub struct HttpTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for HttpTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(url) = &self.0.http_url else {
            return DeliveryResult::err("No http_url configured");
        };
        let payload = build_json_payload(&self.0.http_json_template, text);
        let client = http_client();
        let mut req = client.request(
            self.0.http_method.parse().unwrap_or(reqwest::Method::POST),
            url,
        );
        if let Some(headers) = &self.0.http_headers {
            for (k, v) in headers {
                if let (Ok(name), Ok(val)) = (
                    k.parse::<reqwest::header::HeaderName>(),
                    v.parse::<reqwest::header::HeaderValue>(),
                ) {
                    req = req.header(name, val);
                }
            }
        }
        req = req.json(&payload);
        match req.send().await {
            Ok(r) if r.status().is_success() => DeliveryResult::ok(text.into()),
            Ok(r) => DeliveryResult::err(format!("HTTP {}", r.status())),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        if self.0.http_url.is_some() {
            TestResult { reachable: true, detail: "HTTP target configured".into() }
        } else {
            TestResult { reachable: false, detail: "No http_url configured".into() }
        }
    }
}

// ── WebhookTarget ─────────────────────────────────────────────────────────────

pub struct WebhookTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for WebhookTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let Some(url) = &self.0.webhook_url else {
            return DeliveryResult::err("No webhook_url configured");
        };
        let Some(secret) = &self.0.webhook_secret else {
            return DeliveryResult::err("No webhook_secret configured");
        };
        let payload = build_json_payload(&self.0.webhook_json_template, text);
        let body = serde_json::to_vec(&payload).unwrap_or_default();

        let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes())
            .expect("HMAC accepts any key size");
        mac.update(&body);
        let sig = hex::encode(mac.finalize().into_bytes());

        match http_client()
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Webhook-Signature", sig)
            .body(body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => DeliveryResult::ok(text.into()),
            Ok(r) => DeliveryResult::err(format!("HTTP {}", r.status())),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        if self.0.webhook_url.is_none() {
            return TestResult { reachable: false, detail: "No webhook_url configured".into() };
        }
        if self.0.webhook_secret.is_none() {
            return TestResult { reachable: false, detail: "No webhook_secret configured".into() };
        }
        TestResult { reachable: true, detail: "Webhook target configured".into() }
    }
}

// ── McpTarget ─────────────────────────────────────────────────────────────────

pub struct McpTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for McpTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let tool = self.0.mcp_tool.as_deref().unwrap_or("speak_text");
        let args = build_json_payload(&self.0.mcp_args, text);

        #[cfg(target_os = "linux")]
        let s = {
            let path = self.0.mcp_path.as_deref().unwrap_or("/tmp/voxctrl-mcp.sock");
            match UnixStream::connect(path).await {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(format!("Failed to connect to MCP socket {path}: {e}")),
            }
        };

        #[cfg(target_os = "windows")]
        let s = {
            use tokio::net::windows::named_pipe::ClientOptions;
            let path = self.0.mcp_path.as_deref().unwrap_or(r"\\.\pipe\voxctrl-mcp");
            match ClientOptions::new().open(path) {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(format!("Failed to connect to MCP named pipe {path}: {e}")),
            }
        };

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        return DeliveryResult::err("MCP target not supported on this platform");

        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (reader, mut writer) = tokio::io::split(s);
        let mut lines = BufReader::new(reader).lines();

        // Step 1: initialize request
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "VoxCtrl-Client",
                    "version": "1.0.0"
                }
            }
        });
        let payload = serde_json::to_string(&init_req).unwrap() + "\n";
        if let Err(e) = writer.write_all(payload.as_bytes()).await {
            return DeliveryResult::err(format!("Failed to write initialize to MCP: {e}"));
        }
        if let Err(e) = writer.flush().await {
            return DeliveryResult::err(format!("Failed to flush: {e}"));
        }

        // Read initialize response
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(err) = val.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown initialization error");
                        return DeliveryResult::err(format!("MCP initialization error: {msg}"));
                    }
                } else {
                    return DeliveryResult::err("Failed to parse JSON initialize response from MCP server");
                }
            }
            Ok(None) => return DeliveryResult::err("MCP server closed connection during initialization"),
            Err(e) => return DeliveryResult::err(format!("Failed to read initialize response from MCP server: {e}")),
        }

        // Step 2: notifications/initialized
        let initialized_notify = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let payload = serde_json::to_string(&initialized_notify).unwrap() + "\n";
        if let Err(e) = writer.write_all(payload.as_bytes()).await {
            return DeliveryResult::err(format!("Failed to write initialized notification to MCP: {e}"));
        }
        if let Err(e) = writer.flush().await {
            return DeliveryResult::err(format!("Failed to flush: {e}"));
        }

        // Step 3: tools/call
        let tool_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": args
            }
        });
        let payload = serde_json::to_string(&tool_req).unwrap() + "\n";
        if let Err(e) = writer.write_all(payload.as_bytes()).await {
            return DeliveryResult::err(format!("Failed to write tool call request to MCP: {e}"));
        }
        if let Err(e) = writer.flush().await {
            return DeliveryResult::err(format!("Failed to flush: {e}"));
        }

        // Read tool call response
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(err) = val.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown tool call error");
                        DeliveryResult::err(format!("MCP tool call error: {msg}"))
                    } else if let Some(res) = val.get("result") {
                        DeliveryResult::ok(serde_json::to_string(res).unwrap_or_else(|_| text.to_string()))
                    } else {
                        DeliveryResult::ok(text.to_string())
                    }
                } else {
                    DeliveryResult::err("Failed to parse JSON tool call response from MCP server")
                }
            }
            Ok(None) => DeliveryResult::err("MCP server closed connection during tool call"),
            Err(e) => DeliveryResult::err(format!("Failed to read tool call response from MCP server: {e}")),
        }
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        {
            let path = self.0.mcp_path.as_deref().unwrap_or("/tmp/voxctrl-mcp.sock");
            match UnixStream::connect(path).await {
                Ok(_) => TestResult { reachable: true, detail: format!("MCP socket {path} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            }
        }
        #[cfg(target_os = "windows")]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            let path = self.0.mcp_path.as_deref().unwrap_or(r"\\.\pipe\voxctrl-mcp");
            match ClientOptions::new().open(path) {
                Ok(_) => TestResult { reachable: true, detail: format!("MCP named pipe {path} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        TestResult { reachable: false, detail: "Platform not supported".into() }
    }
}

// ── SpeakTarget ───────────────────────────────────────────────────────────────

pub struct SpeakTarget(#[allow(dead_code)] OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for SpeakTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        if let Some(callback) = SPEAK_CALLBACK.get() {
            callback(text);
            DeliveryResult::ok(text.to_string())
        } else {
            DeliveryResult::err("TTS engine not initialized or speak callback not registered")
        }
    }

    async fn test(&self) -> TestResult {
        if SPEAK_CALLBACK.get().is_some() {
            TestResult {
                reachable: true,
                detail: "TTS speaker callback is registered".into(),
            }
        } else {
            TestResult {
                reachable: false,
                detail: "TTS speaker callback not registered".into(),
            }
        }
    }
}


// ── ChatTarget ────────────────────────────────────────────────────────────────
//
// Speaks to an OpenAI-compatible `/v1/chat/completions` endpoint — a local
// Hermes/Ollama/llama.cpp server, or a remote provider — and keeps the running
// conversation so each dictation is a turn in an ongoing exchange rather than
// an isolated request. The assistant's reply is returned as the delivered text
// and, depending on `chat_reply_mode`, spoken aloud, typed into the focused
// window, or copied to the clipboard.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

type Conversation = Arc<tokio::sync::Mutex<Vec<ChatMessage>>>;

/// Conversation state, keyed by target id.
///
/// Deliberately lives outside `ChatTarget` because `OutputTargetRouter::reload`
/// rebuilds every target whenever settings are saved — history held in the
/// target itself would be silently discarded mid-conversation.
///
/// Each conversation carries its own lock so a slow model on one target can't
/// stall a dictation routed to another.
fn chat_histories() -> &'static std::sync::Mutex<std::collections::HashMap<String, Conversation>> {
    static HISTORIES: OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Conversation>>,
    > = OnceLock::new();
    HISTORIES.get_or_init(Default::default)
}

fn conversation_for(target_id: &str) -> Conversation {
    chat_histories()
        .lock()
        .unwrap()
        .entry(target_id.to_string())
        .or_default()
        .clone()
}

/// Drop the stored conversation for one target. Returns the number of messages
/// that were discarded.
pub async fn reset_chat_history(target_id: &str) -> usize {
    // Clear in place rather than removing the entry, so a delivery already
    // holding this conversation's lock still writes into the same object.
    let convo = {
        let map = chat_histories().lock().unwrap();
        match map.get(target_id) {
            Some(c) => c.clone(),
            None => return 0,
        }
    };
    let mut guard = convo.lock().await;
    let dropped = guard.len();
    guard.clear();
    dropped
}

/// Read a target's conversation without modifying it (for UI / tests).
pub async fn chat_history(target_id: &str) -> Vec<ChatMessage> {
    let convo = {
        let map = chat_histories().lock().unwrap();
        match map.get(target_id) {
            Some(c) => c.clone(),
            None => return Vec::new(),
        }
    };
    let guard = convo.lock().await;
    guard.clone()
}

/// Normalize an endpoint into an OpenAI-style base URL ending in `/v1`, so the
/// user can paste `http://localhost:8080`, a trailing slash, or a full `/v1`.
fn chat_api_base(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

/// Fold to lowercase alphanumerics so "New conversation." matches "new conversation".
fn normalize_phrase(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct ChatTarget(OutputTarget);

impl ChatTarget {
    /// Send the assistant's reply onward according to `chat_reply_mode`.
    async fn surface_reply(&self, reply: &str) -> Option<String> {
        match self.0.chat_reply_mode.as_str() {
            "none" => None,
            "inject" => {
                let mut cfg = self.0.clone();
                cfg.delivery = DeliveryType::Inject;
                InjectTarget(cfg).deliver(reply).await.error
            }
            "clipboard" => {
                let mut cfg = self.0.clone();
                cfg.delivery = DeliveryType::Clipboard;
                ClipboardTarget(cfg).deliver(reply).await.error
            }
            // "speak" and anything unrecognized fall back to text-to-speech,
            // which is the mode this target exists for.
            _ => match SPEAK_CALLBACK.get() {
                Some(callback) => {
                    callback(reply);
                    None
                }
                None => Some("TTS engine not initialized or speak callback not registered".into()),
            },
        }
    }
}

#[async_trait::async_trait]
impl DeliveryTarget for ChatTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(endpoint) = self.0.chat_url.as_deref().filter(|u| !u.trim().is_empty()) else {
            return DeliveryResult::err("No chat_url configured");
        };
        let Some(model) = self.0.chat_model.as_deref().filter(|m| !m.trim().is_empty()) else {
            return DeliveryResult::err("No chat_model configured");
        };

        // A spoken reset phrase clears the conversation instead of becoming a turn.
        if let Some(phrase) = self
            .0
            .chat_reset_phrase
            .as_deref()
            .filter(|p| !p.trim().is_empty())
        {
            if normalize_phrase(text) == normalize_phrase(phrase) {
                let dropped = reset_chat_history(&self.0.id).await;
                tracing::info!(
                    target_id = %self.0.id,
                    dropped,
                    "Chat conversation reset by spoken phrase"
                );
                return DeliveryResult::ok(String::new());
            }
        }

        // Hold this conversation's lock across the request so two dictations to
        // the same target can't interleave and corrupt the turn order.
        let convo = conversation_for(&self.0.id);
        let mut history = convo.lock().await;
        history.push(ChatMessage {
            role: "user".into(),
            content: text.to_string(),
        });

        // Build the wire messages: system prompt (never trimmed) + recent turns.
        let mut messages: Vec<ChatMessage> = Vec::with_capacity(history.len() + 1);
        if let Some(system) = self
            .0
            .chat_system_prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            messages.push(ChatMessage {
                role: "system".into(),
                content: system.to_string(),
            });
        }
        let keep = self.0.chat_max_history as usize;
        let start = if keep > 0 {
            history.len().saturating_sub(keep)
        } else {
            0
        };
        messages.extend(history[start..].iter().cloned());

        let url = format!("{}/chat/completions", chat_api_base(endpoint));
        let body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });

        let mut req = http_client()
            .post(&url)
            .timeout(std::time::Duration::from_secs(self.0.chat_timeout_secs))
            .json(&body);
        if let Some(key) = self
            .0
            .chat_api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            req = req.bearer_auth(key);
        }

        let reply = match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(val) => val
                        .pointer("/choices/0/message/content")
                        .and_then(|c| c.as_str())
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    Err(e) => {
                        history.pop();
                        return DeliveryResult::err(format!("Chat response parse error: {e}"));
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let detail = resp.text().await.unwrap_or_default();
                history.pop();
                let detail = detail.chars().take(300).collect::<String>();
                return DeliveryResult::err(format!("Chat HTTP {status}: {detail}"));
            }
            Err(e) => {
                history.pop();
                return DeliveryResult::err(format!("Chat request failed: {e}"));
            }
        };

        // Roll back the user turn on an empty reply so the next request doesn't
        // resend a question the model already ignored.
        if reply.is_empty() {
            history.pop();
            return DeliveryResult::err("Chat API returned no content");
        }

        history.push(ChatMessage {
            role: "assistant".into(),
            content: reply.clone(),
        });
        // Trim stored history so a long-running conversation can't grow forever.
        // Kept at twice the send window so context survives a couple of turns
        // beyond what is actually transmitted.
        if keep > 0 && history.len() > keep * 2 {
            let excess = history.len() - keep * 2;
            history.drain(..excess);
        }
        drop(history);

        match self.surface_reply(&reply).await {
            Some(err) => DeliveryResult::err(format!("Chat reply delivery failed: {err}")),
            None => DeliveryResult::ok(reply),
        }
    }

    async fn test(&self) -> TestResult {
        let Some(endpoint) = self.0.chat_url.as_deref().filter(|u| !u.trim().is_empty()) else {
            return TestResult { reachable: false, detail: "No chat_url configured".into() };
        };
        if self
            .0
            .chat_model
            .as_deref()
            .map(|m| m.trim().is_empty())
            .unwrap_or(true)
        {
            return TestResult { reachable: false, detail: "No chat_model configured".into() };
        }

        let url = format!("{}/models", chat_api_base(endpoint));
        let mut req = http_client()
            .get(&url)
            .timeout(std::time::Duration::from_secs(5));
        if let Some(key) = self
            .0
            .chat_api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            req = req.bearer_auth(key);
        }
        match req.send().await {
            Ok(r) if r.status().is_success() => {
                TestResult { reachable: true, detail: format!("Chat API reachable at {url}") }
            }
            Ok(r) => TestResult { reachable: false, detail: format!("HTTP {} from {url}", r.status()) },
            Err(e) => TestResult { reachable: false, detail: e.to_string() },
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

/// Public alias for tests — not part of the stable API.
#[cfg(test)]
pub fn shellexpand_tilde_pub(s: &str) -> String {
    shellexpand_tilde(s)
}

/// Public alias for tests — not part of the stable API.
#[cfg(test)]
pub fn chat_api_base_pub(s: &str) -> String {
    chat_api_base(s)
}

fn shellexpand_tilde(s: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        if s == "~" {
            return home.to_string_lossy().into_owned();
        }
        if let Some(rest) = s.strip_prefix("~/") {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

fn build_json_payload(
    template: &Option<serde_json::Value>,
    text: &str,
) -> serde_json::Value {
    if let Some(tmpl) = template {
        substitute_text(tmpl.clone(), text)
    } else {
        serde_json::json!({ "text": text })
    }
}

fn substitute_text(val: serde_json::Value, text: &str) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => {
            serde_json::Value::String(s.replace("{TEXT}", text).replace("{text}", text))
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(|v| substitute_text(v, text)).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, substitute_text(v, text)))
                .collect(),
        ),
        other => other,
    }
}

// ── CommandTarget ──────────────────────────────────────────────────────────────

pub struct CommandTarget(pub OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for CommandTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        // Default to direct text injection when called directly
        InjectTarget(self.0.clone()).deliver(text).await
    }

    async fn test(&self) -> TestResult {
        TestResult {
            reachable: true,
            detail: "Voice Command router active".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceCommandParseResult {
    pub matched_target_id: String,
    pub payload: String,
}

fn is_valid_pre_target_fillers(pre: &str) -> bool {
    let cleaned = pre.trim_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation());
    if cleaned.is_empty() {
        return true;
    }

    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() > 10 {
        return false;
    }

    let filler_words = [
        "add", "put", "send", "write", "save", "log", "append", "record", "post", "push",
        "dispatch", "deliver", "type", "copy", "place", "insert", "route", "direct", "pass",
        "transfer", "go", "get", "take", "set", "store", "keep",
        "this", "that", "it", "us", "me", "is", "them", "some", "these", "those",
        "message", "text", "note", "entry", "content", "data", "info", "information", "payload",
        "to", "in", "into", "for", "on", "at", "with", "from", "onto", "through",
        "my", "the", "a", "an", "our", "your", "its",
        "please", "can", "you", "could", "would", "i", "want", "like", "need", "have", "too", "2",
        "so", "just", "now", "here", "also",
    ];

    for word in words {
        let clean_w = word.trim_matches(|c: char| c.is_ascii_punctuation()).to_lowercase();
        if clean_w.is_empty() {
            continue;
        }
        if !filler_words.contains(&clean_w.as_str()) {
            return false;
        }
    }

    true
}

fn clean_payload(post: &str) -> String {
    let trimmed = post.trim();
    let text_without_punct = trimmed
        .trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == ',' || c == '.' || c == ';' || c == '-' || c == '!' || c == '?' || c == '"' || c == '\'')
        .trim();

    let lower = text_without_punct.to_lowercase();
    let connectors = ["saying that", "saying", "that says", "that", "with text", "with content", "with"];

    for connector in &connectors {
        if lower.starts_with(connector) {
            let after_conn = &text_without_punct[connector.len()..];
            let cleaned = after_conn
                .trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == ',' || c == '.' || c == ';' || c == '-' || c == '!' || c == '?' || c == '"' || c == '\'')
                .trim();
            if !cleaned.is_empty() {
                return cleaned.to_string();
            }
        }
    }

    text_without_punct.to_string()
}

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();
    let mut dp = vec![vec![0; len2 + 1]; len1 + 1];
    for i in 0..=len1 { dp[i][0] = i; }
    for j in 0..=len2 { dp[0][j] = j; }
    for i in 1..=len1 {
        for j in 1..=len2 {
            if s1_chars[i - 1] == s2_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            } else {
                dp[i][j] = 1 + std::cmp::min(dp[i - 1][j - 1], std::cmp::min(dp[i - 1][j], dp[i][j - 1]));
            }
        }
    }
    dp[len1][len2]
}

/// Parse text for keyword "VoxCtrl" and target name/label matching.
/// Supports both direct commands (e.g. "VoxCtrl notes Hi there") and natural,
/// conversational phrasing (e.g. "VoxCtrl add this to my notes. What are you doing here?").
/// Returns `Some(VoiceCommandParseResult)` if the trigger keyword was found AND a target matched;
/// otherwise returns `None`.
pub fn parse_voice_command(
    text: &str,
    targets: &[OutputTarget],
) -> Option<VoiceCommandParseResult> {
    let lower_text = text.to_lowercase();
    let mut found_pos = None;
    let mut trigger_len = 0;

    // 1. Exact & standard triggers
    let exact_triggers = ["voxctrl", "vox ctrl", "vox-ctrl", "vox control"];
    for trigger in &exact_triggers {
        if let Some(pos) = lower_text.find(trigger) {
            if found_pos.map_or(true, |p| pos < p) {
                found_pos = Some(pos);
                trigger_len = trigger.len();
            }
        }
    }

    // 2. Dynamic pattern trigger for any "<word> control" or "<word> ctrl" phrase
    if found_pos.is_none() {
        let words: Vec<&str> = lower_text.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let clean_w = word.trim_matches(|c: char| c.is_ascii_punctuation());
            if clean_w == "control" || clean_w == "ctrl" || clean_w == "ctl" || clean_w == "kontrol" {
                if i > 0 {
                    let start_idx = lower_text.find(words[0]).unwrap_or(0);
                    let ctrl_pos = lower_text.find(word).unwrap_or(0);
                    let end_pos = ctrl_pos + word.len();
                    found_pos = Some(start_idx);
                    trigger_len = end_pos - start_idx;
                    break;
                }
            }
        }
    }

    // 3. Dynamic Levenshtein fuzzy match on leading token(s)
    if found_pos.is_none() {
        let words: Vec<&str> = lower_text.split_whitespace().collect();
        if !words.is_empty() {
            for len in (1..=2.min(words.len())).rev() {
                let candidate = words[..len].join(" ");
                let clean_cand = candidate.trim_matches(|c: char| c.is_ascii_punctuation());
                let dist1 = levenshtein_distance(clean_cand, "voxctrl");
                let dist2 = levenshtein_distance(clean_cand, "vox control");
                if dist1 <= 2 || dist2 <= 3 {
                    if let Some(pos) = lower_text.find(clean_cand) {
                        found_pos = Some(pos);
                        trigger_len = clean_cand.len();
                        break;
                    }
                }
            }
        }
    }

    let pos = found_pos?;
    let after_trigger = &text[pos + trigger_len..];

    // Flatten all target candidates (IDs and Labels) and sort by string length descending
    // so longer/more specific target names (e.g. "Personal Notes") take precedence over shorter ones ("Notes").
    let mut candidate_entries: Vec<(&str, &str)> = Vec::new();
    for target in targets {
        if target.delivery == DeliveryType::Command {
            continue;
        }
        if !target.id.is_empty() {
            candidate_entries.push((&target.id, &target.id));
        }
        if !target.label.is_empty() {
            candidate_entries.push((&target.id, &target.label));
        }
    }

    candidate_entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let after_trigger_lower = after_trigger.to_lowercase();

    for (target_id, candidate) in candidate_entries {
        let cand_lower = candidate.to_lowercase();

        let mut search_start = 0;
        while let Some(match_idx) = after_trigger_lower[search_start..].find(&cand_lower) {
            let abs_match_start = search_start + match_idx;
            let abs_match_end = abs_match_start + cand_lower.len();

            let is_boundary_start = abs_match_start == 0 || {
                let prev_char = after_trigger[..abs_match_start].chars().last().unwrap();
                prev_char.is_whitespace() || prev_char.is_ascii_punctuation()
            };

            let is_boundary_end = abs_match_end == after_trigger.len() || {
                let next_char = after_trigger[abs_match_end..].chars().next().unwrap();
                next_char.is_whitespace() || next_char.is_ascii_punctuation()
            };

            if is_boundary_start && is_boundary_end {
                let pre_target = &after_trigger[..abs_match_start];
                if is_valid_pre_target_fillers(pre_target) {
                    let post_target = &after_trigger[abs_match_end..];
                    let payload = clean_payload(post_target);

                    return Some(VoiceCommandParseResult {
                        matched_target_id: target_id.to_string(),
                        payload,
                    });
                }
            }

            search_start = abs_match_start + 1;
        }
    }

    None
}
