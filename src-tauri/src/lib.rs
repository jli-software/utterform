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

use tauri::Manager;

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

/// Records which step of setup failed, then hands the error on unchanged.
fn setup_stage<E: Into<Box<dyn std::error::Error>>>(
    stage: &'static str,
) -> impl FnOnce(E) -> Box<dyn std::error::Error> {
    move |error| {
        diagnostics::error!("app.setup_failed", stage = stage);
        error.into()
    }
}

fn setup(app: &mut tauri::App, startup_intent: Intent) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(failure) = audio::cleanup_stale_recordings() {
        let message =
            diagnostics::failure!("app.setup_failed", &failure, stage = "stale_recordings");
        return Err(std::io::Error::other(message).into());
    }
    app.state::<StartupIntent>().set(startup_intent);
    diagnostics::info!("app.startup_intent", intent = startup_intent.as_str());
    // Best-effort on purpose: a session that will not grant a global
    // shortcut, or a key another application already holds, must cost
    // the dictation key and not the application. The reason is kept so
    // Settings can report it when the user next looks.
    let handle = app.handle().clone();
    match hotkey::install(&handle) {
        Ok(()) => {
            let configured = match settings::load(&handle) {
                Ok(settings) => settings.global_hotkey,
                Err(failure) => {
                    diagnostics::fallback!(
                        "settings.load_failed",
                        &failure,
                        during = "hotkey_startup"
                    );
                    None
                }
            };
            // On its own thread: registering asks the event loop and waits
            // for the answer, and the loop only starts once setup returns.
            // `apply` records its own outcome.
            std::thread::spawn(move || {
                let _ = hotkey::apply(&handle, configured.as_deref(), "startup");
            });
        }
        Err(failure) => {
            diagnostics::fallback!("hotkey.install_failed", &failure);
        }
    }
    // A menu-bar application, for good: no Dock icon and no ⌘-Tab
    // entry whether the window is showing or hidden, the same policy
    // `LSUIElement` in Info.plist declares for the launch itself, so
    // the two never disagree and nothing ever switches back. Set
    // before the window can first be shown, and never changed again.
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    if let Some(window) = app.get_webview_window("main") {
        if platform::use_borderless_window() {
            window
                .set_decorations(false)
                .map_err(setup_stage("decorations"))?;
        }
        if startup_intent.raises_window() {
            // macOS does not bring an accessory application to the
            // front for being launched, so a launch that asks for the
            // window takes the same path as a tray click.
            #[cfg(target_os = "macos")]
            reveal_main_window(app.handle(), "launch");
            #[cfg(not(target_os = "macos"))]
            {
                window.show().map_err(setup_stage("show_window"))?;
                diagnostics::info!("window.revealed", source = "launch");
            }
        }
    }
    tray::install(app.handle()).map_err(setup_stage("tray"))?;
    Ok(())
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
            diagnostics::info!("app.second_instance", intent = intent.as_str());
            if intent.raises_window() {
                reveal_main_window(app, "second_instance");
            }
            // The interface owns the recording state machine, so a hotkey is
            // routed to it rather than reimplemented here. An autostart entry
            // that meets the running app asks it for nothing at all.
            if intent.reaches_the_interface() {
                diagnostics::notify_interface(app, "remote-intent", intent);
            }
        }))
        .plugin(autostart())
        .plugin(tauri_plugin_dialog::init())
        // Only ever used to stand in for a start cue no output device would play.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Used from Rust alone, to open the log file and its folder for
        // Settings. No JavaScript permission is granted, and no link handler
        // is injected into the WebView.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .manage(audio::AudioCaptureState::default())
        .manage(feedback::DoneCues::default())
        .manage(live::LiveState::default())
        .manage(StartupIntent::default())
        .manage(history::HistoryState::default())
        .manage(hotkey::Failure::default())
        .setup(move |app| {
            // First, so that everything after it — a failure here included —
            // is recorded, and a panic from here on leaves its trace.
            diagnostics::install(app.handle());
            let began = std::time::Instant::now();
            let outcome = setup(app, startup_intent);
            if outcome.is_ok() {
                diagnostics::info!(
                    "app.setup_completed",
                    elapsed_ms = began.elapsed().as_millis()
                );
            }
            outcome
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                // Close means hide to tray, not discard audio. Capture is native and
                // continues across focus changes, minimization and Super+W on Omarchy.
                match window.hide() {
                    Ok(()) => diagnostics::info!("window.hidden", reason = "close_requested"),
                    Err(_) => {
                        diagnostics::warning!("window.hide_failed", reason = "close_requested")
                    }
                }
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
            commands::diagnostics_info,
            commands::copy_diagnostics,
            commands::open_log_file,
            commands::open_log_folder,
            commands::report_frontend_error,
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
            // Utterform has no Dock icon, but opening it again from Finder,
            // Launchpad or Spotlight while it runs does not start a second
            // process on macOS: Launch Services asks the running one to
            // reopen. This is the macOS counterpart of the single-instance
            // hand-off above, and reveals the same one window.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                reveal_main_window(app, "reopen");
            }
            // The one clean ending: tray Quit arrives here after capture was
            // discarded. Anything that never gets here leaves the run marker.
            if let tauri::RunEvent::Exit = &event {
                diagnostics::finish_run();
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
