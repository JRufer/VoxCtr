# UI & Windows

**Frontend:** `src/` (Svelte 5, Tailwind CSS 4, Vite 5)

## Window Layout

VoxCtrl opens separate native windows managed by Tauri, plus a native overlay helper window:

| Window | Route / Process | Default Size | Properties |
|---|---|---|---|
| Settings | `/settings` | 720 × 640 (min 600 × 450) | Resizable, standard chrome |
| Setup Wizard | `/wizard` | Fitted to the display, 16:9 (min 1280 × 720) | Resizable, first run only |
| Setup / Diagnostics | `/udev-warning` | 580 × 600 (min 480 × 420) | Always-on-top |
| Overlay | `voxctrl-overlay` helper (Slint) | 560 × 190 | Transparent, always-on-top, no decorations, click-through |

The Tauri windows are declared in `src-tauri/tauri.conf.json`, start hidden (`visible: false`), and are shown programmatically. The overlay is a separate native process (`src-tauri/src/overlay.rs`) spawned at startup and driven over stdin; the Svelte `/overlay` route hosts the web counterparts of the same visualizers (used for custom HTML overlays).

### Window Lifecycle

Windows close like any other window: the close button destroys them rather than
hiding them. VoxCtrl is a tray application, so the last window closing must not
end the process — the app refuses the resulting exit request (an explicit quit
from the tray carries an exit code, and is honoured) and carries on listening
for its hotkey with nothing on screen.

Because a closed window no longer exists, every entry point rebuilds one on
demand: the tray menu and its left click, a second launch of the binary, the
startup auto-open, and the wizard's hand-off to Settings. Raising an existing
window pins it above others briefly before focusing it — Linux desktops
routinely ignore a bare focus request from a process that does not already own
the focused window — and does not move it.

---

## Settings Window

The main configuration interface. Organized into a sidebar with ten tabs:

### General Tab
- "Open setup wizard" button — re-runs the first-run wizard
- Overlay show/hide toggle
- Overlay style selector
- Auto-show settings on startup toggle
- Desktop notification toggle
- Recording status indicator and word count
- Manual record/stop button

### Engine Tab
- Backend selector (`whisper-cpp`, `moonshine`)
- Whisper model size selector with download status
- Compute device selector (auto / CPU / CUDA / Vulkan)
- Thread count control
- Moonshine model/language settings
- "Download Model" button with progress
- **Missing Model Warning & Auto-Redirection**: Startup check programmatically determines if the configured Whisper voice model file is downloaded on the local machine. If missing, it immediately switches the active Settings tab to "Engine" and presents a Tailwind-styled yellow warning alert prompting the user to select and download a GGUF voice model size.

### Routing Tab
- Visual editor for `targets.toml` — add/edit/delete output targets
- Per-target fields: label, delivery type, type-specific options
- Per-target processing override controls (filler removal, spoken punctuation, list formatting, code mode)
- Single-line mode for `inject` and `command` targets
- Hotkey binding management (add/edit/delete bindings, key combo recorder, gesture selector). Shows which mechanism is delivering shortcuts and, on the portal path, the keys your desktop actually bound for each binding — which may differ from what was requested, since your desktop gets the final say. The key recorder captures inside VoxCtrl's own focused window using ordinary browser key events; it is not a global listener. It refuses combinations no desktop can bind — modifiers with no regular key, or two regular keys — explains why while you are still recording, and leaves the existing shortcut in place.

### Visual Tab
- Preview and selection of overlay animation styles
- **Show overlay on voice command trigger**: Checkbox toggle to enable or disable displaying the temporary UI overlay pill when a voice command trigger is activated (`show_command_overlay`).
- **Command overlay duration (seconds)**: Number input control to set the display duration (1–10s, default 3s) for the voice command overlay pill (`command_overlay_duration_secs`).
- **Overlay Position Control**: Dropdown choice for setting the Heads-Up display screen alignment (**Top**, **Center**, or **Bottom** of the screen).
- **Overlay Display Control**: Dropdown choice to select which target monitor display screen (**Primary Monitor** or specific connected panels like `"HDMI-1"`) the visual overlay appears on. Features a graceful disconnection primary display failover and a golden warning badge alert.
- Overlay appearance controls

### Audio Tab
- Input device selector (lists all CPAL devices)
- Gain slider
- VAD threshold slider
- Noise suppression toggle (RNNoise; applies to the next recording)
- Dynamic stream toggle
- Live audio level meter (VU meter, updates from `audio-level` events during monitoring)
- Evdev device path input

### TTS Tab
- Enable/disable toggle
- Engine selector (eSpeak-NG / Piper / Pocket-TTS / Inflect-Micro-v2 / Breeze-TTS-2)
- Voice selector with download status per voice
- "Download Voice" button per voice
- Stop key configuration
- Response overlay toggle

### Features Tab
- Filler removal toggle
- Spoken punctuation toggle
- Auto-format lists toggle
- Custom vocabulary list editor
- Snippet key-value editor

### OpenAI API Tab
Configures post-processing through any OpenAI-compatible API server (a local
Ollama or LM Studio instance, or a hosted provider).
- Enable/disable toggle
- API URL (defaults to a local server, e.g. `http://localhost:11434`; may include a `/v1` suffix)
- API Key (sent as a `Bearer` token; required by most remote servers, optional for localhost)
- Model name
- Mode selector
- Custom prompt text area
- "Test Connection" button → shows available models

### About Tab
- Version information
- Links to documentation and source

---

## Overlay Window

A transparent, always-on-top, click-through floating HUD that visualizes audio activity, rendered by the native `voxctrl-overlay` helper process (Slint). It has no title bar or decorations, ignores mouse input (the cursor hit-test is disabled at the windowing-system level), and auto-shows/hides based on recording state (controlled by `ui.show_overlay`). Every style plays a spring-driven load animation on appear and an unload animation on dismiss — the window stays alive until the unload animation completes.

The window coordinates are calculated dynamically relative to the active display monitor's size and scale factor, placing the visualizer cleanly in the **Center**, **Top** (60 logical pixels from the top), or **Bottom** (60 logical pixels from the bottom) of the screen depending on the `ui.overlay_position` setting. The HUD target display can be locked to a specific monitor screen (`ui.overlay_monitor`), failing over gracefully to the primary monitor with a golden warning badge if the target screen is unplugged. Position changes are hot-reloaded and applied instantly in real-time.

### Visualization Styles

Set via `ui.overlay_style` in config. Each style has a unique identity, audio visualizer, and target indicator — see the [Overlay UI Guide](./overlays.md) for full details. Svelte components with the same designs live in `src/lib/Overlay/` for the web overlay layer.

#### `blue_wave` (default) — Ocean Wave
A glass tide pool: three layered waves whose tide rises with the microphone level, rising bubbles, and a buoy tag bobbing on the surface that shows the active target. Water fills on load and drains on unload. Component: `BlueWave.svelte`.

#### `voice_card` — Voice Card
A membership-card design with a gold chip, holographic sheen, and a 20×6 VU-meter LED dot matrix (green→amber→red) with fast-attack/slow-decay ballistics. The active target is embossed in the card's `TARGET` field. Deals in/out with a card flip. Component: `VoiceCard.svelte`.

#### `waveform` — Oscilloscope
A green-phosphor oscilloscope with a live scrolling line trace of the microphone signal, graticule grid, and a `TGT ▸` target readout chip. Powers on/off like a CRT (expands from / collapses to a scanline). Component: `Waveform.svelte`.

#### `pulse` — Pulse Ring
A sonar/radar dial: rotating sweep arm with trailing wedge, expanding audio pulse rings, contact blips, and an audio-reactive core — paired with a pulsing "TARGET LOCK" plate showing the active target. Component: `Pulse.svelte`.

#### `none`
Overlay is disabled entirely.

### Speaking Pill

While TTS is speaking, a green "SYSTEM RESPONDING" pill with a live mini-equalizer and the active target label slides up from the bottom of the overlay window (and a red pill is shown for MCP recording in the web overlay layer).

---

## Setup Wizard

Opens once, on a machine whose config file does not exist yet, and is the only
window shown on that first launch. Seven steps, each writing its choice to the
config as it is made:

| Step | Writes | Notes |
|---|---|---|
| Welcome | — | A read-only contents page; the cards preview the steps rather than linking to them |
| Engine | `engine.backend`, model size, `whisper_cpp.device` | Continue downloads the chosen model and waits for it. A model already on disk needs no click; one that is not requires an explicit engine and size, so a multi-gigabyte default is never fetched unasked |
| Hotkey | `bindings.toml` | Only gestures the running shortcut backend can deliver are offered, and the combination is validated by the same Rust rules the portal registration uses. Blocked until the desktop has accepted the shortcut, because the next step is a live test |
| Overlay | `ui.show_overlay`, `ui.overlay_style`, `ui.overlay_position` | Each style previews a recording of the real overlay, bundled at `src/assets/overlays/<style id>.webm`, falling back to a CSS animation |
| Test | — | A real dictation: the transcript is injected into the focused window, and the readout follows the pipeline's own recording and processing state |
| Voice | `tts.enabled`, `tts.engine` | Each engine downloads from its own card; the play button unlocks once its assets are on disk |
| Done | `ui.setup_completed` | Lists anything that failed, with the raw backend error and a copyable diagnostics report |

The first hotkey is bound to a `command` delivery target named "Command",
created by the wizard if it does not exist. Command delivery falls through to
typing into the focused window when no "VoxCtrl &lt;target&gt;" phrase is present,
so it behaves exactly like `inject` until a second target exists.

The window opens at the largest 16:9 size that fits the display, capped at
1600 × 900 and floored at 1280 × 720 — the size below which the layout starts
to wrap. A fixed size is not safe: 1600 × 900 on a 1080p display at 125%
scaling is 2000 × 1125 physical, and opens with its footer off-screen.

Re-runnable afterwards with `voxctrl --setup` (also `--wizard`,
`--setup-wizard`, `--first-run`), which works whether or not the app is already
running, or from Settings → General. A re-run builds a fresh window and starts
at step one.

---

## Frontend State Management

### Config Store (`src/stores/config.ts`)

```typescript
// Reactive store — all Settings components bind to this
export const config = writable<AppConfig>(defaultConfig);
export const configDirty = writable(false);

// Auto-save: debounced 400ms after any change
config.subscribe((cfg) => {
  // 400ms debounce → invoke('save_config', { newConfig: cfg })
});
```

Initialized by `loadConfig()` which calls `get_config()` IPC on startup. The `config-changed` Tauri event refreshes the store when another window or external process modifies config, with a guard to avoid circular auto-save loops.

### Status Store (`src/stores/status.ts`)

```typescript
export const status = writable<AppStatus>({
  recording: false,
  processing: false,
  speaking: false,
  audio_ready: true,
  word_count: 0,
  active_target_id: "default",
  active_target_label: "Focused Window",
});

// Derived convenience stores:
export const recording = derived(status, ($s) => $s.recording);
export const speaking = derived(status, ($s) => $s.speaking);
export const wordCount = derived(status, ($s) => $s.word_count);
export const activeTargetLabel = derived(status, ($s) => $s.active_target_label ?? "Focused Window");
```

Updated by `status-tick` Tauri events (emitted by backend every ~250ms) and an initial `get_status()` call on load.

---

## Tauri Events (Backend → Frontend)

| Event | Payload | Description |
|---|---|---|
| `status-tick` | `AppStatus` | Periodic state update (~250ms) |
| `config-changed` | `AppConfig` | Config was modified (by any window or externally) |
| `audio-level` | `f32` | RMS audio level for VU meter (while monitoring is active) |

---

## Build & Dev

```bash
# Development (hot-reload frontend, Rust recompiles on save)
npm run tauri dev

# Production build (AppImage on Linux, .exe/.msi on Windows)
npm run tauri build
```

The Vite dev server runs on `http://localhost:5173`. In dev mode, Tauri loads the frontend from Vite. In production, frontend assets are bundled into the binary.
