//! Reaching the focused window on Linux through the session's own input tool.
//!
//! Wayland gives no application the right to synthesize input, so this shells
//! out: `wtype` speaks the virtual-keyboard protocol on wlroots compositors
//! such as Hyprland, `xdotool` does the same on X11. Text is always handed
//! over on stdin, so a transcript containing newlines, dashes or any other
//! character is never parsed as an option.

use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::{Paste, paste_for};
use crate::domain::TypingMethod;

/// A tool that can type into the focused window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Wtype,
    Xdotool,
}

impl Tool {
    fn program(self) -> &'static str {
        match self {
            Self::Wtype => "wtype",
            Self::Xdotool => "xdotool",
        }
    }
}

/// The compositor decides which tool can work at all, so the session kind is
/// checked before what happens to be installed.
pub fn tool(wayland: bool, x11: bool, installed: impl Fn(&str) -> bool) -> Option<Tool> {
    if wayland && installed(Tool::Wtype.program()) {
        return Some(Tool::Wtype);
    }
    if x11 && installed(Tool::Xdotool.program()) {
        return Some(Tool::Xdotool);
    }
    None
}

/// Send the text one key at a time.
///
/// The leading Shift tap is not decoration: some Wayland clients discard the
/// first character a newly created virtual keyboard sends, and a modifier that
/// produces no text absorbs that loss. The delay is what keeps the rest in
/// order, because input faster than a human types is exactly what an input
/// method reorders.
pub fn type_args(tool: Tool, delay_ms: u32) -> Vec<String> {
    match tool {
        Tool::Wtype => vec![
            "-P".into(),
            "Shift_L".into(),
            "-p".into(),
            "Shift_L".into(),
            "-d".into(),
            delay_ms.to_string(),
            "-".into(),
        ],
        Tool::Xdotool => vec![
            "type".into(),
            "--clearmodifiers".into(),
            "--delay".into(),
            delay_ms.to_string(),
            "--file".into(),
            "-".into(),
        ],
    }
}

/// Send the paste the focused window understands, as one chord.
pub fn paste_args(tool: Tool, paste: Paste) -> Vec<String> {
    match (tool, paste) {
        (Tool::Wtype, Paste::Plain) => ["-M", "ctrl", "-k", "v", "-m", "ctrl"]
            .iter()
            .map(|argument| (*argument).into())
            .collect(),
        (Tool::Wtype, Paste::Terminal) => [
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ]
        .iter()
        .map(|argument| (*argument).into())
        .collect(),
        (Tool::Xdotool, Paste::Plain) => {
            vec!["key".into(), "--clearmodifiers".into(), "ctrl+v".into()]
        }
        (Tool::Xdotool, Paste::Terminal) => vec![
            "key".into(),
            "--clearmodifiers".into(),
            "ctrl+shift+v".into(),
        ],
    }
}

/// What to tell the user when nothing can reach the focused window. Naming the
/// package is the difference between a dead end and a one-line fix.
pub fn missing_tool_message(wayland: bool, x11: bool) -> String {
    match (wayland, x11) {
        (true, _) => {
            "Typing at the cursor needs wtype. On Omarchy/Arch: sudo pacman -S wtype".into()
        }
        (false, true) => {
            "Typing at the cursor needs xdotool. On Arch: sudo pacman -S xdotool".into()
        }
        _ => "Typing at the cursor needs a graphical session".into(),
    }
}

/// Read the class of the window the text is about to go into, so a terminal
/// can be given the paste it actually listens for. Hyprland answers over its
/// own socket; X11 answers through the tool that is already required there.
fn active_window_class() -> Option<String> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        let output = Command::new("hyprctl")
            .args(["activewindow", "-j"])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let window: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        // `initialClass` survives applications that rewrite their class after
        // mapping; either name identifies the same emulator.
        for key in ["class", "initialClass"] {
            if let Some(class) = window.get(key).and_then(serde_json::Value::as_str)
                && !class.is_empty()
            {
                return Some(class.to_string());
            }
        }
        return None;
    }
    if std::env::var_os("DISPLAY").is_some() {
        let output = Command::new("xdotool")
            .args(["getactivewindow", "getwindowclassname"])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let class = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return (!class.is_empty()).then_some(class);
    }
    None
}

fn session() -> (bool, bool) {
    (
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    )
}

fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
    })
}

/// Run the tool, handing `stdin` over only when there is text for it.
fn run(tool: Tool, args: &[String], stdin_text: Option<&str>) -> Result<(), String> {
    let program = tool.program();
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(if stdin_text.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not run {program}: {error}"))?;
    if let Some(text) = stdin_text {
        child
            .stdin
            .take()
            .ok_or_else(|| format!("Could not send the text to {program}"))?
            .write_all(text.as_bytes())
            .map_err(|error| format!("Could not send the text to {program}: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("{program} did not finish: {error}"))?;
    if !status.success() {
        return Err(format!("{program} could not reach the focused window"));
    }
    Ok(())
}

/// Time for the compositor to hand clipboard ownership over before the target
/// window is asked to read it. Without it the paste can win the race and
/// insert whatever was on the clipboard before.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(60);

pub fn insert<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    method: TypingMethod,
    delay_ms: u32,
    clipboard_holds_text: bool,
) -> Result<(), String> {
    let (wayland, x11) = session();
    let tool = tool(wayland, x11, installed).ok_or_else(|| missing_tool_message(wayland, x11))?;

    match method {
        TypingMethod::Keystrokes => run(tool, &type_args(tool, delay_ms), Some(text)),
        TypingMethod::Paste => {
            if !clipboard_holds_text {
                app.clipboard()
                    .write_text(text)
                    .map_err(|error| format!("Could not put the text on the clipboard: {error}"))?;
            }
            thread::sleep(CLIPBOARD_SETTLE);
            let paste = paste_for(active_window_class().as_deref());
            run(tool, &paste_args(tool, paste), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_sessions_use_wtype() {
        assert_eq!(tool(true, false, |_| true), Some(Tool::Wtype));
        // XWayland sets DISPLAY too; the compositor is still Wayland.
        assert_eq!(tool(true, true, |_| true), Some(Tool::Wtype));
    }

    #[test]
    fn x11_sessions_use_xdotool() {
        assert_eq!(tool(false, true, |_| true), Some(Tool::Xdotool));
    }

    #[test]
    fn a_wayland_session_without_wtype_falls_back_to_x11() {
        assert_eq!(
            tool(true, true, |program| program == "xdotool"),
            Some(Tool::Xdotool)
        );
    }

    #[test]
    fn nothing_installed_means_no_tool() {
        assert_eq!(tool(true, true, |_| false), None);
        assert_eq!(tool(false, false, |_| true), None);
    }

    #[test]
    fn both_tools_read_the_text_from_stdin() {
        // Text is never an argument, so a transcript starting with "-" or
        // containing newlines cannot be read as options.
        for tool in [Tool::Wtype, Tool::Xdotool] {
            assert_eq!(type_args(tool, 15).last().unwrap(), "-", "{tool:?}");
        }
        assert!(type_args(Tool::Xdotool, 15).contains(&"--file".to_string()));
    }

    #[test]
    fn keystrokes_are_paced_and_preceded_by_a_harmless_modifier() {
        let wtype = type_args(Tool::Wtype, 24);
        // A Shift press and release absorbs the first character a fresh
        // virtual keyboard loses, without producing one of its own.
        assert_eq!(wtype[..4], ["-P", "Shift_L", "-p", "Shift_L"]);
        assert_eq!(wtype[4..6], ["-d", "24"]);
        assert!(
            type_args(Tool::Xdotool, 24)
                .windows(2)
                .any(|pair| pair == ["--delay".to_string(), "24".to_string()])
        );
    }

    #[test]
    fn a_terminal_is_pasted_into_with_shift_and_everything_else_without() {
        assert_eq!(
            paste_args(Tool::Wtype, Paste::Plain),
            ["-M", "ctrl", "-k", "v", "-m", "ctrl"]
        );
        assert!(paste_args(Tool::Wtype, Paste::Terminal).contains(&"shift".to_string()));
        assert_eq!(
            paste_args(Tool::Xdotool, Paste::Plain).last().unwrap(),
            "ctrl+v"
        );
        assert_eq!(
            paste_args(Tool::Xdotool, Paste::Terminal).last().unwrap(),
            "ctrl+shift+v"
        );
    }

    #[test]
    fn every_modifier_a_paste_presses_is_released_again() {
        for paste in [Paste::Plain, Paste::Terminal] {
            let args = paste_args(Tool::Wtype, paste);
            let pressed: Vec<_> = args.windows(2).filter(|pair| pair[0] == "-M").collect();
            let released: Vec<_> = args.windows(2).filter(|pair| pair[0] == "-m").collect();
            assert_eq!(pressed.len(), released.len(), "{paste:?}");
            for pair in &pressed {
                assert!(
                    released.iter().any(|other| other[1] == pair[1]),
                    "{} stays held down",
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn a_missing_tool_names_the_package() {
        assert!(missing_tool_message(true, true).contains("wtype"));
        assert!(missing_tool_message(false, true).contains("xdotool"));
        assert!(!missing_tool_message(false, false).is_empty());
    }
}
