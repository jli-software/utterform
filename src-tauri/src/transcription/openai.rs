use std::{io::Cursor, time::Duration};

use reqwest::{Client, Response, StatusCode, multipart};
use serde::Deserialize;

use crate::{audio::RecordingArtifact, domain::AppSettings, secrets};

const TRANSCRIPTIONS_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const MAX_UPLOAD_BYTES: u64 = 25 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// A ten-minute recording is a ~19 MB upload that the model then has to work
// through, so a two-minute cap on the whole exchange cut off legitimate long
// clips. Bound inactivity instead, and keep a generous overall ceiling so a
// dead connection cannot hold the UI in "Transcribing…" forever.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_ATTEMPTS: u32 = 3;
const MAX_BACKOFF: Duration = Duration::from_secs(20);

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
) -> Result<String, String> {
    let audio = normalized_wav(artifact)?;
    if audio.len() as u64 > MAX_UPLOAD_BYTES {
        return Err("The recording exceeds the 25 MB OpenAI upload limit".into());
    }

    // Read once and build once: a retry must not repeat the keyring lookup.
    let key = secrets::openai_api_key()?;
    let client = client()?;
    let languages: Vec<String> = settings
        .language_hints
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();

    let response = send_retrying(
        || {
            // The body is consumed by sending, so each attempt needs its own.
            let part = multipart::Part::bytes(audio.clone())
                .file_name("recording.wav")
                .mime_str("audio/wav")
                .map_err(|error| format!("Could not prepare the recording: {error}"))?;
            let mut form = multipart::Form::new()
                .text("model", "gpt-transcribe")
                .part("file", part);
            for language in &languages {
                form = form.text("languages[]", language.clone());
            }
            Ok(client
                .post(TRANSCRIPTIONS_URL)
                .bearer_auth(&key)
                .multipart(form))
        },
        "OpenAI transcription request",
    )
    .await?;
    let payload = response
        .json::<TranscriptionResponse>()
        .await
        .map_err(|error| format!("OpenAI returned an invalid transcription: {error}"))?;
    let text = payload.text.trim();
    if text.is_empty() {
        return Err("OpenAI returned an empty transcription".into());
    }
    Ok(text.to_string())
}

pub async fn transform(
    transcript: &str,
    action: &str,
    custom_prompt: Option<&str>,
    settings: &AppSettings,
) -> Result<String, String> {
    let instructions = action_instructions(action, custom_prompt)?;
    let body = serde_json::json!({
        "model": settings.text_model,
        "instructions": instructions,
        "input": transcript,
        "store": false
    });
    let key = secrets::openai_api_key()?;
    let client = client()?;
    let response = send_retrying(
        || Ok(client.post(RESPONSES_URL).bearer_auth(&key).json(&body)),
        "OpenAI text transformation",
    )
    .await?;
    let payload = response
        .json::<ResponsesResponse>()
        .await
        .map_err(|error| format!("OpenAI returned an invalid transformation: {error}"))?;
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
        .ok_or_else(|| "OpenAI returned an empty transformation".to_string())?;
    Ok(text.trim().to_string())
}

/// Send, and try again when trying again can plausibly help. The request is
/// rebuilt per attempt because sending consumes the body.
async fn send_retrying(
    mut request: impl FnMut() -> Result<reqwest::RequestBuilder, String>,
    what: &str,
) -> Result<Response, String> {
    let mut attempt = 0;
    loop {
        let outcome = request()?.send().await;
        if let Ok(response) = &outcome
            && response.status().is_success()
        {
            return Ok(outcome.expect("a successful status came from a response"));
        }
        let status = outcome.as_ref().ok().map(Response::status);
        let hint = outcome.as_ref().ok().and_then(retry_after_header);
        let Some(delay) = retry_delay(status, hint, attempt) else {
            return match outcome {
                Ok(response) => Err(api_error(response.status(), response).await),
                Err(error) => Err(format!("{what} failed: {error}")),
            };
        };
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

/// How long to wait before another attempt, or `None` when repeating cannot
/// help. A rejected key or a malformed request fails identically every time;
/// rate limits, server faults and dropped connections do not.
fn retry_delay(
    status: Option<StatusCode>,
    server_hint: Option<Duration>,
    attempt: u32,
) -> Option<Duration> {
    if attempt + 1 >= MAX_ATTEMPTS {
        return None;
    }
    let worth_repeating = match status {
        // No status at all means the exchange never completed: a dropped
        // connection, a timeout, a DNS hiccup.
        None => true,
        Some(code) => code == StatusCode::TOO_MANY_REQUESTS || code.is_server_error(),
    };
    if !worth_repeating {
        return None;
    }
    // The server's own advice wins over guessing, but never unboundedly.
    Some(
        server_hint
            .unwrap_or_else(|| Duration::from_secs(2u64.pow(attempt)))
            .min(MAX_BACKOFF),
    )
}

fn retry_after_header(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("Could not initialize the OpenAI client: {error}"))
}

fn normalized_wav(artifact: &RecordingArtifact) -> Result<Vec<u8>, String> {
    let samples = artifact.whisper_pcm()?;
    let mut cursor = Cursor::new(Vec::new());
    {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|error| format!("Could not prepare the recording upload: {error}"))?;
        for sample in samples {
            let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            writer
                .write_sample(pcm)
                .map_err(|error| format!("Could not prepare the recording upload: {error}"))?;
        }
        writer
            .finalize()
            .map_err(|error| format!("Could not finalize the recording upload: {error}"))?;
    }
    Ok(cursor.into_inner())
}

fn action_instructions(action: &str, custom_prompt: Option<&str>) -> Result<String, String> {
    match action {
        "clean" => Ok("Correct punctuation, capitalization, spelling, and paragraph breaks. Preserve the speaker's language, wording, intent, names, numbers, and level of detail. Remove only obvious filler words. Return only the corrected text.".into()),
        "polish" => Ok("Rewrite the transcript into clear, fluent prose in the speaker's language. Preserve every material fact, requirement, name, number, and the original intent. Do not add information. Return only the polished text.".into()),
        "summarize" => Ok("Summarize the transcript concisely in the speaker's language. Retain decisions, requirements, action items, names, numbers, and caveats. Use short paragraphs or bullets when helpful. Return only the summary.".into()),
        "prompt" => Ok("Convert the transcript into a precise, self-contained prompt for an AI assistant. Preserve all requirements, constraints, examples, and desired output. Remove conversational filler and resolve only unambiguous references. Return only the prompt.".into()),
        "custom" => custom_prompt
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "A custom action requires instructions".to_string()),
        _ => Err(format!("Unknown action: {action}")),
    }
}

async fn api_error(status: StatusCode, response: reqwest::Response) -> String {
    let fallback = format!("OpenAI request failed with HTTP {status}");
    response
        .json::<ApiErrorEnvelope>()
        .await
        .ok()
        .and_then(|payload| payload.error)
        .and_then(|error| error.message)
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_and_server_faults_are_worth_repeating() {
        for code in [
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            assert!(retry_delay(Some(code), None, 0).is_some(), "{code}");
        }
        // A connection that never produced a status is worth another attempt.
        assert!(retry_delay(None, None, 0).is_some());
    }

    #[test]
    fn a_rejected_request_is_not_repeated() {
        for code in [
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::BAD_REQUEST,
            StatusCode::PAYLOAD_TOO_LARGE,
        ] {
            assert_eq!(retry_delay(Some(code), None, 0), None, "{code}");
        }
    }

    #[test]
    fn attempts_are_bounded_and_back_off() {
        let first = retry_delay(None, None, 0).unwrap();
        let second = retry_delay(None, None, 1).unwrap();
        assert!(second > first, "the wait must grow");
        assert_eq!(retry_delay(None, None, MAX_ATTEMPTS - 1), None);
    }

    #[test]
    fn the_servers_own_advice_wins_but_stays_bounded() {
        assert_eq!(
            retry_delay(
                Some(StatusCode::TOO_MANY_REQUESTS),
                Some(Duration::from_secs(7)),
                0
            ),
            Some(Duration::from_secs(7))
        );
        assert_eq!(
            retry_delay(
                Some(StatusCode::TOO_MANY_REQUESTS),
                Some(Duration::from_secs(3600)),
                0
            ),
            Some(MAX_BACKOFF)
        );
    }

    #[test]
    fn plain_is_not_a_transform_action() {
        assert!(action_instructions("plain", None).is_err());
    }

    #[test]
    fn custom_requires_non_empty_instructions() {
        assert!(action_instructions("custom", Some("  ")).is_err());
        assert_eq!(
            action_instructions("custom", Some("Use bullets")).unwrap(),
            "Use bullets"
        );
    }
}
