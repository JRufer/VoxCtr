import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import UdevWarning from "../../src/lib/Diagnostics/UdevWarning.svelte";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({ close: vi.fn() })),
}));

type Overrides = {
  hotkeys?: Record<string, unknown>;
  status?: Record<string, unknown>;
};

function setupStatus({ hotkeys = {}, status = {} }: Overrides = {}) {
  return {
    hotkeys: {
      is_active: true,
      backend: "portal",
      is_private: true,
      portal_error: null,
      portal_refused: false,
      shortcuts: [
        {
          binding_ids: ["default_hold"],
          requested: "LOGO+space",
          trigger_description: "Meta+Space",
          bound: true,
        },
      ],
      session_type: "wayland",
      devices_total: 0,
      devices_readable: 0,
      needs_attention: false,
      needs_manual_enable: false,
      manual_enable_hint: null,
      detail: "Your desktop is handling VoxCtrl's global shortcuts.",
      ...hotkeys,
    },
    hotkeys_active: true,
    model_ready: true,
    model_size: "tiny",
    model_auto_downloads: true,
    missing_injection_tool: null,
    pkexec_available: true,
    manual_package_commands: "sudo sh -c 'apt-get install -y wtype'",
    is_complete: true,
    ...status,
  };
}

function mockStatus(overrides?: Overrides) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_setup_status") return setupStatus(overrides);
    if (cmd === "open_shortcut_settings") return null;
    return {};
  });
}

describe("Setup window", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  test("tells the user their desktop owns the shortcuts and VoxCtrl reads nothing", async () => {
    // The reassurance is the point of the whole first-launch screen: on the
    // portal path there is no permission to grant and no keyboard being read.
    mockStatus();

    render(UdevWarning);

    expect(await screen.findByText(/does not read your keyboard/i)).toBeTruthy();
    expect(await screen.findByText(/needed no special permissions/i)).toBeTruthy();
    expect(await screen.findByText("Meta+Space")).toBeTruthy();
  });

  test("never offers to grant keyboard access", async () => {
    // Regression guard for the change this screen exists to make. Whatever the
    // state, the setup window must not present a button that widens the
    // machine's input permissions.
    for (const backend of ["portal", "x11", "mint_dbus", "evdev", "none", "starting"]) {
      invoke.mockReset();
      mockStatus({
        hotkeys: {
          backend,
          is_active: backend !== "none",
          is_private: backend === "portal" || backend === "mint_dbus",
        },
        status: { hotkeys_active: backend !== "none", is_complete: false },
      });

      const { unmount } = render(UdevWarning);
      await screen.findByText("Global shortcuts");

      expect(screen.queryByText(/grant keyboard access/i)).toBeNull();
      expect(screen.queryByText(/restart voxctrl to finish/i)).toBeNull();
      expect(screen.queryByText(/log out and/i)).toBeNull();
      expect(screen.queryByText(/udev/i)).toBeNull();
      unmount();
    }
  });

  test("explains the evdev fallback honestly instead of hiding it", async () => {
    mockStatus({
      hotkeys: {
        backend: "evdev",
        is_private: false,
        portal_error: "no such interface",
        devices_total: 8,
        devices_readable: 8,
        detail: "VoxCtrl is reading input devices directly.",
      },
      status: { is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText(/every keystroke passes through VoxCtrl/i)).toBeTruthy();
    expect(await screen.findByText(/neither requested nor configured/i)).toBeTruthy();
  });

  test("says why it will not grant itself access when nothing can deliver shortcuts", async () => {
    mockStatus({
      hotkeys: {
        is_active: false,
        backend: "none",
        is_private: false,
        portal_error: "no such interface",
        shortcuts: [],
        devices_total: 8,
        devices_readable: 0,
        needs_attention: true,
        detail: "This desktop does not provide the XDG global-shortcuts portal.",
      },
      status: { hotkeys_active: false, is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText(/will not grant itself keyboard access/i)).toBeTruthy();
    expect(await screen.findByText(/Portal reported: no such interface/i)).toBeTruthy();
  });

  test("explains shortcut approval is required and provides an approve button when refused", async () => {
    mockStatus({
      hotkeys: {
        is_active: false,
        backend: "none",
        is_private: false,
        portal_error: "org.freedesktop.portal.Error.NotAllowed: An app id is required",
        portal_refused: true,
        shortcuts: [],
        needs_attention: true,
        detail: "Global shortcuts require approval from your desktop. Click Approve Keybinds to display the prompt and confirm your keybinds.",
      },
      status: { hotkeys_active: false, is_complete: false },
    });

    render(UdevWarning);

    expect((await screen.findAllByText(/require approval/i)).length).toBeGreaterThan(0);
    expect(await screen.findByRole("button", { name: /Approve Shortcuts|Approve Keybinds/i })).toBeTruthy();
    expect(await screen.findByText(/An app id is required/i)).toBeTruthy();
  });

  test("does not flash a failure while the portal handshake is still running", async () => {
    mockStatus({
      hotkeys: { backend: "starting", detail: "Starting the shortcut listener…" },
      status: { is_complete: false },
    });

    render(UdevWarning);

    await screen.findByText("Global shortcuts");
    expect(screen.queryByText(/will not grant itself/i)).toBeNull();
    expect(screen.queryByText(/every keystroke passes through/i)).toBeNull();
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
    expect(await screen.findByText("Install it")).toBeTruthy();
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

  test("flags shortcuts KDE registered but left disabled, with a button to fix it", async () => {
    mockStatus({
      hotkeys: {
        needs_manual_enable: true,
        manual_enable_hint:
          "KDE registers VoxCtrl's shortcuts disabled by default (KDE bug 483639). Open Shortcut Settings, tick the box next to each VoxCtrl shortcut, and press Apply.",
      },
      status: { hotkeys_active: false, is_complete: false },
    });

    render(UdevWarning);

    expect(await screen.findByText(/One more step on KDE/i)).toBeTruthy();
    expect(await screen.findByText(/KDE bug 483639/i)).toBeTruthy();
    const btn = await screen.findByRole("button", { name: /Open Shortcut Settings/i });
    await fireEvent.click(btn);

    expect(invoke).toHaveBeenCalledWith("open_shortcut_settings", undefined);
  });

  test("shows an error if opening shortcut settings fails", async () => {
    const overrides: Overrides = {
      hotkeys: {
        needs_manual_enable: true,
        manual_enable_hint: "Open Shortcut Settings and enable VoxCtrl's shortcuts.",
      },
      status: { hotkeys_active: false, is_complete: false },
    };
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_setup_status") return setupStatus(overrides);
      if (cmd === "open_shortcut_settings") {
        throw new Error("Could not find a way to open your desktop's shortcut settings automatically.");
      }
      return {};
    });

    render(UdevWarning);

    const btn = await screen.findByRole("button", { name: /Open Shortcut Settings/i });
    await fireEvent.click(btn);

    expect(await screen.findByText(/Could not find a way to open/i)).toBeTruthy();
  });

  test("does not show the manual-enable notice when the desktop needs no extra step", async () => {
    mockStatus();

    render(UdevWarning);

    await screen.findByText("Global shortcuts");
    expect(screen.queryByText(/One more step on KDE/i)).toBeNull();
  });

  test("reports a finished setup", async () => {
    mockStatus();

    render(UdevWarning);

    expect(await screen.findByText("VoxCtrl is ready")).toBeTruthy();
    expect(await screen.findByText("Close")).toBeTruthy();
  });
});
