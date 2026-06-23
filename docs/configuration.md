# Configuration Reference

VoxCtrl uses three configuration files, all stored under `~/.config/voxctrl/`.

| File | Format | Purpose |
|---|---|---|
| `config.json` | JSON | Application settings |
| `targets.toml` | TOML | Output target definitions |
| `bindings.toml` | TOML | Hotkey binding definitions |

All files are **hot-reloaded** — the app watches for external changes and applies them without restart.

---

## config.json

Full schema with defaults:

```json
{
  "engine": {
    "backend": "auto",
    "inference_mode": "Balanced",
    "whisper_cpp": {
      "model_dir": "",
      "model_size": "large-v3",
      "device": "auto",
      "threads": 0
    },
    "moonshine": {
      "model_size": "base",
      "language": "en"
    }
  },
  "audio": {
    "vad_threshold": 0.5,
    "min_silence_duration_ms": 500,
    "input_device_index": null,
    "evdev_device": null,
    "noise_suppression": false,
    "gain": 1.0,
    "dynamic_stream": true
  },
  "ui": {
    "show_overlay": true,
    "overlay_style": "blue_wave",
    "overlay_position": "center",
    "overlay_monitor": "primary",
    "auto_show_settings": true,
    "show_notification": false,
    "history_enabled": false
  },
  "features": {
    "remove_fillers": true,
    "custom_vocabulary": [],
    "spoken_punctuation": true,
    "auto_format_lists": true,
    "quiet_mode": false,
    "snippets": {}
  },
  "ollama": {
    "enabled": false,
    "endpoint": "http://localhost:11434",
    "api_key": null,
    "model": "llama3.2:1b",
    "mode": "clean",
    "custom_prompt": null,
    "timeout_secs": 8
  },
  "tts": {
    "enabled": false,
    "engine": "piper",
    "voice": "en-us-lessac-medium",
    "voice_dir": "",
    "stop_key": ["KEY_ESCAPE"],
    "response_overlay": true,
    "speed": 1.0,
    "gpu": false,
    "kokoro": {
      "voice": "af_heart",
      "quality": "fp16",
      "speed": 1.0,
      "prewarm": false,
      "data_dir": ""
    }
  },
  "mcp": {
    "server_enabled": false,
    "record_timeout": 15.0,
    "visual_feedback": true
  },
  "atspi": {
    "injection": true,
    "context_prompt": true,
    "auto_code_mode": true
  }
}
```

### `engine` section


The engine config is nested into two backend sub-objects.

**Top-level fields:**

| Key | Type | Values | Description |
|---|---|---|---|
| `backend` | string | `"auto"`, `"whisper-cpp"`, `"moonshine"` | Which backend to use; `auto` selects based on GPU availability |
| `inference_mode` | string | `"Balanced"`, `"Aggressive"` | Inference aggressiveness; `Balanced` is recommended |

**`whisper_cpp` sub-object:**

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"large-v3"` | Whisper model to load (see valid values below) |
| `device` | string | `"auto"` | Compute device: `auto`/`cpu`/`cuda`/`vulkan` |
| `threads` | integer | `0` | CPU thread count; 0 = half of logical cores |
| `model_dir` | string | `""` | Custom model directory; empty = `~/.local/share/voxctrl/models/`. Supports `~` expansion. The directory must already exist. |

Valid `model_size` values: `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large-v2`, `large-v3`, `large-v3-turbo`

The `.en` variants are English-only but slightly faster. `large-v3-turbo` is a distilled model balancing quality and speed.

**`moonshine` sub-object** (only used when `backend = "moonshine"`):

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"base"` | `"base"` or `"tiny"` |
| `language` | string | `"en"` | BCP-47 language code |

### `audio` section

| Key | Type | Default | Description |
|---|---|---|---|
| `vad_threshold` | float | `0.5` | Voice Activity Detection sensitivity (0.0–1.0); **higher = more sensitive** (lower RMS gate) |
| `min_silence_duration_ms` | integer | `500` | Milliseconds of silence before stopping a recording session |
| `input_device_index` | integer or null | `null` | CPAL device index; null = auto-detect |
| `evdev_device` | string or null | `null` | Linux evdev keyboard device path for hotkeys, e.g. `"/dev/input/event4"` |
| `noise_suppression` | bool | `false` | Enable basic noise suppression pre-processing |
| `gain` | float | `1.0` | Microphone amplification multiplier |
| `dynamic_stream` | bool | `true` | Open mic on-demand (true) vs. always-on (false) |

**VAD threshold note:** The threshold maps as `rms_gate = (1.0 - vad_threshold) * 0.006`. At 0.5 (default), the gate threshold is 0.003 RMS. At 1.0 (maximum sensitivity), there is no gate (0.0 RMS). At 0.0 (minimum sensitivity), the gate is 0.006 RMS.

### `ui` section

| Key | Type | Values | Default | Description |
|---|---|---|---|---|
| `show_overlay` | bool | | `true` | Whether the overlay window is currently visible |
| `overlay_style` | string | `"voice_card"`, `"waveform"`, `"pulse"`, `"blue_wave"`, `"none"` | `"blue_wave"` | HUD visualization style |
| `overlay_position` | string | `"top"`, `"center"`, `"bottom"` | `"center"` | Screen positioning of the overlay window |
| `overlay_monitor` | string | `"primary"` or monitor name | `"primary"` | Specific display screen for visual overlay |
| `auto_show_settings` | bool | | `true` | Auto-show Settings window on startup |
| `show_notification` | bool | | `false` | Desktop toast notification after text delivery |
| `history_enabled` | bool | | `false` | Enable transcription history tracking |

### `features` section

| Key | Type | Default | Description |
|---|---|---|---|
| `remove_fillers` | bool | `true` | Strip filler words (`uh`, `um`, `hmm`, `er`, `ah`, etc.) |
| `spoken_punctuation` | bool | `true` | Convert spoken punctuation words to symbols (e.g. "period" → ".") |
| `auto_format_lists` | bool | `true` | Detect "first/second/third" patterns and reformat as a numbered list |
| `quiet_mode` | bool | `false` | Suppress overlay notifications during transcription |
| `custom_vocabulary` | string[] | `[]` | Custom words; VoxCtrl uses fuzzy Levenshtein matching to correct near-matches post-transcription |
| `snippets` | object | `{}` | Short code → expansion map |

Example with snippets:
```json
"features": {
  "remove_fillers": true,
  "spoken_punctuation": true,
  "snippets": {
    "addr": "742 Evergreen Terrace, Springfield",
    "sig": "Best regards,\nAlice"
  }
}
```

### `ollama` section

Despite the key name (kept for config-file backward compatibility), this section configures a connection to
**any OpenAI-API-compatible server** — Ollama, LM Studio, vLLM, OpenAI itself, etc. VoxCtrl talks to the
server's `/v1/models` and `/v1/chat/completions` endpoints.

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable LLM post-processing |
| `endpoint` | string | `"http://localhost:11434"` | Base URL of the OpenAI-API-compatible server |
| `api_key` | string or null | `null` | Optional bearer token, sent as `Authorization: Bearer <key>`. Required by most non-localhost servers (e.g. OpenAI). |
| `model` | string | `"llama3.2:1b"` | Model name |
| `mode` | string | `"clean"` | Rewrite style: `clean`/`formal`/`casual`/`bullet`/`concise`/`custom` |
| `custom_prompt` | string or null | `null` | Instruction when mode is `custom`; use `{text}` as placeholder, or text is appended after the prompt |
| `timeout_secs` | integer | `8` | HTTP request timeout in seconds |

### `tts` section

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable TTS subsystem |
| `engine` | string | `"piper"` | Synthesis engine: `"piper"`, `"kokoro"`, or `"espeak"` |
| `voice` | string | `"en-us-lessac-medium"` | Active Piper voice name (hyphen-delimited, e.g. `"en-us-ryan-high"`) |
| `voice_dir` | string | `""` | Directory for Piper voice files; empty = `~/.local/share/voxctrl/piper-voices/`. Supports `~` expansion. |
| `stop_key` | string[] | `["KEY_ESCAPE"]` | Keys that cancel current TTS playback |
| `response_overlay` | bool | `true` | Show overlay indicator while TTS is speaking |
| `speed` | float | `1.0` | Speech synthesis speed multiplier (0.5 – 2.0) |
| `gpu` | bool | `false` | Enable GPU acceleration (CUDA) for Kokoro and Piper |
| `kokoro` | object | | Kokoro engine sub-configuration (see below) |

**`kokoro` sub-object:**

| Key | Type | Default | Description |
|---|---|---|---|
| `voice` | string | `"af_heart"` | Voice ID, e.g. `"af_heart"`, `"am_adam"`, `"bf_emma"`, `"bm_george"` |
| `quality` | string | `"fp16"` | Model precision: `"f32"` (310 MB), `"fp16"` (169 MB), `"int8"` (88 MB) |
| `speed` | float | `1.0` | Speech speed multiplier (0.5 – 2.0) |
| `prewarm` | bool | `false` | Pre-warm model on startup so first speech is instantaneous |
| `data_dir` | string | `""` | Custom directory for model/voices; empty = `~/.local/share/voxctrl/kokoro/` |


### `mcp` section

| Key | Type | Default | Description |
|---|---|---|---|
| `server_enabled` | bool | `false` | Start the MCP socket server on launch |
| `record_timeout` | float | `15.0` | Max seconds for `transcribe_voice` to wait for speech |
| `visual_feedback` | bool | `true` | Show overlay indicator while MCP server is listening to microphone |

### `atspi` section

| Key | Type | Default | Description |
|---|---|---|---|
| `injection` | bool | `true` | Use AT-SPI2 for text insertion when available |
| `context_prompt` | bool | `true` | Read focused widget text to use as Whisper context prompt |
| `auto_code_mode` | bool | `true` | Detect code editors/terminals and enable code-mode processing automatically |

---

## targets.toml

Each output destination is a `[[target]]` block.

### Minimal example
```toml
[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
```

### All common fields
```toml
[[target]]
id = "my_target"
label = "My Target"
delivery = "inject"

# Text formatting
append_newline = true         # Default: true
strip_newlines = false        # Default: false. Replaces newlines with spaces and strips \r (Inject only)
send_on_release = true        # Default: true
initial_prompt = ""           # Whisper context prompt override for this target

# Per-target post-processing overrides (all optional; null = inherit global)
[my_target.processing]
remove_fillers = true
spoken_punctuation = true
auto_format_lists = true
apply_snippets = true
code_mode = false
quiet_mode = false
atspi_context = true
noise_suppression = false
```

### Delivery-specific fields

**`file`:**
```toml
delivery = "file"
file_path = "~/Documents/notes.md"
file_prefix = "- "
file_timestamp = true          # Default: true
file_mode = "append"           # "append" or "write"
```

**`http`:**
```toml
delivery = "http"
http_url = "http://localhost:8080/voice"
http_method = "POST"           # Default: "POST"
# Optional: custom headers and JSON template
```

**`webhook`:**
```toml
delivery = "webhook"
webhook_url = "https://example.com/hook"
webhook_secret = "my-hmac-secret"
```

Note: `webhook` uses `webhook_url`, while `http` uses `http_url`.

**`exec`:**
```toml
delivery = "exec"
command = "/usr/local/bin/handle-voice.sh"
```

**`socket`** (supports Unix and TCP):
```toml
delivery = "socket"
socket_unix = "/tmp/myapp.sock"    # Unix domain socket
# OR:
socket_host = "127.0.0.1"
socket_port = 9000
```

**`pipe`:**
```toml
delivery = "pipe"
pipe_path = "/tmp/voice.fifo"
```

**`speak`:**
```toml
delivery = "speak"
```

**TTS response pipe:**
```toml
response_pipe = "/tmp/tts-response.fifo"  # Optional FIFO for TTS output
```

---

## bindings.toml

Each hotkey is a `[[binding]]` block.

### Full example
```toml
[[binding]]
id = "dictate_hold"
label = "Dictate (Hold)"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_ids = ["default"]
hold_threshold_ms = 200        # Default: 200ms min hold to register
disabled = false
ollama_enabled = true          # Enable Ollama rewrite specifically for this hotkey
ollama_mode = "formal"         # Rewrite output in formal style for this hotkey

[[binding]]
id = "dictate_notes"
label = "Dictate to Notes + Clipboard"
keys = ["KEY_LEFTCTRL", "KEY_LEFTSHIFT", "KEY_N"]
gesture = "toggle"
target_ids = ["notes", "clipboard"]

[[binding]]
id = "quick_code"
label = "Code Dictation (Double-tap)"
keys = ["KEY_F12"]
gesture = "double_tap"
tap_ms = 250                   # Default: 250ms inter-tap window
target_ids = ["code_editor"]

[[binding]]
id = "double_tap_hold_dictate"
label = "Double-Tap & Hold to Dictate"
keys = ["KEY_LEFTMETA"]
gesture = "double_tap_hold"
tap_ms = 250
hold_threshold_ms = 200        # Hold threshold on the second tap
target_ids = ["default"]

[[binding]]
id = "chord_dictation"
label = "Chord Dictation"
keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA"]
subkey = "KEY_Z"
gesture = "chord"
target_ids = ["default"]
```

### Field reference

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `id` | string | Yes | | Unique identifier |
| `label` | string | Yes | `""` | Display name |
| `keys` | string[] | Yes | | Key names (evdev format). For chord gestures, this defines the base combo keys. |
| `subkey` | string | No | | Trigger key for chord gestures (e.g. `"KEY_Z"`). Pressed after base keys are held. |
| `gesture` | string | Yes | | `"hold"`, `"toggle"`, `"double_tap"`, `"double_tap_hold"`, or `"chord"` |
| `target_ids` | string[] | Yes | | Ordered list of target IDs to route to |
| `target_id` | string | No | | Single target (legacy; use `target_ids`) |
| `hold_threshold_ms` | integer | No | `200` | Min hold duration in ms for hold / double-tap-hold gesture |
| `tap_ms` | integer | No | `250` | Double-tap inter-press window in ms |
| `disabled` | bool | No | `false` | Disable without deleting |
| `ollama_enabled` | bool | No | `null` | Enable/disable Ollama post-processing specifically for this hotkey (null = inherit global config) |
| `ollama_model` | string | No | `null` | Ollama model override specifically for this hotkey |
| `ollama_mode` | string | No | `null` | Ollama mode override specifically for this hotkey (`clean`/`formal`/`casual`/`bullet`/`concise`/`custom`) |
| `ollama_prompt` | string | No | `null` | Custom prompt override specifically for this hotkey |

---

## Config Migration

VoxCtrl auto-migrates older config formats on load. Known migrations:

- `features.show_notification` → `ui.show_notification` (moved in an early release; the migrated config is immediately re-saved to disk to clean up the old key)

Unrecognized fields are silently ignored. Missing fields use their defaults. This ensures compatibility when upgrading or downgrading.
