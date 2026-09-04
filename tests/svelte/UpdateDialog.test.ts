import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import UpdateDialog from "../../src/lib/Update/UpdateDialog.svelte";
import { formatBytes, progressPercent } from "../../src/lib/Update/update-types";

const invoke = vi.fn();
const openExternal = vi.fn();

/** Handlers registered by the component, so a test can fire a backend event. */
const listeners = new Map<string, (event: { payload: unknown }) => void>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  },
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: (url: string) => openExternal(url),
}));

function updateInfo(overrides: Record<string, unknown> = {}) {
  return {
    version: "0.4.0",
    tag: "v0.4.0",
    current_version: "0.3.10",
    notes: "Adds a thing.",
    release_url: "https://github.com/JRufer/VoxCtrl/releases/tag/v0.4.0",
    asset_name: "VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage",
    download_size: 98_000_000,
    can_self_update: true,
    unsupported_reason: null,
    ...overrides,
  };
}

/** Answer `get_pending_update` with `update`, and nothing else unexpected. */
function mockPending(update: Record<string, unknown> | null) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_pending_update" || cmd === "check_for_update") {
      return { current_version: "0.3.10", update, skipped: false };
    }
    return null;
  });
}

describe("Update dialog", () => {
  beforeEach(() => {
    invoke.mockReset();
    openExternal.mockReset();
    listeners.clear();
  });

  test("names both versions, so the user knows what they are moving from and to", async () => {
    mockPending(updateInfo());
    render(UpdateDialog);

    expect(await screen.findByText(/VoxCtrl 0\.4\.0 is available/)).toBeTruthy();
    expect(screen.getByText(/You have 0\.3\.10/)).toBeTruthy();
    expect(screen.getByText("Adds a thing.")).toBeTruthy();
  });

  test("says how large the download is before asking the user to commit to it", async () => {
    mockPending(updateInfo());
    const { container } = render(UpdateDialog);

    await screen.findByText(/is available/);
    expect(container.textContent).toContain("93 MB");
  });

  test("'Not now' closes without installing, so the same update is offered next launch", async () => {
    mockPending(updateInfo());
    render(UpdateDialog);

    await fireEvent.click(await screen.findByText("Not now"));

    expect(invoke).toHaveBeenCalledWith("dismiss_update", undefined);
    expect(invoke).not.toHaveBeenCalledWith("install_update", expect.anything());
  });

  test("'Skip this version' records the version before closing", async () => {
    mockPending(updateInfo());
    render(UpdateDialog);

    await fireEvent.click(await screen.findByText("Skip this version"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("skip_update_version", { version: "0.4.0" }),
    );
    expect(invoke).toHaveBeenCalledWith("dismiss_update", undefined);
  });

  test("shows download progress while installing", async () => {
    mockPending(updateInfo());
    render(UpdateDialog);

    await fireEvent.click(await screen.findByText("Update and restart"));
    expect(invoke).toHaveBeenCalledWith("install_update", undefined);

    listeners.get("update-progress")?.({ payload: { downloaded: 49_000_000, total: 98_000_000 } });

    expect(await screen.findByText(/50%/)).toBeTruthy();
  });

  test("a failed install says so and reassures that the running version is intact", async () => {
    mockPending(updateInfo());
    render(UpdateDialog);

    await fireEvent.click(await screen.findByText("Update and restart"));
    listeners.get("update-failed")?.({ payload: "the download did not match its checksum" });

    expect(await screen.findByText(/did not match its checksum/)).toBeTruthy();
    expect(screen.getByText(/current version is untouched/)).toBeTruthy();
  });

  /// A `.deb` or distro install cannot be replaced by the app. Offering a
  /// button that cannot work would be worse than explaining why.
  test("an installation that cannot self-update is told why and offered the download page", async () => {
    mockPending(
      updateInfo({
        can_self_update: false,
        unsupported_reason: "This copy of VoxCtrl was installed by your package manager.",
      }),
    );
    render(UpdateDialog);

    expect(await screen.findByText(/installed by your package manager/)).toBeTruthy();
    expect(screen.queryByText("Update and restart")).toBeNull();

    await fireEvent.click(screen.getByText("Open download page"));
    expect(openExternal).toHaveBeenCalledWith(
      "https://github.com/JRufer/VoxCtrl/releases/tag/v0.4.0",
    );
  });

  test("turning off automatic checks from the dialog reaches the backend", async () => {
    mockPending(updateInfo());
    render(UpdateDialog);

    await fireEvent.click(await screen.findByText("Stop checking automatically"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_update_auto_check", { enabled: false }),
    );
  });

  test("an up-to-date app says so rather than showing an empty dialog", async () => {
    mockPending(null);
    render(UpdateDialog);

    expect(await screen.findByText(/VoxCtrl is up to date/)).toBeTruthy();
    expect(screen.getByText(/0\.3\.10 is the latest release/)).toBeTruthy();
  });
});

describe("update formatting helpers", () => {
  test("sizes read the way a download dialog should", () => {
    expect(formatBytes(0)).toBe("");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024 * 1024 * 98)).toBe("98 MB");
    expect(formatBytes(1024 * 1024 * 1.5)).toBe("1.5 MB");
  });

  test("an unknown total gives no percentage rather than a wrong one", () => {
    expect(progressPercent(null)).toBeNull();
    expect(progressPercent({ downloaded: 10, total: 0 })).toBeNull();
    expect(progressPercent({ downloaded: 50, total: 200 })).toBe(25);
    // A server that over-delivers must not produce "104%".
    expect(progressPercent({ downloaded: 210, total: 200 })).toBe(100);
  });
});
