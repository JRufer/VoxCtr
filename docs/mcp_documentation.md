# Whisper-Wayland MCP Server

Whisper-Wayland ships a built-in **Model Context Protocol (MCP)** server that exposes the app's voice I/O pipeline as tools any MCP-capable AI can call. An agent can trigger the microphone, receive the transcript, and queue a spoken response — all through a standardised JSON-RPC 2.0 interface.

---

## Overview

When the MCP server is enabled, Whisper-Wayland listens on a Unix domain socket and presents three tools:

| Tool | What it does |
|---|---|
| `transcribe_voice` | Opens the mic, records the user, returns the transcript |
| `speak_text` | Queues text for TTS playback through the current voice |
| `get_status` | Returns whether the mic is open and TTS is playing |

This lets an AI agent have a full voice conversation:

```
Agent → transcribe_voice()  → user speaks → transcript returned
Agent → speak_text("…")     → Whisper-Wayland speaks the response aloud
Agent → transcribe_voice()  → next turn …
```

---

## Prerequisites

### Required system packages

| Package | Purpose | Install |
|---|---|---|
| `piper` | Neural TTS engine | `yay -S piper-tts` or [piper releases](https://github.com/rhasspy/piper/releases) |
| `aplay` | PCM audio playback | `sudo pacman -S alsa-utils` |
| `socat` | Stdio ↔ Unix socket bridge (Claude Desktop) | `sudo pacman -S socat` |

`espeak-ng` can be used as a fallback if Piper is not installed:

```bash
sudo pacman -S espeak-ng
```

### Voice models

Voice models are downloaded from inside the app. Go to **Settings → Voice Output**, select a voice from the picker, and click **⬇ Download**. Models are stored in:

```
~/.local/share/whisper-wayland/voices/
```

---

## Enabling the MCP Server

### Via Settings UI

1. Open **Settings → Voice Output**
2. Scroll to the **MCP Server** section
3. Toggle **"Enable MCP Server"**
4. Note the socket path shown (default: `/tmp/whisper-wayland-mcp.sock`)
5. Optionally click **"Register in Claude Desktop"** to auto-configure Claude Desktop

### Via config.json

```json
{
  "mcp_server_enabled": true,
  "mcp_record_timeout": 15.0
}
```

The server starts automatically when the app launches if `mcp_server_enabled` is `true`.

---

## Transport

The server listens on a **Unix domain socket**:

```
/tmp/whisper-wayland-mcp.sock
```

Each connection gets its own daemon thread. The protocol is newline-delimited JSON-RPC 2.0: one JSON object per line, terminated with `\n`.

---

## Protocol

Standard MCP / JSON-RPC 2.0. Every request must include `"jsonrpc": "2.0"`.

### Handshake

```json
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
← {"jsonrpc":"2.0","id":1,"result":{
     "protocolVersion":"2024-11-05",
     "capabilities":{"tools":{}},
     "serverInfo":{"name":"whisper-wayland","version":"1.0.0"}
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
| `timeout_seconds` | `number` | `15.0` | Maximum seconds to wait for speech before returning |

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

- While recording, the recording overlay is always shown (regardless of `show_overlay` config) — the user always has a visual indicator that the mic is live.
- The microphone is released automatically once VAD detects silence or `timeout_seconds` elapses.
- Whisper-Wayland's full post-processing pipeline is applied before the transcript is returned.

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

- Returns as soon as the text is queued — does not block until playback finishes.
- If `tts_response_overlay` is enabled, a teal overlay is displayed while TTS plays.
- The user can interrupt playback at any time with the configured TTS stop key (default: `Escape`).
- If Piper is not installed or the voice model is missing, `espeak-ng` is used as fallback.

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
| `-32601` | Method not found |
| `-32602` | Unknown tool name |
| `-32603` | Internal error (missing required argument, callback exception) |

---

## Connecting: Claude Desktop

### One-click registration

In **Settings → Voice Output → MCP Server**, click **"Register in Claude Desktop"**. The app writes the `socat` bridge entry to:

```
~/.config/claude/claude_desktop_config.json
```

### Manual configuration

Add the following to `~/.config/claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "whisper-wayland": {
      "command": "socat",
      "args": ["STDIO", "UNIX-CONNECT:/tmp/whisper-wayland-mcp.sock"]
    }
  }
}
```

Restart Claude Desktop. The tools `transcribe_voice`, `speak_text`, and `get_status` appear automatically in the tool picker.

> **Note:** Whisper-Wayland must already be running before Claude Desktop connects.

---

## Connecting: Raw Socket (Python)

```python
import socket
import json

SOCK = "/tmp/whisper-wayland-mcp.sock"

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

## Connecting: Shell / socat

```bash
# One-shot: list tools
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
  | socat - UNIX-CONNECT:/tmp/whisper-wayland-mcp.sock \
  | head -1 | python3 -m json.tool
```

---

## Response Loopback (FIFO pipe)

For agents that generate responses to a named FIFO, Whisper-Wayland can read those responses and speak them automatically — without needing the agent to call `speak_text` directly.

### How it works

1. The agent writes its response text to a named FIFO.
2. `ResponseListener` (a daemon thread) reads each line and passes it to `TTSEngine.speak()`.
3. The TTS overlay appears while playback continues.

### Configuration in `targets.toml`

```toml
[[target]]
id = "my-agent"
label = "My Agent"
delivery = "pipe"
pipe_path = "/tmp/my-agent.in"
response_pipe = "/tmp/my-agent.out"   # ← agent writes responses here
post_processing = "none"
append_newline = true
```

### Agent-side setup

```bash
# Create both FIFOs
mkfifo /tmp/my-agent.in /tmp/my-agent.out

# Agent reads commands from .in, writes responses to .out
while true; do
  read -r cmd < /tmp/my-agent.in
  response=$(my_llm_cli "$cmd")
  echo "$response" > /tmp/my-agent.out
done
```

Each line written to `response_pipe` is treated as a separate TTS utterance. Empty lines are ignored. The listener re-opens the FIFO on EOF so the agent can restart without restarting Whisper-Wayland.

---

## TTS Configuration

All TTS settings live in `~/.config/whisper-wayland/config.json` and in **Settings → Voice Output**.

| Key | Type | Default | Description |
|---|---|---|---|
| `tts_enabled` | `bool` | `false` | Master TTS on/off switch |
| `tts_engine` | `string` | `"piper"` | `"piper"` or `"espeak"` |
| `tts_voice` | `string` | `"en-us-lessac-medium"` | Voice ID from the catalog |
| `tts_stop_key` | `string[]` | `["KEY_ESCAPE"]` | evdev key(s) to stop TTS playback |
| `tts_response_overlay` | `bool` | `true` | Show teal overlay while TTS plays |
| `mcp_server_enabled` | `bool` | `false` | Start the MCP server on launch |
| `mcp_record_timeout` | `number` | `15.0` | Default recording timeout in seconds |

### Available voices

| Voice ID | Language | Quality | Size |
|---|---|---|---|
| `en-us-lessac-medium` | US English | Medium | ~55 MB |
| `en-us-ryan-medium` | US English | Medium | ~55 MB |
| `en-us-ryan-high` | US English | High | ~130 MB |
| `en-us-amy-low` | US English | Low | ~5 MB |
| `en-us-joe-medium` | US English | Medium | ~55 MB |
| `en-us-kusal-medium` | US English | Medium | ~55 MB |
| `en-us-danny-low` | US English | Low | ~5 MB |
| `en-gb-alan-low` | GB English | Low | ~5 MB |

Download voices in **Settings → Voice Output → Voice Picker → ⬇ Download**.

---

## MCP Server Internals

### Socket path

```
/tmp/whisper-wayland-mcp.sock
```

### Threading model

- One `threading.Thread` accepts incoming connections in a loop.
- Each accepted connection spawns a short-lived daemon thread that reads newline-delimited JSON, dispatches, and writes the response.
- The `transcribe_voice` handler blocks its connection thread while waiting for the transcript; other connections continue to be served concurrently.

### Recording synchronisation

`transcribe_voice` works by:

1. Scheduling `on_press('mcp')` on the Qt main thread via `QTimer.singleShot`.
2. Starting an auto-stop thread that calls `on_release('mcp')` after `timeout_seconds`.
3. Blocking on a `queue.Queue.get(timeout=timeout+5)`.
4. The main app's `_on_text_injected` callback puts every transcription result into this queue.

This means only one `transcribe_voice` call can be in flight at a time per Whisper-Wayland instance. Concurrent calls from multiple clients will queue at the recording controller.

---

## Troubleshooting

**Socket does not exist**

Whisper-Wayland is not running, or the MCP server is disabled. Enable it in **Settings → Voice Output** or set `"mcp_server_enabled": true` in `config.json` and restart.

**`socat` connection refused**

The socket exists but the server is not listening yet. Wait a moment after Whisper-Wayland starts, or check the app's console output for errors.

**TTS plays but no audio**

- Check that `aplay` is installed (`which aplay`).
- Verify the voice model is downloaded: models live in `~/.local/share/whisper-wayland/voices/`.
- Try `tts_engine = "espeak"` as a fallback.

**`transcribe_voice` returns `(no speech detected)`**

- Confirm your microphone is selected in **Settings → Audio**.
- Raise `timeout_seconds` — the default 15 s may be too short if recording takes time to initialise.
- Check VAD threshold in **Settings → Audio** — a high threshold may cut off quiet speech.

**Claude Desktop does not see the tools**

- Restart Claude Desktop after editing `claude_desktop_config.json`.
- Confirm `socat` is installed and `socat STDIO UNIX-CONNECT:/tmp/whisper-wayland-mcp.sock` connects successfully from a terminal.
- Check that `whisper-wayland-mcp.sock` exists (`ls -la /tmp/*.sock`).
