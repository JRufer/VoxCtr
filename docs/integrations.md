# Integrations

VoxCtrl exposes several interfaces for external tools and services to interact with it.

---

## MCP Server

**Crate:** `crates/voxctrl-mcp/`

### What is MCP?

The [Model Context Protocol](https://modelcontextprotocol.io/) is an open standard for LLM agents to call tools on local servers. VoxCtrl implements an MCP server that lets AI assistants like **Claude Desktop** or **Cursor IDE** trigger voice recording and TTS.

### Transport

| Platform | Socket |
|---|---|
| Linux | `/tmp/voxctrl-mcp.sock` (Unix domain socket) |
| Windows | `\\.\pipe\voxctrl-mcp` (named pipe) |

### Protocol

JSON-RPC 2.0 over the socket. Follows MCP spec v2024-11-05.

**Handshake:**
```json
// Client sends:
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"claude-desktop"}}}

// Server responds:
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"voxctrl","version":"1.0.0"}}}

// Client sends (notification, no id):
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

### Available Tools

#### `transcribe_voice`
Records audio and returns the transcription. Blocks until recording ends or timeout is reached.

```json
{
  "method": "tools/call",
  "params": {
    "name": "transcribe_voice",
    "arguments": { "timeout_seconds": 15 }
  }
}
```

`timeout_seconds` defaults to `mcp.record_timeout` (15.0 seconds). Returns `"(no speech detected)"` if no audio was captured.

Response:
```json
{"content": [{"type": "text", "text": "the transcribed text here"}]}
```

#### `speak_text`
Queues text for TTS playback. Returns immediately while audio plays asynchronously.

```json
{
  "method": "tools/call",
  "params": {
    "name": "speak_text",
    "arguments": { "text": "Recording started. Please speak now." }
  }
}
```

Returns: `{"content": [{"type": "text", "text": "spoken"}]}`

#### `get_status`
Returns current recording/speaking state.

```json
{"method": "tools/call", "params": {"name": "get_status", "arguments": {}}}
```

Response:
```json
{"content": [{"type": "text", "text": "{\"recording\": false, \"speaking\": false}"}]}
```

**Recommended pattern — speak then record safely:**
1. Call `speak_text` with your question
2. Poll `get_status` until `speaking = false`
3. Call `transcribe_voice`

### Enabling the MCP Server

In `config.json`:
```json
"mcp": {
  "server_enabled": true,
  "record_timeout": 15.0
}
```

### Configuring Claude Desktop

Add to `claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "voxctrl": {
      "command": "nc",
      "args": ["-U", "/tmp/voxctrl-mcp.sock"]
    }
  }
}
```

---

## DBus Service (Linux)

**Crate:** `crates/voxctrl-dbus/`

### Interface

VoxCtrl registers on the session bus as:
- **Bus name:** `ai.voxctrl.Dictation`
- **Object path:** `/ai/voxctrl/Dictation`
- **Interface:** `ai.voxctrl.Dictation`

### Methods

| Method | Signature | Description |
|---|---|---|
| `start_recording()` | `() → ()` | Begin recording |
| `stop_recording()` | `() → ()` | Stop recording and process audio |
| `toggle_recording()` | `() → ()` | Toggle recording state |
| `get_status()` | `() → s` | Returns `"idle"`, `"recording"`, or `"transcribing"` |
| `get_word_count()` | `() → u` | Total words dictated this session |

### Signals

| Signal | Signature | Description |
|---|---|---|
| `status_changed` | `(s)` | Emitted when recording state changes |
| `text_injected` | `(s)` | Emitted after text is delivered to a target |

### Example Usage

```bash
# Start recording
dbus-send --session --dest=ai.voxctrl.Dictation \
  /ai/voxctrl/Dictation \
  ai.voxctrl.Dictation.start_recording

# Watch for injected text
dbus-monitor --session "type='signal',interface='ai.voxctrl.Dictation'"

# Get current status ("idle", "recording", or "transcribing")
dbus-send --session --print-reply \
  --dest=ai.voxctrl.Dictation \
  /ai/voxctrl/Dictation \
  ai.voxctrl.Dictation.get_status
```

The DBus service is a stub on non-Linux platforms (compiles but does nothing).

---

## OpenAI-compatible LLM Integration

**Crate:** `crates/voxctrl-llm/`

### Purpose

After Whisper transcribes speech, text can optionally be rewritten by an LLM
served over the **OpenAI API**. This works with any compatible server — a local
server or a hosted provider. Enabled per-hotkey binding via
`openai_enabled = true` in `bindings.toml` (or via the Hotkeys tab in the GUI
settings).

The client calls `POST {endpoint}/v1/chat/completions` for generation and
`GET {endpoint}/v1/models` to list models. The `/v1` suffix is appended
automatically if the configured endpoint doesn't already include it, so the
default `http://localhost:11434` targets a local server's OpenAI-compatible
endpoint. When `api_key` is set it is sent as an `Authorization: Bearer <key>`
header.

> Configs written before the rename used the key `ollama` (and `ollama_*` binding
> fields); those names are still accepted via serde aliases and load transparently.

### Configuration

```json
"openai": {
  "enabled": false,
  "endpoint": "http://localhost:11434",
  "api_key": null,
  "model": "llama3.2:1b",
  "mode": "clean",
  "system_prompt": "Fix grammar and punctuation only. Return only the corrected text, no commentary.",
  "user_prompt": "{text}",
  "timeout_secs": 8
}
```

For a hosted provider, set `endpoint` to its base URL (e.g.
`https://api.openai.com/v1`), set `api_key`, and choose a `model` the provider
offers.

### System & User Prompts

Each request sends two chat messages:

- **System prompt** (`system_prompt`) — describes how to transform the text. Leave
  empty to send no system message.
- **User prompt** (`user_prompt`) — the message itself. It must contain `{text}`,
  which is replaced with the transcribed speech. If the placeholder is missing,
  the transcribed text is appended on a new line as a fallback.

### Presets

The `mode` field selects a preset. The built-in presets are **read-only**:
selecting one fills the system prompt with a fixed value (below) and sets the
user prompt to the plain `{text}` passthrough. To edit the system and user
prompts yourself, choose the `custom` preset. Generation always uses
`system_prompt`/`user_prompt`.

| Preset | System prompt |
|---|---|
| `clean` | "Fix grammar and punctuation only. Return only the corrected text, no commentary." |
| `formal` | "Rewrite the user's text in formal professional language. Return only the result." |
| `casual` | "Rewrite the user's text in casual conversational language. Return only the result." |
| `bullet` | "Convert the user's text to a bullet-point list. Return only the list." |
| `concise` | "Summarize the user's text concisely in 1-2 sentences. Return only the summary." |
| `custom` | No preset — edit the system/user prompts freely |

### Per-Hotkey Overrides

A hotkey binding with `openai_enabled = true` can override the global defaults
for that hotkey only:

- `openai_system_prompt` — overrides the global system prompt (leave empty to
  inherit it).
- `openai_prompt` — overrides the global user prompt template (must contain
  `{text}`; leave empty to inherit it).
- `openai_model` — overrides the model (leave empty to inherit it).

This lets different hotkeys apply different rewriting styles while sharing the
same connection settings.

### Availability Caching

`OpenAiClient.is_available()` probes `GET {endpoint}/v1/models` on first call and **caches the result**. If the server was unreachable at startup, it will appear unreachable until the availability cache is reset (e.g. by changing the endpoint in settings).

### Graceful Fallback

If the server is unreachable, the HTTP request times out, or the response cannot be parsed, VoxCtrl logs the failure and delivers the **original** Whisper transcription unchanged. Text is never dropped.

### Testing the Connection

Via the Settings → OpenAI API tab → "Test Connection" button, or via IPC:
```typescript
const result = await invoke('test_openai', {
  endpoint: 'http://localhost:11434',
  apiKey: null,
  timeoutSecs: 5
});
// result: { success: boolean, message: string, models: string[] }
```

---

## HTTP Webhooks

### Endpoint Delivery (`http` type)
POST with JSON body to `http_url`:
```
POST https://your-endpoint.com/voice
Content-Type: application/json

{"text": "transcribed text"}
```

Configurable: `http_method`, `http_headers`, `http_json_template`.

### Signed Webhooks (`webhook` type)
Same POST (to `webhook_url`) but with HMAC-SHA256 signature:
```
X-VoxCtrl-Signature: sha256=abc123...
```

