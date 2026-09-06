mod audio;
mod commands;
mod domain;
mod history;
mod models;
mod output;
mod secrets;
mod settings;
mod transcription;

use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(audio::AudioCaptureState::default())
        .manage(history::HistoryState::default())
        .setup(|app| {
            audio::cleanup_stale_recordings().map_err(std::io::Error::other)?;
            let show = MenuItem::with_id(app, "show", "Show Utterform", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, event| match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    let state = app.state::<audio::AudioCaptureState>();
                    let _ = audio::cancel_recording(&state);
                    app.exit(0);
                }
                _ => {}
            })
            .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let state = window.state::<audio::AudioCaptureState>();
                let _ = audio::cancel_recording(&state);
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::list_input_devices,
            commands::start_recording,
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
