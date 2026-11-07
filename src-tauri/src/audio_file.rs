//! Audio files the user hands Utterform: a voice message, a recording.
//!
//! A file is decoded once into the same kind of temporary WAV a microphone
//! recording leaves behind — 16 kHz, mono, 16-bit — and from there takes the
//! recording's own path: transcription, the quiet retry, the kept recording,
//! history and delivery. Nothing downstream has to know where the audio came
//! from, and the copy is deleted like every other recording.
//!
//! Symphonia reads the containers and most codecs in pure Rust. Opus, which
//! WhatsApp and Telegram voice messages use, comes from libopus, compiled into
//! the binary, because Symphonia has no Opus decoder of its own.
//!
//! The file's name and path are the user's and never reach the log.

use std::{
    fs::File,
    io::BufWriter,
    path::Path,
    time::{Duration, Instant},
};

use symphonia::core::{
    codecs::{audio::AudioDecoderOptions, registry::CodecRegistry},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

use crate::{
    audio::{self, RecordingArtifact, WHISPER_SAMPLE_RATE},
    diagnostics::{self, Failure},
};

/// Long enough for a meeting, short enough that the decoded audio Whisper
/// reads stays within a few hundred megabytes.
pub const MAX_DURATION: Duration = Duration::from_secs(2 * 60 * 60);

/// The decoded copy, and what the log may say about where it came from.
pub struct Decoded {
    pub artifact: RecordingArtifact,
    /// The codec, as Symphonia names it — `opus`, `aac`, `mp3`…
    pub codec: String,
    pub source_rate: u32,
    pub source_channels: usize,
    /// Packets that could not be decoded and were left out.
    pub skipped_packets: u64,
    pub decode_ms: u128,
}

/// Decode `path` into a temporary 16 kHz mono WAV for recording `recording`.
///
/// Blocking: run it off the async runtime.
pub fn decode(path: &Path, recording: u64) -> Result<Decoded, Failure> {
    let began = Instant::now();
    let file = File::open(path).map_err(|error| {
        let message = if error.kind() == std::io::ErrorKind::NotFound {
            "The audio file could not be found."
        } else {
            "The audio file could not be opened."
        };
        Failure::io("file_open", &error, message)
    })?;
    if file.metadata().is_ok_and(|metadata| metadata.is_dir()) {
        return Err(Failure::guidance(
            "file_not_audio",
            "That is a folder, not an audio file.",
        ));
    }

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| {
            Failure::guidance("file_not_audio", unreadable_message()).detail(error_kind(&error))
        })?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| Failure::guidance("file_no_audio", "The file contains no audio."))?;
    let track_id = track.id;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| Failure::guidance("file_no_audio", "The file contains no audio."))?
        .clone();
    let mut decoder = codecs()
        .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
        .map_err(|error| {
            Failure::guidance("file_codec", unreadable_message()).detail(error_kind(&error))
        })?;
    let codec = codecs()
        .get_audio_decoder(parameters.codec)
        .map(|decoder| decoder.codec.info.short_name.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let (output, temporary) = audio::new_temporary_wav()?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let write_failed = |error: hound::Error| {
        Failure::new(
            "audio_writer",
            format!("Could not write the decoded audio: {error}"),
        )
    };
    let mut wav = hound::WavWriter::new(BufWriter::new(output), spec).map_err(write_failed)?;

    let limit = MAX_DURATION.as_secs() * u64::from(WHISPER_SAMPLE_RATE);
    let mut written = 0_u64;
    let mut resampler: Option<Resampler> = None;
    let mut source_rate = 0;
    let mut source_channels = 0;
    let mut interleaved = Vec::<f32>::new();
    let mut decoded_packets = 0_u64;
    let mut skipped_packets = 0_u64;
    let mut write_error = None;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            // A chained Ogg stream starts over with new tracks; what came
            // before is a whole message already.
            Ok(None) | Err(SymphoniaError::ResetRequired) => break,
            // A file cut off mid-packet still holds everything before the cut.
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => {
                return Err(
                    Failure::new("file_decode", unreadable_message()).detail(error_kind(&error))
                );
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => {
                skipped_packets += 1;
                continue;
            }
            Err(error) => {
                return Err(
                    Failure::new("file_decode", unreadable_message()).detail(error_kind(&error))
                );
            }
        };
        decoded_packets += 1;
        let channels = buffer.spec().channels().count().max(1);
        let rate = buffer.spec().rate();
        if buffer.frames() == 0 || rate == 0 {
            continue;
        }
        let resampler = resampler.get_or_insert_with(|| {
            source_rate = rate;
            source_channels = channels;
            Resampler::new(rate, WHISPER_SAMPLE_RATE)
        });
        interleaved.resize(buffer.samples_interleaved(), 0.0);
        buffer.copy_to_slice_interleaved(&mut interleaved);
        for frame in interleaved.chunks(channels) {
            let mono = frame.iter().sum::<f32>() / frame.len() as f32;
            resampler.push(mono, |sample| {
                if write_error.is_none() {
                    if let Err(error) = wav.write_sample(to_pcm(sample)) {
                        write_error = Some(error);
                    }
                    written += 1;
                }
            });
        }
        if let Some(error) = write_error.take() {
            return Err(write_failed(error));
        }
        if written > limit {
            return Err(Failure::guidance(
                "file_too_long",
                format!(
                    "The audio file is longer than {} hours.",
                    MAX_DURATION.as_secs() / 3600
                ),
            ));
        }
    }
    if let Some(resampler) = resampler.as_mut() {
        resampler.finish(|sample| {
            if write_error.is_none() {
                if let Err(error) = wav.write_sample(to_pcm(sample)) {
                    write_error = Some(error);
                }
                written += 1;
            }
        });
    }
    if let Some(error) = write_error {
        return Err(write_failed(error));
    }
    wav.finalize().map_err(write_failed)?;

    if decoded_packets == 0 && skipped_packets > 0 {
        return Err(Failure::new("file_decode", unreadable_message())
            .detail(format!("{skipped_packets} packets undecodable")));
    }
    if written == 0 {
        return Err(Failure::guidance(
            "empty_recording",
            "The audio file contains no sound.",
        ));
    }
    if skipped_packets > 0 {
        diagnostics::warning!(
            "file.packets_skipped",
            recording = recording,
            skipped = skipped_packets,
            decoded = decoded_packets
        );
    }

    Ok(Decoded {
        artifact: RecordingArtifact {
            recording,
            path: temporary.into_path()?,
            sample_rate: WHISPER_SAMPLE_RATE,
            channels: 1,
            sample_count: written,
            interrupted: false,
        },
        codec,
        source_rate,
        source_channels,
        skipped_packets,
        decode_ms: began.elapsed().as_millis(),
    })
}

fn unreadable_message() -> String {
    "Utterform cannot read this file. Voice messages (Opus, Ogg), M4A, MP3, WAV and FLAC work."
        .to_string()
}

/// What kind of error Symphonia met, without anything it may have quoted.
fn error_kind(error: &SymphoniaError) -> &'static str {
    match error {
        SymphoniaError::IoError(_) => "io",
        SymphoniaError::DecodeError(_) => "decode",
        SymphoniaError::SeekError(_) => "seek",
        SymphoniaError::Unsupported(_) => "unsupported",
        SymphoniaError::LimitError(_) => "limit",
        SymphoniaError::ResetRequired => "reset",
        _ => "other",
    }
}

fn to_pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Symphonia's decoders plus libopus.
fn codecs() -> &'static CodecRegistry {
    static REGISTRY: std::sync::OnceLock<CodecRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        registry
    })
}

/// Streaming sample-rate conversion of one mono channel.
///
/// Going down — 48 kHz Opus, 44.1 kHz MP3 — every output sample is the mean of
/// the input samples it covers, which keeps what lies above the new Nyquist
/// frequency from folding back into speech. Going up — 8 kHz telephone audio —
/// it interpolates linearly. Either way it holds a few samples, never the file.
struct Resampler {
    /// Input samples per output sample.
    ratio: f64,
    seen: u64,
    next_output: u64,
    sum: f32,
    count: u32,
    previous: Option<f32>,
}

impl Resampler {
    fn new(source_rate: u32, target_rate: u32) -> Self {
        Self {
            ratio: f64::from(source_rate) / f64::from(target_rate),
            seen: 0,
            next_output: 0,
            sum: 0.0,
            count: 0,
            previous: None,
        }
    }

    fn push(&mut self, sample: f32, mut emit: impl FnMut(f32)) {
        if self.ratio >= 1.0 {
            let bin = (self.seen as f64 / self.ratio) as u64;
            if bin != self.next_output && self.count > 0 {
                emit(self.sum / self.count as f32);
                self.sum = 0.0;
                self.count = 0;
            }
            self.next_output = bin;
            self.sum += sample;
            self.count += 1;
        } else {
            match self.previous {
                None => {
                    emit(sample);
                    self.next_output = 1;
                }
                Some(previous) => {
                    let left = (self.seen - 1) as f64;
                    loop {
                        let position = self.next_output as f64 * self.ratio;
                        if position > self.seen as f64 {
                            break;
                        }
                        let fraction = (position - left) as f32;
                        emit(previous + (sample - previous) * fraction);
                        self.next_output += 1;
                    }
                }
            }
            self.previous = Some(sample);
        }
        self.seen += 1;
    }

    fn finish(&mut self, mut emit: impl FnMut(f32)) {
        if self.ratio >= 1.0 && self.count > 0 {
            emit(self.sum / self.count as f32);
            self.count = 0;
            self.sum = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
        let mut resampler = Resampler::new(from, to);
        let mut output = Vec::new();
        for sample in input {
            resampler.push(*sample, |value| output.push(value));
        }
        resampler.finish(|value| output.push(value));
        output
    }

    #[test]
    fn opus_rate_becomes_whisper_rate_by_averaging_each_three_samples() {
        let input = [0.3, 0.6, 0.9, -0.3, -0.6, -0.9];
        let output = resample(&input, 48_000, 16_000);
        assert_eq!(output.len(), 2);
        assert!((output[0] - 0.6).abs() < 1e-6);
        assert!((output[1] + 0.6).abs() < 1e-6);
    }

    #[test]
    fn the_length_follows_the_rate_for_odd_ratios() {
        for (from, seconds) in [
            (44_100, 3),
            (22_050, 2),
            (16_000, 1),
            (8_000, 2),
            (11_025, 1),
        ] {
            let input = vec![0.1; (from * seconds) as usize];
            let output = resample(&input, from, 16_000);
            let expected = 16_000 * seconds as usize;
            assert!(
                output.len().abs_diff(expected) <= 1,
                "{from} Hz: {} instead of {expected}",
                output.len()
            );
            assert!(output.iter().all(|sample| (sample - 0.1).abs() < 1e-6));
        }
    }

    #[test]
    fn upsampling_interpolates_between_neighbours() {
        let output = resample(&[0.0, 1.0], 8_000, 16_000);
        assert_eq!(output, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn a_tone_above_the_new_nyquist_does_not_fold_back_at_full_strength() {
        // 12 kHz at 48 kHz: four samples per cycle. Averaging three of them
        // leaves a third of the amplitude rather than all of it.
        let input = (0..48_000)
            .map(|index| (index as f32 * std::f32::consts::FRAC_PI_2).sin())
            .collect::<Vec<_>>();
        let output = resample(&input, 48_000, 16_000);
        let peak = output
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak < 0.4, "{peak}");
    }

    fn write_wav(path: &Path, rate: u32, channels: u16, frames: usize) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for index in 0..frames {
            let value = ((index as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin();
            for _ in 0..channels {
                writer.write_sample((value * 8_000.0) as i16).unwrap();
            }
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn a_stereo_wav_becomes_whisper_audio_of_the_same_length() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Sprachnachricht von Anna.wav");
        write_wav(&path, 44_100, 2, 44_100 * 2);
        let decoded = decode(&path, 7).unwrap();
        assert_eq!(decoded.source_rate, 44_100);
        assert_eq!(decoded.source_channels, 2);
        assert_eq!(decoded.codec, "pcm_s16le");
        let artifact = &decoded.artifact;
        assert_eq!(artifact.recording, 7);
        assert_eq!(artifact.sample_rate, 16_000);
        assert_eq!(artifact.channels, 1);
        assert!(artifact.duration_ms().abs_diff(2_000) <= 1);
        let pcm = artifact.whisper_pcm().unwrap();
        assert!(pcm.len().abs_diff(32_000) <= 1);
        assert!(pcm.iter().any(|sample| sample.abs() > 0.1));
        // The copy is Utterform's own temporary file, never the user's.
        assert_ne!(artifact.path, path);
        let copy = artifact.path.clone();
        drop(decoded);
        assert!(!copy.exists());
        assert!(path.exists());
    }

    #[test]
    fn a_missing_file_says_so_without_naming_it() {
        let failure = match decode(Path::new("/nowhere/Geheim.opus"), 1) {
            Err(failure) => failure,
            Ok(_) => panic!("decoded a missing file"),
        };
        assert_eq!(failure.class(), "file_open");
        assert!(!failure.message().contains("Geheim"));
    }

    #[test]
    fn a_text_file_is_not_audio() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notes.txt");
        std::fs::write(&path, "Dies ist keine Sprachnachricht.\n".repeat(200)).unwrap();
        let failure = match decode(&path, 1) {
            Err(failure) => failure,
            Ok(_) => panic!("decoded text as audio"),
        };
        assert_eq!(failure.class(), "file_not_audio");
        assert!(!failure.message().contains("notes"));
    }

    #[test]
    fn a_folder_is_not_audio() {
        let directory = tempfile::tempdir().unwrap();
        let failure = match decode(directory.path(), 1) {
            Err(failure) => failure,
            Ok(_) => panic!("decoded a folder"),
        };
        assert!(matches!(failure.class(), "file_not_audio" | "file_open"));
    }

    #[test]
    fn silence_of_zero_length_is_an_empty_recording() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("empty.wav");
        write_wav(&path, 16_000, 1, 0);
        let failure = match decode(&path, 1) {
            Err(failure) => failure,
            Ok(_) => panic!("decoded nothing into something"),
        };
        assert!(matches!(
            failure.class(),
            "empty_recording" | "file_no_audio" | "file_not_audio"
        ));
    }

    /// A real WhatsApp-style voice message: Ogg Opus, mono, 48 kHz decode rate.
    /// Generated once with `ffmpeg -f lavfi -i sine=440:d=1.5 -ac 1 -c:a libopus
    /// -b:a 24k -application voip voice.opus`; 1.5 s of tone at ffmpeg's default
    /// amplitude of 1/8, no voice.
    #[test]
    fn a_whatsapp_voice_message_decodes_through_libopus() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/voice.opus");
        let decoded = decode(&path, 3).unwrap();
        assert_eq!(decoded.codec, "opus");
        assert_eq!(decoded.source_rate, 48_000);
        assert_eq!(decoded.source_channels, 1);
        assert_eq!(decoded.skipped_packets, 0);
        let duration = decoded.artifact.duration_ms();
        assert!(duration.abs_diff(1_500) <= 30, "{duration} ms");
        let pcm = decoded.artifact.whisper_pcm().unwrap();
        let peak = pcm
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!((0.1..0.16).contains(&peak), "{peak}");
    }

    #[test]
    fn an_m4a_from_a_phone_decodes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/voice.m4a");
        let decoded = decode(&path, 4).unwrap();
        assert_eq!(decoded.codec, "aac");
        assert!(decoded.artifact.duration_ms().abs_diff(1_500) <= 60);
    }

    #[test]
    fn an_mp3_decodes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/voice.mp3");
        let decoded = decode(&path, 5).unwrap();
        assert_eq!(decoded.codec, "mp3");
        assert!(decoded.artifact.duration_ms().abs_diff(1_500) <= 80);
    }
}
