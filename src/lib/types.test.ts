import { describe, expect, it } from "vitest";
import {
  BUILT_IN_ACTIONS,
  DEFAULT_SETTINGS,
  FORBIDDEN_IN_KEYWORD,
  actionName,
  actionPrompt,
  isActionEdited,
} from "./types";

describe("default product configuration", () => {
  it("keeps output safe and immediately useful", () => {
    expect(DEFAULT_SETTINGS.copy_to_clipboard).toBe(true);
    expect(DEFAULT_SETTINGS.save_to_file).toBe(false);
    expect(DEFAULT_SETTINGS.engine).toBe("open_ai");
  });

  it("edits nothing and sends no reasoning level until asked", () => {
    expect(DEFAULT_SETTINGS.action_overrides).toEqual({});
    expect(DEFAULT_SETTINGS.vocabulary).toEqual([]);
    expect(DEFAULT_SETTINGS.text_effort).toBeNull();
  });

  it("keeps action ids unique, and ships an email prompt", () => {
    expect(new Set(BUILT_IN_ACTIONS.map((action) => action.id)).size).toBe(BUILT_IN_ACTIONS.length);
    expect(BUILT_IN_ACTIONS.map((action) => action.id)).toContain("email");
  });
});

describe("resolving an action against the user's edits", () => {
  const shipped = { id: "clean", name: "Clean", hint: "Fix punctuation", prompt: "Correct the punctuation." };

  it("uses what we ship until something replaces it", () => {
    expect(actionName(shipped, {})).toBe("Clean");
    expect(actionPrompt(shipped, {})).toBe("Correct the punctuation.");
    expect(isActionEdited("clean", {})).toBe(false);
  });

  it("prefers the user's name and instructions", () => {
    const overrides = { clean: { name: "Tidy", prompt: "Fix commas only." } };
    expect(actionName(shipped, overrides)).toBe("Tidy");
    expect(actionPrompt(shipped, overrides)).toBe("Fix commas only.");
    expect(isActionEdited("clean", overrides)).toBe(true);
  });

  it("falls back rather than running on nothing, exactly as the backend does", () => {
    const overrides = { clean: { name: "  ", prompt: "   " } };
    expect(actionName(shipped, overrides)).toBe("Clean");
    expect(actionPrompt(shipped, overrides)).toBe("Correct the punctuation.");
    expect(isActionEdited("clean", overrides)).toBe(false);
  });

  it("renaming an action leaves its instructions following ours", () => {
    const overrides = { clean: { name: "Tidy" } };
    expect(actionPrompt(shipped, overrides)).toBe("Correct the punctuation.");
  });
});

describe("vocabulary terms the transcription API refuses", () => {
  it("catches the angle brackets that would fail the whole request", () => {
    expect(FORBIDDEN_IN_KEYWORD.test("Careum")).toBe(false);
    expect(FORBIDDEN_IN_KEYWORD.test("<name>")).toBe(true);
    expect(FORBIDDEN_IN_KEYWORD.test("a > b")).toBe(true);
  });
});
