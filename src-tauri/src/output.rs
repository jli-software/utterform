use std::{fs, path::PathBuf};

use chrono::Local;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::domain::{AppSettings, OutputFormat, ProcessRequest};

pub struct DeliveryResult {
    pub saved_path: Option<String>,
    pub copied_to_clipboard: bool,
    pub typed_at_cursor: bool,
    pub warnings: Vec<String>,
}

impl DeliveryResult {
    pub fn all_requested_outputs_succeeded(&self, request: &ProcessRequest) -> bool {
        (request.copy_to_clipboard || request.save_to_file || request.type_at_cursor)
            && (!request.copy_to_clipboard || self.copied_to_clipboard)
            && (!request.save_to_file || self.saved_path.is_some())
            && (!request.type_at_cursor || self.typed_at_cursor)
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
        || crate::typing::insert_at_cursor(text),
    )
}

fn deliver_to(
    request: &ProcessRequest,
    clipboard: impl FnOnce() -> Result<(), String>,
    file: impl FnOnce() -> Result<PathBuf, String>,
    type_at_cursor: impl FnOnce() -> Result<(), String>,
) -> Result<DeliveryResult, String> {
    if !request.copy_to_clipboard && !request.save_to_file && !request.type_at_cursor {
        return Err("Select Clipboard, File, or typing at the cursor as an output".into());
    }

    let mut copied_to_clipboard = false;
    let mut typed_at_cursor = false;
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

    // Typed last: the clipboard and the file are already safe by then, so a
    // missing wtype/xdotool costs a warning rather than the text.
    if request.type_at_cursor {
        match type_at_cursor() {
            Ok(()) => typed_at_cursor = true,
            Err(error) => warnings.push(format!("Typing: {error}")),
        }
    }

    if !copied_to_clipboard && saved_path.is_none() && !typed_at_cursor && warnings.is_empty() {
        warnings.push("No output target completed".to_string());
    }
    Ok(DeliveryResult {
        saved_path,
        copied_to_clipboard,
        typed_at_cursor,
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

    fn request(
        copy_to_clipboard: bool,
        save_to_file: bool,
        type_at_cursor: bool,
    ) -> ProcessRequest {
        ProcessRequest {
            action: "plain".into(),
            custom_prompt: None,
            copy_to_clipboard,
            save_to_file,
            type_at_cursor,
            output_format: OutputFormat::Txt,
        }
    }

    #[test]
    fn completion_requires_every_requested_delivery_and_reports_clipboard_truthfully() {
        for clipboard_requested in [false, true] {
            for file_requested in [false, true] {
                for typing_requested in [false, true] {
                    let request = request(clipboard_requested, file_requested, typing_requested);
                    for clipboard_succeeds in [false, true] {
                        for file_succeeds in [false, true] {
                            for typing_succeeds in [false, true] {
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
                                    || {
                                        assert!(typing_requested);
                                        if typing_succeeds {
                                            Ok(())
                                        } else {
                                            Err("wtype is not installed".into())
                                        }
                                    },
                                );
                                if !clipboard_requested && !file_requested && !typing_requested {
                                    assert!(result.is_err());
                                    continue;
                                }
                                let result = result.unwrap();
                                assert_eq!(
                                    result.copied_to_clipboard,
                                    clipboard_requested && clipboard_succeeds
                                );
                                assert_eq!(
                                    result.saved_path.is_some(),
                                    file_requested && file_succeeds
                                );
                                assert_eq!(
                                    result.typed_at_cursor,
                                    typing_requested && typing_succeeds
                                );
                                assert_eq!(
                                    result.warnings.len(),
                                    usize::from(clipboard_requested && !clipboard_succeeds)
                                        + usize::from(file_requested && !file_succeeds)
                                        + usize::from(typing_requested && !typing_succeeds)
                                );
                                assert_eq!(
                                    result.all_requested_outputs_succeeded(&request),
                                    (!clipboard_requested || clipboard_succeeds)
                                        && (!file_requested || file_succeeds)
                                        && (!typing_requested || typing_succeeds)
                                );
                            }
                        }
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
