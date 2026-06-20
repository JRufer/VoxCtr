# Architecture

## High-Level Design

VoxCtrl is a **Tauri 2** application: a compiled Rust backend that spawns a WebView window running a Svelte SPA. The two halves communicate via Tauri's IPC bridge (invoke commands + event emitters).

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                     │
│                                                          │
│  ┌───────────────────┐      ┌──────────────────────────┐ │
│  │   Svelte Frontend │◄────►│    Rust Backend (lib.rs) │ │
│  │   (WebView)       │ IPC  │    + crates workspace     │ │
│  └───────────────────┘      └──────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
         │                              │
   Settings/History              Audio devices,
   windows                       Filesystem, DBus,
                                 Network (Ollama/HTTP)
```

In addition, the backend spawns a **native overlay helper process** (`voxctrl-overlay`, built with Slint from `src-tauri/src/overlay.rs`) that renders the always-on-top, click-through recording HUD. The backend streams newline-delimited JSON (`status` / `position` / `shutdown`) to the helper's stdin; the helper runs its own 16 ms animation loop, drives spring-based load/unload animations, and builds the per-style visualizer geometry (oscilloscope trace, radar sweep, ocean waves, VU LED matrix) each tick.

---

## Crate Workspace

The backend is organized as a Cargo workspace of focused, single-responsibility crates:

```
VoxCtrl/
├── src-tauri/         # Tauri app entry + IPC command handlers
│   └── src/
│       ├── main.rs    # App bootstrap
│       ├── lib.rs     # Pipeline coordinator (~2000 LOC)
│       ├── commands.rs# Tauri #[command] handlers (~330 LOC)
│       └── state.rs   # Shared AppState
│
└── crates/
    ├── voxctrl-config/     # AppConfig struct, TOML/JSON persistence
    ├── voxctrl-audio/      # Microphone capture, resampling, VU meter
    ├── voxctrl-hotkeys/    # Global key listener (evdev / Win32)
    ├── voxctrl-inference/  # Whisper transcription + post-processing
    ├── voxctrl-routing/    # OutputTarget + HotkeyBinding data models, router
    ├── voxctrl-inject/     # Text injection via wtype/xdotool/clipboard
    ├── voxctrl-tts/        # Piper/Espeak TTS engine
    ├── voxctrl-mcp/        # MCP JSON-RPC server (Unix socket / named pipe)
    ├── voxctrl-dbus/       # DBus service (Linux session bus)
    └── voxctrl-llm/        # Ollama HTTP client
```

---

## Data Flow

```
Hotkey press
     │
     ▼
voxctrl-hotkeys ──gesture_tx──► lib.rs coordinator
                                      │
                         ┌────────────┤
                         │            │
                  Start AudioRecorder  Determine target from binding
                  (voxctrl-audio)
                         │
                    audio_tx chunks
                         │
                         ▼
                  Audio Accumulator
                  (lib.rs buffer)
                         │
                  Hotkey release / VAD stop
                         │
                    inference_tx
                         │
                         ▼
                  InferenceEngine.process()
                  (voxctrl-inference)
                    │  Noise gate (VAD)
                    │  Whisper transcription
                    │  Filler removal
                    │  Spoken punctuation
                    │  Auto-format lists
                    │  Snippet expansion
                    │  Custom vocab fuzzy correction
                    │  Code mode
                    │  Silence hallucination filter
                    │  Ollama rewrite (optional, per-target)
                         │
                    text_tx (InferenceOutput)
                         │
                         ▼
                  OutputTargetRouter.route()
                  (voxctrl-routing)
                    ├── inject → voxctrl-inject
                    ├── clipboard → arboard
                    ├── file → tokio::fs
                    ├── http/webhook → reqwest
                    ├── exec → std::process
                    ├── socket → UnixStream
                    ├── dbus → voxctrl-dbus
                    ├── mcp → voxctrl-mcp response queue
                    └── pipe → named FIFO
                         │
                    Tauri event → frontend
                    (status-tick, history update)
```

---

## Concurrency Model

VoxCtrl uses Tokio for async I/O plus dedicated OS threads for latency-sensitive work:

| Thread / Task | Type | Purpose |
|---|---|---|
| Main Tauri thread | OS thread | Window management, IPC dispatch |
| Audio capture | OS thread (cpal) | Microphone streaming at hardware rate |
| Audio level emitter | Tokio task | Forwards RMS levels to UI every ~50ms |
| Hotkey listener | OS thread | evdev/Win32 event loop |
| Inference worker | OS thread | Blocking Whisper computation; `WhisperState` (KV cache + attention buffers) is allocated once at load and reused across all calls |
| Status ticker | Tokio task | Emits `status-tick` events every 250ms |
| Config watcher | Tokio task | `inotify`/`kqueue` on config files |
| MCP server | Tokio task | Unix socket accept loop |
| DBus service | Tokio task | Session bus method handler |
| TTS FIFO watcher | Tokio task | Named pipe reader for TTS input |

**Shared state** is an `Arc<AppState>` with `AtomicBool`/`AtomicU32` for hot-path flags and `Mutex` for heavier data (targets, history, TTS handle).

**Channels** (crossbeam/tokio):
- `audio_tx` / `audio_rx` — `Vec<f32>` chunks
- `inference_tx` / `inference_rx` — `InferenceRequest`
- `text_tx` / `text_rx` — `InferenceOutput`
- `audio_level_tx` / `level_rx` — `f32` RMS
- `gesture_tx` / `gesture_rx` — `GestureEvent`
- `hotkey_reloader` — updated bindings list sent to listener thread (hot-reload)

---

## Frontend Architecture

The Svelte frontend is a single-page app whose routes back the Tauri windows declared in `tauri.conf.json` (`settings`, `history`, `udev-warning`):

```
App.svelte  (route switcher)
  ├── /settings      → Settings component (sidebar with 9 tabs)
  │     ├── GeneralTab
  │     ├── EngineTab
  │     ├── RoutingTab     (targets + bindings editor)
  │     ├── VisualTab
  │     ├── AudioTab
  │     ├── TtsTab
  │     ├── FeaturesTab
  │     ├── OllamaTab
  │     └── AboutTab
  │
  ├── /history       → History component
  └── /udev-warning  → Udev diagnostics component
```

The on-screen overlay HUD is rendered entirely by the native `voxctrl-overlay`
Slint helper process (see [Overlay UI Guide](./overlays.md)), not by the
frontend. `App.svelte` also has a route guard for `/overlay`, and
`src/lib/Overlay/` still contains a parallel set of Svelte components
(`Overlay.svelte`, `BlueWave.svelte`, `VoiceCard.svelte`, `Waveform.svelte`,
`Pulse.svelte`, `MonoBars.svelte`, `Spectrum.svelte`, `Terminal.svelte`,
`Vinyl.svelte`) that mirror eight of the built-in Slint styles. **No Tauri
window currently loads `/overlay`** — `tauri.conf.json` only declares
`settings`, `history`, and `udev-warning` windows, and nothing in
`src-tauri/src/lib.rs` opens one dynamically. This Svelte layer (and the
related `get_custom_overlays` custom-HTML-overlay feature it was built to
host) is inactive/legacy code from before the native Slint renderer became
the only overlay path; it has not been kept in sync with the five styles
added after it (Aurora Ribbon, Holo Ring, Frequency Petals, Minimal Dot
Strip, Starfield Spectrum).

**State management:**
- `src/stores/config.ts` — reactive `AppConfig` with 400ms debounced auto-save via `save_config` IPC; also listens for `config-changed` events
- `src/stores/status.ts` — live state from `status-tick` events + derived stores (`recording`, `speaking`, `wordCount`, `activeTargetLabel`)

---

## File Locations

| Path | Contents |
|---|---|
| `~/.config/voxctrl/config.json` | Main application config |
| `~/.config/voxctrl/targets.toml` | Output target definitions |
| `~/.config/voxctrl/bindings.toml` | Hotkey binding definitions |
| `~/.local/share/voxctrl/models/` | Downloaded Whisper GGUF models |
| `~/.local/share/voxctrl/piper-voices/` | Downloaded Piper voice packs |
| `/tmp/voxctrl-mcp.sock` | MCP Unix domain socket (Linux) |
| `\\.\pipe\voxctrl-mcp` | MCP named pipe (Windows) |
