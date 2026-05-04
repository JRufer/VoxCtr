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

### System & UI
- **Transcription history** — persistent, searchable panel with one-click copy
- **Swappable recording overlays** — Waveform, Pulse Circle, Voice Card, or drop in your own
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

Build your own overlay by dropping a single Python file into `~/.config/whisper-wayland/overlays/`. Click **"Open Overlays Folder"** in Settings to go there directly. A ready-to-edit template (`_template.py`) is created automatically the first time you open the folder.

Full specification and examples: **[docs/overlays.md](docs/overlays.md)**

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
  └── DoubleTapMachine per double_tap binding
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
```

---

## Source Layout

```
src/
├── main.py                   # Application entry point
├── config.py                 # JSON config (model, audio, UI settings)
├── input_listener.py         # evdev hotkey engine (hold / toggle / double-tap)
├── audio_recorder.py         # PyAudio capture + VU meter
├── inference_engine.py       # Transcription + post-processing pipeline
├── text_injector.py          # Text delivery thread (inject + routing dispatch)
├── llm_postprocessor.py      # Ollama integration
├── dbus_service.py           # DBus control interface
├── portal_injector.py        # Wayland RemoteDesktop portal fallback
├── hotkeys/
│   └── double_tap.py         # DoubleTapMachine state machine
├── routing/
│   ├── models.py             # GestureType, HotkeyBinding, DeliveryType, OutputTarget
│   ├── targets.py            # Delivery implementations
│   ├── loader.py             # TOML load/save for targets.toml + bindings.toml
│   └── router.py             # OutputTargetRouter
└── gui/
    ├── settings_window.py    # PyQt6 settings dialog (tabbed)
    ├── tray_icon.py          # System tray icon
    ├── waveform_overlay.py   # OpenGL recording overlay
    └── history_window.py     # Transcription history panel
tests/
├── test_double_tap.py        # DoubleTapMachine state machine tests
├── test_targets.py           # Delivery implementation tests
└── test_routing_loader.py    # TOML config round-trip tests
```

---

## Running Tests

```bash
pip install pytest
python -m pytest tests/ -v
```

---

## License

MIT
