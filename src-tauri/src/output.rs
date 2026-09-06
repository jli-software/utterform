use std::{fs, path::PathBuf};

use chrono::Local;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::domain::{AppSettings, OutputFormat, ProcessRequest};

pub struct DeliveryResult {
    pub saved_path: Option<String>,
    pub warnings: Vec<String>,
}

pub fn deliver(
    app: &AppHandle,
    text: &str,
    request: &ProcessRequest,
    settings: &AppSettings,
) -> Result<DeliveryResult, String> {
    if !request.copy_to_clipboard && !request.save_to_file {
        return Err("Select Clipboard, File, or both as an output".into());
    }

    let mut successes = 0;
    let mut warnings = Vec::new();
    let mut saved_path = None;

    if request.copy_to_clipboard {
        match app.clipboard().write_text(text) {
            Ok(()) => successes += 1,
            Err(error) => warnings.push(format!("Clipboard: {error}")),
        }
    }

    if request.save_to_file {
        match save_file(text, request.output_format, settings) {
            Ok(path) => {
                successes += 1;
                saved_path = Some(path.to_string_lossy().into_owned());
            }
            Err(error) => warnings.push(format!("File: {error}")),
        }
    }

    if successes == 0 && warnings.is_empty() {
        warnings.push("No output target completed".to_string());
    }
    Ok(DeliveryResult {
        saved_path,
        warnings,
    })
}

fn save_file(text: &str, format: OutputFormat, settings: &AppSettings) -> Result<PathBuf, String> {
    let directory = settings
        .output_directory
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Choose a default output folder in Settings".to_string())?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create the output folder: {error}"))?;

    let timestamp = Local::now().format("%Y-%m-%d-%H%M%S-%3f");
    let file_name = format!("utterform-{timestamp}.{}", format.extension());
    let destination = directory.join(file_name);
    let temporary = destination.with_extension(format!("{}.part", format.extension()));
    fs::write(&temporary, text.as_bytes())
        .map_err(|error| format!("Could not write the output file: {error}"))?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Could not finalize the output file: {error}"));
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_stable() {
        assert_eq!(OutputFormat::Txt.extension(), "txt");
        assert_eq!(OutputFormat::Md.extension(), "md");
    }
}
