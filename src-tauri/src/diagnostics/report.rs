//! The report **Copy diagnostics** puts on the clipboard: a short header and
//! the newest log lines that fit, oldest first, sanitized once more.
//!
//! Only log files are read — never settings, history, recordings, exported
//! documents or the clipboard — and only as much of their tails as the report
//! can hold.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use super::redact;

/// The copied text never exceeds this, header and marker included.
pub const LIMIT: usize = 256 * 1024;

/// Report format, so a reader can tell a later layout apart.
pub const FORMAT_VERSION: u32 = 1;

/// Where a set of lines came from, in the order they were written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `utterform.log` in app-local-data, written by 0.7.8 and earlier.
    Legacy,
    Rotated,
    Current,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::Legacy => "legacy log",
            Self::Rotated => "rotated log",
            Self::Current => "current log",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFile {
    pub origin: Origin,
    pub path: PathBuf,
}

/// The files a report can draw on, oldest first: the legacy log, the rotated
/// logs in the order they were rotated, then the file being written.
///
/// Numbered rotations are newest-first (`.1`, `.2`), while the returned list
/// is oldest-first so the report reads chronologically.
pub fn log_files(directory: &Path, stem: &str, legacy: Option<&Path>) -> Vec<LogFile> {
    let mut files = Vec::new();
    if let Some(legacy) = legacy.filter(|path| path.is_file()) {
        files.push(LogFile {
            origin: Origin::Legacy,
            path: legacy.to_path_buf(),
        });
    }
    let current_name = format!("{stem}.log");
    let rotated_prefix = format!("{stem}.");
    let mut rotated = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let index = name
                .strip_prefix(&rotated_prefix)?
                .strip_suffix(".log")?
                .parse::<usize>()
                .ok()?;
            entry.path().is_file().then(|| (index, entry.path()))
        })
        .collect::<Vec<_>>();
    rotated.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
    files.extend(rotated.into_iter().map(|(_, path)| LogFile {
        origin: Origin::Rotated,
        path,
    }));
    let current = directory.join(current_name);
    if current.is_file() {
        files.push(LogFile {
            origin: Origin::Current,
            path: current,
        });
    }
    files
}

/// What the header says about the running application.
#[derive(Debug, Clone)]
pub struct Header {
    pub generated: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub desktop: String,
    pub run: String,
    pub previous_run: String,
    pub log_path: String,
    pub frontend_reports_suppressed: usize,
}

impl Header {
    fn render(&self) -> String {
        format!(
            "Utterform diagnostics report\n\
             format: {FORMAT_VERSION}\n\
             generated: {}\n\
             version: {}\n\
             os: {}\n\
             arch: {}\n\
             desktop: {}\n\
             run: {}\n\
             previous run: {}\n\
             log file: {}\n\
             frontend reports suppressed: {}\n\
             Local diagnostics only: no transcripts, audio, prompts, clipboard contents or keys.\n",
            line(&self.generated),
            line(&self.version),
            line(&self.os),
            line(&self.arch),
            line(&self.desktop),
            line(&self.run),
            line(&self.previous_run),
            line(&self.log_path),
            self.frontend_reports_suppressed,
        )
    }
}

fn line(text: &str) -> String {
    redact::line(text, super::MAX_RECORD_BYTES)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub text: String,
    pub lines: usize,
    pub truncated: bool,
}

impl Report {
    pub fn bytes(&self) -> usize {
        self.text.len()
    }
}

pub const TRUNCATION_MARKER: &str =
    "[earlier diagnostics omitted: a copied report is limited to 256 KiB]\n";

struct Section {
    heading: String,
    lines: Vec<String>,
}

/// Assembles a report of at most `limit` bytes from the newest lines of
/// `files` (given oldest first). Each file is read from its end, only as far
/// as the remaining room, and a partial first line is dropped, so no line —
/// and no UTF-8 sequence — is ever split. The lines kept are one unbroken
/// stretch up to the newest: once a file no longer fits whole, nothing older
/// is read, however small.
pub fn build(header: &Header, files: &[LogFile], limit: usize) -> Report {
    let header = header.render();
    let reserved = header.len() + TRUNCATION_MARKER.len();
    let mut remaining = limit.saturating_sub(reserved);
    let mut truncated = false;
    let mut sections = Vec::new();
    for (index, file) in files.iter().enumerate().rev() {
        let heading = format!(
            "--- {}: {} ---",
            file.origin.label(),
            file.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        let older_content = || {
            files[..=index]
                .iter()
                .any(|file| std::fs::metadata(&file.path).is_ok_and(|m| m.len() > 0))
        };
        if remaining <= heading.len() + 1 {
            truncated |= older_content();
            break;
        }
        let (lines, partial) = match read_tail_lines(&file.path, remaining - heading.len() - 1) {
            Ok(result) => result,
            Err(error) => (
                vec![format!("[this file could not be read: {:?}]", error.kind())],
                false,
            ),
        };
        if !lines.is_empty() {
            let used = heading.len() + 1 + lines.iter().map(|line| line.len() + 1).sum::<usize>();
            remaining = remaining.saturating_sub(used);
            sections.push(Section { heading, lines });
        }
        if partial {
            truncated = true;
            break;
        }
    }

    sections.reverse();
    assemble(header, sections, truncated, limit)
}

/// The final text, with a hard guarantee on its size: sanitizing can make a
/// line slightly longer than it was read, so whole oldest lines are dropped
/// until the report fits.
fn assemble(
    header: String,
    mut sections: Vec<Section>,
    mut truncated: bool,
    limit: usize,
) -> Report {
    loop {
        let body = sections
            .iter()
            .map(|section| {
                section.heading.len() + 1 + section.lines.iter().map(|l| l.len() + 1).sum::<usize>()
            })
            .sum::<usize>();
        let marker = if truncated {
            TRUNCATION_MARKER.len()
        } else {
            0
        };
        if header.len() + marker + body <= limit || sections.is_empty() {
            break;
        }
        truncated = true;
        let first = &mut sections[0];
        if first.lines.is_empty() {
            sections.remove(0);
        } else {
            first.lines.remove(0);
        }
    }
    let mut text = header;
    if truncated {
        text.push_str(TRUNCATION_MARKER);
    }
    for section in sections {
        text.push_str(&section.heading);
        text.push('\n');
        for line in section.lines {
            text.push_str(&line);
            text.push('\n');
        }
    }
    let lines = text.lines().count();
    Report {
        text,
        lines,
        truncated,
    }
}

/// The complete lines within the last `budget` bytes of `path`, sanitized,
/// and whether anything before them was left out.
fn read_tail_lines(path: &Path, budget: usize) -> io::Result<(Vec<String>, bool)> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let wanted = size.min(budget as u64);
    let partial = wanted < size;
    file.seek(SeekFrom::Start(size - wanted))?;
    let mut bytes = Vec::with_capacity(wanted as usize);
    file.take(wanted).read_to_end(&mut bytes)?;
    let start = if partial {
        // Whatever precedes the first newline is the end of a line that did
        // not fit; a newline byte never occurs inside a UTF-8 sequence.
        match bytes.iter().position(|byte| *byte == b'\n') {
            Some(newline) => newline + 1,
            None => bytes.len(),
        }
    } else {
        0
    };
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(line)
        .collect();
    Ok((lines, partial))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            generated: "2026-09-16T08:00:00Z".into(),
            version: "0.7.9".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            desktop: "Hyprland wayland".into(),
            run: "0a1b2c3d".into(),
            previous_run: "clean".into(),
            log_path: "/home/jonas/.local/share/software.jli.utterform/logs/utterform.log".into(),
            frontend_reports_suppressed: 0,
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn files_are_found_and_ordered_oldest_first() {
        let directory = tempfile::tempdir().unwrap();
        let logs = directory.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        let legacy = directory.path().join("utterform.log");
        write(&legacy, "legacy\n");
        write(&logs.join("utterform.log"), "current\n");
        write(&logs.join("utterform.1.log"), "newer\n");
        write(&logs.join("utterform.2.log"), "older\n");
        // Neither a settings file nor an unrelated log is ever a source.
        write(&logs.join("settings.json"), "{}");
        write(&logs.join("other.1.log"), "x");
        let files = log_files(&logs, "utterform", Some(&legacy));
        let names = files
            .iter()
            .map(|file| {
                file.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "utterform.log",
                "utterform.2.log",
                "utterform.1.log",
                "utterform.log"
            ]
        );
        assert_eq!(files[0].origin, Origin::Legacy);
        assert_eq!(files[3].origin, Origin::Current);
        // No legacy file, no directory: nothing to read, and no error.
        assert!(log_files(&directory.path().join("missing"), "utterform", None).is_empty());
    }

    #[test]
    fn a_small_report_keeps_everything_in_order_and_redacts_legacy_lines() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = directory.path().join("legacy.log");
        let rotated = directory.path().join("utterform.1.log");
        let current = directory.path().join("utterform.log");
        write(
            &legacy,
            "2026-09-10 10:00:00.000 opened /home/jonas/Documents/secret.md with sk-proj-CANARY0123456789abcdefgh\n",
        );
        write(&rotated, "rotated one\nrotated two\n");
        write(&current, "current one\n");
        let files = [
            LogFile {
                origin: Origin::Legacy,
                path: legacy,
            },
            LogFile {
                origin: Origin::Rotated,
                path: rotated,
            },
            LogFile {
                origin: Origin::Current,
                path: current,
            },
        ];
        let report = build(&header(), &files, LIMIT);
        assert!(!report.truncated);
        assert!(!report.text.contains("CANARY"), "{}", report.text);
        assert!(!report.text.contains("jonas"), "{}", report.text);
        let order = ["legacy log", "rotated one", "rotated two", "current one"]
            .map(|needle| report.text.find(needle).unwrap());
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        assert!(
            report
                .text
                .starts_with("Utterform diagnostics report\nformat: 1\n")
        );
        assert_eq!(report.lines, report.text.lines().count());
    }

    #[test]
    fn a_large_log_is_capped_at_the_newest_lines_without_splitting_utf8() {
        let directory = tempfile::tempdir().unwrap();
        let current = directory.path().join("utterform.log");
        let rotated = directory.path().join("utterform.1.log");
        // Multi-byte lines, well over the limit in total.
        let body = (0..20_000)
            .map(|index| format!("{index:05} Grüße aus Zürich — ✓ 😀"))
            .collect::<Vec<_>>()
            .join("\n");
        write(&current, &format!("{body}\n"));
        write(&rotated, "an old rotated line\n");
        let files = [
            LogFile {
                origin: Origin::Rotated,
                path: rotated,
            },
            LogFile {
                origin: Origin::Current,
                path: current,
            },
        ];
        for limit in [LIMIT, 4096, 1000] {
            let report = build(&header(), &files, limit);
            assert!(report.bytes() <= limit, "{} > {limit}", report.bytes());
            assert!(report.truncated);
            assert!(report.text.contains(TRUNCATION_MARKER));
            // The newest line is always the one kept.
            assert!(report.text.contains("19999 Grüße"), "limit {limit}");
            // Every kept log line is whole.
            for line in report.text.lines().filter(|line| line.contains("Grüße")) {
                assert!(line.ends_with("😀"), "{line}");
            }
            assert!(!report.text.contains("an old rotated line"));
        }
    }

    #[test]
    fn nothing_older_is_read_once_a_newer_file_was_cut_short() {
        let directory = tempfile::tempdir().unwrap();
        let legacy = directory.path().join("legacy.log");
        let rotated = directory.path().join("utterform_2026-09-15_00-00-00.log");
        let current = directory.path().join("utterform.log");
        write(&legacy, "a small legacy line\n");
        write(&rotated, &"rotated filler line\n".repeat(2_000));
        write(&current, &"current filler line\n".repeat(200));
        let files = [
            LogFile {
                origin: Origin::Legacy,
                path: legacy,
            },
            LogFile {
                origin: Origin::Rotated,
                path: rotated,
            },
            LogFile {
                origin: Origin::Current,
                path: current,
            },
        ];
        let report = build(&header(), &files, 16 * 1024);
        assert!(report.truncated);
        assert!(report.bytes() <= 16 * 1024);
        assert!(report.text.contains("--- current log: utterform.log ---"));
        assert!(report.text.contains("rotated filler line"));
        // The rotated file was cut, so the smaller, older legacy file is not
        // read into a report with a gap before it.
        assert!(!report.text.contains("legacy"), "{}", report.text);
        // Newest last, and the whole current file is there.
        assert!(report.text.ends_with("current filler line\n"));
        assert_eq!(report.text.matches("current filler line").count(), 200);
    }

    #[test]
    fn unreadable_or_missing_files_do_not_fail_the_report() {
        let directory = tempfile::tempdir().unwrap();
        let files = [LogFile {
            origin: Origin::Current,
            path: directory.path().join("gone.log"),
        }];
        let report = build(&header(), &files, LIMIT);
        assert!(report.text.contains("could not be read"));
        assert!(report.bytes() <= LIMIT);
    }
}
