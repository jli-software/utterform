//! Unicode SendInput with an out-of-context WinEvent observer on the owning
//! worker thread. Pumping the message queue delivers even away-and-back events.
use super::super::windows::{send, unicode, verify_live_integrity};
use super::StopLatch;
use std::{
    cell::RefCell,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::{
            DispatchMessageW, EVENT_OBJECT_FOCUS, EVENT_SYSTEM_DESKTOPSWITCH,
            EVENT_SYSTEM_FOREGROUND, GUI_INMENUMODE, GUI_INMOVESIZE, GUI_POPUPMENUMODE,
            GUI_SYSTEMMENUMODE, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo,
            GetWindowThreadProcessId, IsWindow, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
            WINEVENT_OUTOFCONTEXT,
        },
    },
};

struct Observation {
    window: HWND,
    child: HWND,
    thread: u32,
    stopped: bool,
}
thread_local! {
    static OBSERVATION: RefCell<Option<Observation>> = const { RefCell::new(None) };
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    event_thread: u32,
    _time: u32,
) {
    OBSERVATION.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.stopped |= match event {
                EVENT_SYSTEM_FOREGROUND => window != state.window,
                EVENT_SYSTEM_DESKTOPSWITCH => true,
                EVENT_OBJECT_FOCUS => {
                    event_thread == state.thread && !state.child.is_null() && window != state.child
                }
                _ => false,
            };
        }
    });
}

pub(super) struct LiveTyper {
    window: HWND,
    child: HWND,
    hooks: Vec<HWINEVENTHOOK>,
    latch: StopLatch,
}

fn gui_focus() -> Result<GUITHREADINFO, String> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(0, &mut info) } == 0 {
        return Err("Windows cannot observe the focused input target".into());
    }
    if info.flags & (GUI_INMENUMODE | GUI_INMOVESIZE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE) != 0
    {
        return Err("A menu or window operation interrupted live typing".into());
    }
    Ok(info)
}

fn modifiers_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key)) } < 0)
}

impl LiveTyper {
    pub(super) fn set_cancel_flag(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.latch.cancel = Some(flag);
    }

    pub(super) fn capture() -> Result<Self, String> {
        if OBSERVATION.with(|slot| slot.borrow().is_some()) {
            return Err("Another live typing session owns this worker".into());
        }
        let mut session = Self {
            window: std::ptr::null_mut(),
            child: std::ptr::null_mut(),
            hooks: Vec::new(),
            latch: StopLatch::default(),
        };
        // Install before capture, so there is no unobserved capture-to-hook gap.
        for event in [
            EVENT_SYSTEM_FOREGROUND,
            EVENT_OBJECT_FOCUS,
            EVENT_SYSTEM_DESKTOPSWITCH,
        ] {
            let hook = unsafe {
                SetWinEventHook(
                    event,
                    event,
                    std::ptr::null_mut(),
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_null() {
                return Err("Windows could not install the live focus observer".into());
            }
            session.hooks.push(hook);
        }
        session.window = unsafe { GetForegroundWindow() };
        if session.window.is_null() {
            return Err("Focus an external text field before starting live dictation".into());
        }
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(session.window, &mut pid) };
        if pid == 0 || pid == std::process::id() {
            return Err(
                "Start live dictation using the shortcut in another application's text field"
                    .into(),
            );
        }
        verify_live_integrity(session.window)?;
        session.child = gui_focus()?.hwndFocus;
        OBSERVATION.with(|slot| {
            *slot.borrow_mut() = Some(Observation {
                window: session.window,
                child: session.child,
                thread,
                stopped: false,
            })
        });
        session.check_target()?;
        Ok(session)
    }

    pub(super) fn check_target(&mut self) -> Result<(), String> {
        self.latch.check()?;
        // Out-of-context hooks execute on this thread when messages are pumped.
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let stopped =
            OBSERVATION.with(|slot| slot.borrow().as_ref().is_none_or(|state| state.stopped));
        if stopped
            || unsafe { IsWindow(self.window) } == 0
            || unsafe { GetForegroundWindow() } != self.window
        {
            return Err(self.latch.stop("Live typing paused because the target lost focus; start a new dictation to continue"));
        }
        let info = gui_focus().map_err(|error| self.latch.stop(error))?;
        if !self.child.is_null() && info.hwndFocus != self.child {
            return Err(self
                .latch
                .stop("Live typing paused because the focused control changed"));
        }
        Ok(())
    }

    fn wait_for_modifiers(&mut self) -> Result<(), String> {
        let until = Instant::now() + Duration::from_millis(1200);
        // The shortcut's modifiers may still be held when capture runs. Let
        // them release without synthesizing their release or any other key.
        while modifiers_down() {
            self.check_target()?;
            if Instant::now() >= until {
                return Err(self
                    .latch
                    .stop("Live typing paused because a keyboard modifier is held"));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    pub(super) fn insert(&mut self, text: &str) -> Result<(), String> {
        self.check_target()?;
        // At most one Unicode scalar (including both UTF-16 surrogates) per
        // atomic SendInput batch keeps the focus-race exposure very small.
        for character in text.chars() {
            self.wait_for_modifiers()?;
            self.check_target()?;
            let mut units = [0u16; 2];
            let events: Vec<_> = character
                .encode_utf16(&mut units)
                .iter()
                .flat_map(|unit| [unicode(*unit, false), unicode(*unit, true)])
                .collect();
            send(&events).map_err(|error| self.latch.stop(error))?;
        }
        self.check_target()
    }
}

impl Drop for LiveTyper {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            unsafe {
                UnhookWinEvent(hook);
            }
        }
        OBSERVATION.with(|slot| *slot.borrow_mut() = None);
    }
}
