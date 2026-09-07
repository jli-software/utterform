use tauri::AppHandle;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{audio::RecordingArtifact, domain::AppSettings, models};

pub async fn transcribe(
    app: &AppHandle,
    artifact: &RecordingArtifact,
    settings: &AppSettings,
) -> Result<String, String> {
    let model_id = settings
        .local_model_id
        .as_deref()
        .ok_or_else(|| "Select a local Whisper model in Settings".to_string())?;
    let model_path = models::model_path(app, model_id)?;
    if !model_path.exists() {
        return Err(format!(
            "The {model_id} Whisper model is not downloaded. Open Settings to download it."
        ));
    }
    let audio = artifact.whisper_pcm()?;
    if audio.is_empty() {
        return Err("The recording contains no audio samples".into());
    }
    let language = settings
        .language_hints
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());
    // Whisper has no keyword field; the documented way to bias it towards a
    // spelling is to put the words in the prompt it starts from. The same
    // vocabulary therefore helps whichever engine is selected.
    let vocabulary = initial_prompt(&settings.vocabulary);

    tokio::task::spawn_blocking(move || {
        let context = WhisperContext::new_with_params(
            model_path.to_string_lossy().as_ref(),
            WhisperContextParameters::default(),
        )
        .map_err(|error| format!("Could not load the Whisper model: {error}"))?;
        let mut state = context
            .create_state()
            .map_err(|error| format!("Could not initialize Whisper: {error}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_language(language.as_deref());
        if let Some(prompt) = vocabulary.as_deref() {
            params.set_initial_prompt(prompt);
        }
        state
            .full(params, &audio)
            .map_err(|error| format!("Local Whisper transcription failed: {error}"))?;
        let transcript = state
            .as_iter()
            .map(|segment| segment.to_string())
            .collect::<Vec<_>>()
            .join("");
        if transcript.trim().is_empty() {
            return Err("Local Whisper returned an empty transcription".into());
        }
        Ok(transcript.trim().to_string())
    })
    .await
    .map_err(|error| format!("Local Whisper worker failed: {error}"))?
}

/// The vocabulary as a sentence Whisper can start from. Whisper takes prior
/// text rather than a keyword list, and a comma-separated run of the words is
/// the documented way to bias its spelling.
fn initial_prompt(vocabulary: &[String]) -> Option<String> {
    let terms = vocabulary
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_vocabulary_leaves_whisper_alone() {
        assert_eq!(initial_prompt(&[]), None);
        assert_eq!(initial_prompt(&["  ".to_string()]), None);
    }

    #[test]
    fn the_words_reach_whisper_as_prior_text() {
        let vocabulary = ["Careum".to_string(), "  ".into(), " Utterform ".into()];
        assert_eq!(initial_prompt(&vocabulary).unwrap(), "Careum, Utterform");
    }
}
