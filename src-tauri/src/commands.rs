use tauri::{AppHandle, State};

use crate::{
    audio::{self, AudioCaptureState},
    domain::{
        AppSettings, AudioDeviceInfo, LocalModelInfo, ProcessRequest, ProcessResult,
        TranscriptionEngine,
    },
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
    audio::start_recording(&state, input_device.as_deref())
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
            warnings.push(format!(
                "Transformation failed; the plain transcript was used: {error}"
            ));
            transcript
        }
        Err(error) => return Err(error),
    };
    let delivery = output::deliver(&app, &text, &request, &current_settings)?;
    warnings.extend(delivery.warnings);

    Ok(ProcessResult {
        text,
        saved_path: delivery.saved_path,
        delivery_warnings: warnings,
        duration_ms,
        engine: current_settings.engine,
    })
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
