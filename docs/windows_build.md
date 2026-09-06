# Building VoxCtrl on Windows

## Prerequisites

### Required Tools

| Tool | Version | Download |
|---|---|---|
| Rust (via rustup) | 1.75+ | https://rustup.rs/ |
| Node.js | 18+ | https://nodejs.org/ |
| Visual Studio Build Tools | 2019+ | https://visualstudio.microsoft.com/visual-cpp-build-tools/ |
| WebView2 Runtime | Any | Pre-installed on Windows 10 21H2+ and Windows 11 |

### Visual Studio Build Tools

During installation, select the **"Desktop development with C++"** workload. This provides MSVC, the Windows SDK, and the linker required by Rust.

After installation, run builds from a **Visual Studio Developer Command Prompt** or ensure `cl.exe` is on your PATH. The easiest way to ensure this is to install and use `rustup` with the default `stable-x86_64-pc-windows-msvc` toolchain.

### Tauri CLI

```powershell
cargo install tauri-cli
```

### Node dependencies

```powershell
npm install
```

---

## Development Build

```powershell
npm run tauri dev
```

This starts the Vite dev server and compiles the Rust backend in debug mode. Hot-reload is active for Svelte changes; Rust changes require a recompile (~5–30s).

---

## Production Build

### Standard build (no GPU acceleration)

```powershell
npm run tauri build
```

Output artifacts land in `src-tauri\target\release\bundle\`:
- `nsis\VoxCtrl_<version>_x64-setup.exe` — NSIS installer
- `msi\VoxCtrl_<version>_x64.msi` — MSI package

### Build with CUDA acceleration

If you have an NVIDIA GPU and the CUDA Toolkit installed (11.x or 12.x), you can enable GPU-accelerated inference:

```powershell
# Set CUDA path if needed (adjust to your installed version)
$env:CUDA_PATH = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.3"
$env:CUDA_COMPUTE_CAP = "86"   # Set to your GPU's compute capability

npm run tauri build -- --features cuda
```

Without the `cuda` feature flag, Whisper inference runs on the CPU. The `cuda`
feature is opt-in and never required.

**There is deliberately no Vulkan build for Windows.** whisper.cpp's Vulkan
backend fails to register on Windows MSVC static builds — which is exactly what
`whisper-rs-sys` produces for Rust — and falls back to the CPU while reporting
that it found a GPU ([whisper.cpp#3750](https://github.com/ggml-org/whisper.cpp/issues/3750),
open against the `whisper-rs` version this workspace pins). A GPU build that is
silently a CPU build is worse than no GPU build, so the released Windows
installer is CPU-only until either that is fixed or the DirectML execution
provider lands.

### Using the build script

A PowerShell helper script automates prerequisite checks and the build:

```powershell
# Standard build
.\scripts\build_windows.ps1

# With CUDA
.\scripts\build_windows.ps1 -Cuda

# Debug build
.\scripts\build_windows.ps1 -Debug
```

---

## Whisper Models

Place `.bin` model files in `%LOCALAPPDATA%\voxctrl\models\` (created on first run).

Download a model manually:

```powershell
$model = "ggml-large-v3.bin"
$url   = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$model"
$dest  = "$env:LOCALAPPDATA\voxctrl\models\$model"
New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
Invoke-WebRequest $url -OutFile $dest
```

Supported sizes: `tiny`, `base`, `small`, `medium`, `large-v3`, `large-v3-turbo`.

---

## Piper TTS (optional)

Piper is the default TTS engine, and on Windows it is the one thing VoxCtrl
cannot install for you yet. Download the Windows release and place the binary at:

```
%LOCALAPPDATA%\voxctrl\piper\piper.exe
```

Download from: https://github.com/rhasspy/piper/releases

Voice models go in `%LOCALAPPDATA%\voxctrl\piper-voices\`. The Settings UI has a
download button for each supported voice, and those *do* work on Windows — it is
only the engine binary that has to be placed by hand.

**Or pick an engine that needs nothing.** Settings → Text-to-Speech offers
Pocket-TTS and Breeze-TTS-2, which are pure Rust plus ONNX and work out of the box.
eSpeak-NG and Inflect-Micro both shell out to `espeak-ng`, which is not on a stock
Windows machine, so they need it installed and on `PATH` first.

---

## Text Injection

VoxCtrl types dictated text with the Win32 `SendInput` API, one `KEYEVENTF_UNICODE`
event per UTF-16 code unit (see `crates/voxctrl-winput`). The character travels as
itself, so no keyboard layout is consulted and no escaping layer can misread it.

Transcriptions longer than 2000 characters go via the clipboard and a synthesised
Ctrl+V instead, because the receiving application processes one message per
character and a long paragraph visibly crawls into editors that do syntax work per
keystroke. The previous clipboard contents are restored afterwards.

Every synthesised event carries a marker in `dwExtraInfo` so VoxCtrl's own
keyboard hook ignores it. Without that, dictating text that completes a shortcut
would re-trigger that shortcut from VoxCtrl's own output.

> **Previously:** this path shelled out to PowerShell and called
> `SendKeys::SendWait`. The payload was base64-encoded so that no shell
> metacharacter could escape the string — a real defence, and it worked — but
> `SendKeys` then applied *its own* escaping to the decoded text, in which
> `+ ^ % ~ ( ) { } [ ]` are syntax. "50% (a+b)" arrived as "50" plus two stray
> chords and "array[0]" as "array0", so any dictation containing ordinary
> punctuation came out wrong.

### If nothing is typed

`SendInput` cannot deliver into a window owned by a more-privileged process
(Windows calls this UIPI). If dictation works everywhere except one application,
that application is almost certainly running elevated; VoxCtrl has to be elevated
too to type into it. VoxCtrl reports this rather than failing silently.

---

## Global Shortcuts

Shortcuts arrive through a Win32 low-level keyboard hook
(`crates/voxctrl-hotkeys/src/windows/`). Keys are identified by **scan code**,
not virtual-key code, because scan codes are physical positions: a shortcut
recorded on the key left of `S` fires from that key whatever the layout calls it,
which is what `KeyboardEvent.code` in the settings UI records. The mapping from
scan code to VoxCtrl's key names lives in `crates/voxctrl-hotkeys/src/win_keys.rs`
and is deliberately outside the platform gate so its tests run on every platform.

All four gesture styles work — hold, toggle, double-tap, double-tap-hold — the
same as on Linux.

Two limits are inherent to the mechanism and cannot be worked around:

- Windows does not deliver keys to the hook while an **elevated application** has
  focus, unless VoxCtrl is elevated too.
- The hook never sees the **secure desktop** — the UAC prompt, the lock screen,
  Ctrl+Alt+Del — so shortcuts do not fire there.

The non-modifier key that *completes* a shortcut is swallowed, so binding
`Super+Space` does not also open Windows Search. Modifiers are never swallowed:
eating a bare Ctrl or Super would break every other shortcut on the machine.

### Privacy

A low-level keyboard hook is called for **every keystroke on the machine** — the
same exposure as the evdev and X11 backends on Linux, and unlike the XDG portal,
where the compositor owns the grab and VoxCtrl is told only that its own shortcut
fired. The Hotkeys tab says so plainly on Windows and does not show the padlock
it shows for the portal. Keys are matched against your shortcuts and discarded;
nothing is stored, logged, or sent anywhere.

---

## Code Signing (optional, recommended for distribution)

Without a code signing certificate, Windows SmartScreen will display an "Unknown Publisher" warning when users run the installer.

To sign your build:

1. Obtain an Authenticode certificate (EV certificate eliminates SmartScreen entirely; standard OV certificates reduce warnings after enough users install the app).
2. Set the following environment variables before building:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY    = "path\to\key.pem"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "your-passphrase"
```

See the [Tauri signing docs](https://tauri.app/distribute/sign/windows/) for full details.

---

## Troubleshooting

### `error: linker 'link.exe' not found`
MSVC linker is missing. Install Visual Studio Build Tools with the C++ workload, or run `rustup target add x86_64-pc-windows-msvc`.

### `error[E0463]: can't find crate for 'std'`
Wrong target selected. Ensure `rustup default stable-x86_64-pc-windows-msvc`.

### WebView2 missing
Download the WebView2 Evergreen Bootstrapper from Microsoft and run it before launching the app. On Windows 11 and Windows 10 21H2+, WebView2 ships with the OS.

### CUDA build fails
- Confirm `CUDA_PATH` points to an installed CUDA Toolkit.
- Ensure Visual Studio Build Tools are installed (whisper-rs compiles CUDA kernels with MSVC).
- Try without `--features cuda` to rule out a non-CUDA issue first.

### Audio device not detected
VoxCtrl uses WASAPI via `cpal`. If no microphone is listed, check Windows privacy settings: **Settings → Privacy & security → Microphone → Allow apps to access your microphone**.
