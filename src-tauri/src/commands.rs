use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    audio::{self, AudioCaptureState},
    domain::{
        AppSettings, AudioDeviceInfo, LocalModelInfo, ProcessRequest, ProcessResult,
        TranscriptionEngine,
    },
    feedback::{self, Cue},
    history::{self, HistoryEntry},
    models, output, secrets, settings, transcription,
};

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    settings::load(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, value: AppSettings) -> Result<(), String> {
    settings::save(&app, &value)
}

#[tauri::command]
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    audio::list_input_devices()
}

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: State<'_, AudioCaptureState>,
    input_device: Option<String>,
    engine: TranscriptionEngine,
    local_model_id: Option<String>,
    action: String,
) -> Result<(), String> {
    if engine == TranscriptionEngine::OpenAi || action != "plain" {
        secrets::openai_api_key()?;
    }
    if engine == TranscriptionEngine::LocalWhisper {
        let model_id = local_model_id
            .as_deref()
            .ok_or_else(|| "Select a local Whisper model in Settings".to_string())?;
        if !models::model_path(&app, model_id)?.is_file() {
            return Err(format!(
                "The {model_id} Whisper model is not downloaded. Open Settings to download it."
            ));
        }
    }
    let current_settings = settings::load(&app)?;
    let session = audio::start_recording(
        &state,
        input_device.as_deref(),
        current_settings.sound_enabled,
    )?;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            match audio::check_limit(&app.state::<AudioCaptureState>(), session) {
                Ok(audio::LimitCheck::Waiting) => {}
                Ok(audio::LimitCheck::Stopped) => {
                    let _ = app.emit("recording-limit-reached", ());
                    break;
                }
                _ => break,
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn get_recording_status(
    state: State<'_, AudioCaptureState>,
) -> Result<audio::RecordingStatus, String> {
    audio::status(&state)
}

#[tauri::command]
pub fn set_recording_paused(
    state: State<'_, AudioCaptureState>,
    paused: bool,
) -> Result<audio::RecordingStatus, String> {
    audio::set_paused(&state, paused)
}

#[tauri::command]
pub fn cancel_recording(state: State<'_, AudioCaptureState>) -> Result<(), String> {
    audio::cancel_recording(&state)
}

#[tauri::command]
pub async fn finish_recording(
    app: AppHandle,
    state: State<'_, AudioCaptureState>,
    request: ProcessRequest,
) -> Result<ProcessResult, String> {
    let current_settings = settings::load(&app)?;
    let artifact = audio::stop_recording(&state)?;
    let duration_ms = artifact.duration_ms();
    let transcript = transcription::transcribe(&app, &artifact, &current_settings).await?;
    let mut warnings = Vec::new();
    let mut transformation_succeeded = true;
    let text = match transcription::transform(
        &transcript,
        &request.action,
        request.custom_prompt.as_deref(),
        &current_settings,
    )
    .await
    {
        Ok(text) => text,
        Err(error) if request.action != "plain" => {
            transformation_succeeded = false;
            warnings.push(format!(
                "Transformation failed; the plain transcript was used: {error}"
            ));
            transcript
        }
        Err(error) => return Err(error),
    };
    // Persist before external delivery, so clipboard/file failures cannot lose the text.
    let history_entry = if current_settings.history_enabled {
        let entry = HistoryEntry::new(&text, duration_ms, current_settings.engine.clone());
        match history::append(&app, entry.clone()) {
            Ok(()) => Some(entry),
            Err(error) => {
                warnings.push(format!("History was not saved: {error}"));
                None
            }
        }
    } else {
        None
    };
    let delivery = output::deliver(&app, &text, &request, &current_settings)?;
    // Stop only confirms capture ended. Done confirms the requested transformation and
    // every output completed, even when the window is hidden. History is best-effort.
    if current_settings.sound_enabled
        && transformation_succeeded
        && delivery.all_requested_outputs_succeeded(&request)
    {
        // Do not block the async executor while the native output buffer drains.
        let _ = tauri::async_runtime::spawn_blocking(|| feedback::play(Cue::Done)).await;
    }
    warnings.extend(delivery.warnings);

    Ok(ProcessResult {
        history_entry,
        text,
        saved_path: delivery.saved_path,
        copied_to_clipboard: delivery.copied_to_clipboard,
        delivery_warnings: warnings,
        duration_ms,
        engine: current_settings.engine,
    })
}

#[tauri::command]
pub fn list_history(app: AppHandle) -> Result<Vec<HistoryEntry>, String> {
    history::list(&app)
}

#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    history::clear(&app)
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|error| format!("Could not copy text: {error}"))
}

#[tauri::command]
pub fn has_openai_api_key() -> bool {
    secrets::has_openai_api_key()
}

#[tauri::command]
pub fn set_openai_api_key(api_key: String) -> Result<(), String> {
    secrets::set_openai_api_key(&api_key)
}

#[tauri::command]
pub fn delete_openai_api_key() -> Result<(), String> {
    secrets::delete_openai_api_key()
}

#[tauri::command]
pub fn list_local_models(app: AppHandle) -> Result<Vec<LocalModelInfo>, String> {
    models::list(&app)
}

#[tauri::command]
pub async fn download_local_model(app: AppHandle, model_id: String) -> Result<(), String> {
    models::download(&app, &model_id).await
}

#[tauri::command]
pub async fn delete_local_model(app: AppHandle, model_id: String) -> Result<(), String> {
    models::delete(&app, &model_id).await
}
