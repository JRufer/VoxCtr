import { describe, test, expect, vi, afterEach } from "vitest";
import { render, fireEvent, within, waitFor } from "@testing-library/svelte";
import { tick } from "svelte";
import TargetEditorModal from "../../src/lib/Settings/TargetEditorModal.svelte";

const invokeMock = vi.hoisted(() => vi.fn(async () => true as any));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

function newTarget() {
  return {
    id: "new_target",
    label: "New Target",
    delivery: "inject",
    file_prefix: "",
    file_timestamp: false,
    http_method: "POST",
    chat_max_history: 20,
    chat_timeout_secs: 120,
    chat_reply_mode: "speak",
    strip_newlines: false,
    file_timestamp_format: "%Y-%m-%dT%H:%M:%SZ",
    processing: {},
  } as any;
}

function renderModal(overrides: Record<string, unknown> = {}) {
  return render(TargetEditorModal, {
    editingTarget: { ...newTarget(), ...overrides },
    isNew: true,
    existingTargets: [],
    onSave: () => {},
    onCancel: () => {},
  });
}

/** Open the Delivery System select and return its option labels, in order. */
async function deliveryOptions(container: HTMLElement) {
  const triggers = container.querySelectorAll(".custom-select-trigger");
  const trigger = triggers[0] as HTMLElement;
  await fireEvent.click(trigger);
  const menu = container.querySelector(".custom-dropdown-menu") as HTMLElement;
  return {
    menu,
    labels: within(menu)
      .getAllByRole("button")
      .map(b => b.textContent?.trim() ?? ""),
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("TargetEditorModal delivery selector", () => {
  test("lists Voice Command Router first", async () => {
    const { container } = renderModal();
    const { labels } = await deliveryOptions(container);

    expect(labels[0]).toContain("Voice Command Router");
  });

  test("offers every delivery type", async () => {
    const { container } = renderModal();
    const { labels } = await deliveryOptions(container);

    for (const expected of [
      "Voice Command Router",
      "Inject Text Directly",
      "Save to Clipboard",
      "Execute Command",
      "Write to File",
      "FIFO Named Pipe",
      "TCP / Unix Socket",
      "DBus Signal",
      "HTTP Custom Client",
      "Send Webhook Event",
      "Call MCP Server Tool",
      "Speak Text Aloud",
      "Chat with a Local LLM",
    ]) {
      expect(labels.some(l => l.includes(expected))).toBe(true);
    }
  });

  /**
   * The modal body scrolls, so a menu positioned inside it was clipped — the
   * lower half of this list used to be unreachable.
   */
  test("opens the list outside the scrolling modal body", async () => {
    const { container } = renderModal();
    const { menu } = await deliveryOptions(container);

    expect(menu.style.position).toBe("fixed");
  });
});

describe("TargetEditorModal command name", () => {
  test("labels the target's name as the Command Name and explains what it is for", () => {
    const { container } = renderModal();

    expect(container.textContent).toContain("Command Name");
    expect(container.textContent).not.toContain("Display Label");
    // The note tells the user this is the spoken name.
    expect(container.textContent).toMatch(/VoxCtrl,/);
  });
});

describe("TargetEditorModal file timestamp format", () => {
  const fileTarget = { delivery: "file", file_path: "/tmp/notes.md", file_timestamp: true };

  test("shows a format field and the specifier note only while timestamps are on", async () => {
    invokeMock.mockResolvedValue("2026-09-04T17:05:09Z");

    const withStamp = renderModal(fileTarget);
    await tick();
    expect(withStamp.container.textContent).toContain("Timestamp Format");
    expect(withStamp.container.textContent).toContain("%Y");
    withStamp.unmount();

    const withoutStamp = renderModal({ ...fileTarget, file_timestamp: false });
    await tick();
    expect(withoutStamp.container.textContent).not.toContain("Timestamp Format");
  });

  test("previews the rendered timestamp returned by the backend", async () => {
    invokeMock.mockResolvedValue("2026-09-04T17:05:09Z");

    const { container } = renderModal(fileTarget);

    expect(invokeMock).toHaveBeenCalledWith("preview_timestamp_format", {
      format: "%Y-%m-%dT%H:%M:%SZ",
    });
    await waitFor(() => expect(container.textContent).toContain("2026-09-04T17:05:09Z"));
  });

  test("flags a format the backend rejects", async () => {
    invokeMock.mockRejectedValue("Not a valid timestamp format — check the % specifiers.");

    const { container } = renderModal({ ...fileTarget, file_timestamp_format: "%Q" });

    await waitFor(() =>
      expect(container.textContent).toContain("Not a valid timestamp format")
    );
    const input = container.querySelector("input.border-red-500\\!");
    expect(input, "the invalid format field should be marked").not.toBeNull();
  });
});
