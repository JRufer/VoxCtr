import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/svelte";
import CommandsTab from "../../src/lib/Settings/CommandsTab.svelte";

const invoke = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

function target(overrides: Record<string, unknown> = {}) {
  return {
    id: "notes",
    label: "Notes",
    delivery: "file",
    file_path: "~/notes.md",
    processing: {},
    ...overrides,
  };
}

describe("Output Commands tab", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_targets") return [target()];
      if (cmd === "get_bindings") return [];
      return null;
    });
  });

  test("is named Output Commands, in the heading and on the add button", async () => {
    render(CommandsTab);

    expect(await screen.findByText("Output Commands")).toBeTruthy();
    expect(screen.getByText(/Add New Output Command/)).toBeTruthy();
  });

  /// The phrase is the whole point of naming a command, and nothing else in
  /// the UI says it out loud — a user who never reads the docs would other-
  /// wise have no way to discover that speaking the name routes their text.
  test("explains how to trigger a command by voice", async () => {
    const { container } = render(CommandsTab);
    await screen.findByText("Output Commands");

    const note = container.querySelector(".usage-note") as HTMLElement;
    expect(note).toBeTruthy();

    const text = note.textContent ?? "";
    expect(text).toContain("VoxCtrl");
    // The order the user has to say it in: trigger, name, then the text.
    expect(text.indexOf("VoxCtrl notes")).toBeGreaterThan(-1);
    expect(text).toMatch(/command's name/);
  });

  test("still lists the commands that are configured", async () => {
    render(CommandsTab);
    expect(await screen.findByText("Notes")).toBeTruthy();
  });
});
