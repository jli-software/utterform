mod local;
mod openai;

use tauri::AppHandle;

use crate::{
    actions,
    audio::RecordingArtifact,
    domain::{AppSettings, TranscriptionEngine},
};

pub async fn transcribe(
    app: &AppHandle,
    artifact: &RecordingArtifact,
    settings: &AppSettings,
) -> Result<String, String> {
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
) -> Result<String, String> {
    if action == actions::PLAIN {
        return Ok(transcript.trim().to_string());
    }
    openai::transform(transcript, action, custom_prompt, settings).await
}
