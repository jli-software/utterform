mod local;
mod openai;

use tauri::AppHandle;

use crate::{
    actions,
    audio::RecordingArtifact,
    diagnostics::Failure,
    domain::{AppSettings, TranscriptionEngine},
};

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
    openai::transform(transcript, action, custom_prompt, settings).await
}
