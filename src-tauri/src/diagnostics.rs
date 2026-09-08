//! A log file for what a hidden window cannot show.
//!
//! Utterform is mostly used with its window out of sight, and a Windows
//! release build has no console at all, so `eprintln!` reaches nobody there.
//! The questions that come back from a real machine — did the start cue play,
//! on which device, how long did it take, which paste did the terminal get —
//! are answered by a line each in this file. Settings shows where it is.
//!
//! Everything still goes to stderr as well, which is what a Linux terminal or
//! a CI log sees.

use std::{
    fmt::Display,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use tauri::{AppHandle, Manager, Runtime};

const FILE_NAME: &str = "utterform.log";

/// Start over once the file reaches this size. A log that grows for months
/// helps nobody, and the lines that matter are the most recent ones.
const MAX_BYTES: u64 = 1024 * 1024;

static FILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<PathBuf>> {
    FILE.get_or_init(|| Mutex::new(None))
}

/// Decide where the log lives: next to the history, in the app's local data
/// directory. Called once at startup; until then lines go to stderr only.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let Ok(directory) = app.path().app_local_data_dir() else {
        return;
    };
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    if let Ok(mut slot) = slot().lock() {
        *slot = Some(directory.join(FILE_NAME));
    }
    log(format!(
        "Utterform {} started on {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    ));
}

/// Where the log is written, for Settings to show. `None` before `install`,
/// or when no data directory could be found.
pub fn path() -> Option<PathBuf> {
    slot().lock().ok()?.clone()
}

/// Write one line, timestamped, to the log file and to stderr.
pub fn log(message: impl Display) {
    let line = format!(
        "{} {message}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );
    eprintln!("utterform: {message}");
    let Some(path) = path() else {
        return;
    };
    // Best-effort by design: a log that cannot be written must not become an
    // error in the path it was meant to explain.
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_BYTES) {
        let _ = fs::remove_file(&path);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}
