use std::{fs, path::PathBuf};

use chrono::Local;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::domain::{AppSettings, OutputFormat, ProcessRequest};

pub struct DeliveryResult {
    pub saved_path: Option<String>,
    pub copied_to_clipboard: bool,
    pub warnings: Vec<String>,
}

impl DeliveryResult {
    pub fn all_requested_outputs_succeeded(&self, request: &ProcessRequest) -> bool {
        (request.copy_to_clipboard || request.save_to_file)
            && (!request.copy_to_clipboard || self.copied_to_clipboard)
            && (!request.save_to_file || self.saved_path.is_some())
    }
}

pub fn deliver(
    app: &AppHandle,
    text: &str,
    request: &ProcessRequest,
    settings: &AppSettings,
) -> Result<DeliveryResult, String> {
    deliver_to(
        request,
        || {
            app.clipboard()
                .write_text(text)
                .map_err(|error| error.to_string())
        },
        || save_file(text, request.output_format, settings),
    )
}

fn deliver_to(
    request: &ProcessRequest,
    clipboard: impl FnOnce() -> Result<(), String>,
    file: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<DeliveryResult, String> {
    if !request.copy_to_clipboard && !request.save_to_file {
        return Err("Select Clipboard, File, or both as an output".into());
    }

    let mut copied_to_clipboard = false;
    let mut warnings = Vec::new();
    let mut saved_path = None;

    if request.copy_to_clipboard {
        match clipboard() {
            Ok(()) => copied_to_clipboard = true,
            Err(error) => warnings.push(format!("Clipboard: {error}")),
        }
    }

    if request.save_to_file {
        match file() {
            Ok(path) => {
                saved_path = Some(path.to_string_lossy().into_owned());
            }
            Err(error) => warnings.push(format!("File: {error}")),
        }
    }

    if !copied_to_clipboard && saved_path.is_none() && warnings.is_empty() {
        warnings.push("No output target completed".to_string());
    }
    Ok(DeliveryResult {
        saved_path,
        copied_to_clipboard,
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

    fn request(copy_to_clipboard: bool, save_to_file: bool) -> ProcessRequest {
        ProcessRequest {
            action: "plain".into(),
            custom_prompt: None,
            copy_to_clipboard,
            save_to_file,
            output_format: OutputFormat::Txt,
        }
    }

    #[test]
    fn completion_requires_every_requested_delivery_and_reports_clipboard_truthfully() {
        for clipboard_requested in [false, true] {
            for file_requested in [false, true] {
                let request = request(clipboard_requested, file_requested);
                for clipboard_succeeds in [false, true] {
                    for file_succeeds in [false, true] {
                        let result = deliver_to(
                            &request,
                            || {
                                assert!(clipboard_requested);
                                if clipboard_succeeds {
                                    Ok(())
                                } else {
                                    Err("unavailable".into())
                                }
                            },
                            || {
                                assert!(file_requested);
                                if file_succeeds {
                                    Ok(PathBuf::from("note.txt"))
                                } else {
                                    Err("unwritable".into())
                                }
                            },
                        );
                        if !clipboard_requested && !file_requested {
                            assert!(result.is_err());
                            continue;
                        }
                        let result = result.unwrap();
                        assert_eq!(
                            result.copied_to_clipboard,
                            clipboard_requested && clipboard_succeeds
                        );
                        assert_eq!(result.saved_path.is_some(), file_requested && file_succeeds);
                        assert_eq!(
                            result.warnings.len(),
                            usize::from(clipboard_requested && !clipboard_succeeds)
                                + usize::from(file_requested && !file_succeeds)
                        );
                        assert_eq!(
                            result.all_requested_outputs_succeeded(&request),
                            (!clipboard_requested || clipboard_succeeds)
                                && (!file_requested || file_succeeds)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn extensions_are_stable() {
        assert_eq!(OutputFormat::Txt.extension(), "txt");
        assert_eq!(OutputFormat::Md.extension(), "md");
    }
}
