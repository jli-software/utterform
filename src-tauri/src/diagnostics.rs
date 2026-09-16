//! Local diagnostics for what a hidden window cannot show.
//!
//! Utterform mostly runs with its window out of sight, and a Windows release
//! build has no console at all. The questions that come back from a real
//! machine — did the start cue play, on which device, which stage of a
//! recording failed, did the previous run end cleanly — are answered by the
//! events written here, and nowhere else: this module is the one place that
//! knows how an event is named, formatted, redacted, bounded and stored.
//!
//! **Where.** `tauri-plugin-log` supplies the filtered logger and stderr
//! target; the bounded writer below stores `utterform.log` in Tauri's log
//! directory. At 2 MiB the file is rotated to `.1.log`; `.2.log`
//! is the one older file, so the log never takes more than about 6 MiB. The
//! 0.7.8 `utterform.log` in app-local-data is left in place and read, never
//! written.
//!
//! **What.** One line per event: a stable dotted name (`recording.opened`)
//! followed by `key=value` fields — identifiers, durations, counts, modes,
//! device and application names, HTTP statuses, error classes. Never a
//! transcript, prompt, vocabulary, clipboard or file content, key, window
//! title or response body: a [`Failure`] carries what the user reads apart
//! from what the log may say, and has no `Display` to be logged by mistake.
//! Every line is still sanitized once more on its way to disk and again when
//! it is copied.
//!
//! **When.** At boundaries and outcomes, never from an audio callback, a
//! polling success, an animation frame or a retry iteration; a repeated
//! source goes through a [`Limiter`]. Writing is best-effort: a log that
//! cannot be written never fails the work it describes.
//!
//! Nothing here sends anything anywhere. The report only reaches the
//! clipboard when the user asks for it.

mod redact;
pub mod report;

use std::{
    cell::Cell,
    fmt::Display,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, SecondsFormat, Utc};
use log::{Level, LevelFilter};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_log::{Target, TargetKind};
use tauri_plugin_opener::OpenerExt;

/// `utterform.log`, rotated to `utterform.1.log` and `utterform.2.log`.
pub const FILE_STEM: &str = "utterform";
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Rotated files kept next to the active one. Never unbounded.
pub const KEPT_ROTATED_FILES: usize = 2;
/// Every Rust event is written under this target.
pub const RUST_TARGET: &str = "utterform";
/// The one target the WebView reaches, through [`record_frontend_error`].
pub const FRONTEND_TARGET: &str = "utterform::frontend";
/// The 0.7.8 log, next to the history.
const LEGACY_FILE_NAME: &str = "utterform.log";
/// Present while a run is in progress; left behind by one that did not end.
const RUN_MARKER: &str = "utterform.running";
/// A single record, backtraces included, never exceeds this.
pub(crate) const MAX_RECORD_BYTES: usize = 16 * 1024;
const MAX_VALUE_BYTES: usize = 1024;
const MAX_BACKTRACE_BYTES: usize = 8 * 1024;
/// Reports the WebView may make in one run before the rest are only counted.
const FRONTEND_REPORT_LIMIT: usize = 50;

// ---------------------------------------------------------------------------
// Identity

/// This run of the application: eight hex digits, written on every line so a
/// rotated file shared by several runs still says which line is whose.
pub fn run_id() -> &'static str {
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let local = 0_u8;
        let digest = Sha256::digest(format!(
            "{nanos}:{}:{:p}",
            std::process::id(),
            &local as *const u8
        ));
        digest[..4]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    })
}

static NEXT_RECORDING: AtomicU64 = AtomicU64::new(1);
static NEXT_REFERENCE: AtomicU64 = AtomicU64::new(1);

/// A new recording's diagnostic id, shared by batch and Live recordings so the
/// two never reuse a number within a run. Distinct from the `Instant` that
/// identifies a capture session: this one is only ever logged.
pub fn next_recording_id() -> u64 {
    NEXT_RECORDING.fetch_add(1, Ordering::Relaxed)
}

/// A short reference a user can quote, unique in the retained logs:
/// `<run>-<n>`.
pub fn next_reference() -> String {
    format!(
        "{}-{}",
        run_id(),
        NEXT_REFERENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// The message a user reads, with the reference that finds its log event.
pub fn with_reference(message: &str, reference: &str) -> String {
    format!("{message} (ref {reference})")
}

// ---------------------------------------------------------------------------
// Events

/// One field of an event. Values are rendered with `Display`.
pub type Field<'a> = (&'a str, &'a dyn Display);

/// Writes one event at `level`. Prefer the `info!`, `warning!` and `error!`
/// macros, which name the fields.
pub fn emit(level: Level, event: &str, fields: &[Field<'_>]) {
    emit_to(RUST_TARGET, level, event, fields);
}

fn emit_to(target: &str, level: Level, event: &str, fields: &[Field<'_>]) {
    #[cfg(test)]
    let capturing = capture::active();
    #[cfg(not(test))]
    let capturing = false;
    if !capturing && !log::log_enabled!(target: target, level) {
        return;
    }
    let message = event_message(event, fields);
    #[cfg(test)]
    capture::push(format_record(Utc::now(), run_id(), level, target, &message));
    log::log!(target: target, level, "{message}");
}

/// `event key=value key="quoted value"`, sanitized field by field.
pub fn event_message(event: &str, fields: &[Field<'_>]) -> String {
    let mut message = redact::line(event, 128);
    for (key, value) in fields {
        // A backtrace or a stack is only useful past its first frames; the
        // record as a whole stays within `MAX_RECORD_BYTES` regardless.
        let bound = match *key {
            "backtrace" | "stack" => MAX_BACKTRACE_BYTES,
            _ => MAX_VALUE_BYTES,
        };
        message.push(' ');
        message.push_str(key);
        message.push('=');
        message.push_str(&quote(&value.to_string(), bound));
    }
    message
}

/// A value is written bare when it cannot be misread, quoted otherwise.
fn quote(value: &str, max_bytes: usize) -> String {
    let value = redact::line(value, max_bytes);
    let bare = !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '=' | '\\'));
    if bare {
        return value;
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// The line as it is stored:
/// `2026-09-16T08:00:00.123Z INFO  utterform run=0a1b2c3d recording.opened …`.
pub fn format_record(
    timestamp: DateTime<Utc>,
    run: &str,
    level: Level,
    target: &str,
    message: &str,
) -> String {
    redact::line(
        &format!(
            "{} {:<5} {} run={} {}",
            timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
            level,
            target,
            run,
            message
        ),
        MAX_RECORD_BYTES,
    )
}

/// Only Utterform's own events are kept: the dependencies' log output is
/// neither useful to a report nor bounded by anything here.
pub fn accepts_target(target: &str) -> bool {
    [RUST_TARGET, "utterform_lib"].iter().any(|own| {
        target == *own
            || target
                .strip_prefix(own)
                .is_some_and(|rest| rest.starts_with("::"))
    })
}

macro_rules! event {
    ($level:expr, $event:expr $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::diagnostics::emit(
            $level,
            $event,
            &[$((stringify!($key), &$value as &dyn ::std::fmt::Display)),*],
        )
    };
}

/// A boundary or an outcome: `info!("recording.opened", recording = id)`.
macro_rules! info {
    ($($arguments:tt)*) => { $crate::diagnostics::event!(::log::Level::Info, $($arguments)*) };
}

/// Something went wrong that nobody is shown, or that was worked around.
macro_rules! warning {
    ($($arguments:tt)*) => { $crate::diagnostics::event!(::log::Level::Warn, $($arguments)*) };
}

/// A failure nobody is shown; one that is shown goes through `failure!`.
macro_rules! error {
    ($($arguments:tt)*) => { $crate::diagnostics::event!(::log::Level::Error, $($arguments)*) };
}

/// Logs the one owning `ERROR` event of a [`Failure`] and returns what the
/// user reads, with its reference. Guidance is logged as `INFO` without one.
macro_rules! failure {
    ($event:expr, $failure:expr $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::diagnostics::report(
            ::log::Level::Error,
            $event,
            $failure,
            &[$((stringify!($key), &$value as &dyn ::std::fmt::Display)),*],
        )
    };
}

/// Like `failure!`, at `WARN`: the work went on without this part.
macro_rules! fallback {
    ($event:expr, $failure:expr $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::diagnostics::report(
            ::log::Level::Warn,
            $event,
            $failure,
            &[$((stringify!($key), &$value as &dyn ::std::fmt::Display)),*],
        )
    };
}

#[allow(unused_imports)]
pub(crate) use {error, event, failure, fallback, info, warning};

// ---------------------------------------------------------------------------
// Failures

/// A failure on its way to the user: the text they read, kept apart from what
/// the log may say about it.
///
/// Deliberately not `Display`: the message can quote a third party (an OpenAI
/// error body), and nothing but [`report`] turns a failure into log text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    class: &'static str,
    message: String,
    status: Option<u16>,
    detail: Option<String>,
    guidance: bool,
}

impl Failure {
    /// Something went wrong. `class` is a short, stable category.
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
            status: None,
            detail: None,
            guidance: false,
        }
    }

    /// Something the user can act on — choose an output, add a key — rather
    /// than a failure. Logged without a reference, and not as an error.
    pub fn guidance(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            guidance: true,
            ..Self::new(class, message)
        }
    }

    /// A failed file operation, with the kind of error but never its path.
    pub fn io(class: &'static str, error: &io::Error, message: impl Into<String>) -> Self {
        Self::new(class, message).detail(io_detail(error))
    }

    pub fn status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Operating-system or library wording that names no user content: an
    /// error kind, a device error. Sanitized and bounded like every field.
    pub fn detail(mut self, detail: impl Display) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn class(&self) -> &'static str {
        self.class
    }

    /// The message alone, for a failure whose owning event was already written
    /// or that is not shown as a failure at all.
    pub fn into_message(self) -> String {
        self.message
    }
}

/// `NotFound`, or `PermissionDenied (os 13)`: what went wrong, not where.
pub fn io_detail(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!("{:?} (os {code})", error.kind()),
        None => format!("{:?}", error.kind()),
    }
}

/// The owning event of `failure`, and the text the user is shown.
pub fn report(level: Level, event: &str, failure: &Failure, fields: &[Field<'_>]) -> String {
    let mut all: Vec<Field<'_>> = fields.to_vec();
    all.push(("class", &failure.class));
    if let Some(status) = &failure.status {
        all.push(("status", status));
    }
    if let Some(detail) = &failure.detail {
        all.push(("detail", detail));
    }
    if failure.guidance {
        all.push(("outcome", &"guidance"));
        emit(Level::Info, event, &all);
        return failure.message.clone();
    }
    let reference = next_reference();
    all.push(("ref", &reference));
    emit(level, event, &all);
    with_reference(&failure.message, &reference)
}

// ---------------------------------------------------------------------------
// Bounds

/// Admits the first `limit` events of a repeated source and counts the rest,
/// so a chatty desktop or a failing loop cannot fill the log. Only Linux Live
/// input has such a source today; other platforms compile it for their tests.
#[derive(Debug)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct Limiter {
    limit: usize,
    admitted: usize,
    suppressed: usize,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl Limiter {
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            admitted: 0,
            suppressed: 0,
        }
    }

    pub fn admit(&mut self) -> bool {
        if self.admitted < self.limit {
            self.admitted += 1;
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    pub fn suppressed(&self) -> usize {
        self.suppressed
    }
}

/// A [`Limiter`] shared across threads for the lifetime of the process.
#[derive(Debug)]
pub struct SharedLimiter {
    limit: usize,
    seen: AtomicUsize,
}

impl SharedLimiter {
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            seen: AtomicUsize::new(0),
        }
    }

    pub fn admit(&self) -> bool {
        self.seen.fetch_add(1, Ordering::Relaxed) < self.limit
    }

    pub fn suppressed(&self) -> usize {
        self.seen.load(Ordering::Relaxed).saturating_sub(self.limit)
    }
}

// ---------------------------------------------------------------------------
// Installation

/// Where this run's diagnostics live, as Tauri resolved it.
#[derive(Debug, Clone)]
struct Location {
    /// `None` when no log file could be opened; events then reach stderr only.
    log_directory: Option<PathBuf>,
    /// App-local-data: the legacy log and the run marker.
    data_directory: Option<PathBuf>,
}

static LOCATION: OnceLock<Location> = OnceLock::new();
static PREVIOUS_RUN: OnceLock<PreviousRun> = OnceLock::new();
static FRONTEND_REPORTS: SharedLimiter = SharedLimiter::new(FRONTEND_REPORT_LIMIT);

fn level() -> LevelFilter {
    if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    }
}

fn logger_builder() -> tauri_plugin_log::Builder {
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .level(level())
        .filter(|metadata| accepts_target(metadata.target()))
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}",
                format_record(
                    Utc::now(),
                    run_id(),
                    record.level(),
                    record.target(),
                    &message.to_string()
                )
            ))
        })
        .target(Target::new(TargetKind::Stderr))
}

/// A record-at-a-time writer with a hard memory and disk bound.
///
/// `tauri-plugin-log` 2.9 rotates during a run, but its built-in writer keeps
/// every pending byte after a write or rotation error. A full disk or a log
/// directory removed under the application can therefore grow memory without
/// limit. Fern calls `flush` after every record, so this writer buffers at
/// most one bounded record and drops that record after a failed flush. Stderr
/// remains the independent second target.
struct BoundedLogFile {
    directory: PathBuf,
    active: PathBuf,
    inner: Option<File>,
    current_size: u64,
    max_size: u64,
    pending: Vec<u8>,
}

impl BoundedLogFile {
    fn new(directory: PathBuf) -> io::Result<Self> {
        Self::with_limit(directory, MAX_FILE_BYTES)
    }

    fn with_limit(directory: PathBuf, max_size: u64) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let active = directory.join(format!("{FILE_STEM}.log"));
        let mut writer = Self {
            directory,
            active,
            inner: None,
            current_size: 0,
            max_size,
            pending: Vec::with_capacity(MAX_RECORD_BYTES + 1),
        };
        writer.open()?;
        if writer.current_size >= writer.max_size {
            writer.rotate()?;
        }
        Ok(writer)
    }

    fn rotated(&self, index: usize) -> PathBuf {
        self.directory.join(format!("{FILE_STEM}.{index}.log"))
    }

    fn open(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.active)?;
        self.current_size = file.metadata()?.len();
        self.inner = Some(file);
        Ok(())
    }

    /// `utterform.1.log` is newest and `.2.log` oldest. Destinations are
    /// removed first because Windows does not replace them with `rename`.
    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.inner.take() {
            let _ = file.flush();
        }
        let oldest = self.rotated(KEPT_ROTATED_FILES);
        match fs::remove_file(&oldest) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        for index in (1..KEPT_ROTATED_FILES).rev() {
            let from = self.rotated(index);
            let to = self.rotated(index + 1);
            match fs::rename(from, to) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        match fs::rename(&self.active, self.rotated(1)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.open()
    }
}

impl Write for BoundedLogFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let room = (MAX_RECORD_BYTES + 1).saturating_sub(self.pending.len());
        self.pending
            .extend_from_slice(&bytes[..bytes.len().min(room)]);
        // Fern expects the complete formatted write to be accepted. The
        // formatter already enforces this bound; the cap is defence in depth.
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        // Clear before every fallible operation: a failed destination loses
        // one diagnostic record, never accumulates unbounded process memory.
        let pending = std::mem::take(&mut self.pending);
        if self.inner.is_none() {
            self.open()?;
        }
        if self.current_size != 0 && self.current_size + pending.len() as u64 > self.max_size {
            self.rotate()?;
        }
        let file = self
            .inner
            .as_mut()
            .ok_or_else(|| io::Error::other("diagnostic log unavailable"))?;
        file.write_all(&pending)?;
        file.flush()?;
        self.current_size += pending.len() as u64;
        Ok(())
    }
}

/// Attaches the logger, the panic hook and the run marker, and writes
/// `app.started`. Called first in `setup`, once per process.
///
/// The logger is built through the plugin's `split` rather than registered as
/// a plugin: its only other content is a `log` command the WebView must not
/// reach, and a log directory that cannot be created costs the file, not the
/// launch — events then still reach stderr.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let resolved = app.path().app_log_dir().map_err(|error| error.to_string());
    let with_file = resolved.and_then(|directory| {
        BoundedLogFile::new(directory.clone())
            .map(|writer| (directory, writer))
            .map_err(|error| format!("{:?}", error.kind()))
    });
    let (log_directory, file_error) = match with_file {
        Ok((directory, writer)) => {
            let dispatch = tauri_plugin_log::fern::Dispatch::new().chain(
                tauri_plugin_log::fern::Output::writer(Box::new(writer), "\n"),
            );
            match logger_builder()
                .target(Target::new(TargetKind::Dispatch(dispatch)))
                .split(app)
            {
                Ok((_command_plugin, max_level, logger)) => {
                    let _ = tauri_plugin_log::attach_logger(max_level, logger);
                    (Some(directory), None)
                }
                Err(error) => (None, Some(format!("{error:?}"))),
            }
        }
        Err(error) => (None, Some(error)),
    };
    if log_directory.is_none()
        && let Ok((_command_plugin, max_level, logger)) = logger_builder().split(app)
    {
        let _ = tauri_plugin_log::attach_logger(max_level, logger);
    }
    install_panic_hook();
    let data_directory = app.path().app_local_data_dir().ok();
    let _ = LOCATION.set(Location {
        log_directory,
        data_directory: data_directory.clone(),
    });
    let (previous, marker) = match &data_directory {
        Some(directory) => begin_run(directory, run_id()),
        None => (
            PreviousRun::Unknown,
            Err(io::Error::from(io::ErrorKind::NotFound)),
        ),
    };
    info!(
        "app.started",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        desktop = desktop(),
        previous_run = previous.summary(),
    );
    if let PreviousRun::Unclean(run) = &previous {
        warning!(
            "app.previous_run_unclean",
            previous_run = run.as_deref().unwrap_or("unknown")
        );
    }
    if let Err(error) = &marker {
        warning!("app.run_marker_failed", detail = io_detail(error));
    }
    if file_error.is_some() {
        warning!("diagnostics.file_unavailable", detail = "unavailable");
    }
    let _ = PREVIOUS_RUN.set(previous);
}

/// A clean `RunEvent::Exit`: the last event of the run, and the marker gone.
pub fn finish_run() {
    info!(
        "app.exiting",
        frontend_reports_suppressed = FRONTEND_REPORTS.suppressed()
    );
    if let Some(directory) = LOCATION
        .get()
        .and_then(|location| location.data_directory.as_ref())
        && let Err(error) = end_run(directory, run_id())
    {
        warning!("app.run_marker_failed", detail = io_detail(&error));
    }
    log::logger().flush();
}

/// The desktop and session kind, which decide how typing and the tray work.
fn desktop() -> String {
    #[cfg(target_os = "linux")]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into());
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
        let hyprland = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some();
        format!(
            "{desktop}/{session}{}",
            if hyprland { " hyprland" } else { "" }
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::consts::OS.to_string()
    }
}

// ---------------------------------------------------------------------------
// Clean and unclean exits

/// How the previous run ended, as far as a marker file can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviousRun {
    /// No marker: the previous run reached a clean exit, or this is the first.
    Clean,
    /// A marker was left behind — a crash, a killed process, a power loss or a
    /// native fault. Which one cannot be told from here.
    Unclean(Option<String>),
    /// The marker could not be read.
    Unknown,
}

impl PreviousRun {
    fn summary(&self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Unclean(_) => "unclean",
            Self::Unknown => "unknown",
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Clean => "clean".into(),
            Self::Unclean(Some(run)) => format!("unclean (run {run} did not reach a clean exit)"),
            Self::Unclean(None) => "unclean".into(),
            Self::Unknown => "unknown (the run marker could not be read)".into(),
        }
    }
}

/// Reads the previous run's marker and atomically replaces it with this
/// run's. Never fails the caller: the outcome of the write comes back beside
/// what was found.
pub fn begin_run(directory: &Path, run: &str) -> (PreviousRun, io::Result<()>) {
    let marker = directory.join(RUN_MARKER);
    let previous = match fs::read_to_string(&marker) {
        Ok(contents) => PreviousRun::Unclean(
            contents
                .lines()
                .find_map(|line| line.strip_prefix("run="))
                .map(|run| redact::line(run, 32)),
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => PreviousRun::Clean,
        Err(_) => PreviousRun::Unknown,
    };
    let written = fs::create_dir_all(directory).and_then(|()| {
        let temporary = directory.join(format!("{RUN_MARKER}.tmp"));
        fs::write(&temporary, format!("run={run}\n"))?;
        fs::rename(&temporary, &marker)
    });
    (previous, written)
}

/// Removes the marker if it is this run's. A marker another run wrote is left
/// alone, and a missing one is not an error.
pub fn end_run(directory: &Path, run: &str) -> io::Result<()> {
    let marker = directory.join(RUN_MARKER);
    match fs::read_to_string(&marker) {
        Ok(contents) if contents.lines().any(|line| line == format!("run={run}")) => {
            fs::remove_file(marker)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

// ---------------------------------------------------------------------------
// Panics

/// Records a Rust panic before the previous hook prints and the thread
/// unwinds or the process aborts. An abort, a segmentation fault or a native
/// driver crash never reaches a hook; those show up only as an unclean exit.
pub fn install_panic_hook() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            thread_local! {
                static IN_HOOK: Cell<bool> = const { Cell::new(false) };
            }
            // A panic while recording a panic must not recurse.
            if !IN_HOOK.with(|flag| flag.replace(true)) {
                let payload = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|text| text.to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-text panic payload".into());
                let location = info.location().map_or_else(
                    || "unknown".to_string(),
                    |location| {
                        format!(
                            "{}:{}:{}",
                            redact::source_path(location.file()),
                            location.line(),
                            location.column()
                        )
                    },
                );
                let thread = std::thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .to_string();
                let backtrace = std::backtrace::Backtrace::force_capture();
                error!(
                    "app.panicked",
                    thread = thread,
                    location = location,
                    message = panic_message(&payload),
                    backtrace = compact_backtrace(&backtrace.to_string()),
                );
                log::logger().flush();
                IN_HOOK.with(|flag| flag.set(false));
            }
            previous(info);
        }));
    });
}

/// A panic message, without the text Rust quotes when a string is sliced at
/// the wrong place — which could be a transcript. std words these as
/// "… is not a char boundary; it is inside 'x' (bytes 2..4) of `text`",
/// "byte index 9 is out of bounds of `text`" and
/// "begin <= end (5 <= 3) when slicing `text`"; everything from the quoted
/// character or string on is dropped. Other messages, such as an `unwrap`
/// naming `Result::unwrap()`, keep their backticks.
pub fn panic_message(payload: &str) -> String {
    let mut message = payload.to_string();
    let quoted = if let Some(index) = message.find("is not a char boundary") {
        Some(index + "is not a char boundary".len())
    } else if message.contains("out of bounds of `") || message.contains("when slicing `") {
        message.find('`')
    } else {
        None
    };
    if let Some(index) = quoted {
        message.truncate(index);
        message = format!("{} [text omitted]", message.trim_end());
    }
    redact::line(&message, 512)
}

/// Frames of the panic machinery itself, which say nothing about the cause.
const PANIC_MACHINERY: &[&str] = &[
    "std::backtrace",
    "std::panicking",
    "std::panic::",
    "std::sys::backtrace",
    "core::panicking",
    "core::option::unwrap_failed",
    "core::result::unwrap_failed",
    "__rustc::rust_begin_unwind",
    "rust_begin_unwind",
    "diagnostics::install_panic_hook",
    "<alloc::boxed::Box<F,A> as core::ops::function::Fn",
];

/// One line: frames separated by ` | `, the panic machinery left out, build
/// paths reduced, bounded — so the frames that matter fit.
pub fn compact_backtrace(backtrace: &str) -> String {
    let mut frames: Vec<Vec<String>> = Vec::new();
    for line in backtrace
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        match line.strip_prefix("at ") {
            Some(path) => {
                if let Some(frame) = frames.last_mut() {
                    frame.push(format!("at {}", redact::source_path(path)));
                }
            }
            None => frames.push(vec![line.to_string()]),
        }
    }
    let kept = frames
        .into_iter()
        .filter(|frame| {
            let function = frame[0]
                .split_once(": ")
                .map_or(frame[0].as_str(), |(_, name)| name);
            !PANIC_MACHINERY
                .iter()
                .any(|machinery| function.contains(machinery))
        })
        .map(|frame| frame.join(" "))
        .collect::<Vec<_>>()
        .join(" | ");
    redact::truncate(&kept, MAX_BACKTRACE_BYTES)
}

// ---------------------------------------------------------------------------
// The WebView's failures

/// What the interface may say about an error it could not hand to a command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendError {
    pub source: String,
    pub message: String,
    #[serde(default)]
    pub stack: Option<String>,
    pub phase: String,
}

const FRONTEND_SOURCES: &[&str] = &[
    "window.error",
    "unhandled_rejection",
    "autostart",
    "dialog",
    "state",
];
const FRONTEND_PHASES: &[&str] = &[
    "idle",
    "starting",
    "recording",
    "paused",
    "processing",
    "done",
    "error",
];

/// Records one frontend failure and returns its reference, or `None` once the
/// run's allowance is spent. Source and phase are taken only from their known
/// values; the message and stack lose their origins and are bounded.
pub fn record_frontend_error(report: &FrontendError) -> Option<String> {
    if !FRONTEND_REPORTS.admit() {
        return None;
    }
    let reference = next_reference();
    let source = known(&report.source, FRONTEND_SOURCES);
    let phase = known(&report.phase, FRONTEND_PHASES);
    let message = frontend_text(&report.message, 512);
    let stack = report
        .stack
        .as_deref()
        .map(|stack| frontend_text(stack, 4096))
        .unwrap_or_default();
    emit_to(
        FRONTEND_TARGET,
        Level::Error,
        "frontend.error",
        &[
            ("source", &source),
            ("phase", &phase),
            ("message", &message),
            ("stack", &stack),
            ("ref", &reference),
        ],
    );
    Some(reference)
}

fn known(value: &str, allowed: &[&'static str]) -> &'static str {
    allowed
        .iter()
        .find(|candidate| **candidate == value)
        .copied()
        .unwrap_or("other")
}

/// Script locations keep their file, line and column; the origin serving
/// them (`http://tauri.localhost/`, `tauri://localhost/`) is dropped.
pub fn frontend_text(text: &str, max_bytes: usize) -> String {
    let mut reduced = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("://") {
        let scheme_start = rest[..index]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
            .map_or(0, |position| position + 1);
        reduced.push_str(&rest[..scheme_start]);
        let after = &rest[index + 3..];
        let host_end = after
            .find(|c: char| c == '/' || c.is_whitespace() || matches!(c, ')' | '"' | '\''))
            .unwrap_or(after.len());
        let path = &after[host_end..];
        // `file:///home/…` has no host: its path stays whole, for redaction.
        rest = if host_end == 0 {
            path
        } else {
            path.strip_prefix('/').unwrap_or(path)
        };
    }
    reduced.push_str(rest);
    redact::line(&reduced, max_bytes)
}

/// Emits a native event to the interface, recording a failure to do so.
pub fn notify_interface<R: Runtime, S: Serialize + Clone>(
    app: &AppHandle<R>,
    event: &'static str,
    payload: S,
) {
    use tauri::Emitter;
    if app.emit(event, payload).is_err() {
        warning!("interface.event_failed", event = event);
    }
}

// ---------------------------------------------------------------------------
// Support actions

/// What Settings shows about the log.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// The actual file being written, or `None` when only stderr is used.
    pub log_path: Option<String>,
}

pub fn support_info() -> Info {
    Info {
        log_path: current_log().map(|path| path.display().to_string()),
    }
}

fn current_log() -> Option<PathBuf> {
    LOCATION
        .get()?
        .log_directory
        .as_ref()
        .map(|directory| directory.join(format!("{FILE_STEM}.log")))
}

/// What was put on the clipboard, for the confirmation Settings shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copied {
    pub bytes: usize,
    pub lines: usize,
    pub truncated: bool,
}

/// Builds the bounded report from the log tails. Reads log files only.
pub fn build_report() -> report::Report {
    let location = LOCATION.get();
    let legacy = location
        .and_then(|location| location.data_directory.as_ref())
        .map(|directory| directory.join(LEGACY_FILE_NAME));
    let files = match location.and_then(|location| location.log_directory.as_ref()) {
        Some(directory) => report::log_files(directory, FILE_STEM, legacy.as_deref()),
        None => legacy
            .filter(|path| path.is_file())
            .map(|path| {
                vec![report::LogFile {
                    origin: report::Origin::Legacy,
                    path,
                }]
            })
            .unwrap_or_default(),
    };
    let header = report::Header {
        generated: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        desktop: desktop(),
        run: run_id().into(),
        previous_run: PREVIOUS_RUN
            .get()
            .map_or_else(|| "unknown".into(), PreviousRun::describe),
        log_path: current_log()
            .map_or_else(|| "unavailable".into(), |path| path.display().to_string()),
        frontend_reports_suppressed: FRONTEND_REPORTS.suppressed(),
    };
    report::build(&header, &files, report::LIMIT)
}

/// **Copy diagnostics**: the report, on the clipboard. The one explicit action
/// that replaces what the clipboard held.
pub async fn copy_report<R: Runtime>(app: &AppHandle<R>) -> Result<Copied, String> {
    let report = tauri::async_runtime::spawn_blocking(build_report)
        .await
        .map_err(|_| {
            failure!(
                "support.copy_failed",
                &Failure::new("worker", "Diagnostics could not be collected")
            )
        })?;
    let copied = Copied {
        bytes: report.bytes(),
        lines: report.lines,
        truncated: report.truncated,
    };
    app.clipboard().write_text(report.text).map_err(|_| {
        failure!(
            "support.copy_failed",
            &Failure::new(
                "clipboard",
                "Diagnostics could not be copied to the clipboard"
            )
        )
    })?;
    info!(
        "support.diagnostics_copied",
        bytes = copied.bytes,
        lines = copied.lines,
        truncated = copied.truncated
    );
    Ok(copied)
}

/// **Open log file**: the backend's own file, in the default application.
pub async fn open_log_file<R: Runtime>(_app: &AppHandle<R>) -> Result<(), String> {
    let path = log_file_for_support("file")?;
    let opened = tauri::async_runtime::spawn_blocking(move || {
        tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|error| opener_detail(&error))
    })
    .await;
    match opened {
        Ok(Ok(())) => {
            info!("support.log_opened", target = "file");
            Ok(())
        }
        Ok(Err(detail)) => Err(failure!(
            "support.open_failed",
            &Failure::new("opener", "The log file could not be opened").detail(detail),
            target = "file"
        )),
        Err(_) => Err(failure!(
            "support.open_failed",
            &Failure::new("worker", "The log file could not be opened"),
            target = "file"
        )),
    }
}

/// **Open log folder**: the same file, selected in the file manager, or its
/// folder where the desktop offers no way to select a file.
pub async fn open_log_folder<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let path = log_file_for_support("folder")?;
    let handle = app.clone();
    let revealed = tauri::async_runtime::spawn_blocking(move || {
        match handle.opener().reveal_item_in_dir(&path) {
            Ok(()) => Ok("reveal"),
            Err(reveal) => {
                let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
                tauri_plugin_opener::open_path(&folder, None::<&str>)
                    .map(|()| "open_folder")
                    .map_err(|open| (opener_detail(&reveal), opener_detail(&open)))
            }
        }
    })
    .await;
    match revealed {
        Ok(Ok(how)) => {
            info!("support.log_opened", target = "folder", how = how);
            Ok(())
        }
        Ok(Err((reveal, open))) => Err(failure!(
            "support.open_failed",
            &Failure::new("opener", "The log folder could not be opened")
                .detail(format!("reveal {reveal}, open {open}")),
            target = "folder"
        )),
        Err(_) => Err(failure!(
            "support.open_failed",
            &Failure::new("worker", "The log folder could not be opened"),
            target = "folder"
        )),
    }
}

fn log_file_for_support(target: &'static str) -> Result<PathBuf, String> {
    match current_log() {
        Some(path) if path.is_file() => Ok(path),
        Some(_) => Err(failure!(
            "support.open_failed",
            &Failure::new("missing_file", "The log file does not exist yet"),
            target = target
        )),
        None => Err(failure!(
            "support.open_failed",
            &Failure::new(
                "no_log_file",
                "Utterform could not open a log file in this session"
            ),
            target = target
        )),
    }
}

/// The opener's error kind, without the path it was given.
fn opener_detail(error: &tauri_plugin_opener::Error) -> String {
    match error {
        tauri_plugin_opener::Error::Io(error) => io_detail(error),
        other => format!("{other:?}")
            .split(['(', ' ', '{'])
            .next()
            .unwrap_or("other")
            .to_string(),
    }
}

// ---------------------------------------------------------------------------
// A sink for tests

#[cfg(test)]
pub(crate) mod capture {
    use std::cell::RefCell;

    thread_local! {
        static LINES: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    }

    pub fn active() -> bool {
        LINES.with(|lines| lines.borrow().is_some())
    }

    pub fn push(line: String) {
        LINES.with(|lines| {
            if let Some(lines) = lines.borrow_mut().as_mut() {
                lines.push(line);
            }
        });
    }

    /// Every record written on this thread while `work` runs, formatted as it
    /// would be stored. Parallel tests do not see each other's events.
    pub fn records<T>(work: impl FnOnce() -> T) -> (T, Vec<String>) {
        LINES.with(|lines| *lines.borrow_mut() = Some(Vec::new()));
        let value = work();
        let lines = LINES.with(|lines| lines.borrow_mut().take().unwrap_or_default());
        (value, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-16T08:00:00.123Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_record_is_stable_one_line_and_names_level_event_run_and_correlation() {
        let message = event_message(
            "recording.opened",
            &[
                ("recording", &7),
                ("device", &"Studio microphone\nwith a break"),
                ("ref", &"0a1b2c3d-4"),
                ("empty", &""),
                ("quote", &"say \"hi\" = 1"),
            ],
        );
        let record = format_record(timestamp(), "0a1b2c3d", Level::Info, RUST_TARGET, &message);
        assert_eq!(
            record,
            "2026-09-16T08:00:00.123Z INFO  utterform run=0a1b2c3d recording.opened recording=7 \
             device=\"Studio microphone with a break\" ref=0a1b2c3d-4 empty=\"\" \
             quote=\"say \\\"hi\\\" = 1\""
        );
        assert_eq!(record.lines().count(), 1);
        let frontend = format_record(
            timestamp(),
            "0a1b2c3d",
            Level::Error,
            FRONTEND_TARGET,
            "frontend.error",
        );
        assert!(frontend.starts_with(
            "2026-09-16T08:00:00.123Z ERROR utterform::frontend run=0a1b2c3d frontend.error"
        ));
    }

    #[test]
    fn records_redact_canaries_and_stay_bounded() {
        let canary_key = "sk-proj-CANARYKEY0123456789abcdefghij";
        let bearer = format!("Bearer {canary_key}");
        let long = "x".repeat(100_000);
        let message = event_message(
            "test.event",
            &[
                ("path", &"/home/someone/Documents/private.md"),
                ("auth", &bearer),
                ("long", &long),
            ],
        );
        let record = format_record(timestamp(), "run", Level::Warn, RUST_TARGET, &message);
        assert!(!record.contains("CANARYKEY"), "{record}");
        assert!(!record.contains("someone"), "{record}");
        assert!(record.contains("$HOME/Documents/private.md"));
        assert!(record.len() <= MAX_RECORD_BYTES);
    }

    #[test]
    fn the_disk_budget_is_the_active_file_and_two_rotated_ones() {
        assert_eq!(MAX_FILE_BYTES, 2 * 1024 * 1024);
        assert_eq!(KEPT_ROTATED_FILES, 2);
        assert!((1 + KEPT_ROTATED_FILES as u64) * MAX_FILE_BYTES <= 6 * 1024 * 1024);
        assert!((MAX_RECORD_BYTES as u64) < MAX_FILE_BYTES);
        assert_eq!(report::LIMIT, 256 * 1024);
    }

    #[test]
    fn the_file_writer_rotates_during_a_run_and_keeps_exactly_two_archives() {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = BoundedLogFile::with_limit(directory.path().to_path_buf(), 8).unwrap();
        for line in ["aa1\n", "bb2\n", "cc3\n", "dd4\n", "ee5\n", "ff6\n"] {
            writer.write_all(line.as_bytes()).unwrap();
            writer.flush().unwrap();
        }
        assert_eq!(
            fs::read_to_string(directory.path().join("utterform.log")).unwrap(),
            "ee5\nff6\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("utterform.1.log")).unwrap(),
            "cc3\ndd4\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("utterform.2.log")).unwrap(),
            "aa1\nbb2\n"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
    }

    #[test]
    fn a_failed_rotation_drops_one_record_instead_of_retaining_memory() {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = BoundedLogFile::with_limit(directory.path().to_path_buf(), 4).unwrap();
        writer.write_all(b"one\n").unwrap();
        writer.flush().unwrap();
        // A directory at the oldest archive path cannot be removed as a file.
        fs::create_dir(directory.path().join("utterform.2.log")).unwrap();
        writer.write_all(b"two\n").unwrap();
        assert!(writer.flush().is_err());
        assert!(writer.pending.is_empty());
        assert!(writer.pending.capacity() <= MAX_RECORD_BYTES + 1);
    }

    #[test]
    fn only_utterform_targets_are_accepted() {
        for target in [
            "utterform",
            "utterform::frontend",
            "utterform_lib",
            "utterform_lib::audio",
        ] {
            assert!(accepts_target(target), "{target}");
        }
        for target in [
            "tauri",
            "reqwest::connect",
            "webview",
            "utterformer",
            "tao::platform",
            "zbus",
        ] {
            assert!(!accepts_target(target), "{target}");
        }
    }

    #[test]
    fn success_fallback_failure_and_guidance_each_emit_exactly_one_event() {
        let canary = "CANARY transcript: the quarterly numbers are confidential";
        let ((), records) = capture::records(|| {
            info!("delivery.completed", recording = 3, clipboard = "ok");
        });
        assert_eq!(records.len(), 1);
        assert!(records[0].contains(" INFO  "));
        assert!(records[0].contains("delivery.completed recording=3"));

        let (shown, records) = capture::records(|| {
            fallback!(
                "transform.fallback",
                &Failure::new("http", format!("OpenAI said: {canary}")).status(429),
                recording = 3
            )
        });
        assert_eq!(records.len(), 1);
        assert!(records[0].contains(" WARN  ") && records[0].contains("status=429"));
        assert!(!records[0].contains("CANARY"), "{}", records[0]);
        assert!(shown.starts_with("OpenAI said: CANARY"));
        let reference = shown.rsplit("(ref ").next().unwrap().trim_end_matches(')');
        assert!(records[0].contains(&format!("ref={reference}")));

        let (shown, records) = capture::records(|| {
            failure!(
                "transcription.failed",
                &Failure::io(
                    "read",
                    &io::Error::from(io::ErrorKind::PermissionDenied),
                    canary
                ),
                recording = 4
            )
        });
        assert_eq!(records.len(), 1);
        assert!(records[0].contains(" ERROR ") && records[0].contains("detail=PermissionDenied"));
        assert!(!records[0].contains("CANARY"));
        assert!(shown.contains("(ref "));

        let (shown, records) = capture::records(|| {
            failure!(
                "recording.rejected",
                &Failure::guidance("api_key_missing", "Add an OpenAI API key in Settings")
            )
        });
        assert_eq!(records.len(), 1);
        assert!(records[0].contains(" INFO  ") && records[0].contains("outcome=guidance"));
        assert!(!records[0].contains("ref="));
        assert_eq!(shown, "Add an OpenAI API key in Settings");
    }

    #[test]
    fn a_limiter_stops_at_its_bound_and_counts_the_rest() {
        let mut limiter = Limiter::new(3);
        let admitted = (0..10).filter(|_| limiter.admit()).count();
        assert_eq!(admitted, 3);
        assert_eq!(limiter.suppressed(), 7);
        let shared = SharedLimiter::new(2);
        assert!(shared.admit() && shared.admit());
        assert!(!shared.admit() && !shared.admit());
        assert_eq!(shared.suppressed(), 2);
    }

    #[test]
    fn identifiers_are_distinct_and_references_name_the_run() {
        let first = next_recording_id();
        assert!(next_recording_id() > first);
        let reference = next_reference();
        assert!(reference.starts_with(&format!("{}-", run_id())));
        assert_ne!(reference, next_reference());
        assert_eq!(run_id().len(), 8);
        assert_eq!(with_reference("Failed", "ab-1"), "Failed (ref ab-1)");
    }

    #[test]
    fn the_run_marker_tells_clean_unclean_and_unavailable_apart() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("data");
        let (previous, written) = begin_run(&data, "aaaa0001");
        assert_eq!(previous, PreviousRun::Clean);
        written.unwrap();
        // A run that never reached its exit leaves the marker behind.
        let (previous, written) = begin_run(&data, "bbbb0002");
        assert_eq!(previous, PreviousRun::Unclean(Some("aaaa0001".into())));
        written.unwrap();
        // Only the run that wrote the marker removes it; twice is harmless.
        end_run(&data, "aaaa0001").unwrap();
        assert!(data.join(RUN_MARKER).is_file());
        end_run(&data, "bbbb0002").unwrap();
        assert!(!data.join(RUN_MARKER).exists());
        end_run(&data, "bbbb0002").unwrap();
        assert_eq!(begin_run(&data, "cccc0003").0, PreviousRun::Clean);
        // Storage that is not a directory: unknown, and an error to log, not a panic.
        let blocked = directory.path().join("file");
        fs::write(&blocked, "not a directory").unwrap();
        let (previous, written) = begin_run(&blocked, "dddd0004");
        assert_eq!(previous, PreviousRun::Unknown);
        assert!(written.is_err());
        assert!(end_run(&blocked, "dddd0004").is_err());
    }

    #[test]
    fn a_panic_message_keeps_its_cause_but_not_quoted_text() {
        for (payload, kept) in [
            (
                "byte index 3 is not a char boundary; it is inside 'ü' (bytes 2..4) of `Grüße CANARY transcript`",
                "byte index 3 is not a char boundary [text omitted]",
            ),
            (
                "byte index 90 is out of bounds of `CANARY transcript`",
                "byte index 90 is out of bounds of [text omitted]",
            ),
            (
                "begin <= end (5 <= 3) when slicing `CANARY transcript`",
                "begin <= end (5 <= 3) when slicing [text omitted]",
            ),
        ] {
            assert_eq!(panic_message(payload), kept);
        }
        // Nothing is quoted from the user here, and the backticks explain it.
        assert_eq!(
            panic_message("called `Option::unwrap()` on a `None` value"),
            "called `Option::unwrap()` on a `None` value"
        );
    }

    #[test]
    fn a_backtrace_is_one_bounded_line_of_the_frames_that_matter() {
        let machinery = "   0: utterform_lib::diagnostics::install_panic_hook::{{closure}}\n             at ./src/diagnostics.rs:686:33\n   1: <alloc::boxed::Box<F,A> as core::ops::function::Fn<Args>>::call\n             at /rustc/254b/library/alloc/src/boxed.rs:2220:9\n   2: std::panicking::panic_with_hook\n   3: __rustc::rust_begin_unwind\n   4: core::panicking::panic_fmt\n";
        let cause = "   5: utterform_lib::audio::finalize\n             at /home/runner/work/utterform/src-tauri/src/audio.rs:600:5\n   6: std::rt::lang_start\n";
        let compact = compact_backtrace(&format!("{machinery}{cause}"));
        assert_eq!(compact.lines().count(), 1);
        assert!(
            compact
                .starts_with("5: utterform_lib::audio::finalize at src-tauri/src/audio.rs:600:5"),
            "{compact}"
        );
        assert!(!compact.contains("panicking") && !compact.contains("runner"));
        // A deep backtrace keeps up to its own bound in the stored record.
        let deep = (0..400)
            .map(|index| format!("  {index}: utterform_lib::deep::frame_{index}\n      at src-tauri/src/deep.rs:{index}:1"))
            .collect::<Vec<_>>()
            .join("\n");
        let backtrace = compact_backtrace(&deep);
        let message = event_message(
            "app.panicked",
            &[("message", &"boom"), ("backtrace", &backtrace)],
        );
        assert!(message.len() > MAX_VALUE_BYTES * 4, "{}", message.len());
        assert!(message.len() <= MAX_BACKTRACE_BYTES + 256);
        assert!(message.contains("frame_0 at src-tauri/src/deep.rs:0:1"));
    }

    #[test]
    fn frontend_reports_lose_origins_and_unknown_values() {
        let text = frontend_text(
            "TypeError: x is undefined\n    at start (http://tauri.localhost/assets/index-abc.js:1:2345)\n    at file:///home/jonas/app/src/App.svelte:10:2",
            4096,
        );
        assert!(!text.contains("tauri.localhost"), "{text}");
        assert!(text.contains("assets/index-abc.js:1:2345"));
        assert!(!text.contains("jonas"), "{text}");
        assert_eq!(text.lines().count(), 1);
        let report = FrontendError {
            source: "made up".into(),
            message: "sk-proj-CANARYKEY0123456789abcdefghij leaked".into(),
            stack: None,
            phase: "<script>".into(),
        };
        let (reference, records) = capture::records(|| record_frontend_error(&report));
        assert!(reference.is_some());
        assert_eq!(records.len(), 1);
        assert!(records[0].contains("utterform::frontend"));
        assert!(records[0].contains("source=other phase=other"));
        assert!(!records[0].contains("CANARYKEY"));
    }
}
