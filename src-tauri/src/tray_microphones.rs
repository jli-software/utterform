//! The tray's microphone menu: switch the microphone for the next recording
//! without opening Utterform — the laptop's own microphone while the Teams
//! headset lies on the desk, say.
//!
//! The choice is the same setting as Settings → Recording → Microphone. A
//! click writes it to the settings file here, so it holds even if the window
//! never loaded, and then tells the window so its copy of the settings agrees.
//! "System default" follows whatever the operating system calls its default
//! input; a chosen microphone that is unplugged falls back to it, as it always
//! has.
//!
//! Microphones come and go, so the list is read again whenever it might have
//! changed: at startup, when the window lists microphones, and on Windows and
//! macOS whenever the pointer moves onto the tray icon — that is, just before
//! its menu can be opened.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};

use crate::{audio, diagnostics, domain::AudioDeviceInfo, settings};

/// What the window hears when the tray changed the microphone; the payload is
/// the device id, or `null` for the system default.
pub const CHOSEN_EVENT: &str = "microphone-chosen";

const DEFAULT_ID: &str = "microphone:default";
const DEVICE_PREFIX: &str = "microphone:device:";
const MISSING_ID: &str = "microphone:missing";

/// Hovering the tray icon fires many events; one look per this long is enough.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// One line of the Microphone submenu, the same on every platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
}

/// The submenu for these microphones with `selected` chosen (`None`: system
/// default). A chosen microphone that is not connected stays visible as a
/// greyed line, so the menu never claims the system default was chosen.
pub fn entries(devices: &[AudioDeviceInfo], selected: Option<&str>) -> Vec<Entry> {
    let default_label = match devices.iter().find(|device| device.is_default) {
        Some(device) => format!("System default ({})", device.name),
        None => "System default".into(),
    };
    let mut entries = vec![Entry {
        id: DEFAULT_ID.into(),
        label: default_label,
        checked: selected.is_none(),
        enabled: true,
    }];
    if let Some(selected) = selected
        && !devices.iter().any(|device| device.id == selected)
    {
        entries.push(Entry {
            id: MISSING_ID.into(),
            label: "Chosen microphone (not connected, using system default)".into(),
            checked: true,
            enabled: false,
        });
    }
    entries.extend(devices.iter().map(|device| Entry {
        id: format!("{DEVICE_PREFIX}{}", device.id),
        label: device.name.clone(),
        checked: selected == Some(device.id.as_str()),
        enabled: true,
    }));
    entries
}

/// The choice a menu id stands for: `Some(None)` is the system default,
/// `Some(Some(id))` a microphone, `None` not a microphone line at all.
pub fn choice(menu_id: &str) -> Option<Option<String>> {
    if menu_id == DEFAULT_ID {
        return Some(None);
    }
    menu_id
        .strip_prefix(DEVICE_PREFIX)
        .map(|device| Some(device.to_owned()))
}

#[derive(Default)]
struct Known {
    devices: Vec<AudioDeviceInfo>,
    selected: Option<String>,
    refreshed: Option<Instant>,
}

/// What the tray menu currently shows.
#[derive(Default)]
pub struct MicrophoneMenu(Mutex<Known>);

/// The submenu as it should look now.
pub fn current(app: &AppHandle) -> Vec<Entry> {
    let state = app.state::<MicrophoneMenu>();
    let Ok(known) = state.0.lock() else {
        return entries(&[], None);
    };
    entries(&known.devices, known.selected.as_deref())
}

fn update(app: &AppHandle, apply: impl FnOnce(&mut Known)) {
    {
        let state = app.state::<MicrophoneMenu>();
        let Ok(mut known) = state.0.lock() else {
            diagnostics::warning!("tray.microphones_failed", step = "state");
            return;
        };
        apply(&mut known);
    }
    crate::tray::show_microphones(app);
}

/// Load the saved choice and read the microphones once, at startup.
pub fn init(app: &AppHandle) {
    match settings::load(app) {
        Ok(saved) => update(app, |known| known.selected = saved.input_device),
        Err(failure) => {
            diagnostics::warning!(
                "tray.microphones_failed",
                step = "settings",
                class = failure.class()
            )
        }
    }
    refresh(app, false);
}

/// The settings were saved: show their microphone as chosen.
pub fn set_selected(app: &AppHandle, selected: Option<String>) {
    update(app, |known| known.selected = selected);
}

/// The microphones were just read elsewhere; no need to read them again.
pub fn set_devices(app: &AppHandle, devices: Vec<AudioDeviceInfo>) {
    update(app, |known| {
        known.devices = devices;
        known.refreshed = Some(Instant::now());
    });
}

/// Read the microphones again, off the event loop. `throttled` skips the read
/// when one happened moments ago.
pub fn refresh(app: &AppHandle, throttled: bool) {
    {
        let state = app.state::<MicrophoneMenu>();
        let Ok(mut known) = state.0.lock() else {
            return;
        };
        if throttled
            && known
                .refreshed
                .is_some_and(|last| last.elapsed() < REFRESH_INTERVAL)
        {
            return;
        }
        known.refreshed = Some(Instant::now());
    }
    let app = app.clone();
    std::thread::spawn(move || match audio::list_input_devices() {
        Ok(devices) => set_devices(&app, devices),
        Err(failure) => {
            diagnostics::warning!(
                "tray.microphones_failed",
                step = "list",
                class = failure.class()
            )
        }
    });
}

/// A microphone line was clicked: save it as the microphone for recordings and
/// tell the window. A recording already running keeps its microphone.
pub fn choose(app: &AppHandle, selected: Option<String>) {
    let saved = settings::load(app).and_then(|mut value| {
        value.input_device = selected.clone();
        settings::save(app, &value).map(|()| value.microphone_level)
    });
    let level = match saved {
        Ok(level) => level,
        Err(failure) => {
            diagnostics::warning!("tray.microphone_choice_failed", class = failure.class());
            // Show the choice that is still in force.
            update(app, |_| {});
            return;
        }
    };
    // A held level follows the microphone to the one now chosen.
    crate::mic_level::set(app, level, selected.clone());
    diagnostics::info!(
        "tray.microphone_chosen",
        microphone = if selected.is_some() {
            "device"
        } else {
            "default"
        }
    );
    set_selected(app, selected.clone());
    if app.emit(CHOSEN_EVENT, selected).is_err() {
        diagnostics::warning!("tray.microphone_choice_failed", step = "emit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str, is_default: bool) -> AudioDeviceInfo {
        AudioDeviceInfo {
            id: id.into(),
            name: name.into(),
            is_default,
        }
    }

    fn devices() -> Vec<AudioDeviceInfo> {
        vec![
            device("headset", "Headset (Jabra)", true),
            device("laptop", "Microphone Array (Realtek)", false),
        ]
    }

    #[test]
    fn the_system_default_is_checked_and_names_the_device_it_means() {
        let entries = entries(&devices(), None);
        assert_eq!(entries[0].label, "System default (Headset (Jabra))");
        assert!(entries[0].checked);
        assert!(entries[1..].iter().all(|entry| !entry.checked));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn a_chosen_microphone_is_the_only_line_checked() {
        let entries = entries(&devices(), Some("laptop"));
        let checked: Vec<_> = entries.iter().filter(|entry| entry.checked).collect();
        assert_eq!(checked.len(), 1);
        assert_eq!(checked[0].label, "Microphone Array (Realtek)");
    }

    #[test]
    fn an_unplugged_choice_stays_visible_but_cannot_be_clicked() {
        let entries = entries(&devices(), Some("gone"));
        assert!(!entries[0].checked);
        assert_eq!(entries[1].id, MISSING_ID);
        assert!(entries[1].checked && !entries[1].enabled);
        assert_eq!(choice(&entries[1].id), None);
    }

    #[test]
    fn without_microphones_only_the_system_default_remains() {
        let entries = entries(&[], None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "System default");
    }

    #[test]
    fn menu_ids_round_trip_to_the_choice_they_stand_for() {
        for entry in entries(&devices(), None) {
            match entry.id.as_str() {
                DEFAULT_ID => assert_eq!(choice(&entry.id), Some(None)),
                _ => assert!(matches!(choice(&entry.id), Some(Some(_)))),
            }
        }
        assert_eq!(
            choice("microphone:device:{0.0.1.00000000}.{abc}"),
            Some(Some("{0.0.1.00000000}.{abc}".into()))
        );
        assert_eq!(choice("show"), None);
        assert_eq!(choice("quit"), None);
    }
}
