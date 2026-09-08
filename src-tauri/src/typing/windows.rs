//! Reaching the focused window on Windows through `SendInput`.
//!
//! Windows needs no helper program and no per-key pacing: one `SendInput` call
//! appends its whole batch to the input queue atomically, so nothing else can
//! interleave and nothing arrives out of order. Characters go in as Unicode
//! rather than as keyboard scan codes, so the transcript does not depend on
//! the layout the user happens to have active.
//!
//! A paste is a chord, and a chord is where 0.4.4 went wrong. It pressed
//! `VK_CONTROL` and `VK_V` with no scan code, which every classic window
//! accepts and Windows Terminal does not: it asks the key state for the left
//! and right Control keys individually and reads the scan code off the
//! message, and a synthesized key that carries neither looks like no key at
//! all. So every key here is sent the way the keyboard would send it — the
//! left-hand modifier, with its scan code — and a terminal gets Shift+Insert,
//! the paste every Windows console understands, where Ctrl+V is the shell's
//! and Ctrl+Shift+V is unknown to half of them.

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::{
        Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_INSERT,
            VK_LCONTROL, VK_LSHIFT, VK_RETURN, VK_V,
        },
        WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId},
    },
};

use super::{Key, Paste, keys_for, paste_for_window};
use crate::{diagnostics, domain::TypingMethod};

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

/// A key as the keyboard would report it: virtual key and scan code both, and
/// the extended flag on the keys that carry it, so a window that reads either
/// sees a real key.
fn virtual_key(key: VIRTUAL_KEY, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(u32::from(key), MAPVK_VK_TO_VSC) } as u16;
    let extended = if key == VK_INSERT {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: extended | if up { KEYEVENTF_KEYUP } else { 0 },
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

/// A UTF-16 buffer Windows filled, up to its first NUL.
fn string_from(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// The window that will receive the paste: its class, and the name of the
/// program behind it without the `.exe`. Either can be missing — a window
/// belonging to a process Utterform may not open, say — and the paste is then
/// chosen from what there is.
fn focused_window() -> (Option<String>, Option<String>) {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return (None, None);
    }
    let mut class = [0u16; 256];
    let class = match unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) } {
        0 => None,
        _ => Some(string_from(&class)),
    };
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    if process_id == 0 {
        return (class, None);
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return (class, None);
    }
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    let program =
        match unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } {
            0 => None,
            _ => std::path::Path::new(&string_from(&path[..length as usize]))
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned()),
        };
    unsafe { CloseHandle(process) };
    (class, program)
}

/// Press the paste chord the focused window listens for, and say which.
pub fn press_paste() -> Result<(), String> {
    let (class, program) = focused_window();
    let paste = paste_for_window(class.as_deref(), program.as_deref());
    diagnostics::log(format!(
        "pasting into window class {:?} of program {:?} with {}",
        class.as_deref().unwrap_or("unknown"),
        program.as_deref().unwrap_or("unknown"),
        match paste {
            Paste::Plain => "Ctrl+V",
            Paste::Terminal => "Shift+Insert",
        }
    ));
    let (modifier, key) = match paste {
        Paste::Plain => (VK_LCONTROL, VK_V),
        Paste::Terminal => (VK_LSHIFT, VK_INSERT),
    };
    send(&[
        virtual_key(modifier, false),
        virtual_key(key, false),
        virtual_key(key, true),
        virtual_key(modifier, true),
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
