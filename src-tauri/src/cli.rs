//! What a command line asks Utterform to do.
//!
//! Hyprland and other Wayland compositors own the keyboard: an application
//! cannot grab a global shortcut for itself. The compositor can run a command,
//! though, and the single-instance plugin hands that command line to the
//! already running app. `utterform --toggle` from a compositor binding is
//! therefore a real global hotkey, with no extra permissions or daemon.

use std::path::{Path, PathBuf};

/// The argument the operating system's autostart entry carries. Internal: it
/// exists so a start nobody asked for can be told apart from one that was.
pub const AUTOSTART_FLAG: &str = "--autostart";

/// `utterform --file <path>` transcribes an audio file.
pub const FILE_FLAG: &str = "--file";

/// A request that arrives from a second process, or from the command line the
/// app itself was started with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Intent {
    /// Bring the window to the front. What a plain second launch means.
    #[default]
    Show,
    /// Start recording, or finish the running one. The dictation hotkey.
    Toggle,
    /// Start recording; do nothing if one is already running.
    Start,
    /// Finish and process the running recording.
    Stop,
    /// Discard the running recording without transcribing it.
    Cancel,
    /// Signing in started Utterform, not the user. Nothing was asked for, so
    /// nothing happens: the app waits in the tray until it is wanted.
    ///
    /// Without an intent of its own such a start would fall through to the
    /// default — `Show` — and open the window in front of whatever the user
    /// actually signed in to do.
    Autostart,
}

impl Intent {
    /// The command-line spelling, for diagnostic events.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Toggle => "toggle",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Cancel => "cancel",
            Self::Autostart => "autostart",
        }
    }

    /// Only an explicit request for the window should raise it. A dictation
    /// hotkey must leave the user in the application they are typing into.
    pub fn raises_window(self) -> bool {
        self == Intent::Show
    }

    /// Whether the interface has anything to do with this. The recording state
    /// machine lives in Svelte, so everything that touches a recording is sent
    /// there rather than reimplemented in Rust — but an autostart launch is
    /// not something it should ignore, it is something it never hears about.
    pub fn reaches_the_interface(self) -> bool {
        self != Intent::Autostart
    }
}

/// Read the intent from a command line, ignoring the executable path and any
/// flags Utterform does not define. An unknown flag must not silently turn
/// into a recording action.
pub fn intent_from<I, S>(args: I) -> Intent
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .find_map(|arg| match arg.as_ref().trim() {
            "--toggle" => Some(Intent::Toggle),
            "--start" => Some(Intent::Start),
            "--stop" => Some(Intent::Stop),
            "--cancel" => Some(Intent::Cancel),
            "--show" => Some(Intent::Show),
            AUTOSTART_FLAG => Some(Intent::Autostart),
            _ => None,
        })
        .unwrap_or_default()
}

/// The audio files a command line asks Utterform to transcribe, resolved
/// against the directory it was given in.
///
/// `--file <path>` (or `--file=<path>`) is the documented form. A bare
/// argument counts too, but only when it names an existing file: that is how
/// a file manager's "Open with" and Windows' file association hand a file
/// over, and it keeps `utterform toggle` meaning what it always did.
pub fn files_from<I, S>(args: I, cwd: &Path) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let resolve = |value: &str| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let mut files = Vec::new();
    let mut args = args.into_iter().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        if arg == FILE_FLAG {
            if let Some(value) = args.next() {
                files.push(resolve(value.as_ref()));
            }
        } else if let Some(value) = arg.strip_prefix("--file=") {
            files.push(resolve(value));
        } else if let Some(url) = arg.strip_prefix("file://") {
            // Some file managers pass a URL even for `%f`.
            if let Some(path) = file_url_path(url) {
                files.push(path);
            }
        } else if !arg.starts_with('-') && !arg.is_empty() {
            let path = resolve(arg);
            if path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

/// The path of a `file://` URL's remainder, percent-decoded. A host other
/// than none or `localhost` is not a local file.
pub fn file_url_path(rest: &str) -> Option<PathBuf> {
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = path.get(index + 1..index + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            decoded.push(byte);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    let text = String::from_utf8(decoded).ok()?;
    // `file:///C:/Users/...` on Windows.
    #[cfg(windows)]
    let text = text
        .strip_prefix('/')
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .map(str::to_string)
        .unwrap_or(text);
    Some(PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_launch_shows_the_window() {
        assert_eq!(intent_from(["utterform"]), Intent::Show);
        assert_eq!(intent_from(Vec::<String>::new()), Intent::Show);
    }

    #[test]
    fn every_documented_flag_is_understood() {
        for (flag, expected) in [
            ("--toggle", Intent::Toggle),
            ("--start", Intent::Start),
            ("--stop", Intent::Stop),
            ("--cancel", Intent::Cancel),
            ("--show", Intent::Show),
        ] {
            assert_eq!(intent_from(["utterform", flag]), expected, "{flag}");
        }
    }

    #[test]
    fn an_unknown_flag_never_starts_a_recording() {
        assert_eq!(intent_from(["utterform", "--wat"]), Intent::Show);
        assert_eq!(intent_from(["utterform", "-t"]), Intent::Show);
        assert_eq!(intent_from(["utterform", "toggle"]), Intent::Show);
    }

    #[test]
    fn the_executable_path_is_not_a_flag() {
        assert_eq!(intent_from(["--toggle"]), Intent::Show);
    }

    #[test]
    fn only_showing_the_window_raises_it() {
        assert!(Intent::Show.raises_window());
        for intent in [
            Intent::Toggle,
            Intent::Start,
            Intent::Stop,
            Intent::Cancel,
            Intent::Autostart,
        ] {
            assert!(!intent.raises_window(), "{intent:?}");
        }
    }

    #[test]
    fn the_autostart_entry_is_recognised_rather_than_read_as_a_plain_launch() {
        // The whole point of the flag: without it this line would mean Show,
        // and signing in would put the window in front of the user.
        assert_eq!(
            intent_from(["utterform", AUTOSTART_FLAG]),
            Intent::Autostart
        );
        assert_eq!(
            intent_from(["/usr/bin/utterform", "--autostart"]),
            Intent::Autostart
        );
    }

    #[test]
    fn an_autostart_launch_asks_for_nothing_at_all() {
        assert!(!Intent::Autostart.raises_window());
        // Never sent to the recording state machine: a login must not start,
        // stop or cancel anything, on this instance or on a running one.
        assert!(!Intent::Autostart.reaches_the_interface());
        for intent in [
            Intent::Show,
            Intent::Toggle,
            Intent::Start,
            Intent::Stop,
            Intent::Cancel,
        ] {
            assert!(intent.reaches_the_interface(), "{intent:?}");
        }
    }

    #[test]
    fn a_file_flag_names_a_file_relative_to_where_it_was_typed() {
        let cwd = Path::new("/home/anna/Downloads");
        assert_eq!(
            files_from(["utterform", "--file", "voice.opus"], cwd),
            vec![PathBuf::from("/home/anna/Downloads/voice.opus")]
        );
        assert_eq!(
            files_from(["utterform", "--file=/tmp/a b.m4a"], cwd),
            vec![PathBuf::from("/tmp/a b.m4a")]
        );
        assert!(files_from(["utterform", "--file"], cwd).is_empty());
    }

    #[test]
    fn a_file_is_not_a_recording_action() {
        assert_eq!(
            intent_from(["utterform", "--file", "voice.opus"]),
            Intent::Show
        );
    }

    #[test]
    fn a_bare_argument_is_a_file_only_when_one_exists() {
        let directory = tempfile::tempdir().unwrap();
        let voice = directory.path().join("Sprachnachricht.opus");
        std::fs::write(&voice, b"OggS").unwrap();
        assert_eq!(
            files_from(["utterform", "Sprachnachricht.opus"], directory.path()),
            vec![voice.clone()]
        );
        assert!(files_from(["utterform", "toggle"], directory.path()).is_empty());
        assert!(files_from(["utterform", "--toggle"], directory.path()).is_empty());
        // The executable itself is never taken for a file.
        assert!(files_from([voice.to_str().unwrap()], directory.path()).is_empty());
    }

    #[test]
    fn a_file_url_becomes_a_path() {
        assert_eq!(
            files_from(
                ["utterform", "file:///home/anna/Voice%20note.ogg"],
                Path::new("/")
            ),
            vec![PathBuf::from("/home/anna/Voice note.ogg")]
        );
        assert_eq!(
            file_url_path("localhost/tmp/x.opus"),
            Some(PathBuf::from("/tmp/x.opus"))
        );
        assert_eq!(file_url_path("server/share/x.opus"), None);
    }
}
