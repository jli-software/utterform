//! Hold the microphone level: Settings → Recording → Level.
//!
//! The level a microphone records at is set by the operating system, and other
//! programs change it behind the user's back — Teams, Zoom and Chrome turn it
//! up and down on their own. With a level chosen, Utterform looks every couple
//! of seconds while it runs and sets it back. Without one it never touches the
//! level.
//!
//! Which microphone:
//! - Windows: the one chosen for recordings, else the default input
//!   (`IAudioEndpointVolume`, the slider in Sound settings).
//! - Linux: the default input of PipeWire (`wpctl`) or PulseAudio (`pactl`).
//!   The ALSA names Utterform records from do not map to their sources.
//! - macOS: the default input (`osascript`, the slider in Sound settings).

use std::{
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager};

use crate::{diagnostics, settings};

/// How often the level is checked while one is held.
const INTERVAL: Duration = Duration::from_secs(2);

/// A correction is logged at most this often, so a program that keeps turning
/// the level does not fill the log.
const LOG_INTERVAL: Duration = Duration::from_secs(60);

/// What to hold: a level in percent, on the chosen microphone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub level: Option<u8>,
    pub device: Option<String>,
}

/// The way to the watcher thread; `None` until [`init`] started it.
#[derive(Default)]
pub struct MicLevel(Mutex<Option<mpsc::Sender<Target>>>);

/// Start watching with the level in the settings file.
pub fn init(app: &AppHandle) {
    let (sender, receiver) = mpsc::channel();
    let started = thread::Builder::new()
        .name("mic-level".into())
        .spawn(move || watch(receiver));
    if let Err(error) = started {
        diagnostics::warning!("mic_level.thread_failed", error = error);
        return;
    }
    if let Ok(mut slot) = app.state::<MicLevel>().0.lock() {
        *slot = Some(sender);
    }
    if let Ok(value) = settings::load(app) {
        set(app, value.microphone_level, value.input_device);
    }
}

/// Hold `level` (or stop holding with `None`) on `device`. Applied at once.
pub fn set(app: &AppHandle, level: Option<u8>, device: Option<String>) {
    let target = Target {
        level: level.map(|level| level.min(100)),
        device,
    };
    if let Ok(slot) = app.state::<MicLevel>().0.lock()
        && let Some(sender) = slot.as_ref()
    {
        let _ = sender.send(target);
    }
}

fn watch(receiver: mpsc::Receiver<Target>) {
    platform::init_thread();
    let mut target = Target::default();
    let mut last_log: Option<Instant> = None;
    let mut failing = false;
    loop {
        let wait = if target.level.is_some() {
            INTERVAL
        } else {
            Duration::from_secs(3600)
        };
        match receiver.recv_timeout(wait) {
            Ok(next) => {
                if next.level != target.level {
                    diagnostics::info!(
                        "mic_level.target",
                        level = next
                            .level
                            .map_or_else(|| "off".to_string(), |level| level.to_string()),
                    );
                }
                target = next;
                failing = false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let Some(level) = target.level else { continue };
        match hold(level, target.device.as_deref()) {
            Ok(Some(previous)) => {
                failing = false;
                if last_log.is_none_or(|at| at.elapsed() >= LOG_INTERVAL) {
                    last_log = Some(Instant::now());
                    diagnostics::info!("mic_level.restored", from = previous, to = level);
                }
            }
            Ok(None) => failing = false,
            Err(error) => {
                if !failing {
                    failing = true;
                    diagnostics::warning!("mic_level.failed", error = error);
                }
            }
        }
    }
}

/// Set the level back if it moved; `Some(previous)` when it had to.
fn hold(level: u8, device: Option<&str>) -> Result<Option<u8>, String> {
    let current = platform::read(device)?;
    if !needs_correction(current, level) {
        return Ok(None);
    }
    platform::write(device, level)?;
    Ok(Some(current))
}

/// Rounding in the operating system moves a level by a point; that is no change.
fn needs_correction(current: u8, target: u8) -> bool {
    current.abs_diff(target) > 1
}

/// The level now, for Settings to start the slider at.
pub fn read(device: Option<&str>) -> Result<u8, String> {
    platform::read(device)
}

fn percent(fraction: f32) -> u8 {
    (fraction.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// `wpctl get-volume`: "Volume: 0.40" or "Volume: 0.40 [MUTED]".
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_wpctl(output: &str) -> Option<u8> {
    let value = output
        .trim()
        .strip_prefix("Volume:")?
        .split_whitespace()
        .next()?;
    value.parse::<f32>().ok().map(percent)
}

/// `pactl get-source-volume`: "Volume: front-left: 26214 /  40% / -23.88 dB, …".
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_pactl(output: &str) -> Option<u8> {
    output
        .split_whitespace()
        .find_map(|word| word.strip_suffix('%')?.parse::<u32>().ok())
        .map(|value| value.min(100) as u8)
}

/// `osascript -e "input volume of (get volume settings)"`: "75", or
/// "missing value" for a microphone without a level.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_osascript(output: &str) -> Option<u8> {
    output
        .trim()
        .parse::<u32>()
        .ok()
        .map(|value| value.min(100) as u8)
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::{
        Win32::{
            Media::Audio::{
                Endpoints::IAudioEndpointVolume, IMMDevice, IMMDeviceEnumerator,
                MMDeviceEnumerator, eCapture, eConsole,
            },
            System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
        },
        core::HSTRING,
    };

    pub fn init_thread() {
        // The watcher thread is Utterform's own; nothing else initialised COM on it.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    /// The chosen microphone (cpal names it `wasapi:<endpoint id>`), else
    /// the default input — the same fallback a recording takes.
    fn endpoint(device: Option<&str>) -> Result<IAudioEndpointVolume, String> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|error| format!("enumerator: {error}"))?;
            let chosen: Option<IMMDevice> = device
                .and_then(|id| id.strip_prefix("wasapi:"))
                .and_then(|id| enumerator.GetDevice(&HSTRING::from(id)).ok());
            let device = match chosen {
                Some(device) => device,
                None => enumerator
                    .GetDefaultAudioEndpoint(eCapture, eConsole)
                    .map_err(|error| format!("no microphone: {error}"))?,
            };
            device
                .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                .map_err(|error| format!("volume: {error}"))
        }
    }

    pub fn read(device: Option<&str>) -> Result<u8, String> {
        let volume = endpoint(device)?;
        let level = unsafe { volume.GetMasterVolumeLevelScalar() }
            .map_err(|error| format!("read: {error}"))?;
        Ok(super::percent(level))
    }

    pub fn write(device: Option<&str>, level: u8) -> Result<(), String> {
        let volume = endpoint(device)?;
        unsafe { volume.SetMasterVolumeLevelScalar(f32::from(level) / 100.0, std::ptr::null()) }
            .map_err(|error| format!("write: {error}"))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::process::Command;

    pub fn init_thread() {}

    fn run(program: &str, arguments: &[&str]) -> Option<String> {
        let output = Command::new(program).args(arguments).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn read(_device: Option<&str>) -> Result<u8, String> {
        if let Some(output) = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]) {
            return super::parse_wpctl(&output).ok_or_else(|| "wpctl: unreadable".into());
        }
        if let Some(output) = run("pactl", &["get-source-volume", "@DEFAULT_SOURCE@"]) {
            return super::parse_pactl(&output).ok_or_else(|| "pactl: unreadable".into());
        }
        Err("neither wpctl (PipeWire) nor pactl (PulseAudio) answered".into())
    }

    pub fn write(_device: Option<&str>, level: u8) -> Result<(), String> {
        let fraction = format!("{:.2}", f32::from(level) / 100.0);
        if run(
            "wpctl",
            &["set-volume", "@DEFAULT_AUDIO_SOURCE@", &fraction],
        )
        .is_some()
        {
            return Ok(());
        }
        let percent = format!("{level}%");
        if run(
            "pactl",
            &["set-source-volume", "@DEFAULT_SOURCE@", &percent],
        )
        .is_some()
        {
            return Ok(());
        }
        Err("neither wpctl (PipeWire) nor pactl (PulseAudio) set the level".into())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    pub fn init_thread() {}

    fn osascript(script: &str) -> Result<String, String> {
        let output = Command::new("osascript")
            .args(["-e", script])
            .output()
            .map_err(|error| format!("osascript: {error}"))?;
        if !output.status.success() {
            return Err("osascript failed".into());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn read(_device: Option<&str>) -> Result<u8, String> {
        let output = osascript("input volume of (get volume settings)")?;
        super::parse_osascript(&output).ok_or_else(|| "this microphone has no level".into())
    }

    pub fn write(_device: Option<&str>, level: u8) -> Result<(), String> {
        osascript(&format!("set volume input volume {level}")).map(|_| ())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod platform {
    pub fn init_thread() {}

    pub fn read(_device: Option<&str>) -> Result<u8, String> {
        Err("not available on this system".into())
    }

    pub fn write(_device: Option<&str>, _level: u8) -> Result<(), String> {
        Err("not available on this system".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_of_rounding_is_no_change() {
        assert!(!needs_correction(70, 70));
        assert!(!needs_correction(69, 70));
        assert!(!needs_correction(71, 70));
        assert!(needs_correction(68, 70));
        assert!(needs_correction(100, 70));
        assert!(needs_correction(0, 70));
    }

    #[test]
    fn wpctl_volume_is_read_muted_or_not() {
        assert_eq!(parse_wpctl("Volume: 0.40\n"), Some(40));
        assert_eq!(parse_wpctl("Volume: 0.73 [MUTED]\n"), Some(73));
        assert_eq!(parse_wpctl("Volume: 1.50\n"), Some(100));
        assert_eq!(parse_wpctl("nothing\n"), None);
    }

    #[test]
    fn pactl_volume_takes_the_first_channel() {
        let output = "Volume: front-left: 26214 /  40% / -23.88 dB,   front-right: 26214 /  40% / -23.88 dB\n        balance 0.00\n";
        assert_eq!(parse_pactl(output), Some(40));
        assert_eq!(
            parse_pactl("Volume: mono: 98304 / 150% / 10.57 dB"),
            Some(100)
        );
        assert_eq!(parse_pactl("No such entity"), None);
    }

    #[test]
    fn osascript_level_or_none() {
        assert_eq!(parse_osascript("75\n"), Some(75));
        assert_eq!(parse_osascript("missing value\n"), None);
    }

    #[test]
    fn fractions_round_to_whole_percent() {
        assert_eq!(percent(0.0), 0);
        assert_eq!(percent(0.704), 70);
        assert_eq!(percent(1.2), 100);
        assert_eq!(percent(-0.1), 0);
    }
}
