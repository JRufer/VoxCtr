# Whisper-Wayland

Whisper-Wayland is a native, on-device voice-to-text tool for Linux with first-class Wayland support (and X11 compatibility). It uses OpenAI's Whisper model via `faster-whisper` and Silero VAD for fast, private, on-device transcription — and now acts as a programmable **voice input broker** that can route speech to any destination: a focused window, a terminal agent, a file, a socket, or a shell command.

![Banner](assets/banner.png)

---

## Key Features

### Core Dictation
- **Fast on-device transcription** via `faster-whisper` (CTranslate2 backend)
- **Hold-to-Talk** and **Toggle-to-Talk** hotkey modes
- **Double-Tap hotkeys** — press a modifier key twice rapidly to activate a specific route without interfering with normal typing
- **GPU & CPU support** — NVIDIA CUDA (fp16) and high-performance CPU (int8)
- **Quiet Mode** — boosted VAD sensitivity for soft-spoken dictation
- **Spoken punctuation** — say "period", "new line", "open paren" to format as you speak
- **Auto list formatting**, filler-word removal, code mode

### Voice-to-Agent Routing
- **Named output targets** — define any number of destinations with different delivery methods
- **Per-target hotkey bindings** — map any gesture (hold, toggle, double-tap) to any target
- **Delivery types**: `inject` (focused window), `clipboard`, `exec` (shell command), `pipe` (named FIFO), `socket` (TCP / Unix domain), `file` (append to log), `dbus` (signal)
- **Per-target post-processing** — each target independently controls whether snippets, Ollama rewriting, filler removal, or no processing at all is applied
- **Active target indicator** — tray tooltip and waveform overlay show where speech is being routed during recording
- **TOML config files** — human-readable `targets.toml` and `bindings.toml` under `~/.config/whisper-wayland/`
- **Auto-backup** — every settings save copies the previous config to `backups/` (keeps last 20)

### Text Processing
- **Voice snippets** — define triggers like "my email" that expand to full text
- **Code mode** — spoken constructs convert to syntax: "get underscore user dot name" → `get_user.name`
- **AI post-processing** — optional Ollama integration for grammar correction, tone rewriting, bullet-point conversion

### System & UI
- **Transcription history** — persistent, searchable panel with one-click copy
- **Live waveform overlay** — OpenGL-rendered audio feedback during recording
- **Noise suppression** — optional `noisereduce` filter for background hiss
- **DBus interface** — full programmatic control from Waybar, scripts, or Rofi
- **Settings UI** — tabbed PyQt6 dialog covering all features including a Routing tab

---

## Installation

### 1. System dependencies (Arch Linux)

```bash
sudo pacman -S portaudio python-pyaudio wl-clipboard dbus pkgconf python-gobject ydotool wtype
```

### 2. Project setup

```bash
git clone https://github.com/jrufer/whisper-wayland.git
cd whisper-wayland

python -m venv venv
source venv/bin/activate          # bash/zsh
# source venv/bin/activate.fish   # fish

pip install -r requirements.txt

# Optional: noise suppression and DBus support
pip install noisereduce dbus-python

# Optional: NVIDIA GPU acceleration
pip install nvidia-cublas-cu12 nvidia-cudnn-cu12
```

### 3. udev rules (required for hotkeys and text injection)

Create `/etc/udev/rules.d/99-whisper-wayland.rules`:

```
KERNEL=="uinput", GROUP="uinput", MODE="0660"
```

Reload and add your user to the required groups:

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input,uinput $USER
```

Log out and back in for group changes to take effect.

### 4. Run

```bash
cd src
python main.py
```

---

## Usage

### Default hotkeys

| Gesture | Keys | Action |
|---------|------|--------|
| Hold-to-Talk | `Super + Space` | Hold while speaking, release to inject |
| Toggle-to-Talk | `Ctrl + Super + Space` | Tap to start, tap again to stop |

All hotkeys are configurable in **Settings → Hotkeys** or in `bindings.toml`.

### Double-tap hotkeys

Press and release a modifier key, then press it again within the tap window (default 250 ms) and hold while speaking. Release to deliver. This pattern avoids collisions with normal chord usage (e.g., double-tapping Ctrl never fires when Ctrl is used as a modifier in Ctrl+C).

---

## Voice-to-Agent Routing

Routing lets you assign different hotkey gestures to named destinations so speech goes to the right tool without first switching focus.

### Quick example: voice-to-Hermes via named pipe

```bash
# 1. Create the named pipe (once, or add to your shell rc)
mkfifo /tmp/hermes.in

# 2. Start Hermes reading from it
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

### Delivery types

| Type | Mechanism | Typical use |
|------|-----------|-------------|
| `inject` | `wtype` / `xdotool` | Default dictation into focused window |
| `clipboard` | `wl-copy` | Copy to clipboard for manual paste |
| `exec` | `subprocess.Popen` with `shell=False` | Any CLI tool: `claude --print {TEXT}`, `llm {TEXT}` |
| `pipe` | Write to a named FIFO | Interactive terminal agents (Hermes, etc.) |
| `socket` | TCP or Unix domain socket | Daemon-mode agents, remote processes |
| `file` | Append to a file | Voice journaling, meeting notes |
| `dbus` | Emit a DBus signal | Waybar integration, other apps |

Use `{TEXT}` as a placeholder in `exec` commands — it is substituted as a literal argument with `shell=False` to prevent injection from transcribed text.

### Post-processing modes

| Value | Effect |
|-------|--------|
| `default` | Full pipeline: snippets, spoken punctuation, Ollama rewrite (if enabled) |
| `none` | Deliver raw Whisper output — best for agent targets |
| `strip_fillers` | Remove um/uh/hmm only |
| `snippets_only` | Expand snippets, no rewriting |
| `ollama_only` | Skip snippets and code mode; run Ollama rewrite only |

Agent targets (`exec`, `pipe`, `socket`) should almost always use `post_processing = "none"` or `"strip_fillers"` — rewriting alters command semantics.

### Agent examples

| Target | Delivery | Config snippet |
|--------|----------|----------------|
| Hermes Agent | pipe | `pipe_path = "/tmp/hermes.in"` |
| Claude Code | exec | `command = "claude --print {TEXT}"` |
| llm (Simon Willison) | exec | `command = "llm -m gpt-4o {TEXT}"` |
| aichat | exec | `command = "aichat {TEXT}"` |
| Remote GPU server | socket | `socket_host = "192.168.1.50"`, `socket_port = 9000` |
| Voice journal | file | `file_path = "~/Documents/journal.md"`, `file_prefix = "- "` |

### Config file location and backup

```
~/.config/whisper-wayland/
├── config.toml          # Global settings
├── targets.toml         # Output target definitions
├── bindings.toml        # Hotkey → target bindings
├── snippets.toml        # Voice snippet expansions
└── backups/             # Auto-backup before each save (last 20 kept)
```

If neither `targets.toml` nor `bindings.toml` exist, the app creates defaults that preserve the original Super+Space / Ctrl+Super+Space behavior unchanged.

---

## Routing Settings UI

Open **Settings → 🔀 Routing** to manage targets and bindings without editing TOML files.

- **Targets panel** — Add, Edit, Delete, and Test output targets. The Test button checks reachability (FIFO exists, binary on PATH, socket connectable, directory writable).
- **Bindings panel** — Add, Edit, Delete hotkey bindings. Disabled bindings are shown dimmed and skipped at runtime.
- All changes are saved immediately to `targets.toml` / `bindings.toml` and hot-reloaded into the running engine.

---

## Ollama Integration

Whisper-Wayland can post-process transcriptions through a local [Ollama](https://ollama.com) model.

**Recommended models for English:**

| Model | RAM | Best for |
|-------|-----|---------|
| `llama3.2:1b` | ~1.3 GB | Grammar correction, bullet points — fastest |
| `phi3:mini` | ~2 GB | Simple rewrites |
| `mistral` | ~8 GB VRAM | Complex formal/casual rewrites |

Enable in **Settings → 🤖 AI**: click **Re-check** to detect Ollama, then toggle "Enable AI Post-Processing".

Per-target override: set `post_processing = "none"` on agent targets to skip Ollama for those routes even when it is globally enabled.

---

## DBus Control

Control the app from external scripts or status bars.

**Service**: `ai.whisperwayland.Dictation`

| Command | Shell |
|---------|-------|
| Toggle | `dbus-send --session --type=method_call --dest=ai.whisperwayland.Dictation /ai/whisperwayland/Dictation ai.whisperwayland.Dictation.ToggleRecording` |
| Status | `qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetStatus` |
| Word count | `qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetWordCount` |

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
Transcription (faster-whisper + Silero VAD)
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
  ├── inject  → wtype / xdotool / clipboard+paste
  ├── clipboard → wl-copy
  ├── exec    → subprocess (shell=False)
  ├── pipe    → O_NONBLOCK write to FIFO
  ├── socket  → TCP or Unix domain socket
  ├── file    → append with optional timestamp
  └── dbus    → DBus signal emission
```

The routing layer sits entirely above the existing transcription pipeline. The default inject path is unchanged — users who do not configure any custom targets or bindings see exactly the same behavior as before.

---

## Source layout

```
src/
├── main.py                   # Application entry point
├── config.py                 # JSON config (model, audio, UI settings)
├── input_listener.py         # evdev hotkey engine (hold / toggle / double-tap)
├── audio_recorder.py         # PyAudio capture + VU meter
├── inference_engine.py       # faster-whisper + post-processing pipeline
├── text_injector.py          # Text delivery thread (inject + routing dispatch)
├── llm_postprocessor.py      # Ollama integration
├── dbus_service.py           # DBus control interface
├── portal_injector.py        # Wayland RemoteDesktop portal fallback
├── hotkeys/
│   └── double_tap.py         # DoubleTapMachine state machine
├── routing/
│   ├── models.py             # GestureType, HotkeyBinding, DeliveryType, OutputTarget
│   ├── targets.py            # Delivery implementations (inject/exec/pipe/socket/file/dbus)
│   ├── loader.py             # TOML load/save for targets.toml + bindings.toml
│   └── router.py             # OutputTargetRouter
└── gui/
    ├── settings_window.py    # PyQt6 settings dialog (7 tabs including Routing)
    ├── tray_icon.py          # System tray icon + active-target tooltip
    ├── waveform_overlay.py   # OpenGL waveform + target label overlay
    └── history_window.py     # Transcription history panel
tests/
├── test_double_tap.py        # State machine tests
├── test_targets.py           # Delivery implementation tests
└── test_routing_loader.py    # TOML config round-trip tests
```

---

## Running tests

```bash
pip install pytest
python -m pytest tests/ -v
```

---

## License

MIT
