use std::{fs, path::PathBuf};

use tauri::{AppHandle, Manager};

use crate::domain::AppSettings;

const SETTINGS_FILE: &str = "settings.json";

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join(SETTINGS_FILE))
        .map_err(|error| format!("Could not resolve the settings directory: {error}"))
}

pub fn load(app: &AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(app)?;
    let backup = path.with_extension("json.bak");
    let source = if path.exists() {
        path
    } else if backup.exists() {
        backup
    } else {
        return Ok(AppSettings::default());
    };

    let contents =
        fs::read_to_string(&source).map_err(|error| format!("Could not read settings: {error}"))?;
    serde_json::from_str(&contents).map_err(|error| format!("Settings are invalid: {error}"))
}

pub fn save(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Settings path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create the settings directory: {error}"))?;

    let payload = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("Could not serialize settings: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    fs::write(&temporary, payload).map_err(|error| format!("Could not write settings: {error}"))?;

    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Could not replace the settings backup: {error}"))?;
        }
        fs::rename(&path, &backup)
            .map_err(|error| format!("Could not back up settings: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if backup.exists() {
            let _ = fs::rename(&backup, &path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not activate settings: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_serializable() {
        let json = serde_json::to_string(&AppSettings::default()).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.text_model, "gpt-5-mini");
        assert!(restored.copy_to_clipboard);
    }
}
