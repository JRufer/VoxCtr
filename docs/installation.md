# Installation & Setup

## System Requirements

### Linux
- **OS:** Any modern Linux distro — Ubuntu 22.04+, Linux Mint 21+, Debian 12+,
  Fedora 36+, Arch, openSUSE Tumbleweed
- **glibc 2.35 or newer**, with a libstdc++ from GCC 12 or newer. The AppImage
  is built on Ubuntu 22.04, so that is the floor: Ubuntu 20.04 / Mint 20 /
  Debian 11 / RHEL 9 are too old and the AppImage will not start on them.
- **Display:** X11 or Wayland
- **Audio:** PulseAudio or PipeWire (ALSA fallback supported)
- **Required packages:** `libwebkit2gtk-4.1`, `libayatana-appindicator3` or `libappindicator3`
- **Optional:** `wtype` (Wayland injection), `xdotool` (X11 injection)
- **No `libfuse2` needed.** The AppImage ships a runtime that uses your
  system's FUSE 3, and extracts and runs itself when FUSE is unavailable
  entirely — so it starts on a stock Ubuntu 22.04 / Mint 21 desktop with
  nothing installed first. (Releases up to and including 0.3.7 used a runtime
  that needed `libfuse2`; on those, either `sudo apt install libfuse2` or run
  the AppImage with `--appimage-extract-and-run`.)
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

# Run it. That is the whole installation.
./VoxCtrl-x86_64.AppImage
```

### Just run it

There is no required install step. Global shortcuts need no permissions, and on
every Linux launch VoxCtrl writes its own desktop entry and icon into
`~/.local/share/` — no privileges needed for either:

- `~/.local/share/applications/ai.voxctrl.app.desktop` (menu launcher; the
  filename matches the application id VoxCtrl declares to the desktop portal, so
  your desktop's shortcut settings show "VoxCtrl" and its icon rather than a
  bare identifier)
- `~/.local/share/icons/hicolor/128x128/apps/voxctrl.png`

An entry from before that rename (`voxctrl.desktop`) is removed at the same
time, so the application menu does not end up with two VoxCtrls.

### The one thing that may need a package manager

Typing a transcription into another window uses `wtype` (Wayland) or `xdotool`
(X11). Those are host packages, so installing them needs administrator rights —
the one part VoxCtrl cannot do for itself.

You do not need `--install` for it. Launch the AppImage; if a helper is missing,
the setup window says so and offers **Install it** (via `pkexec`), or **Install
it manually** for the exact command to paste into a terminal.

`./VoxCtrl-x86_64.AppImage --install` remains available if you would rather do
that step up front from a terminal. It installs those packages and nothing else
— see [Privacy & Security](privacy.md#what-the-installer-touches).

### Global shortcuts need no setup at all

There is **no keyboard permission step**, because VoxCtrl does not read your
keyboard. Global shortcuts are registered with your desktop through the XDG
`GlobalShortcuts` portal; your desktop owns the key grab and tells VoxCtrl only
that its own shortcut fired.

That means:

- No udev rule. No `input` group. No logout, no reboot, no relaunch.
- Nothing to undo later, and no change to your machine's security posture.
- VoxCtrl cannot see what you type in any other application — it is never given
  the data, rather than choosing not to look at it.

Earlier versions of VoxCtrl installed a udev rule granting read access to every
`/dev/input/event*` device. **It no longer does, and never will** — that rule
lets every program running as your user read every keystroke on the system, not
just VoxCtrl. See [Hotkeys → Why this changed](hotkeys.md#why-this-changed) for
the full reasoning.

The administrator prompt during setup is for installing packages only.

### If your desktop has no shortcuts portal

The portal is implemented by KDE Plasma (5.27+), GNOME 48+, and Hyprland. If
yours does not implement it, VoxCtrl **tells you at launch** and explains the
options. It will not grant itself keyboard access to work around it.

Your choices in that situation:

- Use a desktop that implements the portal — shortcuts then work with no
  permissions at all.
- Start and stop dictation from the tray menu or the D-Bus API instead.
- Grant input access yourself, knowingly, and accept that it applies to every
  program you run — see [Hotkeys → evdev fallback](hotkeys.md#linux--evdev-fallback).
  VoxCtrl will use that access if it already exists, but will never create it.

### Setup is verified, not assumed

The setup window reports what is actually true: which mechanism is delivering
shortcuts, exactly which keys your desktop bound (which may differ from what you
asked for — your desktop gets the final say), and whether the
keystroke-injection helper (`wtype`/`xdotool`) is present. It polls while open,
so a change made elsewhere flips it to green without reopening anything.

---

## Permissions Setup (Linux)

### Global Hotkeys

Nothing to do. Shortcuts go through the desktop portal, which requires no
permissions. If your desktop does not provide the portal, VoxCtrl says so at
launch — see above.

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

On a machine with no `~/.config/voxctrl/config.json`, VoxCtrl will:
1. Create `~/.config/voxctrl/` with default `config.json`, `targets.toml`, and `bindings.toml`
2. Create `~/.local/share/voxctrl/` for model and voice storage
3. Open the **setup wizard**, and nothing else

The wizard covers everything needed to dictate, in seven steps:

1. **Welcome** — what the remaining steps will ask
2. **Engine** — whisper.cpp or Moonshine, and a model size. Continuing downloads
   the model and waits for it, so the later test has something to transcribe
3. **Hotkey** — a gesture and a key combination, registered with your desktop.
   Only gestures your shortcut backend can actually deliver are offered
4. **Overlay** — which on-screen indicator to show while the mic is live, or none
5. **Test** — a real dictation into a box on screen, using the hotkey you just bound
6. **Voice** — optionally enable speech output and download an engine
7. **Done** — a summary, plus anything that failed and the error behind it

Every choice is written to the config as it is made, so quitting the wizard
halfway keeps what you picked. If something fails — a model that will not
download, a shortcut your desktop refuses — the last screen says so with the
underlying error and a copyable diagnostics report, rather than reporting the
app as ready.

The first hotkey is bound to a target named "Command", which types into the
focused window exactly like `inject` until you add a second target, at which
point voice command routing works without re-binding anything.

### Running it again

The wizard is not only for first launch:

```bash
voxctrl --setup
```

Works whether or not VoxCtrl is already running, and `--wizard`,
`--setup-wizard` and `--first-run` do the same thing. Settings → General has an
**Open setup wizard** button for the same purpose. A re-run always starts at
step one.

### Skipping it

"Skip setup" on the first screen closes the wizard and leaves the app running
with its defaults. Everything the wizard asks is also in Settings: Engine for
the model, Hotkeys for bindings, Visual for the overlay, TTS for speech output.

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
4. Paste the token into **Settings → TTS → HuggingFace access token** (or the setup wizard's voice step — it is one token for every gated model), or export `HF_TOKEN` before launching VoxCtrl. An exported token wins over the saved one and is never written to the config; the fields show it read-only when it is set.
5. Pick a voice and click **Download** in Settings → TTS. The model weights, tokenizer, and the selected voice's reference clip are downloaded once and cached locally under `~/.cache/huggingface/hub/`.

### MCP Server (Claude Desktop / Cursor)
1. Enable in Settings → Engine → MCP Server
2. Configure your MCP client to connect to `/tmp/voxctrl-mcp.sock`

---

## Updating

VoxCtrl checks GitHub for a newer release about ten seconds after it starts. If
there is one, a window opens with the release notes and three choices: **Update
and restart**, **Skip this version** (never asked about that release again; a
later one still gets offered), or **Not now** (asked again next launch).

Choosing to update downloads the release file that matches this installation —
the CPU AppImage, the Vulkan AppImage, or the Windows installer — checks it
against the SHA-256 checksum GitHub published for it, replaces the running
application file, and restarts into the new version. Your config, models and
voices live elsewhere and are untouched. If anything fails at any point, the
version you are running is left exactly as it was.

The application file keeps its exact path, so desktop entries, dock pins and
shell aliases go on working — which does mean an AppImage whose file name
carries a version number keeps the old number in its name while containing the
new build. Settings → About reports the version that is actually running.

A few installations cannot update themselves, and say so instead of offering a
button that would not work:

- **A `.deb` or distro package** — the package manager owns those files. Update
  it the way you installed it.
- **A build from source** — nothing to replace; rebuild.
- **An AppImage in a directory you cannot write to** (`/opt`, a read-only mount)
  — move it somewhere you own, or download the new release by hand. This is
  detected *before* the download starts, not after.

To check on demand, or to turn the automatic check off entirely: **Settings →
General → Updates**. With it off, VoxCtrl makes no network request unless you
press "Check now". What the check sends — nothing that identifies you — is
spelled out in [privacy.md](privacy.md#network).

---

## Uninstalling

`scripts/uninstall.sh` reverts everything VoxCtrl's setup and runtime create —
the menu launcher and icon, `~/.config/voxctrl/`, `~/.local/share/voxctrl/`
(Whisper models, Piper engine and voices), the WebKit profile dirs, and the
Pocket-TTS entries in the HuggingFace cache — returning the system to its
pre-VoxCtrl state.

It also removes the udev rule and `input` group membership that **older**
versions of VoxCtrl installed. Current versions create neither, so on a fresh
install there is nothing there to remove; the step exists to clean up after an
upgrade.

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
hotkeys inactive**, and the setup window's first step names the cause. Open it
from the tray.

The usual causes, in order:

1. **On KDE, the shortcuts are registered but not enabled.** This is the most
   common cause on KDE Plasma: an upstream bug
   ([#483639](https://bugs.kde.org/show_bug.cgi?id=483639)) leaves
   portal-registered shortcuts unticked in System Settings. Settings →
   Hotkeys shows a notice with an **Open Shortcut Settings** button when this
   applies — see
   [Hotkeys → KDE registers shortcuts disabled by default](hotkeys.md#kde-registers-shortcuts-disabled-by-default).
2. **Your desktop has no global-shortcuts portal.** The setup window says so
   explicitly. Supported: KDE Plasma 5.27+, GNOME 48+, Hyprland. Not supported:
   Sway and most other wlroots compositors. VoxCtrl will not grant itself
   keyboard access to work around this.
3. **Your desktop refused or reassigned the shortcut.** Settings → Hotkeys shows
   the keys your desktop actually bound next to each binding, and flags any it
   would not accept. Bindings on a bare modifier (a lone Super, say) usually
   have to be chosen in your desktop's own shortcut settings, because a lone
   modifier is not a valid accelerator.
4. **`xdg-desktop-portal` is not running.** Check with
   `systemctl --user status xdg-desktop-portal`, and confirm the interface is
   present:

   ```bash
   busctl --user introspect org.freedesktop.portal.Desktop \
     /org/freedesktop/portal/desktop | grep GlobalShortcuts
   ```

### Setup window keeps reappearing

The setup window opens when something genuinely stops dictation working
end to end. Its steps say which. The two it cannot fix for you:

- **No global-shortcuts portal** — see above. This is a property of your
  desktop, not of the install.
- **Host packages failed to install** — common on a freshly installed rolling
  distro with stale mirrors. Run `sudo pacman -Syu` (or your distro's
  equivalent) and use **Install it** again, or **Install it manually** for the
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
- `Error: No suitable fusermount binary found on the $PATH` — an older release
  (0.3.7 or earlier), whose runtime needed FUSE 2. Either run it with
  `./VoxCtrl.AppImage --appimage-extract-and-run`, install FUSE 2
  (`sudo apt install libfuse2`), or download a newer release, whose runtime
  uses FUSE 3 and needs nothing installed.
- `version 'GLIBC_2.35' not found` or `GLIBCXX_3.4.30 not found` — the distro
  is older than the AppImage's baseline (see [System Requirements](#linux)).
  Ubuntu 20.04, Mint 20, Debian 11 and RHEL 9 cannot run it; build from source
  on those.
- `error while loading shared libraries: <name>` — a library your desktop is
  missing. `libEGL.so.1`/`libGL.so.1` come from your graphics drivers
  (`libegl1`, `libgl1` on Debian/Ubuntu); `libgtk-3.so.0`, `libglib-2.0.so.0`
  and the GStreamer libraries come with any GTK desktop.
- Or extract and run directly: `./VoxCtrl.AppImage --appimage-extract && squashfs-root/AppRun`

### Debugging & Crash Logs
If the application crashes, fails to launch, or encounters hardware/model errors, you can check the local startup and error log file:
- **Location:** `~/.local/share/voxctrl/startup_errors.log`
- **Privacy:** To protect your privacy, this file **never** records or contains any transcribed speech text or LLM prompts. It only logs system configurations (models loaded, input devices, sample rates) and application/compiler errors.
- **Submitting Reports:** Please attach this log file when opening issues or submitting crash reports on GitHub to help us diagnose the problem.

