use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use crate::{
    diagnostics::{self, Failure},
    domain::LocalModelInfo,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

struct ModelDefinition {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    file_name: &'static str,
    url: &'static str,
    size_bytes: u64,
    sha256: &'static str,
}

const MODELS: &[ModelDefinition] = &[
    ModelDefinition {
        id: "tiny",
        name: "Whisper Tiny",
        description: "Fastest · suitable for quick drafts",
        file_name: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        size_bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    ModelDefinition {
        id: "base",
        name: "Whisper Base",
        description: "Balanced · recommended for most devices",
        file_name: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        size_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    ModelDefinition {
        id: "small",
        name: "Whisper Small",
        description: "More accurate · requires more memory and time",
        file_name: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        size_bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    },
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    model_id: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
}

fn model_directory(app: &AppHandle) -> Result<PathBuf, Failure> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("models"))
        .map_err(|error| {
            Failure::new(
                "model_path",
                format!("Could not resolve the model directory: {error}"),
            )
        })
}

fn definition(id: &str) -> Result<&'static ModelDefinition, Failure> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| Failure::guidance("unknown_model", format!("Unknown Whisper model: {id}")))
}

/// A catalog id as a diagnostic event may name it.
pub fn diagnostic_id(id: &str) -> &'static str {
    definition(id).map_or("other", |model| model.id)
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, Failure> {
    let model = definition(id)?;
    Ok(model_directory(app)?.join(model.file_name))
}

pub fn list(app: &AppHandle) -> Result<Vec<LocalModelInfo>, Failure> {
    let directory = model_directory(app)?;
    Ok(MODELS
        .iter()
        .map(|model| LocalModelInfo {
            id: model.id,
            name: model.name,
            description: model.description,
            size_bytes: model.size_bytes,
            downloaded: directory.join(model.file_name).is_file(),
        })
        .collect())
}

/// What kind of network failure a download met, without its URL.
fn network(error: reqwest::Error) -> Failure {
    let class = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_status() {
        "http"
    } else {
        "network"
    };
    let failure = Failure::new(class, format!("Model download failed: {error}"));
    match error.status() {
        Some(status) => failure.status(status.as_u16()),
        None => failure,
    }
}

pub async fn download(app: &AppHandle, id: &str) -> Result<(), Failure> {
    let model = definition(id)?;
    let directory = model_directory(app)?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| {
            Failure::io(
                "model_directory",
                &error,
                format!("Could not create the model directory: {error}"),
            )
        })?;
    let destination = directory.join(model.file_name);
    if destination.is_file() {
        return Ok(());
    }
    let partial = destination.with_extension("bin.part");
    let began = Instant::now();
    diagnostics::info!("models.download_started", model = model.id);
    // Progress reaches the interface once per chunk; a failure to deliver it
    // is counted and reported once, never per chunk.
    let mut undelivered_progress = 0_u64;

    let result = async {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(|error| {
                Failure::new(
                    "client_init",
                    format!("Could not initialize the download client: {error}"),
                )
            })?;
        let response = client
            .get(model.url)
            .send()
            .await
            .map_err(network)?
            .error_for_status()
            .map_err(network)?;
        let total = response.content_length().unwrap_or(model.size_bytes);
        let mut stream = response.bytes_stream();
        let mut file = tokio::fs::File::create(&partial).await.map_err(|error| {
            Failure::io(
                "model_write",
                &error,
                format!("Could not create the model file: {error}"),
            )
        })?;
        let mut downloaded = 0_u64;
        let mut hasher = Sha256::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(network)?;
            file.write_all(&chunk).await.map_err(|error| {
                Failure::io(
                    "model_write",
                    &error,
                    format!("Could not write the model file: {error}"),
                )
            })?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            if app
                .emit(
                    "model-download-progress",
                    DownloadProgress {
                        model_id: model.id,
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                    },
                )
                .is_err()
            {
                undelivered_progress += 1;
            }
        }
        file.flush().await.map_err(|error| {
            Failure::io(
                "model_write",
                &error,
                format!("Could not flush the model file: {error}"),
            )
        })?;
        drop(file);

        let actual = format!("{:x}", hasher.finalize());
        if actual != model.sha256 {
            return Err(Failure::new(
                "checksum",
                "The downloaded model failed SHA-256 verification",
            ));
        }
        tokio::fs::rename(&partial, &destination)
            .await
            .map_err(|error| {
                Failure::io(
                    "model_activate",
                    &error,
                    format!("Could not activate the model: {error}"),
                )
            })?;
        Ok(downloaded)
    }
    .await;

    if undelivered_progress > 0 {
        diagnostics::warning!(
            "interface.event_failed",
            event = "model-download-progress",
            count = undelivered_progress
        );
    }
    match result {
        Ok(bytes) => {
            diagnostics::info!(
                "models.download_completed",
                model = model.id,
                bytes = bytes,
                elapsed_ms = began.elapsed().as_millis()
            );
            Ok(())
        }
        Err(failure) => {
            if let Err(error) = tokio::fs::remove_file(&partial).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                diagnostics::warning!(
                    "models.partial_cleanup_failed",
                    model = model.id,
                    detail = diagnostics::io_detail(&error)
                );
            }
            Err(failure)
        }
    }
}

pub async fn delete(app: &AppHandle, id: &str) -> Result<(), Failure> {
    let path = model_path(app, id)?;
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Failure::io(
            "model_delete",
            &error,
            format!("Could not delete the model: {error}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_unique_and_paths_are_safe() {
        let mut ids = MODELS.iter().map(|model| model.id).collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len());
        assert!(MODELS.iter().all(|model| !model.file_name.contains('/')));
        assert!(MODELS.iter().all(|model| model.sha256.len() == 64));
    }
}
