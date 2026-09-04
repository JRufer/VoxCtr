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
    "whisper_cpp": {
      "model_dir": "",
      "model_size": "tiny",
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
    "input_device_index": null,
    "evdev_device": null,
    "noise_suppression": false,
    "gain": 1.0,
    "dynamic_stream": true
  },
  "ui": {
    "show_overlay": true,
    "show_command_overlay": true,
    "command_overlay_duration_secs": 3,
    "overlay_style": "blue_wave",
    "overlay_position": "center",
    "overlay_monitor": "primary",
    "auto_show_settings": true,
    "show_notification": false,
    "setup_completed": true
  },
  "features": {
    "remove_fillers": true,
    "custom_vocabulary": [],
    "spoken_punctuation": true,
    "auto_format_lists": true,
    "quiet_mode": false,
    "snippets": {}
  },
  "openai": {
    "enabled": false,
    "endpoint": "http://localhost:11434",
    "api_key": null,
    "model": "llama3.2:1b",
    "mode": "clean",
    "system_prompt": "Fix grammar and punctuation only. Return only the corrected text, no commentary.",
    "user_prompt": "{text}",
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
    "pocket_tts": {
      "voice": "alba",
      "prewarm": false,
      "hf_token": null,
      "voice_dir": ""
    },
    "inflect_micro": {
      "model_dir": "",
      "seed": 0,
      "noise_scale": 0.667,
      "prewarm": false
    }
  },
  "mcp": {
    "server_enabled": false,
    "record_timeout": 15.0,
    "visual_feedback": true
  }
}
```

### `engine` section


The engine config is nested into two backend sub-objects.

**Top-level fields:**

| Key | Type | Values | Description |
|---|---|---|---|
| `backend` | string | `"auto"`, `"whisper-cpp"`, `"moonshine"` | Which backend to use; `auto` selects based on GPU availability |

**`whisper_cpp` sub-object:**

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"tiny"` | Whisper model to load (see valid values below). `tiny`/`tiny.en` auto-download silently on first launch; other sizes require an explicit download in Settings → Engine. |
| `device` | string | `"auto"` | Compute device: `auto`/`cpu`/`cuda`/`vulkan` |
| `threads` | integer | `0` | CPU thread count; 0 = half of logical cores |
| `model_dir` | string | `""` | Custom model directory; empty = `~/.local/share/voxctrl/models/`. Supports `~` expansion. The directory must already exist. |

Valid `model_size` values: `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large-v2`, `large-v3`, `large-v3-turbo`

The `.en` variants are English-only but slightly faster. `large-v3-turbo` is a distilled model balancing quality and speed.

**`moonshine` sub-object** (only used when `backend = "moonshine"`):

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"base"` | `"base"` or `"tiny"` |
| `language` | string | `"en"` | BCP-47 language code (output label only) |

> **Build requirement:** the Moonshine backend is a **default** compile-time
> feature, so a standard build includes it. It links ONNX Runtime, which is
> fetched at build time — a build made with `--no-default-features` omits it,
> and selecting `"moonshine"` then transparently falls back to `whisper-cpp`,
> still using the Whisper model configured above. The Settings → Engine panel
> shows whether Moonshine is available in the running build.
>
> A Moonshine model is two upstream ONNX graphs (`encoder_model.onnx` and
> `decoder_model_merged.onnx`), downloaded on demand into
> `~/.local/share/voxctrl/models/moonshine/<size>/`. You can also place those
> two files there manually to run fully offline. The tokenizer is bundled into
> the app, so it is not downloaded.

### `audio` section

| Key | Type | Default | Description |
|---|---|---|---|
| `vad_threshold` | float | `0.5` | Voice Activity Detection sensitivity (0.0–1.0); **higher = more sensitive** (lower RMS gate) |
| `input_device_index` | integer or null | `null` | CPAL device index; null = auto-detect |
| `evdev_device` | string or null | `null` | Linux evdev keyboard device path for hotkeys, e.g. `"/dev/input/event4"` |
| `noise_suppression` | bool | `false` | Enable basic noise suppression pre-processing |
| `gain` | float | `1.0` | Microphone amplification multiplier |
| `dynamic_stream` | bool | `true` | Open mic on-demand (true) vs. always-on (false) |

**VAD threshold note:** The threshold maps as `rms_gate = (1.0 - vad_threshold) * 0.006`. At 0.5 (default), the gate threshold is 0.003 RMS. At 1.0 (maximum sensitivity), there is no gate (0.0 RMS). At 0.0 (minimum sensitivity), the gate is 0.006 RMS.

### `ui` section

| Key | Type | Allowed | Default | Description |
|---|---|---|---|---|
| `show_overlay` | bool | | `true` | Show visual HUD overlay during recording |
| `show_command_overlay` | bool | | `true` | Show temporary UI overlay pill when a voice command trigger is activated |
| `command_overlay_duration_secs` | integer | `1`–`10` | `3` | Duration in seconds to display the voice command overlay pill |
| `overlay_style` | string | `"voice_card"`, `"waveform"`, `"pulse"`, `"blue_wave"`, `"mono_bars"`, `"spectrum"`, `"terminal"`, `"vinyl"`, `"none"` | `"blue_wave"` | HUD visualization style |
| `overlay_position` | string | `"top"`, `"center"`, `"bottom"` | `"center"` | Screen positioning of the overlay window |
| `overlay_monitor` | string | `"primary"` or monitor name | `"primary"` | Specific display screen for visual overlay |
| `auto_show_settings` | bool | | `true` | Auto-show Settings window on startup |
| `show_notification` | bool | | `false` | Desktop toast notification after text delivery |
| `setup_completed` | bool | | `false` on a new install | Whether the first-run wizard has been finished. Absent from a config file written by an earlier VoxCtrl, which is read as `true` — an existing install has plainly been set up already, and must not be handed a setup wizard on upgrade |

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

### `openai` section

LLM post-processing through any OpenAI-compatible API server — a local server or
a hosted provider. Exposed in the GUI under **Settings → OpenAI API**.

> Configs written before this section was renamed used the key `ollama`; that key
> is still accepted (via a serde alias) and loads transparently into `openai`.

Each request sends two chat messages: the **system prompt** (how to transform the
text) and the **user prompt** (the message itself). The user prompt must contain
`{text}`, which is replaced with the dictated speech.

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable LLM post-processing |
| `endpoint` | string | `"http://localhost:11434"` | OpenAI-compatible API base URL. A `/v1` suffix is optional — it is added automatically when missing (e.g. requests go to `{endpoint}/v1/chat/completions`). |
| `api_key` | string or null | `null` | API key sent as a `Bearer` token. Required by most remote providers; usually unnecessary for a local server. |
| `model` | string | `"llama3.2:1b"` | Model name |
| `mode` | string | `"clean"` | Preset that fills the system prompt in the GUI: `clean`/`formal`/`casual`/`bullet`/`concise`/`custom`. Built-in presets are read-only in the GUI; choose `custom` to edit `system_prompt`/`user_prompt`. Generation itself is driven by `system_prompt`/`user_prompt`. |
| `system_prompt` | string | `"Fix grammar and punctuation only…"` | System message describing the transformation. Empty = no system message. |
| `user_prompt` | string | `"{text}"` | User message template. Must contain `{text}`, replaced with the dictated speech. |
| `timeout_secs` | integer | `8` | HTTP request timeout in seconds |

> The legacy `custom_prompt` field (used when `mode` was `custom`) is migrated
> into `user_prompt` automatically the first time an old config is loaded.

### `tts` section

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable TTS subsystem |
| `engine` | string | `"piper"` | Synthesis engine: `"breeze_tts_2"`, `"piper"`, `"pocket_tts"`, `"inflect_micro"`, or `"espeak"` |
| `voice` | string | `"en-us-lessac-medium"` | Active Piper voice name (hyphen-delimited, e.g. `"en-us-ryan-high"`) |
| `voice_dir` | string | `""` | Directory for Piper voice files; empty = `~/.local/share/voxctrl/piper-voices/`. Supports `~` expansion. |
| `stop_key` | string[] | `["KEY_ESCAPE"]` | Keys that cancel current TTS playback |
| `response_overlay` | bool | `true` | Show overlay indicator while TTS is speaking |
| `speed` | float | `1.0` | Speech synthesis speed multiplier (0.5 – 2.0); not used by Pocket-TTS |
| `gpu` | bool | `false` | Enable GPU acceleration (CUDA) for Piper |
| `breeze_tts_2` | object | | Breeze-TTS-2 engine sub-configuration (see below) |
| `pocket_tts` | object | | Pocket-TTS engine sub-configuration (see below) |
| `inflect_micro` | object | | Inflect-Micro-v2 engine sub-configuration (see below) |

**`breeze_tts_2` sub-object:**

[Breeze-TTS-2](https://huggingface.co/BreezeBlue/Breeze-TTS-2) is a bilingual speech generation model with natural-language voice design speaker prompts. The model weights are gated on HuggingFace under the **BreezeBlue Research and Non-Commercial License** — supply your access token via `hf_token` (shared with Pocket-TTS).

| Key | Type | Default | Description |
|---|---|---|---|
| `speaker_prompt` | string | `"A calm and clear female voice speaking at a natural pace"` | Natural-language prompt describing the desired speaker voice for Voice Design |
| `model_dir` | string | `""` | Directory holding model weights & tokenizer; empty = `~/.local/share/voxctrl/models/breeze-tts-2/` |
| `hf_token` | string or null | `null` | HuggingFace access token used to download gated model weights (shared with Pocket-TTS) |
| `prewarm` | bool | `false` | Pre-warm model weights and tensors on startup so first speech is instantaneous |
| `gpu` | bool | `false` | Enable CUDA GPU acceleration for maximum performance |
| `temperature` | float | `0.7` | Sampling temperature controlling voice expressiveness (0.1 – 1.0) |

**`pocket_tts` sub-object:**

Pocket-TTS is a voice-cloning neural TTS engine: each voice is a short reference audio clip
that conditions synthesis, rather than a fixed precomputed voice embedding. The model weights
are hosted in a **gated** HuggingFace repository (`kyutai/pocket-tts`) — you must accept the
license on HuggingFace and supply a personal access token via `hf_token`.

| Key | Type | Default | Description |
|---|---|---|---|
| `voice` | string | `"alba"` | Bundled reference voice ID (`"alba"`, `"anna"`, `"vera"`, `"charles"`, `"michael"`), or the filename stem of a custom clip in `voice_dir` |
| `prewarm` | bool | `false` | Pre-warm model on startup so first speech is instantaneous |
| `hf_token` | string or null | `null` | HuggingFace access token used to download the gated model weights |
| `voice_dir` | string | `""` | Directory scanned for custom `.wav` voice clips; empty = `~/.local/share/voxctrl/pocket-tts-voices/`. Drop a `<id>.wav` file in to add it to the voice list — naming it after a built-in voice (e.g. `alba.wav`) overrides that voice's clip. Supports `~` expansion. |

**`inflect_micro` sub-object:**

[Inflect-Micro-v2](https://huggingface.co/owensong/Inflect-Micro-v2) is a ~9.4M-parameter VITS-family
ONNX model with a single fixed English voice at 24 kHz, so it has no voice setting. Requires the
`inflect-micro` cargo feature at build time and `espeak-ng` at runtime; speaking rate comes from the
shared `tts.speed`. See [tts.md](tts.md) for the full engine notes.

| Key | Type | Default | Description |
|---|---|---|---|
| `model_dir` | string | `""` | Directory holding the ONNX graphs and phoneme vocabulary; empty = `~/.local/share/voxctrl/models/inflect-micro/`. Point at an existing copy to skip downloading. Supports `~` expansion. |
| `seed` | int | `0` | Sampling seed. The model is deterministic for a fixed seed, so repeated synthesis of the same text is identical. |
| `noise_scale` | float | `0.667` | Latent sampling temperature (0.0 – 1.0) — higher is more varied, lower is flatter |
| `prewarm` | bool | `false` | Load the ONNX graphs on startup so the first synthesis has no load delay |


### `mcp` section

| Key | Type | Default | Description |
|---|---|---|---|
| `server_enabled` | bool | `false` | Start the MCP socket server on launch |
| `record_timeout` | float | `15.0` | Max seconds for `transcribe_voice` to wait for speech |
| `visual_feedback` | bool | `true` | Show overlay indicator while MCP server is listening to microphone |

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

**`command`** — Voice Command Router (routes speech based on spoken target names):
```toml
delivery = "command"
```
See [Output Routing](routing.md#command--voice-command-router) for dynamic keyword parsing rules and conversational lead-in examples.

**`chat`** — conversational LLM over an OpenAI-compatible API, with history:
```toml
delivery = "chat"
chat_url = "http://localhost:8080"     # /v1 suffix optional
chat_model = "hermes-4-14b"
chat_system_prompt = "You are a concise voice assistant."
chat_reply_mode = "speak"              # speak | inject | clipboard | none
chat_max_history = 20                  # messages sent per turn; 0 = whole conversation
chat_timeout_secs = 120
chat_reset_phrase = "new conversation" # optional spoken phrase that clears history
# chat_api_key = "sk-..."              # optional bearer token
```
See [Output Routing](routing.md#chat--conversational-llm-openai-compatible) for details.

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
openai_enabled = true          # Enable LLM rewrite specifically for this hotkey
openai_mode = "formal"         # Rewrite output in formal style for this hotkey

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
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "double_tap_hold"
tap_ms = 300
hold_threshold_ms = 200        # Hold threshold on the second tap
target_ids = ["default"]

[[binding]]
id = "toggle_dictation"
label = "Toggle Dictation"
keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]
gesture = "toggle"
target_ids = ["default"]
```

### Field reference

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `id` | string | Yes | | Unique identifier |
| `label` | string | Yes | `""` | Display name |
| `keys` | string[] | Yes | | Key names (evdev format, on every platform). Any number of modifiers plus exactly one regular key — see [Hotkeys](hotkeys.md#what-can-be-a-shortcut) |
| `gesture` | string | Yes | | `"hold"`, `"toggle"`, `"double_tap"`, or `"double_tap_hold"` |
| `target_ids` | string[] | Yes | | Ordered list of target IDs to route to |
| `target_id` | string | No | | Single target (legacy; use `target_ids`) |
| `hold_threshold_ms` | integer | No | `200` | Min hold duration in ms for hold / double-tap-hold gesture |
| `tap_ms` | integer | No | `300` | Longest gap between the first tap's release and the second tap's press, in ms |
| `disabled` | bool | No | `false` | Disable without deleting |
| `openai_enabled` | bool | No | `null` | Enable/disable LLM post-processing specifically for this hotkey (null = inherit global config) |
| `openai_model` | string | No | `null` | LLM model override specifically for this hotkey |
| `openai_mode` | string | No | `null` | LLM mode override specifically for this hotkey (`clean`/`formal`/`casual`/`bullet`/`concise`/`custom`) |
| `openai_prompt` | string | No | `null` | User prompt template override for this hotkey (must contain `{text}`) |
| `openai_system_prompt` | string | No | `null` | System prompt override for this hotkey (empty inherits the global default) |

> The per-hotkey field names were renamed from `ollama_*` to `openai_*`; the
> legacy `ollama_*` names are still accepted via serde aliases.

---

## Config Migration

VoxCtrl auto-migrates older config formats on load. Known migrations:

- `features.show_notification` → `ui.show_notification` (moved in an early release; the migrated config is immediately re-saved to disk to clean up the old key)

Unrecognized fields are silently ignored. Missing fields use their defaults. This ensures compatibility when upgrading or downgrading.
