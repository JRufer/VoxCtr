import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/svelte";
import Settings from "../../src/lib/Settings/Settings.svelte";
import { config, configLoaded } from "../../src/stores/config";

// Mock tauri invoke & event
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "check_model_downloaded") {
      // Mock 'large-v3' is missing (returns false)
      return false;
    }
    if (cmd === "list_audio_devices") {
      return [];
    }
    return {};
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => {
    return () => {};
  }),
}));

const mockShow = vi.fn();
const mockFocus = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    show: mockShow,
    setFocus: mockFocus,
  })),
}));

describe("Settings.svelte Startup Redirect", () => {
  beforeEach(() => {
    config.set({
      engine: {
        backend: "whisper-cpp",
        inference_mode: "Balanced",
        whisper_cpp: {
          model_dir: "",
          model_size: "large-v3", // missing model triggers redirect
          device: "auto",
          threads: 0,
        },
        moonshine: {
          model_size: "base",
          language: "en",
        },
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
      ollama: {
        enabled: false,
        model: "llama3.2:1b",
        mode: "clean",
        custom_prompt: null,
        endpoint: "http://localhost:11434",
        timeout_secs: 8,
      },
      tts: {
        enabled: false,
        engine: "piper",
        voice: "en-us-lessac-medium",
        stop_key: ["KEY_ESCAPE"],
        response_overlay: true,
      },
      mcp: {
        server_enabled: false,
        record_timeout: 15.0,
      },
      atspi: {
        injection: true,
        context_prompt: true,
        auto_code_mode: true,
      },
    } as any);
    configLoaded.set(true);
  });

  test("automatically selects Engine tab and forces window show/focus on mount when voice model is not downloaded", async () => {
    mockShow.mockClear();
    mockFocus.mockClear();

    render(Settings);
    
    // Check if the "Engine" tab is active/selected
    const engineHeader = await screen.findByText("Inference Engine");
    expect(engineHeader).not.toBeNull();

    // Verify window was shown and focused (waiting for async import and tasks to settle)
    await vi.waitFor(() => {
      expect(mockShow).toHaveBeenCalled();
      expect(mockFocus).toHaveBeenCalled();
    });
  });
});
