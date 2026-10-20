//! One tray icon, built the way each platform can actually deliver a click.
//!
//! Windows and macOS use Tauri's native tray. Linux gets its own
//! StatusNotifierItem: Tauri's tray goes through libayatana-appindicator, whose
//! D-Bus interface offers `SecondaryActivate` but no `Activate`, so a left click
//! can never reach the application. Serving the item ourselves — the same thing
//! Chromium-based apps do on Linux — makes a left click on the tray icon open
//! Utterform.

use std::{sync::Mutex, time::Duration};

use tauri::{
    AppHandle, Manager, Runtime,
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::activation::{opens_main_window, reveal_main_window};
use crate::{audio, diagnostics};

/// The id Tauri's tray is registered under, so it can be found again when the
/// recording state changes.
const NATIVE_TRAY: &str = "utterform";

/// Visual feedback is independent of sound. All state and native icon writes
/// are serialized on the event loop, including timeout callbacks.
const SUCCESS_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    #[default]
    Idle,
    Recording,
    Success,
}

impl Status {
    fn tooltip(self) -> &'static str {
        match self {
            Self::Idle => "Utterform",
            Self::Recording => "Utterform — recording",
            Self::Success => "Utterform — text delivered",
        }
    }
}

#[derive(Default)]
struct Indicator {
    status: Status,
    generation: u64,
    recording: Option<u64>,
    expiry: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Indicator {
    fn update(&mut self, status: Status, recording: Option<u64>) -> Option<u64> {
        match (status, recording) {
            (Status::Recording, Some(id)) => {
                if self.recording.is_some_and(|current| id <= current) {
                    return None;
                }
                self.recording = Some(id);
            }
            (Status::Idle, Some(id)) => {
                if self.recording != Some(id) || self.status != Status::Recording {
                    return None;
                }
            }
            (Status::Success, Some(id)) => {
                if self.recording != Some(id) || self.status == Status::Success {
                    return None;
                }
            }
            (Status::Idle, None) => self.recording = None,
            _ => return None,
        }
        Some(self.set(status))
    }

    fn set(&mut self, status: Status) -> u64 {
        if let Some(expiry) = self.expiry.take() {
            expiry.abort();
        }
        self.generation = self.generation.wrapping_add(1);
        self.status = status;
        self.generation
    }

    fn expire(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.status != Status::Success {
            return false;
        }
        self.set(Status::Idle);
        true
    }
}

#[derive(Default)]
pub struct TrayState(Mutex<Indicator>);

pub fn set_recording<R: Runtime>(app: &AppHandle<R>, recording: bool, id: u64) {
    set_status(
        app,
        if recording {
            Status::Recording
        } else {
            Status::Idle
        },
        Some(id),
    );
}

/// Called only after successful processing and requested delivery, never on
/// microphone stop, cancel, partial delivery or a failed transformation.
pub fn set_delivered<R: Runtime>(app: &AppHandle<R>, id: u64) {
    set_status(app, Status::Success, Some(id));
}

pub fn clear<R: Runtime>(app: &AppHandle<R>) {
    set_status(app, Status::Idle, None);
}

fn set_status<R: Runtime>(app: &AppHandle<R>, status: Status, recording: Option<u64>) {
    let queued = app.clone();
    if app
        .run_on_main_thread(move || {
            let state = queued.state::<TrayState>();
            let Ok(mut indicator) = state.0.lock() else {
                diagnostics::warning!("tray.update_failed", step = "state");
                return;
            };
            let Some(generation) = indicator.update(status, recording) else {
                return;
            };
            show_status(&queued, status);
            if status == Status::Success {
                let timer_app = queued.clone();
                indicator.expiry = Some(tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(SUCCESS_DURATION).await;
                    let expired = timer_app.clone();
                    if timer_app
                        .run_on_main_thread(move || {
                            let state = expired.state::<TrayState>();
                            let Ok(mut indicator) = state.0.lock() else {
                                diagnostics::warning!("tray.update_failed", step = "expiry_state");
                                return;
                            };
                            // Checking AND writing on the event loop means an old
                            // timer can never erase a newer recording/success icon.
                            if indicator.expire(generation) {
                                show_status(&expired, Status::Idle);
                            }
                        })
                        .is_err()
                    {
                        diagnostics::warning!("tray.update_failed", step = "queue_expiry");
                    }
                }));
            }
        })
        .is_err()
    {
        diagnostics::warning!("tray.update_failed", step = "queue_status");
    }
}

fn show_status<R: Runtime>(app: &AppHandle<R>, status: Status) {
    #[cfg(target_os = "linux")]
    if status_notifier_item::set_status(app, status) {
        return;
    }
    let Some(tray) = app.tray_by_id(NATIVE_TRAY) else {
        diagnostics::warning!("tray.update_failed", step = "find_tray");
        return;
    };
    let Some(icon) = app.default_window_icon() else {
        diagnostics::warning!("tray.update_failed", step = "icon");
        return;
    };
    let shown = if status == Status::Idle {
        icon.clone()
    } else {
        Image::new_owned(
            status_badge(icon.rgba(), icon.width(), icon.height(), status),
            icon.width(),
            icon.height(),
        )
    };
    if tray.set_icon(Some(shown)).is_err() {
        diagnostics::warning!("tray.update_failed", step = "set_icon");
    }
    if tray.set_tooltip(Some(status.tooltip())).is_err() {
        diagnostics::warning!("tray.update_failed", step = "set_tooltip");
    }
}

/// The app icon with a small red/green dot in its lower right corner. Drawn rather
/// than shipped so it cannot fall out of step with the icon it marks.
///
/// Deliberately modest: a dot a little over a third of the icon wide, with a
/// soft edge and no ring around it. The corner it sits in is the icon's dark
/// surface, so it reads on its own, and at the 16 pixels a Windows tray
/// gives it the dot is still six pixels across — a recording light, not a
/// badge. 0.4.4 drew it twice this size with a white ring, and it looked
/// like an alert.
fn status_badge(rgba: &[u8], width: u32, height: u32, status: Status) -> Vec<u8> {
    let mut pixels = rgba.to_vec();
    let color = match status {
        Status::Idle => return pixels,
        Status::Recording => [229.0, 72.0, 77.0],
        Status::Success => [46.0, 204.0, 113.0],
    };
    let side = width.min(height) as f32;
    let radius = side * 0.19;
    let margin = side * 0.06;
    let center = (
        width as f32 - radius - margin,
        height as f32 - radius - margin,
    );
    for y in 0..height {
        for x in 0..width {
            let distance =
                ((x as f32 + 0.5 - center.0).powi(2) + (y as f32 + 0.5 - center.1).powi(2)).sqrt();
            // One pixel of anti-aliasing at the edge.
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }
            let index = ((y * width + x) * 4) as usize;
            let Some(pixel) = pixels.get_mut(index..index + 4) else {
                continue;
            };
            let blend =
                |under: u8, over: f32| (under as f32 * (1.0 - coverage) + over * coverage) as u8;
            pixel[0] = blend(pixel[0], color[0]);
            pixel[1] = blend(pixel[1], color[1]);
            pixel[2] = blend(pixel[2], color[2]);
            pixel[3] = blend(pixel[3], 255.0);
        }
    }
    pixels
}

/// Quitting stops capture first so a recording never outlives the app.
fn quit<R: Runtime>(app: &AppHandle<R>) {
    diagnostics::info!("tray.quit_requested");
    let state = app.state::<audio::AudioCaptureState>();
    if let Err(failure) = audio::cancel_recording(&state) {
        // Quitting goes on: the process ending releases the microphone.
        diagnostics::warning!("tray.quit_cancel_failed", class = failure.class());
    }
    app.exit(0);
}

/// Install the tray icon, preferring the implementation that gives this
/// platform a working click.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    match status_notifier_item::install(app) {
        Ok(()) => {
            diagnostics::info!("tray.installed", kind = "status_notifier_item");
            return Ok(());
        }
        Err(error) => {
            // No StatusNotifierItem host answered. Fall back rather than leave
            // the user without a tray: AppIndicator can still fall back to a
            // GtkStatusIcon, it just cannot report clicks.
            diagnostics::warning!(
                "tray.fallback",
                from = "status_notifier_item",
                to = "appindicator",
                detail = error
            );
        }
    }
    native(app)?;
    diagnostics::info!("tray.installed", kind = "native");
    Ok(())
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
        "show" => reveal_main_window(app, "tray_menu"),
        "quit" => quit(app),
        _ => {}
    })
    .on_tray_icon_event(|tray, event| {
        if opens_main_window(&event) {
            reveal_main_window(tray.app_handle(), "tray_click");
        }
    })
    .build(app)?;
    Ok(())
}

#[cfg(target_os = "linux")]
mod status_notifier_item {
    use ksni::{Icon, MenuItem, blocking::TrayMethods, menu::StandardItem};
    use tauri::{AppHandle, Manager, Runtime};

    use super::Status;
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
            reveal_main_window(&self.0, "tray");
        }

        fn quit(&self) {
            super::quit(&self.0);
        }
    }

    pub struct UtterformTray<A: TrayActions> {
        actions: A,
        icon: Vec<Icon>,
        recording_icon: Vec<Icon>,
        success_icon: Vec<Icon>,
        status: Status,
    }

    impl<A: TrayActions> ksni::Tray for UtterformTray<A> {
        fn id(&self) -> String {
            "utterform".into()
        }

        fn title(&self) -> String {
            self.status.tooltip().into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            match self.status {
                Status::Idle => self.icon.clone(),
                Status::Recording => self.recording_icon.clone(),
                Status::Success => self.success_icon.clone(),
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
        success_icon: Vec<Icon>,
    ) -> Result<ksni::blocking::Handle<UtterformTray<A>>, ksni::Error> {
        UtterformTray {
            actions,
            icon,
            recording_icon,
            success_icon,
            status: Status::Idle,
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
        let (icon, recording_icon, success_icon) = app
            .default_window_icon()
            .map(|image| {
                let (width, height) = (image.width(), image.height());
                (
                    pixmap(width, height, image.rgba()),
                    pixmap(
                        width,
                        height,
                        &super::status_badge(image.rgba(), width, height, Status::Recording),
                    ),
                    pixmap(
                        width,
                        height,
                        &super::status_badge(image.rgba(), width, height, Status::Success),
                    ),
                )
            })
            .unwrap_or_default();
        let handle = spawn(AppActions(app.clone()), icon, recording_icon, success_icon)?;
        // The tray service re-registers itself when the panel restarts, but it
        // stops as soon as this handle is dropped.
        app.manage(handle);
        Ok(())
    }

    /// Swap the icon; `update` has ksni tell the host it changed. False when
    /// this session fell back to the native tray instead.
    pub(super) fn set_status<R: Runtime>(app: &AppHandle<R>, status: Status) -> bool {
        let Some(handle) = app.try_state::<ksni::blocking::Handle<UtterformTray<AppActions<R>>>>()
        else {
            return false;
        };
        if handle.update(|tray| tray.status = status).is_none() {
            // The tray service has stopped; the native tray does not exist here.
            crate::diagnostics::warning!("tray.update_failed", step = "status_notifier_item",);
        }
        true
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
                pixmap(1, 1, &[0, 0, 0, 255]),
                pixmap(1, 1, &[229, 72, 77, 255]),
                pixmap(1, 1, &[46, 204, 113, 255]),
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
            // Exercise the real D-Bus properties a desktop panel consumes,
            // not only the in-memory image selector.
            for (status, argb) in [
                (Status::Recording, vec![255, 229, 72, 77]),
                (Status::Success, vec![255, 46, 204, 113]),
                (Status::Idle, vec![255, 0, 0, 0]),
            ] {
                _tray.update(|tray| tray.status = status).unwrap();
                let (icons, title) = runtime.block_on(async {
                    let connection = zbus::Connection::session().await.unwrap();
                    let proxy = zbus::Proxy::new(
                        &connection,
                        item.as_str(),
                        "/StatusNotifierItem",
                        "org.kde.StatusNotifierItem",
                    )
                    .await
                    .unwrap();
                    let icons: Vec<(i32, i32, Vec<u8>)> =
                        proxy.get_property("IconPixmap").await.unwrap();
                    let title: String = proxy.get_property("Title").await.unwrap();
                    (icons, title)
                });
                assert_eq!(icons, vec![(1, 1, argb)]);
                assert_eq!(title, status.tooltip());
            }
        }
    }
}

#[cfg(test)]
mod badge_tests {
    use super::{Indicator, SUCCESS_DURATION, Status, status_badge};

    fn recording_badge(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
        status_badge(rgba, width, height, Status::Recording)
    }

    #[test]
    fn success_expires_after_five_seconds_and_restores_idle() {
        assert_eq!(SUCCESS_DURATION.as_secs(), 5);
        let mut indicator = Indicator::default();
        let generation = indicator.set(Status::Success);
        assert!(indicator.expire(generation));
        assert_eq!(indicator.status, Status::Idle);
        assert!(!indicator.expire(generation));
    }

    #[test]
    fn stale_timeout_cannot_clear_recording_cancel_or_new_success() {
        for next in [Status::Recording, Status::Idle, Status::Success] {
            let mut indicator = Indicator::default();
            let stale = indicator.set(Status::Success);
            let current = indicator.set(next);
            assert!(!indicator.expire(stale));
            assert_eq!(indicator.status, next);
            assert_eq!(indicator.expire(current), next == Status::Success);
        }
    }

    #[test]
    fn late_capture_stop_cannot_erase_success_or_a_new_recording() {
        let mut indicator = Indicator::default();
        indicator.update(Status::Recording, Some(1)).unwrap();
        indicator.update(Status::Idle, Some(1)).unwrap();
        let green = indicator.update(Status::Success, Some(1)).unwrap();
        assert!(indicator.update(Status::Idle, Some(1)).is_none());
        assert_eq!(indicator.status, Status::Success);
        indicator.update(Status::Recording, Some(2)).unwrap();
        assert!(!indicator.expire(green));
        assert!(indicator.update(Status::Idle, Some(1)).is_none());
        assert!(indicator.update(Status::Success, Some(1)).is_none());
        assert_eq!(indicator.status, Status::Recording);
        indicator.update(Status::Idle, None).unwrap();
        assert!(indicator.update(Status::Success, Some(2)).is_none());
        assert_eq!(indicator.status, Status::Idle);
    }

    #[test]
    fn success_is_green_at_native_tray_sizes_and_idle_is_unchanged() {
        for side in [16, 32] {
            let plain = vec![0; side * side * 4];
            let green = status_badge(&plain, side as u32, side as u32, Status::Success);
            assert_eq!(
                status_badge(&plain, side as u32, side as u32, Status::Idle),
                plain
            );
            assert_eq!(pixel(&green, side as u32, 0, 0), [0; 4]);
            assert!(
                green
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| **p == [46, 204, 113, 255])
                    .count()
                    >= 8
            );
            assert_ne!(green, recording_badge(&plain, side as u32, side as u32));
        }
    }

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        rgba[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn the_badge_is_a_small_red_dot_in_the_lower_right_and_leaves_the_rest_alone() {
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
        // Icon untouched away from the dot — including the centre, where the
        // microphone is, and the corner the 0.4.4 disc used to reach into.
        assert_eq!(pixel(&badged, width, 0, 0), grey);
        assert_eq!(pixel(&badged, width, 31, 0), grey);
        assert_eq!(pixel(&badged, width, 0, 31), grey);
        assert_eq!(pixel(&badged, width, 16, 16), grey);
        assert_eq!(pixel(&badged, width, 18, 18), grey);
        // Solid red at the dot's centre, and opaque even on a transparent icon.
        let centre = pixel(&badged, width, 24, 24);
        assert_eq!(centre, [229, 72, 77, 255]);
        let clear = vec![0; (width * height * 4) as usize];
        assert_eq!(
            pixel(&recording_badge(&clear, width, height), width, 24, 24)[3],
            255
        );
    }

    #[test]
    fn the_dot_is_modest_but_still_reads_at_windows_tray_size() {
        // 16 pixels: about six across, so a handful of solid pixels and a
        // soft edge — and well under a quarter of the icon.
        let plain = vec![0; 16 * 16 * 4];
        let badged = recording_badge(&plain, 16, 16);
        let solid = (0..16 * 16)
            .filter(|i| badged[i * 4] == 229 && badged[i * 4 + 3] == 255)
            .count();
        let touched = (0..16 * 16).filter(|i| badged[i * 4 + 3] > 0).count();
        assert!(solid >= 8, "{solid} solid red pixels");
        assert!(touched < 64, "{touched} pixels touched");
    }

    #[test]
    fn a_short_buffer_does_not_panic() {
        // Wrong dimensions must degrade to a partial badge, never a crash in
        // the tray path.
        let badged = recording_badge(&[0; 8], 32, 32);
        assert_eq!(badged.len(), 8);
    }
}
