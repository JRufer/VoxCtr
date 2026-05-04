# Whisper Wayland

Whisper Wayland is a native, on-device voice-to-text (dictation) tool designed for Linux, with first-class support for Wayland (and X11 compatibility). It uses OpenAI's Whisper model and Silero VAD to provide fast, accurate, and private transcription that injects text directly into your active window.

It supports two transcription backends: **faster-whisper** (best for NVIDIA/CUDA) and **whisper.cpp** (GPU-accelerated on AMD and Intel via Vulkan), selected automatically based on your hardware.

![Banner](assets/banner.png)

## 🚀 Key Features

### ✨ Core Dictation
- **Dual Transcription Backend**: Automatically selects `faster-whisper` (NVIDIA CUDA) or `whisper.cpp` (AMD/Intel Vulkan) based on your GPU.
- **Fast Transcription**: Low-latency on-device processing — no cloud, no account.
- **Hands-Free Mode**: Choose between "Hold-to-Talk" or "Toggle-to-Talk" modes.
- **GPU & CPU Support**: CUDA for NVIDIA, Vulkan for AMD/Intel, int8 CPU fallback for everyone.
- **Quiet Mode**: Boosts VAD sensitivity for soft-spoken or whispered dictation.

### 🛠️ Power User Productivity
- **📎 Voice Snippets**: Define custom triggers like "my email" to instantly expand into "john@example.com".
- **💻 Developer Code Mode**: Converts spoken constructs into code (e.g., "get underscore user dot name" → `get_user.name`).
- **📋 Transcription History**: A persistent, searchable panel that keeps track of all your dictations with one-click copy buttons.
- **🤖 AI Post-Processing**: Optionally route transcriptions through **Ollama** for grammar correction, tone rewriting, or bullet-point conversion.

### 🔊 Audio & System
- **🔇 Noise Suppression**: Integrated `noisereduce` filter to clean up background hiss and room noise.
- **🎨 Swappable Recording Overlays**: Choose from built-in visual styles — classic Waveform, Pulse Circle, or Voice Card — or drop in your own custom overlay. See [Overlay UI Guide](docs/overlays.md).
- **📡 DBus Interface**: Fully controllable via DBus (e.g., trigger dictation from Waybar, custom scripts, or Rofi).
- **📝 Spoken Punctuation**: Naturally say "period", "comma", or "new line" to format your text.

---

## 🖥️ Hardware Compatibility

| GPU Vendor | Recommended Path | Notes |
|---|---|---|
| NVIDIA (CUDA 11+) | `faster-whisper` auto-selected | Install CUDA libraries — no extra steps |
| AMD (RDNA / GCN, Vulkan driver) | `whisper.cpp` auto-selected | Install `whisper-cpp-vulkan` from AUR or build from source |
| Intel Arc / Iris Xe (Vulkan driver) | `whisper.cpp` auto-selected | Build from source with `GGML_VULKAN=ON` |
| No GPU (CPU only) | `faster-whisper` int8 auto-selected | Works out of the box, slower for large models |

The backend is chosen automatically at startup. You can override it in **⚙ Settings → ⚡ Engine**.

---

## 🛠️ Installation & Setup

### 1. System Dependencies (Arch Linux)

```bash
sudo pacman -S portaudio python-pyaudio wl-clipboard dbus pkgconf python-gobject ydotool
```

### 2. Project Setup

```bash
# Clone the repository
git clone https://github.com/jrufer/whisper-wayland.git
cd whisper-wayland

# Create and activate a virtual environment
python -m venv venv
source venv/bin/activate        # Bash/Zsh
# source venv/bin/activate.fish # Fish shell

# Install dependencies
pip install -r requirements.txt

# Optional: noise suppression and DBus control interface
pip install noisereduce dbus-python
```

### 3. Udev Rules (Required for Hotkeys)

Create `/etc/udev/rules.d/99-whisper-wayland.rules`:
```
KERNEL=="uinput", GROUP="uinput", MODE="0660"
```

Reload rules and add your user to the required groups:
```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input,uinput $USER
```

*You must log out and back in for group changes to take effect.*

---

## ⚡ Backend Setup

### NVIDIA GPU (faster-whisper + CUDA)

No additional binary required. Install the CUDA runtime libraries:

```bash
pip install nvidia-cublas-cu12 nvidia-cudnn-cu12
```

faster-whisper will be selected automatically when CUDA is detected.

### AMD / Intel GPU (whisper.cpp + Vulkan)

whisper.cpp is a native binary and is installed separately from the Python dependencies.

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

Models are managed from **⚙ Settings → ⚡ Engine → whisper.cpp Settings** with a one-click download button. To download manually:

```bash
mkdir -p ~/.local/share/whisper-wayland/models/

# Recommended (large-v3, Q5_K_M, ~1.1 GB):
wget -P ~/.local/share/whisper-wayland/models/ \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_k_m.bin

# Smaller option for CPU-only use (base, ~57 MB):
wget -P ~/.local/share/whisper-wayland/models/ \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin
```

**Optional: lower-latency in-process mode**

Install `pywhispercpp` to run whisper.cpp inside the Python process instead of as a subprocess. This eliminates IPC overhead and is preferred automatically if installed:

```bash
pip install pywhispercpp

# For Vulkan-enabled builds, install from source:
GGML_VULKAN=1 pip install git+https://github.com/abdeladim-s/pywhispercpp
```

---

## ⚡ Engine Settings

Open **⚙ Settings → ⚡ Engine** to:

- **Select backend** — Auto (default), faster-whisper, or whisper-cpp.
- **View detected hardware** — GPU vendor, driver API, VRAM, active backend, and compute type.
- **Manage whisper.cpp models** — See which GGUF models are downloaded, download new ones, and choose the binary path and device (Vulkan/CUDA/CPU).

Switching backends while the app is running unloads the current model and reloads the new one — this takes a few seconds for large models. A restart is prompted after saving if the backend or model changed.

---

## 🤖 Ollama Integration

Whisper Wayland can use [Ollama](https://ollama.com) to automatically refine your text after transcription. Works with both backends.

### Recommended Models

| Model | Size | Best For |
|---|---|---|
| `llama3.2:1b` | ~1.3 GB RAM | Grammar correction, bullet points — fast |
| `phi3:mini` | ~2 GB RAM | Simple rewrites, lightweight |
| `mistral` | ~4 GB RAM | Complex tone rewrites (8 GB+ VRAM recommended) |

```bash
ollama pull llama3.2:1b
```

Enable in **⚙ Settings → 🤖 AI** → Re-check → toggle "Enable AI Post-Processing".

---

## 📡 DBus Control

Control Whisper Wayland from external scripts, Waybar, or Rofi:

**Service**: `ai.whisperwayland.Dictation`

| Action | Command |
|---|---|
| Toggle recording | `dbus-send --session --type=method_call --dest=ai.whisperwayland.Dictation /ai/whisperwayland/Dictation ai.whisperwayland.Dictation.ToggleRecording` |
| Get status | `qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetStatus` |

---

## 📜 Usage

- **Hold to Talk**: Hold `Super+Space` (default), speak, release to transcribe.
- **Toggle to Talk**: Tap `Ctrl+Super+Space` to start, tap again to stop.
- **History**: Access previous dictations from the system tray icon.
- **Settings**: All configuration in one place — open from the tray or DBus.

---

## 🎨 Custom Recording Overlays

The visual overlay shown while recording is fully swappable. Three styles ship out of the box:

| Style | Description |
|-------|-------------|
| **Waveform** | Classic OpenGL oscilloscope (default) |
| **Pulse Circle** | Glowing circle that expands with audio amplitude |
| **Voice Card** | Scrolling bar waveform in a floating card with a pink/magenta gradient |

To switch styles, open **⚙ Settings → ✨ Dictation → Overlay Appearance** and pick from the dropdown. The change takes effect immediately — no restart needed.

You can also build your own overlay by dropping a single Python file into `~/.config/whisper-wayland/overlays/`. Click **"Open Overlays Folder"** in Settings to go there directly. A ready-to-edit template file (`_template.py`) is created automatically the first time you open the folder.

Full specification and examples: **[docs/overlays.md](docs/overlays.md)**

---

## ⚖️ License
MIT
