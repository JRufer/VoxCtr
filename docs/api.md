# API Reference

## Tauri IPC Commands

These are the commands the Svelte frontend (or any Tauri WebView) can call via `invoke()`.

**Source:** `src-tauri/src/commands.rs`

```typescript
import { invoke } from '@tauri-apps/api/core';
```

---

### Status & Recording

#### `get_status() → StatusPayload`
Returns the current application state.

```typescript
const status = await invoke<StatusPayload>('get_status');
```

```typescript
interface StatusPayload {
  recording: boolean;
  processing: boolean;
  speaking: boolean;
  mcp_recording: boolean;
  audio_ready: boolean;
  word_count: number;
  active_target_id: string;
  active_target_label: string;
}
```

---

#### `start_recording() → void`
Sets the recording flag to true. The audio pipeline will start capturing.

```typescript
await invoke('start_recording');
```

---

#### `stop_recording() → void`
Sets the recording flag to false, signaling the audio pipeline to stop.

```typescript
await invoke('stop_recording');
```

---

#### `toggle_recording() → boolean`
Toggles recording state. Returns the **new** recording state.

```typescript
const nowRecording = await invoke<boolean>('toggle_recording');
```

---

### Configuration

#### `get_config() → AppConfig`
Returns the full application configuration.

```typescript
const config = await invoke<AppConfig>('get_config');
```

---

#### `save_config(newConfig: AppConfig) → void`
Persists configuration to `~/.config/voxctrl/config.json` and emits a `config-changed` event to all windows.

```typescript
await invoke('save_config', { newConfig: myConfig });
```

Note the parameter name is `newConfig` (camelCase), not `config`.

---

### Routing

#### `get_targets() → OutputTarget[]`
Returns all output targets from `targets.toml`.

```typescript
const targets = await invoke<OutputTarget[]>('get_targets');
```

---

#### `save_targets(targets: OutputTarget[]) → void`
Writes updated targets to `targets.toml`, updates the in-memory cache, hot-reloads the router, and spawns any new FIFO response pipe listeners.

```typescript
await invoke('save_targets', { targets: myTargets });
```

---

#### `get_bindings() → HotkeyBinding[]`
Returns all hotkey bindings from `bindings.toml`.

```typescript
const bindings = await invoke<HotkeyBinding[]>('get_bindings');
```

---

#### `save_bindings(bindings: HotkeyBinding[]) → void`
Writes updated bindings to `bindings.toml` and sends a hot-reload signal to the hotkey listener thread.

```typescript
await invoke('save_bindings', { bindings: myBindings });
```

---

#### `reset_chat_conversation(targetId: string) → number`
Forgets a `chat` target's stored conversation so the next dictation starts a new thread.
Returns how many messages were discarded. Unknown target ids return `0`.

```typescript
const dropped = await invoke<number>('reset_chat_conversation', { targetId: 'hermes' });
```

---

#### `test_chat_target(target: OutputTarget) → string`
Probes a `chat` target's `GET /v1/models` endpoint. Resolves with a description of the
reachable endpoint, or rejects with the failure reason. Accepts an unsaved target so the
settings UI can test edits before they are persisted.

```typescript
try {
  const detail = await invoke<string>('test_chat_target', { target: editingTarget });
} catch (e) {
  console.error('Chat endpoint unreachable:', e);
}
```

---

### Setup & First Run

#### `open_setup_wizard() → void`
Opens the first-run wizard, building its window if it has been closed. A
re-opened wizard starts at step one.

```typescript
await invoke('open_setup_wizard');
```

---

#### `finish_setup_wizard(openSettings: boolean) → void`
Marks setup complete (`ui.setup_completed = true`), persists the config, and
closes the wizard window. Pass `true` to open Settings afterwards.

```typescript
await invoke('finish_setup_wizard', { openSettings: false });
```

---

#### `get_setup_status() → SetupStatusPayload`
Everything first-run setup depends on, in one call: how global shortcuts are
being delivered, whether text can be typed into other windows, and whether a
speech model is on disk. The wizard's final screen uses it to report problems
it did not itself cause.

```typescript
interface SetupStatusPayload {
  hotkeys: HotkeyStatusPayload;
  hotkeys_active: boolean;
  model_ready: boolean;
  model_size: string;
  model_auto_downloads: boolean;    // small models fetch themselves in the background
  missing_injection_tool: string | null;
  pkexec_available: boolean;
  manual_package_commands: string;
  is_complete: boolean;
}
```

---

### Text-to-Speech

#### `speak_text(text: string, voice?: string) → void`
Queues text for TTS playback.

```typescript
await invoke('speak_text', { text: 'Hello world', voice: 'en-us-lessac-medium' });
```

`voice` is optional — omit to use the configured default.

---

#### `check_voice_downloaded(voiceName: string) → boolean`
Returns whether a Piper voice pack is available locally.

```typescript
const downloaded = await invoke<boolean>('check_voice_downloaded', {
  voiceName: 'en-us-lessac-medium'
});
```

---

#### `download_voice(voiceName: string) → void`
Downloads a Piper voice pack from GitHub.

```typescript
await invoke('download_voice', { voiceName: 'en-us-ryan-high' });
```

---

#### `list_pocket_tts_voices(voiceDir: string) → { id: string; label: string }[]`
Returns the built-in Pocket-TTS voice catalogue merged with any custom `.wav` clips found in `voiceDir` (`""` = default directory). A custom clip named after a built-in voice id overrides that entry's label/source instead of adding a duplicate.

```typescript
const voices = await invoke<{ id: string; label: string }[]>('list_pocket_tts_voices', {
  voiceDir: '',
});
```

---

#### `check_pocket_tts_ready(voice: string, voiceDir: string) → boolean`
Returns whether the model weights, tokenizer, and the selected voice's reference clip are all present locally (no network access).

```typescript
const ready = await invoke<boolean>('check_pocket_tts_ready', {
  voice: 'alba',
  voiceDir: '',
});
```

---

#### `download_pocket_tts(voice: string, voiceDir: string, hfToken: string | null) → void`
Downloads the gated model weights, tokenizer, and the selected voice's reference clip. For a custom voice resolved from `voiceDir`, the clip is already on disk so only the model weights/tokenizer are fetched.

```typescript
await invoke('download_pocket_tts', {
  voice: 'alba',
  voiceDir: '',
  hfToken: '<your HuggingFace token>',
});
```

---

#### `inflect_micro_available() → boolean`
Whether this build was compiled with the `inflect-micro` cargo feature. When `false` the engine can be selected and its model downloaded, but synthesis is unavailable — the Settings panel uses this to explain why Test TTS is disabled.

```typescript
const available = await invoke<boolean>('inflect_micro_available');
```

---

#### `check_inflect_micro_downloaded(modelDir: string) → boolean`
Whether both ONNX graphs and a usable phoneme table are present in `modelDir` (`""` = default directory). The table is detected by parsing rather than by filename, so this agrees with what synthesis will actually accept.

```typescript
const ready = await invoke<boolean>('check_inflect_micro_downloaded', { modelDir: '' });
```

---

#### `download_inflect_micro(modelDir: string) → void`
Downloads `duration.onnx`, `decode.onnx`, their accompanying files, and the ordered symbol list. The export's layout is discovered by listing the Hugging Face API, and the symbol list is fetched separately because it is published in a different repository from the graphs. Independent of the `inflect-micro` feature — the model downloads in any build.

```typescript
await invoke('download_inflect_micro', { modelDir: '' });
```

---

#### `inflect_micro_inspect(modelDir: string) → object`
Reports what the downloaded graphs actually declare: every input and output with its element type and shape, plus the phoneme table's filename and size. Skips the contract check, so it still answers for a model whose signature does *not* match. Requires the `inflect-micro` feature.

```typescript
const signature = await invoke<unknown>('inflect_micro_inspect', { modelDir: '' });
```

---

### Speech Recognition Models

#### `check_model_downloaded(modelSize: string) → boolean`
Returns whether a Whisper model GGUF file is present locally.

```typescript
const downloaded = await invoke<boolean>('check_model_downloaded', { modelSize: 'base' });
```

---

#### `download_model(modelSize: string) → void`
Downloads a Whisper GGUF model.

```typescript
await invoke('download_model', { modelSize: 'small' });
```

Valid sizes: `"tiny"`, `"tiny.en"`, `"base"`, `"base.en"`, `"small"`, `"small.en"`, `"medium"`, `"medium.en"`, `"large-v2"`, `"large-v3"`, `"large-v3-turbo"`

---

### Audio Monitoring

#### `start_monitoring_audio() → void`
Enables the monitoring flag so `audio-level` events are emitted for the VU meter.

```typescript
await invoke('start_monitoring_audio');
```

---

#### `stop_monitoring_audio() → void`
Disables monitoring and stops `audio-level` event streaming.

```typescript
await invoke('stop_monitoring_audio');
```

---

#### `list_audio_devices() → AudioDeviceInfo[]`
Returns all available input devices.

```typescript
const devices = await invoke<AudioDeviceInfo[]>('list_audio_devices');
```

```typescript
interface AudioDeviceInfo {
  index: number;
  name: string;
}
```

---

### OpenAI API (LLM post-processing)

#### `test_openai(endpoint: string, apiKey: string | null, timeoutSecs: number) → OpenAiTestResult`
Pings an OpenAI-compatible API server (`GET {endpoint}/v1/models`) and lists
available models. `apiKey` is sent as a `Bearer` token when present; pass `null`
for servers that don't require authentication (e.g. a local server).

> This command was previously named `test_ollama`; the client speaks the OpenAI
> API and works with any compatible server.

```typescript
const result = await invoke<OpenAiTestResult>('test_openai', {
  endpoint: 'http://localhost:11434',
  apiKey: null,
  timeoutSecs: 5
});
```

```typescript
interface OpenAiTestResult {
  success: boolean;
  message: string;
  models: string[];
}
```

---

### Overlay

#### `show_overlay() → void`
Makes the overlay window visible and sets always-on-top.

```typescript
await invoke('show_overlay');
```

#### `hide_overlay() → void`
Hides the overlay window.

```typescript
await invoke('hide_overlay');
```

---

## Tauri Events (Backend → Frontend)

Subscribe with `listen()` from `@tauri-apps/api/event`.

```typescript
import { listen } from '@tauri-apps/api/event';
```

### `status-tick`
Emitted every ~250ms with the current application state.

```typescript
await listen<AppStatus>('status-tick', (event) => {
  console.log(event.payload.recording);
});
```

### `config-changed`
Emitted when the config is saved (from any window or external change).

```typescript
await listen<AppConfig>('config-changed', (event) => {
  config.set(event.payload);
});
```

### `audio-level`
Emitted during monitoring with the current RMS energy level (0.0–1.0+).

```typescript
await listen<number>('audio-level', (event) => {
  updateVuMeter(event.payload);
});
```

---

## TypeScript Types

These types are defined in `src/stores/config.ts`:

```typescript
interface AppConfig {
  engine: EngineConfig;
  audio: AudioConfig;
  ui: UiConfig;
  features: FeaturesConfig;
  openai: OpenAiConfig;
  tts: TtsConfig;
  mcp: McpConfig;
}

interface EngineConfig {
  backend: "whisper-cpp" | "moonshine";  // a legacy "auto" loads as whisper-cpp
  whisper_cpp: WhisperCppConfig;
  moonshine: MoonshineConfig;
}

interface WhisperCppConfig {
  model_dir: string;
  model_size: string;
  device: string;
  threads: number;
}

interface MoonshineConfig {
  model_size: string;
  language: string;
}

interface AudioConfig {
  vad_threshold: number;
  input_device_index: number | null;
  evdev_device: string | null;
  noise_suppression: boolean;
  gain: number;
  dynamic_stream: boolean;
}

interface UiConfig {
  show_overlay: boolean;
  overlay_style: "voice_card" | "waveform" | "pulse" | "blue_wave" | "none";
  overlay_position: string;
  overlay_monitor: string;
  auto_show_settings: boolean;
  setup_completed: boolean;
  show_notification: boolean;
}

interface FeaturesConfig {
  remove_fillers: boolean;
  custom_vocabulary: string[];
  spoken_punctuation: boolean;
  auto_format_lists: boolean;
  snippets: Record<string, string>;
}

interface OpenAiConfig {
  enabled: boolean;
  model: string;
  mode: "clean" | "formal" | "casual" | "bullet" | "concise" | "custom"; // GUI preset that fills system_prompt
  custom_prompt: string | null; // legacy; migrated into user_prompt on load
  system_prompt: string;   // system message (empty = none)
  user_prompt: string;     // user message template; must contain "{text}"
  endpoint: string;        // OpenAI-compatible API base URL (a `/v1` suffix is optional)
  api_key: string | null;  // sent as a Bearer token when set
  timeout_secs: number;
}

interface PocketTtsConfig {
  voice: string;
  prewarm: boolean;
  hf_token: string | null;
  voice_dir: string;       // custom .wav voice clips; empty = default directory
}

interface InflectMicroConfig {
  model_dir: string;        // empty = default directory
  seed: number;             // deterministic for a fixed seed
  noise_scale: number;      // 0.0-1.0 variation, default 0.667
  prewarm: boolean;
}

interface BreezeTts2Config {
  voice_mode: "prompt" | "clone";
  cloned_voice: string;     // voice id from the shared clip folder
  voice_dir: string;        // shared with pocket_tts; empty = default directory
  speaker_prompt: string;   // Voice Design description
  model_dir: string;        // empty = default directory
  hf_token: string | null;  // shared with pocket_tts
  prewarm: boolean;
  gpu: boolean;             // needs a breeze-cuda / breeze-metal build
}

interface TtsConfig {
  enabled: boolean;
  engine: "piper" | "espeak" | "pocket_tts" | "inflect_micro" | "breeze_tts_2";
  voice: string;
  voice_dir: string;
  stop_key: string[];       // singular field name, plural value
  response_overlay: boolean;
  speed: number;            // not used by pocket_tts
  gpu: boolean;             // only applies to piper; Breeze has its own flag
  pocket_tts: PocketTtsConfig;
  inflect_micro: InflectMicroConfig;  // fixed-voice, so no voice field
  breeze_tts_2: BreezeTts2Config;
  snippets: Record<string, string>;   // pronunciation guide, speech only
}

interface McpConfig {
  server_enabled: boolean;  // not "enabled"
  record_timeout: number;   // default for transcribe_voice, read per call
  visual_feedback: boolean;
}

interface OutputTarget {
  id: string;
  label: string;
  delivery: "inject" | "clipboard" | "exec" | "pipe" | "socket" | "file" | "dbus" | "http" | "webhook" | "mcp" | "speak";

  // exec
  command?: string;

  // pipe
  pipe_path?: string;

  // socket (unix or TCP)
  socket_unix?: string;
  socket_host?: string;
  socket_port?: number;

  // file
  file_path?: string;
  file_prefix: string;
  file_timestamp: boolean;
  file_mode: string;        // "append" or "write"

  // dbus
  dbus_signal?: string;

  // http
  http_url?: string;
  http_method: string;

  // webhook (note: webhook_url, not http_url)
  webhook_url?: string;
  webhook_secret?: string;

  // mcp
  mcp_path?: string;
  mcp_tool?: string;

  // chat (OpenAI-compatible /v1/chat/completions, with conversation history)
  chat_url?: string;
  chat_model?: string;
  chat_api_key?: string;
  chat_system_prompt?: string;
  chat_max_history: number;   // default: 20 (0 = send the whole conversation)
  chat_timeout_secs: number;  // default: 120
  chat_reply_mode: string;    // "speak" | "inject" | "clipboard" | "none"
  chat_reset_phrase?: string;

  send_on_release: boolean;   // default: true
  append_newline: boolean;    // default: true
  strip_newlines: boolean;    // default: false
  initial_prompt?: string;

  processing: TargetProcessingConfig;

  response_pipe?: string;
}

interface TargetProcessingConfig {
  remove_fillers?: boolean;
  spoken_punctuation?: boolean;
  auto_format_lists?: boolean;
  apply_snippets?: boolean;
  code_mode?: boolean;
}

interface HotkeyBinding {
  id: string;
  label: string;
  keys: string[];
  gesture: "hold" | "toggle" | "double_tap" | "double_tap_hold";
  target_id: string;
  target_ids: string[];
  tap_ms: number;           // default: 250
  hold_threshold_ms: number;// default: 200
  disabled: boolean;
  openai_enabled?: boolean;       // legacy alias: ollama_enabled
  openai_model?: string;          // legacy alias: ollama_model
  openai_mode?: string;           // legacy alias: ollama_mode
  openai_prompt?: string;         // user prompt template override (must contain "{text}"); legacy alias: ollama_prompt
  openai_system_prompt?: string;  // system prompt override (empty = inherit global default); legacy alias: ollama_system_prompt
}

interface AppStatus {
  recording: boolean;
  processing: boolean;
  speaking: boolean;
  mcp_recording: boolean;
  audio_ready?: boolean;
  word_count: number;
  active_target_id?: string;
  active_target_label?: string;
}
```
