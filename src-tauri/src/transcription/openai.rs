use std::{io::Cursor, time::Duration};

use reqwest::{Client, StatusCode, multipart};
use serde::Deserialize;

use crate::{actions, audio::RecordingArtifact, domain::AppSettings, secrets};

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

    let part = multipart::Part::bytes(audio)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|error| format!("Could not prepare the recording: {error}"))?;
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
        .map_err(|error| format!("OpenAI transcription request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(api_error(status, response).await);
    }
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
        .map_err(|error| format!("OpenAI text transformation failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(api_error(status, response).await);
    }
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

fn action_instructions(
    action: &str,
    custom_prompt: Option<&str>,
    settings: &AppSettings,
) -> Result<String, String> {
    if action == "custom" {
        return custom_prompt
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "A custom action requires instructions".to_string());
    }
    actions::instructions(action, &settings.action_overrides)
        .ok_or_else(|| format!("Unknown action: {action}"))
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
