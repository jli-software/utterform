import { describe, expect, it } from "vitest";
import { SETTINGS_CATEGORIES, SETTINGS_ROWS, rowMatches, visibleRows } from "./settings-catalog";

describe("the settings catalog", () => {
  it("gives every row a known category and a unique id", () => {
    const categories = new Set(SETTINGS_CATEGORIES.map((category) => category.id));
    expect(SETTINGS_ROWS.every((row) => categories.has(row.category))).toBe(true);
    expect(new Set(SETTINGS_ROWS.map((row) => row.id)).size).toBe(SETTINGS_ROWS.length);
  });

  it("shows a category's own rows when nothing is searched", () => {
    expect([...visibleRows("recording", "  ")]).toEqual(["microphone", "shortcut", "sounds"]);
  });

  it("finds rows across categories by label, keyword or category, ignoring case", () => {
    expect([...visibleRows("recording", "KEY")]).toEqual(["shortcut", "api-key", "typing", "typing-delay"]);
    expect([...visibleRows("app", "mic")]).toEqual(["microphone"]);
    expect([...visibleRows("app", "output folder")]).toEqual(["folder"]);
    expect(visibleRows("app", "xylophone").size).toBe(0);
  });

  it("needs every word of a search", () => {
    const model = SETTINGS_ROWS.find((row) => row.id === "local-model")!;
    expect(rowMatches(model, "local mod")).toBe(true);
    expect(rowMatches(model, "local theme")).toBe(false);
  });
});
