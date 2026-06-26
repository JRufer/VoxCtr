# Installation & Setup

## System Requirements

### Linux
- **OS:** Any modern Linux distro (Ubuntu 20.04+, Fedora 36+, Arch, etc.)
- **Display:** X11 or Wayland
- **Audio:** PulseAudio or PipeWire (ALSA fallback supported)
- **Required packages:** `libwebkit2gtk-4.1`, `libayatana-appindicator3` or `libappindicator3`
- **Optional:** `wtype` (Wayland injection), `xdotool` (X11 injection)
- **For Kokoro TTS:** `espeak-ng` (phonemisation), ONNX Runtime (inference — see [Kokoro TTS](#kokoro-tts) below)

### Windows
- **OS:** Windows 10 (1903+) or Windows 11
- **Runtime:** WebView2 (pre-installed on Win11; auto-downloadable on Win10)

---

## Installing the AppImage (Linux)

The recommended distribution format is an AppImage — a single portable executable with all dependencies bundled.

```bash
# Download the latest AppImage
curl -LO https://github.com/jrufer/voxctrl/releases/latest/download/VoxCtrl.AppImage

# Make executable
chmod +x VoxCtrl.AppImage

# Run normally
./VoxCtrl-x86_64.AppImage

# Or run the built-in installer for desktop integration and hardware permissions:
./VoxCtrl-x86_64.AppImage --install
```

### Setup Methods

VoxCtrl supports two ways to perform system configuration (udev rules, input group, desktop shortcut, and high-res icon):
1. **CLI Mode:** Run `./VoxCtrl-x86_64.AppImage --install` in a terminal. It will prompt for your administrator password via `sudo` and configure everything.
2. **GUI Mode:** Launch the AppImage normally. If hardware permissions are missing, a diagnostics window will automatically appear. Click the **🔧 Setup System Integration** button; it will securely prompt for your password via `pkexec` and configure everything in the background.

The built-in installer:
1. Copies the application icon to `~/.local/share/icons/hicolor/128x128/apps/voxctrl.png`
2. Registers a `.desktop` launcher file in `~/.local/share/applications/voxctrl.desktop` linking to the active AppImage path
3. Establishes hardware udev rules (`/etc/udev/rules.d/99-voxctrl.rules`) to grant hotkey device permissions
4. Adds your active user account to the system `input` group

> **Note:** After running the installer, you must log out and log back in (or reboot) for the new `input` group permissions to take effect. Until then, global hotkeys will not work.
> 
> **Robust Container / Sandbox Support:** VoxCtrl employs intelligent diagnostic checks that support legacy rule files (`99-voxctrl.rules`, `99-voxctl.rules`, or `99-voxctr.rules`) and dynamically fallback to checking the system's NSS group database (`/etc/group`) if the current container process session fails to refresh its active group token, ensuring that the warning screen does not show up repeatedly on every launch.

---

## Permissions Setup (Linux)

### Global Hotkeys
VoxCtrl uses evdev to listen for global keyboard events. Your user must be in the `input` group:

```bash
sudo usermod -aG input $USER
# Log out and back in
```

Verify:
```bash
groups $USER | grep input
```

### Wayland Text Injection
For Wayland sessions, install `wtype`:
```bash
# Ubuntu/Debian
sudo apt install wtype

# Arch
sudo pacman -S wtype

# Fedora
sudo dnf install wtype
```

### X11 Text Injection
For X11 sessions, install `xdotool`:
```bash
# Ubuntu/Debian
sudo apt install xdotool

# Arch
sudo pacman -S xdotool
```

---

## First Run

On first launch, VoxCtrl will:
1. Create `~/.config/voxctrl/` with default `config.json`, `targets.toml`, and `bindings.toml`
2. Create `~/.local/share/voxctrl/` for model and voice storage
3. Open the Settings window

### Download a Whisper Model
Before you can dictate, you need a speech recognition model:
1. Go to Settings → Engine
2. Choose a model size (recommendation: `small` for a good speed/accuracy balance; the default is `large-v3` for maximum accuracy)
3. Click "Download" and wait for completion (~142 MB for `base`, ~466 MB for `small`, ~3 GB for `large-v3`)

### Configure a Hotkey
A default binding (`Super + Space`, hold gesture → inject to focused window) is created automatically. Verify it in Settings → Hotkeys, or change the key combo if it conflicts with your desktop environment.

### Test Dictation
1. Open any text editor
2. Click into the text area
3. Hold `Super + Space` and speak
4. Release to transcribe

---

## Optional Setup

### GPU Acceleration

**Vulkan (AMD / Intel / NVIDIA):** Set `engine.whisper_cpp.device = "vulkan"` in config, or choose "Vulkan" in Settings → Engine. Install driver support if needed:

```bash
# Ubuntu
sudo apt install vulkan-tools libvulkan1

# Arch
sudo pacman -S vulkan-icd-loader
```

**NVIDIA CUDA:** CUDA acceleration requires a CUDA-enabled build of VoxCtrl — it is not available in the standard pre-built AppImage. You must compile from source with:

```bash
npm run tauri build -- --features cuda
```

Once running a CUDA build, set `engine.whisper_cpp.device = "auto"` (or `"cuda"`) and VoxCtrl will use the GPU automatically. The "CUDA (NVIDIA)" option in Settings → Engine is only shown when the binary was compiled with CUDA support.


### LLM Post-Processing (OpenAI-compatible API)
If you want LLM grammar correction, point VoxCtrl at any OpenAI-compatible API
server. For a fully local setup using [Ollama](https://ollama.ai/):
1. Install [Ollama](https://ollama.ai/)
2. Pull a model: `ollama pull llama3.2`
3. Enable in Settings → OpenAI API (the default URL `http://localhost:11434`
   already points at a local Ollama instance)

To use a remote provider instead, set the **API URL** to its base URL and
provide an **API Key**.

### Kokoro TTS

The Kokoro neural TTS engine requires two system components. `install.sh` handles both automatically; for manual setup:

**1. espeak-ng** (phonemisation):

```bash
# Ubuntu/Debian
sudo apt install espeak-ng

# Arch
sudo pacman -S espeak-ng

# Fedora
sudo dnf install espeak-ng

# openSUSE
sudo zypper install espeak-ng
```

**2. ONNX Runtime** (model inference):

```bash
# Arch (via AUR)
yay -S onnxruntime

# All other distros
pip install onnxruntime
```

> **Note:** VoxCtrl auto-discovers the ONNX Runtime library at launch — it searches common system paths and queries `python3` for the location of any installed `onnxruntime` package (including `pip --user` installs). If auto-discovery fails, set `ORT_DYLIB_PATH=/path/to/libonnxruntime.so` in your environment before launching VoxCtrl.

Once both prerequisites are installed, download the Kokoro model from **Settings → TTS → Kokoro**. The `fp16` quality preset (169 MB) is recommended for most systems.

### MCP Server (Claude Desktop / Cursor)
1. Enable in Settings → Engine → MCP Server
2. Configure your MCP client to connect to `/tmp/voxctrl-mcp.sock`

---

## Building from Source

See [Development Guide](./development.md).

---

## Troubleshooting

### Hotkeys not working
- Check you are in the `input` group: `groups | grep input`
- Log out and back in after adding to group
- On some distros, the udev rule path differs — check the built-in installer logic for details

### No audio devices found
- Run `arecord -l` to verify your mic is recognized by ALSA
- Check if PulseAudio/PipeWire is running: `pactl info`
- Try setting `audio.input_device_index` manually to a specific device index (integer, not null)

### Text not injecting on Wayland
- Verify `wtype` is installed: `which wtype`
- Some applications block synthetic input (e.g. terminals with certain settings)
- Clipboard fallback always works — use `delivery = "clipboard"` as a workaround

### Whisper outputs wrong language
- Set `engine.language` to your language code (e.g. `"de"`, `"fr"`, `"es"`)
- Use a larger model for better non-English accuracy

### AppImage won't launch
- Install FUSE: `sudo apt install fuse libfuse2`
- Or extract and run directly: `./VoxCtrl.AppImage --appimage-extract && squashfs-root/AppRun`

### Debugging & Crash Logs
If the application crashes, fails to launch, or encounters hardware/model errors, you can check the local startup and error log file:
- **Location:** `~/.local/share/voxctrl/startup_errors.log`
- **Privacy:** To protect your privacy, this file **never** records or contains any transcribed speech text or LLM prompts. It only logs system configurations (models loaded, input devices, sample rates) and application/compiler errors.
- **Submitting Reports:** Please attach this log file when opening issues or submitting crash reports on GitHub to help us diagnose the problem.

