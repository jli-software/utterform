use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use crate::{
    actions::{self, BuiltInAction},
    audio::{self, AudioCaptureState},
    diagnostics,
    domain::{
        AppSettings, AudioDeviceInfo, LocalModelInfo, ProcessRequest, ProcessResult,
        TranscriptionEngine,
    },
    feedback::{self, Cue},
    history::{self, HistoryEntry},
    live, models, output, secrets, settings, transcription, tray,
};

/// The interface asks once it can act; an intent is delivered to one caller only.
#[tauri::command]
pub fn take_startup_intent(state: State<'_, crate::StartupIntent>) -> Option<crate::cli::Intent> {
    state.take()
}

/// Whether this session hands global shortcuts to applications at all, so
/// Settings can explain a Wayland session instead of showing a dead field.
#[tauri::command]
pub fn global_hotkey_support(app: AppHandle) -> crate::hotkey::Support {
    crate::hotkey::support(&app)
}

/// Put a changed dictation key into effect. Separate from saving settings so
/// the shortcut is only re-registered when the user actually changed it, and
/// so a key another application holds is reported where it was entered.
#[tauri::command]
pub async fn apply_global_hotkey(app: AppHandle, shortcut: Option<String>) -> Result<(), String> {
    // Off the main thread on purpose: registering asks the event loop to do it
    // and waits for the answer, which the main thread cannot give itself.
    tauri::async_runtime::spawn_blocking(move || crate::hotkey::apply(&app, shortcut.as_deref()))
        .await
        .map_err(|error| format!("Could not reach the shortcut manager: {error}"))?
}

/// The prompts Utterform ships with, so Settings can show and reset them
/// without keeping a second copy of the text that would drift out of step.
#[tauri::command]
pub fn list_built_in_actions() -> &'static [BuiltInAction] {
    actions::BUILT_INS
}

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

/// Open the microphone and play the start cue.
///
/// Asynchronous so the work leaves the main thread: a synchronous command runs
/// on the thread that owns the window, and opening an audio device there
/// stalls the interface for as long as the device takes — and on Windows put
/// the start cue's stream on the WebView2 thread, the one place it should not
/// have been. The interface learns the recording began as soon as the
/// microphone is open, not once the cue has been heard.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    input_device: Option<String>,
    engine: TranscriptionEngine,
    local_model_id: Option<String>,
    action: String,
) -> Result<(), String> {
    let current_settings = settings::load(&app)?;
    if app.state::<live::LiveState>().active() {
        return Err("A live recording is already active".into());
    }
    if engine == TranscriptionEngine::OpenAi && live::selected(&current_settings) {
        return live::start(app, current_settings, input_device).await;
    }
    tauri::async_runtime::spawn_blocking(move || {
        begin_recording(
            &app,
            input_device.as_deref(),
            engine,
            local_model_id,
            &action,
        )
    })
    .await
    .map_err(|error| format!("Could not start the recording: {error}"))?
}

fn begin_recording(
    app: &AppHandle,
    input_device: Option<&str>,
    engine: TranscriptionEngine,
    local_model_id: Option<String>,
    action: &str,
) -> Result<(), String> {
    if engine == TranscriptionEngine::OpenAi || action != actions::PLAIN {
        secrets::openai_api_key()?;
    }
    if engine == TranscriptionEngine::LocalWhisper {
        let model_id = local_model_id
            .as_deref()
            .ok_or_else(|| "Select a local Whisper model in Settings".to_string())?;
        if !models::model_path(app, model_id)?.is_file() {
            return Err(format!(
                "The {model_id} Whisper model is not downloaded. Open Settings to download it."
            ));
        }
    }
    let current_settings = settings::load(app)?;
    let started = audio::start_recording(
        &app.state::<AudioCaptureState>(),
        input_device,
        current_settings.sound_enabled,
    )?;
    let session = started.session;
    tray::set_recording(app, true);
    // Off this thread too: the wait is a speaker resuming from idle, which a
    // Bluetooth headset can take a few hundred milliseconds over, and the
    // interface should hear "recording" before that.
    let cued = app.clone();
    std::thread::spawn(move || {
        if let Err(reason) = started.arm(&cued.state::<AudioCaptureState>()) {
            announce_recording(&cued, &reason);
        }
    });
    let watched = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            match audio::check_limit(&watched.state::<AudioCaptureState>(), session) {
                Ok(audio::LimitCheck::Waiting) => {}
                Ok(audio::LimitCheck::Stopped) => {
                    tray::set_recording(&watched, false);
                    let _ = watched.emit("recording-limit-reached", ());
                    break;
                }
                _ => break,
            }
        }
    });
    Ok(())
}

/// Stand in for a start cue that could not be played. With the window hidden
/// the cue is the user's only sign that the microphone is live, so its silence
/// has to be replaced rather than merely logged.
pub(crate) fn announce_recording(app: &AppHandle, reason: &str) {
    diagnostics::log(format!(
        "the start sound could not be played, raising a notification instead: {reason}"
    ));
    let _ = app
        .notification()
        .builder()
        .title("Utterform is recording")
        .body("The start sound could not be played on this output device.")
        .show();
}

/// Play the three cues the way a hotkey recording plays them — on their own
/// threads, from a window that need not be visible — after a pause long enough
/// to put the window in the background first. The outcome comes back on the
/// `test-cues-finished` event and in the log, so a machine where the start
/// click is silent can say whether the cue path or the microphone is at fault.
#[tauri::command]
pub fn play_test_cues(app: AppHandle, delay_seconds: u32) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(u64::from(
            delay_seconds.min(60),
        )));
        diagnostics::log("playing the test cues");
        let mut lines = Vec::new();
        for (cue, label) in [
            (Cue::Start, "Start"),
            (Cue::Stop, "Stop"),
            (Cue::Done, "Done"),
        ] {
            lines.push(match feedback::play(cue) {
                Ok(report) => format!("{label}: {report}"),
                Err(reason) => format!("{label}: not played — {reason}"),
            });
        }
        let _ = app.emit("test-cues-finished", lines.join("\n"));
    });
}

/// Where the log is written, for Settings to show next to the test button.
#[tauri::command]
pub fn diagnostics_log_path() -> Option<String> {
    diagnostics::path().map(|path| path.display().to_string())
}

#[tauri::command]
pub fn get_recording_status(
    state: State<'_, AudioCaptureState>,
) -> Result<audio::RecordingStatus, String> {
    audio::status(&state)
}

#[tauri::command]
pub fn get_live_status(state: State<'_, live::LiveState>) -> Option<live::LiveStatus> {
    state.status()
}

#[tauri::command]
pub fn live_support() -> live::Support {
    live::support()
}

#[tauri::command]
pub fn set_recording_paused(
    state: State<'_, AudioCaptureState>,
    paused: bool,
) -> Result<audio::RecordingStatus, String> {
    audio::set_paused(&state, paused)
}

#[tauri::command]
pub async fn cancel_recording(
    app: AppHandle,
    state: State<'_, AudioCaptureState>,
) -> Result<(), String> {
    live::cancel(&app).await?;
    tray::set_recording(&app, false);
    audio::cancel_recording(&state)
}

#[tauri::command]
pub async fn finish_recording(
    app: AppHandle,
    state: State<'_, AudioCaptureState>,
    request: ProcessRequest,
) -> Result<ProcessResult, String> {
    if app.state::<live::LiveState>().active() {
        return live::finish(app, request).await;
    }
    let current_settings = settings::load(&app)?;
    // Capture is over either way; the icon must not keep claiming otherwise.
    let artifact = audio::stop_recording(&state);
    tray::set_recording(&app, false);
    let artifact = artifact?;
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
        Err(error) if request.action != actions::PLAIN => {
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
        typed_at_cursor: delivery.typed_at_cursor,
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
