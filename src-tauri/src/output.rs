use std::{fs, path::PathBuf};

use chrono::Local;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    diagnostics::{self, Failure},
    domain::{AppSettings, OutputFormat, ProcessRequest},
};

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

/// Delivers `text` to every requested output. Each output's failure becomes a
/// warning carrying the reference of its own `delivery.failed` event, and one
/// `delivery.completed` summarizes the outcome — naming the outputs, the file
/// format and the saved file's name, never the text or the folder.
pub fn deliver(
    app: &AppHandle,
    text: &str,
    request: &ProcessRequest,
    settings: &AppSettings,
    recording: u64,
) -> Result<DeliveryResult, Failure> {
    deliver_to(
        recording,
        request,
        settings.typing_method.as_str(),
        || {
            app.clipboard()
                .write_text(text)
                .map_err(|error| Failure::new("clipboard", format!("{error}")))
        },
        || save_file(text, request.output_format, settings),
        |clipboard_holds_text| {
            // The platform's sentences name tools, programs and counts, never
            // the text, so they are kept as the detail.
            crate::typing::insert_at_cursor(
                app,
                text,
                settings.typing_method,
                settings.typing_delay_ms,
                clipboard_holds_text,
            )
            .map_err(|message| {
                if crate::typing::is_guidance(&message) {
                    Failure::guidance("typing_refused", message.clone()).detail(message)
                } else {
                    Failure::new("typing", message.clone()).detail(message)
                }
            })
        },
    )
}

fn deliver_to(
    recording: u64,
    request: &ProcessRequest,
    typing_method: &str,
    clipboard: impl FnOnce() -> Result<(), Failure>,
    file: impl FnOnce() -> Result<PathBuf, Failure>,
    type_at_cursor: impl FnOnce(bool) -> Result<(), Failure>,
) -> Result<DeliveryResult, Failure> {
    if !request.copy_to_clipboard && !request.save_to_file && !request.type_at_cursor {
        return Err(Failure::guidance(
            "no_output",
            "Select Clipboard, File, or typing at the cursor as an output",
        ));
    }

    let mut copied_to_clipboard = false;
    let mut typed_at_cursor = false;
    let mut warnings = Vec::new();
    let mut saved_path = None;
    let mut saved_name = String::new();
    let outcome = |requested: bool, succeeded: bool| match (requested, succeeded) {
        (false, _) => "off",
        (true, true) => "ok",
        (true, false) => "failed",
    };

    if request.copy_to_clipboard {
        match clipboard() {
            Ok(()) => copied_to_clipboard = true,
            Err(failure) => warnings.push(format!(
                "Clipboard: {}",
                diagnostics::fallback!(
                    "delivery.failed",
                    &failure,
                    recording = recording,
                    target = "clipboard"
                )
            )),
        }
    }

    if request.save_to_file {
        match file() {
            Ok(path) => {
                // The file's own name identifies it; the folder is the user's.
                saved_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                saved_path = Some(path.to_string_lossy().into_owned());
            }
            Err(failure) => warnings.push(format!(
                "File: {}",
                diagnostics::fallback!(
                    "delivery.failed",
                    &failure,
                    recording = recording,
                    target = "file",
                    format = request.output_format.extension()
                )
            )),
        }
    }

    // Typed last: the clipboard and the file are already safe by then, so a
    // missing wtype/xdotool costs a warning rather than the text. Paste
    // delivery is told whether the clipboard already carries the transcript,
    // so it neither writes it twice nor overwrites the clipboard needlessly.
    if request.type_at_cursor {
        match type_at_cursor(copied_to_clipboard) {
            Ok(()) => typed_at_cursor = true,
            Err(failure) => warnings.push(format!(
                "Typing: {}",
                diagnostics::fallback!(
                    "delivery.failed",
                    &failure,
                    recording = recording,
                    target = "cursor",
                    method = typing_method
                )
            )),
        }
    }

    if !copied_to_clipboard && saved_path.is_none() && !typed_at_cursor && warnings.is_empty() {
        warnings.push("No output target completed".to_string());
    }
    diagnostics::info!(
        "delivery.completed",
        recording = recording,
        clipboard = outcome(request.copy_to_clipboard, copied_to_clipboard),
        file = outcome(request.save_to_file, saved_path.is_some()),
        cursor = outcome(request.type_at_cursor, typed_at_cursor),
        format = request.output_format.extension(),
        file_name = saved_name,
        warnings = warnings.len(),
    );
    Ok(DeliveryResult {
        saved_path,
        copied_to_clipboard,
        typed_at_cursor,
        warnings,
    })
}

fn save_file(text: &str, format: OutputFormat, settings: &AppSettings) -> Result<PathBuf, Failure> {
    let directory = settings
        .output_directory
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            Failure::guidance(
                "no_output_folder",
                "Choose a default output folder in Settings",
            )
        })?;
    fs::create_dir_all(&directory).map_err(|error| {
        Failure::io(
            "output_folder",
            &error,
            format!("Could not create the output folder: {error}"),
        )
    })?;

    let timestamp = Local::now().format("%Y-%m-%d-%H%M%S-%3f");
    let file_name = format!("utterform-{timestamp}.{}", format.extension());
    let destination = directory.join(file_name);
    let temporary = destination.with_extension(format!("{}.part", format.extension()));
    fs::write(&temporary, text.as_bytes()).map_err(|error| {
        Failure::io(
            "output_write",
            &error,
            format!("Could not write the output file: {error}"),
        )
    })?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(Failure::io(
            "output_finalize",
            &error,
            format!("Could not finalize the output file: {error}"),
        ));
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
                                    1,
                                    &request,
                                    "paste",
                                    || {
                                        assert!(clipboard_requested);
                                        if clipboard_succeeds {
                                            Ok(())
                                        } else {
                                            Err(Failure::new("clipboard", "unavailable"))
                                        }
                                    },
                                    || {
                                        assert!(file_requested);
                                        if file_succeeds {
                                            Ok(PathBuf::from("note.txt"))
                                        } else {
                                            Err(Failure::new("output_write", "unwritable"))
                                        }
                                    },
                                    |clipboard_holds_text| {
                                        assert!(typing_requested);
                                        assert_eq!(
                                            clipboard_holds_text,
                                            clipboard_requested && clipboard_succeeds,
                                            "paste delivery must know whether the clipboard already holds the text"
                                        );
                                        if typing_succeeds {
                                            Ok(())
                                        } else {
                                            Err(Failure::new("typing", "wtype is not installed"))
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

    /// The transcript reaches every output and no log line: not in a success,
    /// not in a failure, not in a saved file's name.
    #[test]
    fn delivery_events_never_carry_the_text_or_the_folder() {
        let transcript = "CANARY dictated: the merger closes on Friday, tell nobody";
        let ((), records) = diagnostics::capture::records(|| {
            let request = request(true, true, true);
            let result = deliver_to(
                42,
                &request,
                "keystrokes",
                || {
                    Err(Failure::new(
                        "clipboard",
                        format!("could not copy {transcript}"),
                    ))
                },
                || {
                    Ok(PathBuf::from(
                        "/home/someone/Private Notes/utterform-2026.txt",
                    ))
                },
                |_| {
                    Err(Failure::new(
                        "typing",
                        format!("typed {transcript} nowhere"),
                    ))
                },
            )
            .unwrap();
            assert_eq!(result.warnings.len(), 2);
            // The user still reads the whole reason, with its reference.
            assert!(result.warnings[0].contains("CANARY") && result.warnings[0].contains("(ref "));
        });
        assert_eq!(records.len(), 3, "{records:#?}");
        for record in &records {
            assert!(!record.contains("CANARY"), "{record}");
            assert!(!record.contains("merger"), "{record}");
            assert!(!record.contains("Private Notes"), "{record}");
            assert!(!record.contains("someone"), "{record}");
            assert!(record.contains("recording=42"), "{record}");
        }
        assert!(records[2].contains("delivery.completed"));
        assert!(records[2].contains("file_name=utterform-2026.txt"));
        assert!(records[2].contains("clipboard=failed file=ok cursor=failed"));
    }

    #[test]
    fn extensions_are_stable() {
        assert_eq!(OutputFormat::Txt.extension(), "txt");
        assert_eq!(OutputFormat::Md.extension(), "md");
    }
}
