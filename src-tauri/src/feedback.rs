//! Small synthesized mechanical cues, played outside the microphone capture interval.
//!
//! A cue must never hold up recording, but it must not fail quietly either:
//! with the window hidden it is the only confirmation the user gets. So the
//! reason a cue stayed silent is reported to the caller, and playback is built
//! for outputs that are slow rather than instant. A Bluetooth headset or an
//! HDMI display suspends when nothing is playing and needs a few hundred
//! milliseconds to carry sound at all, then queues a few hundred more — a cue
//! written into such a device and cut off a moment later is never heard.
//!
//! Every cue is opened, played, finished and closed on one thread of its own.
//! Until 0.4.4 the start cue was opened on the thread that answered the
//! interface — on Windows the main thread, the one WebView2 owns — and closed
//! on another, while the stop cue that did sound lived on a single fresh
//! thread. Whether or not that was the reason the start click stayed silent
//! on Windows, a cue that does not depend on who asked for it has one
//! difference fewer to explain, and it never blocks the interface either.
//! Every outcome, silent or not, goes to the log.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use crate::diagnostics;

use cpal::{
    Device, SampleFormat, StreamConfig, SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

#[derive(Clone, Copy, Debug)]
pub enum Cue {
    Start,
    Stop,
    Done,
}

impl Cue {
    fn name(self) -> &'static str {
        match self {
            Cue::Start => "start",
            Cue::Stop => "stop",
            Cue::Done => "done",
        }
    }
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

/// How often the cue thread looks up from waiting for the device to see
/// whether its Done cue has been silenced. A device that is still waking up
/// has not asked for a single buffer yet, so the audio callback alone cannot
/// be relied on to notice.
const SILENCE_POLL: Duration = Duration::from_millis(20);

/// How long the audible part of a cue lasts.
fn tone_seconds(cue: Cue) -> f32 {
    match cue {
        // Longer and louder than Stop: it is the one cue the user is waiting
        // for, often through a laptop speaker in a room with other noise, and
        // a tick that can be missed is no confirmation.
        Cue::Start => 0.07,
        Cue::Stop => 0.045,
        Cue::Done => 0.155,
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
    let length = tone_seconds(cue);
    if t >= length {
        return 0.0;
    }
    let (frequency, gain, decay) = match cue {
        Cue::Start => (1450.0, 0.26, 60.0),
        Cue::Stop => (820.0, 0.12, 115.0),
        Cue::Done => unreachable!(),
    };
    let attack = (t / 0.0015).min(1.0);
    let release = ((length - t) / 0.008).min(1.0);
    let tone = (std::f32::consts::TAU * frequency * t).sin()
        + 0.35 * (std::f32::consts::TAU * frequency * 2.7 * t).sin();
    tone * attack * release * (-t * decay).exp() * gain
}

/// What playing a cue took, for the log: which device, how long opening the
/// stream took, and how long until the device had taken the whole waveform —
/// or, for a Done cue the next recording silenced, until it was let go of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub device: String,
    pub opened_in: Duration,
    pub played_in: Duration,
    /// The cue was silenced before its end because the next recording began;
    /// `played_in` then says when. Only ever true of a productive Done cue.
    pub silenced: bool,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.silenced {
            return write!(
                f,
                "silenced on \"{}\" after {} ms, the next recording began — stream open after {} ms",
                self.device,
                self.played_in.as_millis(),
                self.opened_in.as_millis()
            );
        }
        write!(
            f,
            "played on \"{}\" — stream open after {} ms, device took the whole cue after {} ms",
            self.device,
            self.opened_in.as_millis(),
            self.played_in.as_millis()
        )
    }
}

/// The Done cue that confirms a delivered recording, and who is allowed to
/// stop it.
///
/// Only one such cue is ever wanted at a time, and none once the next
/// recording has begun: a chime still sounding then would confirm the wrong
/// recording, and its tail could reach a microphone that has just been armed.
/// So every productive Done carries the generation it was issued in, issuing
/// the next one makes the previous stale, and a new recording makes them all
/// stale before its own start cue is scheduled. A stale cue hands the device
/// silence from the next buffer on and its thread closes the stream. Making a
/// cue stale is one atomic increment; it never waits for the audio thread,
/// the device or the deadline. Dropping a [`Playback`] does none of this — a
/// dropped handle only gives up on the report.
///
/// Start and Stop never carry a generation, and neither does a Done cue played
/// from Settings through [`play`]: only the productive Done is ever cut short.
#[derive(Default)]
pub struct DoneCues {
    generation: Arc<AtomicU64>,
}

/// One productive Done cue's claim to be the current one. Cloned into the
/// audio callback and kept by the cue thread; neither can advance the
/// generation, so an old cue cannot touch a newer one.
#[derive(Clone, Debug)]
pub struct Ticket {
    generation: u64,
    current: Arc<AtomicU64>,
}

impl Ticket {
    /// Whether this cue is still the one wanted. One atomic load, fit for the
    /// audio callback.
    pub fn is_current(&self) -> bool {
        self.current.load(Ordering::Acquire) == self.generation
    }
}

impl DoneCues {
    /// Plays the Done cue for the recording that was just delivered, in place
    /// of any productive Done still sounding, and returns at once — the
    /// interface is ready again the moment the caller says so, and a slow
    /// speaker must not hold that up. The cue thread logs how it went.
    pub fn play(&self) -> Playback {
        start_with(Cue::Done, Some(self.issue()))
    }

    /// Silences the productive Done cue, if one is sounding. Idempotent,
    /// and never waits for anything.
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// A new generation: the previous ticket, if any, is stale from here on.
    fn issue(&self) -> Ticket {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        Ticket {
            generation,
            current: Arc::clone(&self.generation),
        }
    }
}

/// How a cue's waveform ended, as the audio callback reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// The device has taken the whole waveform, silent tail included.
    Complete,
    /// The cue was made stale; the buffer this was reported from is silent.
    Silenced,
}

/// A cue's progress through its waveform, buffer by buffer. This is the whole
/// of what the audio callback decides, kept apart from the device so it can
/// be driven without one.
struct Progress {
    waveform: Waveform,
    position: usize,
    /// `None` for a cue nothing can cut short.
    ticket: Option<Ticket>,
}

impl Progress {
    fn new(waveform: Waveform, ticket: Option<Ticket>) -> Self {
        Self {
            waveform,
            position: 0,
            ticket,
        }
    }

    /// Hands `frames` samples to `write` — one per frame; the caller copies it
    /// to every channel — and says whether the cue ended in this buffer. A
    /// stale cue writes silence and reports so on every buffer from then on.
    fn fill(&mut self, frames: usize, mut write: impl FnMut(f32)) -> Option<Ending> {
        if self
            .ticket
            .as_ref()
            .is_some_and(|ticket| !ticket.is_current())
        {
            for _ in 0..frames {
                write(0.0);
            }
            return Some(Ending::Silenced);
        }
        for _ in 0..frames {
            write(self.waveform.at(self.position));
            self.position += 1;
        }
        (self.position >= self.waveform.len()).then_some(Ending::Complete)
    }
}

/// A cue playing on its own thread.
///
/// A caller that knows it will want the cue can start it and let the device
/// wake-up overlap with its own work; `finish` then waits for the device to
/// take the whole waveform. Dropping the handle does not cut the cue off — the
/// thread plays it to the end regardless — it only gives up on the result.
pub struct Playback {
    outcome: mpsc::Receiver<Result<Report, String>>,
}

impl Playback {
    /// Waits until the device has taken the whole cue, silent tail included,
    /// and says how it went. Never longer than the cue's own deadline plus a
    /// margin for the thread to open the device.
    pub fn finish(self) -> Result<Report, String> {
        self.outcome
            .recv_timeout(DEADLINE * 2)
            .unwrap_or_else(|_| Err("the cue thread gave no answer".into()))
    }
}

/// Hands `cue` to the default output device on a thread of its own and
/// returns without waiting for it. Nothing cuts a cue started this way short.
pub fn start(cue: Cue) -> Playback {
    start_with(cue, None)
}

fn start_with(cue: Cue, ticket: Option<Ticket>) -> Playback {
    let (sender, outcome) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name(format!("utterform-cue-{}", cue.name()))
        .spawn(move || {
            let _ = sender.send(play_here(cue, ticket));
        });
    if let Err(error) = spawned {
        // The receiver simply sees a closed channel; `finish` reports it.
        diagnostics::warning!(
            "cue.thread_failed",
            cue = cue.name(),
            detail = diagnostics::io_detail(&error)
        );
    }
    Playback { outcome }
}

/// Plays `cue` to completion.
pub fn play(cue: Cue) -> Result<Report, String> {
    start(cue).finish()
}

/// Plays `cue` without holding up the caller, still letting the device take all
/// of it. For a cue that confirms something already finished, where the work
/// after it has no reason to wait for a slow speaker. The outcome is logged
/// by the cue thread like any other.
pub fn play_detached(cue: Cue) {
    let _ = start(cue);
}

/// Open, play, finish and close the cue on the calling thread, and log how it
/// went. Every cue passes through here, so the log tells the same story for
/// each of them.
fn play_here(cue: Cue, ticket: Option<Ticket>) -> Result<Report, String> {
    let outcome = play_to_the_end(cue, ticket);
    match &outcome {
        Ok(report) => diagnostics::info!(
            if report.silenced {
                "cue.silenced"
            } else {
                "cue.played"
            },
            cue = cue.name(),
            device = report.device,
            open_ms = report.opened_in.as_millis(),
            played_ms = report.played_in.as_millis(),
        ),
        // The reasons name the device and the audio backend's error, nothing else.
        Err(reason) => diagnostics::warning!("cue.not_played", cue = cue.name(), reason = reason),
    }
    outcome
}

fn play_to_the_end(cue: Cue, ticket: Option<Ticket>) -> Result<Report, String> {
    let began = Instant::now();
    let stale = || ticket.as_ref().is_some_and(|ticket| !ticket.is_current());
    // A cue that was silenced before its device could even be found — the
    // next recording began within milliseconds of the last one's delivery —
    // never opens the device at all.
    if stale() {
        return Err(
            "silenced before the output device was opened, the next recording began".into(),
        );
    }
    let device = output_device()?;
    let name = device.to_string();
    let supported = playable_config(&device)?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let waveform = Waveform {
        cue,
        rate: config.sample_rate,
    };
    let (finished, receiver) = mpsc::sync_channel(1);
    let mut progress = Progress::new(waveform, ticket.clone());
    macro_rules! output {
        ($sample:ty, $convert:expr) => {
            device.build_output_stream(
                config,
                move |data: &mut [$sample], _| {
                    let frames = data.len().div_ceil(channels);
                    let mut chunks = data.chunks_mut(channels);
                    let ending = progress.fill(frames, |value| {
                        if let Some(frame) = chunks.next() {
                            frame.fill(($convert)(value));
                        }
                    });
                    if let Some(ending) = ending {
                        let _ = finished.try_send(ending);
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
    let opened_in = began.elapsed();
    let ending = loop {
        match receiver.recv_timeout(SILENCE_POLL) {
            Ok(ending) => break ending,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("\"{name}\" stopped asking for the cue"));
            }
            // Also from here, not only from the callback: a device still
            // waking up has asked for no buffer yet and would otherwise keep a
            // silenced cue's stream open until it did.
            Err(RecvTimeoutError::Timeout) if stale() => break Ending::Silenced,
            Err(RecvTimeoutError::Timeout) if began.elapsed() >= DEADLINE => {
                return Err(format!(
                    "\"{name}\" did not take the cue within {} seconds",
                    DEADLINE.as_secs()
                ));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    };
    let played_in = began.elapsed();
    // Closed here, on the thread that opened it: once the device has taken the
    // silent tail as well, or at once for a cue that has been silenced —
    // whatever the device still holds of that one is discarded with it.
    drop(stream);
    Ok(Report {
        device: name,
        opened_in,
        played_in,
        silenced: ending == Ending::Silenced,
    })
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
    fn cues_are_short_and_distinct() {
        for cue in [Cue::Start, Cue::Stop] {
            assert_eq!(sample(cue, 0, 48000), 0.0);
            assert_eq!(sample(cue, 4800, 48000), 0.0);
            assert!((0..4800).all(|i| sample(cue, i, 48000).abs() < 0.4));
        }
        assert_ne!(
            sample(Cue::Start, 100, 48000),
            sample(Cue::Stop, 100, 48000)
        );
    }

    #[test]
    fn the_start_cue_is_the_one_that_has_to_be_noticed() {
        let peak = |cue| {
            (0..4800)
                .map(|i| sample(cue, i, 48000).abs())
                .fold(0.0_f32, f32::max)
        };
        let audible_for = |cue| {
            (0..4800)
                .rev()
                .find(|i| sample(cue, *i, 48000).abs() > 0.02)
                .unwrap_or(0) as f32
                / 48000.0
        };
        // Roughly 6 dB above Stop, and audible for longer, so a laptop speaker
        // in a room with other noise still gets it across.
        assert!(peak(Cue::Start) > peak(Cue::Stop) * 1.8);
        assert!(audible_for(Cue::Start) > audible_for(Cue::Stop) * 1.3);
        // Still a click, not an alarm.
        assert!(peak(Cue::Start) < 0.35);
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

    fn done() -> Waveform {
        Waveform {
            cue: Cue::Done,
            rate: 48000,
        }
    }

    /// Drive a cue to its end the way the audio callback would, in buffers
    /// of 480 frames, and count the samples handed over.
    fn play_out(progress: &mut Progress) -> (Ending, usize, bool) {
        let mut written = 0;
        let mut all_silent = true;
        loop {
            let ending = progress.fill(480, |value| {
                written += 1;
                all_silent &= value == 0.0;
            });
            if let Some(ending) = ending {
                return (ending, written, all_silent);
            }
        }
    }

    #[test]
    fn a_productive_done_cue_returns_before_it_could_have_sounded() {
        // What the interface waits for after delivery is this call and nothing
        // more: not the waveform, not a speaker waking up, not the deadline.
        let cues = DoneCues::default();
        let began = Instant::now();
        let playback = cues.play();
        let returned_after = began.elapsed();
        // Silenced straight away so the test machine's speaker stays quiet.
        cues.cancel();
        assert!(
            returned_after < Duration::from_millis(100),
            "{returned_after:?}"
        );
        assert!(returned_after < Duration::from_secs_f32(LEAD_IN_SECONDS));
        // The cue thread notices on its own, from the callback or before the
        // device is even opened, and lets the stream go.
        let outcome = playback.finish();
        assert!(
            matches!(&outcome, Ok(report) if report.silenced)
                || matches!(&outcome, Err(reason) if reason.contains("silenced") || reason.contains("output device")),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_new_done_cue_makes_only_the_previous_done_cue_stale() {
        let cues = DoneCues::default();
        let first = cues.issue();
        assert!(first.is_current());
        let second = cues.issue();
        assert!(!first.is_current());
        assert!(second.is_current());
        // Seen the same way from another thread, which is where the callback runs.
        let seen = {
            let (first, second) = (first.clone(), second.clone());
            thread::spawn(move || (first.is_current(), second.is_current()))
                .join()
                .unwrap()
        };
        assert_eq!(seen, (false, true));

        // The stale cue hands the device silence and ends in its first buffer.
        let mut stale = Progress::new(done(), Some(first));
        let (ending, written, all_silent) = play_out(&mut stale);
        assert_eq!(ending, Ending::Silenced);
        assert_eq!(written, 480);
        assert!(all_silent);
        // The current one sounds all the way through.
        let mut current = Progress::new(done(), Some(second));
        let (ending, written, all_silent) = play_out(&mut current);
        assert_eq!(ending, Ending::Complete);
        assert!(written >= done().len());
        assert!(!all_silent);
    }

    #[test]
    fn cancelling_is_idempotent_and_never_waits() {
        let cues = DoneCues::default();
        let ticket = cues.issue();
        let began = Instant::now();
        for _ in 0..10_000 {
            cues.cancel();
        }
        assert!(began.elapsed() < Duration::from_millis(500));
        assert!(!ticket.is_current());
        // Cancelling with nothing sounding changes nothing that matters: the
        // next Done issued is current, and the one before it is not.
        let next = cues.issue();
        assert!(next.is_current());
        assert!(!ticket.is_current());
    }

    #[test]
    fn start_stop_and_test_cues_are_never_cut_short() {
        // Whatever happens to the productive Done, a cue without a ticket —
        // Start, Stop, and the three cues Settings plays — sounds to its end.
        let cues = DoneCues::default();
        let _ = cues.issue();
        cues.cancel();
        for cue in [Cue::Start, Cue::Stop, Cue::Done] {
            let waveform = Waveform { cue, rate: 48000 };
            let mut progress = Progress::new(waveform, None);
            let (ending, written, all_silent) = play_out(&mut progress);
            assert_eq!(ending, Ending::Complete, "{cue:?}");
            assert!(written >= waveform.len(), "{cue:?}");
            assert!(!all_silent, "{cue:?}");
        }
    }

    #[test]
    fn an_old_cue_cannot_touch_a_newer_cue_or_the_generation() {
        let cues = DoneCues::default();
        let old = cues.issue();
        let new = cues.issue();
        let before = cues.generation.load(Ordering::Acquire);
        // Everything an old cue can do — play out silenced, be dropped, be
        // cloned into a callback — leaves the current generation alone.
        let mut stale = Progress::new(done(), Some(old.clone()));
        assert_eq!(play_out(&mut stale).0, Ending::Silenced);
        drop(stale);
        drop(old);
        assert_eq!(cues.generation.load(Ordering::Acquire), before);
        assert!(new.is_current());
        let mut current = Progress::new(done(), Some(new.clone()));
        assert_eq!(play_out(&mut current).0, Ending::Complete);
        assert!(new.is_current());
    }

    #[test]
    fn a_silenced_cue_is_reported_as_such() {
        let report = Report {
            device: "Speakers".into(),
            opened_in: Duration::from_millis(12),
            played_in: Duration::from_millis(210),
            silenced: true,
        };
        assert_eq!(
            report.to_string(),
            "silenced on \"Speakers\" after 210 ms, the next recording began — stream open after 12 ms"
        );
        let report = Report {
            silenced: false,
            ..report
        };
        assert!(report.to_string().starts_with("played on \"Speakers\""));
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
