//! Opening Utterform always means the running instance, never a second one.
//!
//! Tray clicks, the app launcher and a second process started by hand all end
//! up here so the one existing window is revealed instead of duplicated.

use tauri::{
    AppHandle, Manager, Runtime,
    tray::{MouseButton, MouseButtonState, TrayIconEvent},
};

/// Reveal the main window wherever it is: hidden in the tray, minimized, or
/// merely unfocused. Each step is independently best-effort because a window
/// that is already shown or already focused must not block the others.
pub fn reveal_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// A plain left click and a double click both mean "open Utterform". Reacting
/// to the button release keeps the menu-less left click from opening the window
/// while the button is still down; a double click reports both and revealing an
/// already visible window is harmless.
///
/// Linux delivers no tray clicks at all: libayatana-appindicator exposes only a
/// menu over StatusNotifierItem, so the tray menu stays the supported path
/// there.
pub fn opens_main_window(event: &TrayIconEvent) -> bool {
    matches!(
        event,
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::{PhysicalPosition, Rect, tray::TrayIconId};

    fn click(button: MouseButton, button_state: MouseButtonState) -> TrayIconEvent {
        TrayIconEvent::Click {
            id: TrayIconId::new("main"),
            position: PhysicalPosition::default(),
            rect: Rect::default(),
            button,
            button_state,
        }
    }

    fn double_click(button: MouseButton) -> TrayIconEvent {
        TrayIconEvent::DoubleClick {
            id: TrayIconId::new("main"),
            position: PhysicalPosition::default(),
            rect: Rect::default(),
            button,
        }
    }

    #[test]
    fn left_click_and_double_click_open_the_window() {
        assert!(opens_main_window(&click(
            MouseButton::Left,
            MouseButtonState::Up
        )));
        assert!(opens_main_window(&double_click(MouseButton::Left)));
    }

    #[test]
    fn pressing_the_left_button_waits_for_the_release() {
        assert!(!opens_main_window(&click(
            MouseButton::Left,
            MouseButtonState::Down
        )));
    }

    #[test]
    fn other_buttons_leave_the_window_alone() {
        for button in [MouseButton::Right, MouseButton::Middle] {
            assert!(!opens_main_window(&click(button, MouseButtonState::Up)));
            assert!(!opens_main_window(&double_click(button)));
        }
    }

    #[test]
    fn hovering_the_tray_icon_does_not_open_the_window() {
        for event in [
            TrayIconEvent::Enter {
                id: TrayIconId::new("main"),
                position: PhysicalPosition::default(),
                rect: Rect::default(),
            },
            TrayIconEvent::Move {
                id: TrayIconId::new("main"),
                position: PhysicalPosition::default(),
                rect: Rect::default(),
            },
            TrayIconEvent::Leave {
                id: TrayIconId::new("main"),
                position: PhysicalPosition::default(),
                rect: Rect::default(),
            },
        ] {
            assert!(!opens_main_window(&event));
        }
    }
}
