# Output Routing

**Crate:** `crates/voxctrl-routing/`

## Overview

VoxCtrl's routing system decouples *what you say* from *where it goes*. You define:

- **Output Targets** (`targets.toml`) — named delivery destinations
- **Hotkey Bindings** (`bindings.toml`) — which keys trigger which targets

Both files are hot-reloaded when changed on disk.

---

## Output Targets

Defined in `~/.config/voxctrl/targets.toml`. Each `[[target]]` block describes one destination.

### Common Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | string | required | Unique identifier, referenced by bindings |
| `label` | string | required | Display name in the UI |
| `delivery` | string | required | Delivery type (see below) |
| `append_newline` | bool | `true` | Append `\n` after injected text |
| `strip_newlines` | bool | `false` | Replace newlines (`\n`) with spaces and strip carriage returns (`\r`) (Inject only) |
| `send_on_release` | bool | `true` | Wait for hotkey release before delivering |
| `initial_prompt` | string | null | Whisper context prompt override for this target |
| `processing` | object | (inherit) | Per-target post-processing overrides |
| `response_pipe` | string | null | FIFO path for TTS response output |

### Delivery Types

#### `inject` — Keystroke Injection
Simulates typing into the currently focused window.

```toml
[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
```

Linux injection priority:
1. `wtype` (Wayland)
2. `xdotool type --clearmodifiers` (X11)
3. Clipboard + Ctrl+V fallback

Windows: clipboard paste via PowerShell.

---

#### `clipboard` — System Clipboard
Copies text to the system clipboard. Does not paste.

```toml
[[target]]
id = "clipboard"
label = "Copy to Clipboard"
delivery = "clipboard"
```

---

#### `file` — File Append/Write
Writes text to a file on disk.

```toml
[[target]]
id = "notes"
label = "Meeting Notes"
delivery = "file"
file_path = "~/Documents/notes.md"
file_prefix = "- "        # Prepend to each entry
file_timestamp = true     # Prepend ISO timestamp (default: true)
file_mode = "append"      # "append" or "write" (default: "append")
```

---

#### `exec` — Shell Command
Runs a shell command. The transcribed text is passed as an argument.

```toml
[[target]]
id = "cmd"
label = "Custom Script"
delivery = "exec"
command = "/home/user/scripts/handle-voice.sh"
```

---

#### `http` — HTTP POST
POSTs the text as a JSON body to an HTTP endpoint.

```toml
[[target]]
id = "api"
label = "My API"
delivery = "http"
http_url = "http://localhost:8080/voice"
http_method = "POST"      # Default: "POST"
```

Request body:
```json
{"text": "transcribed text here"}
```

Optional: `http_headers` (table) and `http_json_template` (JSON value).

---

#### `webhook` — Signed HTTP POST
Like `http` but uses `webhook_url` and adds an HMAC-SHA256 signature header.

```toml
[[target]]
id = "secure_hook"
label = "Signed Webhook"
delivery = "webhook"
webhook_url = "https://example.com/hook"
webhook_secret = "your-shared-secret"
```

Adds header: `X-VoxCtrl-Signature: sha256=<hex>`

Optional: `webhook_json_template` (JSON value) to customize the payload shape.

Verify on your server:
```python
import hmac, hashlib

def verify(payload: bytes, secret: str, signature: str) -> bool:
    expected = 'sha256=' + hmac.new(
        secret.encode(), payload, hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, signature)
```

---

#### `socket` — Unix Domain Socket or TCP
Sends text (newline-terminated) to a socket.

```toml
[[target]]
id = "sock"
label = "Unix Socket"
delivery = "socket"
socket_unix = "/tmp/myapp.sock"   # Unix domain socket path

# OR for TCP:
socket_host = "127.0.0.1"
socket_port = 9000
```

---

#### `pipe` — Named FIFO
Writes to a named FIFO pipe.

```toml
[[target]]
id = "fifo"
label = "FIFO Output"
delivery = "pipe"
pipe_path = "/tmp/voice.fifo"
```

---

#### `dbus` — DBus Signal
Emits the text as a `text_injected` signal on the `ai.voxctrl.Dictation` interface.

```toml
[[target]]
id = "dbus"
label = "DBus Output"
delivery = "dbus"
dbus_signal = "text_injected"   # Optional: override signal name
```

---

#### `mcp` — MCP Response Queue
Enqueues the text as a response to a pending `transcribe_voice` tool call from an MCP client.

```toml
[[target]]
id = "mcp_out"
delivery = "mcp"
mcp_path = "/tmp/voxctrl-mcp.sock"   # Optional socket path override
mcp_tool = "transcribe_voice"        # Optional tool name hint
```

---

#### `speak` — Speak Text Aloud (TTS)
Plays the transcribed text aloud via the globally configured Text-to-Speech (TTS) engine. This works offline and does not require an active MCP client or server to be enabled.

```toml
[[target]]
id = "tts_out"
label = "Read Transcription Aloud"
delivery = "speak"
```

---

#### `openai_api` — OpenAI API Call
Sends the transcribed text to an OpenAI-API-compatible chat completions endpoint and delivers
the model's response (not the original transcription). Uses the server URL, API key, and
default model configured in the OpenAI API settings tab (`config.ollama`) unless overridden.

```toml
[[target]]
id = "summarize"
label = "Summarize via LLM"
delivery = "openai_api"
openai_prompt = "Summarize the following transcript in 3 bullet points."
openai_model = "gpt-4o-mini"      # Optional: overrides the global model for this target
openai_max_tokens = 500           # Optional: caps response length
openai_timeout_secs = 30          # Optional: overrides the global request timeout
```

| Field | Type | Default | Description |
|---|---|---|---|
| `openai_prompt` | string | null | System message instructing the model; transcribed text is sent as the user message |
| `openai_model` | string | null (inherit) | Overrides `ollama.model` for this target |
| `openai_max_tokens` | integer | null (server default) | Caps the response length |
| `openai_timeout_secs` | integer | null (inherit) | Overrides `ollama.timeout_secs` for this target |

Each `openai_api` target keeps its own running conversation (up to the last 20 messages, i.e. 10 user/assistant exchanges) in memory and replays it with every request, so follow-up dictations like "what did I just ask you?" resolve correctly. This history lives only in the running app process — it is not persisted to disk, and resets whenever targets are hot-reloaded (e.g. after editing targets in Settings) or the app restarts.

---

### Per-Target Processing

Each target can override global post-processing settings. All fields are optional (`null` = inherit global config):

```toml
[[target]]
id = "code_editor"
label = "Code Editor"
delivery = "inject"

[code_editor.processing]
code_mode = true
remove_fillers = false
spoken_punctuation = true
auto_format_lists = false
apply_snippets = true
atspi_context = true
noise_suppression = false
quiet_mode = false
```

> [!NOTE]
> Ollama post-processing parameters (`ollama_enabled`, `ollama_model`, `ollama_mode`, `ollama_prompt`) are defined per-hotkey binding in `bindings.toml` (or via the Hotkeys UI tab) instead of per-target. This allows you to apply different LLM rewriting styles (e.g. formal, bullet-points) using different hotkeys targeting the same destination.
>
> When `ollama_enabled` is used with a Q&A-style custom prompt (e.g. "answer questions concisely"), each binding keeps its own running conversation (up to the last 10 exchanges) so follow-ups like "what did I just ask you" resolve correctly. This history lives only in the running inference worker — it resets on app restart and is keyed by binding id, so different bindings never share context.

---

## Hotkey Bindings

Defined in `~/.config/voxctrl/bindings.toml`. Each `[[binding]]` block maps a key combo + gesture to one or more targets.

### Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | string | required | Unique identifier |
| `label` | string | `""` | Display name in UI |
| `keys` | string[] | required | Key names (evdev format on Linux). For chord gestures, this defines the base combo keys. |
| `subkey` | string | | Single trigger key for chord gestures (e.g. `"KEY_Z"`), pressed after base keys are held |
| `gesture` | string | required | `"hold"`, `"toggle"`, `"double_tap"`, `"double_tap_hold"`, or `"chord"` |
| `target_ids` | string[] | required | Ordered list of targets to route to |
| `target_id` | string | | Single target (legacy; resolved if `target_ids` is empty) |
| `hold_threshold_ms` | integer | `200` | Minimum hold / double-tap-hold duration in ms |
| `tap_ms` | integer | `250` | Double-tap inter-press window in ms |
| `disabled` | bool | `false` | Disable without removing |

### Gesture Types

| Gesture | Behavior |
|---|---|
| `hold` | Recording starts on press, stops on release |
| `toggle` | First press starts, second press stops |
| `double_tap` | Two rapid presses within `tap_ms` trigger a toggle session |
| `double_tap_hold` | Double-tap and keep held on the second press (enforces `hold_threshold_ms` and has a 2-minute safety timeout), release to stop |
| `chord` | Hold the base `keys` and press the `subkey` trigger to start recording; release any of the base keys to stop |

### Key Names (evdev format)

Common keys:
- `KEY_LEFTMETA` — Left Super/Windows key
- `KEY_LEFTCTRL` — Left Ctrl
- `KEY_LEFTSHIFT` — Left Shift
- `KEY_LEFTALT` — Left Alt
- `KEY_SPACE` — Space bar
- `KEY_F1`–`KEY_F12` — Function keys
- `KEY_A`–`KEY_Z` — Letter keys
- `KEY_ESCAPE` — Escape key

### Example bindings.toml

```toml
[[binding]]
id = "dictate_hold"
label = "Dictate to Cursor (Hold)"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_ids = ["default"]

[[binding]]
id = "dictate_notes"
label = "Dictate to Notes File"
keys = ["KEY_LEFTCTRL", "KEY_LEFTSHIFT", "KEY_N"]
gesture = "toggle"
target_ids = ["notes", "clipboard"]   # Routes to both sequentially

[[binding]]
id = "quick_copy"
label = "Copy to Clipboard"
keys = ["KEY_LEFTMETA", "KEY_V"]
gesture = "double_tap"
target_ids = ["clipboard"]
tap_ms = 300

[[binding]]
id = "double_tap_hold_dictate"
label = "Double-Tap & Hold to Dictate"
keys = ["KEY_LEFTMETA"]
gesture = "double_tap_hold"
tap_ms = 250
hold_threshold_ms = 200
target_ids = ["default"]

[[binding]]
id = "chord_dictate"
label = "Chord Dictation"
keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA"]
subkey = "KEY_Z"
gesture = "chord"
target_ids = ["default"]
```

### Multi-Target Routing

When `target_ids` contains multiple entries, VoxCtrl delivers to each target **sequentially** in the listed order after a single recording session. This lets you, for example, inject text into a window AND log it to a file simultaneously.

### Superset Shadowing

If binding A's keys are a proper subset of binding B's keys (e.g. `META+SPACE` vs `CTRL+META+SPACE`), and both are pressed, only binding B fires. This prevents shorter combos from accidentally triggering when a longer combo is intended.

---

## Router Logic

`OutputTargetRouter::route(text, target_id, targets)`:

1. Look up target by `target_id` from the in-memory cache
2. Apply per-target `processing` overrides (inheriting globals for null fields)
3. Build delivery payload (append newline, prefix, timestamp)
4. Dispatch to the appropriate delivery handler
5. On error (socket unavailable, file unwritable, etc.), log the failure and continue — never crashes or drops the UI

The router is hot-reloadable: `save_targets()` via IPC updates both the TOML file and the in-memory cache, and spawns any new FIFO response pipe listeners.
