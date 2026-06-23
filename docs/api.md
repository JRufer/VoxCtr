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

### History

#### `get_history() → HistoryEntry[]`
Returns the transcription history (in-memory, current session only).

```typescript
const history = await invoke<HistoryEntry[]>('get_history');
```

```typescript
interface HistoryEntry {
  text: string;
  target_id: string;
  timestamp: string;    // ISO 8601
  inference_ms: number;
}
```

---

#### `clear_history() → void`
Clears the in-memory history log and resets the word count.

```typescript
await invoke('clear_history');
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

### OpenAI API

#### `test_ollama(endpoint: string, timeoutSecs: number, apiKey?: string) → OllamaTestResult`
Pings an OpenAI-API-compatible server (Ollama, LM Studio, OpenAI, etc.) and lists available models.

```typescript
const result = await invoke<OllamaTestResult>('test_ollama', {
  endpoint: 'http://localhost:11434',
  timeoutSecs: 5,
  apiKey: null
});
```

```typescript
interface OllamaTestResult {
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
  ollama: OllamaConfig;
  tts: TtsConfig;
  mcp: McpConfig;
  atspi: AtspiConfig;
}

interface EngineConfig {
  backend: "auto" | "whisper-cpp" | "moonshine";
  inference_mode: "Balanced" | "Aggressive";
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
  min_silence_duration_ms: number;
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
  show_notification: boolean;
  history_enabled: boolean;
}

interface FeaturesConfig {
  remove_fillers: boolean;
  custom_vocabulary: string[];
  spoken_punctuation: boolean;
  auto_format_lists: boolean;
  quiet_mode: boolean;
  snippets: Record<string, string>;
}

interface OllamaConfig {
  enabled: boolean;
  model: string;
  mode: "clean" | "formal" | "casual" | "bullet" | "concise" | "custom";
  custom_prompt: string | null;
  endpoint: string;
  api_key: string | null;
  timeout_secs: number;
}

interface KokoroConfig {
  voice: string;
  quality: string;
  speed: number;
  prewarm: boolean;
  data_dir: string;
}

interface TtsConfig {
  enabled: boolean;
  engine: "piper" | "espeak" | "kokoro";
  voice: string;
  voice_dir: string;
  stop_key: string[];       // singular field name, plural value
  response_overlay: boolean;
  speed: number;
  gpu: boolean;
  kokoro: KokoroConfig;
}

interface McpConfig {
  server_enabled: boolean;  // not "enabled"
  record_timeout: number;
  visual_feedback: boolean;
}

interface AtspiConfig {
  injection: boolean;       // not "enabled"
  context_prompt: boolean;
  auto_code_mode: boolean;
}

interface OutputTarget {
  id: string;
  label: string;
  delivery: "inject" | "clipboard" | "exec" | "pipe" | "socket" | "file" | "dbus" | "http" | "webhook" | "mcp" | "speak" | "openai_api";

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

  // openai_api (uses the global ollama endpoint/api_key/model unless overridden)
  openai_prompt?: string;       // system message; transcribed text is the user message
  openai_model?: string;        // overrides the global model for this target
  openai_max_tokens?: number;
  openai_timeout_secs?: number; // overrides ollama.timeout_secs for this target

  send_on_release: boolean;   // default: true
  append_newline: boolean;    // default: true
  strip_newlines: boolean;    // default: false
  initial_prompt?: string;

  processing: TargetProcessingConfig;

  response_pipe?: string;
}

interface TargetProcessingConfig {
  noise_suppression?: boolean;
  quiet_mode?: boolean;
  atspi_context?: boolean;
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
  gesture: "hold" | "toggle" | "double_tap" | "double_tap_hold" | "chord";
  target_id: string;
  target_ids: string[];
  tap_ms: number;           // default: 250
  hold_threshold_ms: number;// default: 200
  subkey?: string;          // Optional subkey trigger for chord gesture
  disabled: boolean;
  ollama_enabled?: boolean;
  ollama_model?: string;
  ollama_mode?: string;
  ollama_prompt?: string;
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

interface HistoryEntry {
  text: string;
  target_id: string;
  timestamp: string;
  inference_ms: number;
}
```
