use std::{
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
    domain::AudioDeviceInfo,
    feedback::{self, Cue},
};
use serde::Serialize;

const CHANNEL_CAPACITY: usize = 64;
const WHISPER_SAMPLE_RATE: u32 = 16_000;
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
    stream_error: Arc<Mutex<Option<String>>>,
}

#[derive(Default)]
struct CaptureInner {
    active: Option<ActiveRecording>,
    completed: Option<Result<RecordingArtifact, String>>,
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
    /// `None` when cues are switched off; otherwise the cue or the reason it
    /// could not even be started, kept so `arm` can report it.
    cue: Option<Result<feedback::Playback, String>>,
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
            Some(Ok(playback)) => playback.finish(),
            Some(Err(reason)) => Err(reason),
        };
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
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?;
    if guard.active.is_some() || guard.completed.is_some() {
        return Err("A recording is already active".into());
    }

    // First, because opening the stream is what resumes a suspended speaker:
    // the wake-up then overlaps with opening the microphone instead of
    // following it. Its failure is reported by `arm`, not here — a silent cue
    // must not cost the recording.
    let cue = sound_enabled.then(|| feedback::start(Cue::Start));

    let host = cpal::default_host();
    let device = select_device(&host, requested_device)?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("Could not read the microphone configuration: {error}"))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let (sender, receiver) = sync_channel::<Vec<f32>>(CHANNEL_CAPACITY);
    let signals = CaptureSignals::default();
    let started = signals.clone();
    let writer_config = config;
    let writer = thread::spawn(move || {
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
            for sample in chunk {
                let normalized = sample.clamp(-1.0, 1.0);
                let pcm = (normalized * f32::from(i16::MAX)).round() as i16;
                wav.write_sample(pcm)
                    .map_err(|error| format!("Could not write the recording: {error}"))?;
                sample_count += 1;
            }
        }
        wav.finalize()
            .map_err(|error| format!("Could not finalize the recording: {error}"))?;
        Ok(RecordingArtifact {
            path: temporary.into_path()?,
            sample_rate: writer_config.sample_rate,
            channels: writer_config.channels,
            sample_count,
        })
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

    let started_at = Instant::now();
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
            let stream_error = signals.stream_error.clone();
            let sender = sender.clone();
            device.build_input_stream(
                *config,
                move |data: &[$sample], _| {
                    capture_chunk(data, $convert, &sender, &signals);
                },
                move |error| {
                    if let Ok(mut slot) = stream_error.lock() {
                        *slot = Some(format!("Microphone stream failed: {error}"));
                    }
                },
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
