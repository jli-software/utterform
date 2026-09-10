//! Reaching the focused window on macOS through CoreGraphics keyboard events.
//!
//! macOS needs no helper program: `CGEventPost` hands a keyboard event to the
//! window server, which delivers it to the frontmost application as if a
//! keyboard had sent it. Characters go in as the event's Unicode string
//! rather than as a key position, so the transcript does not depend on the
//! layout the user has active, and every line ending is one real Return.
//!
//! A paste is ⌘V everywhere. Unlike Linux and Windows, a Mac terminal takes
//! the same chord as every other window — Command is the Mac's own modifier
//! and never reaches the shell — so there is no terminal chord to guess.
//!
//! One thing no event can do without: Accessibility. The window server drops
//! keyboard events from a process the user has not listed under System
//! Settings → Privacy & Security → Accessibility, and reports nothing. The
//! grant is checked before the first event and, when missing, the system's
//! own request dialog is opened and the delivery reported as a warning; the
//! text is already on the clipboard by then.

use std::{thread, time::Duration};

use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use objc2_app_kit::NSWorkspace;
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::{Key, keys_for};
use crate::{diagnostics, domain::TypingMethod, macos};

/// Virtual key codes are positions on the keyboard, the same on every layout.
const KEY_RETURN: CGKeyCode = 36;
/// The key ⌘V is pressed on. A character event carries its own text and only
/// needs some key to be reported on; this is the one nothing reads a meaning
/// into.
const KEY_V: CGKeyCode = 9;

/// Time for the pasteboard write to land before the frontmost window is asked
/// to read it. Without it the paste can win the race and insert whatever was
/// on the clipboard before.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(60);

/// A CGEvent can carry a whole string, but the window server truncates long
/// ones and some applications only read the first character. One character
/// per event — two code units when it needs a surrogate pair — is what every
/// receiver agrees on.
fn post_key(
    source: &CGEventSource,
    keycode: CGKeyCode,
    text: &[u16],
    flags: CGEventFlags,
) -> Result<(), String> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.clone(), keycode, down)
            .map_err(|()| "macOS refused to create a keyboard event".to_string())?;
        if !text.is_empty() {
            event.set_string_from_utf16_unchecked(text);
        }
        event.set_flags(flags);
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}

fn event_source() -> Result<CGEventSource, String> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|()| "macOS refused to create a keyboard event source".to_string())
}

/// Type the text as its own characters, paced so a window that reads input
/// slower than the window server delivers it still gets every character.
pub fn type_text(text: &str, delay_ms: u32) -> Result<(), String> {
    let source = event_source()?;
    let delay = Duration::from_millis(u64::from(delay_ms));
    let mut pending: Vec<u16> = Vec::with_capacity(2);
    for key in keys_for(text) {
        match key {
            Key::Unit(unit) => {
                pending.push(unit);
                // A high surrogate waits for its partner; the pair goes in one event.
                if (0xD800..0xDC00).contains(&unit) {
                    continue;
                }
                post_key(&source, 0, &pending, CGEventFlags::CGEventFlagNull)?;
                pending.clear();
            }
            Key::Enter => {
                post_key(&source, KEY_RETURN, &[], CGEventFlags::CGEventFlagNull)?;
            }
        }
        if !delay.is_zero() {
            thread::sleep(delay);
        }
    }
    Ok(())
}

/// The application that will receive the text, for the log. macOS names it;
/// nothing is decided from it.
fn frontmost_application() -> String {
    let workspace = NSWorkspace::sharedWorkspace();
    workspace
        .frontmostApplication()
        .map(|application| {
            let name = application
                .localizedName()
                .map(|name| name.to_string())
                .unwrap_or_else(|| "unknown".into());
            let identifier = application
                .bundleIdentifier()
                .map(|identifier| identifier.to_string())
                .unwrap_or_else(|| "no bundle identifier".into());
            format!("{name} ({identifier})")
        })
        .unwrap_or_else(|| "no frontmost application".into())
}

fn press_paste() -> Result<(), String> {
    let source = event_source()?;
    post_key(&source, KEY_V, &[], CGEventFlags::CGEventFlagCommand)
}

pub fn insert<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    method: TypingMethod,
    delay_ms: u32,
    clipboard_holds_text: bool,
) -> Result<(), String> {
    if !macos::accessibility_trusted(true) {
        diagnostics::log(
            "not typing: Utterform is not trusted for Accessibility, and macOS would drop the keystrokes; the system request dialog was opened",
        );
        return Err(macos::ACCESSIBILITY_HELP.into());
    }
    let target = frontmost_application();
    match method {
        TypingMethod::Keystrokes => {
            diagnostics::log(format!(
                "typing {} characters into {target}",
                text.chars().count()
            ));
            type_text(text, delay_ms)
        }
        TypingMethod::Paste => {
            if !clipboard_holds_text {
                app.clipboard()
                    .write_text(text)
                    .map_err(|error| format!("Could not put the text on the clipboard: {error}"))?;
            }
            thread::sleep(CLIPBOARD_SETTLE);
            diagnostics::log(format!("pasting into {target} with Cmd+V"));
            press_paste()
        }
    }
}
