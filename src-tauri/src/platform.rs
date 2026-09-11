//! Window policy belongs to the app, not to edits of the user's compositor config.

pub fn use_borderless_window() -> bool {
    #[cfg(target_os = "linux")]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let omarchy_path = std::env::var_os("OMARCHY_PATH")
            .is_some_and(|path| std::path::Path::new(&path).is_dir());
        let installed = std::path::Path::new("/usr/share/omarchy").is_dir()
            || std::env::var_os("HOME").is_some_and(|home| {
                std::path::Path::new(&home)
                    .join(".local/share/omarchy")
                    .is_dir()
            });
        is_omarchy_session(&desktop, omarchy_path || installed)
    }
    #[cfg(not(target_os = "linux"))]
    false
}

#[cfg(any(target_os = "linux", test))]
fn is_omarchy_session(desktop: &str, omarchy_present: bool) -> bool {
    omarchy_present
        && desktop
            .split(':')
            .any(|part| part.eq_ignore_ascii_case("Hyprland"))
}

/// The graphics backend an autostart launch has to be given, or `None` when
/// the session already chose one.
///
/// Every other Linux start goes through the launcher the installer writes,
/// which pins `GDK_BACKEND=x11`; an XDG autostart entry runs the executable
/// directly and would otherwise hand a login start a different backend than
/// the same app gets from the app menu. Applying the launcher's policy here
/// keeps one behaviour rather than two, and a backend the user set themselves
/// still wins.
#[cfg(any(target_os = "linux", test))]
pub fn autostart_gdk_backend(configured: Option<&str>) -> Option<&'static str> {
    match configured {
        Some(value) if !value.trim().is_empty() => None,
        _ => Some("x11"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_omarchy_hyprland_sessions_are_borderless() {
        assert!(is_omarchy_session("Hyprland", true));
        assert!(is_omarchy_session("Hyprland:wlroots", true));
        assert!(!is_omarchy_session("Hyprland", false));
        assert!(!is_omarchy_session("GNOME", true));
        assert!(!is_omarchy_session("KDE", true));
        assert!(!is_omarchy_session("", false));
    }

    #[test]
    fn an_autostart_launch_gets_the_backend_the_launcher_would_have_given_it() {
        assert_eq!(autostart_gdk_backend(None), Some("x11"));
        assert_eq!(autostart_gdk_backend(Some("")), Some("x11"));
        assert_eq!(autostart_gdk_backend(Some("  ")), Some("x11"));
    }

    #[test]
    fn a_backend_the_session_chose_is_left_alone() {
        assert_eq!(autostart_gdk_backend(Some("wayland")), None);
        assert_eq!(autostart_gdk_backend(Some("x11")), None);
    }
}
