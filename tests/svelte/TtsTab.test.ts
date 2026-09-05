import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/svelte";
import TtsTab from "../../src/lib/Settings/TtsTab.svelte";

let mockKeysCheck: Record<string, unknown> = {};
let keysCheckCalls: string[][] = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "check_hotkey_keys") {
      keysCheckCalls.push([...((args as { keys: string[] })?.keys ?? [])]);
      return mockKeysCheck;
    }
    if (cmd === "check_voice_downloaded") return false;
    if (cmd === "hf_token_env") return null;
    return {};
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

/// Enough of the config for the TTS tab; it reads nothing outside `cfg.tts`.
function cfg(stopKey: string[]) {
  return {
    tts: {
      enabled: true,
      engine: "espeak",
      voice: "en-us-lessac-medium",
      voice_dir: "",
      stop_key: stopKey,
      response_overlay: true,
      speed: 1.0,
      gpu: false,
      hf_token: null,
      pocket_tts: { voice: "alba", prewarm: false, voice_dir: "" },
      inflect_micro: { model_dir: "", seed: 0, noise_scale: 0.667, prewarm: false },
      breeze_tts_2: {
        speaker_prompt: "",
        model_dir: "",
        prewarm: false,
        gpu: false,
      },
      snippets: {},
    },
  };
}

describe("TtsTab stop key", () => {
  beforeEach(() => {
    keysCheckCalls = [];
    mockKeysCheck = { accepted: true, enforced: false, accelerator: null, problem: null, message: null };
  });

  test("says so when the desktop will not be asked to grab the stop key", async () => {
    // Bare Escape on a portal desktop: VoxCtrl refuses to register it, because
    // the grab would be exclusive and no other app would see Escape again. The
    // user has to be told — this is the default stop key, so it can be in a
    // config nobody chose.
    mockKeysCheck = {
      accepted: false,
      enforced: true,
      accelerator: null,
      problem: "reserved_key",
      message: "VoxCtrl will not register this shortcut with your desktop. Add Ctrl+Escape.",
    };

    render(TtsTab, { props: { cfg: cfg(["KEY_ESC"]) } });

    await waitFor(() => {
      expect(screen.getByText(/will not register this shortcut/)).toBeTruthy();
    });
    expect(keysCheckCalls).toContainEqual(["KEY_ESC"]);
  });

  test("stays quiet where VoxCtrl watches the keyboard itself", async () => {
    // X11, evdev and the Windows hook grab nothing: Escape stops playback and
    // still reaches the app underneath. An advisory note here would be noise.
    mockKeysCheck = {
      accepted: true,
      enforced: false,
      accelerator: null,
      problem: "reserved_key",
      message: "This works right now, because VoxCtrl watches the keyboard itself.",
    };

    render(TtsTab, { props: { cfg: cfg(["KEY_ESC"]) } });

    await waitFor(() => expect(keysCheckCalls).toContainEqual(["KEY_ESC"]));
    expect(screen.queryByText(/watches the keyboard itself/)).toBeNull();
  });

  test("checks nothing when no stop key is set", async () => {
    render(TtsTab, { props: { cfg: cfg([]) } });
    await new Promise((r) => setTimeout(r, 20));
    expect(keysCheckCalls).toEqual([]);
  });
});
