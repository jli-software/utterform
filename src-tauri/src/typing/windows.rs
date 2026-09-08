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
//!
//! One window no chord can reach: one running as administrator. Windows lets
//! a process inject input only into windows of its own integrity level or
//! below, and `SendInput` does not say when it has dropped the events for
//! that reason — it reports them all as delivered. Jonas's terminal is
//! elevated, so 0.4.5 pasted into it, was told it had succeeded, and nothing
//! arrived. The target's integrity level is therefore read beforehand and a
//! window that outranks Utterform is reported instead of typed into.

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenIntegrityLevel,
    },
    System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
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

pub(super) fn unicode(unit: u16, up: bool) -> INPUT {
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
/// `SendInput` reports how many events it accepted, which is fewer than asked
/// when another thread has the input blocked. It is not fewer for a window
/// that outranks Utterform — those events are dropped after being counted —
/// which is why `insert` checks the window first.
pub(super) fn send(events: &[INPUT]) -> Result<(), String> {
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
        return Err(format!(
            "Windows accepted only {sent} of {} key events",
            events.len()
        ));
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

/// The integrity level a process runs at, from its token's mandatory label:
/// medium for an ordinary program, high for one started as administrator.
/// `None` when it cannot be read, which is treated as ordinary.
fn integrity_level(process: HANDLE) -> Option<u32> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let mut needed = 0u32;
    unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    let mut buffer = vec![0u8; needed as usize];
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    unsafe { CloseHandle(token) };
    if read == 0 || buffer.len() < std::mem::size_of::<TOKEN_MANDATORY_LABEL>() {
        return None;
    }
    // The level is the SID's last sub-authority: 0x2000 medium, 0x3000 high.
    let label = unsafe {
        buffer
            .as_ptr()
            .cast::<TOKEN_MANDATORY_LABEL>()
            .read_unaligned()
    };
    let sid = label.Label.Sid;
    if sid.is_null() {
        return None;
    }
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    if count == 0 {
        return None;
    }
    Some(unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) })
}

/// The window that will receive the text.
struct FocusedWindow {
    /// The window class, if Windows would say.
    class: Option<String>,
    /// The program behind it without the `.exe`, if the process could be
    /// opened — one Utterform may not open is treated as an ordinary window.
    program: Option<String>,
    /// Whether the program runs at a higher integrity level than Utterform —
    /// as administrator, in practice — so that Windows will drop whatever
    /// Utterform types into it.
    outranks_us: bool,
}

fn focused_window() -> FocusedWindow {
    let mut found = FocusedWindow {
        class: None,
        program: None,
        outranks_us: false,
    };
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return found;
    }
    let mut class = [0u16; 256];
    if unsafe { GetClassNameW(window, class.as_mut_ptr(), class.len() as i32) } != 0 {
        found.class = Some(string_from(&class));
    }
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    if process_id == 0 {
        return found;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return found;
    }
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } != 0 {
        found.program = std::path::Path::new(&string_from(&path[..length as usize]))
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
    }
    if let (Some(theirs), Some(ours)) = (
        integrity_level(process),
        integrity_level(unsafe { GetCurrentProcess() }),
    ) {
        found.outranks_us = theirs > ours;
    }
    unsafe { CloseHandle(process) };
    found
}

/// Unlike optional batch-delivery diagnostics, live delivery fails closed when
/// process integrity cannot be inspected. SendInput cannot confirm app receipt.
pub(super) fn verify_live_integrity(
    window: windows_sys::Win32::Foundation::HWND,
) -> Result<(), String> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err("Windows cannot verify the target application's input permissions".into());
    }
    let theirs = integrity_level(process);
    unsafe { CloseHandle(process) };
    match (theirs, integrity_level(unsafe { GetCurrentProcess() })) {
        (Some(theirs), Some(ours)) if theirs <= ours => Ok(()),
        (Some(_), Some(_)) => {
            Err("Live typing cannot reach a program running as administrator".into())
        }
        _ => Err("Windows cannot verify the target application's input permissions".into()),
    }
}

/// Press the paste chord the focused window listens for, and say which.
fn press_paste(target: &FocusedWindow) -> Result<(), String> {
    let paste = paste_for_window(target.class.as_deref(), target.program.as_deref());
    diagnostics::log(format!(
        "pasting into window class {:?} of program {:?} with {}",
        target.class.as_deref().unwrap_or("unknown"),
        target.program.as_deref().unwrap_or("unknown"),
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
    let target = focused_window();
    if target.outranks_us {
        let program = target.program.as_deref().unwrap_or("The focused window");
        diagnostics::log(format!(
            "not typing into {program}: it runs at a higher integrity level than Utterform, and Windows would drop the keystrokes"
        ));
        return Err(format!(
            "{program} is running as administrator, and Windows lets no ordinary program type into it. Paste from the clipboard, or start Utterform as administrator too."
        ));
    }
    match method {
        TypingMethod::Keystrokes => type_text(text),
        TypingMethod::Paste => {
            if !clipboard_holds_text {
                app.clipboard()
                    .write_text(text)
                    .map_err(|error| format!("Could not put the text on the clipboard: {error}"))?;
            }
            press_paste(&target)
        }
    }
}
