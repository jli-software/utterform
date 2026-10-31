use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use crate::{
    actions::{self, BuiltInAction},
    activation::reveal_main_window,
    audio::{self, AudioCaptureState, RecordingArtifact},
    diagnostics::{self, Failure},
    domain::{
        AppSettings, AudioDeviceInfo, LocalModelInfo, ProcessRequest, ProcessResult,
        TranscriptionEngine,
    },
    feedback::{self, Cue},
    history::{self, HistoryEntry},
    live, models, output,
    retained::{self, Retained, RetainedRecordings},
    secrets, settings, transcription, tray,
};

/// How long a dropped connection gets to come back before the one retry.
const TRANSIENT_RETRY_DELAY: Duration = Duration::from_secs(2);
/// Desktop notifications truncate or refuse long bodies on their own terms.
const NOTIFICATION_LIMIT: usize = 240;

/// The interface asks once it can act; an intent is delivered to one caller only.
#[tauri::command]
pub fn take_startup_intent(state: State<'_, crate::StartupIntent>) -> Option<crate::cli::Intent> {
    state.take()
}

/// Read the operating system's startup registration. Windows validates the
/// exact quoted command and Explorer's approval state rather than treating any
/// registry value as a working entry.
#[tauri::command]
pub fn autostart_enabled(app: AppHandle) -> Result<bool, String> {
    crate::autostart::enabled(&app)
        .inspect(|&enabled| {
            diagnostics::info!("autostart.read", enabled = enabled);
        })
        .map_err(|failure| diagnostics::failure!("autostart.read_failed", &failure))
}

#[tauri::command]
pub fn enable_autostart(app: AppHandle) -> Result<(), String> {
    crate::autostart::enable(&app)
        .map(|()| diagnostics::info!("autostart.enabled"))
        .map_err(|failure| diagnostics::failure!("autostart.enable_failed", &failure))
}

#[tauri::command]
pub fn disable_autostart(app: AppHandle) -> Result<(), String> {
    crate::autostart::disable(&app)
        .map(|()| diagnostics::info!("autostart.disabled"))
        .map_err(|failure| diagnostics::failure!("autostart.disable_failed", &failure))
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
    tauri::async_runtime::spawn_blocking(move || {
        crate::hotkey::apply(&app, shortcut.as_deref(), "settings")
    })
    .await
    .map_err(|error| {
        diagnostics::failure!(
            "hotkey.apply_failed",
            &Failure::new(
                "worker",
                format!("Could not reach the shortcut manager: {error}")
            ),
            during = "settings"
        )
    })?
}

/// The prompts Utterform ships with, so Settings can show and reset them
/// without keeping a second copy of the text that would drift out of step.
#[tauri::command]
pub fn list_built_in_actions() -> &'static [BuiltInAction] {
    actions::BUILT_INS
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    settings::load(&app).map_err(|failure| {
        diagnostics::failure!("settings.load_failed", &failure, during = "get_settings")
    })
}

/// The modes a saved configuration runs in are diagnostic metadata; its
/// paths, prompts, vocabulary and context are not, and are never logged.
#[tauri::command]
pub fn save_settings(app: AppHandle, value: AppSettings) -> Result<(), String> {
    match settings::save(&app, &value) {
        Ok(()) => {
            diagnostics::info!(
                "settings.saved",
                engine = value.engine.as_str(),
                cloud_model = value.cloud_model.as_str(),
                typing_method = value.typing_method.as_str(),
                clipboard = value.copy_to_clipboard,
                file = value.save_to_file,
                cursor = value.type_at_cursor,
                history = value.history_enabled,
                sound = value.sound_enabled,
                hotkey = value.global_hotkey.is_some(),
            );
            Ok(())
        }
        Err(failure) => Err(diagnostics::failure!("settings.save_failed", &failure)),
    }
}

#[tauri::command]
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    audio::list_input_devices()
        .map_err(|failure| diagnostics::failure!("audio.devices_failed", &failure))
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
    let current_settings = settings::load(&app).map_err(|failure| {
        diagnostics::failure!("settings.load_failed", &failure, during = "recording.start")
    })?;
    if app.state::<live::LiveState>().active() {
        return Err(diagnostics::failure!(
            "recording.start_failed",
            &Failure::guidance("live_active", "A live recording is already active")
        ));
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
    .map_err(|error| {
        diagnostics::failure!(
            "recording.start_failed",
            &Failure::new("worker", format!("Could not start the recording: {error}"))
        )
    })?
}

/// The text model as an event may name it. The field is typed by the user, so
/// anything that does not look like a model id is only `custom`.
fn text_model_name(model: &str) -> &str {
    let plausible = !model.is_empty()
        && model.len() <= 64
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    if plausible { model } else { "custom" }
}

/// The model a transcription runs on, as an event may name it.
fn transcription_model(engine: &TranscriptionEngine, local_model_id: Option<&str>) -> &'static str {
    match engine {
        TranscriptionEngine::OpenAi => "gpt-transcribe",
        TranscriptionEngine::LocalWhisper => local_model_id.map_or("none", models::diagnostic_id),
    }
}

fn begin_recording(
    app: &AppHandle,
    input_device: Option<&str>,
    engine: TranscriptionEngine,
    local_model_id: Option<String>,
    action: &str,
) -> Result<(), String> {
    let recording = diagnostics::next_recording_id();
    diagnostics::info!(
        "recording.requested",
        recording = recording,
        mode = "batch",
        engine = engine.as_str(),
        model = transcription_model(&engine, local_model_id.as_deref()),
        action = actions::diagnostic_id(action),
    );
    let rejected = |failure: Failure| {
        diagnostics::failure!(
            "recording.start_failed",
            &failure,
            recording = recording,
            mode = "batch"
        )
    };
    if engine == TranscriptionEngine::OpenAi || action != actions::PLAIN {
        // A missing key stops here, while nothing has been said yet. A keyring
        // that cannot answer does not: the recording is kept if it still
        // cannot answer afterwards, and the words survive the outage.
        match secrets::openai_api_key() {
            Err(failure) if failure.class() == "keyring" => {
                diagnostics::warning!(
                    "recording.key_unavailable",
                    recording = recording,
                    class = failure.class()
                );
            }
            Err(failure) => return Err(rejected(failure)),
            Ok(_) => {}
        }
    }
    if engine == TranscriptionEngine::LocalWhisper {
        let model_id = local_model_id.as_deref().ok_or_else(|| {
            rejected(Failure::guidance(
                "model_not_selected",
                "Select a local Whisper model in Settings",
            ))
        })?;
        if !models::model_path(app, model_id)
            .map_err(rejected)?
            .is_file()
        {
            return Err(rejected(Failure::guidance(
                "model_missing",
                format!(
                    "The {model_id} Whisper model is not downloaded. Open Settings to download it."
                ),
            )));
        }
    }
    let current_settings = settings::load(app).map_err(|failure| {
        diagnostics::failure!(
            "settings.load_failed",
            &failure,
            during = "recording.start",
            recording = recording
        )
    })?;
    // The previous recording's Done cue ends here if it is still sounding,
    // before the start cue is scheduled: a chime finishing inside the next
    // dictation would confirm the wrong recording, and its tail must not be
    // what the microphone hears once the start cue has armed it.
    app.state::<feedback::DoneCues>().cancel();
    let started = audio::start_recording(
        &app.state::<AudioCaptureState>(),
        input_device,
        current_settings.sound_enabled,
        recording,
    )
    .map_err(rejected)?;
    let session = started.session;
    tray::set_recording(app, true, recording);
    // Off this thread too: the wait is a speaker resuming from idle, which a
    // Bluetooth headset can take a few hundred milliseconds over, and the
    // interface should hear "recording" before that.
    let cued = app.clone();
    std::thread::spawn(move || {
        if started.arm(&cued.state::<AudioCaptureState>()).is_err() {
            announce_recording(&cued, recording);
        }
    });
    let watched = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            match audio::check_limit(&watched.state::<AudioCaptureState>(), session) {
                Ok(audio::LimitCheck::Waiting) => {}
                Ok(audio::LimitCheck::Stopped) => {
                    tray::set_recording(&watched, false, recording);
                    diagnostics::notify_interface(&watched, "recording-limit-reached", ());
                    break;
                }
                Ok(audio::LimitCheck::Gone) => break,
                Err(failure) => {
                    diagnostics::warning!(
                        "recording.watchdog_failed",
                        recording = recording,
                        class = failure.class()
                    );
                    break;
                }
            }
        }
    });
    Ok(())
}

/// Stand in for a start cue that could not be played. With the window hidden
/// the cue is the user's only sign that the microphone is live, so its silence
/// has to be replaced rather than merely logged. Why the cue stayed silent is
/// already in its own `cue.not_played` event.
pub(crate) fn announce_recording(app: &AppHandle, recording: u64) {
    let shown = app
        .notification()
        .builder()
        .title("Utterform is recording")
        .body("The start sound could not be played on this output device.")
        .show();
    match shown {
        Ok(()) => diagnostics::warning!("cue.replaced_by_notification", recording = recording),
        Err(_) => diagnostics::warning!("cue.notification_failed", recording = recording),
    }
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
        diagnostics::info!("cue.test_started");
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
        diagnostics::notify_interface(&app, "test-cues-finished", lines.join("\n"));
    });
}

/// Where the log is written, for Settings → General → Support & diagnostics.
/// A query: nothing is logged for asking.
#[tauri::command]
pub fn diagnostics_info() -> diagnostics::Info {
    diagnostics::support_info()
}

/// Copy the bounded diagnostic report to the clipboard. Takes no path: the
/// backend alone decides which log files are read.
#[tauri::command]
pub async fn copy_diagnostics(app: AppHandle) -> Result<diagnostics::Copied, String> {
    diagnostics::copy_report(&app).await
}

/// Open the current log file in the system's default application.
#[tauri::command]
pub async fn open_log_file(app: AppHandle) -> Result<(), String> {
    diagnostics::open_log_file(&app).await
}

/// Reveal the current log file in the file manager.
#[tauri::command]
pub async fn open_log_folder(app: AppHandle) -> Result<(), String> {
    diagnostics::open_log_folder(&app).await
}

/// A failure only the interface saw. Returns the reference to show beside it,
/// or nothing once this run's allowance of reports is spent.
#[tauri::command]
pub fn report_frontend_error(report: diagnostics::FrontendError) -> Option<String> {
    diagnostics::record_frontend_error(&report)
}

/// Set once the status poll has reported its failure: it asks ten times a
/// second, and every later failure is the same one.
static STATUS_FAILURE_REPORTED: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn get_recording_status(
    state: State<'_, AudioCaptureState>,
) -> Result<audio::RecordingStatus, String> {
    audio::status(&state).map_err(|failure| {
        if STATUS_FAILURE_REPORTED.swap(true, Ordering::Relaxed) {
            failure.into_message()
        } else {
            diagnostics::fallback!("recording.status_failed", &failure)
        }
    })
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
pub fn typing_support() -> crate::typing::Support {
    crate::typing::support()
}

#[tauri::command]
pub fn set_recording_paused(
    state: State<'_, AudioCaptureState>,
    paused: bool,
) -> Result<audio::RecordingStatus, String> {
    audio::set_paused(&state, paused).map_err(|failure| {
        diagnostics::failure!("recording.pause_failed", &failure, paused = paused)
    })
}

#[tauri::command]
pub async fn cancel_recording(
    app: AppHandle,
    state: State<'_, AudioCaptureState>,
) -> Result<(), String> {
    live::cancel(&app).await?;
    tray::clear(&app);
    audio::cancel_recording(&state)
        .map_err(|failure| diagnostics::failure!("recording.cancel_failed", &failure))
}

/// Record → Transcribe → Transform → Deliver, as ordered events that share
/// the recording's id: enough to reconstruct a run without any of its text.
#[tauri::command]
pub async fn finish_recording(
    app: AppHandle,
    state: State<'_, AudioCaptureState>,
    request: ProcessRequest,
) -> Result<ProcessResult, String> {
    if app.state::<live::LiveState>().active() {
        return live::finish(app, request).await;
    }
    let began = Instant::now();
    let current_settings = settings::load(&app).map_err(|failure| {
        diagnostics::failure!(
            "settings.load_failed",
            &failure,
            during = "recording.finish"
        )
    })?;
    // Capture is over either way; the icon must not keep claiming otherwise.
    let artifact = audio::stop_recording(&state);
    match &artifact {
        Ok(artifact) => tray::set_recording(&app, false, artifact.recording),
        Err(_) => tray::clear(&app),
    }
    let artifact = artifact.map_err(|failure| {
        diagnostics::failure!("recording.failed", &failure, stage = "capture")
    })?;
    process_recording(&app, artifact, request, current_settings, began).await
}

/// Worth one quiet second attempt: the connection dropped or the service said
/// to come back. A timeout is not, having already waited minutes.
fn is_transient(failure: &Failure) -> bool {
    matches!(failure.class(), "connect" | "transport" | "body")
        || failure
            .http_status()
            .is_some_and(|status| status == 429 || status >= 500)
}

/// Nothing in the audio to retry with. Everything else — no network, a key to
/// fix, an account out of credit — leaves words worth keeping.
fn worth_keeping(failure: &Failure) -> bool {
    !matches!(failure.class(), "empty_result" | "empty_recording")
}

async fn transcribe_with_retry(
    app: &AppHandle,
    artifact: &RecordingArtifact,
    settings: &AppSettings,
) -> Result<String, Failure> {
    match transcription::transcribe(app, artifact, settings).await {
        Err(failure) if is_transient(&failure) => {
            diagnostics::warning!(
                "transcription.retrying",
                recording = artifact.recording,
                class = failure.class()
            );
            tokio::time::sleep(TRANSIENT_RETRY_DELAY).await;
            transcription::transcribe(app, artifact, settings).await
        }
        outcome => outcome,
    }
}

async fn process_recording(
    app: &AppHandle,
    artifact: RecordingArtifact,
    request: ProcessRequest,
    current_settings: AppSettings,
    began: Instant,
) -> Result<ProcessResult, String> {
    let recording = artifact.recording;
    let duration_ms = artifact.duration_ms();
    let engine = current_settings.engine.as_str();
    let model = transcription_model(
        &current_settings.engine,
        current_settings.local_model_id.as_deref(),
    );
    diagnostics::info!(
        "transcription.started",
        recording = recording,
        engine = engine,
        model = model,
        audio_ms = duration_ms
    );
    let transcribing = Instant::now();
    let transcript = match transcribe_with_retry(app, &artifact, &current_settings).await {
        Ok(transcript) => {
            diagnostics::info!(
                "transcription.completed",
                recording = recording,
                engine = engine,
                model = model,
                elapsed_ms = transcribing.elapsed().as_millis(),
                characters = transcript.chars().count()
            );
            transcript
        }
        Err(failure) => {
            let message = diagnostics::failure!(
                "transcription.failed",
                &failure,
                recording = recording,
                engine = engine,
                model = model,
                audio_ms = duration_ms,
                elapsed_ms = transcribing.elapsed().as_millis()
            );
            if !worth_keeping(&failure) {
                return Err(message);
            }
            app.state::<RetainedRecordings>()
                .keep(Retained { artifact, request });
            return Err(format!(
                "{message} The recording is kept, so you can try again."
            ));
        }
    };
    let mut warnings = Vec::new();
    if artifact.interrupted {
        warnings.push(
            "The microphone stopped early, so this covers only what was recorded until then."
                .to_string(),
        );
    }
    let mut transformation_succeeded = true;
    let action = actions::diagnostic_id(&request.action);
    let transforms = request.action != actions::PLAIN;
    let effort = current_settings
        .text_effort
        .map_or("auto", |effort| effort.as_str());
    if transforms {
        diagnostics::info!(
            "transform.started",
            recording = recording,
            action = action,
            model = text_model_name(&current_settings.text_model),
            effort = effort
        );
    }
    let transforming = Instant::now();
    let text = match transcription::transform(
        &transcript,
        &request.action,
        request.custom_prompt.as_deref(),
        &current_settings,
    )
    .await
    {
        Ok(text) => {
            if transforms {
                diagnostics::info!(
                    "transform.completed",
                    recording = recording,
                    action = action,
                    elapsed_ms = transforming.elapsed().as_millis()
                );
            }
            text
        }
        Err(failure) if transforms => {
            transformation_succeeded = false;
            warnings.push(format!(
                "Transformation failed; the plain transcript was used: {}",
                diagnostics::fallback!(
                    "transform.fallback",
                    &failure,
                    recording = recording,
                    action = action,
                    model = text_model_name(&current_settings.text_model),
                    effort = effort,
                    elapsed_ms = transforming.elapsed().as_millis()
                )
            ));
            transcript
        }
        Err(failure) => {
            return Err(diagnostics::failure!(
                "transform.failed",
                &failure,
                recording = recording,
                action = action
            ));
        }
    };
    // Persist before external delivery, so clipboard/file failures cannot lose the text.
    let history_entry = if current_settings.history_enabled {
        let entry = HistoryEntry::new(&text, duration_ms, current_settings.engine.clone());
        match history::append(app, entry.clone()) {
            Ok(()) => {
                diagnostics::info!("history.saved", recording = recording);
                Some(entry)
            }
            Err(failure) => {
                warnings.push(format!(
                    "History was not saved: {}",
                    diagnostics::fallback!("history.failed", &failure, recording = recording)
                ));
                None
            }
        }
    } else {
        None
    };
    let delivery =
        output::deliver(app, &text, &request, &current_settings, recording).map_err(|failure| {
            diagnostics::failure!("delivery.rejected", &failure, recording = recording)
        })?;
    // Stop only confirms capture ended. Done confirms the requested transformation and
    // every output completed, even when the window is hidden. History is best-effort.
    let delivered = transformation_succeeded
        && !artifact.interrupted
        && delivery.all_requested_outputs_succeeded(&request);
    if delivered {
        tray::set_delivered(app, recording);
    }
    let done_cue = current_settings.sound_enabled && delivered;
    if done_cue {
        // Not awaited: the interface is ready for the next recording the
        // moment this command returns, and the cue must never hold that up.
        // The cue thread logs the outcome; the next recording silences it if
        // it is still sounding by then.
        let _ = app.state::<feedback::DoneCues>().play();
    }
    warnings.extend(delivery.warnings);
    diagnostics::info!(
        "recording.completed",
        recording = recording,
        mode = "batch",
        total_ms = began.elapsed().as_millis(),
        warnings = warnings.len(),
        done_cue = done_cue
    );

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

/// The kept recording next in line, if any. A query: nothing is logged.
#[tauri::command]
pub fn retained_recording(state: State<'_, RetainedRecordings>) -> Option<retained::Waiting> {
    state.waiting()
}

/// Run a kept recording through the same path, with the settings as they are
/// now, since fixing them may be why the user is trying again.
///
/// The text goes to the clipboard and never the cursor: this was asked for
/// from Utterform's own window, so the cursor is no longer where it was.
#[tauri::command]
pub async fn retry_recording(app: AppHandle) -> Result<ProcessResult, String> {
    let retained = app.state::<RetainedRecordings>();
    let Some(Retained { artifact, request }) = retained.take() else {
        return Err(diagnostics::failure!(
            "recording.retry_failed",
            &Failure::guidance("nothing_retained", "No recording is waiting")
        ));
    };
    let began = Instant::now();
    let current_settings = match settings::load(&app) {
        Ok(settings) => settings,
        Err(failure) => {
            retained.restore(Retained { artifact, request });
            return Err(diagnostics::failure!(
                "settings.load_failed",
                &failure,
                during = "recording.retry"
            ));
        }
    };
    diagnostics::info!("recording.retry", recording = artifact.recording);
    let request = ProcessRequest {
        type_at_cursor: false,
        copy_to_clipboard: true,
        ..request
    };
    process_recording(&app, artifact, request, current_settings, began).await
}

/// Give up the kept recording next in line, deleting its audio.
#[tauri::command]
pub fn discard_recording(state: State<'_, RetainedRecordings>) {
    if let Some(recording) = state.take() {
        diagnostics::info!(
            "recording.retained_discarded",
            recording = recording.artifact.recording
        );
    }
}

/// Say what happened when the user is not looking at Utterform. A dictation
/// started by a hotkey has no other way to report: the message is in a hidden
/// window, and silence reads as success. A notification that cannot be shown
/// opens the window instead, since being interrupted beats not knowing.
///
/// The text is shown, never logged: it can quote a third party.
#[tauri::command]
pub fn notify_if_unattended(app: AppHandle, message: String) {
    let attended = app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    });
    if attended {
        return;
    }
    let shown = app
        .notification()
        .builder()
        .title("Utterform")
        .body(message.chars().take(NOTIFICATION_LIMIT).collect::<String>())
        .show();
    match shown {
        Ok(()) => diagnostics::info!("attention.notified"),
        Err(_) => {
            diagnostics::warning!("attention.notification_failed");
            reveal_main_window(&app, "attention");
        }
    }
}

#[tauri::command]
pub fn list_history(app: AppHandle) -> Result<Vec<HistoryEntry>, String> {
    history::list(&app).map_err(|failure| diagnostics::failure!("history.read_failed", &failure))
}

#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    match history::clear(&app) {
        Ok(()) => {
            diagnostics::info!("history.cleared");
            Ok(())
        }
        Err(failure) => Err(diagnostics::failure!("history.clear_failed", &failure)),
    }
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    match app.clipboard().write_text(text) {
        Ok(()) => {
            diagnostics::info!("clipboard.copied", source = "manual");
            Ok(())
        }
        Err(error) => Err(diagnostics::failure!(
            "clipboard.copy_failed",
            &Failure::new("clipboard", format!("Could not copy text: {error}")),
            source = "manual"
        )),
    }
}

#[tauri::command]
pub fn has_openai_api_key() -> bool {
    secrets::has_openai_api_key()
}

#[tauri::command]
pub fn set_openai_api_key(api_key: String) -> Result<(), String> {
    match secrets::set_openai_api_key(&api_key) {
        Ok(()) => {
            diagnostics::info!("secrets.key_saved");
            Ok(())
        }
        Err(failure) => Err(diagnostics::failure!("secrets.save_failed", &failure)),
    }
}

#[tauri::command]
pub fn delete_openai_api_key() -> Result<(), String> {
    match secrets::delete_openai_api_key() {
        Ok(()) => {
            diagnostics::info!("secrets.key_deleted");
            Ok(())
        }
        Err(failure) => Err(diagnostics::failure!("secrets.delete_failed", &failure)),
    }
}

#[tauri::command]
pub fn list_local_models(app: AppHandle) -> Result<Vec<LocalModelInfo>, String> {
    models::list(&app).map_err(|failure| diagnostics::failure!("models.list_failed", &failure))
}

#[tauri::command]
pub async fn download_local_model(app: AppHandle, model_id: String) -> Result<(), String> {
    models::download(&app, &model_id).await.map_err(|failure| {
        diagnostics::failure!(
            "models.download_failed",
            &failure,
            model = models::diagnostic_id(&model_id)
        )
    })
}

#[tauri::command]
pub async fn delete_local_model(app: AppHandle, model_id: String) -> Result<(), String> {
    let model = models::diagnostic_id(&model_id);
    match models::delete(&app, &model_id).await {
        Ok(()) => {
            diagnostics::info!("models.deleted", model = model);
            Ok(())
        }
        Err(failure) => Err(diagnostics::failure!(
            "models.delete_failed",
            &failure,
            model = model
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plausible_model_id_is_named() {
        assert_eq!(text_model_name("gpt-5-mini"), "gpt-5-mini");
        assert_eq!(
            text_model_name("ft:gpt-4o-mini:acme:1"),
            "ft:gpt-4o-mini:acme:1"
        );
        for typed in ["", "my secret project notes", "gpt/5", &"x".repeat(65)] {
            assert_eq!(text_model_name(typed), "custom", "{typed}");
        }
    }

    #[test]
    fn only_a_dropped_connection_or_a_busy_service_earns_the_quiet_retry() {
        assert!(is_transient(&Failure::new("connect", "")));
        assert!(is_transient(&Failure::new("transport", "")));
        assert!(is_transient(&Failure::new("http", "").status(503)));
        assert!(is_transient(&Failure::new("http", "").status(429)));
        assert!(!is_transient(&Failure::new("http", "").status(401)));
        assert!(!is_transient(&Failure::new("timeout", "")));
        assert!(!is_transient(&Failure::new("keyring", "")));
    }

    #[test]
    fn a_recording_is_kept_unless_there_was_nothing_in_it() {
        assert!(worth_keeping(&Failure::new("connect", "")));
        assert!(worth_keeping(&Failure::new("keyring", "")));
        assert!(worth_keeping(&Failure::new("http", "").status(401)));
        assert!(!worth_keeping(&Failure::guidance("empty_result", "")));
        assert!(!worth_keeping(&Failure::new("empty_recording", "")));
    }
}
