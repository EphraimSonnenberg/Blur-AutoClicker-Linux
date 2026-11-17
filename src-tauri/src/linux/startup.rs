//! Linux-specific startup work: bring up the input backends early and tell
//! the user, through the app's normal warning line, when permissions are
//! missing.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use super::inject::{self, Backend};
use super::monitor::{self, Source};
use crate::engine::mouse::{set_cached_monitor_rects, VirtualScreenRect};
use crate::engine::worker::emit_status;
use crate::error::poisoned_inner;
use crate::ClickerState;

static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

const INPUT_GROUP_HINT: &str =
    "Add your user to the 'input' group (sudo usermod -aG input $USER) and log in again.";

/// What, if anything, the user should be told about input access.
fn permission_warning(backend: Backend, source: Source, wayland: bool) -> Option<String> {
    let problem = match (backend, source) {
        (Backend::None, _) => "Cannot simulate clicks or key presses.",
        (_, Source::None) => "Hotkeys are unavailable.",
        (Backend::X11, _) if wayland => "Clicks only reach XWayland windows.",
        (_, Source::X11) if wayland => "Hotkeys only work while an XWayland window has focus.",
        _ => return None,
    };
    Some(format!("{problem} {INPUT_GROUP_HINT}"))
}

fn cache_monitors(app: &AppHandle) {
    let Ok(monitors) = app.available_monitors() else {
        return;
    };
    let rects: Vec<VirtualScreenRect> = monitors
        .iter()
        .map(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            VirtualScreenRect::new(
                position.x,
                position.y,
                size.width as i32,
                size.height as i32,
            )
        })
        .collect();
    log::info!("[Init] {} monitor(s) reported by the toolkit", rects.len());
    set_cached_monitor_rects(rects);
}

/// Runs before GTK starts.
///
/// The overlay, the click indicator and "always on top" all need to place
/// windows at exact desktop coordinates, which Wayland does not let a client
/// do. On a Wayland session with XWayland the app therefore runs its windows
/// through XWayland (input still goes through uinput / evdev and reaches
/// every window). Setting `GDK_BACKEND` yourself overrides this.
pub fn prepare_environment() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let xwayland = std::env::var_os("DISPLAY").is_some();
    if wayland && xwayland && std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

/// How much the web view enlarges the page because of the desktop's font DPI
/// (`Xft.dpi`), relative to the 96 DPI a CSS pixel assumes.
///
/// On X11 a fractional display scale is expressed only through that DPI: the
/// window scale stays an integer, so the page is laid out larger than the
/// window it lives in. The frontend already compensates for exactly this kind
/// of platform-applied scale (the Windows text scale factor), so it is
/// reported through the same command.
pub fn font_dpi_scale() -> f64 {
    let Some(app) = APP.get() else {
        return 1.0;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let queued = app.run_on_main_thread(move || {
        use gtk::prelude::GtkSettingsExt;
        let dpi = gtk::Settings::default().map(|settings| settings.gtk_xft_dpi());
        let _ = tx.send(dpi);
    });
    if queued.is_err() {
        return 1.0;
    }
    let dpi = rx.recv_timeout(Duration::from_millis(500)).ok().flatten();
    scale_from_xft_dpi(dpi)
}

/// `gtk-xft-dpi` is in 1/1024 of a dot per inch, and -1 when unset.
fn scale_from_xft_dpi(dpi: Option<i32>) -> f64 {
    match dpi {
        Some(value) if value > 0 => {
            let scale = value as f64 / 1024.0 / 96.0;
            if (0.5..=4.0).contains(&scale) {
                (scale * 100.0).round() / 100.0
            } else {
                1.0
            }
        }
        _ => 1.0,
    }
}

/// Gives a window its GDK surface without showing it.
pub fn realize_window(window: &tauri::WebviewWindow) {
    use gtk::prelude::WidgetExt;
    let target = window.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let queued = window.run_on_main_thread(move || {
        if let Ok(gtk_window) = target.gtk_window() {
            gtk_window.realize();
        }
        let _ = done_tx.send(());
    });
    // Runs inline when already on the main thread; otherwise wait for it so
    // the caller's next request finds the surface in place.
    if queued.is_ok() {
        let _ = done_rx.recv_timeout(Duration::from_millis(500));
    }
}

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    cache_monitors(app);

    let app = app.clone();
    std::thread::spawn(move || {
        // Creating the virtual devices now gives the compositor time to adopt
        // them before the first click is sent.
        inject::init();
        monitor::ensure_started();

        let mut source = monitor::source();
        for _ in 0..20 {
            if source != Source::Starting {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            source = monitor::source();
        }

        log::info!(
            "[Init] Input: sending through {}, hotkeys through {}",
            inject::describe(),
            monitor::describe()
        );

        let Some(warning) =
            permission_warning(inject::backend(), source, inject::is_wayland_session())
        else {
            return;
        };
        log::warn!("[Init] {warning}");
        let state = app.state::<ClickerState>();
        {
            let mut current = state.warning.lock().unwrap_or_else(poisoned_inner);
            if current.is_none() {
                *current = Some(warning);
            }
        }
        emit_status(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_dpi_becomes_a_page_scale() {
        assert_eq!(scale_from_xft_dpi(Some(96 * 1024)), 1.0);
        assert_eq!(scale_from_xft_dpi(Some(168 * 1024)), 1.75);
        assert_eq!(scale_from_xft_dpi(Some(-1)), 1.0);
        assert_eq!(scale_from_xft_dpi(None), 1.0);
        assert_eq!(scale_from_xft_dpi(Some(96 * 1024 * 50)), 1.0);
    }

    #[test]
    fn full_access_needs_no_warning() {
        assert_eq!(
            permission_warning(Backend::Uinput, Source::Evdev, true),
            None
        );
        assert_eq!(permission_warning(Backend::X11, Source::Evdev, false), None);
        assert_eq!(permission_warning(Backend::X11, Source::X11, false), None);
    }

    #[test]
    fn missing_injection_is_the_most_important_problem() {
        let warning = permission_warning(Backend::None, Source::None, true).unwrap();
        assert!(warning.starts_with("Cannot simulate"));
        assert!(warning.contains("'input' group"));
    }

    #[test]
    fn wayland_without_uinput_or_evdev_is_called_out() {
        assert!(permission_warning(Backend::X11, Source::Evdev, true)
            .unwrap()
            .starts_with("Clicks only reach XWayland"));
        assert!(permission_warning(Backend::Uinput, Source::X11, true)
            .unwrap()
            .starts_with("Hotkeys only work"));
        assert!(permission_warning(Backend::Uinput, Source::None, false)
            .unwrap()
            .starts_with("Hotkeys are unavailable"));
    }
}
