//! Putting the finished text into whatever window has focus.
//!
//! Two ways lead there, and they fail differently. Synthesized keystrokes send
//! one character at a time through the compositor, the input method and the
//! target application; any of them may drop or reorder one, which is how a
//! terminal turns "Session" into "ession". A paste hands over the whole text
//! at once, so nothing can be reordered — that is why it is the default, at
//! the price of leaving the text on the clipboard.
//!
//! Keystrokes stay available for the windows that refuse a paste, and get the
//! two fixes that make them survivable: a leading Shift tap, because some
//! Wayland clients swallow the first character a fresh virtual keyboard sends,
//! and a delay between keys, because input arriving faster than a human can
//! type is what makes them arrive out of order in the first place.

use tauri::{AppHandle, Runtime};

use crate::domain::TypingMethod;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// Slow enough for the terminals that drop characters, fast enough that a
/// sentence does not visibly crawl in.
pub const DEFAULT_DELAY_MS: u32 = 15;
const MAX_DELAY_MS: u32 = 500;

/// Keep a hand-edited settings file from stalling delivery for minutes.
pub fn clamp_delay(milliseconds: u32) -> u32 {
    milliseconds.min(MAX_DELAY_MS)
}

/// Which paste the focused window understands. Terminals reserve plain Ctrl+V
/// for the shell, so they take the text on Ctrl+Shift+V instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paste {
    Plain,
    Terminal,
}

/// Window classes that are terminal emulators. Guessing wrong costs a paste
/// that does not arrive, never the text: clipboard and file are already done
/// by the time typing runs, and a failed paste is reported as a warning.
const TERMINALS: &[&str] = &[
    "alacritty",
    "aterm",
    "blackbox",
    "console",
    "contour",
    "cool-retro-term",
    "extraterm",
    "foot",
    "footclient",
    "germinal",
    "ghostty",
    "guake",
    "havoc",
    "hyper",
    "kermit",
    "kgx",
    "kitty",
    "konsole",
    "qterminal",
    "rio",
    "roxterm",
    "rxvt",
    "sakura",
    "st",
    "terminator",
    "terminology",
    "tilix",
    "tym",
    "urxvt",
    "warp",
    "wave",
    "yakuake",
    "zutty",
];

/// Reverse-DNS classes (`org.gnome.Console`, `com.mitchellh.ghostty`) name the
/// application in their last segment; everything before it is the vendor.
pub fn is_terminal_class(class: &str) -> bool {
    let name = class
        .trim()
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    TERMINALS.contains(&name.as_str())
        || name.contains("terminal")
        || name.ends_with("term")
        || name.ends_with("term-gui")
}

/// An unknown window gets the paste every graphical toolkit agrees on.
pub fn paste_for(window_class: Option<&str>) -> Paste {
    match window_class {
        Some(class) if is_terminal_class(class) => Paste::Terminal,
        _ => Paste::Plain,
    }
}

/// One synthesized key event, independent of the platform that sends it.
///
/// Only Windows synthesizes keys itself; wtype and xdotool take the text whole
/// and do their own mapping. The rule still belongs here, where it is tested
/// on every platform rather than only where it runs.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character the window should receive verbatim, as a UTF-16 code unit.
    /// Characters outside the basic plane arrive as their two surrogates, in
    /// order, which is how the receiving window puts them back together.
    Unit(u16),
    /// A real Return press. A newline delivered as a character is inserted
    /// literally by some windows and dropped by the rest, which silently runs
    /// two lines of a summary together.
    Enter,
}

/// Turn text into the key events that reproduce it.
///
/// Every line ending — `\r\n`, a lone `\n`, a lone `\r` — becomes exactly one
/// Return, so text that travelled through a Windows editor does not arrive
/// double-spaced.
#[cfg(any(target_os = "windows", test))]
pub fn keys_for(text: &str) -> Vec<Key> {
    let mut keys = Vec::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                keys.push(Key::Enter);
            }
            '\n' => keys.push(Key::Enter),
            _ => {
                let mut buffer = [0u16; 2];
                keys.extend(
                    character
                        .encode_utf16(&mut buffer)
                        .iter()
                        .map(|unit| Key::Unit(*unit)),
                );
            }
        }
    }
    keys
}

/// Put the text into the focused window.
///
/// `clipboard_holds_text` says the clipboard delivery already ran and the
/// transcript is on it, so a paste needs no clipboard write of its own and
/// nothing the user had there is lost twice.
///
/// Errors are returned, never swallowed: the caller reports them as a delivery
/// warning next to clipboard and file.
pub fn insert_at_cursor<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    method: TypingMethod,
    delay_ms: u32,
    clipboard_holds_text: bool,
) -> Result<(), String> {
    let _ = (app, text, method, delay_ms, clipboard_holds_text);
    #[cfg(target_os = "linux")]
    {
        linux::insert(
            app,
            text,
            method,
            clamp_delay(delay_ms),
            clipboard_holds_text,
        )
    }
    #[cfg(target_os = "windows")]
    {
        windows::insert(app, text, method, clipboard_holds_text)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Typing at the cursor is not available on this platform yet".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_take_the_terminal_paste() {
        for class in [
            "Alacritty",
            "foot",
            "kitty",
            "com.mitchellh.ghostty",
            "org.wezfurlong.wezterm",
            "org.gnome.Terminal",
            "gnome-terminal-server",
            "xfce4-terminal",
            "lxterminal",
            "konsole",
            "xterm",
            "URxvt",
            "org.gnome.Console",
            "st",
            "  Alacritty  ",
        ] {
            assert_eq!(paste_for(Some(class)), Paste::Terminal, "{class}");
        }
    }

    #[test]
    fn ordinary_windows_take_the_ordinary_paste() {
        // Ctrl+Shift+V means "paste without formatting" in a browser and
        // "open the Markdown preview" in an editor. Guessing terminal here
        // would not paste, it would do something else entirely.
        for class in [
            "firefox",
            "Chromium",
            "code",
            "dev.zed.Zed",
            "Slack",
            "obsidian",
            "org.gnome.TextEditor",
            "libreoffice-writer",
            "thunderbird",
            "Utterform",
        ] {
            assert_eq!(paste_for(Some(class)), Paste::Plain, "{class}");
        }
    }

    #[test]
    fn an_unknown_window_takes_the_ordinary_paste() {
        assert_eq!(paste_for(None), Paste::Plain);
        assert_eq!(paste_for(Some("")), Paste::Plain);
    }

    #[test]
    fn every_line_ending_becomes_exactly_one_return() {
        assert_eq!(
            keys_for("a\r\nb"),
            [Key::Unit(97), Key::Enter, Key::Unit(98)]
        );
        assert_eq!(keys_for("a\nb"), [Key::Unit(97), Key::Enter, Key::Unit(98)]);
        assert_eq!(keys_for("a\rb"), [Key::Unit(97), Key::Enter, Key::Unit(98)]);
        assert_eq!(keys_for("\r\n\r\n"), [Key::Enter, Key::Enter]);
        assert_eq!(keys_for(""), []);
    }

    #[test]
    fn text_outside_ascii_keeps_every_character() {
        // The umlauts and the em dash a transcript is full of, and an emoji
        // that only fits in two code units.
        let text = "Grüße — 😀";
        let keys = keys_for(text);
        let units: Vec<u16> = keys
            .iter()
            .map(|key| match key {
                Key::Unit(unit) => *unit,
                Key::Enter => unreachable!(),
            })
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), text);
    }

    #[test]
    fn a_hand_edited_delay_cannot_stall_delivery() {
        assert_eq!(clamp_delay(0), 0);
        assert_eq!(clamp_delay(DEFAULT_DELAY_MS), DEFAULT_DELAY_MS);
        assert_eq!(clamp_delay(u32::MAX), MAX_DELAY_MS);
    }
}
