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
mod live;
#[cfg(target_os = "macos")]
mod macos;
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

    /// An intent the interface has no part in is not kept at all. An autostart
    /// launch must arrive there as nothing, rather than as something it has to
    /// remember to ignore.
    fn set(&self, intent: Intent) {
        if !intent.reaches_the_interface() {
            return;
        }
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(intent);
        }
    }
}

/// The operating system's own "start when I sign in" entry.
///
/// Nothing about it is stored in `settings.json`: the entry itself is the only
/// truth, and Settings reads it back through the plugin. The fixed argument is
/// what makes a login start silent — see `cli::Intent::Autostart`. macOS keeps
/// this builder's default, the per-user LaunchAgent.
fn autostart<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new()
        .app_name("Utterform")
        .arg(cli::AUTOSTART_FLAG)
        .build()
}

/// Give a login start the environment every other Linux start already has.
/// The installer's launcher pins the graphics backend; an autostart entry runs
/// the executable directly, so the policy is applied here instead — before
/// anything of GTK exists, and only for this one kind of start.
#[cfg(target_os = "linux")]
fn prepare_autostart_environment(intent: Intent) {
    if intent != Intent::Autostart {
        return;
    }
    let configured = std::env::var("GDK_BACKEND").ok();
    if let Some(backend) = platform::autostart_gdk_backend(configured.as_deref()) {
        // SAFETY: the first thing the program does, with no second thread and
        // no library that could be reading the environment yet.
        unsafe { std::env::set_var("GDK_BACKEND", backend) };
    }
}

pub fn run() {
    // Read before the window and the event loop exist: on Linux an autostart
    // launch has to settle its environment before GTK is initialized.
    let startup_intent = cli::intent_from(std::env::args());
    #[cfg(target_os = "linux")]
    prepare_autostart_environment(startup_intent);

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
            // routed to it rather than reimplemented here. An autostart entry
            // that meets the running app asks it for nothing at all.
            if intent.reaches_the_interface() {
                let _ = app.emit("remote-intent", intent);
            }
        }))
        .plugin(autostart())
        .plugin(tauri_plugin_dialog::init())
        // Only ever used to stand in for a start cue no output device would play.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(audio::AudioCaptureState::default())
        .manage(live::LiveState::default())
        .manage(StartupIntent::default())
        .manage(history::HistoryState::default())
        .manage(hotkey::Failure::default())
        .setup(move |app| {
            diagnostics::install(app.handle());
            audio::cleanup_stale_recordings().map_err(std::io::Error::other)?;
            app.state::<StartupIntent>().set(startup_intent);
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
                if startup_intent.raises_window() {
                    window.show()?;
                }
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
            commands::typing_support,
            commands::get_settings,
            commands::save_settings,
            commands::list_input_devices,
            commands::start_recording,
            commands::get_recording_status,
            commands::get_live_status,
            commands::live_support,
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
        .build(tauri::generate_context!())
        .expect("error while building Utterform")
        .run(|app, event| {
            // A click on the Dock icon while the window is hidden to the tray
            // is macOS's way of asking for it back; nowhere else sends this.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                reveal_main_window(app);
            }
            let _ = (app, &event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_the_interface_acts_on_waits_for_it() {
        // A hotkey that had to start Utterform still means "record now", so
        // the intent has to survive until the interface is ready to take it.
        for intent in [Intent::Toggle, Intent::Start, Intent::Stop, Intent::Cancel] {
            let pending = StartupIntent::default();
            pending.set(intent);
            assert_eq!(pending.take(), Some(intent), "{intent:?}");
            // Delivered once; a reload must not record a second time.
            assert_eq!(pending.take(), None, "{intent:?}");
        }
    }

    #[test]
    fn an_autostart_launch_never_reaches_the_interface() {
        let pending = StartupIntent::default();
        pending.set(Intent::Autostart);
        assert_eq!(pending.take(), None);
    }
}
