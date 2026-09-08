//! One tray icon, built the way each platform can actually deliver a click.
//!
//! Windows and macOS use Tauri's native tray. Linux gets its own
//! StatusNotifierItem: Tauri's tray goes through libayatana-appindicator, whose
//! D-Bus interface offers `SecondaryActivate` but no `Activate`, so a left click
//! can never reach the application. Serving the item ourselves — the same thing
//! Chromium-based apps do on Linux — makes a left click on the tray icon open
//! Utterform.

use tauri::{
    AppHandle, Manager, Runtime,
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::activation::{opens_main_window, reveal_main_window};
use crate::audio;

/// The id Tauri's tray is registered under, so it can be found again when the
/// recording state changes.
const NATIVE_TRAY: &str = "utterform";

/// Show in the tray whether a recording is running. The sounds confirm it
/// where the window is hidden, but a speaker that is asleep or muted can miss
/// one, and an icon on the panel cannot.
pub fn set_recording<R: Runtime>(app: &AppHandle<R>, recording: bool) {
    #[cfg(target_os = "linux")]
    if status_notifier_item::set_recording(app, recording) {
        return;
    }
    let Some(tray) = app.tray_by_id(NATIVE_TRAY) else {
        return;
    };
    let Some(icon) = app.default_window_icon() else {
        return;
    };
    let shown = if recording {
        Image::new_owned(
            recording_badge(icon.rgba(), icon.width(), icon.height()),
            icon.width(),
            icon.height(),
        )
    } else {
        icon.clone()
    };
    let _ = tray.set_icon(Some(shown));
    let _ = tray.set_tooltip(Some(if recording {
        "Utterform — recording"
    } else {
        "Utterform"
    }));
}

/// The app icon with a red disc over its lower right quarter. Drawn rather
/// than shipped so it cannot fall out of step with the icon it marks, and
/// large enough to read at the 16 pixels a Windows tray gives it.
fn recording_badge(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut pixels = rgba.to_vec();
    let side = width.min(height) as f32;
    let radius = side * 0.28;
    let ring = (side / 16.0).max(1.0);
    let center = (width as f32 - radius - ring, height as f32 - radius - ring);
    for y in 0..height {
        for x in 0..width {
            let distance =
                ((x as f32 + 0.5 - center.0).powi(2) + (y as f32 + 0.5 - center.1).powi(2)).sqrt();
            let coverage = (radius + ring - distance).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }
            // A white ring separates the disc from an icon of similar colour.
            let (r, g, b) = if distance > radius {
                (255.0, 255.0, 255.0)
            } else {
                (229.0, 72.0, 77.0)
            };
            let index = ((y * width + x) * 4) as usize;
            let Some(pixel) = pixels.get_mut(index..index + 4) else {
                continue;
            };
            let blend =
                |under: u8, over: f32| (under as f32 * (1.0 - coverage) + over * coverage) as u8;
            pixel[0] = blend(pixel[0], r);
            pixel[1] = blend(pixel[1], g);
            pixel[2] = blend(pixel[2], b);
            pixel[3] = blend(pixel[3], 255.0);
        }
    }
    pixels
}

/// Quitting stops capture first so a recording never outlives the app.
fn quit<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<audio::AudioCaptureState>();
    let _ = audio::cancel_recording(&state);
    app.exit(0);
}

/// Install the tray icon, preferring the implementation that gives this
/// platform a working click.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    match status_notifier_item::install(app) {
        Ok(()) => return Ok(()),
        Err(error) => {
            // No StatusNotifierItem host answered. Fall back rather than leave
            // the user without a tray: AppIndicator can still fall back to a
            // GtkStatusIcon, it just cannot report clicks.
            eprintln!(
                "Utterform: no StatusNotifierItem host ({error}); using the AppIndicator tray, where only the menu works"
            );
        }
    }
    native(app)
}

/// Tauri's own tray. Left click and double click open the window where the
/// platform reports them.
fn native<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Utterform", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit_item])?;
    let mut tray = TrayIconBuilder::with_id(NATIVE_TRAY)
        .menu(&menu)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "show" => reveal_main_window(app),
        "quit" => quit(app),
        _ => {}
    })
    .on_tray_icon_event(|tray, event| {
        if opens_main_window(&event) {
            reveal_main_window(tray.app_handle());
        }
    })
    .build(app)?;
    Ok(())
}

#[cfg(target_os = "linux")]
mod status_notifier_item {
    use ksni::{Icon, MenuItem, blocking::TrayMethods, menu::StandardItem};
    use tauri::{AppHandle, Manager, Runtime};

    use crate::activation::reveal_main_window;

    /// What the tray can ask Utterform to do. Keeping this behind a trait lets
    /// the D-Bus behaviour be tested without a running desktop application.
    pub trait TrayActions: Send + 'static {
        fn show(&self);
        fn quit(&self);
    }

    struct AppActions<R: Runtime>(AppHandle<R>);

    impl<R: Runtime> TrayActions for AppActions<R> {
        fn show(&self) {
            reveal_main_window(&self.0);
        }

        fn quit(&self) {
            super::quit(&self.0);
        }
    }

    pub struct UtterformTray<A: TrayActions> {
        actions: A,
        icon: Vec<Icon>,
        recording_icon: Vec<Icon>,
        recording: bool,
    }

    impl<A: TrayActions> ksni::Tray for UtterformTray<A> {
        fn id(&self) -> String {
            "utterform".into()
        }

        fn title(&self) -> String {
            "Utterform".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            if self.recording {
                self.recording_icon.clone()
            } else {
                self.icon.clone()
            }
        }

        /// A left click on the tray icon. This is the whole reason Utterform
        /// serves the item itself instead of using AppIndicator.
        fn activate(&mut self, _x: i32, _y: i32) {
            self.actions.show();
        }

        /// A middle click means the same thing; there is nothing else the tray
        /// icon could usefully do.
        fn secondary_activate(&mut self, _x: i32, _y: i32) {
            self.actions.show();
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                StandardItem {
                    label: "Show Utterform".into(),
                    activate: Box::new(|tray: &mut Self| tray.actions.show()),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|tray: &mut Self| tray.actions.quit()),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    /// Convert Tauri's RGBA icon bytes to the ARGB32 network byte order that the
    /// StatusNotifierItem specification asks for.
    fn rgba_to_argb32(rgba: &[u8]) -> Vec<u8> {
        let mut argb = Vec::with_capacity(rgba.len());
        for pixel in rgba.as_chunks::<4>().0 {
            argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
        }
        argb
    }

    pub fn spawn<A: TrayActions>(
        actions: A,
        icon: Vec<Icon>,
        recording_icon: Vec<Icon>,
    ) -> Result<ksni::blocking::Handle<UtterformTray<A>>, ksni::Error> {
        UtterformTray {
            actions,
            icon,
            recording_icon,
            recording: false,
        }
        .spawn()
    }

    fn pixmap(width: u32, height: u32, rgba: &[u8]) -> Vec<Icon> {
        vec![Icon {
            width: width as i32,
            height: height as i32,
            data: rgba_to_argb32(rgba),
        }]
    }

    pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), ksni::Error> {
        let (icon, recording_icon) = app
            .default_window_icon()
            .map(|image| {
                let (width, height) = (image.width(), image.height());
                (
                    pixmap(width, height, image.rgba()),
                    pixmap(
                        width,
                        height,
                        &super::recording_badge(image.rgba(), width, height),
                    ),
                )
            })
            .unwrap_or_default();
        let handle = spawn(AppActions(app.clone()), icon, recording_icon)?;
        // The tray service re-registers itself when the panel restarts, but it
        // stops as soon as this handle is dropped.
        app.manage(handle);
        Ok(())
    }

    /// Swap the icon; `update` has ksni tell the host it changed. False when
    /// this session fell back to the native tray instead.
    pub fn set_recording<R: Runtime>(app: &AppHandle<R>, recording: bool) -> bool {
        let Some(handle) = app.try_state::<ksni::blocking::Handle<UtterformTray<AppActions<R>>>>()
        else {
            return false;
        };
        handle.update(|tray| tray.recording = recording).is_some()
    }

    #[cfg(test)]
    mod tests {
        use std::sync::Arc;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc::{Sender, channel};
        use std::time::Duration;

        use super::*;

        struct CountingActions {
            shown: Arc<AtomicUsize>,
        }

        impl TrayActions for CountingActions {
            fn show(&self) {
                self.shown.fetch_add(1, Ordering::SeqCst);
            }

            fn quit(&self) {}
        }

        /// The smallest watcher a StatusNotifierItem needs to find: it only has
        /// to accept the registration and report which bus name registered.
        struct Watcher {
            registered: Mutex<Sender<String>>,
        }

        #[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
        impl Watcher {
            async fn register_status_notifier_item(&self, service: String) {
                let _ = self.registered.lock().unwrap().send(service);
            }

            /// A real host has to answer this before the item will show itself.
            #[zbus(property)]
            fn is_status_notifier_host_registered(&self) -> bool {
                true
            }

            #[zbus(property)]
            fn protocol_version(&self) -> i32 {
                0
            }

            #[zbus(property)]
            fn registered_status_notifier_items(&self) -> Vec<String> {
                Vec::new()
            }
        }

        #[test]
        fn icons_are_converted_to_argb32_network_byte_order() {
            let opaque_red = [255, 0, 0, 255];
            let transparent_blue = [0, 0, 255, 0];
            let rgba: Vec<u8> = opaque_red.into_iter().chain(transparent_blue).collect();

            assert_eq!(
                rgba_to_argb32(&rgba),
                vec![255, 255, 0, 0, /* */ 0, 0, 0, 255]
            );
        }

        #[test]
        fn an_empty_icon_stays_empty() {
            assert!(rgba_to_argb32(&[]).is_empty());
        }

        /// The reason the Linux tray exists at all: a StatusNotifierItem host
        /// turns a left click into an `Activate` call, and Utterform must open.
        /// AppIndicator cannot do this because it never exposes `Activate`.
        ///
        /// Needs a session bus of its own: run the suite through
        /// `dbus-run-session -- cargo test`.
        #[test]
        fn a_left_click_reaches_the_application() {
            assert!(
                std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
                "no session bus; run the tests with `dbus-run-session -- cargo test`"
            );

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("test runtime should start");
            let (registrations, registered) = channel();
            let _watcher = runtime
                .block_on(async {
                    zbus::connection::Builder::session()?
                        .name("org.kde.StatusNotifierWatcher")?
                        .serve_at(
                            "/StatusNotifierWatcher",
                            Watcher {
                                registered: Mutex::new(registrations),
                            },
                        )?
                        .build()
                        .await
                })
                .expect("the fake watcher should own its name");

            let shown = Arc::new(AtomicUsize::new(0));
            // Spawned outside the runtime above: ksni's blocking API drives its
            // own runtime and must not be started from inside another one.
            let _tray = spawn(
                CountingActions {
                    shown: Arc::clone(&shown),
                },
                Vec::new(),
                Vec::new(),
            )
            .expect("the tray should register with the watcher");

            let item = registered
                .recv_timeout(Duration::from_secs(10))
                .expect("the tray should announce itself to the watcher");
            assert!(item.starts_with("org.kde.StatusNotifierItem-"));

            runtime
                .block_on(async {
                    zbus::Connection::session()
                        .await?
                        .call_method(
                            Some(item.as_str()),
                            "/StatusNotifierItem",
                            Some("org.kde.StatusNotifierItem"),
                            "Activate",
                            &(0i32, 0i32),
                        )
                        .await
                })
                .expect("Activate should be answered, not rejected as unknown");

            assert_eq!(
                shown.load(Ordering::SeqCst),
                1,
                "a left click should have opened Utterform exactly once"
            );
        }
    }
}

#[cfg(test)]
mod badge_tests {
    use super::recording_badge;

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        rgba[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn the_badge_is_a_red_disc_in_the_lower_right_and_leaves_the_rest_alone() {
        let (width, height) = (32, 32);
        let grey = [90, 90, 90, 255];
        let plain: Vec<u8> = grey
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect();
        let badged = recording_badge(&plain, width, height);
        assert_eq!(badged.len(), plain.len());
        // Icon untouched away from the badge.
        assert_eq!(pixel(&badged, width, 0, 0), grey);
        assert_eq!(pixel(&badged, width, 31, 0), grey);
        assert_eq!(pixel(&badged, width, 0, 31), grey);
        assert_eq!(pixel(&badged, width, 12, 12), grey);
        // Solid red at the disc's centre, and opaque even on a transparent icon.
        let centre = pixel(&badged, width, 22, 22);
        assert_eq!(centre, [229, 72, 77, 255]);
        let clear = vec![0; (width * height * 4) as usize];
        assert_eq!(
            pixel(&recording_badge(&clear, width, height), width, 22, 22)[3],
            255
        );
    }

    #[test]
    fn the_badge_reads_at_windows_tray_size() {
        // 16 pixels: the disc must still be several pixels across.
        let plain = vec![0; 16 * 16 * 4];
        let badged = recording_badge(&plain, 16, 16);
        let red = (0..16 * 16)
            .filter(|i| badged[i * 4] == 229 && badged[i * 4 + 3] == 255)
            .count();
        assert!(red >= 12, "{red} solid red pixels");
    }

    #[test]
    fn a_short_buffer_does_not_panic() {
        // Wrong dimensions must degrade to a partial badge, never a crash in
        // the tray path.
        let badged = recording_badge(&[0; 8], 32, 32);
        assert_eq!(badged.len(), 8);
    }
}
