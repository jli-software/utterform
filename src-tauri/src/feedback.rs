//! Small synthesized mechanical cues, played outside the microphone capture interval.
//!
//! A cue must never hold up recording, but it must not fail quietly either:
//! with the window hidden it is the only confirmation the user gets. So the
//! reason a cue stayed silent is reported to the caller, and playback is built
//! for outputs that are slow rather than instant. A Bluetooth headset or an
//! HDMI display suspends when nothing is playing and needs a few hundred
//! milliseconds to carry sound at all, then queues a few hundred more — a cue
//! written into such a device and cut off a moment later is never heard.

use std::{sync::mpsc, thread, time::Duration};

use cpal::{
    Device, SampleFormat, StreamConfig, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    Stop,
    Done,
}

/// Silence before the tone. A device resuming from idle can discard the first
/// samples it is handed; spending them on silence costs nothing audible and
/// keeps the tone itself out of that window.
const LEAD_IN_SECONDS: f32 = 0.06;

/// Silence after the tone. Closing a stream discards whatever the device is
/// still holding, and a Bluetooth or HDMI output holds 150-250 ms, so the tail
/// has to outlast the queue rather than the tone.
const DRAIN_SECONDS: f32 = 0.3;

/// The upper bound on resuming a suspended device, not the expected wait. An
/// output that is already running takes a whole cue in well under half a
/// second; one that was asleep needs a fraction of this. It exists so a device
/// that will never play cannot keep a thread forever.
const DEADLINE: Duration = Duration::from_secs(3);

/// How long the audible part of a cue lasts.
fn tone_seconds(cue: Cue) -> f32 {
    if matches!(cue, Cue::Done) {
        0.155
    } else {
        0.045
    }
}

/// The whole waveform handed to the device: silent lead-in, tone, silent tail.
#[derive(Clone, Copy)]
struct Waveform {
    cue: Cue,
    rate: u32,
}

impl Waveform {
    fn lead_in(&self) -> usize {
        (self.rate as f32 * LEAD_IN_SECONDS) as usize
    }

    fn at(&self, position: usize) -> f32 {
        match position.checked_sub(self.lead_in()) {
            Some(offset) => sample(self.cue, offset, self.rate),
            None => 0.0,
        }
    }

    /// The point at which the device has taken the tail as well, so closing the
    /// stream can no longer cut the tone off.
    fn len(&self) -> usize {
        let seconds = LEAD_IN_SECONDS + tone_seconds(self.cue) + DRAIN_SECONDS;
        (self.rate as f32 * seconds) as usize
    }
}

/// A cue the output device has been handed and is still playing.
///
/// Opening the stream is what resumes a suspended device, so a caller that
/// knows it will want the cue can start it first and let the wake-up overlap
/// with its own work. Dropping the handle instead of calling `finish` cuts the
/// cue off wherever it has got to.
pub struct Playback {
    // Playback lasts exactly as long as the stream is alive.
    _stream: cpal::Stream,
    finished: mpsc::Receiver<()>,
}

impl Playback {
    /// Waits until the device has taken the whole cue, silent tail included.
    pub fn finish(self) -> Result<(), String> {
        self.finished.recv_timeout(DEADLINE).map_err(|_| {
            format!(
                "the output device did not play the cue within {} seconds",
                DEADLINE.as_secs()
            )
        })
    }
}

fn sample(cue: Cue, position: usize, sample_rate: u32) -> f32 {
    let t = position as f32 / sample_rate as f32;
    if matches!(cue, Cue::Done) {
        // A quiet ascending pair, intentionally unlike the single mechanical Stop click.
        let (start, frequency) = if t < 0.09 {
            (0.0, 1100.0)
        } else {
            (0.09, 1650.0)
        };
        let local = t - start;
        if local >= 0.065 {
            return 0.0;
        }
        let envelope = (local / 0.004).min(1.0) * ((0.065 - local) / 0.025).min(1.0);
        return (std::f32::consts::TAU * frequency * local).sin() * envelope * 0.08;
    }
    if t >= 0.045 {
        return 0.0;
    }
    let frequency = match cue {
        Cue::Start => 1450.0,
        Cue::Stop => 820.0,
        Cue::Done => unreachable!(),
    };
    let attack = (t / 0.0015).min(1.0);
    let release = ((0.045 - t) / 0.008).min(1.0);
    let tone = (std::f32::consts::TAU * frequency * t).sin()
        + 0.35 * (std::f32::consts::TAU * frequency * 2.7 * t).sin();
    tone * attack * release * (-t * 115.0).exp() * 0.12
}

/// Hands `cue` to the default output device and returns without waiting for it.
pub fn start(cue: Cue) -> Result<Playback, String> {
    let device = output_device()?;
    let supported = playable_config(&device)?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let waveform = Waveform {
        cue,
        rate: config.sample_rate,
    };
    let length = waveform.len();
    let (finished, receiver) = mpsc::sync_channel(1);
    let mut position = 0;
    macro_rules! output {
        ($sample:ty, $convert:expr) => {
            device.build_output_stream(
                config,
                move |data: &mut [$sample], _| {
                    for frame in data.chunks_mut(channels) {
                        frame.fill(($convert)(waveform.at(position)));
                        position += 1;
                    }
                    if position >= length {
                        let _ = finished.try_send(());
                    }
                },
                |_| {},
                None,
            )
        };
    }
    let stream = match format {
        SampleFormat::F32 => output!(f32, |value: f32| value),
        SampleFormat::I16 => output!(i16, |value: f32| (value * i16::MAX as f32) as i16),
        SampleFormat::U16 => output!(u16, |value: f32| ((value + 1.0) * 32767.5) as u16),
        // `playable_config` only ever returns one of the three above.
        unsupported => {
            return Err(format!(
                "the sample format {unsupported:?} cannot be played"
            ));
        }
    }
    .map_err(|error| format!("the output stream could not be opened: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("the output device refused to start: {error}"))?;
    Ok(Playback {
        _stream: stream,
        finished: receiver,
    })
}

/// Plays `cue` to completion.
pub fn play(cue: Cue) -> Result<(), String> {
    start(cue)?.finish()
}

/// Plays `cue` without holding up the caller, still letting the device take all
/// of it. For a cue that confirms something already finished, where the work
/// after it has no reason to wait for a slow speaker.
pub fn play_detached(cue: Cue) {
    thread::spawn(move || {
        let _ = play(cue);
    });
}

/// The system's current output, asked for more than once: a default can be
/// missing for a moment while a headset connects or a dock wakes, and asking a
/// single time turns that moment into a cue nobody hears.
fn output_device() -> Result<Device, String> {
    let host = cpal::default_host();
    for _ in 0..5 {
        if let Some(device) = host.default_output_device() {
            return Ok(device);
        }
        thread::sleep(Duration::from_millis(40));
    }
    Err("no output device is available".into())
}

/// The device's own default, unless its sample format is one we cannot write —
/// then any offered format we can. Accepting only the default left a device
/// negotiating, say, `I32` with silence and no complaint.
fn playable_config(device: &Device) -> Result<SupportedStreamConfig, String> {
    let default = device
        .default_output_config()
        .map_err(|error| format!("the output configuration could not be read: {error}"))?;
    if playable(default.sample_format()) {
        return Ok(default);
    }
    let rate = default.sample_rate();
    device
        .supported_output_configs()
        .map_err(|error| format!("the output formats could not be listed: {error}"))?
        .filter(|range| playable(range.sample_format()))
        .map(|range| {
            range
                .try_with_sample_rate(rate)
                .unwrap_or_else(|| range.with_max_sample_rate())
        })
        .next()
        .ok_or_else(|| {
            format!(
                "the output device offers no sample format Utterform can play (it defaults to {:?})",
                default.sample_format()
            )
        })
}

fn playable(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_has_two_quiet_pulses_and_a_silent_tail() {
        for rate in [44100, 48000, 96000] {
            let at = |seconds: f32| (seconds * rate as f32) as usize;
            let peak = |start, end| {
                (at(start)..at(end))
                    .map(|i| sample(Cue::Done, i, rate).abs())
                    .fold(0.0_f32, f32::max)
            };
            assert!(peak(0.005, 0.06) > 0.07);
            assert_eq!(peak(0.07, 0.085), 0.0);
            assert!(peak(0.095, 0.15) > 0.07);
            assert_eq!(peak(0.16, 0.3), 0.0);
            assert!(peak(0.0, 0.3) <= 0.08);
        }
    }

    #[test]
    fn cues_are_short_quiet_and_distinct() {
        for cue in [Cue::Start, Cue::Stop] {
            assert_eq!(sample(cue, 0, 48000), 0.0);
            assert_eq!(sample(cue, 2400, 48000), 0.0);
            assert!((0..4800).all(|i| sample(cue, i, 48000).abs() < 0.17));
        }
        assert_ne!(
            sample(Cue::Start, 100, 48000),
            sample(Cue::Stop, 100, 48000)
        );
    }

    #[test]
    fn every_cue_opens_with_silence_a_waking_device_can_swallow() {
        for cue in [Cue::Start, Cue::Stop, Cue::Done] {
            for rate in [44100, 48000, 96000] {
                let waveform = Waveform { cue, rate };
                let lead_in = waveform.lead_in();
                assert!(lead_in > 0);
                assert!((0..lead_in).all(|i| waveform.at(i) == 0.0));
                // The tone itself begins the moment the lead-in ends, rather
                // than being pushed later by it.
                let tone = (0..waveform.len())
                    .map(|i| waveform.at(i).abs())
                    .position(|value| value > 0.0)
                    .expect("a cue has to make a sound");
                assert!(tone - lead_in < rate as usize / 500, "{tone} vs {lead_in}");
            }
        }
    }

    #[test]
    fn every_cue_ends_in_enough_silence_to_leave_a_slow_device() {
        // A stream is closed, not drained, so the tail is what stands between a
        // buffered output and a cue that is never heard.
        for cue in [Cue::Start, Cue::Stop, Cue::Done] {
            let rate = 48000;
            let waveform = Waveform { cue, rate };
            let last_sound = (0..waveform.len())
                .rev()
                .find(|i| waveform.at(*i) != 0.0)
                .expect("a cue has to make a sound");
            let tail = (waveform.len() - last_sound) as f32 / rate as f32;
            assert!(tail >= 0.25, "{tail}");
        }
    }

    #[test]
    fn only_formats_that_can_be_written_are_accepted() {
        assert!(playable(SampleFormat::F32));
        assert!(playable(SampleFormat::I16));
        assert!(playable(SampleFormat::U16));
        // Silently returning success for these is what made a cue disappear
        // with no way to find out why.
        assert!(!playable(SampleFormat::I32));
        assert!(!playable(SampleFormat::F64));
        assert!(!playable(SampleFormat::I8));
    }
}
