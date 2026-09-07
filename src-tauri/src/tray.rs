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
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::activation::{opens_main_window, reveal_main_window};
use crate::audio;

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
    let mut tray = TrayIconBuilder::new()
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
    }

    impl<A: TrayActions> ksni::Tray for UtterformTray<A> {
        fn id(&self) -> String {
            "utterform".into()
        }

        fn title(&self) -> String {
            "Utterform".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            self.icon.clone()
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
    ) -> Result<ksni::blocking::Handle<UtterformTray<A>>, ksni::Error> {
        UtterformTray { actions, icon }.spawn()
    }

    pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), ksni::Error> {
        let icon = app
            .default_window_icon()
            .map(|image| {
                vec![Icon {
                    width: image.width() as i32,
                    height: image.height() as i32,
                    data: rgba_to_argb32(image.rgba()),
                }]
            })
            .unwrap_or_default();
        let handle = spawn(AppActions(app.clone()), icon)?;
        // The tray service re-registers itself when the panel restarts, but it
        // stops as soon as this handle is dropped.
        app.manage(handle);
        Ok(())
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
