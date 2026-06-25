import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import HotkeysTab from "../../src/lib/Settings/HotkeysTab.svelte";
import type { HotkeyBinding, OutputTarget } from "../../src/lib/Settings/routing-types";

let mockBindings: HotkeyBinding[] = [];
let mockTargets: OutputTarget[] = [];

// Mock tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd) => {
    if (cmd === "get_targets") {
      return mockTargets;
    }
    if (cmd === "get_bindings") {
      return mockBindings;
    }
    return {};
  }),
}));

describe("HotkeysTab.svelte Conflict Detection and Nested Modal", () => {
  beforeEach(() => {
    mockTargets = [
      {
        id: "default",
        label: "Focused Window",
        delivery: "inject",
        file_prefix: "",
        file_timestamp: true,
        send_on_release: true,
        append_newline: false,
        strip_newlines: false,
        tts_engine: "None",
      },
    ];
    mockBindings = [];
  });

  test("does not show conflict warnings when there are no conflicts", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
      {
        id: "bind2",
        keys: ["KEY_LEFTMETA", "KEY_ENTER"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 2",
        disabled: false,
      },
    ];

    render(HotkeysTab);

    // Conflict banner should NOT be present
    const banner = screen.queryByText(/Conflict detected/i);
    expect(banner).toBeNull();

    // No CONFLICT markers should be present
    const marker = screen.queryByText("CONFLICT");
    expect(marker).toBeNull();
  });

  test("shows active conflicts with yellow background and CONFLICT marker when both are enabled", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
      {
        id: "bind2",
        keys: ["KEY_SPACE", "KEY_LEFTMETA"], // Same keys, different order
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 2",
        disabled: false,
      },
    ];

    const { container } = render(HotkeysTab);

    // Conflict banner should be present
    const banner = await screen.findByText(/Conflict detected/i);
    expect(banner).not.toBeNull();

    // CONFLICT markers should be rendered
    const markers = await screen.findAllByText("CONFLICT");
    expect(markers.length).toBe(2);

    // The cards should have active-conflict class
    const conflictItems = container.querySelectorAll(".active-conflict");
    expect(conflictItems.length).toBe(2);
  });

  test("shows conflict borders but no CONFLICT markers or active-conflict background when one is disabled", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
      {
        id: "bind2",
        keys: ["KEY_SPACE", "KEY_LEFTMETA"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 2",
        disabled: true, // One is disabled
      },
    ];

    const { container } = render(HotkeysTab);

    // Conflict banner should still be present because a conflict exists in the list
    const banner = await screen.findByText(/Conflict detected/i);
    expect(banner).not.toBeNull();

    // No CONFLICT markers should be rendered (since one is disabled, the active one works)
    const marker = screen.queryByText("CONFLICT");
    expect(marker).toBeNull();

    // The cards should have has-conflict class but NOT active-conflict class
    const hasConflictItems = container.querySelectorAll(".has-conflict");
    expect(hasConflictItems.length).toBe(2);

    const activeConflictItems = container.querySelectorAll(".active-conflict");
    expect(activeConflictItems.length).toBe(0);
  });

  test("shows LLM badge when ollama_enabled is true", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
        ollama_enabled: true,
      },
    ];

    render(HotkeysTab);

    const badge = await screen.findByText(/^LLM$/);
    expect(badge).not.toBeNull();
  });

  test("does not show LLM badge when ollama_enabled is false", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
        ollama_enabled: false,
      },
    ];

    render(HotkeysTab);

    const badge = screen.queryByText(/^LLM$/);
    expect(badge).toBeNull();
  });

  test("opens nested Target modal and cancels to revert select value", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
    ];

    const { container } = render(HotkeysTab);

    // Click Edit button to open Hotkey Editor modal
    const editBtn = await screen.findByRole("button", { name: /Edit/i });
    await fireEvent.click(editBtn);

    // Verify Binding Editor modal is open
    expect(screen.getByText("Edit Hotkey Binding")).not.toBeNull();

    // Find the custom dropdown trigger button
    const trigger = container.querySelector(".custom-select-trigger") as HTMLButtonElement;
    expect(trigger).not.toBeNull();
    expect(trigger.textContent).toContain("Focused Window");

    // Click trigger to open dropdown list
    await fireEvent.click(trigger);

    // Click "-- Create New Target --" option button
    const createBtn = screen.getByText("Create New Target");
    expect(createBtn).not.toBeNull();
    await fireEvent.click(createBtn);

    // Verify Target Editor modal opens
    expect(await screen.findByText("Create Target")).not.toBeNull();

    // Verify that Target ID input is hidden in nested mode
    const targetIdInput = screen.queryByPlaceholderText("e.g. obsidian_vault");
    expect(targetIdInput).toBeNull();

    // Click Cancel button in the Target modal
    const cancelButtons = screen.getAllByRole("button", { name: /Cancel/i });
    await fireEvent.click(cancelButtons[1]);

    // Verify Target modal is closed
    expect(screen.queryByText("Create Target")).toBeNull();

    // Verify select visually reverts back to previous value
    expect(trigger.textContent).toContain("Focused Window");
  });

  test("opens nested Target modal, creates a new target, and auto-selects it", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
    ];

    const { container } = render(HotkeysTab);

    // Click Edit button to open Hotkey Editor modal
    const editBtn = await screen.findByRole("button", { name: /Edit/i });
    await fireEvent.click(editBtn);

    // Find the custom dropdown trigger button
    const trigger = container.querySelector(".custom-select-trigger") as HTMLButtonElement;
    expect(trigger).not.toBeNull();

    // Click trigger to open dropdown list
    await fireEvent.click(trigger);
    
    // Click "-- Create New Target --" option button
    const createBtn = screen.getByText("Create New Target");
    await fireEvent.click(createBtn);

    // Verify Target Editor modal opens
    expect(await screen.findByText("Create Target")).not.toBeNull();

    // Set Target display label
    const labelInput = screen.getByPlaceholderText("e.g. Type directly into Obsidian");
    await fireEvent.input(labelInput, { target: { value: "My Nested Target" } });

    // Click Done to save target
    const doneButtons = screen.getAllByRole("button", { name: /Done/i });
    await fireEvent.click(doneButtons[1]);

    // Verify Target modal is closed
    expect(screen.queryByText("Create Target")).toBeNull();

    // Verify select value was updated to the new target's label and delivery
    expect(trigger.textContent).toContain("My Nested Target (inject)");
  });
});
