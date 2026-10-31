//! Recordings whose transcription failed, kept so the words are not lost.
//!
//! A failed network request is no reason to ask someone to say it all again.
//! The audio stays in its temporary file until a retry succeeds, the user
//! discards it, or Utterform quits; the next start removes whatever is left.
//! Nothing here reaches the disk beyond that file, and only the count and
//! duration leave this module.

use std::{collections::VecDeque, sync::Mutex};

use serde::Serialize;

use crate::{audio::RecordingArtifact, diagnostics, domain::ProcessRequest};

/// A few failed dictations in a row is an outage; more than this is a
/// recording habit the temporary directory should not have to absorb.
const LIMIT: usize = 5;

pub struct Retained {
    pub artifact: RecordingArtifact,
    pub request: ProcessRequest,
}

/// What the interface offers: the recording next in line and how many wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    pub count: usize,
    pub duration_ms: u64,
}

#[derive(Default)]
pub struct RetainedRecordings(Mutex<VecDeque<Retained>>);

impl RetainedRecordings {
    fn queue(&self) -> std::sync::MutexGuard<'_, VecDeque<Retained>> {
        // The queue is valid after any panic: entries are only moved whole.
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn keep(&self, recording: Retained) {
        let mut queue = self.queue();
        queue.push_back(recording);
        if queue.len() > LIMIT {
            queue.pop_front();
            diagnostics::warning!("recording.retained_dropped", limit = LIMIT);
        }
    }

    /// The oldest recording, removed from the queue. Dropping it deletes its
    /// audio, so a caller that fails must [`restore`](Self::restore) it.
    pub fn take(&self) -> Option<Retained> {
        self.queue().pop_front()
    }

    pub fn restore(&self, recording: Retained) {
        self.queue().push_front(recording);
    }

    pub fn waiting(&self) -> Option<Waiting> {
        let queue = self.queue();
        queue.front().map(|next| Waiting {
            count: queue.len(),
            duration_ms: next.artifact.duration_ms(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::OutputFormat;

    fn recording(id: u64, sample_count: u64) -> Retained {
        Retained {
            artifact: RecordingArtifact {
                interrupted: false,
                recording: id,
                path: PathBuf::from(format!("/nonexistent/utterform-retained-test-{id}.wav")),
                sample_rate: 1000,
                channels: 1,
                sample_count,
            },
            request: ProcessRequest {
                action: "plain".into(),
                custom_prompt: None,
                copy_to_clipboard: true,
                save_to_file: false,
                type_at_cursor: true,
                output_format: OutputFormat::Txt,
            },
        }
    }

    #[test]
    fn recordings_wait_oldest_first_and_a_failed_retry_keeps_its_place() {
        let retained = RetainedRecordings::default();
        assert_eq!(retained.waiting(), None);
        retained.keep(recording(1, 1000));
        retained.keep(recording(2, 2000));
        assert_eq!(
            retained.waiting(),
            Some(Waiting {
                count: 2,
                duration_ms: 1000
            })
        );

        let first = retained.take().unwrap();
        assert_eq!(first.artifact.recording, 1);
        retained.restore(first);
        assert_eq!(retained.take().unwrap().artifact.recording, 1);
        assert_eq!(retained.take().unwrap().artifact.recording, 2);
        assert!(retained.take().is_none());
    }

    #[test]
    fn the_queue_is_bounded_by_giving_up_the_oldest() {
        let retained = RetainedRecordings::default();
        for id in 0..=(LIMIT as u64) {
            retained.keep(recording(id, 1000));
        }
        assert_eq!(retained.waiting().unwrap().count, LIMIT);
        assert_eq!(retained.take().unwrap().artifact.recording, 1);
    }
}
