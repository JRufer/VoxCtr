//! JSON-RPC 2.0 MCP server over a Unix domain socket (Linux) or a named pipe (Windows).
//!
//! Exposed tools:
//!   - transcribe_voice(timeout_seconds) → {text}
//!   - speak_text(text) → {}
//!   - get_status() → {recording, speaking}

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

pub const SOCKET_PATH: &str = "/tmp/voxctrl-mcp.sock";

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ── App callbacks ─────────────────────────────────────────────────────────────

/// Callbacks that the MCP server uses to interact with the app coordinator.
pub trait McpCallbacks: Send + Sync + 'static {
    /// Start recording and return the transcribed text (blocks until done or timeout).
    fn transcribe_voice(&self, timeout_secs: f64) -> impl std::future::Future<Output = Result<String>> + Send;

    /// Queue text for TTS playback.
    fn speak_text(&self, text: String) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Get current status flags.
    fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send;

    /// The user's configured recording timeout (Settings → General → Record
    /// timeout), in seconds. Used for `transcribe_voice` whenever the caller
    /// does not pass an explicit `timeout_seconds`, and advertised as the
    /// default in the tool schema. Read per call so a change in Settings
    /// applies without restarting the server.
    fn default_record_timeout(&self) -> impl std::future::Future<Output = f64> + Send;
}

// ── Server ────────────────────────────────────────────────────────────────────

pub async fn run_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    #[cfg(target_os = "linux")]
    run_unix_server(callbacks).await?;

    #[cfg(target_os = "windows")]
    run_named_pipe_server(callbacks).await?;

    Ok(())
}

// ── Unix socket server (Linux) ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
async fn run_unix_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    use tokio::net::UnixListener;

    let path = PathBuf::from(SOCKET_PATH);
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path)?;
    // Restrict to the owning user only so other local users cannot
    // activate the microphone or issue TTS via the MCP socket.
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    info!("MCP server listening on {SOCKET_PATH}");

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let cb = callbacks.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, cb).await {
                        debug!("MCP connection error: {e}");
                    }
                });
            }
            Err(e) => warn!("MCP accept error: {e}"),
        }
    }
}

async fn handle_connection<S, C>(stream: S, callbacks: Arc<C>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    C: McpCallbacks,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        debug!("MCP ← {line}");
        if let Some(response) = dispatch(&line, &callbacks).await {
            let out = serde_json::to_string(&response)? + "\n";
            debug!("MCP → {out}");
            writer.write_all(out.as_bytes()).await?;
        }
    }
    Ok(())
}

// ── Windows named pipe server ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
async fn run_named_pipe_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    // Windows named pipes use the \\.\pipe\ namespace
    // tokio supports this via tokio::net::windows::named_pipe
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    const PIPE_NAME: &str = r"\\.\pipe\voxctrl-mcp";
    info!("MCP server listening on {PIPE_NAME}");

    loop {
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .create(PIPE_NAME)?;

        server.connect().await?;

        let cb = callbacks.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(server, cb).await {
                debug!("MCP connection error: {e}");
            }
        });
    }
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

async fn dispatch<C: McpCallbacks>(raw: &str, cb: &Arc<C>) -> Option<JsonRpcResponse> {
    let req: JsonRpcRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => return Some(JsonRpcResponse::err(None, -32700, format!("Parse error: {e}"))),
    };

    let id = req.id.clone();

    // If it's a notification (no id) and NOT initialize, do not return a response
    if id.is_none() && req.method != "initialize" {
        return None;
    }

    let response = match req.method.as_str() {
        "initialize" => {
            JsonRpcResponse::ok(id, json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "voxctrl",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {}
                }
            }))
        }
        "notifications/initialized" => {
            JsonRpcResponse::ok(id, json!({}))
        }
        "tools/list" => {
            JsonRpcResponse::ok(id, get_tool_list(cb.default_record_timeout().await))
        }
        "tools/call" => {
            let name = req.params.as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let args = req.params.as_ref()
                .and_then(|p| p.get("arguments"));

            match call_tool(name, args, cb).await {
                Ok(res) => JsonRpcResponse::ok(id, res),
                Err(e) => JsonRpcResponse::err(id, -32603, e.to_string()),
            }
        }
        other => {
            JsonRpcResponse::err(id, -32601, format!("Method not found: {other}"))
        }
    };

    Some(response)
}

// ── Tool List Definition ──────────────────────────────────────────────────────

fn get_tool_list(default_record_timeout: f64) -> Value {
    json!({
        "tools": [
            {
                "name": "transcribe_voice",
                "description": "Records the user's voice through their microphone and returns the transcribed text.\n\nHOW IT WORKS:\n  Calling this tool immediately activates the user's microphone. The user speaks, and when they stop (or the timeout is reached) the audio is transcribed locally using Whisper and the text is returned. The microphone indicator in the app's system tray will show a recording state while this tool is active.\n\nWHEN TO USE:\n  - Whenever you need a spoken response or clarification from the user.\n  - To conduct a voice-driven conversation: speak a question with speak_text, then call transcribe_voice to capture the answer.\n  - When the user has indicated they prefer to respond by voice rather than typing.\n  - To capture dictated content such as notes, messages, or commands.\n\nWHEN NOT TO USE:\n  - Do not call while get_status shows recording=true (a recording is already in progress). Check status first if unsure.\n  - Do not call while get_status shows speaking=true; wait for TTS to finish so the microphone does not pick up the synthesised voice.\n  - Do not loop rapidly on empty results; if '(no speech detected)' is returned twice in a row, inform the user and wait for a typed prompt instead.\n\nPARAMETERS:\n  timeout_seconds (number, optional): Maximum wall-clock seconds to wait for the user to finish speaking. Use a shorter value (5–8 s) for quick yes/no questions. Use a longer value (30–60 s) when asking the user to dictate a paragraph or give detailed instructions.\n\nRETURN VALUE:\n  A plain-text string containing the transcribed speech. If no speech was detected within the timeout the string will be '(no speech detected)'. The transcript may contain minor errors from the speech model; treat it as lightly noisy text and correct obvious errors from context before acting on it.\n\nEXAMPLE FLOW — voice Q&A:\n  1. speak_text(\"What city are you in?\")\n  2. transcribe_voice(timeout_seconds=10)  → \"I'm in Seattle\"\n  3. Use 'Seattle' in subsequent tool calls or responses.\n\nEXAMPLE FLOW — voice dictation:\n  1. speak_text(\"Please dictate your message now.\")\n  2. transcribe_voice(timeout_seconds=45)  → full dictated message\n  3. Present the transcript back to the user for confirmation before sending.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "timeout_seconds": {
                            "type": "number",
                            "description": format!("Maximum seconds to wait for the user to finish speaking. Defaults to {default_record_timeout} (the Record timeout set in VoxCtrl Settings). Use 5–8 for short answers, 30–60 for dictation.")
                        }
                    }
                }
            },
            {
                "name": "speak_text",
                "description": "Converts text to speech and plays it aloud through the user's speakers.\n\nHOW IT WORKS:\n  The text is queued for playback using the locally configured TTS engine (piper neural TTS by default, espeak-ng as fallback). Playback happens asynchronously — this tool returns immediately while audio plays in the background. The app's system tray will show a speaking indicator. Only one utterance plays at a time; additional calls are queued and played in order.\n\nWHEN TO USE:\n  - To read your response aloud so the user does not have to look at the screen.\n  - To prompt the user before calling transcribe_voice (speak the question, then listen).\n  - To confirm actions: \"Done — I've sent your message.\"\n  - For accessibility: whenever the user has indicated they prefer audio output.\n  - To narrate step-by-step instructions the user can follow hands-free.\n\nWHEN NOT TO USE:\n  - Do not pass extremely long text (>500 words) in a single call; break long content into logical paragraphs and call speak_text once per paragraph so the user can interrupt between them.\n  - Do not include markdown syntax (**, ##, -, etc.) — it will be read aloud literally. Strip formatting before speaking.\n  - Do not include URLs, file paths, or code snippets; summarise them in plain prose instead.\n  - If get_status shows speaking=true and you need to ask a follow-up question, queue the next speak_text call; do not call transcribe_voice until speaking=false.\n\nPARAMETERS:\n  text (string, required): The plain-text content to speak. Should be natural prose — complete sentences, no markdown, no raw symbols. Punctuation is used by the TTS engine for pacing; commas and periods produce natural pauses.\n\nRETURN VALUE:\n  Returns the string 'spoken' when the text has been successfully queued for playback. This does NOT mean playback is complete — use get_status to check speaking=false if you need to wait before recording.\n\nEXAMPLE — confirm then record:\n  1. speak_text(\"I'll listen for your answer. Go ahead.\")\n  2. # Poll until speaking=false before recording\n  3. transcribe_voice(timeout_seconds=15)\n\nEXAMPLE — multi-part narration:\n  1. speak_text(\"Here are your three reminders for today.\")\n  2. speak_text(\"First: team standup at 9 AM.\")\n  3. speak_text(\"Second: review the pull request from Jordan.\")\n  4. speak_text(\"Third: submit your expense report by 5 PM.\")",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Plain-text content to speak aloud. Use natural prose with punctuation for pacing. No markdown, no code, no URLs."
                        }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "get_status",
                "description": "Returns the current state of the voice interface as a JSON object.\n\nHOW IT WORKS:\n  Queries the running VoxCtrl app for its live recording and TTS state. This is a lightweight, non-blocking call that returns immediately.\n\nWHEN TO USE:\n  - Before calling transcribe_voice: confirm recording=false so you do not start a second recording session while one is already active.\n  - Before calling transcribe_voice after speak_text: confirm speaking=false so the microphone does not capture TTS audio.\n  - To implement a wait loop: poll get_status every 1–2 seconds until speaking=false before proceeding to record.\n  - To diagnose unexpected behaviour: if transcribe_voice returns no speech, check whether the app is still in a speaking state.\n\nRETURN VALUE:\n  A JSON object with the following fields:\n    recording (boolean): true while the microphone is actively capturing audio for transcription. Only one recording session can run at a time.\n    speaking (boolean): true while TTS audio is currently playing through the speakers. The queue may contain additional utterances that will play after the current one finishes.\n\nEXAMPLE RESPONSE:\n  {\"recording\": false, \"speaking\": false}  — idle, safe to record or speak\n  {\"recording\": true,  \"speaking\": false}  — recording in progress\n  {\"recording\": false, \"speaking\": true}   — TTS playing, wait before recording\n\nRECOMMENDED PATTERN — speak then record safely:\n  1. speak_text(\"Your question here.\")\n  2. Loop: get_status → if speaking=true, wait 1 s and repeat\n  3. transcribe_voice(timeout_seconds=15)\n\nNOTE: Both fields can briefly be false between a speak_text call returning and the audio actually starting. If precise synchronisation matters, add a short delay (0.5 s) after speak_text before beginning to poll.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }
        ]
    })
}

// ── Tool Call Execution ───────────────────────────────────────────────────────

async fn call_tool<C: McpCallbacks>(name: &str, args: Option<&Value>, cb: &Arc<C>) -> Result<Value> {
    match name {
        "transcribe_voice" => {
            let timeout = match args.and_then(|a| a.get("timeout_seconds")).and_then(|t| t.as_f64()) {
                Some(t) => t,
                None => cb.default_record_timeout().await,
            };

            let text = cb.transcribe_voice(timeout).await?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": if text.is_empty() { "(no speech detected)".to_string() } else { text }
                    }
                ]
            }))
        }
        "speak_text" => {
            let text = args
                .and_then(|a| a.get("text"))
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("speak_text requires 'text' argument"))?
                .to_string();

            cb.speak_text(text).await?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": "spoken"
                    }
                ]
            }))
        }
        "get_status" => {
            let (recording, speaking) = cb.get_status().await;
            let status = json!({
                "recording": recording,
                "speaking": speaking
            });
            let status_str = serde_json::to_string(&status)?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": status_str
                    }
                ]
            }))
        }
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Records the timeout `transcribe_voice` was actually called with, and
    /// reports a configured default distinguishable from the old hard-coded
    /// 15.0 so a regression cannot pass by coincidence.
    struct FakeApp {
        configured_timeout: f64,
        /// Last timeout seen by `transcribe_voice`, as f64 bits.
        seen: AtomicU64,
    }

    impl FakeApp {
        fn new(configured_timeout: f64) -> Arc<Self> {
            Arc::new(Self {
                configured_timeout,
                seen: AtomicU64::new(f64::NAN.to_bits()),
            })
        }

        fn seen_timeout(&self) -> f64 {
            f64::from_bits(self.seen.load(Ordering::SeqCst))
        }
    }

    impl McpCallbacks for FakeApp {
        fn transcribe_voice(
            &self,
            timeout_secs: f64,
        ) -> impl std::future::Future<Output = Result<String>> + Send {
            async move {
                self.seen.store(timeout_secs.to_bits(), Ordering::SeqCst);
                Ok("hello".to_string())
            }
        }

        fn speak_text(&self, _text: String) -> impl std::future::Future<Output = Result<()>> + Send {
            async move { Ok(()) }
        }

        fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send {
            async move { (false, false) }
        }

        fn default_record_timeout(&self) -> impl std::future::Future<Output = f64> + Send {
            async move { self.configured_timeout }
        }
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn transcribe_request(arguments: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "transcribe_voice", "arguments": arguments},
        })
        .to_string()
    }

    #[test]
    fn omitted_timeout_uses_the_configured_record_timeout() {
        let app = FakeApp::new(42.0);
        let res = block_on(dispatch(&transcribe_request(json!({})), &app));

        assert!(res.is_some(), "request produced no response");
        assert_eq!(app.seen_timeout(), 42.0);
    }

    #[test]
    fn missing_arguments_object_still_uses_the_configured_timeout() {
        let app = FakeApp::new(7.5);
        let raw = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "transcribe_voice"},
        })
        .to_string();

        block_on(dispatch(&raw, &app));
        assert_eq!(app.seen_timeout(), 7.5);
    }

    #[test]
    fn explicit_timeout_wins_over_the_configured_one() {
        let app = FakeApp::new(42.0);
        block_on(dispatch(
            &transcribe_request(json!({"timeout_seconds": 3.0})),
            &app,
        ));
        assert_eq!(app.seen_timeout(), 3.0);
    }

    #[test]
    fn tool_schema_advertises_the_configured_timeout() {
        let app = FakeApp::new(42.0);
        let raw = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
        let res = block_on(dispatch(&raw, &app)).expect("no response");

        let described = serde_json::to_string(&res.result.unwrap()).unwrap();
        assert!(
            described.contains("Defaults to 42"),
            "tools/list did not advertise the configured default: {described}"
        );
    }
}
