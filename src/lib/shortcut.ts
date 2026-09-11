/// Reading a dictation key off the keyboard, in the one spelling Utterform
/// already uses everywhere else.
///
/// The physical key comes from `KeyboardEvent.code`, never from `event.key`:
/// `code` names the key's place on the keyboard, so `Ctrl+Alt+Y` is the same
/// combination on a German and on an American layout, and a dead key or an
/// AltGr character cannot turn into a shortcut nobody can press again.
///
/// Every token produced here is one the Rust parser in
/// `src-tauri/src/hotkey.rs` accepts — `Ctrl+Alt+D`, `Alt+F9`,
/// `Ctrl+Shift+Space` — so what is shown, what is stored in `settings.json`
/// and what is registered with the operating system stay one string. Rust
/// still has the last word when the settings are saved; nothing here is a
/// second shortcut syntax.

/// Written in the order Utterform has always written them, which is also the
/// order macOS shows in its own menus (⌃⌥⇧⌘).
const MODIFIERS = [
  { held: (event: ShortcutEvent) => event.ctrlKey, token: "Ctrl" },
  { held: (event: ShortcutEvent) => event.altKey, token: "Alt" },
  { held: (event: ShortcutEvent) => event.shiftKey, token: "Shift" },
  // `Super` is what the parser takes; macOS is shown `Cmd` for the same key.
  { held: (event: ShortcutEvent) => event.metaKey, token: "Super" },
];

/// Keys that are neither letters, digits, function nor numpad keys. Lock keys
/// are deliberately absent: a shortcut made of one would fire while the
/// keyboard is changing state.
const NAMED_KEYS = new Map<string, string>([
  ["Space", "Space"],
  ["Enter", "Enter"],
  ["Tab", "Tab"],
  ["Backspace", "Backspace"],
  ["Delete", "Delete"],
  ["Insert", "Insert"],
  ["Home", "Home"],
  ["End", "End"],
  ["PageUp", "PageUp"],
  ["PageDown", "PageDown"],
  ["ArrowUp", "Up"],
  ["ArrowDown", "Down"],
  ["ArrowLeft", "Left"],
  ["ArrowRight", "Right"],
  ["Minus", "Minus"],
  ["Equal", "Equal"],
  ["Comma", "Comma"],
  ["Period", "Period"],
  ["Slash", "Slash"],
  ["Semicolon", "Semicolon"],
  ["Quote", "Quote"],
  ["Backquote", "Backquote"],
  ["BracketLeft", "BracketLeft"],
  ["BracketRight", "BracketRight"],
  ["Backslash", "Backslash"],
]);

/// How a stored token is read out to a person: the punctuation as the
/// character it types, the rest as words rather than run together.
const FRIENDLY: Record<string, string> = {
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  PageUp: "Page Up",
  PageDown: "Page Down",
  NumpadAdd: "Numpad +",
  NumpadSubtract: "Numpad -",
  NumpadMultiply: "Numpad *",
  NumpadDivide: "Numpad /",
  NumpadDecimal: "Numpad .",
  NumpadEnter: "Numpad Enter",
};

/// Only the parts of a keyboard event a shortcut is made of, so the reading
/// can be tested without a browser.
export interface ShortcutEvent {
  code: string;
  repeat?: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type ShortcutReading =
  /// Nothing to act on yet: a held modifier, a key repeat, or a key that has
  /// no name the shortcut parser knows.
  | { status: "waiting" }
  /// A complete combination, spelled the way it will be stored.
  | { status: "captured"; shortcut: string }
  /// A key on its own. Reserving one system-wide would stop it typing in
  /// every other application, so it is refused here as it is in Rust.
  | { status: "unmodified"; message: string };

/// The token for the physical key, or `null` for a key that cannot be part of
/// a shortcut — a modifier on its own included.
export function keyToken(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code;
  if (/^Numpad(\d|Add|Subtract|Multiply|Divide|Decimal|Enter)$/.test(code)) return code;
  return NAMED_KEYS.get(code) ?? null;
}

/// What a key press means while a shortcut is being read.
export function readShortcut(event: ShortcutEvent, apple = false): ShortcutReading {
  // A held key reports again and again; the first report is the whole event.
  if (event.repeat) return { status: "waiting" };
  const key = keyToken(event.code);
  if (!key) return { status: "waiting" };
  const modifiers = MODIFIERS.filter((modifier) => modifier.held(event)).map((modifier) => modifier.token);
  if (!modifiers.length) {
    const held = apple ? "Ctrl, Option, Shift or Cmd" : "Ctrl, Alt, Shift or Super";
    return {
      status: "unmodified",
      message: `${formatShortcut(key, apple)} needs a modifier — hold ${held} and press it again.`,
    };
  }
  return { status: "captured", shortcut: [...modifiers, key].join("+") };
}

/// The same combination, written for the person looking at it. macOS calls the
/// key it stores as `Super` ⌘, so it is shown there as `Cmd`.
export function formatShortcut(shortcut: string, apple = false): string {
  return shortcut
    .split("+")
    .map((token) => token.trim())
    .filter(Boolean)
    .map((token) => {
      if (token === "Super") return apple ? "Cmd" : "Super";
      return FRIENDLY[token] ?? token;
    })
    .join("+");
}

/// Whether this is a Mac, where the same physical key is called ⌘ rather than
/// Super. Read from the browser because the interface has no other way to ask.
export function isApplePlatform(): boolean {
  return /Mac|iPhone|iPad/.test(navigator.platform);
}
