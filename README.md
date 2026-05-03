# Whisper Wayland

Whisper Wayland is a native, on-device voice-to-text (dictation) tool designed for Linux, with first-class support for Wayland (and X11 compatibility). It uses OpenAI's Whisper model (via `faster-whisper`) and Silero VAD to provide fast, accurate, and private transcription that injects text directly into your active window.

![Banner](assets/banner.png)

## 🚀 Key Features

### ✨ Core Dictation
- **Fast Transcription**: Uses `faster-whisper` for low-latency on-device processing.
- **Hands-Free Mode**: Choose between "Hold-to-Talk" or "Toggle-to-Talk" modes.
- **GPU & CPU Support**: Optimized for NVIDIA CUDA (GPU) and high-performance CPU inference (int8).
- **Quiet Mode**: A specialized mode that boosts sensitivity for whispering or soft-spoken dictation.

### 🛠️ Power User Productivity
- **📎 Voice Snippets**: Define custom triggers like "my email" to instantly expand into "john@example.com".
- **💻 Developer Code Mode**: A specialized mode that converts spoken constructs into code (e.g., "get underscore user dot name" → `get_user.name`).
- **📋 Transcription History**: A persistent, searchable panel that keeps track of all your dictations with one-click copy buttons.
- **🤖 AI Post-Processing**: Optionally route transcriptions through **Ollama** for grammar correction, tone rewriting, or bullet-point conversion.

### 🔊 Audio & System
- **🔇 Noise Suppression**: Integrated `noisereduce` filter to clean up background hiss and room noise.
- **🎤 Live Waveform Overlay**: Visual feedback of your voice level while recording.
- **📡 DBus Interface**: Fully controllable via DBus (e.g., trigger dictation from Waybar, custom scripts, or Rofi).
- **📝 Spoken Punctuation**: Naturally say "period", "comma", or "new line" to format your text.

---

## 🛠️ Installation & Setup

### 1. System Dependencies (Arch Linux)
Whisper Wayland requires some system-level libraries for audio, DBus, and hotkey handling:

```bash
sudo pacman -S portaudio python-pyaudio wl-clipboard dbus pkgconf python-gobject ydotool
```

### 2. Project Setup
We recommend using a Virtual Environment (`venv`) to avoid "externally-managed-environment" errors.

```bash
# Clone the repository
git clone https://github.com/jrufer/whisper-wayland.git
cd whisper-wayland

# Create and activate venv
python -m venv venv

# For Fish shell:
source venv/bin/activate.fish
# For Bash/Zsh:
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt

# Optional: Install P2 Features (AI & Noise Suppression)
pip install noisereduce dbus-python

# Optional: NVIDIA GPU Acceleration
pip install nvidia-cublas-cu12 nvidia-cudnn-cu12
```

### 3. Udev Rules (Required for Hotkeys)
The app needs permission to listen to your keyboard and inject text.

Create `/etc/udev/rules.d/99-whisper-wayland.rules`:
```bash
KERNEL=="uinput", GROUP="uinput", MODE="0660"
```
Reload rules and add your user to the necessary groups:
```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG input,uinput $USER
```
*Note: You must log out and back in for group changes to take effect.*

---

## 🤖 Ollama Integration

Whisper Wayland can use [Ollama](https://ollama.com) to automatically refine your text after transcription.

### Recommended Models for English
For the fastest possible post-processing while maintain high quality:

1. **`llama3.2:1b` (Highly Recommended)**: 
   - **Pros**: Extremely fast (~1.3GB RAM), excellent at grammar correction and bullet points.
   - **Command**: `ollama pull llama3.2:1b`
2. **`phi3:mini`**: 
   - **Pros**: Very lightweight and efficient for simple rewrites.
   - **Command**: `ollama pull phi3:mini`
3. **`mistral`**: 
   - **Pros**: Better for complex "Casual" or "Formal" rewrites if your GPU has 8GB+ VRAM.

### How to Enable
Go to **⚙ Settings → 🤖 AI**, click **Re-check** to detect your Ollama installation, and toggle "Enable AI Post-Processing".

---

## 📡 DBus Control (Programmatic Usage)

You can control the app from external scripts or bars (e.g. Waybar):

**Service Name**: `ai.whisperwayland.Dictation`
**Interface**: `ai.whisperwayland.Dictation`

| Command | Shell Usage |
|---------|-------------|
| **Toggle** | `dbus-send --session --type=method_call --dest=ai.whisperwayland.Dictation /ai/whisperwayland/Dictation ai.whisperwayland.Dictation.ToggleRecording` |
| **Status** | `qdbus ai.whisperwayland.Dictation /ai/whisperwayland/Dictation GetStatus` |

---

## 📜 Usage
- **Hold to Talk**: Hold `Super+Space` (default), speak, release.
- **Toggle to Talk**: Tap `Ctrl+Super+Space` to start/stop.
- **History**: Access the history panel from the System Tray icon to find previous dictations.

## ⚖️ License
MIT
