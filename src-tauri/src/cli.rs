//! What a command line asks Utterform to do.
//!
//! Hyprland and other Wayland compositors own the keyboard: an application
//! cannot grab a global shortcut for itself. The compositor can run a command,
//! though, and the single-instance plugin hands that command line to the
//! already running app. `utterform --toggle` from a compositor binding is
//! therefore a real global hotkey, with no extra permissions or daemon.

/// The argument the operating system's autostart entry carries. Internal: it
/// exists so a start nobody asked for can be told apart from one that was.
pub const AUTOSTART_FLAG: &str = "--autostart";

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
}
