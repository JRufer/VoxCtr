import { describe, test, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

import {
  COMMAND_TARGET_ID,
  COMMAND_TARGET_LABEL,
  WIZARD_BINDING_ID,
  buildCommandTarget,
  wizard,
  type HotkeyBindingLike,
  type OutputTargetLike,
} from "../../src/lib/Wizard/wizard-state.svelte";

/** The shape `get_targets` really returns for a fresh install. */
function injectTarget(): OutputTargetLike {
  return {
    id: "default",
    label: "Focused Window",
    delivery: "inject",
    command: null,
    file_prefix: "",
    file_timestamp: true,
    http_method: "POST",
    chat_max_history: 10,
    chat_timeout_secs: 30,
    chat_reply_mode: "inject",
    strip_newlines: false,
  };
}

/** Capture whatever the wizard passed to a given command. */
function lastArgsFor(cmd: string): any {
  const call = [...invoke.mock.calls].reverse().find(([name]) => name === cmd);
  return call?.[1];
}

beforeEach(() => {
  invoke.mockReset();
  wizard.reset();
});

describe("buildCommandTarget", () => {
  test("clones an existing target so no delivery field is left undefined", () => {
    const built = buildCommandTarget(injectTarget());
    // Every key the template carried survives; only identity and delivery move.
    for (const key of Object.keys(injectTarget())) {
      expect(built).toHaveProperty(key);
    }
    expect(built.chat_reply_mode).toBe("inject");
    expect(built.http_method).toBe("POST");
  });

  test("produces a Command-delivery target named Command", () => {
    const built = buildCommandTarget(injectTarget());
    expect(built.id).toBe(COMMAND_TARGET_ID);
    expect(built.label).toBe(COMMAND_TARGET_LABEL);
    expect(built.delivery).toBe("command");
  });

  test("leaves the shell-command field empty — that belongs to exec delivery", () => {
    const built = buildCommandTarget({ ...injectTarget(), command: "echo {text}" });
    expect(built.command).toBeNull();
  });
});

describe("ensureCommandTarget", () => {
  test("creates the Command target on a fresh install and keeps the existing ones", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_targets") return [injectTarget()];
      return undefined;
    });

    const id = await wizard.ensureCommandTarget();

    expect(id).toBe(COMMAND_TARGET_ID);
    const saved = lastArgsFor("save_targets").targets as OutputTargetLike[];
    expect(saved.map((t) => t.id)).toEqual(["default", COMMAND_TARGET_ID]);
    expect(saved[1].delivery).toBe("command");
  });

  test("does not rewrite targets when the Command target already exists", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_targets") {
        return [injectTarget(), buildCommandTarget(injectTarget())];
      }
      return undefined;
    });

    const id = await wizard.ensureCommandTarget();

    expect(id).toBe(COMMAND_TARGET_ID);
    expect(invoke.mock.calls.some(([name]) => name === "save_targets")).toBe(false);
  });

  test("falls back to the built-in target rather than writing a half-built one", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_targets") return [];
      return undefined;
    });

    const id = await wizard.ensureCommandTarget();

    expect(id).toBe("default");
    expect(invoke.mock.calls.some(([name]) => name === "save_targets")).toBe(false);
  });
});

describe("saveBinding", () => {
  function mockBackend(bindings: Partial<HotkeyBindingLike>[] = []) {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_targets") return [injectTarget()];
      if (cmd === "get_bindings") return bindings;
      return undefined;
    });
  }

  test("points the first binding at the Command target, not the raw inject one", async () => {
    mockBackend();
    wizard.combo = ["KEY_LEFTMETA", "KEY_SPACE"];
    wizard.gesture = "hold";

    await wizard.saveBinding();

    const saved = lastArgsFor("save_bindings").bindings as HotkeyBindingLike[];
    expect(saved[0].target_id).toBe(COMMAND_TARGET_ID);
    expect(saved[0].target_ids).toEqual([COMMAND_TARGET_ID]);
  });

  test("writes the captured keys and the chosen gesture", async () => {
    mockBackend();
    wizard.combo = ["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"];
    wizard.gesture = "double_tap";

    await wizard.saveBinding();

    const saved = lastArgsFor("save_bindings").bindings as HotkeyBindingLike[];
    expect(saved[0].id).toBe(WIZARD_BINDING_ID);
    expect(saved[0].keys).toEqual(["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"]);
    expect(saved[0].gesture).toBe("double_tap");
    expect(saved[0].disabled).toBe(false);
  });

  test("replaces the shipped defaults instead of leaving a second shortcut behind", async () => {
    mockBackend([
      { id: "default_hold", keys: ["KEY_LEFTMETA", "KEY_SPACE"], gesture: "hold" },
      { id: "default_toggle", keys: ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"], gesture: "toggle" },
    ]);
    wizard.combo = ["KEY_LEFTALT", "KEY_V"];

    await wizard.saveBinding();

    const saved = lastArgsFor("save_bindings").bindings as HotkeyBindingLike[];
    expect(saved).toHaveLength(1);
    expect(saved[0].keys).toEqual(["KEY_LEFTALT", "KEY_V"]);
  });

  test("keeps bindings the user made themselves", async () => {
    mockBackend([
      { id: "default_hold", keys: ["KEY_LEFTMETA", "KEY_SPACE"], gesture: "hold" },
      { id: "binding_mine", keys: ["KEY_LEFTCTRL", "KEY_M"], gesture: "toggle" },
    ]);
    wizard.combo = ["KEY_LEFTALT", "KEY_V"];

    await wizard.saveBinding();

    const saved = lastArgsFor("save_bindings").bindings as HotkeyBindingLike[];
    expect(saved.map((b) => b.id)).toEqual([WIZARD_BINDING_ID, "binding_mine"]);
  });

  test("never writes the internal TTS stop binding back as a user binding", async () => {
    mockBackend([{ id: "__tts_stop__", keys: ["KEY_ESC"], gesture: "hold" }]);
    wizard.combo = ["KEY_LEFTALT", "KEY_V"];

    await wizard.saveBinding();

    const saved = lastArgsFor("save_bindings").bindings as HotkeyBindingLike[];
    expect(saved.map((b) => b.id)).toEqual([WIZARD_BINDING_ID]);
  });

  test("writes nothing when no combination has been captured", async () => {
    mockBackend();
    wizard.combo = null;

    await wizard.saveBinding();

    expect(invoke.mock.calls.some(([name]) => name === "save_bindings")).toBe(false);
  });
});

describe("loadBinding", () => {
  test("restores the keys and gesture a previous pass wrote", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_bindings") {
        return [{ id: WIZARD_BINDING_ID, keys: ["KEY_LEFTALT", "KEY_V"], gesture: "toggle" }];
      }
      return undefined;
    });

    await wizard.loadBinding();

    expect(wizard.combo).toEqual(["KEY_LEFTALT", "KEY_V"]);
    expect(wizard.gesture).toBe("toggle");
  });

  test("ignores a gesture the wizard does not offer rather than showing a blank card", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_bindings") {
        return [{ id: WIZARD_BINDING_ID, keys: ["KEY_LEFTALT", "KEY_V"], gesture: "nonsense" }];
      }
      return undefined;
    });

    await wizard.loadBinding();

    expect(wizard.combo).toEqual(["KEY_LEFTALT", "KEY_V"]);
    expect(wizard.gesture).toBe("hold");
  });

  test("survives a backend that cannot read bindings at all", async () => {
    invoke.mockImplementation(async () => {
      throw new Error("bindings.toml is unreadable");
    });

    await expect(wizard.loadBinding()).resolves.toBeUndefined();
    expect(wizard.combo).toBeNull();
  });
});

describe("issue log", () => {
  test("records a failure with its technical detail", () => {
    wizard.recordIssue({ id: "model-download", step: 1, title: "boom", detail: "HTTP 503" });
    expect(wizard.issues).toHaveLength(1);
    expect(wizard.issues[0].detail).toBe("HTTP 503");
  });

  test("a retried step replaces its entry instead of stacking duplicates", () => {
    wizard.recordIssue({ id: "model-download", step: 1, title: "boom", detail: "first" });
    wizard.recordIssue({ id: "model-download", step: 1, title: "boom", detail: "second" });
    expect(wizard.issues).toHaveLength(1);
    expect(wizard.issues[0].detail).toBe("second");
  });

  test("a resolved problem is cleared and stops being reported", () => {
    wizard.recordIssue({ id: "model-download", step: 1, title: "boom", detail: "x" });
    wizard.recordIssue({ id: "shortcut-register", step: 2, title: "bang", detail: "y" });
    wizard.clearIssue("model-download");
    expect(wizard.issues.map((i) => i.id)).toEqual(["shortcut-register"]);
  });

  test("clearing something that never failed is a no-op", () => {
    wizard.clearIssue("never-happened");
    expect(wizard.issues).toEqual([]);
  });
});

describe("step navigation", () => {
  test("moving forward records how far the user has reached", async () => {
    vi.useFakeTimers();
    wizard.goTo(1);
    vi.advanceTimersByTime(250);
    expect(wizard.step).toBe(1);
    expect(wizard.visited).toBe(1);

    wizard.goTo(0);
    vi.advanceTimersByTime(250);
    expect(wizard.step).toBe(0);
    // Going back must not take the tracker's unlocked steps away again.
    expect(wizard.visited).toBe(1);
    vi.useRealTimers();
  });

  test("refuses to leave the wizard through either end", () => {
    vi.useFakeTimers();
    wizard.goTo(-1);
    wizard.goTo(99);
    vi.advanceTimersByTime(250);
    expect(wizard.step).toBe(0);
    vi.useRealTimers();
  });

  test("leaving a step stops the key recorder, so it cannot swallow keys elsewhere", () => {
    vi.useFakeTimers();
    wizard.recording = true;
    wizard.goTo(3);
    expect(wizard.recording).toBe(false);
    vi.advanceTimersByTime(250);
    vi.useRealTimers();
  });
});
