//! Omarchy/Hyprland-only live input. One Wayland virtual keyboard lives for the
//! whole session; Hyprland's event socket records even quick away/back switches.
//! No wtype child per delta, X11 fallback, clipboard write, or focus restoration.
//!
//! References: https://wiki.hypr.land/IPC/ and virtual-keyboard-unstable-v1.
//! Hyprland IPC identifies windows, not fields/cursors or physical key state.
//! The virtual keyboard uses its own zero modifier state. Concurrent physical
//! typing (including shortcut keys) during dictation is not supported.
//!
//! Focus policy (0.6.1): only a confirmed change of the active window address
//! or the closing of the target window is a loss of the target. Workspace,
//! monitor, layer, submap and config-reload events are recorded for diagnosis
//! but never stop a session on their own; the target is re-confirmed with one
//! synchronous Hyprland query per delivered text chunk, not every 20 ms and
//! not per character. Technical IPC failures stop typing for safety but are
//! reported as such, never as a focus change by the user.
//!
//! Keycode policy (0.6.1): Hyprland resolves its own key bindings for a virtual
//! keyboard through the compositor's configured layout by *keycode*, with the
//! modifiers of every keyboard merged in, not through the keymap this keyboard
//! uploads. Chromium on Wayland additionally drops keys whose evdev code has no
//! DOM code, and derives editing behaviour from the key's US-layout meaning.
//! Characters are therefore placed on keycodes that carry no default binding,
//! no editing or browser meaning and a DOM code, before ordinary letter keys
//! are used; keys such as Escape, Tab, Backspace, Return, F1–F12, navigation,
//! media, brightness, print and power keys are never used.
use super::StopLatch;
use crate::diagnostics;
use std::{
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    protocol::{wl_callback, wl_registry, wl_seat},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

const IO_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_EVENTS: usize = 64 * 1024;
/// A busy compositor may miss one command-socket deadline; a user focus change
/// is never inferred from that. The query is repeated before typing stops.
const QUERY_ATTEMPTS: usize = 3;
const QUERY_RETRY_DELAY: Duration = Duration::from_millis(100);
/// Pacing between key presses for clients that reorder fast input.
const KEY_PACING: Duration = Duration::from_millis(5);
/// Diagnostic lines per session, so a chatty desktop cannot fill the log.
const MAX_LOGGED_EVENTS: usize = 200;

/// Keycodes (XKB numbering, evdev + 8) that carry no default Hyprland/Omarchy
/// binding with or without a modifier, no editing, browser, media or launch
/// meaning in Chromium/Blink, and a DOM code so Chromium on Wayland accepts
/// them: keypad keys, F13–F19/F24, the international keys and a few legacy
/// keys. Order is preference order.
const QUIET_KEYCODES: &[u32] = &[
    79, 80, 81, 83, 84, 85, 86, 87, 88, 89, 90, 106, 125, 129, 126, // keypad
    191, 192, 193, 194, 195, 196, 197, 202, // F13–F19, F24
    94, 97, 132, 217, // IntlBackslash, IntlRo, IntlYen, BassBoost
    127, 140, 142, 144, 189, 137, 190, 139, 141, 143, 145, // Pause … Cut
];
/// Ordinary printable keys. Every client accepts them, but a compositor binding
/// may combine them with a modifier that is still physically held right after
/// the dictation shortcut, so they are used only once the quiet keys are taken.
const ORDINARY_KEYCODES: &[u32] = &[
    47, 48, 49, 51, 34, 35, 59, 60, 61, // ; ' ` \ [ ] , . /
    24, 26, 27, 28, 29, 30, 31, 32, 38, 39, 40, 41, 42, 43, 45, 46, 52, 53, 54, 55, 56, 57,
    58, // letters except w, j, p
    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, // digits
    20, 21, 44, 33, 25, 65, // - = j p w space
];
/// Characters mapped before the first delta, most frequent in dictation first,
/// so the common ones take the quiet keys. Everything else is mapped on demand.
const SEED: &str = " enirstadhulcgmobwfkzpv.,jyxqDSEIWABMKGFHNUVZJLOPRTCYXQ-?!:;\"'()/";
/// Reserved priming key: keycode 8 is unmapped in every XKB layout.
const PRIMING_KEY: u32 = 0;

#[derive(Default)]
struct WaylandState {
    seats: Vec<(u32, wl_seat::WlSeat)>,
    manager: Option<(u32, ZwpVirtualKeyboardManagerV1)>,
    removed: bool,
    synced: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == "wl_seat" {
                    state
                        .seats
                        .push((name, registry.bind(name, version.min(7), qh, ())));
                } else if interface == "zwp_virtual_keyboard_manager_v1" {
                    state.manager = Some((name, registry.bind(name, 1, qh, ())));
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.removed |= state.seats.iter().any(|(id, _)| *id == name)
                    || state.manager.as_ref().is_some_and(|(id, _)| *id == name);
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_callback::WlCallback, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_callback::Event::Done { .. }) {
            state.synced = true;
        }
    }
}
delegate_noop!(WaylandState: ignore wl_seat::WlSeat);
delegate_noop!(WaylandState: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(WaylandState: ignore ZwpVirtualKeyboardV1);

/// Bounded roundtrip. A stalled compositor must not strand the typing worker
/// forever while the audio path continues to produce queued transcript text.
fn sync(
    connection: &Connection,
    queue: &mut EventQueue<WaylandState>,
    state: &mut WaylandState,
) -> Result<(), String> {
    state.synced = false;
    connection.display().sync(&queue.handle(), ());
    connection
        .flush()
        .map_err(|_| technical("the Wayland input connection failed"))?;
    let until = Instant::now() + IO_TIMEOUT;
    while !state.synced {
        queue
            .dispatch_pending(state)
            .map_err(|_| technical("the Wayland input observer failed"))?;
        if state.removed {
            return Err(technical("the Wayland keyboard or seat disappeared"));
        }
        if state.synced {
            break;
        }
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(technical("the Wayland input connection timed out"));
        }
        let mut fd = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready =
            unsafe { libc::poll(&mut fd, 1, left.as_millis().min(i32::MAX as u128) as i32) };
        if ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if ready <= 0 {
            return Err(technical(
                "the Wayland input connection timed out or failed",
            ));
        }
        guard
            .read()
            .map_err(|_| technical("the Wayland input connection closed"))?;
    }
    Ok(())
}

/// A failure of the observation machinery itself. Worded so it cannot be read
/// as the user having switched windows.
fn technical(detail: &str) -> String {
    format!(
        "Live typing stopped for safety because {detail}; this is a technical fault, not a focus change"
    )
}

const LOST_OTHER_WINDOW: &str =
    "Live typing paused because another window received focus; start a new dictation to continue";
const LOST_TARGET_CLOSED: &str =
    "Live typing paused because the target window was closed; start a new dictation to continue";
const LOST_NO_FOCUS: &str = "Live typing paused because the target window no longer has keyboard focus (a launcher, popup or empty workspace took it); start a new dictation to continue";

fn normalize_address(address: &str) -> &str {
    let address = address.trim();
    address.strip_prefix("0x").unwrap_or(address)
}

/// What one Hyprland event-socket line means for the captured target window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusEvent {
    /// The target window has keyboard focus (again).
    Target,
    /// No window has keyboard focus: a launcher or other layer, or an empty
    /// workspace. Not a loss by itself; re-confirmed before the next chunk.
    NoWindow,
    /// A different window received focus. A confirmed loss, even if the target
    /// gets focus back later: text may already have gone elsewhere.
    OtherWindow,
    /// The target window was closed.
    TargetClosed,
    /// Everything else, including workspace, monitor, special-workspace, layer,
    /// submap and config-reload events. Never a loss on its own.
    Unrelated,
}

fn classify_event(line: &str, target: &str) -> FocusEvent {
    let Some((event, data)) = line.split_once(">>") else {
        return FocusEvent::Unrelated;
    };
    match event {
        "activewindowv2" => {
            let address = normalize_address(data);
            if address.is_empty() || address == "0" {
                FocusEvent::NoWindow
            } else if address == normalize_address(target) {
                FocusEvent::Target
            } else {
                FocusEvent::OtherWindow
            }
        }
        "closewindow" => {
            if normalize_address(data.split(',').next().unwrap_or_default())
                == normalize_address(target)
            {
                FocusEvent::TargetClosed
            } else {
                FocusEvent::Unrelated
            }
        }
        _ => FocusEvent::Unrelated,
    }
}

/// A privacy-safe rendering of an event line for the diagnostic log: the event
/// name, plus a window address or workspace id where the event carries one.
/// Titles, classes and workspace names are never logged. `None` skips events
/// that say nothing about focus (title changes, layout changes, …).
fn describe_event(line: &str) -> Option<String> {
    let (event, data) = line.split_once(">>")?;
    let first = |data: &str| data.split(',').next().unwrap_or_default().trim().to_owned();
    Some(match event {
        "activewindowv2" | "closewindow" | "openwindow" | "movewindow" | "movewindowv2"
        | "urgent" => format!("{event} {}", first(data)),
        "workspacev2" | "focusedmonv2" | "activespecialv2" | "createworkspacev2"
        | "destroyworkspacev2" => format!("{event} {}", first(data)),
        "workspace" | "focusedmon" | "activespecial" | "openlayer" | "closelayer" | "submap"
        | "configreloaded" | "monitoradded" | "monitorremoved" | "monitoraddedv2"
        | "monitorremovedv2" | "createworkspace" | "destroyworkspace" | "lockgroups"
        | "fullscreen" | "minimized" => event.to_owned(),
        _ => return None,
    })
}

struct FocusGuard {
    command_path: PathBuf,
    events: UnixStream,
    pending: Vec<u8>,
    address: String,
    /// Last known keyboard-focus state from the event stream.
    focused: bool,
    label: String,
    logged: usize,
}

impl FocusGuard {
    fn capture(label: String) -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err("Live typing on Linux needs an Omarchy/Hyprland Wayland session".into());
        }
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .map_err(|_| "Live typing on Linux currently supports Omarchy/Hyprland only")?;
        if signature.is_empty() || signature.contains('/') || signature == "." || signature == ".."
        {
            return Err("Invalid Hyprland session identity".into());
        }
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or("Hyprland runtime directory is unavailable")?;
        let directory = PathBuf::from(runtime).join("hypr").join(signature);
        // Subscribe before capturing the target. Socket2 retains all events
        // while the worker waits for the next transcript delta.
        let events = UnixStream::connect(directory.join(".socket2.sock"))
            .map_err(|_| "Could not subscribe to Hyprland focus events")?;
        events
            .set_nonblocking(true)
            .map_err(|_| "Could not configure Hyprland focus observation")?;
        let mut guard = Self {
            command_path: directory.join(".socket.sock"),
            events,
            pending: Vec::new(),
            address: String::new(),
            focused: true,
            label,
            logged: 0,
        };
        let window = guard.query_active_window()?;
        let address = window
            .get("address")
            .and_then(serde_json::Value::as_str)
            .filter(|address| {
                !normalize_address(address).is_empty() && normalize_address(address) != "0"
            })
            .ok_or("Focus an external text field before starting live dictation")?;
        let pid = window
            .get("pid")
            .and_then(serde_json::Value::as_u64)
            .ok_or("Hyprland cannot identify the focused application")?;
        if pid == u64::from(std::process::id())
            || ["class", "initialClass"].iter().any(|key| {
                window
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value.to_ascii_lowercase().contains("utterform"))
            })
        {
            return Err(
                "Start live dictation using the shortcut in another application's text field"
                    .into(),
            );
        }
        let workspace = window
            .get("workspace")
            .and_then(|workspace| workspace.get("id"))
            .and_then(serde_json::Value::as_i64)
            .map_or_else(|| "?".to_owned(), |id| id.to_string());
        let xwayland = window
            .get("xwayland")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        guard.address = address.to_owned();
        diagnostics::log(format!(
            "{}: target window {} on workspace {}{}; Hyprland focus events subscribed",
            guard.label,
            guard.address,
            workspace,
            if xwayland { " (XWayland)" } else { "" }
        ));
        guard.confirm()?;
        Ok(guard)
    }

    fn active_window(&self) -> Result<serde_json::Value, String> {
        // Hyprland's command socket is synchronous: write immediately after
        // connect and always close after reading; never keep this socket idle.
        let mut stream = UnixStream::connect(&self.command_path)
            .map_err(|_| "the Hyprland command socket refused the connection".to_owned())?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|_| "the Hyprland query timeout could not be set".to_owned())?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|_| "the Hyprland query timeout could not be set".to_owned())?;
        stream
            .write_all(b"j/activewindow")
            .map_err(|_| "the Hyprland focus query could not be sent".to_owned())?;
        let mut response = Vec::new();
        stream
            .take(MAX_EVENTS as u64 + 1)
            .read_to_end(&mut response)
            .map_err(|_| "the Hyprland focus query timed out".to_owned())?;
        if response.len() > MAX_EVENTS {
            return Err("the Hyprland focus response exceeded its limit".into());
        }
        serde_json::from_slice(&response)
            .map_err(|_| "Hyprland returned an invalid focus response".into())
    }

    /// The active window, retried on transport failures. A persistent failure
    /// is reported as a technical fault.
    fn query_active_window(&mut self) -> Result<serde_json::Value, String> {
        let mut last = String::new();
        for attempt in 1..=QUERY_ATTEMPTS {
            match self.active_window() {
                Ok(window) => return Ok(window),
                Err(detail) => {
                    self.log(format!("focus query attempt {attempt} failed: {detail}"));
                    last = detail;
                    if attempt < QUERY_ATTEMPTS {
                        thread::sleep(QUERY_RETRY_DELAY);
                    }
                }
            }
        }
        Err(technical(&last))
    }

    fn log(&mut self, message: impl std::fmt::Display) {
        if self.logged < MAX_LOGGED_EVENTS {
            self.logged += 1;
            diagnostics::log(format!("{}: {message}", self.label));
        }
    }

    /// Read every queued event without blocking. A confirmed loss is an error;
    /// keyboard focus leaving every window is remembered for the next confirm.
    fn drain_events(&mut self) -> Result<(), String> {
        let mut buffer = [0u8; 4096];
        let mut total = 0;
        loop {
            match self.events.read(&mut buffer) {
                Ok(0) => {
                    return Err(technical("the Hyprland focus event socket was closed"));
                }
                Ok(count) => {
                    total += count;
                    if total > MAX_EVENTS {
                        return Err(technical(
                            "the Hyprland focus event rate exceeded its limit",
                        ));
                    }
                    self.pending.extend_from_slice(&buffer[..count]);
                    if self.pending.len() > MAX_EVENTS {
                        return Err(technical("the Hyprland focus event buffer overflowed"));
                    }
                    while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                        let line = std::str::from_utf8(&self.pending[..end])
                            .map_err(|_| technical("Hyprland sent an invalid focus event"))?
                            .to_owned();
                        self.pending.drain(..=end);
                        self.observe(&line)?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(technical("the Hyprland focus observer failed")),
            }
        }
    }

    fn observe(&mut self, line: &str) -> Result<(), String> {
        let classification = classify_event(line, &self.address);
        if let Some(described) = describe_event(line) {
            self.log(format!("event {described} -> {classification:?}"));
        }
        match classification {
            FocusEvent::Target => self.focused = true,
            FocusEvent::NoWindow => self.focused = false,
            FocusEvent::OtherWindow => return Err(LOST_OTHER_WINDOW.into()),
            FocusEvent::TargetClosed => return Err(LOST_TARGET_CLOSED.into()),
            FocusEvent::Unrelated => {}
        }
        Ok(())
    }

    /// Confirm the target before a chunk of text is typed: queued events, then
    /// one synchronous query of the active window, then the events produced
    /// meanwhile. Called once per chunk and at session boundaries only.
    fn confirm(&mut self) -> Result<(), String> {
        self.drain_events()?;
        if !self.focused {
            self.log("confirm: no window has keyboard focus");
            return Err(LOST_NO_FOCUS.into());
        }
        let window = self.query_active_window()?;
        match window.get("address").and_then(serde_json::Value::as_str) {
            Some(address) if normalize_address(address) == normalize_address(&self.address) => {}
            Some(address) if !normalize_address(address).is_empty() => {
                self.log(format!(
                    "confirm: active window is {address}, not the target"
                ));
                return Err(LOST_OTHER_WINDOW.into());
            }
            _ => {
                self.log("confirm: Hyprland reports no active window");
                return Err(LOST_NO_FOCUS.into());
            }
        }
        self.drain_events()
    }
}

/// Which keycode carries which character in the uploaded keymap. Quiet keys
/// first, ordinary keys after that, least recently used slot recycled last.
struct KeycodeMap {
    slots: Vec<Slot>,
    tick: u64,
}
struct Slot {
    keycode: u32,
    character: Option<char>,
    used: u64,
}
impl KeycodeMap {
    fn new() -> Self {
        let slots = QUIET_KEYCODES
            .iter()
            .chain(ORDINARY_KEYCODES)
            .map(|keycode| Slot {
                keycode: *keycode,
                character: None,
                used: 0,
            })
            .collect();
        let mut map = Self { slots, tick: 0 };
        for character in SEED.chars() {
            map.assign(character);
        }
        map
    }

    /// The keycode of a mapped character, marking it as recently used.
    fn lookup(&mut self, character: char) -> Option<u32> {
        self.tick += 1;
        let tick = self.tick;
        self.slots
            .iter_mut()
            .find(|slot| slot.character == Some(character))
            .map(|slot| {
                slot.used = tick;
                slot.keycode
            })
    }

    /// Map a new character: the first free slot in preference order, or the
    /// least recently used one once every slot is taken. Returns the keycode.
    fn assign(&mut self, character: char) -> u32 {
        self.tick += 1;
        let tick = self.tick;
        let index = self
            .slots
            .iter()
            .position(|slot| slot.character.is_none())
            .unwrap_or_else(|| {
                self.slots
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, slot)| slot.used)
                    .map_or(0, |(index, _)| index)
            });
        let slot = &mut self.slots[index];
        slot.character = Some(character);
        slot.used = tick;
        slot.keycode
    }

    /// Self-contained XKB keymap, independent of the physical keyboard layout.
    /// ONE_LEVEL avoids capitalization/AltGr transforms; only literal Unicode
    /// keysyms are mapped, so no Return, Backspace, Tab, or shortcut key exists.
    fn keymap(&self) -> String {
        let mut map = String::from(
            "xkb_keymap { xkb_keycodes \"utterform\" { minimum = 8; maximum = 255; <INIT> = 8;\n",
        );
        for slot in self.slots.iter().filter(|slot| slot.character.is_some()) {
            map.push_str(&format!("<K{0}> = {0};\n", slot.keycode));
        }
        map.push_str("}; xkb_types \"utterform\" { type \"ONE_LEVEL\" { modifiers = None; map[None] = Level1; level_name[Level1] = \"Any\"; }; }; xkb_compatibility \"utterform\" {}; xkb_symbols \"utterform\" { key <INIT> { type[Group1] = \"ONE_LEVEL\", [ NoSymbol ] };\n");
        for slot in &self.slots {
            if let Some(character) = slot.character {
                map.push_str(&format!(
                    "key <K{}> {{ type[Group1] = \"ONE_LEVEL\", [ U{:04X} ] }};\n",
                    slot.keycode, character as u32
                ));
            }
        }
        map.push_str("}; };\0");
        map
    }
}

pub(super) struct LiveTyper {
    focus: FocusGuard,
    connection: Connection,
    queue: EventQueue<WaylandState>,
    state: WaylandState,
    keyboard: ZwpVirtualKeyboardV1,
    map: KeycodeMap,
    uploads: usize,
    epoch: Instant,
    latch: StopLatch,
}

impl LiveTyper {
    pub(super) fn set_cancel_flag(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.latch.cancel = Some(flag);
    }

    pub(super) fn set_stop_hold(
        &mut self,
        epoch: Instant,
        until: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ) {
        self.latch.hold = Some((epoch, until));
    }

    pub(super) fn capture(session_id: u64) -> Result<Self, String> {
        let label = format!("live session {session_id}");
        let focus = FocusGuard::capture(label)?;
        let connection = Connection::connect_to_env()
            .map_err(|_| "Could not connect to the Wayland compositor")?;
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut state = WaylandState::default();
        sync(&connection, &mut queue, &mut state)?;
        if state.seats.len() != 1 {
            return Err("Live typing requires one unambiguous Wayland seat".into());
        }
        let manager = &state
            .manager
            .as_ref()
            .ok_or("The compositor does not offer the Wayland virtual-keyboard protocol")?
            .1;
        let keyboard = manager.create_virtual_keyboard(&state.seats[0].1, &queue.handle(), ());
        let mut session = Self {
            focus,
            connection,
            queue,
            state,
            keyboard,
            map: KeycodeMap::new(),
            uploads: 0,
            epoch: Instant::now(),
            latch: StopLatch::default(),
        };
        session.upload_keymap()?;
        session.focus.confirm()?;
        session.focus.log("virtual keyboard ready");
        Ok(session)
    }

    pub(super) fn target_description(&self) -> String {
        format!("Hyprland window {}", self.focus.address)
    }

    fn upload_keymap(&mut self) -> Result<(), String> {
        let map = self.map.keymap();
        let mut file = tempfile::tempfile()
            .map_err(|_| technical("the live Unicode keymap could not be created"))?;
        file.write_all(map.as_bytes())
            .map_err(|_| technical("the live Unicode keymap could not be written"))?;
        self.keyboard.keymap(1, file.as_fd(), map.len() as u32);
        self.keyboard.modifiers(0, 0, 0, 0);
        sync(&self.connection, &mut self.queue, &mut self.state)?;
        // Some clients discard the first event after a virtual keyboard/keymap
        // appears. Prime with reserved key 0 mapped to NoSymbol, never a real
        // character, modifier, or editing key. Do not "fix" missing text by
        // retrying a character whose application-level delivery is unknowable.
        let time = self.epoch.elapsed().as_millis() as u32;
        self.keyboard.key(time, PRIMING_KEY, 1);
        self.keyboard.key(time, PRIMING_KEY, 0);
        sync(&self.connection, &mut self.queue, &mut self.state)?;
        thread::sleep(KEY_PACING);
        self.uploads += 1;
        Ok(())
    }

    /// Cheap idle check between deltas: cancellation and queued focus events
    /// only. No compositor query, no Wayland roundtrip.
    pub(super) fn poll_events(&mut self) -> Result<(), String> {
        self.latch.check()?;
        self.focus
            .drain_events()
            .map_err(|error| self.latch.stop(error))
    }

    pub(super) fn insert(&mut self, text: &str) -> Result<(), String> {
        self.latch.check()?;
        // One confirmed target per chunk, and one bounded compositor roundtrip
        // so a dead input connection cannot silently consume the stream.
        self.focus
            .confirm()
            .map_err(|error| self.latch.stop(error))?;
        sync(&self.connection, &mut self.queue, &mut self.state)
            .map_err(|error| self.latch.stop(error))?;
        for character in text.chars() {
            // A stop shortcut may still be physically held; its modifier would
            // combine with the key below into a compositor binding.
            self.latch.wait_for_hold()?;
            let keycode = match self.map.lookup(character) {
                Some(keycode) => keycode,
                None => {
                    let keycode = self.map.assign(character);
                    self.upload_keymap()
                        .map_err(|error| self.latch.stop(error))?;
                    keycode
                }
            };
            let time = self.epoch.elapsed().as_millis() as u32;
            self.keyboard.modifiers(0, 0, 0, 0);
            // The protocol takes evdev codes: XKB keycode minus 8.
            self.keyboard.key(time, keycode - 8, 1);
            self.keyboard.key(time, keycode - 8, 0);
            sync(&self.connection, &mut self.queue, &mut self.state)
                .map_err(|error| self.latch.stop(error))?;
            thread::sleep(KEY_PACING);
        }
        // Events only: a window change during this chunk is reported now, and
        // the next chunk confirms the target again before anything is typed.
        self.focus
            .drain_events()
            .map_err(|error| self.latch.stop(error))
    }
}

impl Drop for LiveTyper {
    fn drop(&mut self) {
        self.keyboard.destroy();
        let _ = self.connection.flush();
        self.focus.log(format!(
            "virtual keyboard destroyed after {} keymap upload(s)",
            self.uploads
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    fn guard_with(events: UnixStream, command_path: PathBuf) -> FocusGuard {
        events.set_nonblocking(true).unwrap();
        FocusGuard {
            command_path,
            events,
            pending: Vec::new(),
            address: "0xabc".into(),
            focused: true,
            label: "test".into(),
            logged: 0,
        }
    }

    /// A stand-in for Hyprland's command socket answering `j/activewindow`.
    struct FakeHyprland {
        path: PathBuf,
        responses: Arc<std::sync::Mutex<Vec<String>>>,
        served: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        _directory: tempfile::TempDir,
    }
    impl FakeHyprland {
        fn start(response: &str) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(".socket.sock");
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();
            let responses = Arc::new(std::sync::Mutex::new(vec![response.to_owned()]));
            let served = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (queue, count, done) = (responses.clone(), served.clone(), stop.clone());
            thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut request = [0u8; 64];
                            let _ = stream.read(&mut request);
                            let reply = {
                                let mut queue = queue.lock().unwrap();
                                if queue.len() > 1 {
                                    queue.remove(0)
                                } else {
                                    queue[0].clone()
                                }
                            };
                            let _ = stream.write_all(reply.as_bytes());
                            count.fetch_add(1, Ordering::AcqRel);
                        }
                        Err(_) => thread::sleep(Duration::from_millis(2)),
                    }
                }
            });
            Self {
                path,
                responses,
                served,
                stop,
                _directory: directory,
            }
        }
        fn answer(&self, response: &str) {
            self.responses.lock().unwrap().push(response.to_owned());
        }
    }
    impl Drop for FakeHyprland {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
        }
    }
    const TARGET: &str = r#"{"address":"0xabc","pid":4242,"class":"editor","workspace":{"id":1}}"#;
    const OTHER: &str = r#"{"address":"0xdef","pid":4243,"class":"browser","workspace":{"id":2}}"#;

    #[test]
    fn only_a_different_window_or_a_closed_target_is_a_loss() {
        assert_eq!(
            classify_event("activewindowv2>>abc", "0xabc"),
            FocusEvent::Target
        );
        assert_eq!(
            classify_event("activewindowv2>>0xabc", "abc"),
            FocusEvent::Target
        );
        assert_eq!(
            classify_event("activewindowv2>>def", "0xabc"),
            FocusEvent::OtherWindow
        );
        assert_eq!(
            classify_event("activewindowv2>>", "0xabc"),
            FocusEvent::NoWindow
        );
        assert_eq!(
            classify_event("closewindow>>abc", "0xabc"),
            FocusEvent::TargetClosed
        );
        assert_eq!(
            classify_event("closewindow>>def", "0xabc"),
            FocusEvent::Unrelated
        );
        for event in [
            "workspace>>2",
            "workspacev2>>2,name",
            "focusedmon>>DP-1,2",
            "focusedmonv2>>DP-1,2",
            "activespecial>>special,DP-1",
            "activespecialv2>>-98,special,DP-1",
            "openlayer>>swayosd",
            "closelayer>>swayosd",
            "submap>>resize",
            "configreloaded>>",
            "monitorremoved>>DP-2",
            "monitorremovedv2>>1,DP-2,desc",
            "monitoradded>>DP-2",
            "movewindow>>abc,2",
            "movewindowv2>>abc,2,name",
            "windowtitlev2>>abc,Typing, title",
            "activelayout>>keyboard,English",
            "urgent>>def",
            "openwindow>>def,2,browser,title",
            "garbage without separator",
            "",
        ] {
            assert_eq!(
                classify_event(event, "0xabc"),
                FocusEvent::Unrelated,
                "{event}"
            );
        }
    }

    #[test]
    fn diagnostics_never_carry_titles_or_names() {
        assert_eq!(
            describe_event("activewindowv2>>abc").as_deref(),
            Some("activewindowv2 abc")
        );
        assert_eq!(
            describe_event("workspacev2>>3,mail and secrets").as_deref(),
            Some("workspacev2 3")
        );
        assert_eq!(
            describe_event("openlayer>>walker").as_deref(),
            Some("openlayer")
        );
        assert_eq!(
            describe_event("workspace>>mail").as_deref(),
            Some("workspace")
        );
        assert_eq!(describe_event("activewindow>>class,Secret title"), None);
        assert_eq!(describe_event("windowtitlev2>>abc,Secret title"), None);
        assert_eq!(describe_event("no separator"), None);
    }

    #[test]
    fn desktop_events_with_an_unchanged_address_do_not_stop_typing() {
        let hyprland = FakeHyprland::start(TARGET);
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        writer
            .write_all(b"workspace>>2\nworkspacev2>>2,two\nopenlayer>>swayosd\nsubmap>>resize\nconfigreloaded>>\nfocusedmon>>DP-1,2\nactivespecial>>special,DP-1\nmonitoraddedv2>>1,DP-2,d\ncloselayer>>swayosd\nactivewindowv2>>abc\n")
            .unwrap();
        assert_eq!(guard.confirm(), Ok(()));
        assert!(guard.focused);
        assert_eq!(hyprland.served.load(Ordering::Acquire), 1);
        assert_eq!(guard.confirm(), Ok(()));
        assert_eq!(hyprland.served.load(Ordering::Acquire), 2);
    }

    #[test]
    fn a_confirmed_window_change_is_a_loss_even_after_focus_returns() {
        let hyprland = FakeHyprland::start(TARGET);
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        writer
            .write_all(b"activewindowv2>>def\nactivewindowv2>>abc\n")
            .unwrap();
        assert_eq!(guard.confirm(), Err(LOST_OTHER_WINDOW.to_owned()));
        // No query is made once the events already prove the loss.
        assert_eq!(hyprland.served.load(Ordering::Acquire), 0);
    }

    #[test]
    fn the_query_catches_a_change_the_events_missed() {
        let hyprland = FakeHyprland::start(OTHER);
        let (reader, _writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        assert_eq!(guard.confirm(), Err(LOST_OTHER_WINDOW.to_owned()));
        let hyprland = FakeHyprland::start("{}");
        let (reader, _writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        assert_eq!(guard.confirm(), Err(LOST_NO_FOCUS.to_owned()));
    }

    #[test]
    fn closing_the_target_window_is_a_loss() {
        let hyprland = FakeHyprland::start(TARGET);
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        writer.write_all(b"closewindow>>def\n").unwrap();
        assert_eq!(guard.confirm(), Ok(()));
        writer.write_all(b"closewindow>>abc\n").unwrap();
        assert_eq!(guard.drain_events(), Err(LOST_TARGET_CLOSED.to_owned()));
    }

    #[test]
    fn keyboard_focus_leaving_every_window_blocks_only_while_it_lasts() {
        let hyprland = FakeHyprland::start(TARGET);
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        // A launcher opened and closed between two deltas is harmless.
        writer
            .write_all(
                b"openlayer>>walker\nactivewindowv2>>\ncloselayer>>walker\nactivewindowv2>>abc\n",
            )
            .unwrap();
        assert_eq!(guard.confirm(), Ok(()));
        // A launcher still open when text must go out is a loss for this text.
        writer.write_all(b"activewindowv2>>\n").unwrap();
        assert_eq!(guard.confirm(), Err(LOST_NO_FOCUS.to_owned()));
        assert_eq!(hyprland.served.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_dead_command_socket_is_a_technical_fault_after_retries() {
        let directory = tempfile::tempdir().unwrap();
        let (reader, _writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, directory.path().join("missing.sock"));
        let started = Instant::now();
        let error = guard.confirm().unwrap_err();
        assert!(error.contains("technical fault"), "{error}");
        assert!(!error.contains("focus changed"), "{error}");
        assert!(error.contains("refused the connection"), "{error}");
        assert!(started.elapsed() >= QUERY_RETRY_DELAY * (QUERY_ATTEMPTS as u32 - 1));
    }

    #[test]
    fn one_missed_query_is_retried_without_stopping() {
        // The first answer is malformed, every later one is the target.
        let hyprland = FakeHyprland::start("not json");
        hyprland.answer(TARGET);
        let (reader, _writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        assert_eq!(guard.confirm(), Ok(()));
        assert_eq!(hyprland.served.load(Ordering::Acquire), 2);
    }

    #[test]
    fn a_closed_event_socket_is_a_technical_fault_not_a_focus_change() {
        let hyprland = FakeHyprland::start(TARGET);
        let (reader, writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        drop(writer);
        let error = guard.drain_events().unwrap_err();
        assert!(error.contains("technical fault"), "{error}");
        assert!(error.contains("event socket was closed"), "{error}");
    }

    #[test]
    fn fragmented_events_are_retained_until_complete() {
        let hyprland = FakeHyprland::start(TARGET);
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut guard = guard_with(reader, hyprland.path.clone());
        writer
            .write_all(b"activewindowv2>>abc\nactivewindowv2>>de")
            .unwrap();
        assert_eq!(guard.drain_events(), Ok(()));
        assert_eq!(guard.pending, b"activewindowv2>>de");
        writer.write_all(b"f\n").unwrap();
        assert_eq!(guard.drain_events(), Err(LOST_OTHER_WINDOW.to_owned()));
    }

    #[test]
    fn a_stale_guard_from_an_earlier_session_cannot_touch_a_new_one() {
        // Each session owns its own event socket and target; late events on
        // the old socket change nothing for the new guard.
        let hyprland = FakeHyprland::start(TARGET);
        let (old_reader, mut old_writer) = UnixStream::pair().unwrap();
        let mut old = guard_with(old_reader, hyprland.path.clone());
        let (new_reader, mut new_writer) = UnixStream::pair().unwrap();
        let mut new = guard_with(new_reader, hyprland.path.clone());
        old_writer.write_all(b"activewindowv2>>def\n").unwrap();
        assert!(old.drain_events().is_err());
        drop(old);
        new_writer.write_all(b"workspace>>1\n").unwrap();
        assert_eq!(new.confirm(), Ok(()));
        assert!(new.focused);
    }

    #[test]
    fn frequent_characters_take_quiet_keys_and_no_forbidden_key_is_ever_used() {
        let mut map = KeycodeMap::new();
        for character in " enirstad.,".chars() {
            let keycode = map.lookup(character).unwrap();
            assert!(
                QUIET_KEYCODES.contains(&keycode),
                "{character:?} on {keycode}"
            );
        }
        assert!(map.lookup('s').is_some(), "Chromium must receive an s");
        // Escape, Backspace, Tab, Return, modifiers, locks, Print, navigation,
        // KP_Enter, F1–F12, media/brightness/power keys, keys without a DOM code.
        let forbidden = [
            9, 22, 23, 36, 37, 50, 62, 64, 66, 77, 78, 92, 63, 82, 91, 103, 104, 105, 107, 108,
            109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 128,
            133, 134, 135, 136, 146, 148, 150, 151, 152, 158, 160, 163, 164, 166, 167, 169, 171,
            172, 173, 174, 175, 176, 177, 180, 181, 182, 198, 199, 200, 201, 209, 212, 213, 214,
            215, 216, 218, 225, 232, 233, 248,
        ];
        for range in [67..=76, 95..=96] {
            for keycode in range {
                assert!(
                    !QUIET_KEYCODES.contains(&keycode) && !ORDINARY_KEYCODES.contains(&keycode)
                );
            }
        }
        let mut seen = std::collections::HashSet::new();
        for keycode in QUIET_KEYCODES.iter().chain(ORDINARY_KEYCODES) {
            assert!(
                !forbidden.contains(keycode),
                "keycode {keycode} is forbidden"
            );
            assert!((9..=255).contains(keycode));
            assert!(seen.insert(*keycode), "keycode {keycode} listed twice");
        }
        assert!(map.slots.len() > SEED.chars().count() + 16);
        assert!(!map.keymap().contains("Return"));
        assert!(!map.keymap().contains("include"));
    }

    #[test]
    fn new_characters_get_free_keys_and_recycling_keeps_the_recent_ones() {
        let mut map = KeycodeMap::new();
        let umlaut = map.assign('ä');
        assert_ne!(umlaut, 172, "ä must never sit on XF86AudioPlay");
        assert!(!QUIET_KEYCODES.contains(&172));
        assert_eq!(map.lookup('ä'), Some(umlaut));
        let mut filler = 0x4E00u32;
        while map.slots.iter().any(|slot| slot.character.is_none()) {
            map.assign(char::from_u32(filler).unwrap());
            filler += 1;
        }
        // Everything is taken: the least recently used slot goes, not 'ä' or
        // the seed characters used since.
        assert!(map.lookup('e').is_some());
        let recycled = map.assign('€');
        assert_eq!(map.lookup('€'), Some(recycled));
        assert_eq!(map.lookup('ä'), Some(umlaut));
        assert!(map.lookup('e').is_some());
        assert!(map.keymap().contains("U20AC"));
    }

    #[test]
    fn unicode_keymap_roundtrips_through_the_real_xkb_parser() {
        // Load the compositor's ubiquitous runtime library without introducing
        // a development-header/linker dependency into release builds.
        unsafe {
            let library = libc::dlopen(c"libxkbcommon.so.0".as_ptr(), libc::RTLD_NOW);
            assert!(
                !library.is_null(),
                "libxkbcommon runtime is needed for this Linux test"
            );
            type NewContext = unsafe extern "C" fn(u32) -> *mut libc::c_void;
            type NewMap = unsafe extern "C" fn(
                *mut libc::c_void,
                *const libc::c_char,
                u32,
                u32,
            ) -> *mut libc::c_void;
            type NewState = unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void;
            type Utf32 = unsafe extern "C" fn(*mut libc::c_void, u32) -> u32;
            type Unref = unsafe extern "C" fn(*mut libc::c_void);
            let context_new: NewContext =
                std::mem::transmute(libc::dlsym(library, c"xkb_context_new".as_ptr()));
            let map_new: NewMap =
                std::mem::transmute(libc::dlsym(library, c"xkb_keymap_new_from_string".as_ptr()));
            let state_new: NewState =
                std::mem::transmute(libc::dlsym(library, c"xkb_state_new".as_ptr()));
            let utf32: Utf32 =
                std::mem::transmute(libc::dlsym(library, c"xkb_state_key_get_utf32".as_ptr()));
            let context_unref: Unref =
                std::mem::transmute(libc::dlsym(library, c"xkb_context_unref".as_ptr()));
            let map_unref: Unref =
                std::mem::transmute(libc::dlsym(library, c"xkb_keymap_unref".as_ptr()));
            let state_unref: Unref =
                std::mem::transmute(libc::dlsym(library, c"xkb_state_unref".as_ptr()));
            let context = context_new(0);
            assert!(!context.is_null());
            let mut keycodes = KeycodeMap::new();
            let characters = [' ', 'A', 'ä', '中', '😀', '\u{301}', 's', '-'];
            let assigned: Vec<u32> = characters
                .iter()
                .map(|character| {
                    keycodes
                        .lookup(*character)
                        .unwrap_or_else(|| keycodes.assign(*character))
                })
                .collect();
            let source = keycodes.keymap();
            let map = map_new(context, source.as_ptr().cast(), 1, 0);
            assert!(!map.is_null(), "generated keymap must parse");
            let state = state_new(map);
            assert!(!state.is_null());
            assert_eq!(utf32(state, 8), 0, "initialization key produces no text");
            for (character, keycode) in characters.iter().zip(assigned) {
                assert_eq!(utf32(state, keycode), *character as u32, "{character:?}");
            }
            state_unref(state);
            map_unref(map);
            context_unref(context);
            libc::dlclose(library);
        }
    }
}
