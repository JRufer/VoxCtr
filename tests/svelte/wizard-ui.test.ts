import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/svelte";

// Hoisted: the config and status stores call invoke() at import time, which
// happens before a plain `const` in this file would be initialised.
const { invoke, listeners } = vi.hoisted(() => ({
  // Always thenable: the stores call invoke() during module import, before any
  // test has had a chance to install its own implementation.
  invoke: vi.fn(async () => undefined),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return () => listeners.delete(name);
  }),
}));

import { config } from "../../src/stores/config";
import { status } from "../../src/stores/status";
import { wizard } from "../../src/lib/Wizard/wizard-state.svelte";
import SetupWizard from "../../src/lib/Wizard/SetupWizard.svelte";
import EngineStep from "../../src/lib/Wizard/steps/EngineStep.svelte";
import HotkeyStep from "../../src/lib/Wizard/steps/HotkeyStep.svelte";
import OverlayStep from "../../src/lib/Wizard/steps/OverlayStep.svelte";
import VoiceStep from "../../src/lib/Wizard/steps/VoiceStep.svelte";
import DoneStep from "../../src/lib/Wizard/steps/DoneStep.svelte";
import TestStep from "../../src/lib/Wizard/steps/TestStep.svelte";
import { OVERLAY_STYLES } from "../../src/lib/Wizard/wizard-data";

/** Minimal but complete-enough config for the wizard's reads. */
function baseConfig(): any {
  return {
    engine: {
      backend: "whisper-cpp",
      whisper_cpp: { model_dir: "", model_size: "small", device: "auto", threads: 0 },
      moonshine: { model_size: "base", language: "en" },
    },
    audio: {},
    ui: {
      show_overlay: true,
      overlay_style: "mono_bars",
      overlay_position: "center",
      overlay_monitor: "primary",
      setup_completed: false,
    },
    features: {},
    openai: {},
    tts: {
      enabled: false,
      engine: "piper",
      voice: "en-us-lessac-medium",
      voice_dir: "",
      hf_token: null,
      pocket_tts: { voice: "alba", voice_dir: "" },
      inflect_micro: { model_dir: "" },
      breeze_tts_2: { model_dir: "" },
    },
    mcp: {},
  };
}

function hotkeyStatus(over: Record<string, unknown> = {}) {
  return {
    is_active: true,
    backend: "evdev",
    is_private: false,
    portal_error: null,
    portal_refused: false,
    shortcuts: [],
    supported_gestures: ["hold", "toggle", "double_tap", "double_tap_hold"],
    ...over,
  };
}

const noopGate = () => {};
const noopBlocker = () => {};

/** The shape the status tick really delivers. */
function baseStatus(over: Record<string, unknown> = {}): any {
  return {
    recording: false,
    processing: false,
    speaking: false,
    mcp_recording: false,
    audio_ready: true,
    word_count: 0,
    ...over,
  };
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(async () => undefined);
  listeners.clear();
  wizard.reset();
  config.set(baseConfig());
  status.set(baseStatus());
});

describe("SetupWizard shell", () => {
  beforeEach(() => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_config") return baseConfig();
      return undefined;
    });
  });

  test("opens on the welcome screen and counts the steps", async () => {
    render(SetupWizard);
    expect(await screen.findByText(/Speak to/)).toBeTruthy();
    expect(screen.getByText(/step 1 \/ 7/)).toBeTruthy();
  });

  test("offers a way out that still leaves a configured app behind", async () => {
    render(SetupWizard);
    const skip = await screen.findByText("Skip setup");
    await fireEvent.click(skip);
    await waitFor(() =>
      expect(invoke.mock.calls.some(([c]) => c === "finish_setup_wizard")).toBe(true),
    );
  });

  test("finishing marks setup complete so the wizard does not reappear", async () => {
    wizard.step = 6;
    wizard.visited = 6;
    render(SetupWizard);
    const finish = await screen.findByText("Finish");
    await fireEvent.click(finish);
    await waitFor(() => {
      const call = invoke.mock.calls.find(([c]) => c === "finish_setup_wizard");
      expect(call).toBeTruthy();
      expect((call as any)[1]).toEqual({ openSettings: false });
    });
  });

  test("the welcome cards preview the steps without jumping to them", async () => {
    // A wizard whose contents page is also a menu is not a wizard: the live
    // test has nothing to test until an engine and a hotkey exist, so landing
    // there directly lands on a screen that cannot work.
    render(SetupWizard);

    const card = (await screen.findByText("Live test")).closest(".step-card") as HTMLElement;
    expect(card).toBeTruthy();
    expect(card.tagName).toBe("DIV");
    expect(card.querySelector("button")).toBeNull();

    await fireEvent.click(card);
    await new Promise((r) => setTimeout(r, 300));
    expect(wizard.step).toBe(0);
  });

  test("Back returns to the previous step", async () => {
    wizard.step = 3;
    wizard.visited = 3;
    render(SetupWizard);

    await fireEvent.click(await screen.findByText("← Back"));

    await waitFor(() => expect(wizard.step).toBe(2), { timeout: 2000 });
  });

  test("Back is offered on every step after the first, and never on it", async () => {
    wizard.step = 0;
    render(SetupWizard);
    expect(await screen.findByText("Skip setup")).toBeTruthy();
    expect(screen.queryByText("← Back")).toBeNull();
  });

  test("says 'Finish anyway' when problems were logged along the way", async () => {
    wizard.step = 6;
    wizard.visited = 6;
    wizard.recordIssue({ id: "x", step: 1, title: "broken", detail: "detail" });
    render(SetupWizard);
    expect(await screen.findByText("Finish anyway")).toBeTruthy();
  });
});

describe("EngineStep", () => {
  test("says the chosen model will download before the next step", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_model_downloaded") return false;
      if (cmd === "check_moonshine_downloaded") return false;
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return undefined;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });
    await fireEvent.click((await screen.findAllByRole("radio"))[0]);
    await fireEvent.click((await screen.findAllByText("small"))[0].closest("button")!);
    expect(await screen.findByText(/small will download when you continue/)).toBeTruthy();
  });

  test("confirms a model that is already on disk instead of re-fetching it", async () => {
    invoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "check_model_downloaded") return args.modelSize === "small";
      if (cmd === "check_moonshine_downloaded") return false;
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return undefined;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });
    expect(await screen.findByText(/small is on disk and ready/)).toBeTruthy();
  });

  test("picking an engine marks that card selected on screen", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });

    const cards = await screen.findAllByRole("radio");
    const [whisper, moonshine] = cards;
    expect(whisper.getAttribute("aria-checked")).toBe("true");

    await fireEvent.click(moonshine);

    // The card has to visibly change, not just the config underneath it.
    await waitFor(() => expect(moonshine.getAttribute("aria-checked")).toBe("true"));
    expect(whisper.getAttribute("aria-checked")).toBe("false");
  });

  test("picking a model size highlights that size on screen", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });

    const button = (await screen.findByText("medium")).closest("button") as HTMLElement;
    await fireEvent.click(button);

    await waitFor(() => expect(button.classList.contains("on")).toBe(true));
  });

  test("the GPU toggle flips, and says which path it will use", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return true;
      return false;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });

    const toggle = (await screen.findByText("GPU offloading")).closest("button") as HTMLElement;
    await waitFor(() => expect(toggle.textContent).toContain("ON · CUDA"));

    await fireEvent.click(toggle);
    await waitFor(() => expect(toggle.textContent).toContain("OFF · CPU"));

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.engine.whisper_cpp.device).toBe("cpu");

    await fireEvent.click(toggle);
    await waitFor(() => expect(toggle.textContent).toContain("ON · CUDA"));
  });

  test("an already-downloaded model needs no click before continuing", async () => {
    // The gate is there to stop an unwanted download. There is nothing to
    // download here, so making the user click their own existing choice would
    // be a ceremony that protects nobody.
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "check_model_downloaded") return args.modelSize === "small";
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, r: string | null) => blockers.push(r),
    });

    await waitFor(() => expect(blockers.at(-1)).toBeNull());
    expect(wizard.engineChosen).toBe(false);
    expect(wizard.modelChosen).toBe(false);
  });

  test("switching to a model that is not downloaded brings the gate back", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "check_model_downloaded") return args.modelSize === "small";
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, r: string | null) => blockers.push(r),
    });
    await waitFor(() => expect(blockers.at(-1)).toBeNull());

    // Picking "medium" is itself an explicit choice, so the gate is satisfied
    // and the pill switches to warning about the download instead.
    await fireEvent.click((await screen.findByText("medium")).closest("button") as HTMLElement);
    await waitFor(() =>
      expect(screen.getByText(/medium will download when you continue/)).toBeTruthy(),
    );
    expect(blockers.at(-1)).toBeNull();
  });

  test("waits for the on-disk check before deciding anything", async () => {
    const blockers: (string | null)[] = [];
    let releaseCheck: (v: boolean) => void = () => {};
    const pending = new Promise<boolean>((r) => (releaseCheck = r));
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_model_downloaded") return pending;
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, r: string | null) => blockers.push(r),
    });

    // Both readiness maps start empty; deciding from them would call every
    // model missing and demand a pointless click.
    await waitFor(() => expect(blockers.at(-1)).toMatch(/Checking which models/));
    releaseCheck(true);
    await waitFor(() => expect(blockers.at(-1)).toBeNull());
  });

  test("will not move on until an engine and a model size are actually chosen", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, r: string | null) => blockers.push(r),
    });

    // The config ships with a backend and a size already set, so an untouched
    // screen must still count as "nothing chosen".
    await waitFor(() => expect(blockers.at(-1)).toMatch(/Choose a transcription engine/));

    await fireEvent.click((await screen.findAllByRole("radio"))[0]);
    await waitFor(() => expect(blockers.at(-1)).toMatch(/Choose a model size/));

    await fireEvent.click((await screen.findAllByText("small"))[0].closest("button") as HTMLElement);
    await waitFor(() => expect(blockers.at(-1)).toBeNull());
  });

  test("picking a size counts as picking its engine too", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, r: string | null) => blockers.push(r),
    });

    const whisperCard = (await screen.findAllByRole("radio"))[0];
    await fireEvent.click(within(whisperCard).getByText("tiny").closest("button") as HTMLElement);
    await waitFor(() => expect(blockers.at(-1)).toBeNull());
  });

  test("picking a model size writes it straight into the config", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });

    const medium = await screen.findByText("medium");
    await fireEvent.click(medium);

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.engine.whisper_cpp.model_size).toBe("medium");
    expect(current.engine.backend).toBe("whisper-cpp");
  });

  test("warns when Moonshine was not compiled into this build", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return false;
      if (cmd === "cuda_enabled") return false;
      return false;
    });
    render(EngineStep, { registerGate: noopGate, setBlocker: noopBlocker });
    expect(await screen.findByText(/compiled without the Moonshine backend/)).toBeTruthy();
  });

  test("the download gate fetches the model and records the failure if it fails", async () => {
    let gate: (() => Promise<boolean>) | null = null;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "moonshine_available") return true;
      if (cmd === "cuda_enabled") return false;
      if (cmd === "download_model") throw new Error("network unreachable");
      return false;
    });
    render(EngineStep, {
      registerGate: (_step: number, g: any) => (gate = g ?? gate),
      setBlocker: noopBlocker,
    });

    await waitFor(() => expect(gate).toBeTruthy());
    const ok = await gate!();

    expect(ok).toBe(false);
    expect(wizard.issues.map((i) => i.id)).toEqual(["model-download"]);
    expect(wizard.issues[0].detail).toContain("network unreachable");
  });
});

describe("HotkeyStep", () => {
  test("hides gestures this desktop's shortcut backend cannot deliver", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") return hotkeyStatus({ supported_gestures: ["toggle"] });
      if (cmd === "get_bindings") return [];
      return undefined;
    });
    render(HotkeyStep, { registerGate: noopGate, setBlocker: noopBlocker });

    expect(await screen.findByText("Tap to talk")).toBeTruthy();
    expect(screen.queryByText("Hold to talk")).toBeNull();
    expect(screen.queryByText("Double-tap & hold")).toBeNull();
    expect(screen.getByText(/3 gestures hidden/)).toBeTruthy();
  });

  test("falls back to a supported gesture when the default is unavailable", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") return hotkeyStatus({ supported_gestures: ["toggle"] });
      if (cmd === "get_bindings") return [];
      return undefined;
    });
    render(HotkeyStep, { registerGate: noopGate, setBlocker: noopBlocker });
    await waitFor(() => expect(wizard.gesture).toBe("toggle"));
  });

  test("blocks the way forward until a combination is recorded", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") return hotkeyStatus();
      if (cmd === "get_bindings") return [];
      return undefined;
    });
    render(HotkeyStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, reason: string | null) => blockers.push(reason),
    });
    await waitFor(() => expect(blockers.length).toBeGreaterThan(0));
    expect(blockers.at(-1)).toMatch(/Record a key combination/);
  });

  test("blocks the way forward while the desktop has not accepted the shortcut", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") {
        return hotkeyStatus({ is_active: false, backend: "portal", portal_refused: true });
      }
      if (cmd === "get_bindings") {
        return [{ id: "default_hold", keys: ["KEY_LEFTMETA", "KEY_SPACE"], gesture: "hold" }];
      }
      return undefined;
    });
    render(HotkeyStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, reason: string | null) => blockers.push(reason),
    });

    await waitFor(() => expect(blockers.at(-1)).toMatch(/Register the shortcut/));
  });

  test("lets the user through once the shortcut is live", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") return hotkeyStatus({ is_active: true });
      if (cmd === "get_bindings") {
        return [{ id: "default_hold", keys: ["KEY_LEFTMETA", "KEY_SPACE"], gesture: "hold" }];
      }
      return undefined;
    });
    render(HotkeyStep, {
      registerGate: noopGate,
      setBlocker: (_s: number, reason: string | null) => blockers.push(reason),
    });

    await waitFor(() => expect(blockers.at(-1)).toBeNull());
    expect(await screen.findByText(/Shortcuts are live/)).toBeTruthy();
  });

  test("recording a valid combination saves a binding pointed at the Command target", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") return hotkeyStatus();
      if (cmd === "get_bindings") return [];
      if (cmd === "get_targets") {
        return [{ id: "default", label: "Focused Window", delivery: "inject" }];
      }
      if (cmd === "check_hotkey_keys") return { accepted: true, enforced: true, accelerator: null, problem: null, message: null };
      return undefined;
    });
    render(HotkeyStep, { registerGate: noopGate, setBlocker: noopBlocker });

    const recorder = await screen.findByText("click to record");
    await fireEvent.click(recorder);

    await fireEvent.keyDown(window, { key: "Alt", code: "AltLeft" });
    await fireEvent.keyDown(window, { key: "v", code: "KeyV" });
    await fireEvent.keyUp(window, { key: "v", code: "KeyV" });

    await waitFor(() => {
      const call = invoke.mock.calls.find(([c]) => c === "save_bindings");
      expect(call).toBeTruthy();
      const bindings = (call as any)[1].bindings;
      expect(bindings[0].keys).toEqual(["KEY_LEFTALT", "KEY_V"]);
      expect(bindings[0].target_id).toBe("command");
    });
  });

  test("a combination the backend refuses is explained, not silently accepted", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "check_hotkey_status") return hotkeyStatus();
      if (cmd === "get_bindings") return [];
      if (cmd === "check_hotkey_keys") {
        return {
          accepted: false,
          enforced: true,
          accelerator: null,
          problem: "modifiers_only",
          message: "Modifiers alone cannot be a shortcut.",
        };
      }
      return undefined;
    });
    render(HotkeyStep, { registerGate: noopGate, setBlocker: noopBlocker });

    await fireEvent.click(await screen.findByText("click to record"));
    await fireEvent.keyDown(window, { key: "Control", code: "ControlLeft" });
    await fireEvent.keyUp(window, { key: "Control", code: "ControlLeft" });

    expect(await screen.findByText("Modifiers alone cannot be a shortcut.")).toBeTruthy();
    expect(wizard.combo).toBeNull();
    expect(invoke.mock.calls.some(([c]) => c === "save_bindings")).toBe(false);
  });
});

describe("OverlayStep", () => {
  beforeEach(() => {
    invoke.mockImplementation(async () => undefined);
  });

  test("picking a style writes the id Settings → Visual uses", async () => {
    render(OverlayStep);
    await fireEvent.click(await screen.findByText("Retro Terminal"));

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.ui.overlay_style).toBe("terminal");
    expect(current.ui.show_overlay).toBe(true);
  });

  test("picking a position writes it to the config", async () => {
    render(OverlayStep);
    await fireEvent.click(await screen.findByText("Bottom"));

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.ui.overlay_position).toBe("bottom");
  });

  test("turning the overlay off is saved as well", async () => {
    render(OverlayStep);
    await fireEvent.click(await screen.findByText("No overlay"));

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.ui.show_overlay).toBe(false);
  });
});

describe("OverlayPreview", () => {
  // The clips are resolved from src/assets/overlays at build time, so a style
  // whose file is missing or renamed silently loses its clip. Every style must
  // still render something — a black rectangle where a preview should be is
  // worse than no preview at all.
  test("every style renders a preview, clip or fallback", async () => {
    invoke.mockImplementation(async () => undefined);
    const { container } = render(OverlayStep);

    const thumbs = container.querySelectorAll(".thumb");
    expect(thumbs.length).toBe(OVERLAY_STYLES.length);
    for (const thumb of thumbs) {
      // Either a clip or the CSS fallback, never an empty box.
      const hasSomething =
        thumb.querySelector("video") !== null || thumb.querySelector(".preview") !== null;
      expect(hasSomething).toBe(true);
    }
  });

  test("a bundled clip is muted, looping and autoplaying, as a silent UI animation must be", async () => {
    invoke.mockImplementation(async () => undefined);
    const { container } = render(OverlayStep);

    for (const video of container.querySelectorAll("video")) {
      expect(video.hasAttribute("muted") || (video as HTMLVideoElement).muted).toBe(true);
      expect(video.hasAttribute("loop")).toBe(true);
      expect(video.hasAttribute("autoplay")).toBe(true);
      // Hidden until it can actually paint a frame.
      expect(video.classList.contains("ready")).toBe(false);
    }
  });
});

describe("VoiceStep", () => {
  test("a voice that is not downloaded cannot be auditioned", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    const play = await screen.findAllByTitle("Download this voice first");
    expect(play.length).toBeGreaterThan(0);
    expect((play[0] as HTMLButtonElement).disabled).toBe(true);
  });

  test("downloading a voice enables its play button", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "download_voice") return undefined;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    const button = await screen.findByText(/Download 60 MB/);
    await fireEvent.click(button);

    await waitFor(() => expect(screen.getAllByTitle("Play a sample").length).toBeGreaterThan(0));
  });

  test("playing a sample saves the engine choice and speaks through the app", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "check_voice_downloaded") return true;
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });

    // Several engines are playable at once (eSpeak needs no download), so the
    // sample has to be started from the Piper card specifically.
    expect(container).toBeTruthy();
    const piperCard = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
    expect(piperCard).toBeTruthy();
    // The readiness probe is async, so the card starts out un-playable.
    const play = await waitFor(() => within(piperCard).getByTitle("Play a sample"));
    await fireEvent.click(play);

    await waitFor(() => {
      const speak = invoke.mock.calls.find(([c]) => c === "speak_text");
      expect(speak).toBeTruthy();
      expect((speak as any)[1].text).toContain("Piper TTS");
      expect((speak as any)[1].voice).toBe("en-us-lessac-medium");
    });

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.tts.enabled).toBe(true);
    expect(current.tts.engine).toBe("piper");
  });

  test("gated voices are locked, and say why, until a token is entered", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    for (const name of ["Breeze-TTS-2", "Pocket TTS"]) {
      const card = (await screen.findByText(name)).closest(".card") as HTMLElement;
      expect(card.classList.contains("locked")).toBe(true);
      expect(card.textContent).toContain("Needs a HuggingFace access token");
      const dl = within(card).getByText(/Download/).closest("button") as HTMLButtonElement;
      expect(dl.disabled).toBe(true);
    }

    // A locked card must not become the chosen engine.
    const breeze = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
    await fireEvent.click(breeze);
    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.tts.engine).toBe("piper");
  });

  test("entering a token unlocks the gated voices and saves it once", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = container.querySelector(".hf-input") as HTMLInputElement;
    expect(field, "the wizard should ask for a HuggingFace token").toBeTruthy();
    await fireEvent.input(field, { target: { value: "  hf_abc123  " } });

    let current: any;
    config.subscribe((c) => (current = c))();
    // Trimmed, stored once, and in the same place Settings writes it.
    expect(current.tts.hf_token).toBe("hf_abc123");
    expect(current.tts.pocket_tts.hf_token).toBeUndefined();
    expect(current.tts.breeze_tts_2.hf_token).toBeUndefined();

    await waitFor(async () => {
      const card = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
      expect(card.classList.contains("locked")).toBe(false);
    });

    const card = (await screen.findByText("Pocket TTS")).closest(".card") as HTMLElement;
    await fireEvent.click(card);
    config.subscribe((c) => (current = c))();
    expect(current.tts.engine).toBe("pocket_tts");
  });

  test("an exported HF_TOKEN fills the field, unlocks the voices, and stays out of the config", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "hf_token_env") return "hf_from_env";
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = await waitFor(() => {
      const el = container.querySelector(".hf-input") as HTMLInputElement;
      expect(el.value).toBe("hf_from_env");
      return el;
    });
    // Read-only: HF_TOKEN wins at download time, so typing over it would only
    // save a value the app then ignores.
    expect(field.readOnly).toBe(true);
    expect(container.textContent).toContain("HF_TOKEN environment variable");

    const breeze = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
    expect(breeze.classList.contains("locked")).toBe(false);

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.tts.hf_token, "the environment token must not be saved").toBeNull();
  });

  test("typing is ignored while the environment supplies the token", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "hf_token_env") return "hf_from_env";
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = await waitFor(() => {
      const el = container.querySelector(".hf-input") as HTMLInputElement;
      expect(el.value).toBe("hf_from_env");
      return el;
    });
    await fireEvent.input(field, { target: { value: "hf_typed_over" } });

    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.tts.hf_token).toBeNull();
  });

  test("a gated download sends the one saved token", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "download_breeze_tts_2") return undefined;
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = container.querySelector(".hf-input") as HTMLInputElement;
    await fireEvent.input(field, { target: { value: "hf_abc123" } });

    const breeze = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
    const dl = await waitFor(() => {
      const b = within(breeze).getByText(/Download/).closest("button") as HTMLButtonElement;
      expect(b.disabled).toBe(false);
      return b;
    });
    await fireEvent.click(dl);

    await waitFor(() => {
      const call = invoke.mock.calls.find(([c]) => c === "download_breeze_tts_2");
      expect(call).toBeTruthy();
      expect((call as any)[1].hfToken).toBe("hf_abc123");
    });
  });

  test("a gated voice already on disk needs no token to pick or continue", async () => {
    // The weights are there — from an earlier run, from Settings, or from a
    // shell that had HF_TOKEN exported. The token only fetches them.
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "check_breeze_tts_2_ready") return true;
      return false;
    });
    const blocked: (string | null)[] = [];
    render(VoiceStep, { setBlocker: (_step: number, reason: string | null) => blocked.push(reason) });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const breeze = await waitFor(async () => {
      const card = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
      expect(card.classList.contains("locked")).toBe(false);
      return card;
    });
    expect(breeze.textContent).not.toContain("Needs a HuggingFace access token");
    expect(breeze.textContent).toContain("Already downloaded");

    await fireEvent.click(breeze);
    let current: any;
    config.subscribe((c) => (current = c))();
    expect(current.tts.engine).toBe("breeze_tts_2");

    // And Continue is not held back for a token that would fetch nothing.
    await waitFor(() => expect(blocked.at(-1)).toBeNull());
  });

  test("a gated voice that is not on disk still needs a token", async () => {
    // The mirror of the case above: readiness is what unlocks it, not the mere
    // fact that it is gated.
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    const breeze = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
    await waitFor(() => expect(breeze.classList.contains("locked")).toBe(true));
  });

  test("a token HuggingFace refuses is reported as refused, not as a raw error", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "download_breeze_tts_2") {
        // What the backend actually hands back for a 401 on a gated repo.
        throw "hf-token-rejected: HuggingFace did not accept the access token for " +
          "BreezeBlue/Breeze-TTS-2. Check the token is valid and that its account has " +
          "accepted the model's licence.";
      }
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = container.querySelector(".hf-input") as HTMLInputElement;
    await fireEvent.input(field, { target: { value: "hf_wrong" } });

    const breeze = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
    const dl = await waitFor(() => {
      const b = within(breeze).getByText(/Download/).closest("button") as HTMLButtonElement;
      expect(b.disabled).toBe(false);
      return b;
    });
    await fireEvent.click(dl);

    // The card says what went wrong and where to fix it, and the field's own
    // state line stops claiming the token is good.
    await waitFor(() => {
      expect(breeze.textContent).toContain("did not accept that access token");
      expect(breeze.textContent).toContain("huggingface.co/BreezeBlue/Breeze-TTS-2");
    });
    expect(container.querySelector(".hf-state")?.textContent).toContain("did not accept this token");
    // Not the tag the backend uses to mark it — that is machinery, not a message.
    expect(breeze.textContent).not.toContain("hf-token-rejected");

    // Editing the token retracts the verdict rather than leaving it standing
    // over a value that has not been tried.
    await fireEvent.input(field, { target: { value: "hf_another" } });
    await waitFor(() =>
      expect(container.querySelector(".hf-state")?.textContent).not.toContain(
        "did not accept this token",
      ),
    );
  });

  test("a download that fails for other reasons still shows the real error", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "download_breeze_tts_2") throw "error sending request: connection refused";
      return false;
    });
    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = container.querySelector(".hf-input") as HTMLInputElement;
    await fireEvent.input(field, { target: { value: "hf_good" } });

    const breeze = (await screen.findByText("Breeze-TTS-2")).closest(".card") as HTMLElement;
    const dl = await waitFor(() => {
      const b = within(breeze).getByText(/Download/).closest("button") as HTMLButtonElement;
      expect(b.disabled).toBe(false);
      return b;
    });
    await fireEvent.click(dl);

    await waitFor(() => expect(breeze.textContent).toContain("connection refused"));
    // A dead network is not the token's fault, and must not be blamed on it.
    expect(container.querySelector(".hf-state")?.textContent).not.toContain(
      "did not accept this token",
    );
  });

  test("a saved token fills the field, masked, and unlocks the gated voices", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      return false;
    });
    const cfg = baseConfig();
    cfg.tts.hf_token = "hf_saved_earlier";
    config.set(cfg);

    const { container } = render(VoiceStep, { setBlocker: noopBlocker });
    await fireEvent.click(await screen.findByText("Enable speech output"));

    const field = container.querySelector(".hf-input") as HTMLInputElement;
    expect(field.value).toBe("hf_saved_earlier");
    // Masked: the wizard is the sort of screen someone screen-shares.
    expect(field.type).toBe("password");
    expect(field.readOnly).toBe(false);

    await waitFor(async () => {
      const card = (await screen.findByText("Pocket TTS")).closest(".card") as HTMLElement;
      expect(card.classList.contains("locked")).toBe(false);
    });
  });

  test("the play button returns to idle when the end event arrives", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "check_voice_downloaded") return true;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    const piper = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
    await fireEvent.click(await waitFor(() => within(piper).getByTitle("Play a sample")));
    await waitFor(() => expect(within(piper).queryByText("play sample")).toBeNull());

    listeners.get("tts-playback-end")?.({ payload: undefined });

    await waitFor(() => expect(within(piper).getByText("play sample")).toBeTruthy());
  });

  test("recovers with no events at all, by asking the backend", async () => {
    // The window this runs in receives neither tts-playback-end nor the status
    // tick, so the only channel left is the one that plays the sample.
    vi.useFakeTimers();
    try {
      let speaking = false;
      invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "inflect_micro_available") return true;
        if (cmd === "check_voice_downloaded") return true;
        if (cmd === "get_status") return { speaking };
        return false;
      });
      render(VoiceStep, { setBlocker: noopBlocker });
      await vi.advanceTimersByTimeAsync(60);

      const piper = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
      await fireEvent.click(within(piper).getByTitle("Play a sample"));
      await vi.advanceTimersByTimeAsync(60);
      expect(within(piper).queryByText("play sample")).toBeNull();

      // The engine starts speaking, and keeps the card busy while it does.
      speaking = true;
      await vi.advanceTimersByTimeAsync(2000);
      expect(within(piper).queryByText("play sample")).toBeNull();

      // It finishes. No event says so — the next poll finds out.
      speaking = false;
      await vi.advanceTimersByTimeAsync(600);

      expect(within(piper).getByText("play sample")).toBeTruthy();
      expect(screen.queryByText(/never finished playing/)).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  test("a sample too short to catch between polls still ends", async () => {
    // Never observed speaking: without a settle window this would hang until
    // the watchdog, which is a 30-second stall for a sample that worked.
    vi.useFakeTimers();
    try {
      invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "inflect_micro_available") return true;
        if (cmd === "check_voice_downloaded") return true;
        if (cmd === "get_status") return { speaking: false };
        return false;
      });
      render(VoiceStep, { setBlocker: noopBlocker });
      await vi.advanceTimersByTimeAsync(60);

      const piper = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
      await fireEvent.click(within(piper).getByTitle("Play a sample"));
      await vi.advanceTimersByTimeAsync(60);

      // Held through the settle window rather than snapping back instantly.
      await vi.advanceTimersByTimeAsync(900);
      expect(within(piper).queryByText("play sample")).toBeNull();

      await vi.advanceTimersByTimeAsync(1200);
      expect(within(piper).getByText("play sample")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  test("one card playing does not strand the others", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "check_voice_downloaded") return true;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    const piper = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
    const espeak = (await screen.findByText("eSpeak-NG")).closest(".card") as HTMLElement;
    await fireEvent.click(await waitFor(() => within(piper).getByTitle("Play a sample")));

    expect((within(espeak).getByTitle("Play a sample") as HTMLButtonElement).disabled).toBe(true);

    listeners.get("tts-playback-end")?.({ payload: undefined });

    await waitFor(() =>
      expect((within(espeak).getByTitle("Play a sample") as HTMLButtonElement).disabled).toBe(false),
    );
  });

  test("pressing the speaking card again stops it", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "check_voice_downloaded") return true;
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    const piper = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
    await fireEvent.click(await waitFor(() => within(piper).getByTitle("Play a sample")));

    const stop = await waitFor(() => within(piper).getByTitle("Stop"));
    await fireEvent.click(stop);

    await waitFor(() => expect(within(piper).getByText("play sample")).toBeTruthy());
    expect(invoke.mock.calls.some(([c]) => c === "stop_tts")).toBe(true);
  });

  test("an engine that never reports back is given up on rather than left stuck", async () => {
    vi.useFakeTimers();
    try {
      invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "inflect_micro_available") return true;
        if (cmd === "check_voice_downloaded") return true;
        // Status calls hang forever: the one case the poll cannot resolve.
        if (cmd === "get_status") return new Promise(() => {});
        return false;
      });
      render(VoiceStep, { setBlocker: noopBlocker });

      await vi.advanceTimersByTimeAsync(50);
      const piper = (await screen.findByText("Piper TTS")).closest(".card") as HTMLElement;
      await fireEvent.click(within(piper).getByTitle("Play a sample"));
      await vi.advanceTimersByTimeAsync(50);
      expect(within(piper).queryByText("play sample")).toBeNull();

      // No end event, no usable status, no error — just silence.
      await vi.advanceTimersByTimeAsync(31_000);

      expect(within(piper).getByText("play sample")).toBeTruthy();
      expect(screen.getByText(/never finished playing/)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  test("a failed voice download is logged for the final screen", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "inflect_micro_available") return true;
      if (cmd === "download_voice") throw new Error("404 from huggingface");
      return false;
    });
    render(VoiceStep, { setBlocker: noopBlocker });

    await fireEvent.click(await screen.findByText(/Download 60 MB/));

    await waitFor(() => {
      expect(wizard.issues.map((i) => i.id)).toContain("tts-download-piper");
      expect(wizard.issues[0].detail).toContain("404 from huggingface");
    });
  });

  test("speech output left off never blocks the way forward", async () => {
    const blockers: (string | null)[] = [];
    invoke.mockImplementation(async () => false);
    render(VoiceStep, { setBlocker: (_s: number, r: string | null) => blockers.push(r) });
    await waitFor(() => expect(blockers.length).toBeGreaterThan(0));
    expect(blockers.at(-1)).toBeNull();
  });
});

describe("TestStep", () => {
  function setStatus(over: Record<string, unknown>) {
    status.set({
      recording: false,
      processing: false,
      speaking: false,
      mcp_recording: false,
      audio_ready: true,
      word_count: 0,
      ...over,
    } as any);
  }

  beforeEach(() => {
    setStatus({});
    wizard.combo = ["KEY_LEFTALT", "KEY_V"];
    wizard.gesture = "hold";
  });

  test("tells the user how to trigger the gesture they actually chose", async () => {
    wizard.gesture = "double_tap";
    render(TestStep);
    expect(await screen.findByText("double-tap")).toBeTruthy();
    expect(screen.getByText("Alt")).toBeTruthy();
    expect(screen.getByText("V")).toBeTruthy();
  });

  test("follows the real pipeline state rather than a timed animation", async () => {
    render(TestStep);
    expect(await screen.findByText("waiting for hotkey")).toBeTruthy();

    setStatus({ recording: true });
    expect(await screen.findByText("recording")).toBeTruthy();

    setStatus({ recording: false, processing: true });
    expect(await screen.findByText("transcribing")).toBeTruthy();
  });

  test("counts the transcription landing in the box as success", async () => {
    const { container } = render(TestStep);
    const box = container.querySelector("textarea") as HTMLTextAreaElement;

    setStatus({ recording: true });
    await screen.findByText("recording");
    await fireEvent.input(box, { target: { value: "hello from voxctrl" } });

    expect(await screen.findByText("It works.")).toBeTruthy();
  });

  test("typing by hand before ever pressing the hotkey is not a passing test", async () => {
    const { container } = render(TestStep);
    const box = container.querySelector("textarea") as HTMLTextAreaElement;

    await fireEvent.input(box, { target: { value: "typed by hand" } });

    expect(screen.queryByText("It works.")).toBeNull();
    expect(screen.getByText("waiting for hotkey")).toBeTruthy();
  });
});

describe("DoneStep", () => {
  function setupStatus(over: Record<string, unknown> = {}) {
    return {
      hotkeys: { backend: "evdev", portal_error: null, portal_refused: false },
      hotkeys_active: true,
      model_ready: true,
      model_size: "small",
      model_auto_downloads: false,
      missing_injection_tool: null,
      manual_package_commands: "",
      pkexec_available: true,
      is_complete: true,
      ...over,
    };
  }

  test("a clean run says the app is ready", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_setup_status") return setupStatus();
      return undefined;
    });
    render(DoneStep);
    await waitFor(() => expect(screen.getByText("ready")).toBeTruthy());
    expect(screen.queryByText("Copy diagnostics")).toBeNull();
  });

  test("a failure during the wizard is explained with its technical detail", async () => {
    wizard.recordIssue({
      id: "model-download",
      step: 1,
      title: "Speech model could not be downloaded.",
      detail: "engine=whisper-cpp model=small\nError: connection reset",
    });
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_setup_status") return setupStatus();
      return undefined;
    });
    render(DoneStep);

    expect(await screen.findByText("Speech model could not be downloaded.")).toBeTruthy();
    expect(screen.getByText(/connection reset/)).toBeTruthy();
    expect(screen.getByText("incomplete")).toBeTruthy();
    expect(screen.getByText("Copy diagnostics")).toBeTruthy();
  });

  test("problems the wizard never saw are found by re-checking the install", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_setup_status") {
        return setupStatus({
          hotkeys_active: false,
          missing_injection_tool: "wtype",
          manual_package_commands: "sudo apt install wtype",
          hotkeys: { backend: "portal", portal_error: "portal timed out", portal_refused: true },
          is_complete: false,
        });
      }
      return undefined;
    });
    render(DoneStep);

    expect(await screen.findByText(/No global shortcut is active/)).toBeTruthy();
    expect(screen.getByText(/cannot type text into other windows/)).toBeTruthy();
    // The raw portal error is what makes a bug report actionable.
    expect(screen.getByText(/portal timed out/)).toBeTruthy();
    expect(screen.getByText(/sudo apt install wtype/)).toBeTruthy();
  });

  test("a model that downloads itself in the background is not reported as broken", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_setup_status") {
        return setupStatus({ model_ready: false, model_auto_downloads: true });
      }
      return undefined;
    });
    render(DoneStep);
    await waitFor(() => expect(screen.getByText("ready")).toBeTruthy());
  });

  test("the summary reflects what the user actually chose", async () => {
    wizard.combo = ["KEY_LEFTALT", "KEY_V"];
    wizard.gesture = "toggle";
    config.set({
      ...baseConfig(),
      ui: { ...baseConfig().ui, overlay_style: "terminal", overlay_position: "top" },
      tts: { ...baseConfig().tts, enabled: true, engine: "piper" },
    } as any);
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_setup_status") return setupStatus();
      return undefined;
    });
    render(DoneStep);

    expect(await screen.findByText("Alt + V · Tap to talk")).toBeTruthy();
    expect(screen.getByText("Retro Terminal · top")).toBeTruthy();
    expect(screen.getByText("Piper TTS")).toBeTruthy();
  });
});
