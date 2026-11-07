use std::{io::Cursor, time::Duration};

use reqwest::{Client, StatusCode, multipart};
use serde::Deserialize;

use crate::{
    actions,
    audio::RecordingArtifact,
    diagnostics::{self, Failure},
    domain::AppSettings,
    secrets,
};

const TRANSCRIPTIONS_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const MAX_UPLOAD_BYTES: u64 = 25 * 1024 * 1024;
/// 16 kHz, 16-bit mono is 32 kB a second: twelve minutes are 23 MB.
const SINGLE_UPLOAD_SAMPLES: usize = 12 * 60 * 16_000;
const PART_SAMPLES: usize = 10 * 60 * 16_000;
const CUT_SEARCH_SAMPLES: usize = 30 * 16_000;
const CUT_WINDOW_SAMPLES: usize = 1_600;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// A ten-minute recording is a ~19 MB upload that the model then has to work
// through, so a two-minute cap on the whole exchange cut off legitimate long
// clips. Bound inactivity instead, and keep a generous overall ceiling so a
// dead connection cannot hold the UI in "Transcribing…" forever.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Deserialize)]
struct ApiErrorEnvelope {
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    output: Vec<ResponseItem>,
}

#[derive(Deserialize)]
struct ResponseItem {
    #[serde(default)]
    content: Vec<ResponseContent>,
}

#[derive(Deserialize)]
struct ResponseContent {
    text: Option<String>,
}

pub async fn transcribe(
    artifact: &RecordingArtifact,
    settings: &AppSettings,
) -> Result<String, Failure> {
    let pcm = artifact.whisper_pcm()?;
    let parts = split_at_pauses(&pcm);
    if parts.len() > 1 {
        diagnostics::info!(
            "transcription.split",
            recording = artifact.recording,
            parts = parts.len()
        );
    }
    let mut texts = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        match transcribe_part(part, settings).await {
            Ok(text) => texts.push(text),
            // A long file can hold minutes of silence; that part simply adds
            // nothing. A single part that is silent is the whole answer.
            Err(failure) if failure.class() == "empty_result" && parts.len() > 1 => {}
            Err(failure) => {
                if parts.len() > 1 {
                    diagnostics::warning!(
                        "transcription.part_failed",
                        recording = artifact.recording,
                        part = index + 1,
                        parts = parts.len(),
                        class = failure.class()
                    );
                }
                return Err(failure);
            }
        }
    }
    if texts.is_empty() {
        return Err(Failure::guidance("empty_result", super::NOTHING_HEARD));
    }
    Ok(texts.join(" "))
}

async fn transcribe_part(samples: &[f32], settings: &AppSettings) -> Result<String, Failure> {
    let audio = wav_bytes(samples)?;
    if audio.len() as u64 > MAX_UPLOAD_BYTES {
        return Err(Failure::guidance(
            "upload_too_large",
            "The recording exceeds the 25 MB OpenAI upload limit",
        ));
    }

    let part = multipart::Part::bytes(audio)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|error| {
            Failure::new(
                "prepare",
                format!("Could not prepare the recording: {error}"),
            )
        })?;
    let mut form = multipart::Form::new()
        .text("model", "gpt-transcribe")
        .part("file", part);
    for language in settings
        .language_hints
        .iter()
        .filter(|value| !value.trim().is_empty())
    {
        form = form.text("languages[]", language.trim().to_string());
    }
    for keyword in usable_keywords(&settings.vocabulary) {
        form = form.text("keywords[]", keyword);
    }
    let context = settings.transcription_context.trim();
    if !context.is_empty() {
        form = form.text("prompt", context.to_string());
    }

    let response = client()?
        .post(TRANSCRIPTIONS_URL)
        .bearer_auth(secrets::openai_api_key()?)
        .multipart(form)
        .send()
        .await
        .map_err(|error| {
            Failure::new(
                transport_class(&error),
                format!("OpenAI transcription request failed: {error}"),
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(api_error(status, response).await);
    }
    let payload = response
        .json::<TranscriptionResponse>()
        .await
        .map_err(|error| {
            Failure::new(
                "invalid_response",
                format!("OpenAI returned an invalid transcription: {error}"),
            )
        })?;
    let text = payload.text.trim();
    if text.is_empty() {
        return Err(Failure::guidance("empty_result", super::NOTHING_HEARD));
    }
    Ok(text.to_string())
}

pub async fn transform(
    transcript: &str,
    action: &str,
    custom_prompt: Option<&str>,
    settings: &AppSettings,
) -> Result<String, Failure> {
    let instructions = action_instructions(action, custom_prompt, settings)?;
    let mut body = serde_json::json!({
        "model": settings.text_model,
        "instructions": instructions,
        "input": transcript,
        "store": false
    });
    // Sent only when the user chose a level: a model that does not offer the
    // one we would have guessed rejects the whole request.
    if let Some(effort) = settings.text_effort {
        body["reasoning"] = serde_json::json!({ "effort": effort.as_str() });
    }
    let response = client()?
        .post(RESPONSES_URL)
        .bearer_auth(secrets::openai_api_key()?)
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            Failure::new(
                transport_class(&error),
                format!("OpenAI text transformation failed: {error}"),
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(api_error(status, response).await);
    }
    let payload = response
        .json::<ResponsesResponse>()
        .await
        .map_err(|error| {
            Failure::new(
                "invalid_response",
                format!("OpenAI returned an invalid transformation: {error}"),
            )
        })?;
    let text = payload
        .output_text
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            let combined = payload
                .output
                .into_iter()
                .flat_map(|item| item.content)
                .filter_map(|content| content.text)
                .collect::<Vec<_>>()
                .join("\n");
            (!combined.trim().is_empty()).then_some(combined)
        })
        .ok_or_else(|| Failure::new("empty_result", "OpenAI returned an empty transformation"))?;
    Ok(text.trim().to_string())
}

fn client() -> Result<Client, Failure> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| {
            Failure::new(
                "client_init",
                format!("Could not initialize the OpenAI client: {error}"),
            )
        })
}

/// What kind of transport failure a request met, without its URL or body.
pub(super) fn transport_class(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
    }
}

/// Where a long recording is cut for upload, as `[start, end)` ranges of
/// 16 kHz samples.
///
/// Anything up to [`SINGLE_UPLOAD_SAMPLES`] goes up whole, as every microphone
/// recording does. Longer audio — a file — is cut roughly every
/// [`PART_SAMPLES`], each cut moved to the quietest tenth of a second in the
/// half minute before it, so a word is rarely split in two.
fn split_at_pauses(samples: &[f32]) -> Vec<&[f32]> {
    if samples.len() <= SINGLE_UPLOAD_SAMPLES {
        return vec![samples];
    }
    let mut parts = Vec::new();
    let mut start = 0;
    while samples.len() - start > SINGLE_UPLOAD_SAMPLES {
        let target = start + PART_SAMPLES;
        let cut = quietest_window(samples, target - CUT_SEARCH_SAMPLES, target);
        parts.push(&samples[start..cut]);
        start = cut;
    }
    parts.push(&samples[start..]);
    parts
}

/// The start of the quietest [`CUT_WINDOW_SAMPLES`] window in `from..to`.
fn quietest_window(samples: &[f32], from: usize, to: usize) -> usize {
    (from..to.saturating_sub(CUT_WINDOW_SAMPLES))
        .step_by(CUT_WINDOW_SAMPLES / 2)
        .min_by(|left, right| {
            let energy = |at: usize| {
                samples[at..at + CUT_WINDOW_SAMPLES]
                    .iter()
                    .map(|sample| sample * sample)
                    .sum::<f32>()
            };
            energy(*left).total_cmp(&energy(*right))
        })
        .map_or(to, |at| at + CUT_WINDOW_SAMPLES / 2)
}

fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>, Failure> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let upload = |error: hound::Error| {
            Failure::new(
                "prepare",
                format!("Could not prepare the recording upload: {error}"),
            )
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(upload)?;
        for sample in samples {
            let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            writer.write_sample(pcm).map_err(upload)?;
        }
        writer.finalize().map_err(|error| {
            Failure::new(
                "prepare",
                format!("Could not finalize the recording upload: {error}"),
            )
        })?;
    }
    Ok(cursor.into_inner())
}

/// Words or phrases the model should expect to hear.
///
/// The API rejects the entire request when a keyword carries `<`, `>`, a
/// carriage return or a line feed, so one stray character would cost the
/// recording rather than the term. Drop the term instead; Settings says so
/// where the words are entered.
fn usable_keywords(vocabulary: &[String]) -> Vec<String> {
    vocabulary
        .iter()
        .map(|keyword| keyword.trim())
        .filter(|keyword| !keyword.is_empty() && !keyword.contains(['<', '>', '\r', '\n']))
        .map(str::to_string)
        .collect()
}

pub(super) fn action_instructions(
    action: &str,
    custom_prompt: Option<&str>,
    settings: &AppSettings,
) -> Result<String, Failure> {
    if action == "custom" {
        return custom_prompt
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                Failure::guidance(
                    "instructions_missing",
                    "A custom action requires instructions",
                )
            });
    }
    actions::instructions(action, &settings.action_overrides)
        .ok_or_else(|| Failure::guidance("unknown_action", format!("Unknown action: {action}")))
}

/// The user reads what OpenAI said; the log only ever gets the status.
async fn api_error(status: StatusCode, response: reqwest::Response) -> Failure {
    let fallback = format!("OpenAI request failed with HTTP {status}");
    let message = response
        .json::<ApiErrorEnvelope>()
        .await
        .ok()
        .and_then(|payload| payload.error)
        .and_then(|error| error.message)
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback);
    Failure::new("http", message).status(status.as_u16())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole cloud path for a real file with the stored key:
    /// `UTTERFORM_LIVE_AUDIO=<file> cargo test --lib live_openai -- --ignored --nocapture`.
    /// Costs a few cents per ten minutes of audio; never run by CI.
    #[tokio::test]
    #[ignore = "uploads audio to OpenAI with the stored key"]
    async fn live_openai_transcribes_an_audio_file() {
        let path = std::env::var("UTTERFORM_LIVE_AUDIO").expect("set UTTERFORM_LIVE_AUDIO");
        let decoded = crate::audio_file::decode(std::path::Path::new(&path), 1).unwrap();
        let settings = AppSettings {
            language_hints: vec!["de".into()],
            ..AppSettings::default()
        };
        let pcm = decoded.artifact.whisper_pcm().unwrap();
        for (index, part) in split_at_pauses(&pcm).iter().enumerate() {
            let began = std::time::Instant::now();
            let text = transcribe_part(part, &settings).await.unwrap();
            println!(
                "part {}: {} s audio, {} ms, {} characters: {text}",
                index + 1,
                part.len() / 16_000,
                began.elapsed().as_millis(),
                text.chars().count()
            );
        }
        let text = transcribe(&decoded.artifact, &settings).await.unwrap();
        assert!(!text.trim().is_empty());
    }

    #[test]
    fn a_microphone_recording_goes_up_whole() {
        let ten_minutes = vec![0.1; 10 * 60 * 16_000];
        assert_eq!(split_at_pauses(&ten_minutes).len(), 1);
        let twelve_minutes = vec![0.1; SINGLE_UPLOAD_SAMPLES];
        assert_eq!(split_at_pauses(&twelve_minutes).len(), 1);
    }

    #[test]
    fn a_long_file_is_cut_into_parts_that_each_fit_one_upload() {
        let half_hour = vec![0.1; 30 * 60 * 16_000 + 123];
        let parts = split_at_pauses(&half_hour);
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts.iter().map(|part| part.len()).sum::<usize>(),
            half_hour.len()
        );
        for part in &parts {
            assert!(part.len() <= SINGLE_UPLOAD_SAMPLES);
            assert!(wav_bytes(part).unwrap().len() as u64 <= MAX_UPLOAD_BYTES);
        }
    }

    #[test]
    fn a_cut_lands_in_the_pause_before_the_ten_minute_mark() {
        let mut audio = vec![0.4; 25 * 60 * 16_000];
        // A second of silence twenty seconds before the mark.
        let pause = PART_SAMPLES - 20 * 16_000;
        audio[pause..pause + 16_000].fill(0.0);
        let parts = split_at_pauses(&audio);
        let cut = parts[0].len();
        assert!(cut > pause && cut < pause + 16_000, "cut at {cut}");
        assert!(
            audio[cut - 400..cut + 400]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn plain_is_not_a_transform_action() {
        assert!(action_instructions("plain", None, &AppSettings::default()).is_err());
    }

    #[test]
    fn custom_requires_non_empty_instructions() {
        let settings = AppSettings::default();
        assert!(action_instructions("custom", Some("  "), &settings).is_err());
        assert_eq!(
            action_instructions("custom", Some("Use bullets"), &settings).unwrap(),
            "Use bullets"
        );
    }

    #[test]
    fn a_shipped_prompt_runs_until_the_user_replaces_it() {
        let mut settings = AppSettings::default();
        assert_eq!(
            action_instructions("email", None, &settings).unwrap(),
            actions::find("email").unwrap().prompt
        );

        settings.action_overrides.insert(
            "email".into(),
            actions::ActionOverride {
                name: None,
                prompt: Some("Answer as a postcard.".into()),
            },
        );
        assert_eq!(
            action_instructions("email", None, &settings).unwrap(),
            "Answer as a postcard."
        );
    }

    #[test]
    fn keywords_that_would_be_refused_are_dropped_rather_than_sent() {
        let vocabulary = [
            "  Careum  ",
            "",
            "   ",
            "a<b",
            "a>b",
            "two\nlines",
            "carriage\rreturn",
            "Utterform",
        ]
        .map(str::to_string);
        assert_eq!(
            usable_keywords(&vocabulary),
            vec!["Careum".to_string(), "Utterform".to_string()]
        );
    }
}
