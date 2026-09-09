//! Append-only live delivery. A session never reacquires its target or replays
//! a failed insertion. Keep this object on one dedicated OS worker thread:
//! call `poll_events` while waiting for transcript deltas and `insert` per
//! chunk; each insert confirms the target once before typing.
//!
//! Window identity is not a universal text-field/cursor identity. In particular,
//! browser tab changes and cursor movement within the same field can be invisible.
//! Focus checks and OS injection cannot be made atomic.

#[cfg(target_os = "linux")]
#[path = "live_linux.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "live_windows.rs"]
mod platform;

#[cfg(any(target_os = "linux", target_os = "windows", test))]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// A fail-closed session: once stopped, a return to the original window does
/// not allow more input. The original error is retained for a useful UI status.
#[derive(Default)]
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub(super) struct StopLatch {
    error: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
    /// Milliseconds after `epoch` until which typing is held back, set by the
    /// orchestrator when a stop was requested: the shortcut's modifier may
    /// still be physically held, and a compositor combines it with any key
    /// this session sends. Zero means no hold.
    hold: Option<(Instant, Arc<AtomicU64>)>,
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
impl StopLatch {
    pub(super) fn check(&mut self) -> Result<(), String> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            self.stop("Live typing was cancelled");
        }
        self.error.clone().map_or(Ok(()), Err)
    }

    pub(super) fn stop(&mut self, message: impl Into<String>) -> String {
        self.error.get_or_insert_with(|| message.into()).clone()
    }

    /// How much longer typing must wait for a requested stop to settle.
    fn hold_remaining(&self) -> Option<Duration> {
        let (epoch, until) = self.hold.as_ref()?;
        let until = until.load(Ordering::Acquire);
        let now = epoch.elapsed().as_millis() as u64;
        (until > now).then(|| Duration::from_millis(until - now))
    }

    /// Wait out a stop hold in small steps, so cancellation is still honoured.
    pub(super) fn wait_for_hold(&mut self) -> Result<(), String> {
        loop {
            self.check()?;
            match self.hold_remaining() {
                None => return Ok(()),
                Some(remaining) => std::thread::sleep(remaining.min(Duration::from_millis(50))),
            }
        }
    }
}

fn validate_text(text: &str) -> Result<(), String> {
    if text
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err("Live typing rejected control characters; no input was sent".into());
    }
    Ok(())
}

/// Persistent native live-input session. Capture while the external text field
/// has focus, normally via the global shortcut (never by focusing Utterform).
pub struct LiveTyper {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    inner: platform::LiveTyper,
    // WinEvent callbacks are thread-local. Explicitly prohibit moving a session
    // between async executor threads, including platforms whose handles are Send.
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl LiveTyper {
    /// The caller sets this flag on abort; cancellation is checked between
    /// Unicode scalars and while waiting for held shortcut modifiers to release.
    pub fn set_cancel_flag(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        self.inner.set_cancel_flag(flag);
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let _ = flag;
    }

    /// Milliseconds after `epoch` before which no key is sent, updated by the
    /// caller whenever a stop is requested through the dictation shortcut.
    pub fn set_stop_hold(
        &mut self,
        epoch: std::time::Instant,
        until: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        self.inner.set_stop_hold(epoch, until);
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let _ = (epoch, until);
    }

    /// `session_id` labels this session's diagnostic log lines.
    pub fn capture(session_id: u64) -> Result<Self, String> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            Ok(Self {
                inner: platform::LiveTyper::capture(session_id)?,
                _thread_bound: std::marker::PhantomData,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = session_id;
            Err("Live typing is available on Windows and Omarchy/Hyprland in 0.6.x".into())
        }
    }

    /// A privacy-safe description of the captured target for the log: a window
    /// identity, never a title or text.
    pub fn target_description(&self) -> String {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        return self.inner.target_description();
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        String::from("no live target on this platform")
    }

    /// Append verbatim Unicode, without clipboard use or editing keys. The
    /// target is confirmed once before the chunk is typed.
    /// Any error may follow partial delivery: never retry this text automatically.
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        validate_text(text)?;
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        return self.inner.insert(text);
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        Err("Live typing is not supported on this platform".into())
    }

    /// Cheap idle check: cancellation and queued focus notifications. A
    /// detected loss is permanent for this session, including a quick
    /// away-and-back switch.
    pub fn poll_events(&mut self) -> Result<(), String> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        return self.inner.poll_events();
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        Err("Live typing is not supported on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_text_never_contains_editing_or_submit_controls() {
        for text in [
            "hello\n", "\r\n", "\t", "\u{8}", "\0", "\u{7f}", "\u{85}", "\u{2028}", "\u{2029}",
        ] {
            assert!(validate_text(text).is_err(), "{text:?}");
        }
        assert!(validate_text("Grüße – 中文 😀 e\u{301} ").is_ok());
        assert!(validate_text("").is_ok());
    }

    #[test]
    fn a_stopped_target_never_automatically_recovers() {
        let mut latch = StopLatch::default();
        assert!(latch.check().is_ok());
        latch.stop("Focus changed");
        assert_eq!(latch.check(), Err("Focus changed".into()));
        assert_eq!(latch.stop("A later error"), "Focus changed");
        assert_eq!(latch.check(), Err("Focus changed".into()));
    }

    #[test]
    fn cancellation_is_latched_even_if_caller_clears_the_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        let mut latch = StopLatch {
            error: None,
            cancel: Some(flag.clone()),
            hold: None,
        };
        assert!(latch.check().is_ok());
        flag.store(true, Ordering::Release);
        assert!(latch.check().is_err());
        flag.store(false, Ordering::Release);
        assert!(latch.check().is_err());
    }

    #[test]
    fn a_requested_stop_holds_typing_until_the_shortcut_has_settled() {
        let epoch = Instant::now();
        let until = Arc::new(AtomicU64::new(0));
        let mut latch = StopLatch {
            error: None,
            cancel: None,
            hold: Some((epoch, until.clone())),
        };
        assert_eq!(latch.hold_remaining(), None);
        assert!(latch.wait_for_hold().is_ok());
        until.store(epoch.elapsed().as_millis() as u64 + 120, Ordering::Release);
        let started = Instant::now();
        assert!(latch.wait_for_hold().is_ok());
        assert!(started.elapsed() >= Duration::from_millis(100));
        assert_eq!(latch.hold_remaining(), None);
    }

    #[test]
    fn cancellation_interrupts_a_hold() {
        let epoch = Instant::now();
        let flag = Arc::new(AtomicBool::new(false));
        let until = Arc::new(AtomicU64::new(10_000));
        let mut latch = StopLatch {
            error: None,
            cancel: Some(flag.clone()),
            hold: Some((epoch, until)),
        };
        let cancel = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            cancel.store(true, Ordering::Release);
        });
        let started = Instant::now();
        assert!(latch.wait_for_hold().is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
