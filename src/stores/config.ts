import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface AppConfig {
  engine: EngineConfig;
  audio: AudioConfig;
  ui: UiConfig;
  features: FeaturesConfig;
  openai: OpenAiConfig;
  tts: TtsConfig;
  mcp: McpConfig;
  atspi: AtspiConfig;
}

export interface EngineConfig {
  backend: "auto" | "whisper-cpp" | "moonshine";
  inference_mode: "Balanced" | "Aggressive";
  whisper_cpp: WhisperCppConfig;
  moonshine: MoonshineConfig;
}

export interface WhisperCppConfig {
  model_dir: string;
  model_size: string;
  device: string;
  threads: number;
}

export interface MoonshineConfig {
  model_size: string;
  language: string;
}

export interface AudioConfig {
  vad_threshold: number;
  min_silence_duration_ms: number;
  input_device_index: number | null;
  evdev_device: string | null;
  noise_suppression: boolean;
  gain: number;
  dynamic_stream: boolean;
}

export interface UiConfig {
  show_overlay: boolean;
  overlay_style: string;
  overlay_position: string;
  overlay_monitor: string;
  auto_show_settings: boolean;
  show_notification: boolean;
  history_enabled: boolean;
}

export interface FeaturesConfig {
  remove_fillers: boolean;
  custom_vocabulary: string[];
  spoken_punctuation: boolean;
  auto_format_lists: boolean;
  quiet_mode: boolean;
  snippets: Record<string, string>;
}

export interface OpenAiConfig {
  enabled: boolean;
  model: string;
  mode: "clean" | "formal" | "casual" | "bullet" | "concise" | "custom";
  custom_prompt: string | null;
  system_prompt: string;
  user_prompt: string;
  endpoint: string;
  api_key: string | null;
  timeout_secs: number;
}

export interface PocketTtsConfig {
  voice: string;
  prewarm: boolean;
  hf_token: string | null;
}

export interface TtsConfig {
  enabled: boolean;
  engine: "piper" | "espeak" | "pocket_tts";
  voice: string;
  voice_dir: string;
  stop_key: string[];
  response_overlay: boolean;
  speed: number;
  gpu: boolean;
  pocket_tts: PocketTtsConfig;
}

export interface McpConfig {
  server_enabled: boolean;
  record_timeout: number;
  visual_feedback: boolean;
}

export interface AtspiConfig {
  injection: boolean;
  context_prompt: boolean;
  auto_code_mode: boolean;
}

const defaultConfig: AppConfig = {
  engine: {
    backend: "auto",
    inference_mode: "Balanced",
    whisper_cpp: {
      model_dir: "",
      model_size: "large-v3",
      device: "auto",
      threads: 0,
    },
    moonshine: { model_size: "base", language: "en" },
  },
  audio: {
    vad_threshold: 0.5,
    min_silence_duration_ms: 500,
    input_device_index: null,
    evdev_device: null,
    noise_suppression: false,
    gain: 1.0,
    dynamic_stream: true,
  },
  ui: {
    show_overlay: true,
    overlay_style: "mono_bars",
    overlay_position: "center",
    overlay_monitor: "primary",
    auto_show_settings: true,
    show_notification: false,
    history_enabled: false,
  },
  features: {
    remove_fillers: true,
    custom_vocabulary: [],
    spoken_punctuation: true,
    auto_format_lists: true,
    quiet_mode: false,
    snippets: {},
  },
  openai: {
    enabled: false,
    model: "llama3.2:1b",
    mode: "clean",
    custom_prompt: null,
    system_prompt: "Fix grammar and punctuation only. Return only the corrected text, no commentary.",
    user_prompt: "{text}",
    endpoint: "http://localhost:11434",
    api_key: null,
    timeout_secs: 8,
  },
  tts: {
    enabled: false,
    engine: "piper",
    voice: "en-us-lessac-medium",
    voice_dir: "",
    stop_key: ["KEY_ESCAPE"],
    response_overlay: true,
    speed: 1.0,
    gpu: false,
    pocket_tts: {
      voice: "alba",
      prewarm: false,
      hf_token: null,
    },
  },
  mcp: { server_enabled: false, record_timeout: 15.0, visual_feedback: true },
  atspi: { injection: true, context_prompt: true, auto_code_mode: true },
};

export const config = writable<AppConfig>(defaultConfig);
export const configDirty = writable(false);
export const configLoaded = writable(false);

let isLoaded = false;
let saveTimeout: any = null;

export async function loadConfig() {
  try {
    const loaded = await invoke<AppConfig>("get_config");
    config.set(loaded);
    configDirty.set(false);
    configLoaded.set(true);
    setTimeout(() => {
      isLoaded = true;
    }, 0);
  } catch (e) {
    console.error("loadConfig:", e);
    configLoaded.set(true);
    setTimeout(() => {
      isLoaded = true;
    }, 0);
  }
}

export async function saveConfig(cfg: AppConfig) {
  await invoke("save_config", { newConfig: cfg });
  configDirty.set(false);
}

config.subscribe((cfg) => {
  if (!isLoaded) return;
  
  if (saveTimeout) clearTimeout(saveTimeout);
  saveTimeout = setTimeout(async () => {
    try {
      await saveConfig(cfg);
      console.log("Config auto-saved successfully!");
    } catch (e) {
      console.error("Auto-saving config failed:", e);
    }
  }, 400);
});

loadConfig();

// Listen for config-changed events from other windows or the backend
// to keep the in-memory store synchronized without circular auto-save feedback loops
listen<AppConfig>("config-changed", (event) => {
  isLoaded = false;
  config.set(event.payload);
  setTimeout(() => {
    isLoaded = true;
  }, 0);
}).catch((e) => {
  console.error("Failed to setup config-changed listener:", e);
});

