//! A system-wide dictation key, where the operating system grants one.
//!
//! Windows, macOS and X11 let an application reserve a key combination for
//! itself, so Utterform can offer there what a Hyprland binding already gives
//! on Omarchy: press once to start, again to finish, without the window ever
//! taking focus. Wayland deliberately refuses — the compositor owns the
//! keyboard — so a Wayland session keeps taking its orders from
//! `utterform --toggle` and never registers anything.
//!
//! The key reaches the same `remote-intent` event as the command line, so the
//! interface has one recording path regardless of how the request arrived.
//!
//! Registering asks the event loop to do the work and waits for its answer, so
//! it must never run on the main thread: during `setup` the loop has not
//! started yet, and afterwards the main thread cannot answer itself. Both
//! callers here are off it — a thread at startup, a blocking task from the
//! command.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcut, Shortcut, ShortcutState};

use crate::cli::Intent;

/// Dictate. Win+D belongs to the Windows shell and cannot be reserved, and a
/// bare function key would collide with whatever the user already has, so the
/// default takes the modifier pair almost nothing else claims.
pub const DEFAULT: &str = "Ctrl+Alt+D";

/// Why the configured key is not in effect, kept until Settings is opened.
///
/// Startup registration happens on a thread with no one to report to; without
/// this the user would find a shortcut in Settings that quietly does nothing.
#[derive(Default)]
pub struct Failure(Mutex<Option<String>>);

impl Failure {
    fn record(&self, reason: Option<String>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = reason;
        }
    }

    fn reason(&self) -> Option<String> {
        self.0.lock().ok()?.clone()
    }
}

/// Whether this session can hand a global shortcut to an application at all.
/// A Wayland compositor cannot, and asking would only produce an error the
/// user can do nothing about.
pub fn supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// What Settings needs to show for the dictation key, without the interface
/// having to know which platforms grant one.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Support {
    pub supported: bool,
    pub default: &'static str,
    /// Empty when the session grants shortcuts; otherwise what to do instead.
    pub explanation: &'static str,
    /// Set when the stored key could not be registered at startup.
    pub failure: Option<String>,
}

pub fn support<R: Runtime>(app: &AppHandle<R>) -> Support {
    support_with(app.try_state::<Failure>().and_then(|state| state.reason()))
}

fn support_with(failure: Option<String>) -> Support {
    let supported = supported();
    Support {
        supported,
        default: DEFAULT,
        explanation: if supported {
            ""
        } else {
            "Wayland gives no application a global shortcut — the compositor owns the keyboard. Bind `utterform --toggle` there instead."
        },
        failure,
    }
}

/// Read a shortcut written the way the Settings field asks for it. Rejecting
/// it here keeps a typo out of the plugin, where it would unregister the
/// working key before discovering the new one is unusable.
pub fn parse(spec: &str) -> Result<Shortcut, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("Enter a shortcut such as Ctrl+Alt+D".into());
    }
    if !spec.contains('+') {
        // A shortcut without a modifier is reserved system-wide, so the key
        // would stop producing its own character in every other application.
        return Err(format!(
            "{spec} needs a modifier, for example Ctrl+Alt+{spec}"
        ));
    }
    spec.parse::<Shortcut>()
        .map_err(|error| format!("{spec} is not a usable shortcut: {error}"))
}

/// Register `spec` as the dictation key, replacing whatever was registered
/// before. `None` leaves the session without one.
///
/// The old key is released only after the new one parses, so a rejected entry
/// costs nothing. A session that cannot take shortcuts at all reports success:
/// the setting is stored, it simply has nothing to act on here.
///
/// Must not be called from the main thread — see the module note.
pub fn apply<R: Runtime>(app: &AppHandle<R>, spec: Option<&str>) -> Result<(), String> {
    let outcome = register(app, spec);
    if let Some(state) = app.try_state::<Failure>() {
        state.record(outcome.as_ref().err().cloned());
    }
    outcome
}

fn register<R: Runtime>(app: &AppHandle<R>, spec: Option<&str>) -> Result<(), String> {
    let shortcut = spec.map(parse).transpose()?;
    let Some(manager) = app.try_state::<GlobalShortcut<R>>() else {
        return Ok(());
    };
    manager
        .unregister_all()
        .map_err(|error| format!("Could not release the previous shortcut: {error}"))?;
    let Some(shortcut) = shortcut else {
        return Ok(());
    };
    manager.register(shortcut).map_err(|error| {
        format!(
            "{} is not available — another application may already hold it ({error})",
            spec.unwrap_or_default()
        )
    })
}

/// Install the plugin the shortcuts live in, on the sessions that have them.
///
/// Registered from `setup` rather than at build time so a host that refuses to
/// create a hotkey manager costs the dictation key and not the application, and
/// deliberately with no shortcut of its own: one the OS rejects would fail the
/// whole installation and leave Settings with nothing to correct it through.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if !supported() {
        return Ok(());
    }
    let plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            // A hotkey reports both edges; acting on the release too would
            // start and immediately finish the same recording.
            if event.state == ShortcutState::Pressed {
                // Deliberately not revealing the window: the point of the key
                // is to dictate into whatever the user is already typing in.
                let _ = app.emit("remote-intent", Intent::Toggle);
            }
        })
        .build();
    app.plugin(plugin)
        .map_err(|error| format!("Global shortcuts are unavailable: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_shortcut_parses() {
        assert!(parse(DEFAULT).is_ok());
    }

    #[test]
    fn shortcuts_are_read_the_way_the_settings_field_writes_them() {
        for spec in [
            "Ctrl+Alt+D",
            "ctrl+alt+d",
            " Ctrl+Shift+Space ",
            "Super+D",
            "Alt+F9",
        ] {
            assert!(parse(spec).is_ok(), "{spec}");
        }
    }

    #[test]
    fn a_typo_is_reported_rather_than_registered() {
        for spec in ["", "   ", "Ctrl+", "Ctrl+Alt+Nope", "Ctrl+D+Alt"] {
            assert!(parse(spec).is_err(), "{spec}");
        }
    }

    #[test]
    fn a_bare_key_is_refused_before_it_can_swallow_that_key_everywhere() {
        // "D" and "F9" both parse as hotkeys; reserving one system-wide would
        // stop it typing anywhere else.
        for spec in ["D", "F9", "Space"] {
            let error = parse(spec).unwrap_err();
            assert!(error.contains("modifier"), "{spec}: {error}");
        }
    }

    #[test]
    fn a_session_without_shortcuts_is_told_what_to_use_instead() {
        let support = support_with(None);
        assert_eq!(support.default, DEFAULT);
        assert_eq!(support.explanation.is_empty(), support.supported);
        if !support.supported {
            assert!(support.explanation.contains("--toggle"));
        }
    }

    #[test]
    fn a_key_that_could_not_be_registered_is_remembered_for_settings() {
        // Startup registration runs on a thread with no one to report to, so
        // the reason has to survive until Settings asks for it.
        let failure = Failure::default();
        assert_eq!(support_with(failure.reason()).failure, None);

        failure.record(parse("Ctrl+Alt+Nope").err());
        assert!(
            support_with(failure.reason())
                .failure
                .is_some_and(|reason| reason.contains("Ctrl+Alt+Nope"))
        );

        // A key that works clears it again, rather than leaving a stale
        // complaint standing next to a shortcut that is now in effect.
        failure.record(None);
        assert_eq!(support_with(failure.reason()).failure, None);
    }
}
