use std::{
    io::BufWriter,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread,
    time::Duration,
};

use cpal::{
    Device, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::domain::AudioDeviceInfo;

const CHANNEL_CAPACITY: usize = 64;
const WHISPER_SAMPLE_RATE: u32 = 16_000;
const RECORDING_DIRECTORY: &str = "utterform";
const STALE_RECORDING_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub struct AudioCaptureState {
    active: Mutex<Option<ActiveRecording>>,
}

impl Default for AudioCaptureState {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
        }
    }
}

struct ActiveRecording {
    stream: Stream,
    sender: SyncSender<Vec<f32>>,
    writer: thread::JoinHandle<Result<RecordingArtifact, String>>,
    overrun: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
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

pub fn start_recording(
    state: &AudioCaptureState,
    requested_device: Option<&str>,
) -> Result<(), String> {
    let mut guard = state
        .active
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?;
    if guard.is_some() {
        return Err("A recording is already active".into());
    }

    let host = cpal::default_host();
    let device = select_device(&host, requested_device)?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("Could not read the microphone configuration: {error}"))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let (sender, receiver) = sync_channel::<Vec<f32>>(CHANNEL_CAPACITY);
    let overrun = Arc::new(AtomicBool::new(false));
    let stream_error = Arc::new(Mutex::new(None));
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
        overrun.clone(),
        stream_error.clone(),
    )?;
    stream
        .play()
        .map_err(|error| format!("Could not start the microphone: {error}"))?;

    *guard = Some(ActiveRecording {
        stream,
        sender,
        writer,
        overrun,
        stream_error,
    });
    Ok(())
}

fn recording_directory() -> PathBuf {
    std::env::temp_dir().join(RECORDING_DIRECTORY)
}

pub fn stop_recording(state: &AudioCaptureState) -> Result<RecordingArtifact, String> {
    let active = state
        .active
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?
        .take()
        .ok_or_else(|| "No recording is active".to_string())?;

    drop(active.stream);
    drop(active.sender);
    let artifact = active
        .writer
        .join()
        .map_err(|_| "The audio writer stopped unexpectedly".to_string())??;

    if let Some(error) = active
        .stream_error
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?
        .take()
    {
        return Err(error);
    }
    if active.overrun.load(Ordering::Relaxed) {
        return Err("The microphone produced audio faster than it could be stored".into());
    }
    if artifact.sample_count == 0 {
        return Err("The recording is empty".into());
    }
    Ok(artifact)
}

pub fn cancel_recording(state: &AudioCaptureState) -> Result<(), String> {
    let active = state
        .active
        .lock()
        .map_err(|_| "Audio state is unavailable".to_string())?
        .take();
    if let Some(active) = active {
        drop(active.stream);
        drop(active.sender);
        if let Ok(Ok(artifact)) = active.writer.join() {
            drop(artifact);
        }
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
    overrun: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
) -> Result<Stream, String> {
    macro_rules! stream {
        ($sample:ty, $convert:expr) => {{
            let overrun = overrun.clone();
            let sender = sender.clone();
            device.build_input_stream(
                *config,
                move |data: &[$sample], _| {
                    let chunk = data.iter().copied().map($convert).collect::<Vec<f32>>();
                    if sender.try_send(chunk).is_err() {
                        overrun.store(true, Ordering::Relaxed);
                    }
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
