//! The text a Context action works on: what is selected, or else what is copied.
//!
//! Only the Context action reads it, and only at the moment its recording
//! starts, while the selection is still where the user left it. The text is
//! the user's own and follows the same rule as a transcript: it goes to the
//! text model and nowhere else. No event carries it; diagnostics learn only
//! where it came from and how long it was.
//!
//! Linux reads the primary selection directly, so nothing is pressed in the
//! focused window. Windows and macOS have no such selection; reaching it there
//! would mean pressing Ctrl+C for the user, which stops a terminal's running
//! program. They use the clipboard alone until a safer way is found.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Mutex,
};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{diagnostics, domain::AppSettings};

/// Enough for a long email thread, small enough to stay quick and cheap.
pub const MAX_CHARS: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Selection,
    Clipboard,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::Clipboard => "clipboard",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub source: Source,
    pub text: String,
    /// The text was longer than [`MAX_CHARS`] and was cut there.
    pub truncated: bool,
}

/// What a recording read when it started, held until it is finished. One at a
/// time: a new recording replaces whatever an abandoned one left behind.
#[derive(Default)]
pub struct CapturedContext(Mutex<Option<(u64, Captured)>>);

impl CapturedContext {
    pub fn hold(&self, recording: u64, captured: Captured) {
        if let Ok(mut held) = self.0.lock() {
            *held = Some((recording, captured));
        }
    }

    /// The context of this recording, if it read any. Someone else's is dropped.
    pub fn take(&self, recording: u64) -> Option<Captured> {
        let mut held = self.0.lock().ok()?;
        match held.take() {
            Some((owner, captured)) if owner == recording => Some(captured),
            _ => None,
        }
    }

    pub fn clear(&self) {
        if let Ok(mut held) = self.0.lock() {
            *held = None;
        }
    }
}

/// Whether the selection can be read here, and what to install if not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Support {
    pub selection: bool,
    /// Empty when the selection can be read.
    pub explanation: String,
}

pub fn support() -> Support {
    #[cfg(target_os = "linux")]
    {
        let (wayland, x11) = linux::session();
        match linux::selection_tool(wayland, x11, linux::installed) {
            Some(_) => Support {
                selection: true,
                explanation: String::new(),
            },
            None => Support {
                selection: false,
                explanation: linux::missing_tool_message(wayland, x11).into(),
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Support {
            selection: false,
            explanation: "Linux only for now. Copy the text first.".into(),
        }
    }
}

/// The last text Utterform itself delivered, as a fingerprint. Copied output is
/// still on the clipboard when the next dictation starts, and treating one's
/// own last answer as new material would surprise everyone.
static OWN_OUTPUT: Mutex<Option<u64>> = Mutex::new(None);

fn fingerprint(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.trim().hash(&mut hasher);
    hasher.finish()
}

pub fn remember_own_output(text: &str) {
    if let Ok(mut own) = OWN_OUTPUT.lock() {
        *own = Some(fingerprint(text));
    }
}

fn is_own_output(text: &str) -> bool {
    OWN_OUTPUT
        .lock()
        .is_ok_and(|own| *own == Some(fingerprint(text)))
}

/// Read the context for a recording that is starting, as the settings allow.
pub fn capture(app: &AppHandle, settings: &AppSettings, recording: u64) -> Option<Captured> {
    let selection = || {
        #[cfg(target_os = "linux")]
        {
            linux::primary_selection()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None::<String>
        }
    };
    let clipboard = || app.clipboard().read_text().ok();
    let captured = choose(
        settings.context_selection,
        settings.context_clipboard,
        selection,
        clipboard,
        is_own_output,
    );
    match &captured {
        Some(captured) => diagnostics::info!(
            "context.captured",
            recording = recording,
            source = captured.source.as_str(),
            characters = captured.text.chars().count(),
            truncated = captured.truncated
        ),
        None => diagnostics::info!(
            "context.none",
            recording = recording,
            selection = settings.context_selection,
            clipboard = settings.context_clipboard
        ),
    }
    captured
}

/// The selection first, the clipboard second; blank text is no context, and
/// neither is Utterform's own last answer still sitting on the clipboard.
fn choose(
    use_selection: bool,
    use_clipboard: bool,
    selection: impl FnOnce() -> Option<String>,
    clipboard: impl FnOnce() -> Option<String>,
    own_output: impl Fn(&str) -> bool,
) -> Option<Captured> {
    let usable = |text: Option<String>| text.filter(|text| !text.trim().is_empty());
    if use_selection && let Some(text) = usable(selection()) {
        return Some(cap(Source::Selection, &text));
    }
    if use_clipboard
        && let Some(text) = usable(clipboard())
        && !own_output(&text)
    {
        return Some(cap(Source::Clipboard, &text));
    }
    None
}

fn cap(source: Source, text: &str) -> Captured {
    let text = text.trim();
    let truncated = text.chars().count() > MAX_CHARS;
    let text = if truncated {
        text.chars().take(MAX_CHARS).collect()
    } else {
        text.to_string()
    };
    Captured {
        source,
        text,
        truncated,
    }
}

/// What the text model receives: the spoken instruction, and the material
/// marked off as data so nothing inside it is taken for an instruction.
pub fn model_input(instruction: &str, context: Option<&Captured>) -> String {
    let instruction = instruction.trim();
    match context {
        Some(context) => format!(
            "<instruction>\n{instruction}\n</instruction>\n\n<material source=\"{}\">\n{}\n</material>",
            context.source.as_str(),
            context.text
        ),
        None => format!("<instruction>\n{instruction}\n</instruction>\n\n<material/>"),
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    /// A selection owner that does not answer must not hold up the recording.
    const TIMEOUT: Duration = Duration::from_millis(800);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Tool {
        WlPaste,
        Xclip,
        Xsel,
    }

    impl Tool {
        fn program(self) -> &'static str {
            match self {
                Self::WlPaste => "wl-paste",
                Self::Xclip => "xclip",
                Self::Xsel => "xsel",
            }
        }

        pub fn args(self) -> &'static [&'static str] {
            match self {
                Self::WlPaste => &["--primary", "--no-newline", "--type", "text"],
                Self::Xclip => &["-o", "-selection", "primary"],
                Self::Xsel => &["--output", "--primary"],
            }
        }
    }

    pub fn session() -> (bool, bool) {
        (
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
            std::env::var_os("DISPLAY").is_some(),
        )
    }

    pub fn installed(program: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
        })
    }

    pub fn selection_tool(
        wayland: bool,
        x11: bool,
        installed: impl Fn(&str) -> bool,
    ) -> Option<Tool> {
        let candidates: &[Tool] = match (wayland, x11) {
            (true, _) => &[Tool::WlPaste],
            (false, true) => &[Tool::Xclip, Tool::Xsel],
            _ => &[],
        };
        candidates
            .iter()
            .copied()
            .find(|tool| installed(tool.program()))
    }

    pub fn missing_tool_message(wayland: bool, x11: bool) -> &'static str {
        if wayland {
            "Needs wl-clipboard: sudo pacman -S wl-clipboard"
        } else if x11 {
            "Needs xclip: sudo pacman -S xclip"
        } else {
            "No desktop session found."
        }
    }

    pub fn primary_selection() -> Option<String> {
        let (wayland, x11) = session();
        let tool = selection_tool(wayland, x11, installed)?;
        let mut child = Command::new(tool.program())
            .args(tool.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        // Read on a thread so a large selection cannot fill the pipe while we wait.
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if started.elapsed() < TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let bytes = reader.join().ok()?;
        // "No selection" is a non-zero exit, not an empty answer.
        status
            .filter(std::process::ExitStatus::success)
            .map(|_| String::from_utf8_lossy(&bytes).into_owned())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn wayland_reads_the_primary_selection_with_wl_paste() {
            assert_eq!(selection_tool(true, true, |_| true), Some(Tool::WlPaste));
            assert!(Tool::WlPaste.args().contains(&"--primary"));
            assert_eq!(selection_tool(true, true, |_| false), None);
        }

        #[test]
        fn x11_takes_xclip_and_then_xsel() {
            assert_eq!(selection_tool(false, true, |_| true), Some(Tool::Xclip));
            assert_eq!(
                selection_tool(false, true, |program| program == "xsel"),
                Some(Tool::Xsel)
            );
            assert_eq!(selection_tool(false, false, |_| true), None);
        }

        #[test]
        fn a_missing_tool_names_the_package() {
            assert!(missing_tool_message(true, false).contains("wl-clipboard"));
            assert!(missing_tool_message(false, true).contains("xclip"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(text: &str) -> impl FnOnce() -> Option<String> {
        let text = text.to_string();
        move || Some(text)
    }

    fn never() -> Option<String> {
        panic!("a source that is switched off must not be read")
    }

    #[test]
    fn the_selection_comes_first_and_the_clipboard_second() {
        let captured = choose(true, true, some("selected"), some("copied"), |_| false).unwrap();
        assert_eq!(captured.source, Source::Selection);
        assert_eq!(captured.text, "selected");

        let captured = choose(true, true, || None, some("copied"), |_| false).unwrap();
        assert_eq!(captured.source, Source::Clipboard);

        let captured = choose(true, true, some("  \n "), some("copied"), |_| false).unwrap();
        assert_eq!(
            captured.source,
            Source::Clipboard,
            "a blank selection is none"
        );
    }

    #[test]
    fn a_source_switched_off_is_never_read() {
        let captured = choose(false, true, never, some("copied"), |_| false).unwrap();
        assert_eq!(captured.source, Source::Clipboard);
        assert_eq!(choose(true, false, || None, never, |_| false), None);
        assert_eq!(choose(false, false, never, never, |_| false), None);
    }

    #[test]
    fn utterforms_own_last_answer_is_not_new_material() {
        assert_eq!(
            choose(
                true,
                true,
                || None,
                some("my last answer"),
                |text| text == "my last answer"
            ),
            None
        );
    }

    #[test]
    fn own_output_is_recognised_by_fingerprint() {
        remember_own_output("  Hallo zusammen  ");
        assert!(is_own_output("Hallo zusammen"));
        assert!(!is_own_output("Hallo"));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        let long = "ä".repeat(MAX_CHARS + 5);
        let captured = cap(Source::Selection, &long);
        assert!(captured.truncated);
        assert_eq!(captured.text.chars().count(), MAX_CHARS);
        assert!(!cap(Source::Selection, "kurz").truncated);
    }

    #[test]
    fn the_material_is_marked_off_from_the_instruction() {
        let context = cap(Source::Selection, "Hi Jonas, do you have time?");
        let input = model_input(" reply that Monday works ", Some(&context));
        assert!(input.starts_with("<instruction>\nreply that Monday works\n</instruction>"));
        assert!(
            input.contains(
                "<material source=\"selection\">\nHi Jonas, do you have time?\n</material>"
            )
        );
        assert!(model_input("write a haiku", None).ends_with("<material/>"));
    }

    #[test]
    fn a_held_context_belongs_to_its_recording() {
        let held = CapturedContext::default();
        held.hold(7, cap(Source::Clipboard, "text"));
        assert_eq!(held.take(8), None, "another recording gets nothing");
        assert_eq!(held.take(7), None, "and the stale one is gone");
        held.hold(9, cap(Source::Clipboard, "text"));
        assert_eq!(held.take(9).unwrap().text, "text");
    }
}
