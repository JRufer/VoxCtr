import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import EngineTab from "../../src/lib/Settings/EngineTab.svelte";

// Mock tauri invoke & event
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "check_model_downloaded") {
      return args.modelSize === "base"; // mock base downloaded, others missing
    }
    return true;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => {
    return () => {};
  }),
}));

const mockConfig = {
  engine: {
    backend: "whisper-cpp",
    whisper_cpp: {
      model_dir: "",
      model_size: "large-v3", // missing
      device: "auto",
      threads: 0,
    },
    moonshine: {
      model_size: "base",
      language: "en",
    },
  },
} as any;

describe("EngineTab.svelte Warning Banner", () => {
  test("shows warning banner if Whisper voice model is not downloaded", async () => {
    render(EngineTab, { cfg: mockConfig });
    
    // Check if warning title is in document
    const title = await screen.findByText("Voice Model Not Downloaded");
    expect(title).not.toBeNull();
  });

  test("does not show warning banner if Moonshine backend is selected", async () => {
    const moonshineConfig = {
      ...mockConfig,
      engine: {
        ...mockConfig.engine,
        backend: "moonshine",
      },
    };
    render(EngineTab, { cfg: moonshineConfig });
    
    // Warning banner should NOT be in the document
    const title = screen.queryByText("Voice Model Not Downloaded");
    expect(title).toBeNull();
  });
});

describe("EngineTab.svelte Backend selector", () => {
  /** Open the first CustomSelect (Backend) and read its option labels. */
  async function backendOptionLabels(cfg: any) {
    const { container } = render(EngineTab, { cfg });
    const trigger = container.querySelector(".custom-select-trigger") as HTMLElement;
    await fireEvent.click(trigger);
    const menu = trigger.parentElement as HTMLElement;
    return within(menu)
      .getAllByRole("button")
      .map(b => b.textContent?.trim())
      .filter(Boolean);
  }

  test("offers only the concrete backends, with no auto-detect entry", async () => {
    const labels = await backendOptionLabels({ ...mockConfig });

    expect(labels).toContain("Whisper.cpp");
    expect(labels.some(l => /auto/i.test(l!))).toBe(false);
  });

  test("shows the selected backend in the trigger", async () => {
    const { container } = render(EngineTab, { cfg: { ...mockConfig } });
    const trigger = container.querySelector(".custom-select-trigger") as HTMLElement;
    expect(trigger.textContent).toContain("Whisper.cpp");
  });
});

/**
 * GPU support is a property of the build, not of the machine, and the two
 * engines have different answers in the build most people run. These cover the
 * Engine tab reporting that honestly, rather than offering settings nothing
 * downstream can honour.
 */
describe("EngineTab.svelte GPU support", () => {
  /** Answer `accelerator_support` with a given build, keeping the model checks. */
  function buildWith(support: { whisper_gpu: string | null; moonshine_gpu: string | null }) {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "accelerator_support") return support;
      if (cmd === "check_model_downloaded") return args?.modelSize === "base";
      return true;
    });
  }

  /** Open the Device CustomSelect and read its option labels. */
  async function deviceOptionLabels(cfg: any) {
    render(EngineTab, { cfg });
    const label = (await screen.findByText("Device")).closest("label") as HTMLElement;
    const trigger = label.querySelector(".custom-select-trigger") as HTMLElement;
    await fireEvent.click(trigger);
    return within(trigger.parentElement as HTMLElement)
      .getAllByRole("button")
      .map((b) => b.textContent?.trim())
      .filter(Boolean) as string[];
  }

  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  test("offers the one GPU backend this build has, and not the other", async () => {
    buildWith({ whisper_gpu: "vulkan", moonshine_gpu: null });

    const labels = await deviceOptionLabels({
      ...mockConfig,
      engine: { ...mockConfig.engine, whisper_cpp: { ...mockConfig.engine.whisper_cpp } },
    });

    await waitFor(() => expect(labels.some((l) => /vulkan/i.test(l))).toBe(true));
    expect(labels.some((l) => /cuda/i.test(l))).toBe(false);
    expect(labels).toContain("CPU");
  });

  test("offers no GPU at all on a CPU-only build", async () => {
    buildWith({ whisper_gpu: null, moonshine_gpu: null });

    const labels = await deviceOptionLabels({
      ...mockConfig,
      engine: { ...mockConfig.engine, whisper_cpp: { ...mockConfig.engine.whisper_cpp } },
    });

    expect(labels.some((l) => /cuda|vulkan/i.test(l))).toBe(false);
    expect(labels).toContain("CPU");
  });

  /**
   * A config carried over from the CUDA build, opened on the Vulkan one. The
   * value has no entry in the dropdown, so it reads as a working GPU setting
   * while meaning nothing — reset it to the one that does work.
   */
  test("resets a device this build cannot provide", async () => {
    buildWith({ whisper_gpu: "vulkan", moonshine_gpu: null });
    const cfg = {
      ...mockConfig,
      engine: {
        ...mockConfig.engine,
        whisper_cpp: { ...mockConfig.engine.whisper_cpp, device: "cuda" },
      },
    };

    render(EngineTab, { cfg });

    await waitFor(() => expect(cfg.engine.whisper_cpp.device).toBe("auto"));
  });

  test("leaves an explicit CPU choice alone", async () => {
    buildWith({ whisper_gpu: "vulkan", moonshine_gpu: null });
    const cfg = {
      ...mockConfig,
      engine: {
        ...mockConfig.engine,
        whisper_cpp: { ...mockConfig.engine.whisper_cpp, device: "cpu" },
      },
    };

    render(EngineTab, { cfg });

    await new Promise((r) => setTimeout(r, 0));
    expect(cfg.engine.whisper_cpp.device).toBe("cpu");
  });

  /**
   * The gap that let a backend switch quietly cost ~430 MB of RAM: Moonshine
   * has no GPU path in any shipped build, and the tab said nothing about it
   * beside a Device setting that only ever applied to whisper.cpp.
   */
  test("says Moonshine is on the CPU when the build has no provider for it", async () => {
    buildWith({ whisper_gpu: "vulkan", moonshine_gpu: null });

    render(EngineTab, {
      cfg: { ...mockConfig, engine: { ...mockConfig.engine, backend: "moonshine" } },
    });

    expect(await screen.findByText("Moonshine runs on the CPU in this build")).toBeTruthy();
  });

  test("says nothing of the sort when Moonshine does have a provider", async () => {
    buildWith({ whisper_gpu: "cuda", moonshine_gpu: "cuda" });

    render(EngineTab, {
      cfg: { ...mockConfig, engine: { ...mockConfig.engine, backend: "moonshine" } },
    });

    // The notice is the pre-answer state too, so this has to wait for the
    // build report to land rather than sample before it does.
    await waitFor(() =>
      expect(screen.queryByText("Moonshine runs on the CPU in this build")).toBeNull(),
    );
  });
});
