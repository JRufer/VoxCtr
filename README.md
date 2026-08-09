# VoxCtrl

![VoxCtrl Banner](assets/banner.png)

A high-performance, private, on-device voice-to-text dictation application and programmable **voice input broker** built natively in Rust and Tauri with a Svelte frontend. 

**Zero Telemetry. Zero Cloud. 100% On-Device.**
VoxCtrl acts as an intelligent desktop voice gateway, routing your speech to any destination—whether typing directly into a focused window, invoking terminal agents, appending to journals, triggering shell commands, or feeding local AI assistants.

---

## 🔒 Privacy First & Fully On-Device

In an era of cloud processing, VoxCtrl is built from the ground up to guarantee absolute data sovereignty:
* **VoxCtrl does not read your keyboard**: Global shortcuts are registered with your desktop through the XDG `GlobalShortcuts` portal. Your desktop owns the key grab and tells VoxCtrl exactly one thing — that its own shortcut fired. VoxCtrl cannot see what you type in your browser, your terminal, or your password manager, because it is never given the data.
* **No permissions to grant**: No udev rule, no `input` group, no logout, no reboot. There is nothing to undo later, and installing VoxCtrl does not change your machine's security posture. *(Earlier versions installed a udev rule granting read access to every input device. That has been removed — see [why](docs/hotkeys.md#why-this-changed).)*
* **No Cloud API Keys Required**: VoxCtrl relies exclusively on OpenAI's Whisper models (via native CPU/GPU accelerated `whisper-rs`) running directly on your local hardware.
* **No Telemetry**: Your ambient microphone data never leaves your machine. There are no hidden tracking scripts or analytical pings.
* **Air-Gapped Ready**: Once the application and models are downloaded, VoxCtrl requires zero internet access to function.
* **Local Neural Voices**: All text-to-speech feedback is generated offline by a local engine — Piper, Pocket-TTS, Inflect-Micro-v2, or eSpeak-NG.

**Full detail, including how to verify each claim yourself: [docs/privacy.md](docs/privacy.md).**

> **The app tells you which of these is true, live.** Settings → Hotkeys shows
> exactly how shortcuts are reaching VoxCtrl and which keys your desktop bound.
> If your desktop provides no shortcuts portal, VoxCtrl says so at launch and
> explains the trade-off — it will not grant itself keyboard access to work
> around it, because that access would apply to every program you run, not just
> this one.

---

## 🌟 Key Features

* **High-Performance Offline Speech Recognition**: Local on-device inference using native `whisper.cpp` (via `whisper-rs`) supporting multi-threaded CPU execution. NVIDIA CUDA GPU acceleration is available as an opt-in compile-time feature (`--features cuda`); Vulkan acceleration (AMD/Intel/NVIDIA) works in the standard build. The Moonshine ONNX backend is compiled in by default.
* **Modern GUI & Tray System**: A sleek Svelte-based user interface with dedicated, swappable, fully animated overlays (Ocean Wave, Voice Card, Waveform, and Pulse Ring), a searchable transcription history panel, and a native desktop System Tray utility.
* **Low-Latency Audio Loop**: Streamlined recording and VAD (Voice Activity Detection) built using `cpal` to minimize capture latency.
* **Built-in Model Context Protocol (MCP) Server**: Exposes voice dictation and speech synthesis as high-level JSON-RPC tools to AI clients (like Claude Desktop or Cursor) via local secure sockets—keeping integrations fully local.
* **Privacy-Preserving Global Hotkeys**: Shortcuts are registered with your desktop through the XDG `GlobalShortcuts` portal (KDE Plasma, GNOME 48+, Hyprland), so VoxCtrl receives its own shortcuts and never reads a keystroke. Bind hold-to-talk, toggle-to-talk, double-tap, or double-tap & hold gestures. Works identically on Wayland and X11, with no permission setup at all.
* **DBus Dictation Service**: Exposes `ai.voxctrl.Dictation` on the local Linux session bus, letting you script recording states securely without network exposure.
* **Neural Text-to-Speech (TTS)**: Built-in local voice feedback with a choice of four engines — **Piper** (neural, high quality), **Pocket-TTS** (neural, clones a voice from a reference clip), **Inflect-Micro-v2** (neural, 38 MB ONNX), and **eSpeak-NG** (lightweight, always available) — with automatic local package installation and an in-app model downloader. All four are compiled in by default, so any build can use them.
* **Intelligent Post-Processing & LLM Rewriting**: Real-time automatic filler-word cleanup (e.g. stripping "um", "uh", "hmm") to sanitize dictation, combined with optional post-processing through any **OpenAI-compatible API server** (a local [Ollama](https://ollama.ai/) or LM Studio instance, or a hosted provider) for real-time grammar correction, tone rewriting, or custom formatting. Point it at any URL and supply an API key when the server requires one.

---

## 🎯 The Deep Targeting System

The core of VoxCtrl is its **Output Target Router**. Rather than simply pasting text where your cursor is, VoxCtrl allows you to declare **named output targets** in `targets.toml` and bind them to different global keyboard gestures. This turns your voice into a programmable router.

**New in v0.1:** You can now bind **multiple targets** to a single hotkey gesture! When activated, your text is broadcast concurrently to all bound targets. Configurations also **hot-reload instantly** in the background, without requiring an app restart.

Below are the 11 target types supported by VoxCtrl and what they are used for:

| Delivery Type | Mechanism | Perfect Use Case |
| :--- | :--- | :--- |
| **`inject`** | Keystroke simulation via native `wtype` (Wayland), `xdotool` (X11), or PowerShell (Windows). | Standard voice dictation directly into any focused editor, web browser, or chat window. |
| **`clipboard`** | Fast clipboard population using the native `arboard` library. | Quiet copying of notes, code snippets, or templates for manual pasting without modifying active focuses. |
| **`exec`** | Spawns a shell command substituting `{TEXT}` cleanly and safely (uses `shell=False` to prevent command injection). | Integrating with CLI tools (e.g., pipe directly into `llm {TEXT}`, open a web search, or post to `git commit -m "{TEXT}"`). |
| **`pipe`** | Writes raw transcription bytes to a local named FIFO pipe. | Interfacing with custom CLI shell scripts, event listeners, or local terminal agents waiting for command buffers. |
| **`socket`** | Streams text directly over a TCP connection or local Unix Domain Socket. | Communicating with long-running daemons, remote servers, or external development container environments. |
| **`file`** | Appends transcriptions to a local file with customizable prefixes and optional UTC timestamps. | Automatic hands-free voice journaling, log keeping, standup note compilation, or task lists. |
| **`dbus`** | Emits a custom DBus signal containing the text on the session bus. | Triggering complex desktop notification actions, scripting custom desktop widget updates, or chaining custom system automation. |
| **`http`** | Sends a fast HTTP POST/GET request containing the transcription formatted inside a JSON template. | Streaming transcriptions directly to webhooks, database ingestion services, or remote HTTP endpoints. |
| **`webhook`** | Sends a signed, secure HTTP POST request with an HMAC-SHA256 signature generated using a shared secret. | Securely connecting dictation triggers to external APIs or home automation platforms (e.g., Home Assistant). |
| **`speak`** | Plays back the transcribed text aloud via the globally configured Text-to-Speech (TTS) engine. | Hearing the transcribed text spoken back to you directly, even without an active MCP server connection. |
| **`chat`** | Holds a running conversation with an OpenAI-compatible `/v1/chat/completions` server, sending prior turns as context and reading the reply back. | Talking to a local LLM — Hermes, Ollama, llama.cpp — hands-free, with the answer spoken aloud, typed at your cursor, or copied to the clipboard. |

> [!TIP]
> `chat` turns VoxCtrl into a voice front end for the same API Open WebUI uses. Enable your
> server's OpenAI-compatible HTTP API, point `chat_url` at it, and speak. See
> [`examples/targets-hermes-chat.toml`](examples/targets-hermes-chat.toml) and the
> [routing reference](docs/routing.md#chat--conversational-llm-openai-compatible).

---

## 🛠️ The Architecture

```
                  ┌──────────────────────────────┐
                  │  Desktop Shortcuts Portal    │
                  │  org.freedesktop.portal.*    │
                  │  GlobalShortcuts             │
                  └──────────────┬───────────────┘
                                 │ "your shortcut fired"
                                 │  (no keystroke data)
                                 ▼
                  ┌──────────────────────────────┐
                  │      Gesture Recognizer      │
                  │  (Hold / Toggle / Double)    │
                  └──────────────┬───────────────┘
                                 │ on_press(target_id)
                                 ▼
                  ┌──────────────────────────────┐
                  │  Recording Module (cpal)     │
                  └──────────────┬───────────────┘
                                 │ float32 raw audio chunks
                                 ▼
                  ┌──────────────────────────────┐
                  │   Whisper Inference Engine   │
                  │  (whisper.cpp via CUDA/CPU)  │
                  └──────────────┬───────────────┘
                                 │ (transcription, target_id)
                                 ▼
                  ┌──────────────────────────────┐
                  │     Output Target Router     │
                  │      (targets.toml)          │
                  └───────┬───────┬────────┬─────┘
                          │       │        │
                          ▼       ▼        ▼
                  ┌──────────────────────────────┐
                  │  Optional AI Post-processing │
                  │  (Filler Removal / LLM API)  │
                  └───────┬───────┬────────┬─────┘
                          │       │        │
            ┌─────────────┘       │        └─────────────┐
            ▼                     ▼                      ▼
     [inject / clipboard]    [exec / pipe / file]   [dbus / http / socket]
            │                     │                      │
            ▼                     ▼                      ▼
     Focused Editor          Terminal / Scripting    Integration Services
```

---

## 🖥️ User Interface

VoxCtrl provides a clean, native settings window and overlay environment:

![Settings Panel](assets/settings.png)

### 📌 Interactive Settings UI
* **General tab**: Configure core system attributes, including the local MCP JSON-RPC server toggles, record timeouts, and Wayland/X11 AT-SPI2 text injection behaviors.
* **Visual tab**: A premium Cyber Obsidian interface that groups all aesthetic and presentation settings. It features an interactive **Overlay Style Selector** (supporting Voice Card, Waveform, Pulse Ring, Ocean Wave, or Disabled styles), toggles for displaying heads-up HUD overlays while speaking, and controls for sending system notifications on transcription. It also lets you configure if the Settings window should open automatically at launch or start minimized in the system tray.
* **Audio tab**: Configure device gain, input indices, and toggle dynamic streaming/VAD threshold settings.
* **Routing tab**: Define named targets (`targets.toml`), delivery properties, and post-processors.
* **Hotkeys tab**: Setup keybindings (`bindings.toml`) and detect subset/exact-match conflicts in real time.
* **Voice Output tab**: Pick a TTS engine, download its models or voices, and preview them for local speech synthesis.

### 🎨 Heads-Up HUD Overlay Styles

VoxCtrl features a dynamic transparent overlay window — always-on-top and fully click-through — that renders floating real-time audio visualization above your desktop during dictation. Every style has its own identity, audio visualizer, active-target indicator, and animated load/unload transitions. The visual presentation is fully hot-swappable in the **Visual Tab** settings (which synchronizes across windows in real-time) and supports five unique visual options:

1. **Ocean Wave (Default) 🌊**
   A glass tide pool at night with a glowing moon, rising bubbles, and three overlapping parallax wave layers (Deep Blue, Aqua Cyan, and Ice Teal).
   * **Voice Reactive Tide:** Both the waterline and the wave amplitude swell dynamically in response to microphone sound levels, receding to a calm low tide when silent.
   * **Floating Buoy Target Tag:** The active routing target label floats on a buoy that bobs on the wave surface.
   * **Fill & Drain Transitions:** The water fills the pool when dictation starts and drains away when it ends.

2. **Voice Card 💳**
   A literal membership card: gold contact chip, embossed VOXCTRL branding, holographic sheen, and a 20×6 VU-meter LED dot matrix (green→amber→red) lit bottom-up.
   * **Real VU Ballistics:** Instant attack and slow decay, with a sensitivity curve tuned so even quiet speech lights the meter.
   * **Card Flip Transitions:** The card deals in with a flip when dictation starts and flips back out when it ends, with an embossed `TARGET` field and a blinking `REC`/`INIT`/`PROC` stamp.

3. **Waveform 📈**
   A green-phosphor oscilloscope ("OSC-01") with a graticule grid and a live scrolling line trace of your microphone signal, rendered with a phosphor glow. Includes a `TGT ▸` target readout chip and switches to a blue sine sweep during AI post-processing. Powers on and off like a CRT, expanding from (and collapsing back into) a single scanline.

4. **Pulse Ring 🟠**
   A sonar/radar dial: a rotating sweep arm with a trailing wedge, expanding pulse rings that brighten with voice intensity, contact blips that flash as the sweep passes, and an audio-reactive core — paired with a pulsing "TARGET LOCK" plate showing the active routing target.

5. **Disabled (None) ❌**
   Turns off the transparent heads-up display entirely, relying purely on tray icon changes or system bus triggers for dictation feedback.

### ⚙️ Window Management & Focus Raising
* **Foreground Focus Raising**: If the settings page is already open but hidden behind other windows, clicking the **⚙ Settings** button in the native system tray menu or double-clicking the system tray icon will trigger standard `show()` and `set_focus()` commands to immediately bring the settings dashboard to the absolute foreground of the screen.

---

## 🔌 Built-in Model Context Protocol (MCP) Server

VoxCtrl features a native Model Context Protocol (MCP) server listening on a local Unix socket at `/tmp/voxctrl-mcp.sock`. This allows advanced LLM agents (such as **Claude Desktop** or **Cursor**) to interface directly with your voice and speak responses back to you.

### Exposed MCP Tools
1. **`transcribe_voice(timeout_secs)`**: Prompts the application to open your default recording device, capture speech, transcribe it using the Whisper engine, and return the raw text to the model.
2. **`speak_text(text)`**: Queues text to be spoken aloud locally on the user's host machine using the configured neural TTS engine.
3. **`get_status()`**: Returns a JSON object with boolean states indicating whether the microphone is currently recording or the TTS engine is currently speaking.

### 🎯 Generic MCP Routing Target
VoxCtrl supports routing transcribed text directly to any local or networked MCP server via its **Output Target Router** using the `mcp` delivery type in `targets.toml`. 

The client is fully standard-compliant (Option B, performing `initialize` -> `notifications/initialized` -> `tools/call` handshakes on socket connect) to guarantee maximum compatibility with strict third-party MCP servers.

#### Configuration Schema
You can declare generic MCP targets in your `targets.toml` or configure them through the GUI Settings window:

```toml
[[target]]
id = "self_speak"
label = "Synthesize Speech Loopback"
delivery = "mcp"
mcp_path = "/tmp/voxctrl-mcp.sock"   # Optional custom socket or pipe path (defaults to standard socket/pipe)
mcp_tool = "speak_text"            # The name of the MCP tool to call (defaults to 'speak_text')

[target.mcp_args]
text = "{TEXT}"                    # Custom arguments template (substitutes the transcription at {TEXT})
```

---

## 📦 Portable AppImage & Installation

VoxCtrl runs natively on Linux (optimized for CachyOS/Arch, Ubuntu/Debian, Fedora, and openSUSE). We support seamless standalone execution using a portable **AppImage**, which features a built-in installer to handle system integration.

### 1. Unified Setup & System Integration

To install runtime dependencies and integrate VoxCtrl into your desktop environment launcher, run the AppImage with the `--install` flag:

```bash
chmod +x VoxCtrl-*-x86_64.AppImage
./VoxCtrl-*-x86_64.AppImage --install
```

Alternatively, just launch the AppImage normally. If anything is missing, a setup window appears listing every step with its live status.

#### What the built-in installer accomplishes automatically:
* **System Runtime Packages**: Detects your package manager (`apt`, `pacman`, `dnf`, `zypper`) and installs WebKitGTK, OpenSSL, PortAudio, `wtype`, `xdotool`, and clipboard utilities.
* **Desktop Menu Integration**: Registers a modern `.desktop` entry in `~/.local/share/applications/` and copies the application icon so VoxCtrl appears in your desktop application menus.

> [!IMPORTANT]
> **The installer does not touch keyboard permissions, and there is no step that does.**
> Global shortcuts go through the XDG desktop portal, so nothing needs granting.
> The administrator prompt is for installing the packages above and nothing else.
>
> Older VoxCtrl versions wrote `/etc/udev/rules.d/99-voxctrl.rules`, which let
> every program running as your user read every keystroke on your system. The
> installer now **removes** that rule if it finds it, and never creates it.
> [Why](docs/hotkeys.md#why-this-changed).

> [!NOTE]
> VoxCtrl keeps watching: if shortcuts cannot reach it, it says so in the tray
> and in a notification rather than silently ignoring your keypress, and it
> starts working the moment the situation changes — without an app restart.

---

### 2. Standalone AppImage Compilation

If you wish to compile the application and bundle a fresh, portable AppImage manually from source, run the dedicated compiler script:

```bash
chmod +x build_appimage.sh
./build_appimage.sh
```

This compilation script:
* Restructures the workspace compiler toolchain, wrapping the local `appimagetool` to execute inside headless and FUSE-less build/sandbox environments using `--appimage-extract-and-run`.
* Runs frontend compilation via Vite/Svelte and compiles the Rust Tauri backend.
* Automatically injects system GPU/CUDA library paths into the compiler environment for hardware-accelerated transcription (if compatible NVIDIA cards are present).
* Moves and exposes the final, standalone, portable AppImage directly to the root of the workspace as `VoxCtrl-x86_64.AppImage`.

---

### 3. Execution Options

Once set up, you can execute the application in three ways:

* **From Desktop Menu**: Launch **VoxCtrl** directly from your desktop launcher or application drawer.
* **Standalone Portable AppImage**: Run the standalone AppImage executable in the root directory:
  ```bash
  ./VoxCtrl-x86_64.AppImage
  ```
* **Helper Script Wrapper**: Run the workspace helper script:
  ```bash
  ./voxctrl.sh
  ```

---

## ⚙️ Configuration File Schema

All configurations are stored locally inside `~/.config/voxctrl/`.

### `targets.toml`
Defines the output target router destinations:
```toml
format_version = "1.1"

[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
append_newline = false

[[target]]
id = "notes"
label = "Meeting Journal"
delivery = "file"
file_path = "~/Documents/meeting_notes.md"
file_prefix = "- "
file_timestamp = true
```

### `bindings.toml`
Binds hotkey gestures directly to target IDs (supports single or **multiple sequential targets**):
```toml
format_version = "1.1"

[[binding]]
id = "dictate_hold"
label = "Dictate into Focused Window (Hold)"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_id = "default"

[[binding]]
id = "dictate_and_log"
label = "Type & Save Journal (Hold)"
keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_id = "default"                        # Backward compatibility fallback (first target)
target_ids = ["default", "notes"]            # Sequential delivery to both targets!

[[binding]]
id = "double_tap_dictation"
label = "Double-Tap & Hold to Dictate"
keys = ["KEY_LEFTMETA"]
gesture = "double_tap_hold"
tap_ms = 300                                 # Gap allowed between the two taps
hold_threshold_ms = 200                      # Hold on the second tap before recording
target_ids = ["default"]
```

Supported gestures are `hold`, `toggle`, `double_tap` and `double_tap_hold`.
See [docs/hotkeys.md](docs/hotkeys.md) for how each behaves and how to tune the
double-tap timings.

### Multi-Target Hotkey Bindings
VoxCtrl supports routing your speech to **multiple output targets simultaneously** using a single hotkey gesture! 

When a multi-target binding is activated:
1. Your speech is captured and transcribed **once**.
2. The final text is delivered **sequentially** to each target specified in `target_ids`.
3. The UI automatically ensures you cannot assign the same target more than once to prevent accidental duplicates.

#### Svelte UI Target Setup
Inside the Hotkey Binding Editor modal:
- Dynamic target selector fields let you add additional routing destinations using the `＋ Add Target` button.
- Already selected targets are automatically disabled in other dropdowns so you cannot select duplicates.
- Extra dropdown rows feature a clear `✕` button to remove them if added by accident.

---

## 🧪 Development & Verification

### Running the Frontend
To run the Svelte UI in standard hot-reloading development mode:
```bash
cargo tauri dev
```

### Compiling manually
```bash
npm run build
npx tauri build
```

---

## 📄 License

This project is open-source and licensed under the [MIT License](LICENSE).
