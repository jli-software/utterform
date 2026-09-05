use std::{path::PathBuf, time::Duration};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use crate::domain::LocalModelInfo;

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

fn model_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("models"))
        .map_err(|error| format!("Could not resolve the model directory: {error}"))
}

fn definition(id: &str) -> Result<&'static ModelDefinition, String> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("Unknown Whisper model: {id}"))
}

pub fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let model = definition(id)?;
    Ok(model_directory(app)?.join(model.file_name))
}

pub fn list(app: &AppHandle) -> Result<Vec<LocalModelInfo>, String> {
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

pub async fn download(app: &AppHandle, id: &str) -> Result<(), String> {
    let model = definition(id)?;
    let directory = model_directory(app)?;
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| format!("Could not create the model directory: {error}"))?;
    let destination = directory.join(model.file_name);
    if destination.is_file() {
        return Ok(());
    }
    let partial = destination.with_extension("bin.part");

    let result = async {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(|error| format!("Could not initialize the download client: {error}"))?;
        let response = client
            .get(model.url)
            .send()
            .await
            .map_err(|error| format!("Model download failed: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Model download failed: {error}"))?;
        let total = response.content_length().unwrap_or(model.size_bytes);
        let mut stream = response.bytes_stream();
        let mut file = tokio::fs::File::create(&partial)
            .await
            .map_err(|error| format!("Could not create the model file: {error}"))?;
        let mut downloaded = 0_u64;
        let mut hasher = Sha256::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| format!("Model download failed: {error}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("Could not write the model file: {error}"))?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            let _ = app.emit(
                "model-download-progress",
                DownloadProgress {
                    model_id: model.id,
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                },
            );
        }
        file.flush()
            .await
            .map_err(|error| format!("Could not flush the model file: {error}"))?;
        drop(file);

        let actual = format!("{:x}", hasher.finalize());
        if actual != model.sha256 {
            return Err("The downloaded model failed SHA-256 verification".to_string());
        }
        tokio::fs::rename(&partial, &destination)
            .await
            .map_err(|error| format!("Could not activate the model: {error}"))?;
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    result
}

pub async fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    let path = model_path(app, id)?;
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not delete the model: {error}")),
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
