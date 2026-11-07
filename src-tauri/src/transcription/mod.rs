pub mod codex;
mod local;
mod openai;

use tauri::AppHandle;

use crate::{
    actions,
    audio::RecordingArtifact,
    diagnostics::Failure,
    domain::{AppSettings, TextProvider, TranscriptionEngine},
};

/// Most often a key pressed by accident or a recording stopped too soon: it is
/// not a fault, and there is nothing in the audio to try again with.
const NOTHING_HEARD: &str = "No speech was heard, so nothing was copied or typed.";

/// A failure's class and status are for the caller's one owning event; its
/// message is for the user alone.
pub async fn transcribe(
    app: &AppHandle,
    artifact: &RecordingArtifact,
    settings: &AppSettings,
) -> Result<String, Failure> {
    match settings.engine {
        TranscriptionEngine::OpenAi => openai::transcribe(artifact, settings).await,
        TranscriptionEngine::LocalWhisper => local::transcribe(app, artifact, settings).await,
    }
}

pub async fn transform(
    transcript: &str,
    action: &str,
    custom_prompt: Option<&str>,
    settings: &AppSettings,
) -> Result<String, Failure> {
    if action == actions::PLAIN {
        return Ok(transcript.trim().to_string());
    }
    match settings.text_provider {
        TextProvider::OpenAiKey => {
            openai::transform(transcript, action, custom_prompt, settings).await
        }
        // No fallback to the API key: a failure here is ChatGPT's to report.
        TextProvider::Codex => {
            let instructions = openai::action_instructions(action, custom_prompt, settings)?;
            codex::transform(&instructions, transcript, settings).await
        }
    }
}

/// The text model an action runs on, for diagnostic events.
pub fn text_model(settings: &AppSettings) -> String {
    match settings.text_provider {
        TextProvider::OpenAiKey => settings.text_model.clone(),
        TextProvider::Codex => codex::effective_model(settings),
    }
}
