import { describe, expect, it } from "vitest";
import { formatShortcut, keyToken, readShortcut } from "./shortcut";
import type { ShortcutEvent } from "./shortcut";

/// A key press as the browser reports it, with nothing held unless said.
function press(code: string, held: Partial<ShortcutEvent> = {}): ShortcutEvent {
  return { code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...held };
}

describe("reading a shortcut off the keyboard", () => {
  it("writes a combination the way Utterform stores it", () => {
    const reading = readShortcut(press("KeyK", { ctrlKey: true, altKey: true }));
    expect(reading).toEqual({ status: "captured", shortcut: "Ctrl+Alt+K" });
  });

  it("keeps one order no matter which modifier arrives first", () => {
    const all = press("KeyD", { metaKey: true, shiftKey: true, altKey: true, ctrlKey: true });
    expect(readShortcut(all)).toEqual({ status: "captured", shortcut: "Ctrl+Alt+Shift+Super+D" });
  });

  it("stores the Mac's command key as Super and shows it as Cmd", () => {
    // The parser in hotkey.rs takes Super, Cmd and Command for the same key;
    // one spelling is stored so a settings file cannot end up with three.
    const reading = readShortcut(press("KeyD", { metaKey: true }), true);
    expect(reading).toEqual({ status: "captured", shortcut: "Super+D" });
    expect(formatShortcut("Super+D", true)).toBe("Cmd+D");
    expect(formatShortcut("Super+D")).toBe("Super+D");
  });

  it("takes the physical key, so a layout cannot change the shortcut", () => {
    // On a German keyboard this key types Z, and on an American one Y; the
    // shortcut has to be the same one either way.
    expect(readShortcut(press("KeyY", { ctrlKey: true }))).toEqual({ status: "captured", shortcut: "Ctrl+Y" });
  });

  it("reads space, digits, function keys, arrows and punctuation", () => {
    for (const [code, expected] of [
      ["Space", "Ctrl+Alt+Space"],
      ["Digit7", "Ctrl+Alt+7"],
      ["F9", "Ctrl+Alt+F9"],
      ["ArrowUp", "Ctrl+Alt+Up"],
      ["Enter", "Ctrl+Alt+Enter"],
      ["Comma", "Ctrl+Alt+Comma"],
      ["Numpad5", "Ctrl+Alt+Numpad5"],
    ] as const) {
      expect(readShortcut(press(code, { ctrlKey: true, altKey: true }))).toEqual({
        status: "captured",
        shortcut: expected,
      });
    }
  });

  it("waits while only modifiers are held", () => {
    for (const code of ["ControlLeft", "AltRight", "ShiftLeft", "MetaLeft"]) {
      expect(readShortcut(press(code, { ctrlKey: true })).status).toBe("waiting");
    }
  });

  it("waits through key repeat, so holding a key reads once", () => {
    expect(readShortcut(press("KeyK", { ctrlKey: true, repeat: true })).status).toBe("waiting");
  });

  it("waits for a key it has no name for", () => {
    // Reserving something the Rust parser cannot read back would store a
    // shortcut that fails the moment it is registered.
    for (const code of ["IntlBackslash", "ContextMenu", "CapsLock", "Lang1", ""]) {
      expect(readShortcut(press(code, { ctrlKey: true })).status).toBe("waiting");
    }
  });

  it("refuses a key on its own and says what is missing", () => {
    const reading = readShortcut(press("KeyD"));
    expect(reading.status).toBe("unmodified");
    if (reading.status !== "unmodified") return;
    expect(reading.message).toContain("D needs a modifier");
    expect(reading.message).toContain("Ctrl, Alt, Shift or Super");
    expect(readShortcut(press("F9"), true)).toMatchObject({ message: expect.stringContaining("Cmd") });
  });
});

describe("showing a shortcut", () => {
  it("reads punctuation and long names out as they are typed", () => {
    expect(formatShortcut("Ctrl+Alt+Comma")).toBe("Ctrl+Alt+,");
    expect(formatShortcut("Ctrl+Shift+PageUp")).toBe("Ctrl+Shift+Page Up");
    expect(formatShortcut("Alt+NumpadAdd")).toBe("Alt+Numpad +");
  });

  it("leaves a shortcut that was already readable alone", () => {
    for (const spec of ["Ctrl+Alt+D", "Alt+F9", "Ctrl+Shift+Space", "Ctrl+Alt+Up"]) {
      expect(formatShortcut(spec)).toBe(spec);
    }
  });
});

describe("the keys a shortcut can be made of", () => {
  it("names every letter, digit and function key the parser knows", () => {
    expect(keyToken("KeyA")).toBe("A");
    expect(keyToken("KeyZ")).toBe("Z");
    expect(keyToken("Digit0")).toBe("0");
    expect(keyToken("F24")).toBe("F24");
    expect(keyToken("F25")).toBeNull();
  });
});
