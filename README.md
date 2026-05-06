# Whisper-Wayland

A native, on-device voice-to-text tool for Linux with first-class Wayland support (and X11 compatibility). Uses OpenAI's Whisper model for fast, private, offline transcription — and acts as a programmable **voice input broker** that routes speech to any destination: a focused window, a terminal agent, a file, a socket, or a shell command.

![Banner](assets/banner.png)

---

## Features

### Core Dictation
- **Dual transcription backends** — `faster-whisper` (NVIDIA CUDA) or `whisper.cpp` (AMD/Intel Vulkan), selected automatically
- **Hold-to-Talk**, **Toggle-to-Talk**, and **Double-Tap** hotkey modes
- **GPU & CPU support** — CUDA fp16, Vulkan, and int8 CPU fallback
- **Quiet Mode** — boosted VAD sensitivity for soft-spoken dictation
- **Spoken punctuation** — say "period", "new line", "open paren" to format as you speak
- **Filler-word removal**, **auto list formatting**, and **code mode**

### Voice-to-Agent Routing
- **Named output targets** — define any number of destinations with different delivery methods
- **Per-target hotkey bindings** — map any gesture (hold, toggle, double-tap) to any target
- **Delivery types**: `inject` (focused window), `clipboard`, `exec` (shell command), `pipe` (named FIFO), `socket` (TCP/Unix), `file` (append), `dbus` (signal)
- **Per-target post-processing** — independently control snippets, Ollama rewriting, and filler removal per target
- **TOML config files** — `targets.toml` and `bindings.toml` under `~/.config/whisper-wayland/`

### Text Processing
- **Voice snippets** — define triggers like "my email" that expand to full text
- **Code mode** — spoken constructs convert to syntax: `"get underscore user dot name"` → `get_user.name`
- **AI post-processing** — optional Ollama integration for grammar correction, tone rewriting, or bullet points

### Voice Output (TTS)
- **Neural TTS with Piper** — high-quality, on-device speech synthesis; `espeak-ng` used automatically as fallback
- **Voice picker** — choose from 8 curated Piper voices directly in Settings; each shows download status at a glance
- **One-click model download** — progress bar in-app; models stored in `~/.local/share/whisper-wayland/voices/`
- **Test button** — play a sample of any voice before committing to it
- **TTS stop key** — configurable global hotkey (default: `Escape`) interrupts playback from any window
- **Response overlay** — optional teal overlay displayed while TTS plays, distinct from the recording overlay

### MCP Server (AI Voice Gateway)
- **Built-in MCP server** — exposes voice I/O as tools any MCP-capable AI can call
- **`transcribe_voice` tool** — AI triggers the mic, user speaks, transcript returned
- **`speak_text` tool** — AI queues spoken responses through Piper/espeak
- **`get_status` tool** — AI queries whether recording or speaking is in progress
- **Claude Desktop integration** — one-click registration writes the `socat` bridge to `claude_desktop_config.json`
- **Response loopback** — per-target `response_pipe` FIFO: agents write responses there and they are spoken automatically
- Full documentation: **[docs/mcp_documentation.md](docs/mcp_documentation.md)**

### AT-SPI2 Accessibility Integration (optional)
- **Direct text insertion** — injects transcribed text via `AT-SPI2 Text.insertText` instead of simulating keystrokes; no modifier-key conflicts, no need for `wtype` or `xdotool`
- **Context-aware transcription** — reads the text preceding your cursor at recording start and passes it to Whisper as an `initial_prompt`, improving accuracy by priming the model with your document's vocabulary and style
- **Auto code mode** — automatically switches to code dictation mode when a terminal or IDE text widget is focused, without changing your global Settings

### System & UI
- **Transcription history** — persistent, searchable panel with one-click copy
- **Swappable recording overlays** — Waveform, Pulse Circle, Voice Card, or drop in your own; each displays a **routing indicator badge** showing exactly which output target is active while you record
- **Noise suppression** — optional `noisereduce` filter
- **DBus interface** — control from Waybar, scripts, or Rofi
- **Settings UI** — tabbed PyQt6 dialog covering all features
- **Keybind conflict detection** — inline warnings in Settings → Hotkeys flag exact duplicates, subset collisions, double-tap/combo overlaps, and bare single-key bindings

---

## Hardware Compatibility

| GPU Vendor | Backend | Notes |
|---|---|---|
| NVIDIA (CUDA 11+) | `faster-whisper` auto-selected | Install CUDA pip libraries — no extra steps |
| AMD (RDNA/GCN, Vulkan driver) | `whisper.cpp` auto-selected | Install `whisper-cpp-vulkan` from AUR or build from source |
| Intel Arc / Iris Xe (Vulkan driver) | `whisper.cpp` auto-selected | Build from source with `GGML_VULKAN=ON` |
| No GPU (CPU only) | `faster-whisper` int8 auto-selected | Works out of the box; slower for large models |

The backend is chosen automatically at startup. Override it in **Settings → Engine**.

---

## Installation

### 1. System dependencies

```bash
sudo pacman -S portaudio python-pyaudio wl-clipboard dbus pkgconf python-gobject ydotool wtype

# For TTS voice output (recommended)
sudo pacman -S alsa-utils          # aplay for Piper audio output
yay -S piper-tts                   # Neural TTS engine
# OR minimal fallback:
sudo pacman -S espeak-ng

# For MCP server → Claude Desktop bridge
sudo pacman -S socat
```

### 2. Clone and set up the virtual environment

```bash
git clone https://github.com/jrufer/whisper-wayland.git
cd whisper-wayland

python -m venv venv
source venv/bin/activate        # bash/zsh
# source venv/bin/activate.fish # fish

pip install -r requirements.txt

# Optional: noise suppression
pip install noisereduce

# Optional: DBus control interface
pip install dbus-python

# Optional: AT-SPI2 accessibility integration (focus tracking, context-aware
# transcription, direct text insertion — see section below)
# Arch Linux:
sudo pacman -S python-atspi
# Debian / Ubuntu:
# sudo apt install python3-pyatspi

# Optional: NVIDIA GPU acceleration
pip install nvidia-cublas-cu12 nvidia-cudnn-cu12
```

### 3. Launch

```bash
./whisper-wayland.sh
```

The app starts in the system tray. If your compositor doesn't support system trays, the Settings window opens directly.

**On first launch**, if global hotkeys aren't yet configured, a setup wizard appears automatically. Click **Set Up Permissions**, enter your administrator password when prompted, then log out and back in. That's it — no terminal commands, no scripts to run manually.

> You can also open the wizard any time from the tray icon → **Set Up Hotkeys…**

---

## Backend Setup

### NVIDIA GPU — faster-whisper + CUDA

No binary required. Install the CUDA runtime libraries and `faster-whisper` is selected automatically when CUDA is detected:

```bash
pip install nvidia-cublas-cu12 nvidia-cudnn-cu12
```

### AMD / Intel GPU — whisper.cpp + Vulkan

whisper.cpp is a native binary installed separately from the Python dependencies.

**Option A — AUR (Arch Linux, recommended)**

```bash
# CPU only:
yay -S whisper-cpp

# With Vulkan GPU acceleration:
yay -S whisper-cpp-vulkan
```

**Option B — Build from source**

```bash
git clone https://github.com/ggerganov/whisper.cpp
cd whisper.cpp

# With Vulkan (AMD / Intel):
cmake -B build -DGGML_VULKAN=ON && cmake --build build -j$(nproc)

# With CUDA (NVIDIA alternative):
cmake -B build -DGGML_CUDA=ON && cmake --build build -j$(nproc)

sudo install build/bin/whisper-cli /usr/local/bin/
```

**Download a GGUF model**

Models are managed from **Settings → Engine → whisper.cpp Settings** with a one-click download button. To download manually:

```bash
mkdir -p ~/.local/share/whisper-wayland/models/

# Recommended — large-v3, Q5_K_M (~1.1 GB):
wget -P ~/.local/share/whisper-wayland/models/ \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_k_m.bin

# Smaller option for CPU-only use — base (~57 MB):
wget -P ~/.local/share/whisper-wayland/models/ \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin
```

**Optional: in-process mode (lower latency)**

Install `pywhispercpp` to run whisper.cpp inside the Python process instead of as a subprocess:

```bash
pip install pywhispercpp

# For Vulkan-enabled builds, install from source:
GGML_VULKAN=1 pip install git+https://github.com/abdeladim-s/pywhispercpp
```

---

## AT-SPI2 Accessibility Integration

AT-SPI2 (Assistive Technology Service Provider Interface) is the standard Linux accessibility bus. When the optional `pyatspi` library is installed, Whisper-Wayland gains three capabilities that work transparently alongside the existing injection chain.

### Installation

```bash
# Arch Linux
sudo pacman -S python-atspi

# Debian / Ubuntu
sudo apt install python3-pyatspi
```

No restart is needed — the module is loaded at startup and gracefully disabled when the library is absent.

### What it does

#### 1. Direct text insertion (no keystrokes)

When AT-SPI2 is available, transcribed text is inserted directly into the focused widget via the `AT-SPI2 Text.insertText` interface instead of simulating key events with `wtype` or `xdotool`. This eliminates the modifier-key conflicts that can occur when a hotkey is released at the same time virtual keyboard events are sent.

The app falls back automatically to `wtype` → portal → `xdotool` → clipboard for widgets that do not expose the `Text` interface (e.g. Electron apps, native terminal emulators using raw PTY I/O).

#### 2. Context-aware transcription

When you press your recording hotkey, Whisper-Wayland reads up to 300 characters of text immediately before the cursor in the focused widget and passes it to Whisper as an `initial_prompt`. This primes the model with your document's vocabulary, spelling, and sentence style, reducing errors on specialised terminology and proper nouns without any manual prompt configuration.

#### 3. Auto code mode

When the focused widget is a terminal or IDE text area (AT-SPI2 role `terminal` or `text`), the app automatically switches to **code dictation mode** for that recording session. Spoken constructs are converted to syntax (`"get underscore user dot name"` → `get_user.name`) without changing your global Settings. The mode resets to your configured default on the next recording.

### Configuration

All three behaviours are individually switchable in `~/.config/whisper-wayland/config.json`:

```json
{
  "atspi_injection":       true,
  "atspi_context_prompt":  true,
  "atspi_auto_code_mode":  true
}
```

| Key | Default | Description |
|---|---|---|
| `atspi_injection` | `true` | Try AT-SPI2 `insertText` before falling back to `wtype`/`xdotool` |
| `atspi_context_prompt` | `true` | Feed surrounding text to Whisper as `initial_prompt` at recording start |
| `atspi_auto_code_mode` | `true` | Switch to code dictation mode when a terminal/IDE widget is focused |

---

## Default Hotkeys

| Gesture | Keys | Action |
|---|---|---|
| Hold-to-Talk | `Super + Space` | Hold while speaking, release to transcribe and inject |
| Toggle-to-Talk | `Ctrl + Super + Space` | Tap to start recording, tap again to stop |
| Double-Tap | `Alt` | Double-tap and hold `Alt` to record, release to deliver |

All hotkeys are configurable in **Settings → Hotkeys** or directly in `bindings.toml`. Each gesture can be individually disabled from the same screen without deleting the binding.

### Conflict detection

The Hotkeys settings screen checks for common problems as you record new keys and shows inline warnings for:

- **Exact duplicate** — two gestures share the same keys (both fire simultaneously)
- **Subset collision** — one binding's keys are a subset of another's (the shorter one always fires with the longer)
- **Double-tap overlap** — the double-tap key appears in a hold or toggle combo (may cause mis-fires during normal chords)
- **Bare single key** — a non-modifier key used alone as hold or toggle intercepts every press of that key

### Double-tap hotkeys

Press and release a modifier key, then press it again within the tap window (default 250 ms) and hold while speaking. Release to deliver. This avoids collisions with normal modifier usage — double-tapping `Alt` never fires when `Alt` is held as part of a normal chord like `Alt+Tab`.

---

## Voice-to-Agent Routing

Routing lets you assign different hotkey gestures to named destinations so speech goes to the right tool without switching focus first.

### Quick example: voice to a terminal agent via named pipe

```bash
# 1. Create the named pipe (once, or add to your shell rc)
mkfifo /tmp/hermes.in

# 2. Start your agent reading from it
cat /tmp/hermes.in | hermes
```

`~/.config/whisper-wayland/targets.toml`:

```toml
format_version = "1.0"

[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
post_processing = "default"
append_newline = false

[[target]]
id = "hermes"
label = "Hermes Agent"
delivery = "pipe"
pipe_path = "/tmp/hermes.in"
post_processing = "strip_fillers"
append_newline = true
```

`~/.config/whisper-wayland/bindings.toml`:

```toml
format_version = "1.0"

[[binding]]
id = "default_hold"
label = "Dictate (Hold)"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_id = "default"

[[binding]]
id = "hermes_doubletap"
label = "Voice to Hermes (double-tap Ctrl)"
keys = ["KEY_LEFTCTRL"]
gesture = "double_tap"
target_id = "hermes"
tap_ms = 280
hold_threshold_ms = 200
```

If neither file exists, the app creates defaults that preserve the original `Super+Space` / `Ctrl+Super+Space` behavior.

### Delivery types

| Type | Mechanism | Typical use |
|---|---|---|
| `inject` | `wtype` / `xdotool` | Default dictation into focused window |
| `clipboard` | `wl-copy` | Copy to clipboard for manual paste |
| `exec` | `subprocess.Popen` (shell=False) | Any CLI tool: `claude --print {TEXT}`, `llm {TEXT}` |
| `pipe` | Write to a named FIFO | Interactive terminal agents |
| `socket` | TCP or Unix domain socket | Daemon-mode agents, remote processes |
| `file` | Append to a file | Voice journaling, meeting notes |
| `dbus` | Emit a DBus signal | Waybar integration, other apps |

Use `{TEXT}` as a placeholder in `exec` commands. It is substituted as a literal argument with `shell=False` to prevent injection attacks from transcribed text.

### Post-processing modes

| Value | Effect |
|---|---|
| `default` | Full pipeline: snippets, spoken punctuation, Ollama rewrite (if enabled) |
| `none` | Raw Whisper output — best for agent targets |
| `strip_fillers` | Remove um/uh/hmm only |
| `snippets_only` | Expand snippets, no rewriting |
| `ollama_only` | Skip snippets and code mode; run Ollama rewrite only |

> Agent targets (`exec`, `pipe`, `socket`) should almost always use `post_processing = "none"` or `"strip_fillers"` — rewriting alters command semantics.

### Agent examples

| Target | Delivery | Config snippet |
|---|---|---|
| Hermes Agent | pipe | `pipe_path = "/tmp/hermes.in"` |
| Claude Code | exec | `command = "claude --print {TEXT}"` |
| llm (Simon Willison) | exec | `command = "llm -m gpt-4o {TEXT}"` |
| Remote GPU server | socket | `socket_host = "192.168.1.50"`, `socket_port = 9000` |
| Voice journal | file | `file_path = "~/Documents/journal.md"`, `file_prefix = "- "` |

### Config file locations

```
~/.config/whisper-wayland/
├── config.json          # Global settings (managed by Settings UI)
├── targets.toml         # Output target definitions
├── bindings.toml        # Hotkey → target bindings
└── backups/             # Auto-backup before each save (last 20 kept)
```

---

## Custom Recording Overlays

The visual overlay shown while recording is fully swappable. Three styles ship out of the box:

| Style | Description |
|---|---|
| **Waveform** | Classic OpenGL oscilloscope (default) |
| **Pulse Circle** | Glowing circle that expands with audio amplitude |
| **Voice Card** | Scrolling bar waveform in a floating card |

Switch styles in **Settings → Appearance → Recording Overlay**. Changes take effect immediately — no restart needed.

### Routing Indicator Badge

Every overlay displays a **routing indicator badge** while recording — a small label showing the human-readable name of the active output target (e.g. `Focused Window`, `Hermes Agent`, `Voice Journal`). This gives you an unambiguous, at-a-glance confirmation of where your speech is being sent before you say a word.

- **Voice Card** — badge appears in the top-right corner of the card
- **Waveform** — badge appears centered above the waveform box
- **Pulse Circle** — badge appears centered above the pulse ring

The badge text comes directly from the `label` field of the active `OutputTarget` in `targets.toml`. When you use multiple hotkeys to route to different destinations, the badge changes with each activation so you always know which route is live.

Custom overlays receive the routing label through the `label` parameter of `show_mode(label)` and can use it however they like — or ignore it.

### Building Custom Overlays

Drop a single Python file into `~/.config/whisper-wayland/overlays/`. Click **"Open Overlays Folder"** in Settings to go there directly. A ready-to-edit template (`_template.py`) is created automatically the first time you open the folder.

Full specification and examples: **[docs/overlays.md](docs/overlays.md)**

---

## Voice Output (TTS)

Whisper-Wayland can speak responses aloud using [Piper](https://github.com/rhasspy/piper), an on-device neural TTS engine.

### Setup

1. Install Piper: `yay -S piper-tts` (or download from the [Piper releases page](https://github.com/rhasspy/piper/releases))
2. Open **Settings → Voice Output**
3. Select a voice from the picker
4. Click **⬇ Download** to fetch the model (~5–130 MB depending on quality)
5. Click **▶ Test Voice** to preview
6. Toggle **"Enable TTS"** on

`espeak-ng` is used automatically if Piper is not installed — no configuration needed.

### Voice models

| Voice | Language | Quality | Size |
|---|---|---|---|
| Lessac | US English | Medium | ~55 MB |
| Ryan | US English | Medium / High | ~55–130 MB |
| Amy | US English | Low | ~5 MB |
| Joe | US English | Medium | ~55 MB |
| Kusal | US English | Medium | ~55 MB |
| Danny | US English | Low | ~5 MB |
| Alan | GB English | Low | ~5 MB |

Models are downloaded from GitHub releases and stored in `~/.local/share/whisper-wayland/voices/`. Download once, use offline forever.

### TTS stop key

Press the configured key (default: `Escape`) from any window to stop TTS playback instantly. Change it in **Settings → Voice Output → TTS Stop Key** using the same Record/Done flow as hotkeys.

### Response overlay

When enabled, a teal floating overlay appears while TTS plays — distinct from the pink recording overlay — so you always know when the app is speaking.

---

## MCP Server

Whisper-Wayland can act as a **voice I/O gateway for AI agents** via its built-in MCP server. Enable it in **Settings → Voice Output → MCP Server**.

```json
{
  "mcp_server_enabled": true
}
```

An AI with MCP tool access can then:
- Call `transcribe_voice` → the app opens the mic and returns the user's speech as text
- Call `speak_text` → the app speaks the response aloud through Piper
- Call `get_status` → check if mic or TTS is currently active

**Claude Desktop integration:** click **"Register in Claude Desktop"** in Settings — the app writes the socat bridge config automatically.

Full setup guide, protocol reference, and integration examples: **[docs/mcp_documentation.md](docs/mcp_documentation.md)**

---

## Ollama AI Post-Processing

Whisper-Wayland can post-process transcriptions through a local [Ollama](https://ollama.com) model.

| Model | RAM | Best for |
|---|---|---|
| `llama3.2:1b` | ~1.3 GB | Grammar correction, bullet points — fastest |
| `phi3:mini` | ~2 GB | Simple rewrites |
| `mistral` | ~8 GB VRAM | Complex formal/casual rewrites |

```bash
ollama pull llama3.2:1b
```

Enable in **Settings → AI**: click **Re-check** to detect Ollama, then toggle **"Enable AI Post-Processing"**.

Per-target override: set `post_processing = "none"` on agent targets to skip Ollama for those routes even when it is globally enabled.

---

## DBus Control

Control the app from external scripts, Waybar, or Rofi.

**Service**: `ai.whisperwayland.Dictation`

| Action | Command |
|---|---|
| Toggle recording | `dbus-send --session --type=method_call --dest=ai.whisperwayland.Dictation /ai/whisperwayland/Dictation ai.whisperwayland.Dictation.ToggleRecording` |
| Get status | `qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetStatus` |
| Get word count | `qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetWordCount` |

---

## Architecture

```
Input Engine (evdev)
  ├── Hold / Toggle gesture handlers
  ├── DoubleTapMachine per double_tap binding
  └── TTS stop key interceptor → TTSEngine.stop()
        │ on_press(target_id)
        ▼
Recording Controller (AudioRecorder)
        │ numpy float32 audio
        ▼
Transcription (faster-whisper / whisper.cpp + Silero VAD)
        │ (text, target_id)
        ▼
Post-Processing (per target_id setting)
  ├── default: snippets + spoken punct + Ollama
  ├── none: raw Whisper output
  ├── strip_fillers: remove um/uh only
  ├── snippets_only: expand snippets
  └── ollama_only: Ollama rewrite only
        │
        ▼
OutputTargetRouter
  ├── inject    → wtype / xdotool / clipboard+paste
  ├── clipboard → wl-copy
  ├── exec      → subprocess (shell=False)
  ├── pipe      → O_NONBLOCK write to FIFO
  ├── socket    → TCP or Unix domain socket
  ├── file      → append with optional timestamp
  └── dbus      → DBus signal emission
        │
        ▼ (response_pipe per target)
ResponseListener(s)  ←── agent writes response text to FIFO
        │ tts_speak(line)
        ▼
TTSEngine (queue + worker thread)
  ├── piper --model … --output_raw | aplay …
  └── espeak-ng fallback
        │ on_started / on_finished callbacks
        ▼
TTSResponseOverlay (teal floating widget, shown while speaking)

                 ┌─────────────────────────┐
                 │    MCP Server           │
                 │  Unix socket JSON-RPC   │
                 │  transcribe_voice ──────┼──→ triggers recording
                 │  speak_text ────────────┼──→ TTSEngine.speak()
                 │  get_status ────────────┼──→ recording/speaking flags
                 └─────────────────────────┘
```

---

## Source Layout

```
src/
├── main.py                   # Application entry point
├── config.py                 # JSON config (model, audio, UI settings)
├── input_listener.py         # evdev hotkey engine (hold / toggle / double-tap / TTS stop)
├── audio_recorder.py         # PyAudio capture + VU meter
├── inference_engine.py       # Transcription + post-processing pipeline
├── text_injector.py          # Text delivery thread (inject + routing dispatch)
├── llm_postprocessor.py      # Ollama integration
├── dbus_service.py           # DBus control interface
├── portal_injector.py        # Wayland RemoteDesktop portal fallback
├── tts_engine.py             # Piper/espeak TTS engine, voice catalog, model download
├── tts_responder.py          # ResponseListener — reads agent FIFO → TTSEngine
├── mcp_server.py             # MCP JSON-RPC server (Unix socket)
├── hotkeys/
│   └── double_tap.py         # DoubleTapMachine state machine
├── routing/
│   ├── models.py             # GestureType, HotkeyBinding, DeliveryType, OutputTarget
│   ├── targets.py            # Delivery implementations
│   ├── loader.py             # TOML load/save for targets.toml + bindings.toml
│   └── router.py             # OutputTargetRouter
└── gui/
    ├── settings_window.py    # PyQt6 settings dialog (tabbed, incl. Voice Output tab)
    ├── tray_icon.py          # System tray icon
    ├── waveform_overlay.py   # OpenGL recording overlay
    ├── history_window.py     # Transcription history panel
    └── overlays/
        └── tts_response.py   # Teal TTS response overlay widget
docs/
├── overlays.md               # Custom overlay specification
└── mcp_documentation.md      # MCP server setup, protocol reference, integration guide
tests/
├── test_double_tap.py        # DoubleTapMachine state machine tests
├── test_targets.py           # Delivery implementation tests
├── test_routing_loader.py    # TOML config round-trip tests
├── test_tts_engine.py        # Voice catalog, download helpers, TTSEngine (30 tests)
├── test_tts_responder.py     # ResponseListener FIFO reading and TTS dispatch (6 tests)
└── test_mcp_server.py        # MCP protocol dispatch and socket server (16 tests)
```

---

## Running Tests

```bash
pip install pytest
python -m pytest tests/ -v
```

The test suite covers:

| File | Coverage |
|---|---|
| `test_double_tap.py` | DoubleTapMachine timing and state transitions |
| `test_targets.py` | All delivery types (inject, clipboard, exec, pipe, socket, file, dbus) |
| `test_routing_loader.py` | TOML round-trips for targets.toml and bindings.toml |
| `test_tts_engine.py` | Voice catalog validation, path helpers, download extraction, TTSEngine (30 tests) |
| `test_tts_responder.py` | ResponseListener FIFO reading, ordering, empty-line skip, late FIFO (6 tests) |
| `test_mcp_server.py` | JSON-RPC dispatch, all tools, error codes, socket server integration (16 tests) |

---

## License

MIT
