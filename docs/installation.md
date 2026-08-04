# Installation & Setup

## System Requirements

### Linux
- **OS:** Any modern Linux distro (Ubuntu 20.04+, Fedora 36+, Arch, etc.)
- **Display:** X11 or Wayland
- **Audio:** PulseAudio or PipeWire (ALSA fallback supported)
- **Required packages:** `libwebkit2gtk-4.1`, `libayatana-appindicator3` or `libappindicator3`
- **Optional:** `wtype` (Wayland injection), `xdotool` (X11 injection)
- **For Pocket-TTS:** a HuggingFace account with the [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts) license accepted and an access token (see [Pocket-TTS](#pocket-tts) below)

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
2. **GUI Mode:** Launch the AppImage normally. If anything is missing, a **VoxCtrl Setup** window appears listing every step and its live status. Click **Grant keyboard access**; it securely prompts for your password via `pkexec` and configures everything in the background.

The built-in installer:
1. Copies the application icon to `~/.local/share/icons/hicolor/128x128/apps/voxctrl.png`
2. Registers a `.desktop` launcher file in `~/.local/share/applications/voxctrl.desktop` linking to the active AppImage path
3. Establishes hardware udev rules (`/etc/udev/rules.d/99-voxctrl.rules`) to grant hotkey device permissions
4. Adds your active user account to the system `input` group as a fallback

### No logout required

The udev rule tags input devices with systemd's `uaccess`, so `systemd-logind`
grants your **active session** an ACL as soon as the rules are reloaded and the
devices are re-triggered. That happens inside the setup itself, which is why
hotkeys normally work the second the password prompt closes.

If a system cannot apply that (no logind, an unusual session), VoxCtrl falls
back in order:

1. **Restart VoxCtrl** — offered as a button in the setup window, and done
   automatically at startup when possible. VoxCtrl relaunches itself through
   `sg input`, which picks up the group membership `usermod` just wrote without
   a new login session.
2. **Log out and back in** — only shown when neither of the above can work, and
   stated explicitly rather than assumed.

### Setup is verified, not assumed

The setup window reports what is actually true: how many input devices exist,
how many VoxCtrl can read, whether the installed rule is the current one, and
whether the keystroke-injection helper (`wtype`/`xdotool`) is present. It polls
while open, so finishing the setup — in the app or in a terminal — flips it to
green without reopening anything.

> **Robust Container / Sandbox Support:** VoxCtrl supports legacy rule files (`99-voxctrl.rules`, `99-voxctl.rules`, or `99-voxctr.rules`) and falls back to the system's NSS group database (`/etc/group`) when the current process session has not refreshed its group token, so the setup window does not reappear on every launch.

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

### Pocket-TTS

Pocket-TTS is a pure-Rust voice-cloning TTS engine (no system packages required) but its model weights live in a **gated** HuggingFace repository:

1. Create a free [HuggingFace](https://huggingface.co/) account if you don't have one.
2. Visit [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts) and accept the model license.
3. Create an access token at [huggingface.co/settings/tokens](https://huggingface.co/settings/tokens) (read access is sufficient).
4. Paste the token into **Settings → TTS → Pocket-TTS → HuggingFace Token**, or set it via the `HF_TOKEN` environment variable before launching VoxCtrl.
5. Pick a voice and click **Download** in Settings → TTS. The model weights, tokenizer, and the selected voice's reference clip are downloaded once and cached locally under `~/.cache/huggingface/hub/`.

### MCP Server (Claude Desktop / Cursor)
1. Enable in Settings → Engine → MCP Server
2. Configure your MCP client to connect to `/tmp/voxctrl-mcp.sock`

---

## Uninstalling

`scripts/uninstall.sh` reverts everything VoxCtrl's setup and runtime create —
the udev rule, your `input` group membership, the menu launcher and icon,
`~/.config/voxctrl/`, `~/.local/share/voxctrl/` (Whisper models, Piper engine
and voices), the WebKit profile dirs, and the Pocket-TTS entries in the
HuggingFace cache — returning the system to its pre-VoxCtrl state:

```bash
# From a clone of this repository:
./scripts/uninstall.sh              # interactive
./scripts/uninstall.sh --yes        # no prompts (keeps the .AppImage file)

# Or without cloning:
curl -fsSL https://raw.githubusercontent.com/JRufer/VoxCtrl/master/scripts/uninstall.sh | bash -s -- --yes
```

Optional flags: `--remove-appimage` also deletes the `.AppImage` file itself;
`--remove-packages` also removes the host packages the installer added
(`wtype`, `xdotool`, `wl-clipboard`, `xclip`, `portaudio`, `espeak-ng`) —
opt-in because other software may use them. Log out and back in afterwards for
the `input` group removal to take effect.

---

## Building from Source

See [Development Guide](./development.md).

---

## Troubleshooting

### Hotkeys not working
VoxCtrl tells you about this itself: the tray entry reads **⚠️ Finish setup —
hotkeys inactive**, and a notification appears when it cannot read your
keyboard. Open the setup window from the tray; its first step names the cause.

To check by hand:
- `ls -l /dev/input/event*` — VoxCtrl needs read access to at least one of these
- `getfacl /dev/input/event0 | grep $USER` — confirms the `uaccess` ACL applied
- `groups | grep input` — the fallback path
- If the rule predates this VoxCtrl version, re-run the setup: older rules only
  granted access through the `input` group, which needs a fresh login.

### Permissions screen keeps reappearing (fresh Arch / CachyOS installs)
The setup window reappears while global hotkeys still do not work:
1. Click **Grant keyboard access** and enter your password. This writes the
   udev rule and reloads it *first*; the optional host package installation
   runs afterwards and is allowed to fail (stale pacman mirrors on a freshly
   installed system are common — run `sudo pacman -Syu` later to fix them).
2. That is normally the end of it. If the window still shows the step as
   unfinished, it offers **Restart VoxCtrl to finish** — one click, no logout.
3. Only if that button is absent does the window ask you to log out, and it
   says so plainly.

If `pkexec` is unavailable, the window's **Set it up manually** link shows the
exact commands to paste into a terminal.

### Hotkey records but no text is typed (and no overlay)
- Download a **Whisper model** first: Settings → Engine → Download. On a fresh
  install no model is present; VoxCtrl now shows a notification when you press
  a dictation hotkey without one.
- The default "Dictate (Hold)" gesture requires the combo to be **held** ~200ms
  before recording starts — a very quick tap is ignored by design.
- If a "no microphone audio is arriving" notification appears, pick a working
  input device in Settings → Audio.

### TTS engines refuse to play
- **Piper**: download a voice in Settings → TTS first — this also installs the
  standalone Piper engine into `~/.local/share/voxctrl/piper/`.
- **eSpeak-NG**: requires the system package (`sudo pacman -S espeak-ng` /
  `sudo apt install espeak-ng`).
- **Pocket-TTS**: requires a one-time model download (and a HuggingFace token —
  see above).
- Failures now surface directly in Settings → TTS next to the Test button.

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

