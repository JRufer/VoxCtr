# Output Routing

**Crate:** `crates/voxctrl-routing/`

## Overview

VoxCtrl's routing system decouples *what you say* from *where it goes*. You define:

- **Output Commands** (`targets.toml`) — named delivery destinations
- **Hotkey Bindings** (`bindings.toml`) — which keys trigger which commands

> [!NOTE]
> Output Commands are called *targets* everywhere below the UI: the file is
> `targets.toml`, each block is `[[target]]`, bindings reference `target_id`,
> and the Tauri commands are `get_targets` / `save_targets`. Only the name the
> app shows you changed; nothing on disk did.

Both files are hot-reloaded when changed on disk.

---

## Output Commands

Defined in `~/.config/voxctrl/targets.toml`. Each `[[target]]` block describes one destination.

**Saying one by name.** Start dictation and say *"VoxCtrl"*, then the command's
name, then the text: *"VoxCtrl notes, remember to call the plumber"* delivers
*remember to call the plumber* to the command named **notes**, whatever the
active hotkey was pointed at. Everything after the name is the payload. The full
matching rules — conversational lead-ins, fuzzy name matching, the overlay —
are under [`command` — Voice Command Router](#command--voice-command-router).

### Common Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | string | required | Unique identifier, referenced by bindings |
| `label` | string | required | The target's name — shown in the UI, and spoken to route dictation here through a `command` target (see [Voice Command Router](#command--voice-command-router)) |
| `delivery` | string | required | Delivery type (see below) |
| `strip_newlines` | bool | `false` | Replace newlines (`\n`) with spaces and strip carriage returns (`\r`). Honored by the `inject` and `command` targets |
| `processing` | object | (inherit) | Per-target post-processing overrides |
| `response_pipe` | string | null | FIFO path for TTS response output |

> [!NOTE]
> Two targets carry a conversation with an LLM. Use `chat` when the model is reachable
> over an OpenAI-compatible HTTP API; use `pipe` + `response_pipe` when you are driving a
> local agent process over FIFOs.

### Delivery Types

#### `inject` — Keystroke Injection
Simulates typing into the currently focused window.

```toml
[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
```

Delivered text always ends with a single space, so consecutive dictations do
not run their last and first words together; text that already ends in
whitespace is left alone. The clipboard target does the same. Nothing appends a
newline.

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
file_timestamp = true     # Prepend a timestamp (default: true)
file_timestamp_format = "%Y-%m-%dT%H:%M:%SZ"   # strftime pattern, UTC
file_mode = "append"      # "append" or "write" (default: "append")
```

With `file_timestamp` on, each line is prefixed with `[<timestamp>] `.
`file_timestamp_format` is a chrono
[strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html)
pattern rendered in UTC — `%Y` year, `%m` month, `%d` day, `%H` hour, `%M`
minute, `%S` second, `%b` month name, `%a` weekday, `%p` AM/PM, `%Z` zone,
`%%` a literal percent; anything else is written as typed. The target editor
previews the pattern as you type and flags one it cannot render. A pattern that
is unusable at delivery time falls back to the default,
`%Y-%m-%dT%H:%M:%SZ`, with a warning — a bad format never costs you the
dictation.

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

#### `command` — Voice Command Router
Dynamically routes dictated text to other targets based on spoken trigger phrases.

```toml
[[target]]
id = "cmd_router"
label = "Voice Command Router"
delivery = "command"
```

On a new install the first-run wizard creates this target, named "Command", and
binds the first hotkey to it. Until a second target exists it behaves exactly
like `inject` — a transcription with no trigger keyword in it falls through to
typing into the focused window — so voice command routing works the day another
target is added, with no re-binding.

**How Voice Command Routing Works:**
- **Trigger Keyword**: Listens for the `"VoxCtrl"` keyword (case-insensitive, supporting `VoxCtrl`, `voxctrl`, `vox ctrl`, `vox-ctrl`, and optional punctuation like `VoxCtrl:`).
- **Target Resolution**: Matches spoken target names against all configured target IDs and Labels (case-insensitively). Longest candidate target names take precedence (e.g. `"Personal Notes"` is matched before `"Notes"`).
- **Conversational Lead-in Support**: Supports natural lead-in command phrases between the trigger keyword and the target name, such as *"VoxCtrl send this to my notes. I love you."*, *"VoxCtrl add this to my personal notes, help"*, *"VoxCtrl put this into my Notes: hello"*, or *"VoxCtrl send us to my notes. I love you."*.
- **Payload Extraction**: Strips transition punctuation (`.`, `:`, `,`, `;`) and connector words (`saying`, `that`, `with text`) and routes the remaining text payload to the matched target.
- **Command UI Overlay**: When a voice command trigger is matched and executed, VoxCtrl automatically displays a temporary purple/indigo HUD overlay pill displaying the command name and text payload summary for a configurable duration (default: 3 seconds).
- **Auto-Trigger Detection**: Dictating with the `"VoxCtrl"` keyword (e.g. *"VoxCtrl notes Help me!"*) automatically activates voice command routing and surfaces the overlay pill regardless of whether the active hotkey target is set to `command` or `inject`.
- **Fallback**: If no `"VoxCtrl"` keyword is spoken or if no target matches, it falls back to direct text injection into the active application (identical to the `inject` target).

---

#### `chat` — Conversational LLM (OpenAI-compatible)

Sends each dictation as a turn in an ongoing conversation to an OpenAI-compatible
`/v1/chat/completions` endpoint — Hermes, Ollama, llama.cpp, LM Studio, vLLM, or a
remote provider — and surfaces the model's reply.

Unlike `http`, this target **reads the response** and **remembers the exchange**, so the
model keeps its context across turns. This makes VoxCtrl a voice front end for the same
API that Open WebUI talks to.

```toml
[[target]]
id = "hermes"
label = "Hermes"
delivery = "chat"
chat_url = "http://localhost:8080"          # /v1 suffix optional
chat_model = "hermes-4-14b"
chat_system_prompt = "You are a concise voice assistant. Answer in one or two spoken sentences."
chat_reply_mode = "speak"                    # speak | inject | clipboard | none
chat_max_history = 20                        # messages sent per turn; 0 = whole conversation
chat_timeout_secs = 120
chat_reset_phrase = "new conversation"       # optional; clears history instead of asking
# chat_api_key = "sk-..."                    # optional; usually unnecessary locally
```

| Field | Type | Default | Description |
|---|---|---|---|
| `chat_url` | string | required | Server base URL. A missing `/v1` suffix is added automatically |
| `chat_model` | string | required | Model id as reported by `GET /v1/models` |
| `chat_api_key` | string | null | Sent as `Authorization: Bearer …` when set |
| `chat_system_prompt` | string | null | Prepended to every request; never trimmed from the window |
| `chat_max_history` | int | `20` | Most recent messages sent per turn. `0` sends everything |
| `chat_timeout_secs` | int | `120` | Per-request timeout — local models can be slow to first token |
| `chat_reply_mode` | string | `"speak"` | `speak`, `inject`, `clipboard`, or `none` |
| `chat_reset_phrase` | string | null | Saying this clears the conversation instead of sending it |

**Conversation lifetime.** History is held in memory, keyed by target id, and survives
saving settings (which rebuilds every target). It is cleared by the spoken reset phrase,
by the *Reset conversation* button in the target editor, or by restarting VoxCtrl. It is
never written to disk.

**Reply handling.** `speak` requires TTS to be enabled under Settings → TTS. `inject`
types the reply into the focused window and honours the target's `strip_newlines`
setting. `none` runs the conversation without surfacing replies.

**Failure behaviour.** If the request fails, times out, or returns an empty completion,
the unanswered turn is rolled back so it is not resent on the next dictation.

> [!TIP]
> Point `chat_url` at whatever base URL you gave Open WebUI. If Open WebUI can reach the
> server, so can this target — use its model list to find the exact `chat_model` id, or
> press **Fetch models** in the target editor.

---

### Per-Target Processing

Each target can override global post-processing settings. All fields are optional (`null` = inherit global config). Snippet expansion is not among them — snippets always apply, and are switched off only by defining none.

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
```

> [!NOTE]
> LLM post-processing parameters (`openai_enabled`, `openai_model`, `openai_mode`, `openai_prompt`, `openai_system_prompt`) are defined per-hotkey binding in `bindings.toml` (or via the Hotkeys UI tab) instead of per-target. This allows you to apply different LLM rewriting styles (e.g. formal, bullet-points) using different hotkeys targeting the same destination. The model is served over the OpenAI API (configured under Settings → OpenAI API). The field names use the `openai_` prefix; the legacy `ollama_` names are still accepted for backwards compatibility.

---

## Hotkey Bindings

Defined in `~/.config/voxctrl/bindings.toml`. Each `[[binding]]` block maps a key combo + gesture to one or more targets.

### Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `id` | string | required | Unique identifier |
| `label` | string | `""` | Display name in UI |
| `keys` | string[] | required | Key names (evdev format, on every platform). Any number of modifiers plus exactly one regular key — see [Hotkeys](hotkeys.md#what-can-be-a-shortcut) |
| `gesture` | string | required | `"hold"`, `"toggle"`, `"double_tap"`, or `"double_tap_hold"` |
| `target_ids` | string[] | required | Ordered list of targets to route to |
| `target_id` | string | | Single target (legacy; resolved if `target_ids` is empty) |
| `hold_threshold_ms` | integer | `200` | Minimum hold / double-tap-hold duration in ms |
| `tap_ms` | integer | `300` | Longest gap between the first tap's release and the second tap's press, in ms |
| `disabled` | bool | `false` | Disable without removing |

### Gesture Types

| Gesture | Behavior |
|---|---|
| `hold` | Recording starts on press, stops on release |
| `toggle` | First press starts, second press stops |
| `double_tap` | Two taps within `tap_ms` toggle a session. Starts on the second press, unless a `double_tap_hold` shares the same keys — then it resolves on the release, to tell the two apart |
| `double_tap_hold` | Double-tap and keep held on the second press (`hold_threshold_ms` before recording starts). Releasing always stops it; a 2-minute timeout is a backstop for a release that never arrives |

`chord` was removed — see [Hotkeys → Migrating from `chord`](hotkeys.md#migrating-from-chord). Existing bindings load as `hold` automatically.

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
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "double_tap_hold"
tap_ms = 300
hold_threshold_ms = 200
target_ids = ["default"]

[[binding]]
id = "toggle_dictate"
label = "Toggle Dictation"
keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]
gesture = "toggle"
target_ids = ["default"]
```

### Multi-Target Routing

When `target_ids` contains multiple entries, VoxCtrl delivers to each target **sequentially** in the listed order after a single recording session. This lets you, for example, inject text into a window AND log it to a file simultaneously.

### Superset Shadowing

If binding A's keys are a proper subset of binding B's keys (e.g. `META+SPACE` vs `CTRL+META+SPACE`), and both are pressed, only binding B fires. This prevents shorter combos from accidentally triggering when a longer combo is intended.

Shadowing is resolved when a key goes *down*. Releasing `CTRL` part-way through a `CTRL+META+SPACE` gesture therefore does not start a `META+SPACE` recording.

---

## Router Logic

`OutputTargetRouter::route(text, target_id, targets)`:

1. Look up target by `target_id` from the in-memory cache
2. Apply per-target `processing` overrides (inheriting globals for null fields)
3. Build delivery payload (single-line flattening, trailing space, prefix, timestamp)
4. Dispatch to the appropriate delivery handler
5. On error (socket unavailable, file unwritable, etc.), log the failure and continue — never crashes or drops the UI

The router is hot-reloadable: `save_targets()` via IPC updates both the TOML file and the in-memory cache, and spawns any new FIFO response pipe listeners.
