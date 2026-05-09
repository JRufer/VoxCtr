# VoxCtr Windows Port — Technical Plan

## Table of Contents

1. [Tech Stack Inventory & Windows Availability](#1-tech-stack-inventory--windows-availability)
2. [Component-by-Component Replacement Map](#2-component-by-component-replacement-map)
3. [New Windows-Specific Packages Required](#3-new-windows-specific-packages-required)
4. [Technical Hurdles & Mitigations](#4-technical-hurdles--mitigations)
5. [Full Port Plan](#5-full-port-plan)
6. [Cross-Platform Sync Strategy](#6-cross-platform-sync-strategy)
7. [Summary](#7-summary)

---

## 1. Tech Stack Inventory & Windows Availability

### Python Core — No Changes Required

| Component | Package | Version | Windows Status |
|-----------|---------|---------|----------------|
| GUI | PyQt6 | 6.10.2 | **Available** — official Windows wheels on PyPI |
| Transcription | faster-whisper | 1.2.1 | **Available** — CUDA wheels on PyPI; CPU-only fallback works |
| Transcription | ctranslate2 | 4.7.1 | **Available** — Windows wheels on PyPI |
| Audio I/O | PyAudio | 0.2.14 | **Conditionally available** — unofficial wheels via `pipwin` or `conda`; requires PortAudio DLL |
| Audio I/O | sounddevice | 0.5.5 | **Available** — ships its own PortAudio DLL on Windows |
| Numeric | numpy / scipy | latest | **Available** — first-class Windows support |
| Noise reduction | noisereduce | 3.0.3 | **Available** — pure Python + numpy |
| AI/LLM | mcp, websockets | ≥1.0 | **Available** — pure Python |
| Config | tomllib / tomli | stdlib / pip | **Available** |
| Clipboard | pyperclip | in use | **Available** — uses `clip.exe` / `ctypes` on Windows |

### Linux-Specific Packages — Must Replace

| Current (Linux) | Purpose | Windows Replacement | Effort |
|----------------|---------|-------------------|--------|
| `evdev` 1.9.3 | Hotkeys via `/dev/input/*` | `pynput` (keyboard hook) | HIGH |
| `dbus-python` 1.4.0 | D-Bus IPC service | Named-pipe IPC via `pywin32` | HIGH |
| `pyatspi` (system pkg) | AT-SPI2 text context | `comtypes` + UIAutomation | MEDIUM |

### External Binaries — Must Replace or Remove

| Binary | Purpose | Windows Alternative |
|--------|---------|-------------------|
| `wtype` / `xdotool` | Text injection | `SendInput` via `ctypes.windll.user32` |
| `wl-copy` / `xclip` | Clipboard | `clip.exe` / `pyperclip` (already present) |
| `aplay` | Audio playback | `sounddevice.play()` or `winsound.PlaySound` |
| `piper` binary | TTS engine | rhasspy ships a `piper.exe` — drop-in replacement |
| `espeak-ng` | TTS fallback | Official Windows installer available |
| `socat` | MCP stdio bridge | Replace with TCP `localhost` socket |
| `whisper.cpp` | AMD/Intel GPU backend | Windows build available; skip for v1 |
| `nvidia-smi` | GPU detection | NVML via `pynvml` (same pip package) |
| `vulkaninfo` | GPU detection | Vulkan SDK ships same binary on Windows |

---

## 2. Component-by-Component Replacement Map

### 2.1 Input / Hotkeys — `src/input_listener.py`

**Current:** reads raw events from `/dev/input/event*` via `evdev`.

**Windows replacement:** `pynput.keyboard.Listener` (uses `SetWindowsHookEx(WH_KEYBOARD_LL)` internally). `pynput` is cross-platform, so a single abstraction layer wraps both backends cleanly.

**New structure:**

```
src/platform/
    __init__.py
    linux_input.py      ← current evdev code, moved here
    windows_input.py    ← pynput-based implementation
```

`input_listener.py` becomes a thin dispatcher:

```python
import sys
if sys.platform == "win32":
    from platform.windows_input import InputListener
else:
    from platform.linux_input import InputListener
```

**Notable limitation:** `pynput` on Windows requires the script to run from the main thread and cannot suppress media keys without UAC elevation. Hold-to-talk and push-to-talk work without elevation. Suppressing keys in UAC-elevated apps or games requires running VoxCtr elevated or accepting that suppression silently fails.

---

### 2.2 Text Injection — `src/text_injector.py` + `src/portal_injector.py`

**Current:** `wtype` → Wayland; `xdotool` → X11; AT-SPI2 for context.

**Windows replacement:**

- **Primary:** `SendInput` via `ctypes.windll.user32` — unicode-aware, works with most apps.
- **Context reading:** `comtypes` + `IUIAutomation.GetFocusedElement()` to read the focused element's text (functional equivalent of AT-SPI2).
- **Clipboard+paste fallback:** already present via `pyperclip`; promoted to first fallback when `SendInput` fails.

```
src/platform/
    windows_injector.py    ← SendInput implementation
    windows_context.py     ← UIAutomation context reader
```

`SendInput` handles all Unicode natively via `KEYEVENTF_UNICODE`. Games with anti-cheat that block `SendInput` require the clipboard fallback — this is a documented limitation.

---

### 2.3 D-Bus Service — `src/dbus_service.py`

**Current:** exposes `ai.voxctl.Dictation` service on the session bus for external start/stop/status control.

**Windows replacement:** D-Bus does not exist on Windows. Replace with a named-pipe server (`\\.\pipe\voxctl-control`) using Python's `multiprocessing.connection` or raw `win32pipe` from `pywin32`. This gives external tools an equivalent control surface.

**MCP server (`src/mcp_server.py`):** switch from Unix domain socket (`/tmp/voxctl-mcp.sock`) to a TCP socket on a fixed port (`127.0.0.1:9847`). This change is platform-neutral — it is an improvement for Linux as well (removes the `socat` bridge requirement).

---

### 2.4 GPU Detection — `src/backends/selector.py`

**Current:** reads `/sys/class/drm/card*/device/vendor`; calls `nvidia-smi`; calls `vulkaninfo`.

**Windows replacement:**

- **NVIDIA:** `pynvml.nvmlInit()` — same pip package, works identically on Windows.
- **AMD/Intel:** `wmi` Python package or `ctypes` + DXGI `IDXGIFactory` COM interface.
- **Vulkan:** `vulkaninfo` binary is available in the Windows Vulkan SDK — the same subprocess call works unchanged.

A new `src/platform/windows_gpu.py` mirrors the Linux `_sysfs_gpu_vendor` interface so `selector.py` requires no structural changes.

---

### 2.5 Audio Output — Piper / espeak-ng

**Current:** Piper binary pipes raw PCM → `aplay` (ALSA).

**Windows replacement:** introduce `src/platform/audio_output.py`:

```python
import sys, sounddevice as sd, numpy as np

def play_pcm(raw_bytes: bytes, samplerate: int = 22050) -> None:
    if sys.platform == "win32":
        audio = np.frombuffer(raw_bytes, dtype=np.int16)
        sd.play(audio, samplerate=samplerate, blocking=True)
    else:
        # existing aplay subprocess call
        ...
```

`sounddevice` is already in `requirements.txt` and works identically on Windows. Piper's `--output_raw` flag and `espeak-ng` CLI flags are unchanged between platforms.

---

### 2.6 Config and Data Paths

**Current:** XDG base dirs — `~/.config/voxctl/`, `~/.local/share/voxctl/`, `/tmp/voxctl-*`.

**Windows replacement:** use `platformdirs` to resolve paths automatically.

```python
# src/paths.py (new unified file)
from platformdirs import user_config_dir, user_data_dir, user_cache_dir
from pathlib import Path
import tempfile, sys

CONFIG_DIR = Path(user_config_dir("voxctl"))
DATA_DIR   = Path(user_data_dir("voxctl"))
TEMP_DIR   = Path(tempfile.gettempdir()) / "voxctl"
```

On Windows this resolves to:

- Config → `%APPDATA%\voxctl\`
- Data → `%LOCALAPPDATA%\voxctl\`
- Temp → `%TEMP%\voxctl\`

All hardcoded `~/.config/voxctl` strings across the codebase are replaced with `paths.CONFIG_DIR`. This is a single-pass mechanical refactor.

---

### 2.7 System Tray — `src/gui/tray_icon.py`

PyQt6's `QSystemTrayIcon` works on Windows without any changes. The Windows tray shows the same icon, right-click context menu, and notifications via `QSystemTrayIcon.showMessage`. **No work required.**

---

### 2.8 Setup / Permissions Dialog — `src/gui/setup_dialog.py`

**Current:** checks `/dev/input` group membership; offers to run `setup-permissions.sh`.

**Windows replacement:** no udev, no groups. The Windows setup dialog checks:

1. Is `pynput` installed and the keyboard hook registerable?
2. Can the app write to `%APPDATA%\voxctl\`?
3. Is a working audio input device present?

This is a simpler wizard — roughly half the code of the Linux version.

---

### 2.9 Named Pipes / FIFO — `src/routing/targets.py`

**Current:** creates Linux named FIFOs via `os.mkfifo`.

**Windows:** `os.mkfifo` does not exist. Replace with a Windows named pipe (`\\.\pipe\voxctl-<name>`) using `win32pipe`, or fall back to a local TCP socket. The `pipe` delivery type maps transparently to the appropriate mechanism per platform via a factory in `src/platform/__init__.py`.

---

## 3. New Windows-Specific Packages Required

```
# requirements-windows.txt (additions over requirements-common.txt)
pynput>=1.7.7          # global hotkeys — replaces evdev
pynvml>=11.5.0         # NVIDIA GPU detection — replaces sysfs reads
wmi>=1.5.1             # AMD/Intel GPU & system info via WMI
comtypes>=1.4.1        # UIAutomation COM bindings — replaces pyatspi
platformdirs>=4.2.0    # cross-platform config/data paths
pywin32>=308           # Win32 API access: named pipes, SendInput helpers
```

`dbus-python` and `evdev` are **removed** from `requirements-windows.txt`. `pyatspi` is not installed on Windows at all.

---

## 4. Technical Hurdles & Mitigations

### Hurdle 1: `SendInput` blocked by elevated processes

Windows prevents a lower-privilege process from injecting keystrokes into a higher-privilege window. If the target app (IDE, browser) is running elevated, text injection silently fails.

**Mitigation:** detect the failure (no exception is raised; `SendInput` returns 0 and `GetLastError` returns `ERROR_ACCESS_DENIED`), log a warning, and automatically fall back to clipboard+paste. Optionally offer to relaunch VoxCtr elevated on first failure (UAC prompt stored in app manifest).

---

### Hurdle 2: `pynput` and antivirus false positives

Low-level keyboard hooks (`WH_KEYBOARD_LL`) are flagged by some AV products (Windows Defender, Malwarebytes) as potential keyloggers.

**Mitigation:** code-sign the installer and the bundled Python process with an EV (Extended Validation) certificate. Provide an AV exclusion guide in the README. This is a distribution problem, not a code problem — the hook itself is legitimate.

---

### Hurdle 3: `faster-whisper` CUDA on Windows

CUDA 12.x is supported on Windows but requires specific cuDNN DLL versions on `PATH`. `ctranslate2` wheels on PyPI are not always built against the latest CUDA release, causing silent fallback to CPU.

**Mitigation:** ship a `scripts\check_cuda.py` diagnostic script that validates CUDA availability and prints a clear error with remediation steps. For CPU-only users, the standard pip wheel works without any CUDA setup. Document this clearly in the installer.

---

### Hurdle 4: PyAudio install friction

`PyAudio` requires a native build on Windows. The official PyPI wheel is often stale or unavailable for newer Python versions.

**Mitigation:** promote `sounddevice` to the primary audio I/O layer — it ships its own PortAudio DLL and installs cleanly via pip. Keep `PyAudio` as an optional fallback. `sounddevice` is already in `requirements.txt`, so no new dependency is added.

---

### Hurdle 5: `whisper.cpp` Vulkan backend on Windows

The `whisper.cpp` Vulkan backend must be compiled with MSVC or MinGW. Pre-built Windows binaries exist on the project's GitHub releases but are not always current or tested.

**Mitigation:** for v1, support only the `faster-whisper` (NVIDIA/CPU) backend on Windows. Document Vulkan/AMD support as "planned for v2." This significantly reduces build complexity for the first release without affecting most users.

---

### Hurdle 6: Piper TTS and `aplay`

Current code shells out to `aplay` for audio playback after Piper generates raw PCM. `aplay` does not exist on Windows.

**Mitigation:** introduce `src/platform/audio_output.py` (see §2.5). Linux continues to use `aplay`; Windows uses `sounddevice.play()`. Piper's raw 16-bit LE PCM output format is handled identically by both paths.

---

### Hurdle 7: Unix domain socket for MCP

Claude Desktop's MCP transport expects either stdio or a socket path. Windows does not support `AF_UNIX` sockets reliably across all configurations (available in Win10 1803+ but not universally stable).

**Mitigation:** switch the MCP server from a Unix domain socket to TCP (`127.0.0.1:9847`). Update the Claude Desktop config snippet in the README for both platforms. This also eliminates the `socat` bridge requirement on Linux — a net improvement for everyone.

---

### Hurdle 8: Path separator assumptions

Several places in the codebase likely use hardcoded `/` separators or bare string concatenation that assumes POSIX paths.

**Mitigation:** audit all path construction during Phase 0 and replace bare string concatenation with `pathlib.Path`. This is a required cleanup pass and catches latent bugs regardless of the Windows port.

---

### Hurdle 9: Subprocess shell calls assume bash

`install.sh`, `build_appimage.sh`, and related scripts in `scripts/` are bash scripts that do not run on Windows.

**Mitigation:** write parallel PowerShell equivalents: `scripts\install.ps1` and `scripts\build_installer.ps1`. The bash scripts for Linux are left completely unchanged.

---

### Hurdle 10: Packaging format

AppImage is a Linux-only distribution format.

**Mitigation:** use PyInstaller to produce a single-folder distribution (`--onedir`), then package it with NSIS or the WiX Toolset to create a signed `.exe` or `.msi` installer. The PyInstaller `.spec` file handles bundling all wheels, the Piper binary, and espeak-ng DLLs.

---

## 5. Full Port Plan

### Phase 0 — Foundation (Week 1)

**Goal:** get the app to import and launch on Windows with no Linux-only imports crashing at startup.

- [ ] Create `src/platform/__init__.py` with a `PLATFORM = sys.platform` constant and platform-dispatching factory functions
- [ ] Wrap all Linux-only top-level imports in `if sys.platform != "win32":` guards so the app launches on Windows without immediately crashing
- [ ] Create `requirements-common.txt`, `requirements-linux.txt`, and `requirements-windows.txt`
- [ ] Add `platformdirs` and create `src/paths.py` for cross-platform config/data path resolution
- [ ] Replace all hardcoded `~/.config/voxctl` strings with `paths.CONFIG_DIR`
- [ ] Audit all path construction — replace bare string concatenation with `pathlib.Path` throughout
- [ ] Add a `windows-latest` runner to `.github/workflows/` CI matrix running `pytest` with mocked platform calls

**Exit criterion:** `python src/main.py` launches on Windows, even if most features show "not supported" placeholders.

---

### Phase 1 — Audio & Transcription (Week 2)

**Goal:** record audio and transcribe on Windows.

- [ ] Verify `sounddevice` device enumeration on Windows — works out of the box
- [ ] Verify `faster-whisper` CPU mode transcribes correctly on Windows — works out of the box
- [ ] Implement `src/platform/audio_output.py` with `play_pcm_linux` (aplay) and `play_pcm_windows` (sounddevice)
- [ ] Update `src/tts_engine.py` to call `audio_output.play_pcm()` instead of shelling out to `aplay`
- [ ] Test `piper.exe` on Windows with `--output_raw` — same flag, same behavior
- [ ] Write unit tests for audio output that mock `sounddevice.play`

**Exit criterion:** voice is recorded, transcribed, and TTS plays back on Windows.

---

### Phase 2 — Hotkeys (Week 3)

**Goal:** push-to-talk and toggle-to-talk work on Windows.

- [ ] Move current evdev code to `src/platform/linux_input.py`
- [ ] Create `src/platform/windows_input.py` using `pynput.keyboard.Listener`
- [ ] Make `src/input_listener.py` dispatch between the two via the platform factory
- [ ] Verify the hold/double-tap gesture state machine in `src/hotkeys/double_tap.py` — it is platform-neutral and requires no changes
- [ ] Handle the elevated-window injection failure: log warning + auto-fallback to clipboard
- [ ] Update the setup dialog's key-capture widget to work with `pynput` on Windows
- [ ] Write tests mocking `pynput` keyboard events

**Exit criterion:** hold-to-talk and toggle-to-talk work correctly in standard Windows applications.

---

### Phase 3 — Text Injection (Week 4)

**Goal:** transcribed text appears in the focused application on Windows.

- [ ] Create `src/platform/windows_injector.py` using `ctypes.windll.user32.SendInput` with `INPUT_KEYBOARD` + `KEYEVENTF_UNICODE`
- [ ] Create `src/platform/windows_context.py` using `comtypes` + `IUIAutomation.GetFocusedElement()` to read context text
- [ ] Update `src/text_injector.py` to dispatch to the platform injector via factory
- [ ] Verify the clipboard fallback path (`pyperclip` → `clip.exe`) works correctly
- [ ] Integration test across: Notepad, VS Code, browser address bar, Microsoft Word

**Exit criterion:** dictated text appears correctly in standard Windows applications.

---

### Phase 4 — IPC / MCP Server (Week 5)

**Goal:** the MCP server and external control interface work on Windows.

- [ ] Change MCP server transport from Unix socket to TCP (`127.0.0.1:9847`) — platform-neutral improvement
- [ ] Create `src/platform/windows_ipc.py` as a named-pipe control server replacing D-Bus
- [ ] Update `src/dbus_service.py` to dispatch to the platform IPC server via factory
- [ ] Update Claude Desktop integration instructions in the README for both platforms
- [ ] Replace `os.mkfifo` in the `pipe` routing target with a platform-aware wrapper
- [ ] Update `socat` bridge documentation — TCP replaces the Unix socket path

**Exit criterion:** Claude Desktop can invoke VoxCtr MCP tools on Windows.

---

### Phase 5 — GPU Detection & Backend Selection (Week 6)

**Goal:** the correct transcription backend is auto-selected on Windows.

- [ ] Create `src/platform/windows_gpu.py` using `pynvml` for NVIDIA and `wmi` for AMD/Intel
- [ ] Update `src/backends/selector.py` to dispatch between platform GPU detectors via factory
- [ ] Test the NVIDIA CUDA path on a Windows machine with a CUDA-capable GPU
- [ ] Verify CPU-only fallback path works for all GPU configurations
- [ ] Document v1 limitation: Vulkan/AMD backend not supported on Windows

**Exit criterion:** `selector.py` correctly identifies GPU vendor on Windows and selects the appropriate backend.

---

### Phase 6 — Installer & Packaging (Week 7)

**Goal:** a distributable Windows installer that works on a clean machine.

- [ ] Write a PyInstaller `.spec` file bundling all pip wheels, `piper.exe`, `espeak-ng`, and optional CUDA DLLs
- [ ] Write `scripts\build_installer.ps1` invoking PyInstaller and NSIS/WiX
- [ ] Write `scripts\install.ps1` for manual installs without a package manager
- [ ] Package with NSIS or WiX into a signed `.exe` / `.msi` installer
- [ ] Test clean install on a fresh Windows 10 VM and a fresh Windows 11 VM
- [ ] Write `scripts\check_cuda.py` diagnostic for CUDA troubleshooting

**Exit criterion:** one-click installer completes successfully on a clean Windows 10/11 machine with no manual steps.

---

### Phase 7 — Testing & Polish (Week 8)

**Goal:** all tests pass on Windows CI; all known limitations are documented.

- [ ] Update all tests that mock `evdev` / `dbus` to also mock `pynput` / named-pipe equivalents
- [ ] Add Windows-specific test cases for `SendInput`, UIAutomation context reading, and named-pipe IPC
- [ ] Add `@pytest.mark.skipif(sys.platform != "linux", ...)` to Linux-only test cases
- [ ] Update `README.md` with a Windows installation and usage section
- [ ] Create `docs/platform-matrix.md` with the initial feature parity table
- [ ] Fix any remaining path separator issues found during the test run
- [ ] Benchmark transcription latency on Windows vs. Linux (expected to be identical for CPU; CUDA path may vary)

**Exit criterion:** full `pytest` suite passes on `windows-latest` CI runner.

---

## 6. Cross-Platform Sync Strategy

The greatest long-term risk after the initial port is the two platforms drifting as new features are added only to Linux. The following strategy prevents that.

### Directory Structure

```
src/
    platform/
        __init__.py           ← PLATFORM constant + all factory functions
        linux_input.py        ← evdev implementation
        windows_input.py      ← pynput implementation
        linux_injector.py     ← wtype/xdotool implementation
        windows_injector.py   ← SendInput implementation
        linux_context.py      ← AT-SPI2 implementation
        windows_context.py    ← UIAutomation implementation
        linux_gpu.py          ← sysfs + vulkaninfo
        windows_gpu.py        ← pynvml + wmi + DXGI
        linux_ipc.py          ← D-Bus service + Unix socket
        windows_ipc.py        ← Named pipe + TCP socket
        audio_output.py       ← unified: aplay on Linux, sounddevice on Windows
    paths.py                  ← unified via platformdirs
    input_listener.py         ← thin dispatcher only, no platform logic
    text_injector.py          ← thin dispatcher only, no platform logic
    ... all other src files — platform-neutral, no sys.platform checks
```

**The invariant:** everything inside `src/platform/` is platform-specific; everything outside it is platform-neutral. A PR that touches any file outside `src/platform/` must not introduce any `sys.platform` checks — those belong inside the platform directory. Code reviewers should enforce this rule mechanically.

---

### Requirements Files

```
requirements-common.txt     ← PyQt6, faster-whisper, sounddevice, numpy, scipy,
                               noisereduce, mcp, websockets, tomli, pyperclip,
                               pynvml, platformdirs, ...

requirements-linux.txt      ← -r requirements-common.txt
                               evdev
                               dbus-python
                               # pyatspi installed as system package, not pip

requirements-windows.txt    ← -r requirements-common.txt
                               pynput>=1.7.7
                               pywin32>=308
                               comtypes>=1.4.1
                               wmi>=1.5.1
```

---

### CI Matrix

```yaml
# .github/workflows/test.yml
strategy:
  matrix:
    os: [ubuntu-latest, windows-latest]
    python: ["3.11", "3.12"]
```

Both runners execute the full `pytest` suite. Platform-specific modules are mocked on the opposite OS using the same mock interfaces. Any test that is genuinely only meaningful on one platform uses an explicit skip marker:

```python
@pytest.mark.skipif(sys.platform != "linux", reason="evdev is Linux-only")
def test_evdev_hold_gesture():
    ...
```

PRs may not be merged unless both CI runners are green.

---

### Feature Parity Table — `docs/platform-matrix.md`

A living table tracking every user-facing feature and its support status per platform. Updated in the same PR as the feature itself. New features must either be implemented for both platforms in the same PR, or explicitly record a "planned" status in the table with a linked issue.

| Feature | Linux | Windows | Notes |
|---------|-------|---------|-------|
| Push-to-talk hotkey | ✅ | ✅ | |
| Toggle-to-talk hotkey | ✅ | ✅ | |
| Text injection (inject target) | ✅ | ✅ | Clipboard fallback on elevated targets |
| Clipboard target | ✅ | ✅ | |
| Exec target | ✅ | ✅ | |
| File target | ✅ | ✅ | |
| Socket target (TCP) | ✅ | ✅ | |
| Pipe target (named) | ✅ | ✅ | FIFO on Linux; named pipe on Windows |
| D-Bus target | ✅ | ❌ | Linux-only by design |
| faster-whisper (NVIDIA) | ✅ | ✅ | Requires cuDNN on PATH |
| whisper.cpp (Vulkan/AMD) | ✅ | 🔜 planned | v2 |
| CPU-only transcription | ✅ | ✅ | |
| Piper TTS | ✅ | ✅ | |
| espeak-ng TTS | ✅ | ✅ | |
| AT-SPI2 / UIAutomation context | ✅ | ✅ | Different APIs, same capability |
| MCP server (TCP) | ✅ | ✅ | |
| D-Bus control interface | ✅ | ❌ | Replaced by named-pipe IPC |
| Named-pipe control interface | ❌ | ✅ | Windows-only |
| System tray | ✅ | ✅ | |
| Ollama post-processor | ✅ | ✅ | HTTP-based, no changes |
| Snippet expansion | ✅ | ✅ | |
| Waveform / pulse overlays | ✅ | ✅ | |

---

### Branching Model

- `main` — always builds and tests green on **both** platform CI runners before merge
- `feature/*` — must pass both platform jobs before merge; no exceptions
- `platform/windows-*` — short-lived branches for Windows-only fixes that cannot be made cross-platform in a single pass; must be resolved within one sprint

Never allow a Linux-only feature branch to merge without a corresponding Windows implementation or a documented "planned" entry in the platform matrix. Letting this debt accumulate is the primary driver of platform drift.

---

### Factory Function Contract

Every function in `src/platform/__init__.py` must have an identical signature on both platforms. This is the concrete enforcement mechanism:

```python
# src/platform/__init__.py
import sys

if sys.platform == "win32":
    from .windows_input import InputBackend
    from .windows_injector import InjectionBackend
    from .windows_context import ContextReader
    from .windows_gpu import detect_gpu
    from .windows_ipc import ControlServer
    from .audio_output import play_pcm          # shared file, platform-branched inside
else:
    from .linux_input import InputBackend
    from .linux_injector import InjectionBackend
    from .linux_context import ContextReader
    from .linux_gpu import detect_gpu
    from .linux_ipc import ControlServer
    from .audio_output import play_pcm

__all__ = [
    "InputBackend",
    "InjectionBackend",
    "ContextReader",
    "detect_gpu",
    "ControlServer",
    "play_pcm",
]
```

Any new Linux feature that requires a platform module must add a stub (raising `NotImplementedError` with a clear message) or a full implementation on the Windows side in the **same PR**.

---

## 7. Summary

### Effort by Phase

| Phase | Scope | Duration | Risk |
|-------|-------|----------|------|
| 0 — Foundation | Import guards, paths, CI matrix | 1 week | Low |
| 1 — Audio & Transcription | sounddevice, Piper.exe, Whisper | 1 week | Low |
| 2 — Hotkeys | `pynput` replaces `evdev` | 1 week | Medium |
| 3 — Text Injection | `SendInput`, UIAutomation | 1 week | Medium |
| 4 — IPC / MCP Server | TCP socket, named pipe | 1 week | Low |
| 5 — GPU Detection | `pynvml`, `wmi` | 1 week | Low |
| 6 — Installer & Packaging | PyInstaller + NSIS/WiX | 1 week | Medium |
| 7 — Testing & Polish | CI matrix, docs, benchmarks | 1 week | Low |
| **Total** | | **~8 weeks** | |

### Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Hotkey backend | `pynput` | Cross-platform; wraps `SetWindowsHookEx` on Windows |
| Text injection | `SendInput` via ctypes | Native Unicode support; no extra binary required |
| MCP transport | TCP localhost | Platform-neutral; removes `socat` dependency on Linux too |
| AT-SPI2 equivalent | `comtypes` + UIAutomation | Official Windows accessibility API |
| Audio output | `sounddevice` | Already in `requirements.txt`; ships its own PortAudio DLL |
| Packaging | PyInstaller + NSIS/WiX | Closest equivalent to AppImage + `install.sh` |
| v1 GPU scope | NVIDIA + CPU only | Avoids MSVC build complexity; covers the majority of users |
| Config paths | `platformdirs` | Industry-standard library for cross-platform path resolution |

### What Does Not Change

The following modules are fully portable and require zero platform-specific work:

- `src/inference_engine.py` — Whisper transcription pipeline
- `src/routing/` — all routing logic and TOML loading
- `src/llm_postprocessor.py` — Ollama integration (HTTP-based)
- `src/gui/` — all PyQt6 UI (tray icon, overlays, settings window)
- `src/hotkeys/double_tap.py` — gesture state machine
- `src/mcp_server.py` — MCP protocol (JSON-RPC) after TCP switch
- All 275+ tests that do not depend on Linux-specific mocks

Roughly 60% of the codebase is already cross-platform. The Windows port is scoped to replacing the remaining 40% — the hotkey, injection, IPC, GPU, and audio output layers — with Windows-native equivalents behind a clean platform abstraction.
