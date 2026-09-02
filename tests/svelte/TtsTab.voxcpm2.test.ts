import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, cleanup } from "@testing-library/svelte";
import { get } from "svelte/store";
import TtsTab from "../../src/lib/Settings/TtsTab.svelte";
import { config } from "../../src/stores/config";
import type { AppConfig } from "../../src/stores/config";

// What the backend's `voxcpm2_status` command reports. Each test rewrites this
// before rendering to stand in for a different machine state.
let mockVoxcpmStatus = {
  compiled: true,
  backend: "GPU (wgpu: Vulkan / Metal / DX12)",
  ready: true,
  missing: [] as string[],
  model_dir: "/home/u/.local/share/voxctrl/models/voxcpm2",
};

let downloadCalls: Array<Record<string, unknown>> = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown>) => {
    // The config store calls `get_config` at import time and overwrites itself
    // with the result. Failing the call leaves it holding the app's own
    // defaults, which is exactly what these tests want to assert against.
    if (cmd === "get_config") throw new Error("no backend in tests");
    if (cmd === "voxcpm2_status") return mockVoxcpmStatus;
    if (cmd === "download_voxcpm2") {
      downloadCalls.push(args);
      return null;
    }
    if (cmd === "list_pocket_tts_voices") {
      return [
        { id: "alba", label: "Alba (Female)" },
        { id: "my_voice", label: "My Voice (Custom)" },
      ];
    }
    if (cmd === "check_voice_downloaded") return true;
    if (cmd === "check_directory_exists") return true;
    if (cmd === "inflect_micro_available") return true;
    return false;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

// Snapshot of the app's own defaults, taken before any test renders anything.
// The tab writes edits straight back into the shared store, so cloning the
// *live* store would let one test's edits become the next test's starting point.
const PRISTINE = structuredClone(get(config)) as AppConfig;

/// Building from the shipped defaults keeps these tests honest about what
/// actually ships rather than restating a config by hand.
function configWithVoxcpm(overrides: Partial<AppConfig["tts"]["voxcpm2"]> = {}): AppConfig {
  const base = structuredClone(PRISTINE) as AppConfig;
  base.tts.enabled = true;
  base.tts.engine = "voxcpm2";
  base.tts.voxcpm2 = { ...base.tts.voxcpm2, ...overrides };
  return base;
}

describe("TtsTab VoxCPM2 engine section", () => {
  afterEach(() => {
    // Each test renders the tab again; without this the previous render's DOM
    // stays mounted and text queries match two copies of every element.
    cleanup();
    config.set(structuredClone(PRISTINE) as AppConfig);
  });

  beforeEach(() => {
    downloadCalls = [];
    mockVoxcpmStatus = {
      compiled: true,
      backend: "GPU (wgpu: Vulkan / Metal / DX12)",
      ready: true,
      missing: [],
      model_dir: "/home/u/.local/share/voxctrl/models/voxcpm2",
    };
  });

  test("ships VoxCPM2 as a selectable engine with both voice modes", () => {
    const cfg = configWithVoxcpm();
    render(TtsTab, { props: { cfg } });

    expect(screen.getByText("VoxCPM2 Voice")).toBeTruthy();
    expect(screen.getByText("🗣️ Voice Design (Prompt)")).toBeTruthy();
    expect(screen.getByText("🎙️ Voice Cloning (Shared Folder)")).toBeTruthy();
  });

  test("design mode shows the voice prompt and no reference clip picker", () => {
    const cfg = configWithVoxcpm({ voice_mode: "design" });
    render(TtsTab, { props: { cfg } });

    expect(screen.getByText("Speaker Voice Prompt (Voice Design)")).toBeTruthy();
    // The clip picker belongs to clone mode only; showing both at once would
    // imply the design prompt is conditioning a cloned voice.
    expect(screen.queryByText("Cloned Voice Reference Clip")).toBeNull();
  });

  test("clone mode swaps the prompt for a clip picker and a style instruction", async () => {
    // Rendered in clone mode rather than toggled into it: the tab writes the
    // mode back through a `$bindable` prop that only becomes reactive when the
    // parent owns it as `$state`, so a toggle here would test the harness.
    const cfg = configWithVoxcpm({ voice_mode: "clone", cloned_voice: "alba" });
    render(TtsTab, { props: { cfg } });

    expect(await screen.findByText("Cloned Voice Reference Clip")).toBeTruthy();
    expect(screen.getByText("Style Instruction (optional)")).toBeTruthy();
    // The design prompt must not linger: it would suggest the description is
    // conditioning the cloned voice, when only the style instruction applies.
    expect(screen.queryByText("Speaker Voice Prompt (Voice Design)")).toBeNull();
  });

  test("clone mode explains the optional transcript file", async () => {
    const cfg = configWithVoxcpm({ voice_mode: "clone", cloned_voice: "alba" });
    render(TtsTab, { props: { cfg } });

    expect(await screen.findByText(/Voice Cloning Transcript Requirement/)).toBeTruthy();
  });

  test("reports the compute backend the build will actually use", async () => {
    const cfg = configWithVoxcpm();
    render(TtsTab, { props: { cfg } });

    await waitFor(() => {
      expect(screen.getByText("GPU (wgpu: Vulkan / Metal / DX12)")).toBeTruthy();
    });
  });

  test("names the missing files when the checkpoint is incomplete", async () => {
    mockVoxcpmStatus = {
      ...mockVoxcpmStatus,
      ready: false,
      missing: ["model.safetensors or model.pth or model.pt"],
    };
    const cfg = configWithVoxcpm();
    render(TtsTab, { props: { cfg } });

    expect(await screen.findByText("❌ Model files missing")).toBeTruthy();
    // A half-finished download should say what is left, not just "not ready".
    expect(
      await screen.findByText(/Missing: model\.safetensors or model\.pth or model\.pt/),
    ).toBeTruthy();
  });

  test("Test TTS is blocked until the checkpoint is downloaded", async () => {
    mockVoxcpmStatus = { ...mockVoxcpmStatus, ready: false, missing: ["config.json"] };
    const cfg = configWithVoxcpm();
    render(TtsTab, { props: { cfg } });

    await waitFor(() => {
      const button = screen.getByText("Test TTS") as HTMLButtonElement;
      expect(button.disabled).toBe(true);
    });
  });

  test("Test TTS is blocked in clone mode until a reference clip is chosen", async () => {
    // Cloning with no clip selected would otherwise fail inside the engine,
    // after the user has already waited for generation to start.
    const cfg = configWithVoxcpm({ voice_mode: "clone", cloned_voice: "" });
    render(TtsTab, { props: { cfg } });

    // The reason only settles once `voxcpm2_status` reports the checkpoint is
    // present; before that the blocking reason is the missing download.
    expect(await screen.findByText(/Pick a reference clip to clone/)).toBeTruthy();
    const button = screen.getByText("Test TTS") as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  test("a build without the inference feature says so instead of failing later", async () => {
    mockVoxcpmStatus = { ...mockVoxcpmStatus, compiled: false, backend: "not compiled in" };
    const cfg = configWithVoxcpm();
    render(TtsTab, { props: { cfg } });

    await waitFor(() => {
      // Stated twice on purpose: once as a banner on the engine section, and
      // once as the reason the Test TTS button is greyed out.
      expect(screen.getAllByText(/compiled without the/).length).toBeGreaterThan(0);
    });
    const button = screen.getByText("Test TTS") as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  test("download passes the configured directory and repository through", async () => {
    mockVoxcpmStatus = { ...mockVoxcpmStatus, ready: false, missing: ["config.json"] };
    const cfg = configWithVoxcpm({ model_dir: "/tmp/vox", model_repo: "openbmb/VoxCPM2" });
    render(TtsTab, { props: { cfg } });

    const button = (await screen.findByText("📥 Download")) as HTMLButtonElement;
    button.click();

    await waitFor(() => {
      expect(downloadCalls.length).toBe(1);
    });
    expect(downloadCalls[0]).toMatchObject({
      modelDir: "/tmp/vox",
      repo: "openbmb/VoxCPM2",
    });
  });

  test("the chunk slider states the audio-per-chunk it controls", () => {
    // One patch is ~80 ms of audio, so the label has to move with the slider.
    const cfg = configWithVoxcpm({ chunk_patches: 3 });
    render(TtsTab, { props: { cfg } });

    expect(screen.getByText("Chunk Size (3 patches ≈ 240 ms)")).toBeTruthy();
  });

  test("the lead buffer is exposed as the fix for stalling speech", () => {
    // Choppy playback is a starved audio sink, so the control that cures it has
    // to be reachable and has to say what it is for.
    const cfg = configWithVoxcpm({ prebuffer_ms: 800 });
    render(TtsTab, { props: { cfg } });

    expect(screen.getByText("Lead Buffer (800 ms)")).toBeTruthy();
    expect(screen.getByText(/speech stalls or breaks up part-way through, raise this/)).toBeTruthy();
  });

  test("defaults are the low-latency ones the engine needs", () => {
    const defaults = PRISTINE.tts.voxcpm2;
    expect(defaults.prewarm).toBe(true);
    expect(defaults.chunk_patches).toBe(4);
    expect(defaults.inference_timesteps).toBe(6);
    // Playback must not start on the first chunk: the sink drains in real time
    // and would starve before the next chunk arrived.
    expect(defaults.prebuffer_ms).toBe(400);
    expect(defaults.voice_mode).toBe("design");
  });
});
