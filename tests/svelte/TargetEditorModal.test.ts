import { describe, test, expect, vi, afterEach } from "vitest";
import { render, fireEvent, within } from "@testing-library/svelte";
import TargetEditorModal from "../../src/lib/Settings/TargetEditorModal.svelte";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => true),
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
    processing: {},
  } as any;
}

function renderModal() {
  return render(TargetEditorModal, {
    editingTarget: newTarget(),
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
