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
}
