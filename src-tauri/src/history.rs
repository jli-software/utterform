//! Device-local transcript history. Never syncs or makes network requests.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::domain::TranscriptionEngine;

const HISTORY_LIMIT: usize = 100;

#[derive(Default)]
pub struct HistoryState(Mutex<()>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    #[serde(default)]
    pub created_at_ms: Option<u64>,
    pub title: String,
    pub text: String,
    pub duration_ms: u64,
    pub engine: TranscriptionEngine,
}

impl HistoryEntry {
    pub fn new(text: &str, duration_ms: u64, engine: TranscriptionEngine) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            id: timestamp.as_nanos().to_string(),
            created_at_ms: Some(timestamp.as_millis() as u64),
            title: title_from_text(text),
            text: text.to_string(),
            duration_ms,
            engine,
        }
    }
}

fn title_from_text(text: &str) -> String {
    let words = text.split_whitespace().take(9).collect::<Vec<_>>();
    let title = words.join(" ");
    let title = title.trim_start_matches(['#', '*', '-', ' ']);
    if title.is_empty() {
        return "Untitled text".into();
    }
    let short = title.chars().take(64).collect::<String>();
    if title.chars().count() > 64 || text.split_whitespace().count() > 9 {
        format!("{}…", short.trim_end())
    } else {
        short
    }
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("history.json"))
        .map_err(|error| format!("Could not resolve the history directory: {error}"))
}

pub fn list(app: &AppHandle) -> Result<Vec<HistoryEntry>, String> {
    let state = app.state::<HistoryState>();
    let _guard = state.0.lock().map_err(|_| "History is unavailable")?;
    read_entries(&history_path(app)?)
}

pub fn append(app: &AppHandle, entry: HistoryEntry) -> Result<(), String> {
    let state = app.state::<HistoryState>();
    let _guard = state.0.lock().map_err(|_| "History is unavailable")?;
    append_at(&history_path(app)?, entry)
}

pub fn clear(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<HistoryState>();
    let _guard = state.0.lock().map_err(|_| "History is unavailable")?;
    write_entries(&history_path(app)?, &[])
}

fn read_entries(path: &Path) -> Result<Vec<HistoryEntry>, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut entries: Vec<HistoryEntry> = serde_json::from_slice(&bytes)
                .map_err(|error| format!("History is invalid (file left untouched): {error}"))?;
            entries.truncate(HISTORY_LIMIT);
            for entry in &mut entries {
                if entry.created_at_ms.is_none() {
                    // Beta 1/2 IDs are Unix nanoseconds. Recover dates without
                    // rewriting the original history merely by opening the app.
                    entry.created_at_ms = entry
                        .id
                        .parse::<u128>()
                        .ok()
                        .and_then(|nanos| u64::try_from(nanos / 1_000_000).ok());
                }
            }
            Ok(entries)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("Could not read history: {error}")),
    }
}

fn append_at(path: &Path, entry: HistoryEntry) -> Result<(), String> {
    // Refuse to overwrite unreadable/corrupt history. The result is still returned to the UI.
    let mut entries = read_entries(path)?;
    entries.insert(0, entry);
    entries.truncate(HISTORY_LIMIT);
    write_entries(path, &entries)
}

fn write_entries(path: &Path, entries: &[HistoryEntry]) -> Result<(), String> {
    let parent = path.parent().ok_or("History path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| format!("Could not create history: {error}"))?;
    // Same-directory, owner-only temporary file, flushed before atomic replacement.
    // Unlike delete-then-rename this leaves the previous history intact on failure.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("Could not prepare history: {error}"))?;
    let bytes = serde_json::to_vec(entries).map_err(|error| error.to_string())?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| format!("Could not write history: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("Could not save history: {error}"))?;
    #[cfg(unix)]
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("Could not flush history directory: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> HistoryEntry {
        HistoryEntry::new(text, 1234, TranscriptionEngine::LocalWhisper)
    }

    #[test]
    fn titles_are_local_short_and_unicode_safe() {
        assert_eq!(
            title_from_text("#  Plan für morgen\nüberarbeiten"),
            "Plan für morgen überarbeiten"
        );
        assert_eq!(title_from_text(" \n "), "Untitled text");
        assert_eq!(title_from_text(&"🦀".repeat(80)).chars().count(), 65);
        assert!(title_from_text("one two three four five six seven eight nine ten").ends_with('…'));
    }

    #[test]
    fn history_survives_reopening_and_keeps_newest_hundred() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        assert!(read_entries(&path).unwrap().is_empty());
        for index in 0..105 {
            append_at(&path, entry(&format!("Text {index}"))).unwrap();
        }
        let restored = read_entries(&path).unwrap();
        assert_eq!(restored.len(), HISTORY_LIMIT);
        assert_eq!(restored[0].text, "Text 104");
        assert_eq!(restored[99].text, "Text 5");
        assert_eq!(restored[0].duration_ms, 1234);
        write_entries(&path, &[]).unwrap();
        assert!(read_entries(&path).unwrap().is_empty());
    }

    #[test]
    fn legacy_history_recovers_dates_without_rewriting_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        let original = r#"[{"id":"1788705000123456789","title":"Old idea","text":"Old idea","durationMs":1000,"engine":"open_ai"}]"#;
        fs::write(&path, original).unwrap();
        let restored = read_entries(&path).unwrap();
        assert_eq!(restored[0].created_at_ms, Some(1_788_705_000_123));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let fresh = entry("New idea");
        assert_eq!(
            fresh.created_at_ms.unwrap() as u128,
            fresh.id.parse::<u128>().unwrap() / 1_000_000
        );
        append_at(&path, fresh.clone()).unwrap();
        let reopened = read_entries(&path).unwrap();
        assert_eq!(reopened[0].created_at_ms, fresh.created_at_ms);
        assert_eq!(reopened[1].created_at_ms, restored[0].created_at_ms);
    }

    #[test]
    fn invalid_history_is_never_silently_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        fs::write(&path, "broken original").unwrap();
        assert!(append_at(&path, entry("Keep this in the UI")).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "broken original");
    }

    #[cfg(unix)]
    #[test]
    fn history_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        append_at(&path, entry("Private text")).unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
