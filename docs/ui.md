# UI & Windows

**Frontend:** `src/` (Svelte 5, Tailwind CSS 4, Vite 5)

## Window Layout

VoxCtrl opens separate native windows managed by Tauri, plus a native overlay helper window:

| Window | Route / Process | Default Size | Properties |
|---|---|---|---|
| Settings | `/settings` | 840 × 640 (min 700 × 500) | Resizable, standard chrome |
| Overlay | `voxctrl-overlay` helper (Slint) | 560 × 190 | Transparent, always-on-top, no decorations, click-through |
| History | `/history` | 600 × 500 | Resizable, standard chrome |

The Tauri windows are declared in `src-tauri/tauri.conf.json`, start hidden (`visible: false`), and are shown programmatically. The overlay is a separate native process (`src-tauri/src/overlay.rs`) spawned at startup and driven over stdin. There is also a `/overlay` route and a matching set of Svelte components in `src/lib/Overlay/`, but no Tauri window currently loads that route — it's legacy/inactive code left over from before the native Slint renderer became the only overlay path, and it only mirrors 8 of the 13 current styles (the 5 newest styles have no Svelte counterpart).

---

## Settings Window

The main configuration interface. Organized into a sidebar with nine tabs:

### General Tab
- Overlay show/hide toggle
- Overlay style selector
- Auto-show settings on startup toggle
- Desktop notification toggle
- History tracking toggle
- Recording status indicator and word count
- Manual record/stop button

### Engine Tab
- Backend selector (`auto`, `whisper-cpp`, `moonshine`)
- Inference mode selector (`Balanced`, `Aggressive`)
- Whisper model size selector with download status
- Compute device selector (auto / CPU / CUDA / Vulkan)
- Thread count control
- Moonshine model/language settings
- "Download Model" button with progress
- **Missing Model Warning & Auto-Redirection**: Startup check programmatically determines if the configured Whisper voice model file is downloaded on the local machine. If missing, it immediately switches the active Settings tab to "Engine" and presents a Tailwind-styled yellow warning alert prompting the user to select and download a GGUF voice model size.

### Routing Tab
- Visual editor for `targets.toml` — add/edit/delete output targets
- Per-target fields: label, delivery type, type-specific options
- Per-target processing override controls
- Hotkey binding management (add/edit/delete bindings, key combo recorder, gesture selector)

### Visual Tab
- Preview and selection of overlay animation styles
- **Overlay Position Control**: Dropdown choice for setting the Heads-Up display screen alignment (**Top**, **Center**, or **Bottom** of the screen).
- **Overlay Display Control**: Dropdown choice to select which target monitor display screen (**Primary Monitor** or specific connected panels like `"HDMI-1"`) the visual overlay appears on. Features a graceful disconnection primary display failover and a golden warning badge alert.
- Overlay appearance controls

### Audio Tab
- Input device selector (lists all CPAL devices)
- Gain slider
- VAD threshold slider
- Noise suppression toggle
- Dynamic stream toggle
- Live audio level meter (VU meter, updates from `audio-level` events during monitoring)
- Evdev device path input

### TTS Tab
- Enable/disable toggle
- Engine selector (Piper / Espeak)
- Voice selector with download status per voice
- "Download Voice" button per voice
- Stop key configuration
- Response overlay toggle

### Features Tab
- Filler removal toggle
- Spoken punctuation toggle
- Auto-format lists toggle
- Quiet mode toggle
- Custom vocabulary list editor
- Snippet key-value editor

### Ollama Tab
- Enable/disable toggle
- Endpoint URL
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

Set via `ui.overlay_style` in config. Each style has a unique identity, audio visualizer, and target indicator — see the [Overlay UI Guide](./overlays.md) for full details. Eight of the thirteen styles also have a same-named (but currently unused — see above) Svelte component in `src/lib/Overlay/`, noted below where one exists.

#### `blue_wave` (default) — Ocean Wave
A glass tide pool: three layered waves whose tide rises with the microphone level, rising bubbles, and a buoy tag bobbing on the surface that shows the active target. Water fills on load and drains on unload. Svelte component: `BlueWave.svelte`.

#### `voice_card` — Voice Card
A membership-card design with a gold chip, holographic sheen, and a 20×6 VU-meter LED dot matrix (green→amber→red) with fast-attack/slow-decay ballistics. The active target is embossed in the card's `TARGET` field. Deals in/out with a card flip. Svelte component: `VoiceCard.svelte`.

#### `waveform` — Oscilloscope
A green-phosphor oscilloscope with a live scrolling line trace of the microphone signal, graticule grid, and a `TGT ▸` target readout chip. Powers on/off like a CRT (expands from / collapses to a scanline). Svelte component: `Waveform.svelte`.

#### `pulse` — Pulse Ring
A sonar/radar dial: rotating sweep arm with trailing wedge, expanding audio pulse rings, contact blips, and an audio-reactive core — paired with a pulsing "TARGET LOCK" plate showing the active target. Svelte component: `Pulse.svelte`.

#### `mono_bars` — Mono Bars
A pure black & white, centre-weighted 5-bar level meter with no color or glow. Svelte component: `MonoBars.svelte`.

#### `spectrum` — Neon Spectrum
A 16-band magenta-to-cyan equalizer that rises out of the floor on load. Svelte component: `Spectrum.svelte`.

#### `terminal` — Retro Terminal
A DOS-blue console with a block-character ASCII meter and a `$ voxctrl listen --target "..."` command line. Svelte component: `Terminal.svelte`.

#### `vinyl` — Analog VU
A vintage VU meter with a spring-loaded needle sweeping a `-20` to `+3` dial. Svelte component: `Vinyl.svelte`.

#### `aurora` — Aurora Ribbon
A flowing, tri-tone (cyan/violet/pink) sine ribbon trace whose swing widens with voice energy. No Svelte component.

#### `holo` — Holo Ring
A rotating holographic atom of orbital rings and particle nodes that flash as the sweep passes. No Svelte component.

#### `petals` — Frequency Petals
A radial mandala of spokes blooming outward from a centre hub, magenta to violet. No Svelte component.

#### `dotstrip` — Minimal Dot Strip
An ultra-compact pill of dots that grow and brighten centre-out with voice energy. No Svelte component.

#### `starfield` — Starfield Spectrum
A constellation over a deep indigo field with a connected level-history thread running through it. No Svelte component.

#### `none`
Overlay is disabled entirely.

### Speaking Pill

While TTS is speaking, a green "SYSTEM RESPONDING" pill with a live mini-equalizer and the active target label slides up from the bottom of the overlay window (and a red pill is shown for MCP recording in the web overlay layer).

---

## History Window

Displays a log of all transcription sessions in reverse-chronological order.

Each entry shows:
- Timestamp
- Transcribed text
- Target that received the text
- Inference time (ms)

Features:
- "Clear History" button (also resets word count)
- History persists in memory until the app is closed (not written to disk)

Enabled via `ui.history_enabled = true` in config.

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
