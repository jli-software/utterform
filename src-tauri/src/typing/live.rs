//! Append-only live delivery. A session never reacquires its target or replays
//! a failed insertion. Keep this object on one dedicated OS worker thread and
//! call `check_target` regularly while waiting for transcript deltas.
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

/// A fail-closed session: once stopped, a return to the original window does
/// not allow more input. The original error is retained for a useful UI status.
#[derive(Default)]
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub(super) struct StopLatch {
    error: Option<String>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
impl StopLatch {
    pub(super) fn check(&mut self) -> Result<(), String> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
        {
            self.stop("Live typing was cancelled");
        }
        self.error.clone().map_or(Ok(()), Err)
    }

    pub(super) fn stop(&mut self, message: impl Into<String>) -> String {
        self.error.get_or_insert_with(|| message.into()).clone()
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

    pub fn capture() -> Result<Self, String> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            Ok(Self {
                inner: platform::LiveTyper::capture()?,
                _thread_bound: std::marker::PhantomData,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        Err("Live typing is available on Windows and Omarchy/Hyprland in 0.6.0".into())
    }

    /// Append verbatim Unicode, without clipboard use or editing keys.
    /// Any error may follow partial delivery: never retry this text automatically.
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        validate_text(text)?;
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        return self.inner.insert(text);
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        Err("Live typing is not supported on this platform".into())
    }

    /// Drain focus notifications and verify the current target. A detected loss
    /// is permanent for this session, including a quick away-and-back switch.
    pub fn check_target(&mut self) -> Result<(), String> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        return self.inner.check_target();
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
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let flag = Arc::new(AtomicBool::new(false));
        let mut latch = StopLatch {
            error: None,
            cancel: Some(flag.clone()),
        };
        assert!(latch.check().is_ok());
        flag.store(true, Ordering::Release);
        assert!(latch.check().is_err());
        flag.store(false, Ordering::Release);
        assert!(latch.check().is_err());
    }
}
