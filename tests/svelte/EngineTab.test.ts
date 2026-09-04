import { describe, test, expect, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
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
