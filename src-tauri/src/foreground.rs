//! The application in front when a dictation starts, for the actions chosen
//! per application.
//!
//! Read once, when the dictation key is pressed, and never logged: an
//! application's name says what someone is doing as clearly as a window title.
//! Utterform itself is never the answer, so a recording started from its own
//! window follows the action chosen there.
//!
//! Linux asks the compositor (Hyprland, Sway) or, on X11, the window manager's
//! `_NET_ACTIVE_WINDOW` through `xprop`. GNOME and KDE on Wayland tell no
//! application which window is focused, so there the feature stays off.
//! Windows names the foreground window's program, macOS the frontmost
//! application.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForegroundApp {
    /// What a rule is matched against: the Wayland/X11 class, the program's
    /// file name on Windows, the bundle identifier on macOS.
    pub id: String,
    /// What Settings shows.
    pub name: String,
}

/// Whether this session can say which application is in front.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Support {
    pub supported: bool,
    /// Empty when supported.
    pub explanation: String,
}

pub fn support() -> Support {
    #[cfg(target_os = "linux")]
    {
        if linux::backend().is_some() {
            return Support {
                supported: true,
                explanation: String::new(),
            };
        }
        Support {
            supported: false,
            explanation: linux::unsupported_message().into(),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Support {
            supported: true,
            explanation: String::new(),
        }
    }
}

/// Where the answer comes from, as a diagnostic event may name it.
pub fn source() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        match linux::backend() {
            Some(linux::Backend::Hyprland) => "hyprland",
            Some(linux::Backend::Sway) => "sway",
            Some(linux::Backend::X11) => "x11",
            None => "none",
        }
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
}

/// The application in front, unless it is Utterform or cannot be told.
pub fn current() -> Option<ForegroundApp> {
    let own = std::process::id();
    #[cfg(target_os = "linux")]
    {
        linux::current(own)
    }
    #[cfg(target_os = "windows")]
    {
        windows::current(own)
    }
    #[cfg(target_os = "macos")]
    {
        macos::current(own)
    }
}

/// A Linux class or a program name as a person would write it:
/// `org.mozilla.Thunderbird` → `Thunderbird`, `com.mitchellh.ghostty` →
/// `Ghostty`, `thunderbird` → `Thunderbird`. A name with capitals of its own
/// (`OUTLOOK`, `WhatsApp`) is kept as it is.
pub fn display_name(id: &str) -> String {
    let id = id.trim();
    let last = if id.contains('.') && !id.ends_with('.') {
        id.rsplit('.').next().unwrap_or(id)
    } else {
        id
    };
    if last.chars().any(char::is_uppercase) {
        return last.to_string();
    }
    let mut chars = last.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// An application from what the platform reported, unless it is Utterform.
fn found(id: Option<&str>, pid: Option<u32>, own: u32) -> Option<ForegroundApp> {
    if pid == Some(own) {
        return None;
    }
    let id = id.map(str::trim).filter(|id| !id.is_empty())?;
    Some(ForegroundApp {
        id: id.to_string(),
        name: display_name(id),
    })
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use serde_json::Value;

    use super::{ForegroundApp, found};

    /// The key was just pressed; a compositor that does not answer by then
    /// must not hold up the recording.
    const TIMEOUT: Duration = Duration::from_millis(400);
    const MAX_RESPONSE: u64 = 4 * 1024 * 1024;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Backend {
        Hyprland,
        Sway,
        X11,
    }

    pub fn backend() -> Option<Backend> {
        let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        choose(
            set("HYPRLAND_INSTANCE_SIGNATURE"),
            set("SWAYSOCK"),
            set("WAYLAND_DISPLAY"),
            set("DISPLAY"),
            installed("swaymsg"),
            installed("xprop"),
        )
    }

    fn installed(program: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
        })
    }

    /// The compositor first: on Wayland the X11 answer only knows XWayland
    /// windows, and would mistake every native one for none at all.
    pub fn choose(
        hyprland: bool,
        sway: bool,
        wayland: bool,
        x11: bool,
        swaymsg: bool,
        xprop: bool,
    ) -> Option<Backend> {
        if hyprland {
            Some(Backend::Hyprland)
        } else if sway && swaymsg {
            Some(Backend::Sway)
        } else if !wayland && x11 && xprop {
            Some(Backend::X11)
        } else {
            None
        }
    }

    pub fn unsupported_message() -> &'static str {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        if wayland {
            "Works on Hyprland, Sway and X11. This desktop does not say which app is in front."
        } else {
            "Install xprop to use this."
        }
    }

    pub fn current(own: u32) -> Option<ForegroundApp> {
        match backend()? {
            Backend::Hyprland => hyprland(own),
            Backend::Sway => run("swaymsg", &["-t", "get_tree", "-r"])
                .and_then(|tree| serde_json::from_str::<Value>(&tree).ok())
                .and_then(|tree| sway_focused(&tree, own)),
            Backend::X11 => {
                let root = run("xprop", &["-root", "_NET_ACTIVE_WINDOW"])?;
                let window = xprop_active_window(&root)?;
                let props = run("xprop", &["-id", &window, "WM_CLASS", "_NET_WM_PID"])?;
                let (class, pid) = xprop_class(&props);
                found(class.as_deref(), pid, own)
            }
        }
    }

    fn hyprland(own: u32) -> Option<ForegroundApp> {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        if signature.is_empty() || signature.contains('/') || signature == "." || signature == ".."
        {
            return None;
        }
        let path = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?)
            .join("hypr")
            .join(signature)
            .join(".socket.sock");
        // The command socket answers once and closes; it is never kept open.
        let mut stream = UnixStream::connect(path).ok()?;
        stream.set_read_timeout(Some(TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(TIMEOUT)).ok()?;
        stream.write_all(b"j/activewindow").ok()?;
        let mut response = Vec::new();
        stream.take(MAX_RESPONSE).read_to_end(&mut response).ok()?;
        hyprland_window(&serde_json::from_slice(&response).ok()?, own)
    }

    pub fn hyprland_window(window: &Value, own: u32) -> Option<ForegroundApp> {
        let class = ["class", "initialClass"]
            .iter()
            .filter_map(|key| window.get(key)?.as_str())
            .find(|class| !class.trim().is_empty());
        let pid = window
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok());
        found(class, pid, own)
    }

    /// The focused leaf of Sway's tree: `app_id` for Wayland windows, the X11
    /// class for XWayland ones.
    pub fn sway_focused(node: &Value, own: u32) -> Option<ForegroundApp> {
        let is_window = node.get("pid").is_some();
        if is_window && node.get("focused").and_then(Value::as_bool) == Some(true) {
            let id = node
                .get("app_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .or_else(|| node.pointer("/window_properties/class")?.as_str());
            let pid = node
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok());
            return found(id, pid, own);
        }
        ["nodes", "floating_nodes"]
            .iter()
            .filter_map(|key| node.get(key)?.as_array())
            .flatten()
            .find_map(|child| sway_focused(child, own))
    }

    /// `_NET_ACTIVE_WINDOW(WINDOW): window id # 0x3a00007` → `0x3a00007`.
    pub fn xprop_active_window(output: &str) -> Option<String> {
        let id = output.rsplit('#').next()?.trim();
        let id = id.split(|c: char| c == ',' || c.is_whitespace()).next()?;
        (id.starts_with("0x") && id != "0x0").then(|| id.to_string())
    }

    /// `WM_CLASS(STRING) = "navigator", "firefox"` names the class second.
    pub fn xprop_class(output: &str) -> (Option<String>, Option<u32>) {
        let mut class = None;
        let mut pid = None;
        for line in output.lines() {
            if let Some(rest) = line.strip_prefix("WM_CLASS(STRING) = ") {
                class = rest
                    .split(", ")
                    .map(|part| part.trim().trim_matches('"').to_string())
                    .filter(|part| !part.is_empty())
                    .last();
            } else if let Some(rest) = line.strip_prefix("_NET_WM_PID(CARDINAL) = ") {
                pid = rest.trim().parse().ok();
            }
        }
        (class, pid)
    }

    fn run(program: &str, args: &[&str]) -> Option<String> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = (&mut stdout).take(MAX_RESPONSE).read_to_end(&mut output);
            output
        });
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(_)) | Err(_) => return None,
                Ok(None) if started.elapsed() > TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        String::from_utf8(reader.join().ok()?).ok()
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };

    use super::{ForegroundApp, found};

    /// The foreground window's program, by its file name without `.exe`.
    pub fn current(own: u32) -> Option<ForegroundApp> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_null() {
            return None;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == 0 {
            return None;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let mut length = path.len() as u32;
        let named =
            unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) } != 0;
        unsafe { CloseHandle(process) };
        if !named {
            return None;
        }
        let path = String::from_utf16_lossy(&path[..length as usize]);
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned());
        found(stem.as_deref(), Some(pid), own)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::NSWorkspace;

    use super::{ForegroundApp, found};

    /// The frontmost application, by bundle identifier, with the name macOS
    /// shows for it.
    pub fn current(own: u32) -> Option<ForegroundApp> {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = u32::try_from(application.processIdentifier()).ok();
        let name = application.localizedName().map(|name| name.to_string());
        let id = application
            .bundleIdentifier()
            .map(|id| id.to_string())
            .or_else(|| name.clone());
        let app = found(id.as_deref(), pid, own)?;
        Some(ForegroundApp {
            name: name.unwrap_or(app.name),
            id: app.id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_the_way_people_write_them() {
        assert_eq!(display_name("org.mozilla.Thunderbird"), "Thunderbird");
        assert_eq!(display_name("com.mitchellh.ghostty"), "Ghostty");
        assert_eq!(display_name("thunderbird"), "Thunderbird");
        assert_eq!(display_name("OUTLOOK"), "OUTLOOK");
        assert_eq!(display_name("WhatsApp"), "WhatsApp");
        assert_eq!(display_name("Alacritty"), "Alacritty");
    }

    #[test]
    fn utterform_itself_is_never_the_application_in_front() {
        assert_eq!(found(Some("Utterform"), Some(42), 42), None);
        assert_eq!(found(Some("  "), Some(7), 42), None);
        assert_eq!(
            found(Some("thunderbird"), Some(7), 42),
            Some(ForegroundApp {
                id: "thunderbird".into(),
                name: "Thunderbird".into()
            })
        );
    }

    #[cfg(target_os = "linux")]
    mod linux_parsing {
        use super::super::linux::*;
        use serde_json::json;

        #[test]
        fn the_compositor_is_asked_before_x11() {
            assert_eq!(
                choose(true, false, true, true, false, true),
                Some(Backend::Hyprland)
            );
            assert_eq!(
                choose(false, true, true, true, true, true),
                Some(Backend::Sway)
            );
            assert_eq!(
                choose(false, false, false, true, false, true),
                Some(Backend::X11)
            );
            // GNOME or KDE on Wayland: XWayland would only know its own windows.
            assert_eq!(choose(false, false, true, true, false, true), None);
            assert_eq!(choose(false, true, true, false, false, false), None);
        }

        #[test]
        fn hyprland_names_the_class_and_skips_utterform() {
            let window = json!({"class": "thunderbird", "initialClass": "thunderbird", "pid": 7});
            assert_eq!(hyprland_window(&window, 42).unwrap().id, "thunderbird");
            assert_eq!(hyprland_window(&window, 7), None);
            let no_class = json!({"class": "", "initialClass": "Alacritty", "pid": 7});
            assert_eq!(hyprland_window(&no_class, 42).unwrap().name, "Alacritty");
            // No window focused: Hyprland answers with an empty object.
            assert_eq!(hyprland_window(&json!({}), 42), None);
        }

        #[test]
        fn sway_finds_the_focused_window_also_among_floating_ones() {
            let tree = json!({"focused": false, "nodes": [
                {"focused": false, "nodes": [
                    {"focused": false, "pid": 3, "app_id": "foot", "nodes": []}
                ], "floating_nodes": [
                    {"focused": true, "pid": 5, "app_id": null,
                     "window_properties": {"class": "Slack"}, "nodes": []}
                ]}
            ]});
            assert_eq!(sway_focused(&tree, 42).unwrap().id, "Slack");
            let workspace_focused = json!({"focused": true, "nodes": []});
            assert_eq!(sway_focused(&workspace_focused, 42), None);
        }

        #[test]
        fn xprop_output_gives_window_class_and_pid() {
            assert_eq!(
                xprop_active_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x3a00007\n"),
                Some("0x3a00007".into())
            );
            assert_eq!(
                xprop_active_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n"),
                None
            );
            let (class, pid) = xprop_class(
                "WM_CLASS(STRING) = \"navigator\", \"firefox\"\n_NET_WM_PID(CARDINAL) = 1234\n",
            );
            assert_eq!(class.as_deref(), Some("firefox"));
            assert_eq!(pid, Some(1234));
        }
    }
}
#[cfg(test)]
mod live_probe {
    #[test]
    #[ignore = "asks the running desktop"]
    fn print_the_application_in_front() {
        eprintln!("support: {:?}", super::support());
        eprintln!("in front: {:?}", super::current());
    }
}
