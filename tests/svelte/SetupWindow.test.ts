import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/svelte";
import UdevWarning from "../../src/lib/Diagnostics/UdevWarning.svelte";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ close: vi.fn() })),
}));

type Overrides = {
  permissions?: Record<string, unknown>;
  status?: Record<string, unknown>;
};

function setupStatus({ permissions = {}, status = {} }: Overrides = {}) {
  return {
    permissions: {
      is_configured: true,
      rule_exists: true,
      in_group: true,
      needs_relogin: false,
      rule_is_current: true,
      devices_total: 8,
      devices_readable: 8,
      can_relaunch: false,
      pkexec_available: true,
      session_type: "wayland",
      detail: "Global hotkeys are active.",
      manual_commands: "sudo tee /etc/udev/rules.d/99-voxctrl.rules",
      ...permissions,
    },
    hotkeys_active: true,
    model_ready: true,
    model_size: "tiny",
    model_auto_downloads: true,
    missing_injection_tool: null,
    is_complete: true,
    ...status,
  };
}

function mockStatus(overrides?: Overrides) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_setup_status") return setupStatus(overrides);
    return {};
  });
}

describe("Setup window", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  test("offers a restart instead of a logout when VoxCtrl can recover on its own", async () => {
    // The whole point of the change: a user who has not logged out gets a
    // button, not an instruction to end their desktop session.
    mockStatus({
      permissions: {
        is_configured: false,
        in_group: false,
        needs_relogin: true,
        can_relaunch: true,
        devices_readable: 0,
        detail: "Restart VoxCtrl to finish — you do not need to log out.",
      },
      status: { hotkeys_active: false, is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText("Restart VoxCtrl to finish")).toBeTruthy();
    expect(screen.queryByText(/Log out and\s+back in/i)).toBeNull();
  });

  test("falls back to the logout instruction only when it cannot recover", async () => {
    mockStatus({
      permissions: {
        is_configured: false,
        in_group: false,
        needs_relogin: true,
        can_relaunch: false,
        devices_readable: 0,
        detail: "Your desktop session has not picked the permissions up.",
      },
      status: { hotkeys_active: false, is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText(/log out and/i)).toBeTruthy();
    expect(screen.queryByText("Restart VoxCtrl to finish")).toBeNull();
  });

  test("treats permissions as unfinished while the listener has no keyboard", async () => {
    // The udev rule existing is not the same as hotkeys working; reporting
    // success here is what made the old flow feel broken.
    mockStatus({
      permissions: { detail: "Rule installed." },
      status: { hotkeys_active: false, is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText("Finish setting up VoxCtrl")).toBeTruthy();
    expect(await screen.findByText("Grant keyboard access")).toBeTruthy();
  });

  test("surfaces a missing keystroke-injection tool as its own step", async () => {
    mockStatus({
      status: { missing_injection_tool: "wtype", is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText("wtype")).toBeTruthy();
    expect(
      await screen.findByText(/cannot be\s+typed into the focused window/i),
    ).toBeTruthy();
  });

  test("asks the user to download only models that do not download themselves", async () => {
    mockStatus({
      status: {
        model_ready: false,
        model_size: "large-v3",
        model_auto_downloads: false,
        is_complete: false,
      },
    });

    render(UdevWarning);

    expect(await screen.findByText("Download large-v3")).toBeTruthy();
    expect(await screen.findByText("Choose a different model")).toBeTruthy();
  });

  test("shows the background download as progress, not as a chore", async () => {
    mockStatus({
      status: { model_ready: false, model_size: "tiny", model_auto_downloads: true, is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText(/no action needed/i)).toBeTruthy();
    expect(screen.queryByText("Download tiny")).toBeNull();
  });

  test("reports a finished setup", async () => {
    mockStatus();

    render(UdevWarning);

    expect(await screen.findByText("VoxCtrl is ready")).toBeTruthy();
    expect(await screen.findByText("Close")).toBeTruthy();
  });
});
