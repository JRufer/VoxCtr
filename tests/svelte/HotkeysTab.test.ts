import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import HotkeysTab from "../../src/lib/Settings/HotkeysTab.svelte";
import type { HotkeyBinding, OutputTarget } from "../../src/lib/Settings/routing-types";

let mockBindings: HotkeyBinding[] = [];
let mockTargets: OutputTarget[] = [];
let mockHotkeyStatus: Record<string, unknown> = {};
let mockKeysChecks: Array<Record<string, unknown>> = [];
let keysCheckCalls: string[][] = [];

// Mock tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd, args) => {
    if (cmd === "get_targets") {
      return mockTargets;
    }
    if (cmd === "get_bindings") {
      return mockBindings;
    }
    if (cmd === "check_hotkey_status") {
      return mockHotkeyStatus;
    }
    if (cmd === "check_hotkey_keys") {
      keysCheckCalls.push([...((args as { keys: string[] })?.keys ?? [])]);
      return mockKeysChecks.shift() ?? { accepted: true, enforced: false, accelerator: null, problem: null, message: null };
    }
    return {};
  }),
}));

/// The backend's verdict for a combination the desktop cannot register.
function rejected(message: string) {
  return {
    accepted: false,
    enforced: true,
    accelerator: null,
    problem: "modifiers_only",
    message,
  };
}

function hotkeyStatus(overrides: Record<string, unknown> = {}) {
  return {
    is_active: true,
    backend: "portal",
    is_private: true,
    portal_error: null,
    portal_refused: false,
    shortcuts: [],
    session_type: "wayland",
    devices_total: 0,
    devices_readable: 0,
    needs_attention: false,
    detail: "Your desktop is handling VoxCtrl's global shortcuts.",
    ...overrides,
  };
}

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
    mockHotkeyStatus = hotkeyStatus();
    mockKeysChecks = [];
    keysCheckCalls = [];
  });

  test("refuses a bare-modifier capture and says why", async () => {
    // A lone Super looks like a perfectly good hotkey to a user, and no desktop
    // can bind it. Discovering that later, silently, is the failure this
    // prevents.
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"],
        gesture: "double_tap",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];
    mockKeysChecks = [
      rejected(
        "Your desktop cannot register this shortcut: a shortcut needs at least one regular key. Add a regular key to the combination — Super+Space and Ctrl+Alt+D both work.",
      ),
    ];

    const { container } = render(HotkeysTab);
    const editBtn = await screen.findByRole("button", { name: /Edit/i });
    await fireEvent.click(editBtn);

    const recorder = container.querySelector('[aria-label="Base Hotkey recorder input"]')!;
    await fireEvent.focus(recorder);
    await fireEvent.keyDown(recorder, { key: "Meta", code: "MetaLeft" });
    await fireEvent.keyUp(recorder, { key: "Meta", code: "MetaLeft" });

    expect(await screen.findByText(/That combination was not accepted/i)).toBeTruthy();
    expect(await screen.findByText(/needs at least one regular key/i)).toBeTruthy();
    expect(keysCheckCalls).toEqual([["KEY_LEFTMETA"]]);
  });

  test("keeps the previous combination when a capture is refused", async () => {
    // Rejecting must not also destroy the working shortcut the user already had.
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"],
        gesture: "hold",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];
    mockKeysChecks = [rejected("Your desktop cannot register this shortcut.")];

    const { container } = render(HotkeysTab);
    await fireEvent.click(await screen.findByRole("button", { name: /Edit/i }));

    const recorder = container.querySelector('[aria-label="Base Hotkey recorder input"]')!;
    await fireEvent.focus(recorder);
    await fireEvent.keyDown(recorder, { key: "Shift", code: "ShiftLeft" });
    await fireEvent.keyUp(recorder, { key: "Shift", code: "ShiftLeft" });

    await screen.findByText(/not accepted/i);
    // Chips render the evdev name minus the KEY_ prefix.
    for (const key of ["LEFTCTRL", "LEFTALT", "D"]) {
      expect(screen.getAllByText(key).length).toBeGreaterThan(0);
    }
    expect(screen.queryByText("LEFTSHIFT")).toBeNull();
  });

  test("accepts a valid combination and shows what the desktop will bind", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"],
        gesture: "hold",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];
    mockKeysChecks = [
      { accepted: true, enforced: false, accelerator: "LOGO+space", problem: null, message: null },
    ];

    const { container } = render(HotkeysTab);
    await fireEvent.click(await screen.findByRole("button", { name: /Edit/i }));

    const recorder = container.querySelector('[aria-label="Base Hotkey recorder input"]')!;
    await fireEvent.focus(recorder);
    await fireEvent.keyDown(recorder, { key: "Meta", code: "MetaLeft" });
    await fireEvent.keyDown(recorder, { key: " ", code: "Space" });
    await fireEvent.keyUp(recorder, { key: " ", code: "Space" });

    expect(await screen.findByText("LOGO+space")).toBeTruthy();
    expect(screen.queryByText(/not accepted/i)).toBeNull();
    expect(keysCheckCalls).toEqual([["KEY_LEFTMETA", "KEY_SPACE"]]);
  });

  test("nudges the user while only modifiers are held", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"],
        gesture: "hold",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];

    const { container } = render(HotkeysTab);
    await fireEvent.click(await screen.findByRole("button", { name: /Edit/i }));

    const recorder = container.querySelector('[aria-label="Base Hotkey recorder input"]')!;
    await fireEvent.focus(recorder);
    await fireEvent.keyDown(recorder, { key: "Meta", code: "MetaLeft" });

    // Told before they lift the key, not after the capture is thrown away.
    expect(await screen.findByText(/add a regular key/i)).toBeTruthy();
  });

  test("flags a saved binding the desktop cannot register", async () => {
    // Bindings from an older VoxCtrl are not silently broken — they are named.
    mockBindings = [
      {
        id: "legacy",
        keys: ["KEY_LEFTMETA"],
        gesture: "double_tap",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Legacy Super Tap",
        disabled: false,
      },
    ];

    render(HotkeysTab);

    expect(await screen.findByText(/needs a regular key/i)).toBeTruthy();
  });

  test("does not block bare modifiers when VoxCtrl watches the keyboard itself", async () => {
    // On the evdev fallback a lone Super genuinely works, so refusing it would
    // be wrong — but it is still worth saying it is fragile.
    mockHotkeyStatus = hotkeyStatus({
      backend: "evdev",
      is_private: false,
      detail: "VoxCtrl is reading input devices directly.",
    });
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTCTRL", "KEY_LEFTALT", "KEY_D"],
        gesture: "hold",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];
    mockKeysChecks = [
      {
        accepted: true,
        enforced: false,
        accelerator: null,
        problem: "modifiers_only",
        message: "This works right now, because VoxCtrl is watching the keyboard itself.",
      },
    ];

    const { container } = render(HotkeysTab);
    await fireEvent.click(await screen.findByRole("button", { name: /Edit/i }));

    const recorder = container.querySelector('[aria-label="Base Hotkey recorder input"]')!;
    await fireEvent.focus(recorder);
    await fireEvent.keyDown(recorder, { key: "Meta", code: "MetaLeft" });
    await fireEvent.keyUp(recorder, { key: "Meta", code: "MetaLeft" });

    expect(await screen.findByText(/watching the keyboard itself/i)).toBeTruthy();
    expect(screen.queryByText(/not accepted/i)).toBeNull();
    expect(screen.getAllByText("LEFTMETA").length).toBeGreaterThan(0);
  });

  test("offers only the four supported gestures", async () => {
    // `chord` was removed; it must not reappear as a selectable option.
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA"],
        gesture: "double_tap",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Tap",
        disabled: false,
      },
    ];

    render(HotkeysTab);
    const editBtn = await screen.findByRole("button", { name: /Edit/i });
    await fireEvent.click(editBtn);
    expect(screen.getByText("Edit Hotkey Binding")).not.toBeNull();

    expect(screen.queryByText(/chord/i)).toBeNull();
    expect(screen.queryByText(/sub ?key/i)).toBeNull();
    expect(screen.queryByText(/Base Combo/i)).toBeNull();
    expect(
      await screen.findByText(/Double-tap hotkey to trigger recording/i),
    ).toBeTruthy();
  });

  test("tells the user their desktop owns the shortcut keys", async () => {
    render(HotkeysTab);

    expect(
      await screen.findByText("Your desktop is handling these shortcuts"),
    ).toBeTruthy();
    expect(await screen.findByText(/Your desktop decides which keys/i)).toBeTruthy();
  });

  test("shows the keys the compositor actually bound, not the ones requested", async () => {
    // The portal lets the user pick different keys, and the app must show
    // what is really in effect rather than what it asked for.
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];
    mockHotkeyStatus = hotkeyStatus({
      shortcuts: [
        {
          binding_ids: ["bind1"],
          requested: "LOGO+space",
          trigger_description: "Ctrl+Alt+D",
          bound: true,
        },
      ],
    });

    render(HotkeysTab);

    expect(await screen.findByText(/desktop: Ctrl\+Alt\+D/)).toBeTruthy();
  });

  test("flags a shortcut the desktop refused to bind", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA"],
        gesture: "double_tap",
        target_id: "default",
        tap_ms: 300,
        hold_threshold_ms: 200,
        label: "Dictate",
        disabled: false,
      },
    ];
    mockHotkeyStatus = hotkeyStatus({
      shortcuts: [
        {
          binding_ids: ["bind1"],
          requested: null,
          trigger_description: "",
          bound: false,
        },
      ],
    });

    render(HotkeysTab);

    expect(await screen.findByText(/not bound by your desktop/i)).toBeTruthy();
  });

  test("warns when shortcuts come from reading input devices", async () => {
    mockHotkeyStatus = hotkeyStatus({
      backend: "evdev",
      is_private: false,
      portal_error: "no such interface",
      detail: "VoxCtrl is reading input devices directly.",
    });

    render(HotkeysTab);

    expect(await screen.findByText("Reading input devices directly")).toBeTruthy();
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

  test("shows LLM badge when openai_enabled is true", async () => {
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
        openai_enabled: true,
      },
    ];

    render(HotkeysTab);

    const badge = await screen.findByText(/^LLM$/);
    expect(badge).not.toBeNull();
  });

  test("does not show LLM badge when openai_enabled is false", async () => {
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
        openai_enabled: false,
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
