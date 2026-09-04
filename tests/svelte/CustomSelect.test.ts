import { describe, test, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/svelte";
import CustomSelect from "../../src/lib/Settings/CustomSelect.svelte";

const options = [
  { value: "a", label: "Option A" },
  { value: "b", label: "Option B" },
  { value: "c", label: "Option C" },
];

/**
 * jsdom lays nothing out, so the trigger's box is whatever we say it is. This
 * is what decides where the menu goes.
 */
function stubTriggerRect(top: number, height = 36) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    top,
    bottom: top + height,
    left: 20,
    right: 320,
    width: 300,
    height,
    x: 20,
    y: top,
    toJSON: () => ({}),
  } as DOMRect);
}

async function openMenu(container: HTMLElement) {
  const trigger = container.querySelector(".custom-select-trigger") as HTMLElement;
  await fireEvent.click(trigger);
  return container.querySelector(".custom-dropdown-menu") as HTMLElement;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("CustomSelect dropdown placement", () => {
  test("shows every option when opened", async () => {
    stubTriggerRect(100);
    const { container } = render(CustomSelect, { value: "a", options });
    const menu = await openMenu(container);

    // Scoped to the menu: the selected label also shows in the trigger.
    for (const opt of options) {
      expect(within(menu).getByText(opt.label)).not.toBeNull();
    }
  });

  /**
   * The settings panels and the target editor scroll their content, and an
   * absolutely positioned menu is clipped by that overflow — the bug that hid
   * half the delivery list. Fixed positioning is what escapes the clip.
   */
  test("positions the menu with fixed coordinates so a scrolling panel cannot clip it", async () => {
    stubTriggerRect(100);
    const { container } = render(CustomSelect, { value: "a", options });
    const menu = await openMenu(container);

    expect(menu.style.position).toBe("fixed");
    expect(menu.style.left).toBe("20px");
    expect(menu.style.width).toBe("300px");
    // Opens downward from the bottom edge of the trigger.
    expect(menu.style.top).toBe("140px");
    expect(menu.style.bottom).toBe("");
  });

  test("caps the menu height to the space it has on screen", async () => {
    stubTriggerRect(100);
    const { container } = render(CustomSelect, { value: "a", options });
    const menu = await openMenu(container);

    const maxHeight = parseInt(menu.style.maxHeight, 10);
    expect(maxHeight).toBeGreaterThan(0);
    expect(maxHeight).toBeLessThanOrEqual(window.innerHeight);
  });

  test("flips above the trigger when there is no room below", async () => {
    // A trigger near the bottom of the viewport has nowhere to open downward.
    stubTriggerRect(window.innerHeight - 60);
    const { container } = render(CustomSelect, { value: "a", options });
    const menu = await openMenu(container);

    expect(menu.style.position).toBe("fixed");
    expect(menu.style.bottom).not.toBe("");
    expect(menu.style.top).toBe("");
  });

  test("selecting an option closes the menu and reports the value", async () => {
    stubTriggerRect(100);
    const onchange = vi.fn();
    const { container } = render(CustomSelect, { value: "a", options, onchange });
    await openMenu(container);

    await fireEvent.click(screen.getByText("Option B"));

    expect(onchange).toHaveBeenCalledWith("b");
    expect(container.querySelector(".custom-dropdown-menu")).toBeNull();
  });
});
