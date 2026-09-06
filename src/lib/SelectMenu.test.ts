// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/svelte";
import SelectMenu from "./SelectMenu.svelte";

afterEach(cleanup);
const options = [
  { value: "plain", label: "Plain", hint: "Transcription only" },
  { value: "polish", label: "Polish", hint: "Rewrite for clarity" },
  { value: "summary", label: "Summarize" },
];

describe("themed select menu", () => {
  it("supports arrow navigation, selection and escape without leaking shortcuts", async () => {
    const onchange = vi.fn();
    const { getByRole, queryByRole } = render(SelectMenu, { id: "action", label: "Action", value: "plain", options, onchange });
    const trigger = getByRole("combobox");
    const windowKey = vi.fn();
    window.addEventListener("keydown", windowKey);
    await fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(getByRole("listbox")).toBeTruthy();
    await fireEvent.keyDown(trigger, { key: "ArrowDown" });
    await fireEvent.keyDown(trigger, { key: "Enter" });
    expect(onchange).toHaveBeenCalledWith("polish");
    expect(trigger.textContent).toContain("Polish");
    expect(queryByRole("listbox")).toBeNull();
    await fireEvent.click(trigger);
    await fireEvent.keyDown(trigger, { key: "Escape" });
    expect(queryByRole("listbox")).toBeNull();
    expect(windowKey).not.toHaveBeenCalled();
    window.removeEventListener("keydown", windowKey);
  });

  it("selects by pointer and closes on outside interaction", async () => {
    const onchange = vi.fn();
    const { getByRole, queryByRole } = render(SelectMenu, { id: "action", label: "Action", value: "plain", options, onchange });
    await fireEvent.click(getByRole("combobox"));
    await fireEvent.click(getByRole("option", { name: "Summarize" }));
    expect(onchange).toHaveBeenCalledWith("summary");
    await fireEvent.click(getByRole("combobox"));
    await fireEvent.pointerDown(document.body);
    expect(queryByRole("listbox")).toBeNull();
  });

  it("supports typeahead and disables an already open menu", async () => {
    const { getByRole, queryByRole, rerender } = render(SelectMenu, { id: "action", label: "Action", value: "plain", options });
    const trigger = getByRole("combobox");
    await fireEvent.keyDown(trigger, { key: "s" });
    expect(trigger.getAttribute("aria-activedescendant")).toBe("action-option-2");
    await rerender({ disabled: true });
    expect(queryByRole("listbox")).toBeNull();
    expect((trigger as HTMLButtonElement).disabled).toBe(true);
  });
});
