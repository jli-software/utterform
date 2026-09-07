//! Reaching the focused window on Windows through `SendInput`.
//!
//! Windows needs no helper program and no per-key pacing: one `SendInput` call
//! appends its whole batch to the input queue atomically, so nothing else can
//! interleave and nothing arrives out of order. Characters go in as Unicode
//! rather than as keyboard scan codes, so the transcript does not depend on
//! the layout the user happens to have active.

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_RETURN, VK_V,
};

use super::{Key, keys_for};
use crate::domain::TypingMethod;

/// One batch stays well inside what the queue accepts while remaining large
/// enough that an ordinary transcript is a single, uninterruptible injection.
const BATCH: usize = 512;

fn unicode(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn virtual_key(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Hand a batch to the input queue, refusing to claim success it did not get.
///
/// `SendInput` reports how many events it accepted. It stops early when a more
/// privileged window owns the foreground: Windows blocks input from a normal
/// process to an elevated one, and saying so beats an empty text field.
fn send(events: &[INPUT]) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }
    let sent = unsafe {
        SendInput(
            events.len() as u32,
            events.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != events.len() {
        return Err(
            "Windows blocked the keystrokes — the focused window may be running as administrator"
                .into(),
        );
    }
    Ok(())
}

/// Type the text as its own characters.
pub fn type_text(text: &str) -> Result<(), String> {
    let mut batch = Vec::with_capacity(BATCH);
    for key in keys_for(text) {
        match key {
            Key::Unit(unit) => {
                batch.push(unicode(unit, false));
                batch.push(unicode(unit, true));
            }
            Key::Enter => {
                batch.push(virtual_key(VK_RETURN, false));
                batch.push(virtual_key(VK_RETURN, true));
            }
        }
        if batch.len() >= BATCH {
            send(&batch)?;
            batch.clear();
        }
    }
    send(&batch)
}

/// Press Ctrl+V. Windows Terminal, the console host and every graphical
/// toolkit paste on it, so there is no terminal special case to make here.
pub fn press_paste() -> Result<(), String> {
    send(&[
        virtual_key(VK_CONTROL, false),
        virtual_key(VK_V, false),
        virtual_key(VK_V, true),
        virtual_key(VK_CONTROL, true),
    ])
}

pub fn insert<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    method: TypingMethod,
    clipboard_holds_text: bool,
) -> Result<(), String> {
    match method {
        TypingMethod::Keystrokes => type_text(text),
        TypingMethod::Paste => {
            if !clipboard_holds_text {
                app.clipboard()
                    .write_text(text)
                    .map_err(|error| format!("Could not put the text on the clipboard: {error}"))?;
            }
            press_paste()
        }
    }
}
