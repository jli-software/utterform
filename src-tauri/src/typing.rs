//! Typing the finished text into whatever window has focus.
//!
//! Wayland gives no application the right to synthesize input, so this shells
//! out to the session's own tool: `wtype` speaks the virtual-keyboard protocol
//! on wlroots compositors such as Hyprland, `xdotool` does the same on X11.
//! Both read the text from stdin, so a transcript containing newlines, dashes
//! or any other character is never parsed as an option.

use std::{
    io::Write,
    process::{Command, Stdio},
};

/// A tool that can type into the focused window, and how to invoke it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typist {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// `wtype -` and `xdotool type --file -` both take the text on stdin.
const WTYPE: Typist = Typist {
    program: "wtype",
    args: &["-"],
};
const XDOTOOL: Typist = Typist {
    program: "xdotool",
    args: &["type", "--clearmodifiers", "--file", "-"],
};

/// Choose a tool for this session. The compositor decides which one can work,
/// so the session kind is checked before what happens to be installed.
pub fn typist(wayland: bool, x11: bool, installed: impl Fn(&str) -> bool) -> Option<Typist> {
    if wayland && installed(WTYPE.program) {
        return Some(WTYPE);
    }
    if x11 && installed(XDOTOOL.program) {
        return Some(XDOTOOL);
    }
    None
}

/// What to tell the user when nothing can type for them. Naming the package is
/// the difference between a dead end and a one-line fix.
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

#[cfg(target_os = "linux")]
fn session() -> (bool, bool) {
    (
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    )
}

#[cfg(not(target_os = "linux"))]
fn session() -> (bool, bool) {
    (false, false)
}

fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
    })
}

/// Type the text into the focused window. Errors are returned, never swallowed:
/// the caller reports them as a delivery warning next to clipboard and file.
pub fn insert_at_cursor(text: &str) -> Result<(), String> {
    let (wayland, x11) = session();
    let tool = typist(wayland, x11, installed).ok_or_else(|| missing_tool_message(wayland, x11))?;

    let mut child = Command::new(tool.program)
        .args(tool.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not run {}: {error}", tool.program))?;
    child
        .stdin
        .take()
        .ok_or_else(|| format!("Could not send the text to {}", tool.program))?
        .write_all(text.as_bytes())
        .map_err(|error| format!("Could not send the text to {}: {error}", tool.program))?;
    let status = child
        .wait()
        .map_err(|error| format!("{} did not finish: {error}", tool.program))?;
    if !status.success() {
        return Err(format!("{} could not type the text", tool.program));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_sessions_type_with_wtype() {
        assert_eq!(typist(true, false, |_| true), Some(WTYPE));
        // XWayland sets DISPLAY too; the compositor is still Wayland.
        assert_eq!(typist(true, true, |_| true), Some(WTYPE));
    }

    #[test]
    fn x11_sessions_type_with_xdotool() {
        assert_eq!(typist(false, true, |_| true), Some(XDOTOOL));
    }

    #[test]
    fn a_wayland_session_without_wtype_falls_back_to_x11() {
        assert_eq!(
            typist(true, true, |program| program == "xdotool"),
            Some(XDOTOOL)
        );
    }

    #[test]
    fn nothing_installed_means_no_typist() {
        assert_eq!(typist(true, true, |_| false), None);
        assert_eq!(typist(false, false, |_| true), None);
    }

    #[test]
    fn both_tools_read_the_text_from_stdin() {
        // Text is never an argument, so a transcript starting with "-" or
        // containing newlines cannot be read as options.
        assert!(WTYPE.args.contains(&"-"));
        assert!(XDOTOOL.args.ends_with(&["--file", "-"]));
    }

    #[test]
    fn a_missing_tool_names_the_package() {
        assert!(missing_tool_message(true, true).contains("wtype"));
        assert!(missing_tool_message(false, true).contains("xdotool"));
        assert!(!missing_tool_message(false, false).is_empty());
    }
}
