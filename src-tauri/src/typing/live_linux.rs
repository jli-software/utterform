//! Omarchy/Hyprland-only live input. One Wayland virtual keyboard lives for the
//! whole session; Hyprland's event socket records even quick away/back switches.
//! No wtype child per delta, X11 fallback, clipboard write, or focus restoration.
//!
//! References: https://wiki.hypr.land/IPC/ and virtual-keyboard-unstable-v1.
//! Hyprland IPC identifies windows, not fields/cursors or physical key state.
//! The virtual keyboard uses its own zero modifier state. Concurrent physical
//! typing (including shortcut keys) during dictation is not supported.
use super::StopLatch;
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
        .map_err(|_| "Wayland input connection failed")?;
    let until = Instant::now() + IO_TIMEOUT;
    while !state.synced {
        queue
            .dispatch_pending(state)
            .map_err(|_| "Wayland input observer failed")?;
        if state.removed {
            return Err("Wayland keyboard or seat disappeared".into());
        }
        if state.synced {
            break;
        }
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("Wayland input connection timed out".into());
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
            return Err("Wayland input connection timed out or failed".into());
        }
        guard
            .read()
            .map_err(|_| "Wayland input connection closed")?;
    }
    Ok(())
}

fn normalize_address(address: &str) -> &str {
    address.strip_prefix("0x").unwrap_or(address)
}

fn event_loses_target(line: &str, target: &str) -> bool {
    let Some((event, data)) = line.split_once(">>") else {
        return true;
    };
    match event {
        "activewindowv2" => normalize_address(data) != normalize_address(target),
        "closewindow" | "kill" | "movewindow" | "movewindowv2" => {
            normalize_address(data.split(',').next().unwrap_or_default())
                == normalize_address(target)
        }
        // Conservative: a layer may be a launcher taking keyboard focus, and
        // workspace/monitor/submap changes are explicit user context changes.
        "workspace" | "workspacev2" | "focusedmon" | "focusedmonv2" | "activespecial"
        | "activespecialv2" | "openlayer" | "submap" | "configreloaded" | "monitorremoved"
        | "monitorremovedv2" => true,
        _ => false,
    }
}

struct FocusGuard {
    command_path: PathBuf,
    events: UnixStream,
    pending: Vec<u8>,
    address: String,
}

impl FocusGuard {
    fn capture() -> Result<Self, String> {
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
        };
        let window = guard.active_window()?;
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
        guard.address = address.to_owned();
        guard.check()?;
        Ok(guard)
    }

    fn active_window(&self) -> Result<serde_json::Value, String> {
        // Hyprland's command socket is synchronous: write immediately after
        // connect and always close after reading; never keep this socket idle.
        let mut stream =
            UnixStream::connect(&self.command_path).map_err(|_| "Hyprland focus query failed")?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|_| "Hyprland focus timeout could not be set")?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|_| "Hyprland focus timeout could not be set")?;
        stream
            .write_all(b"j/activewindow")
            .map_err(|_| "Hyprland focus query failed")?;
        let mut response = Vec::new();
        stream
            .take(MAX_EVENTS as u64 + 1)
            .read_to_end(&mut response)
            .map_err(|_| "Hyprland focus query timed out")?;
        if response.len() > MAX_EVENTS {
            return Err("Hyprland focus response exceeded its limit".into());
        }
        serde_json::from_slice(&response)
            .map_err(|_| "Hyprland returned an invalid focus response".into())
    }

    fn drain_events(&mut self) -> Result<(), String> {
        let mut buffer = [0u8; 4096];
        loop {
            match self.events.read(&mut buffer) {
                Ok(0) => return Err("Hyprland focus observer disconnected".into()),
                Ok(count) => {
                    self.pending.extend_from_slice(&buffer[..count]);
                    if self.pending.len() > MAX_EVENTS {
                        return Err("Hyprland focus event buffer overflowed".into());
                    }
                    while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                        let line = std::str::from_utf8(&self.pending[..end])
                            .map_err(|_| "Invalid Hyprland focus event")?;
                        if event_loses_target(line, &self.address) {
                            return Err("Live typing paused because the target or desktop focus changed; start a new dictation to continue".into());
                        }
                        self.pending.drain(..=end);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("Hyprland focus observer failed".into()),
            }
        }
    }

    fn check(&mut self) -> Result<(), String> {
        self.drain_events()?;
        let window = self.active_window()?;
        if window.get("address").and_then(serde_json::Value::as_str) != Some(self.address.as_str())
        {
            return Err("Live typing paused because the target lost focus".into());
        }
        // Also observe events produced during the synchronous focus query.
        self.drain_events()
    }
}

/// Self-contained XKB keymap, independent of the physical keyboard layout.
/// ONE_LEVEL avoids capitalization/AltGr transforms; only literal Unicode
/// keysyms are mapped, so no Return, Backspace, Tab, or shortcut key exists.
fn keymap_for(characters: &[char]) -> String {
    let mut map =
        String::from("xkb_keymap { xkb_keycodes \"utterform\" { minimum = 8; maximum = 255;\n");
    for (index, _) in characters.iter().enumerate() {
        map.push_str(&format!("<K{index}> = {};\n", index + 9));
    }
    map.push_str("}; xkb_types \"utterform\" { type \"ONE_LEVEL\" { modifiers = None; map[None] = Level1; level_name[Level1] = \"Any\"; }; }; xkb_compatibility \"utterform\" {}; xkb_symbols \"utterform\" {\n");
    for (index, character) in characters.iter().enumerate() {
        map.push_str(&format!(
            "key <K{index}> {{ type[Group1] = \"ONE_LEVEL\", [ U{:04X} ] }};\n",
            *character as u32
        ));
    }
    map.push_str("}; };\0");
    map
}

pub(super) struct LiveTyper {
    focus: FocusGuard,
    connection: Connection,
    queue: EventQueue<WaylandState>,
    state: WaylandState,
    keyboard: ZwpVirtualKeyboardV1,
    characters: Vec<char>,
    epoch: Instant,
    latch: StopLatch,
}

impl LiveTyper {
    pub(super) fn set_cancel_flag(&mut self, flag: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.latch.cancel = Some(flag);
    }

    pub(super) fn capture() -> Result<Self, String> {
        let focus = FocusGuard::capture()?;
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
            characters: (' '..='~').chain('\u{a0}'..='\u{ff}').collect(),
            epoch: Instant::now(),
            latch: StopLatch::default(),
        };
        session.upload_keymap()?;
        session.check_target()?;
        Ok(session)
    }

    fn upload_keymap(&mut self) -> Result<(), String> {
        let map = keymap_for(&self.characters);
        let mut file =
            tempfile::tempfile().map_err(|_| "Could not create the live Unicode keymap")?;
        file.write_all(map.as_bytes())
            .map_err(|_| "Could not write the live Unicode keymap")?;
        self.keyboard.keymap(1, file.as_fd(), map.len() as u32);
        self.keyboard.modifiers(0, 0, 0, 0);
        sync(&self.connection, &mut self.queue, &mut self.state)
    }

    pub(super) fn check_target(&mut self) -> Result<(), String> {
        self.latch.check()?;
        self.focus.check().map_err(|error| self.latch.stop(error))?;
        // Includes a bounded compositor roundtrip, so no dead input connection
        // silently consumes the stream while no text reaches the application.
        sync(&self.connection, &mut self.queue, &mut self.state)
            .map_err(|error| self.latch.stop(error))
    }

    pub(super) fn insert(&mut self, text: &str) -> Result<(), String> {
        self.check_target()?;
        for character in text.chars() {
            let index = match self.characters.iter().position(|c| *c == character) {
                Some(index) => index,
                None => {
                    // Stay within the core XKB/XWayland keycode range. Keep the
                    // common Latin mapping and recycle only extension slots.
                    if self.characters.len() >= 240 {
                        self.characters.truncate(191);
                    }
                    self.characters.push(character);
                    self.upload_keymap()
                        .map_err(|error| self.latch.stop(error))?;
                    self.characters.len() - 1
                }
            };
            self.check_target()?;
            let time = self.epoch.elapsed().as_millis() as u32;
            self.keyboard.modifiers(0, 0, 0, 0);
            // XKB adds 8 to the protocol's evdev code: K0=9 => protocol key 1.
            self.keyboard.key(time, index as u32 + 1, 1);
            self.keyboard.key(time, index as u32 + 1, 0);
            sync(&self.connection, &mut self.queue, &mut self.state)
                .map_err(|error| self.latch.stop(error))?;
            thread::sleep(Duration::from_millis(5));
        }
        self.check_target()
    }
}

impl Drop for LiveTyper {
    fn drop(&mut self) {
        self.keyboard.destroy();
        let _ = self.connection.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_events_include_away_and_back_and_ignore_title_changes() {
        assert!(!event_loses_target("activewindowv2>>abc", "0xabc"));
        assert!(event_loses_target("activewindowv2>>def", "0xabc"));
        assert!(event_loses_target("activewindowv2>>", "0xabc"));
        assert!(!event_loses_target(
            "windowtitlev2>>abc,Typing, title",
            "0xabc"
        ));
        assert!(event_loses_target("closewindow>>abc", "0xabc"));
        assert!(!event_loses_target("closewindow>>def", "0xabc"));
        for event in [
            "workspace>>2",
            "openlayer>>launcher",
            "submap>>resize",
            "configreloaded>>",
        ] {
            assert!(event_loses_target(event, "0xabc"));
        }
    }

    #[test]
    fn unicode_keymap_has_literal_symbols_and_evdev_offset() {
        let map = keymap_for(&['A', 'ä', '中', '😀']);
        assert!(map.contains("<K0> = 9;"));
        for symbol in ["U0041", "U00E4", "U4E2D", "U1F600"] {
            assert!(map.contains(symbol));
        }
        assert!(map.ends_with('\0'));
        assert!(!map.contains("Return"));
        assert!(!map.contains("include"));
    }

    #[test]
    fn fragmented_socket_events_are_retained_until_complete_and_eof_fails() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let mut guard = FocusGuard {
            command_path: PathBuf::new(),
            events: reader,
            pending: Vec::new(),
            address: "0xabc".into(),
        };
        writer
            .write_all(b"activewindowv2>>abc\nactivewindowv2>>de")
            .unwrap();
        assert!(guard.drain_events().is_ok());
        writer.write_all(b"f\nactivewindowv2>>abc\n").unwrap();
        assert!(guard.drain_events().is_err());
        let (reader, writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        guard.events = reader;
        guard.pending.clear();
        drop(writer);
        assert!(guard.drain_events().is_err());
    }
}
