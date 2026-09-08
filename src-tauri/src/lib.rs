mod actions;
mod activation;
mod audio;
mod cli;
mod commands;
mod diagnostics;
mod domain;
mod feedback;
mod history;
mod hotkey;
mod models;
mod output;
mod platform;
mod secrets;
mod settings;
mod transcription;
mod tray;
mod typing;

use tauri::{Emitter, Manager};

use activation::reveal_main_window;
use cli::Intent;

/// The intent Utterform itself was launched with, kept until the interface is
/// ready to act on it. A hotkey that starts the app must still start recording.
#[derive(Default)]
pub struct StartupIntent(std::sync::Mutex<Option<Intent>>);

impl StartupIntent {
    pub fn take(&self) -> Option<Intent> {
        self.0.lock().ok().and_then(|mut intent| intent.take())
    }

    fn set(&self, intent: Intent) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(intent);
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        // Registered first so a launcher entry or a second `utterform` process
        // hands its arguments to the running app instead of starting a rival
        // instance with its own tray icon and recording state.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let intent = cli::intent_from(args);
            if intent.raises_window() {
                reveal_main_window(app);
            }
            // The interface owns the recording state machine, so a hotkey is
            // routed to it rather than reimplemented here.
            let _ = app.emit("remote-intent", intent);
        }))
        .plugin(tauri_plugin_dialog::init())
        // Only ever used to stand in for a start cue no output device would play.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(audio::AudioCaptureState::default())
        .manage(StartupIntent::default())
        .manage(history::HistoryState::default())
        .manage(hotkey::Failure::default())
        .setup(|app| {
            diagnostics::install(app.handle());
            audio::cleanup_stale_recordings().map_err(std::io::Error::other)?;
            app.state::<StartupIntent>()
                .set(cli::intent_from(std::env::args()));
            // Best-effort on purpose: a session that will not grant a global
            // shortcut, or a key another application already holds, must cost
            // the dictation key and not the application. The reason is kept so
            // Settings can report it when the user next looks.
            let handle = app.handle().clone();
            if hotkey::install(&handle).is_ok() {
                let configured = settings::load(&handle)
                    .ok()
                    .and_then(|settings| settings.global_hotkey);
                // On its own thread: registering asks the event loop and waits
                // for the answer, and the loop only starts once setup returns.
                std::thread::spawn(move || {
                    let _ = hotkey::apply(&handle, configured.as_deref());
                });
            }
            if let Some(window) = app.get_webview_window("main") {
                if platform::use_borderless_window() {
                    window.set_decorations(false)?;
                }
                window.show()?;
            }
            tray::install(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                // Close means hide to tray, not discard audio. Capture is native and
                // continues across focus changes, minimization and Super+W on Omarchy.
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::take_startup_intent,
            commands::global_hotkey_support,
            commands::apply_global_hotkey,
            commands::list_built_in_actions,
            commands::get_settings,
            commands::save_settings,
            commands::list_input_devices,
            commands::start_recording,
            commands::get_recording_status,
            commands::play_test_cues,
            commands::diagnostics_log_path,
            commands::set_recording_paused,
            commands::cancel_recording,
            commands::finish_recording,
            commands::list_history,
            commands::clear_history,
            commands::copy_text,
            commands::has_openai_api_key,
            commands::set_openai_api_key,
            commands::delete_openai_api_key,
            commands::list_local_models,
            commands::download_local_model,
            commands::delete_local_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Utterform");
}
