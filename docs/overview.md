# Overview

## What is VoxCtrl?

VoxCtrl ("Voice Controller") is a desktop dictation application that turns your voice into text and routes it wherever you need it — injected directly into the focused window, saved to a file, sent to an HTTP endpoint, or handed off to an LLM agent via the MCP protocol.

It is designed as a **programmable voice input broker**: you define output targets (where text goes) and hotkey bindings (what keys trigger recording for which targets), and VoxCtrl handles the rest.

Everything runs locally. No audio ever leaves your machine.

---

## Key Features

### Core Dictation
- **Hold-to-record, toggle, double-tap, or double-tap & hold** gesture modes per hotkey binding
- **Global shortcuts without keyboard access** — registered with your desktop through the XDG `GlobalShortcuts` portal, so VoxCtrl is told only when its own shortcut fires and never reads a keystroke ([details](privacy.md))
- **Whisper speech recognition** — the same model family powering OpenAI's transcription API, running entirely on-device
- **GPU acceleration** — automatic CUDA or Vulkan selection when available; falls back to CPU
- **Multiple model sizes** — tiny through large-v3, trading speed for accuracy

### Privacy & Offline Operation
- Zero network requests during normal operation
- No analytics, crash reporting, or telemetry
- All models and voices stored locally under `~/.local/share/voxctrl/`

### Flexible Output Routing
Nine output delivery types:

| Type | What it does |
|---|---|
| `inject` | Simulates keystrokes into the focused window (wtype / xdotool / Ctrl+V) |
| `clipboard` | Copies text to the system clipboard |
| `exec` | Runs a shell command with the text as an argument |
| `pipe` | Writes to a named FIFO pipe |
| `socket` | Sends over a Unix domain socket |
| `file` | Appends to a file with optional timestamp prefix |
| `dbus` | Emits a DBus signal on the session bus |
| `http` | POSTs to an HTTP endpoint |
| `webhook` | POSTs with HMAC-SHA256 signed payload |

A single hotkey binding can route to **multiple targets simultaneously**.

### Visualization & HUD
- Transparent floating overlay window with real-time audio visualization
- Four overlay styles: Blue Wave (default), Voice Card, Waveform, Pulse
- Auto-show on recording start, auto-hide on completion

### Post-Processing Pipeline
Applied after transcription before delivery:
- Filler word removal (`um`, `uh`, `hmm`, `er`, `ah`, `ugh`, `mhm`)
- Spoken punctuation conversion (`"period"` → `.`, `"comma"` → `,`, and 20+ more)
- Auto-format lists (detects "first/second/third" ordinals → numbered list)
- Snippet expansion (custom shorthand → full text)
- Custom vocabulary fuzzy correction (Levenshtein matching for proper nouns/domain terms)
- Code mode (camelCase conversion, spoken operators)
- Optional LLM rewrite via any OpenAI-compatible API server (clean, formal, casual, bullet, concise, or custom prompt)

### Text-to-Speech
- Neural TTS via Piper (ONNX, ~11 English voices)
- Espeak-ng fallback
- `speak_text` callable from hotkeys, MCP, or routing targets

### LLM Integration (MCP Server)
VoxCtrl exposes a Model Context Protocol server so LLM agents (Claude Desktop, Cursor, etc.) can:
- Trigger voice recording and receive the transcription
- Queue TTS playback
- Query live recording/speaking status

### DBus Service (Linux)
Exposes `ai.voxctrl.Dictation` on the session bus for shell scripts and desktop integrations to start/stop recording and receive text output as signals.

### History
Maintains an in-memory transcript log with word count, timestamps, and source target. Viewable in a dedicated history window.

---

## Design Principles

**Local-first.** The app functions identically offline. All models download once and run locally forever after.

**Composable routing.** Targets and bindings are data files (TOML), not hardcoded behavior. You can change where your voice goes without touching the app.

**Hot-reloadable config.** The app watches its config files. Edit `targets.toml` or `config.json` in your editor and VoxCtrl picks up changes instantly.

**Minimal footprint.** No Electron, no Node runtime. Tauri gives you a native WebView shell around a compiled Rust backend. The installed binary is ~tens of MB vs ~hundreds for Electron equivalents.

**Low latency.** Audio is captured on a dedicated thread. Inference runs on a dedicated thread. UI updates and delivery happen concurrently via async channels. Hold a key and you're recording within milliseconds.
