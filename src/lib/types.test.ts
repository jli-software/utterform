import { describe, expect, it } from "vitest";
import { ACTIONS, DEFAULT_SETTINGS } from "./types";

describe("default product configuration", () => {
  it("keeps output safe and immediately useful", () => {
    expect(DEFAULT_SETTINGS.copy_to_clipboard).toBe(true);
    expect(DEFAULT_SETTINGS.save_to_file).toBe(false);
    expect(DEFAULT_SETTINGS.engine).toBe("open_ai");
  });

  it("keeps action shortcut keys unique", () => {
    expect(new Set(ACTIONS.map((action) => action.key)).size).toBe(ACTIONS.length);
  });
});
