use std::{
    collections::VecDeque,
    io::BufWriter,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    Device, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::{
    diagnostics,
    domain::AudioDeviceInfo,
    feedback::{self, Cue},
};
use serde::Serialize;

const CHANNEL_CAPACITY: usize = 64;
const WHISPER_SAMPLE_RATE: u32 = 16_000;
const LIVE_SAMPLE_RATE: u32 = 24_000;
const LIVE_CHUNK_SAMPLES: usize = 480; // 20 ms, independent of microphone callbacks.
const RECORDING_DIRECTORY: &str = "utterform";
const STALE_RECORDING_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_RECORDING_DURATION: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct AudioCaptureState {
    inner: Mutex<CaptureInner>,
}

#[derive(Clone, Default)]
struct CaptureSignals {
    level: Arc<AtomicU32>,
    /// Whether captured audio is kept. False until the start cue has been
    /// played, so the cue cannot end up in its own recording.
    armed: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    overrun: Arc<AtomicBool>,
    /// Buffer under- or overruns the backend reported while the stream kept
    /// running: a short gap in the audio, counted rather than fatal.
    glitches: Arc<AtomicU32>,
    stream_error: Arc<Mutex<Option<String>>>,
    live_enabled: bool,
    live_failure: Arc<Mutex<Option<String>>>,
}

#[derive(Default)]
struct CaptureInner {
    active: Option<ActiveRecording>,
    completed: Option<Result<RecordingArtifact, String>>,
    // Retained through stop/limit so the transport can still observe a final flush error.
    live_signals: Option<CaptureSignals>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub recording: bool,
    pub paused: bool,
    pub limit_reached: bool,
    pub elapsed_seconds: u64,
    pub level: f32,
}

pub enum LimitCheck {
    Waiting,
    Stopped,
    Gone,
}

struct ActiveRecording {
    started_at: Instant,
    sound_enabled: bool,
    stream: Stream,
    sender: SyncSender<Vec<f32>>,
    writer: thread::JoinHandle<Result<RecordingArtifact, String>>,
    signals: CaptureSignals,
    clock: RecordingClock,
}

// Session identity stays fixed; only active capture time counts toward the limit.
struct RecordingClock {
    started_at: Instant,
    paused_at: Option<Instant>,
    paused_duration: Duration,
}

impl RecordingClock {
    fn new(now: Instant) -> Self {
        Self {
            started_at: now,
            paused_at: None,
            paused_duration: Duration::ZERO,
        }
    }

    fn elapsed(&self, now: Instant) -> Duration {
        self.paused_at
            .unwrap_or(now)
            .saturating_duration_since(self.started_at)
            .saturating_sub(self.paused_duration)
    }

    fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(start)) => {
                self.paused_duration += now.saturating_duration_since(start);
                self.paused_at = None;
            }
            _ => {}
        }
    }
}

pub struct RecordingArtifact {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_count: u64,
}

struct TemporaryRecording {
    path: Option<PathBuf>,
}

impl TemporaryRecording {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn into_path(mut self) -> Result<PathBuf, String> {
        self.path
            .take()
            .ok_or_else(|| "Temporary recording path is unavailable".to_string())
    }
}

impl Drop for TemporaryRecording {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl RecordingArtifact {
    pub fn duration_ms(&self) -> u64 {
        let frames = self.sample_count / u64::from(self.channels.max(1));
        frames.saturating_mul(1_000) / u64::from(self.sample_rate.max(1))
    }

    pub fn whisper_pcm(&self) -> Result<Vec<f32>, String> {
        let mut reader = hound::WavReader::open(&self.path)
            .map_err(|error| format!("Could not read the recording: {error}"))?;
        let channels = usize::from(reader.spec().channels.max(1));
        let samples: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
        let samples =
            samples.map_err(|error| format!("Could not decode the recording: {error}"))?;

        let mono = samples
            .chunks(channels)
            .map(|frame| {
                let sum = frame.iter().map(|sample| f32::from(*sample)).sum::<f32>();
                sum / (frame.len() as f32 * f32::from(i16::MAX))
            })
            .collect::<Vec<_>>();

        Ok(resample_linear(
            &mono,
            self.sample_rate,
            WHISPER_SAMPLE_RATE,
        ))
    }
}

impl Drop for RecordingArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host
        .input_devices()
        .map_err(|error| format!("Could not enumerate microphones: {error}"))?;

    devices
        .enumerate()
        .map(|(index, device)| {
            let name = device.to_string();
            let id = device
                .id()
                .map(|id| id.to_string())
                .unwrap_or_else(|_| format!("fallback-{index}-{name}"));
            Ok(AudioDeviceInfo {
                is_default: default_id.as_ref() == Some(&id),
                id,
                name,
            })
        })
        .collect()
}

pub fn cleanup_stale_recordings() -> Result<(), String> {
    let directory = recording_directory();
    if !directory.exists() {
        return Ok(());
    }

    let entries = std::fs::read_dir(&directory)
        .map_err(|error| format!("Could not inspect temporary recordings: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_recording = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("utterform-") && name.ends_with(".wav"));
        let is_stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
            .is_ok_and(|age| age >= STALE_RECORDING_AGE);
        if is_recording && is_stale {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

/// A recording whose microphone is open but whose audio is not yet kept.
///
/// The start cue is the only confirmation a hidden window gives, so it has to
/// be heard before the user starts speaking — and an output device resuming
/// from idle can take a few hundred milliseconds to make any sound at all.
/// Capture therefore begins immediately and discards, and [`Self::arm`] admits
/// audio once the cue has actually been played.
pub struct StartedRecording {
    pub session: Instant,
    signals: CaptureSignals,
    /// `None` when cues are switched off.
    cue: Option<feedback::Playback>,
}

impl StartedRecording {
    /// Waits for the start cue, then keeps what the microphone hears. Returns
    /// why the cue was inaudible when it could not be played, so the caller can
    /// confirm the recording some other way.
    ///
    /// Must not run on the thread that answers the interface: the wait is the
    /// device wake-up, and nothing else should queue behind it.
    pub fn arm(self, state: &AudioCaptureState) -> Result<(), String> {
        let played = match self.cue {
            None => Ok(()),
            Some(playback) => playback.finish().map(|_| ()),
        };
        diagnostics::log(format!(
            "recording armed {} ms after the microphone opened{}",
            self.session.elapsed().as_millis(),
            match &played {
                Ok(()) => "",
                Err(_) => " — without a start cue",
            }
        ));
        // The recording limit counts kept audio, so its clock starts here and
        // not when the device was opened. A session that ended while the cue
        // was playing is left alone.
        if let Ok(mut guard) = state.inner.lock() {
            match guard.active.as_mut() {
                Some(active) if active.started_at == self.session => {
                    active.clock = RecordingClock::new(Instant::now());
                }
                _ => {}
            }
        }
        self.signals.armed.store(true, Ordering::Release);
        played
    }
}

pub fn start_recording(
    state: &AudioCaptureState,
    requested_device: Option<&str>,
    sound_enabled: bool,
) -> Result<StartedRecording, String> {
    start_recording_inner(state, requested_device, sound_enabled, None)
}

/// Opens the same recording path, additionally tapping bounded 24 kHz mono
/// PCM16 little-endian packets on its writer thread. The caller must drain the
/// receiver concurrently, including while stopping the recording.
pub fn start_recording_live(
    state: &AudioCaptureState,
    requested_device: Option<&str>,
    sound_enabled: bool,
    live_sender: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Result<StartedRecording, String> {
    start_recording_inner(state, requested_device, sound_enabled, Some(live_sender))
}

/// A latched failure is retained until the next successful recording start.
/// No network or UI work runs in the microphone callback.
pub fn live_error(state: &AudioCaptureState) -> Option<String> {
    let guard = match state.inner.lock() {
        Ok(guard) => guard,
        Err(_) => return Some("Audio state is unavailable".into()),
    };
    guard.live_signals.as_ref().and_then(capture_live_error)
}

fn capture_live_error(signals: &CaptureSignals) -> Option<String> {
    if !signals.live_enabled {
        return None;
    }
    if let Ok(slot) = signals.live_failure.lock()
        && slot.is_some()
    {
        return slot.clone();
    }
    if let Ok(slot) = signals.stream_error.lock()
        && slot.is_some()
    {
        return slot.clone();
    }
    if signals.overrun.load(Ordering::Relaxed) {
        return Some(
            "Live recording stopped: microphone audio could not be stored fast enough".into(),
        );
    }
    if signals.glitches.load(Ordering::Relaxed) > 0 {
        return Some("Live recording stopped: the microphone reported a gap in the audio".into());
    }
    None
}

fn latch_live_error(signals: &CaptureSignals, error: String) {
    if signals.live_enabled
        && let Ok(mut slot) = signals.live_failure.lock()
        && slot.is_none()
    {
        *slot = Some(error);
    }
}

fn start_recording_inner(
    state: &AudioCaptureState,
    requested_device: Option<&str>,
    sound_enabled: bool,
    live_sender: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
) -> Result<StartedRecording, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?;
    if guard.active.is_some() || guard.completed.is_some() {
        return Err("A recording is already active".into());
    }
    // A microphone macOS has refused would open fine and deliver silence.
    #[cfg(target_os = "macos")]
    crate::macos::microphone_access()?;

    let opening = Instant::now();
    let host = cpal::default_host();
    let device = select_device(&host, requested_device)?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("Could not read the microphone configuration: {error}"))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let device_name = device.to_string();

    let (sender, receiver) = sync_channel::<Vec<f32>>(CHANNEL_CAPACITY);
    let signals = CaptureSignals {
        live_enabled: live_sender.is_some(),
        ..CaptureSignals::default()
    };
    let started = signals.clone();
    let writer_signals = signals.clone();
    let writer_config = config;
    let writer = thread::spawn(move || {
        let result: Result<RecordingArtifact, String> = (|| {
            let mut live = live_sender.map(|sender| {
                LiveTap::new(sender, writer_config.sample_rate, writer_config.channels)
            });
            let directory = recording_directory();
            std::fs::create_dir_all(&directory)
                .map_err(|error| format!("Could not create the recording directory: {error}"))?;
            let temporary = tempfile::Builder::new()
                .prefix("utterform-")
                .suffix(".wav")
                .tempfile_in(directory)
                .map_err(|error| format!("Could not create a temporary recording: {error}"))?;
            let (file, path) = temporary
                .keep()
                .map_err(|error| format!("Could not retain the temporary recording: {error}"))?;
            let temporary = TemporaryRecording::new(path);
            let spec = hound::WavSpec {
                channels: writer_config.channels,
                sample_rate: writer_config.sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut wav = hound::WavWriter::new(BufWriter::new(file), spec)
                .map_err(|error| format!("Could not initialize the recording: {error}"))?;
            let mut sample_count = 0_u64;
            while let Ok(chunk) = receiver.recv() {
                if let Some(tap) = live.as_mut() {
                    let result = match capture_live_error(&writer_signals) {
                        Some(error) => Err(error),
                        None => tap.push(&chunk),
                    };
                    if let Err(error) = result {
                        latch_live_error(&writer_signals, error);
                        live = None;
                    }
                }
                for sample in chunk {
                    let normalized = sample.clamp(-1.0, 1.0);
                    let pcm = (normalized * f32::from(i16::MAX)).round() as i16;
                    wav.write_sample(pcm)
                        .map_err(|error| format!("Could not write the recording: {error}"))?;
                    sample_count += 1;
                }
            }
            if let Some(mut tap) = live
                && let Err(error) =
                    capture_live_error(&writer_signals).map_or_else(|| tap.finish(), Err)
            {
                latch_live_error(&writer_signals, error);
            }
            wav.finalize()
                .map_err(|error| format!("Could not finalize the recording: {error}"))?;
            Ok(RecordingArtifact {
                path: temporary.into_path()?,
                sample_rate: writer_config.sample_rate,
                channels: writer_config.channels,
                sample_count,
            })
        })();
        if let Err(error) = &result {
            latch_live_error(&writer_signals, error.clone());
        }
        result
    });

    let stream = build_input_stream(
        &device,
        &config,
        sample_format,
        sender.clone(),
        signals.clone(),
    )?;
    stream
        .play()
        .map_err(|error| format!("Could not start the microphone: {error}"))?;
    diagnostics::log(format!(
        "microphone \"{device_name}\" open after {} ms: {} Hz, {} channel(s), {sample_format:?}",
        opening.elapsed().as_millis(),
        config.sample_rate,
        config.channels
    ));

    // Only now, with the microphone already running: opening a capture stream
    // can reconfigure the device that plays the cue — a headset switching
    // profile, a dock re-plumbing its shared codec — and a cue started before
    // that would be cut off while every sample still counts as taken. Its
    // failure is reported by `arm`, not here: a silent cue must not cost the
    // recording. Capture discards until `arm` says the cue has been heard.
    let cue = sound_enabled.then(|| feedback::start(Cue::Start));

    let started_at = Instant::now();
    guard.live_signals = signals.live_enabled.then(|| signals.clone());
    guard.active = Some(ActiveRecording {
        started_at,
        sound_enabled,
        stream,
        sender,
        writer,
        signals,
        clock: RecordingClock::new(started_at),
    });
    Ok(StartedRecording {
        session: started_at,
        signals: started,
        cue,
    })
}

fn recording_directory() -> PathBuf {
    std::env::temp_dir().join(RECORDING_DIRECTORY)
}

pub fn status(state: &AudioCaptureState) -> Result<RecordingStatus, String> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable")?;
    Ok(status_inner(&guard))
}

fn status_inner(guard: &CaptureInner) -> RecordingStatus {
    RecordingStatus {
        recording: guard.active.is_some(),
        paused: guard
            .active
            .as_ref()
            .is_some_and(|active| active.clock.paused_at.is_some()),
        limit_reached: guard.completed.is_some(),
        elapsed_seconds: guard.active.as_ref().map_or(
            if guard.completed.is_some() {
                MAX_RECORDING_DURATION.as_secs()
            } else {
                0
            },
            |active| active.clock.elapsed(Instant::now()).as_secs(),
        ),
        level: guard.active.as_ref().map_or(0.0, |active| {
            if active.clock.paused_at.is_some() {
                0.0
            } else {
                f32::from_bits(active.signals.level.load(Ordering::Relaxed))
            }
        }),
    }
}

pub fn set_paused(state: &AudioCaptureState, paused: bool) -> Result<RecordingStatus, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable")?;
    if let Some(active) = guard.active.as_mut() {
        // Keep the device stream open across platforms. Paused callbacks discard
        // samples before allocating/writing; no silence or paused speech is stored.
        active.signals.paused.store(paused, Ordering::Release);
        active.clock.set_paused(paused, Instant::now());
        active.signals.level.store(0, Ordering::Relaxed);
    } else if guard.completed.is_none() {
        return Err("No recording is active".into());
    }
    // The watchdog may have won the race. Return its completed status so the UI
    // consumes that artifact once rather than getting stuck in a paused phase.
    Ok(status_inner(&guard))
}

// Runs on a native watchdog, not a WebView timer (which can be throttled when hidden).
pub fn check_limit(state: &AudioCaptureState, session: Instant) -> Result<LimitCheck, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable")?;
    match guard.active.as_ref() {
        Some(active) if active.started_at == session => {
            if active.clock.elapsed(Instant::now()) < MAX_RECORDING_DURATION {
                return Ok(LimitCheck::Waiting);
            }
        }
        _ => return Ok(LimitCheck::Gone),
    }
    if let Some(active) = guard.active.take() {
        guard.completed = Some(finalize(active));
    }
    Ok(LimitCheck::Stopped)
}

pub fn stop_recording(state: &AudioCaptureState) -> Result<RecordingArtifact, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable")?;
    if let Some(completed) = guard.completed.take() {
        return completed;
    }
    let active = guard.active.take().ok_or("No recording is active")?;
    finalize(active)
}

fn finalize(active: ActiveRecording) -> Result<RecordingArtifact, String> {
    drop(active.stream);
    drop(active.sender);
    if active.sound_enabled {
        // Detached: the cue confirms capture that has already ended, and a
        // speaker waking from idle must not hold up transcription.
        feedback::play_detached(Cue::Stop);
    }
    let artifact = active
        .writer
        .join()
        .map_err(|_| "The audio writer stopped unexpectedly".to_string())??;

    if let Some(error) = capture_live_error(&active.signals) {
        return Err(error);
    }
    if let Some(error) = active
        .signals
        .stream_error
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?
        .take()
    {
        return Err(error);
    }
    if active.signals.overrun.load(Ordering::Relaxed) {
        return Err("The microphone produced audio faster than it could be stored".into());
    }
    let glitches = active.signals.glitches.load(Ordering::Relaxed);
    if glitches > 0 {
        diagnostics::log(format!(
            "the microphone reported {glitches} gap(s) during the recording"
        ));
    }
    if artifact.sample_count == 0 {
        return Err("The recording is empty".into());
    }
    Ok(artifact)
}

pub fn cancel_recording(state: &AudioCaptureState) -> Result<(), String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable")?;
    guard.completed = None;
    if let Some(active) = guard.active.take() {
        let _ = finalize(active);
    }
    Ok(())
}

fn select_device(host: &cpal::Host, requested: Option<&str>) -> Result<Device, String> {
    if let Some(requested) = requested {
        let devices = host
            .input_devices()
            .map_err(|error| format!("Could not enumerate microphones: {error}"))?;
        for device in devices {
            if device.id().ok().map(|id| id.to_string()).as_deref() == Some(requested) {
                return Ok(device);
            }
        }
    }
    host.default_input_device()
        .ok_or_else(|| "No input device is available".to_string())
}

fn build_input_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    sender: SyncSender<Vec<f32>>,
    signals: CaptureSignals,
) -> Result<Stream, String> {
    macro_rules! stream {
        ($sample:ty, $convert:expr) => {{
            let signals = signals.clone();
            let errors = signals.clone();
            let sender = sender.clone();
            device.build_input_stream(
                *config,
                move |data: &[$sample], _| {
                    capture_chunk(data, $convert, &sender, &signals);
                },
                move |error| note_stream_error(error, &errors),
                None,
            )
        }};
    }

    let result = match format {
        SampleFormat::F32 => stream!(f32, |sample| sample),
        SampleFormat::I16 => stream!(i16, |sample| f32::from(sample) / 32_768.0),
        SampleFormat::U16 => stream!(u16, |sample| f32::from(sample) / 32_767.5 - 1.0),
        unsupported => {
            return Err(format!(
                "The microphone sample format {unsupported:?} is not supported yet"
            ));
        }
    };
    result.map_err(|error| format!("Could not open the microphone: {error}"))
}

/// What the backend reports about the stream while it runs.
///
/// A buffer under- or overrun is not one of the stream's failures: Windows and
/// macOS raise it on a stream that carries on, to say a few milliseconds went
/// missing. Treating it as fatal threw away every recording made through a
/// microphone Windows had just re-enumerated after a dock was plugged back in.
/// Only an error that means the stream is gone ends the recording.
fn note_stream_error(error: cpal::Error, signals: &CaptureSignals) {
    if error.kind() == cpal::ErrorKind::Xrun {
        signals.glitches.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if let Ok(mut slot) = signals.stream_error.lock() {
        *slot = Some(format!("Microphone stream failed: {error}"));
    }
}

fn capture_chunk<T: Copy>(
    data: &[T],
    convert: impl Fn(T) -> f32,
    sender: &SyncSender<Vec<f32>>,
    signals: &CaptureSignals,
) {
    if !signals.armed.load(Ordering::Acquire) || signals.paused.load(Ordering::Acquire) {
        return;
    }
    let chunk = data.iter().copied().map(convert).collect::<Vec<f32>>();
    signals
        .level
        .store(visual_level(&chunk).to_bits(), Ordering::Relaxed);
    if sender.try_send(chunk).is_err() {
        signals.overrun.store(true, Ordering::Relaxed);
    }
}

fn visual_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms =
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt();
    // Compress speech dynamics into a restrained, useful visual range.
    (rms * 5.0).sqrt().clamp(0.0, 1.0)
}

/// Incremental, centred windowed-sinc resampling. Channel frames and the
/// rational output clock persist across callback boundaries. The small lookahead
/// provides an anti-alias low-pass filter, with edge extension only at start/end.
/// History is bounded to the FIR footprint, not the recording duration.
struct LiveResampler {
    source_rate: u32,
    channels: usize,
    channel_sum: f64,
    channel_count: usize,
    frames: VecDeque<f64>,
    first_frame: f64,
    base_frame: u64,
    total_frames: u64,
    output_index: u64,
    cutoff: f64,
    radius: i64,
}

impl LiveResampler {
    fn new(source_rate: u32, channels: u16) -> Self {
        let source_rate = source_rate.max(1);
        let ratio = (f64::from(LIVE_SAMPLE_RATE) / f64::from(source_rate)).min(1.0);
        Self {
            source_rate,
            channels: usize::from(channels.max(1)),
            channel_sum: 0.0,
            channel_count: 0,
            frames: VecDeque::new(),
            first_frame: 0.0,
            base_frame: 0,
            total_frames: 0,
            output_index: 0,
            cutoff: ratio * 0.90,
            radius: (32.0 / ratio).ceil() as i64,
        }
    }

    fn push(
        &mut self,
        samples: &[f32],
        mut emit: impl FnMut(i16) -> Result<(), String>,
    ) -> Result<(), String> {
        for &sample in samples {
            // A bad floating point sample must not contaminate filter history.
            self.channel_sum += if sample.is_finite() {
                f64::from(sample.clamp(-1.0, 1.0))
            } else {
                0.0
            };
            self.channel_count += 1;
            if self.channel_count == self.channels {
                let mono = self.channel_sum / self.channels as f64;
                if self.total_frames == 0 {
                    self.first_frame = mono;
                }
                self.frames.push_back(mono);
                self.total_frames += 1;
                self.channel_sum = 0.0;
                self.channel_count = 0;
                self.emit_ready(false, &mut emit)?;
            }
        }
        Ok(())
    }

    fn finish(&mut self, mut emit: impl FnMut(i16) -> Result<(), String>) -> Result<(), String> {
        if self.channel_count != 0 {
            return Err("Live recording ended with an incomplete microphone channel frame".into());
        }
        self.emit_ready(true, &mut emit)
    }

    fn emit_ready(
        &mut self,
        finishing: bool,
        emit: &mut impl FnMut(i16) -> Result<(), String>,
    ) -> Result<(), String> {
        let output_count =
            self.total_frames * u64::from(LIVE_SAMPLE_RATE) / u64::from(self.source_rate);
        while self.output_index < output_count {
            let numerator = self.output_index * u64::from(self.source_rate);
            let centre = (numerator / u64::from(LIVE_SAMPLE_RATE)) as i64;
            if !finishing && centre + self.radius >= self.total_frames as i64 {
                break;
            }
            let fraction =
                (numerator % u64::from(LIVE_SAMPLE_RATE)) as f64 / f64::from(LIVE_SAMPLE_RATE);
            let mut weighted = 0.0;
            let mut weight_sum = 0.0;
            for index in centre - self.radius + 1..=centre + self.radius {
                let distance = index as f64 - centre as f64 - fraction;
                let scaled = distance * self.cutoff;
                let sinc = if scaled.abs() < 1e-12 {
                    1.0
                } else {
                    (std::f64::consts::PI * scaled).sin() / (std::f64::consts::PI * scaled)
                };
                let window =
                    0.5 + 0.5 * (std::f64::consts::PI * distance / self.radius as f64).cos();
                let weight = sinc * window;
                let sample = if index < 0 {
                    self.first_frame
                } else if index >= self.total_frames as i64 {
                    *self.frames.back().unwrap_or(&0.0)
                } else {
                    self.frames[(index as u64 - self.base_frame) as usize]
                };
                weighted += sample * weight;
                weight_sum += weight;
            }
            let normalized = (weighted / weight_sum).clamp(-1.0, 1.0);
            emit((normalized * f64::from(i16::MAX)).round() as i16)?;
            self.output_index += 1;
            let next_centre =
                self.output_index * u64::from(self.source_rate) / u64::from(LIVE_SAMPLE_RATE);
            let retain_from = next_centre.saturating_sub(self.radius as u64);
            while self.base_frame < retain_from && !self.frames.is_empty() {
                self.frames.pop_front();
                self.base_frame += 1;
            }
        }
        Ok(())
    }
}

struct LiveTap {
    resampler: LiveResampler,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    pending: Vec<u8>,
}

impl LiveTap {
    fn new(sender: tokio::sync::mpsc::Sender<Vec<u8>>, source_rate: u32, channels: u16) -> Self {
        Self {
            resampler: LiveResampler::new(source_rate, channels),
            sender,
            pending: Vec::with_capacity(LIVE_CHUNK_SAMPLES * 2),
        }
    }

    fn push(&mut self, samples: &[f32]) -> Result<(), String> {
        let Self {
            resampler,
            sender,
            pending,
        } = self;
        resampler.push(samples, |sample| Self::append(sender, pending, sample))
    }

    fn finish(&mut self) -> Result<(), String> {
        let Self {
            resampler,
            sender,
            pending,
        } = self;
        resampler.finish(|sample| Self::append(sender, pending, sample))?;
        if !pending.is_empty() {
            Self::flush(sender, pending)?;
        }
        Ok(())
    }

    fn append(
        sender: &tokio::sync::mpsc::Sender<Vec<u8>>,
        pending: &mut Vec<u8>,
        sample: i16,
    ) -> Result<(), String> {
        pending.extend_from_slice(&sample.to_le_bytes());
        if pending.len() == LIVE_CHUNK_SAMPLES * 2 {
            Self::flush(sender, pending)?;
        }
        Ok(())
    }

    fn flush(
        sender: &tokio::sync::mpsc::Sender<Vec<u8>>,
        pending: &mut Vec<u8>,
    ) -> Result<(), String> {
        let packet = std::mem::replace(pending, Vec::with_capacity(LIVE_CHUNK_SAMPLES * 2));
        sender.try_send(packet).map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                "Live recording stopped: the connection could not keep up with microphone audio"
                    .into()
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                "Live recording stopped: the audio connection was closed".into()
            }
        })
    }
}

fn resample_linear(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if input.is_empty() || source_rate == 0 || target_rate == 0 {
        return Vec::new();
    }
    if source_rate == target_rate {
        return input.to_vec();
    }

    let output_len =
        ((input.len() as u64 * u64::from(target_rate)) / u64::from(source_rate)).max(1) as usize;
    let ratio = source_rate as f64 / target_rate as f64;
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let left = position.floor() as usize;
            let right = (left + 1).min(input.len() - 1);
            let fraction = (position - left as f64) as f32;
            input[left.min(input.len() - 1)] * (1.0 - fraction) + input[right] * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_pcm(input: &[f32], rate: u32, channels: u16, partition: usize) -> Vec<i16> {
        let mut resampler = LiveResampler::new(rate, channels);
        let mut output = Vec::new();
        for chunk in input.chunks(partition) {
            resampler
                .push(chunk, |sample| {
                    output.push(sample);
                    Ok(())
                })
                .unwrap();
            assert!(resampler.frames.len() <= (resampler.radius * 2 + 4) as usize);
        }
        resampler
            .finish(|sample| {
                output.push(sample);
                Ok(())
            })
            .unwrap();
        output
    }

    #[test]
    fn live_resampling_is_invariant_to_callback_and_channel_boundaries() {
        let input = (0..8_820)
            .map(|i| (i as f32 * 0.03).sin() * 0.8)
            .collect::<Vec<_>>();
        for rate in [16_000, 24_000, 44_100, 48_000, 96_000] {
            let expected = live_pcm(&input, rate, 2, input.len());
            assert_eq!(
                expected.len(),
                input.len() / 2 * LIVE_SAMPLE_RATE as usize / rate as usize
            );
            for partition in [1, 3, 127, 960] {
                assert_eq!(
                    live_pcm(&input, rate, 2, partition),
                    expected,
                    "rate {rate}, partition {partition}"
                );
            }
        }
    }

    #[test]
    fn live_stereo_downmix_preserves_dc_and_filters_above_nyquist() {
        let stereo = [0.75, -0.25].repeat(4_800);
        let output = live_pcm(&stereo, 48_000, 2, 31);
        assert_eq!(output.len(), 2_400);
        assert!(output.iter().all(|&sample| sample == 8_192));
        let high = (0..4_800)
            .map(|i| (std::f32::consts::TAU * 18_000.0 * i as f32 / 48_000.0).sin())
            .collect::<Vec<_>>();
        let low = (0..4_800)
            .map(|i| (std::f32::consts::TAU * 1_000.0 * i as f32 / 48_000.0).sin())
            .collect::<Vec<_>>();
        let rms = |pcm: Vec<i16>| {
            (pcm[100..pcm.len() - 100]
                .iter()
                .map(|&v| f64::from(v).powi(2))
                .sum::<f64>()
                / (pcm.len() - 200) as f64)
                .sqrt()
        };
        assert!(rms(live_pcm(&high, 48_000, 1, 97)) < 100.0);
        assert!(rms(live_pcm(&low, 48_000, 1, 97)) > 20_000.0);
    }

    #[test]
    fn live_empty_nonfinite_and_incomplete_frames_are_handled() {
        assert!(live_pcm(&[], 48_000, 1, 1).is_empty());
        assert!(
            live_pcm(
                &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY].repeat(100),
                24_000,
                1,
                1
            )
            .iter()
            .all(|&sample| sample == 0)
        );
        assert!(
            live_pcm(&[2.0; 100], 24_000, 1, 7)
                .iter()
                .all(|&sample| sample == i16::MAX)
        );
        let mut resampler = LiveResampler::new(48_000, 2);
        resampler.push(&[0.5], |_| Ok(())).unwrap();
        assert!(
            resampler
                .finish(|_| Ok(()))
                .unwrap_err()
                .contains("incomplete")
        );
    }

    #[test]
    fn live_packets_flush_final_tail_then_close() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        let mut tap = LiveTap::new(sender, 24_000, 1);
        tap.push(&[0.25; 1_001]).unwrap();
        tap.finish().unwrap();
        drop(tap);
        let mut sizes = Vec::new();
        while let Ok(chunk) = receiver.try_recv() {
            sizes.push(chunk.len());
            assert!(
                chunk
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|bytes| i16::from_le_bytes(*bytes) == 8_192)
            );
        }
        assert_eq!(sizes, [960, 960, 82]);
        assert!(receiver.is_closed());
    }

    #[test]
    fn live_backpressure_disconnect_and_capture_errors_are_visible_and_latched() {
        let (sender, _receiver) = tokio::sync::mpsc::channel(1);
        let mut tap = LiveTap::new(sender, 24_000, 1);
        let error = tap.push(&[0.0; 2_000]).unwrap_err();
        assert!(error.contains("could not keep up"));
        let signals = CaptureSignals {
            live_enabled: true,
            ..CaptureSignals::default()
        };
        latch_live_error(&signals, error.clone());
        latch_live_error(&signals, "later".into());
        let state = AudioCaptureState::default();
        state.inner.lock().unwrap().live_signals = Some(signals);
        assert_eq!(live_error(&state), Some(error));

        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        drop(receiver);
        let mut tap = LiveTap::new(sender, 24_000, 1);
        tap.push(&[0.0; 10]).unwrap();
        assert!(tap.finish().unwrap_err().contains("closed"));

        let signals = CaptureSignals {
            live_enabled: true,
            ..CaptureSignals::default()
        };
        signals.overrun.store(true, Ordering::Relaxed);
        assert!(
            capture_live_error(&signals)
                .unwrap()
                .contains("stored fast enough")
        );
        let signals = CaptureSignals {
            live_enabled: true,
            ..CaptureSignals::default()
        };
        note_stream_error(cpal::Error::from(cpal::ErrorKind::Xrun), &signals);
        assert!(capture_live_error(&signals).unwrap().contains("gap"));
        note_stream_error(
            cpal::Error::from(cpal::ErrorKind::DeviceNotAvailable),
            &signals,
        );
        assert!(
            capture_live_error(&signals)
                .unwrap()
                .contains("Microphone stream failed")
        );
    }

    #[test]
    fn clock_excludes_repeated_pauses_and_keeps_the_active_time_limit() {
        let start = Instant::now();
        let mut clock = RecordingClock::new(start);
        clock.set_paused(true, start + Duration::from_secs(12));
        clock.set_paused(true, start + Duration::from_secs(60));
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(900)),
            Duration::from_secs(12)
        );
        clock.set_paused(false, start + Duration::from_secs(912));
        clock.set_paused(false, start + Duration::from_secs(913));
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(920)),
            Duration::from_secs(20)
        );
        clock.set_paused(true, start + Duration::from_secs(920));
        clock.set_paused(false, start + Duration::from_secs(940));
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(1519)),
            MAX_RECORDING_DURATION - Duration::from_secs(1)
        );
        assert_eq!(
            clock.elapsed(start + Duration::from_secs(1520)),
            MAX_RECORDING_DURATION
        );
    }

    #[test]
    fn nothing_is_kept_until_the_start_cue_has_been_played() {
        // The microphone is open while the cue plays, so that the recording is
        // ready the moment the user hears it. Keeping those samples would put
        // the cue into its own recording.
        let signals = CaptureSignals::default();
        let (sender, receiver) = sync_channel(4);
        capture_chunk(
            &[0.9; 500],
            |_| panic!("Audio before the start cue must not be converted"),
            &sender,
            &signals,
        );
        signals.armed.store(true, Ordering::Release);
        capture_chunk(&[0.1, 0.2], |sample| sample, &sender, &signals);
        drop(sender);
        assert_eq!(
            receiver.into_iter().flatten().collect::<Vec<_>>(),
            vec![0.1, 0.2]
        );
        // Discarding is not an overrun: nothing was dropped for want of room.
        assert!(!signals.overrun.load(Ordering::Relaxed));
    }

    #[test]
    fn a_gap_in_the_audio_is_counted_and_only_a_lost_device_ends_the_recording() {
        let signals = CaptureSignals::default();
        note_stream_error(cpal::Error::from(cpal::ErrorKind::Xrun), &signals);
        note_stream_error(cpal::Error::from(cpal::ErrorKind::Xrun), &signals);
        assert_eq!(signals.glitches.load(Ordering::Relaxed), 2);
        assert!(signals.stream_error.lock().unwrap().is_none());

        note_stream_error(
            cpal::Error::from(cpal::ErrorKind::DeviceNotAvailable),
            &signals,
        );
        let error = signals.stream_error.lock().unwrap().clone();
        assert!(error.is_some_and(|error| error.starts_with("Microphone stream failed")));
    }

    #[test]
    fn paused_audio_is_discarded_without_silence_or_conversion() {
        let signals = CaptureSignals::default();
        signals.armed.store(true, Ordering::Release);
        let (sender, receiver) = sync_channel(4);
        capture_chunk(&[0.1, 0.2], |sample| sample, &sender, &signals);
        signals.paused.store(true, Ordering::Release);
        capture_chunk(
            &[0.9; 500],
            |_| panic!("Paused samples must not be converted"),
            &sender,
            &signals,
        );
        signals.paused.store(false, Ordering::Release);
        capture_chunk(&[0.3, 0.4], |sample| sample, &sender, &signals);
        drop(sender);
        assert_eq!(
            receiver.into_iter().flatten().collect::<Vec<_>>(),
            vec![0.1, 0.2, 0.3, 0.4]
        );
        assert!(!signals.overrun.load(Ordering::Relaxed));
    }

    #[test]
    fn pause_after_limit_returns_completed_status_without_consuming_audio() {
        let state = AudioCaptureState::default();
        assert!(set_paused(&state, true).is_err());
        state.inner.lock().unwrap().completed = Some(Err("Synthetic artifact".into()));
        let status = set_paused(&state, true).unwrap();
        assert!(status.limit_reached);
        assert!(!status.paused);
        assert!(!status.recording);
        assert!(state.inner.lock().unwrap().completed.is_some());
    }

    #[test]
    fn metering_is_bounded_and_silence_is_quiet() {
        assert_eq!(visual_level(&[]), 0.0);
        assert_eq!(visual_level(&[0.0; 100]), 0.0);
        assert_eq!(visual_level(&[1.0; 100]), 1.0);
        assert!(visual_level(&[0.02; 100]) > visual_level(&[0.001; 100]));
    }

    #[test]
    fn inactive_watchdog_does_not_touch_another_session() {
        let state = AudioCaptureState::default();
        assert!(matches!(
            check_limit(&state, Instant::now()).unwrap(),
            LimitCheck::Gone
        ));
        assert!(!status(&state).unwrap().recording);
        assert!(!status(&state).unwrap().limit_reached);
        assert!(stop_recording(&state).is_err());
        assert!(cancel_recording(&state).is_ok());
    }

    #[test]
    fn auto_stopped_audio_is_consumed_exactly_once() {
        let state = AudioCaptureState::default();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recording.wav");
        std::fs::write(&path, []).unwrap();
        state.inner.lock().unwrap().completed = Some(Ok(RecordingArtifact {
            path: path.clone(),
            sample_rate: 16000,
            channels: 1,
            sample_count: 16000,
        }));
        assert!(status(&state).unwrap().limit_reached);
        let artifact = stop_recording(&state).unwrap();
        assert_eq!(artifact.duration_ms(), 1000);
        assert!(!status(&state).unwrap().limit_reached);
        assert!(stop_recording(&state).is_err());
        drop(artifact);
        assert!(!path.exists());
    }

    #[test]
    fn resampling_preserves_duration() {
        let input = vec![0.25; 48_000];
        let output = resample_linear(&input, 48_000, 16_000);
        assert_eq!(output.len(), 16_000);
        assert!(
            output
                .iter()
                .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn resampling_empty_input_is_safe() {
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
    }
}
