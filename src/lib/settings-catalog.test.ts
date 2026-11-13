import { describe, expect, it } from "vitest";
import { SETTINGS_CATEGORIES, SETTINGS_ROWS, rowMatches, visibleRows } from "./settings-catalog";

describe("the settings catalog", () => {
  it("gives every row a known category and a unique id", () => {
    const categories = new Set(SETTINGS_CATEGORIES.map((category) => category.id));
    expect(SETTINGS_ROWS.every((row) => categories.has(row.category))).toBe(true);
    expect(new Set(SETTINGS_ROWS.map((row) => row.id)).size).toBe(SETTINGS_ROWS.length);
  });

  it("shows a category's own rows when nothing is searched", () => {
    expect([...visibleRows("recording", "  ")]).toEqual(["microphone", "mic-level", "shortcut", "sounds"]);
  });

  it("finds rows across categories by label, keyword or category, ignoring case", () => {
    expect([...visibleRows("recording", "KEY")]).toEqual(["shortcut", "api-key", "typing", "typing-delay"]);
    expect([...visibleRows("app", "mic")]).toEqual(["microphone"]);
    expect([...visibleRows("app", "output folder")]).toEqual(["folder"]);
    expect(visibleRows("app", "xylophone").size).toBe(0);
    expect([...visibleRows("app", "chatgpt")]).toEqual(["text-provider"]);
    expect([...visibleRows("app", "codex")]).toEqual(["text-provider"]);
    expect([...visibleRows("app", "selected")]).toEqual(["context-selection"]);
  });

  it("gives the Context action's sources and the actions per app a category of their own", () => {
    expect([...visibleRows("context", "")]).toEqual(["context-selection", "context-clipboard", "app-actions"]);
    expect([...visibleRows("app", "per app")]).toEqual(["app-actions"]);
    expect([...visibleRows("app", "topic")]).toEqual(["context"]);
  });

  it("needs every word of a search", () => {
    const model = SETTINGS_ROWS.find((row) => row.id === "local-model")!;
    expect(rowMatches(model, "local mod")).toBe(true);
    expect(rowMatches(model, "local theme")).toBe(false);
  });
});
