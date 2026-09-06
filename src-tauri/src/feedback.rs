//! Small synthesized mechanical cues, played outside the microphone capture interval.
//! Audio feedback is best-effort: an unavailable speaker must never block recording.
use std::{sync::mpsc, time::Duration};

use cpal::{
    SampleFormat, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    Stop,
    Done,
}

pub fn play(cue: Cue) {
    let _ = try_play(cue);
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

fn try_play(cue: Cue) -> Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let rate = config.sample_rate;
    let (done, received) = mpsc::sync_channel(1);
    let mut position = 0;
    let playback_seconds = if matches!(cue, Cue::Done) { 0.22 } else { 0.1 };
    let playback_samples = (rate as f32 * playback_seconds) as usize;
    macro_rules! output {
        ($sample:ty, $convert:expr) => {
            device.build_output_stream(
                config,
                move |data: &mut [$sample], _| {
                    for frame in data.chunks_mut(channels) {
                        let value = sample(cue, position, rate);
                        frame.fill(($convert)(value));
                        position += 1;
                    }
                    // Include a silent tail so the cue can leave the device buffer.
                    if position >= playback_samples {
                        let _ = done.try_send(());
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
        _ => return Ok(()),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    let _ = received.recv_timeout(Duration::from_millis(500));
    Ok(())
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
}
