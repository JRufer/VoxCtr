# VoxCtrl MCP Server

VoxCtrl ships a built-in **Model Context Protocol (MCP)** server that exposes the app's voice I/O pipeline as tools any MCP-capable AI can call. An agent can trigger the microphone, receive the transcript, and queue a spoken response — all through a standardised JSON-RPC 2.0 interface.

---

## Overview

When the MCP server is enabled, VoxCtrl listens on a local transport and presents three tools:

| Tool | What it does |
|---|---|
| `transcribe_voice` | Opens the mic, records the user, returns the transcript |
| `speak_text` | Queues text for TTS playback through the current voice |
| `get_status` | Returns whether the mic is open and TTS is playing |

This lets an AI agent have a full voice conversation:

```
Agent → transcribe_voice()  → user speaks → transcript returned
Agent → speak_text("…")     → VoxCtrl speaks the response aloud
Agent → transcribe_voice()  → next turn …
```

---

## Prerequisites

### Required system packages

| Package | Purpose | Install |
|---|---|---|
| `piper` | Neural TTS engine | `yay -S piper-tts` or [piper releases](https://github.com/rhasspy/piper/releases) |
| `aplay` | PCM audio playback (Linux) | `sudo pacman -S alsa-utils` |
| `socat` | Stdio ↔ Unix socket bridge (Claude Desktop Linux) | `sudo pacman -S socat` |

**Automatic Fallback Engine**:
`espeak-ng` serves as an automatic fallback when the configured `piper` engine fails or its local ONNX voice models are missing. To ensure fallback functions correctly, install `espeak-ng`:

```bash
sudo pacman -S espeak-ng
```

### Voice models

Voice models are downloaded from inside the app. Go to **Settings → TTS**, select a voice from the picker, and click **⬇ Download**. Models are stored locally in:

```
~/.local/share/voxctrl/piper-voices/
```

---

## Enabling the MCP Server

### Via Settings UI

1. Open **Settings → General**
2. Scroll to the **MCP Server** section
3. Toggle **"Enable MCP Server"**
4. The server will bind to the standard socket/pipe path shown in the settings window.

### Via config.json

Add the `mcp` config block to your active `config.json` configuration file located at `~/.config/voxctrl/config.json`:

```json
{
  "mcp": {
    "server_enabled": true,
    "record_timeout": 15.0,
    "visual_feedback": true
  }
}
```

The server starts automatically when the app launches if `server_enabled` is `true`.

---

## Transport

The transport layer is platform-dependent:

* **Linux**: A **Unix domain socket** located at:
  ```
  /tmp/voxctrl-mcp.sock
  ```
* **Windows**: A **Named Pipe** located at:
  ```
  \\.\pipe\voxctrl-mcp
  ```

Each connection is spawned as an asynchronous `tokio` task. The protocol is newline-delimited JSON-RPC 2.0: one JSON object per line, terminated with `\n`.

---

## Protocol

Standard MCP / JSON-RPC 2.0. Every request must include `"jsonrpc": "2.0"`.

### Handshake

```json
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
← {"jsonrpc":"2.0","id":1,"result":{
     "protocolVersion":"2024-11-05",
     "capabilities":{"tools":{}},
     "serverInfo":{"name":"voxctrl","version":"1.0.0"}
   }}
```

After `initialize`, send `notifications/initialized` (no response expected):

```json
→ {"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
```

### List tools

```json
→ {"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
← {"jsonrpc":"2.0","id":2,"result":{"tools":[…]}}
```

---

## Tools

### `transcribe_voice`

Opens the microphone and returns a transcript when speech ends.

**Parameters**

| Name | Type | Default | Description |
|---|---|---|---|
| `timeout_seconds` | `number` | `mcp.record_timeout` (`15.0`) | Maximum seconds to wait for speech before returning. Omit it to use the timeout configured in Settings → General |

**Request**

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "transcribe_voice",
    "arguments": {"timeout_seconds": 10.0}
  }
}
```

**Response — speech detected**

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [{"type": "text", "text": "Schedule a meeting for Thursday at 3 pm"}]
  }
}
```

**Response — no speech / silence**

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [{"type": "text", "text": "(no speech detected)"}]
  }
}
```

**Behaviour notes**

* While recording, the active waveform or recording overlay is shown — the user always has a visual indicator that the mic is live.
* The microphone is released automatically once VAD detects silence or `timeout_seconds` elapses.
* VoxCtrl's full post-processing pipeline (including OpenAI API LLM formatting/cleaning if enabled) is applied before the transcript is returned.

---

### `speak_text`

Queues text for playback using the configured TTS voice.

**Parameters**

| Name | Type | Required | Description |
|---|---|---|---|
| `text` | `string` | yes | The text to speak |

**Request**

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "speak_text",
    "arguments": {"text": "The meeting has been scheduled."}
  }
}
```

**Response**

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "content": [{"type": "text", "text": "spoken"}]
  }
}
```

**Behaviour notes**

* Returns as soon as the text is queued — does not block until playback finishes.
* If `response_overlay` is enabled, a visual speaking overlay is displayed while TTS plays.
* The user can interrupt playback at any time with the configured TTS stop key (default: `Escape`).
* If `piper` is not installed, fails to spawn, or its ONNX voice files are missing, the system automatically falls back to speaking via `espeak-ng`.

**Error — missing `text` argument**

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "error": {"code": -32603, "message": "speak_text requires 'text' argument"}
}
```

---

### `get_status`

Returns the current state of audio I/O.

**Request**

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "tools/call",
  "params": {"name": "get_status", "arguments": {}}
}
```

**Response**

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "content": [{"type": "text", "text": "{\"recording\": false, \"speaking\": true}"}]
  }
}
```

The `text` field is a JSON-encoded object:

| Field | Type | Description |
|---|---|---|
| `recording` | `boolean` | `true` while the microphone is open |
| `speaking` | `boolean` | `true` while TTS is playing |

---

## Error Codes

| Code | Meaning |
|---|---|
| `-32700` | Parse error (malformed JSON) |
| `-32601` | Method not found |
| `-32603` | Internal error (unknown tool name, missing required argument, callback exception) |

---

## Connecting: Claude Desktop (Linux)

Add the following to `~/.config/claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "voxctrl": {
      "command": "socat",
      "args": ["STDIO", "UNIX-CONNECT:/tmp/voxctrl-mcp.sock"]
    }
  }
}
```

Restart Claude Desktop. The tools `transcribe_voice`, `speak_text`, and `get_status` appear automatically in the tool picker.

> **Note:** VoxCtrl must already be running before Claude Desktop connects.

---

## Connecting: Raw Socket (Python)

```python
import socket
import json
import platform

SOCK = "/tmp/voxctrl-mcp.sock"
PIPE = r"\\.\pipe\voxctrl-mcp"

def rpc(sock, method, params=None, rpc_id=1):
    req = {"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params or {}}
    sock.sendall((json.dumps(req) + "\n").encode())
    data = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            break
        data += chunk
        if b"\n" in data:
            break
    return json.loads(data.split(b"\n")[0])

# Linux/Unix connection example
if platform.system() != "Windows":
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(SOCK)
        
        # Handshake
        rpc(s, "initialize")
        s.sendall((json.dumps({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}) + "\n").encode())
    
        # Ask the user a question
        rpc(s, "tools/call", {"name": "speak_text", "arguments": {"text": "What would you like to do?"}}, rpc_id=2)
    
        # Record the reply
        resp = rpc(s, "tools/call", {"name": "transcribe_voice", "arguments": {"timeout_seconds": 15}}, rpc_id=3)
        transcript = resp["result"]["content"][0]["text"]
        print("User said:", transcript)
```

---

## Connecting: Shell / socat (Linux)

```bash
# One-shot: list tools
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
  | socat - UNIX-CONNECT:/tmp/voxctrl-mcp.sock \
  | head -1 | python3 -m json.tool
```

---

## Response Loopback (FIFO pipe)

For agents that generate responses to a named FIFO, VoxCtrl can dynamically read those responses and speak them automatically — without needing the agent to call `speak_text` directly.

### How it works

1. The user configures an output target specifying a `response_pipe` path.
2. The Tauri application spawns an asynchronous `run_fifo_responder` task that watches this FIFO file.
3. When the agent writes its response text to the FIFO, VoxCtrl reads each line and pushes it to the TTS Engine worker queue.
4. Playback proceeds automatically, showing the visual speaking overlay.
5. The task automatically handles agent disconnects (EOF) and will reconnect on the next session without restarting VoxCtrl.

### Configuration in `targets.toml`

```toml
[[target]]
id = "my-agent"
label = "My Agent"
delivery = "pipe"
pipe_path = "/tmp/my-agent.in"
response_pipe = "/tmp/my-agent.out"   # ← agent writes responses here
append_newline = true
```

---

## TTS Configuration

All TTS settings live in `~/.config/voxctrl/config.json` under the `tts` key (**Settings → TTS**). MCP settings live under the `mcp` key (**Settings → General**).

| Key | Type | Default | Description |
|---|---|---|---|
| `tts.enabled` | `bool` | `false` | Master TTS on/off switch |
| `tts.engine` | `string` | `"piper"` | `"piper"` or `"espeak"` |
| `tts.voice` | `string` | `"en-us-lessac-medium"` | Voice ID from the catalog |
| `tts.stop_key` | `string[]` | `["KEY_ESCAPE"]` | Hotkeys to stop TTS playback |
| `tts.response_overlay` | `bool` | `true` | Show speaking overlay while TTS plays |
| `mcp.server_enabled` | `bool` | `false` | Start the MCP server on launch |
| `mcp.record_timeout` | `number` | `15.0` | Timeout used by `transcribe_voice` when the caller passes no `timeout_seconds`. Read per call, so it applies without a restart; a non-positive value falls back to `15.0` |
| `mcp.visual_feedback` | `bool` | `true` | Show overlay indicator while MCP server is listening to microphone |

### Available voices

| Voice ID | Language | Quality | Sample Rate |
|---|---|---|---|
| `en-us-libritts-high` | US English | High | 22050 Hz |
| `en-us-amy-low` | US English | Low | 16000 Hz |
| `en-us-kathleen-low` | US English | Low | 16000 Hz |
| `en-gb-southern_english_female-low` | GB English | Low | 16000 Hz |
| `en-us-ryan-high` | US English | High | 22050 Hz |
| `en-us-ryan-medium` | US English | Medium | 22050 Hz |
| `en-us-ryan-low` | US English | Low | 16000 Hz |
| `en-us-lessac-medium` | US English | Medium | 16000 Hz |
| `en-us-lessac-low` | US English | Low | 16000 Hz |
| `en-us-danny-low` | US English | Low | 16000 Hz |
| `en-gb-alan-low` | GB English | Low | 16000 Hz |

Download voices in **Settings → TTS → Voice Picker → ⬇ Download**.

---

## MCP Server Internals

### Socket and pipe paths

* **Linux**: `/tmp/voxctrl-mcp.sock`
* **Windows**: `\\.\pipe\voxctrl-mcp`

### Threading model

* Spawns an asynchronous transport listener loop via `tokio::spawn` upon application startup if enabled in settings.
* Each incoming connection spawns its own async tokio task (`tokio::spawn`), enabling fully non-blocking handling of concurrent client requests.
* The `transcribe_voice` handler operates asynchronously: other connections can continue to list status or write spoken text without being blocked.

### Recording synchronisation

`transcribe_voice` is handled by the shared atomic `AppState` structure:

1. The tool handler locks and clears `last_text` (a thread-safe Mutex-protected string representing the latest transcription result).
2. It sets the `recording` atomic boolean to `true`, which immediately triggers the active recording overlays in the Tauri/Svelte frontend.
3. A background timer task is spawned that sleeps for `timeout_seconds` and automatically flips `recording` back to `false` (and feeds a sentinel empty buffer to the audio channel to flush the VAD processor).
4. The tool task polls `self.is_recording()` at 50ms intervals until the recording session closes (triggered by the timer or manual user stop).
5. Once recording ends, the task polls the `last_text` buffer for up to 3.0 seconds (60 iterations × 50ms) to allow the local Whisper.cpp or Moonshine inference worker thread to compile the waveform.
6. The resulting transcription text is packaged into standard MCP JSON-RPC format and returned. If no speech is recorded or the buffer is blank, `(no speech detected)` is returned.

---

## Troubleshooting

**Socket does not exist**

VoxCtrl is not running, or the MCP server is disabled. Enable it in **Settings → General** or set `"mcp": { "server_enabled": true }` in `config.json` and restart.

**`socat` connection refused**

The socket exists but the server is not listening yet. Wait a moment after VoxCtrl starts, or check the app's console output for errors.

**TTS plays but no audio**

* **Linux**: Check that `aplay` is installed (`which aplay`).
* Verify the voice model is downloaded: models live in `~/.local/share/voxctrl/piper-voices/`.
* Try changing the global TTS engine to `"espeak"` in settings or `config.json` as a fallback choice.

**`transcribe_voice` returns `(no speech detected)`**

* Confirm your microphone is selected in **Settings → Audio**.
* Raise `timeout_seconds` — the configured default (15 s out of the box) may be too short if recording takes time to initialize.
* Check the VAD threshold in **Settings → Audio** — a higher sensitivity value (lower raw threshold) may be needed for quiet speech.

**Claude Desktop does not see the tools**

* Restart Claude Desktop after editing `claude_desktop_config.json`.
* Confirm `socat` is installed and `socat STDIO UNIX-CONNECT:/tmp/voxctrl-mcp.sock` connects successfully from a terminal.
* Check that `voxctrl-mcp.sock` exists (`ls -la /tmp/*.sock`).
* Ensure VoxCtrl is running and the MCP server is enabled in **Settings → General**.
