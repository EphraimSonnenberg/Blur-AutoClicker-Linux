//! Chooses how simulated input is delivered and exposes the three things the
//! engine needs: buttons, keys and absolute cursor moves.
//!
//! * On a Wayland session uinput is preferred, because XTEST only reaches
//!   XWayland windows.
//! * On a plain X11 session XTEST is preferred: it needs no extra
//!   permissions and positions the cursor exactly.
//!
//! `BLUR_INPUT_BACKEND=x11|uinput` overrides the choice.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use super::win32::*;
use super::{keymap, uinput, x11};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    X11,
    Uinput,
    None,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

pub fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind == "wayland")
}

fn choose() -> Backend {
    let requested = std::env::var("BLUR_INPUT_BACKEND")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let backend = match requested.as_str() {
        "x11" if x11::xtest_available() => Backend::X11,
        "uinput" if uinput::available() => Backend::Uinput,
        _ => {
            if is_wayland_session() && uinput::available() {
                Backend::Uinput
            } else if x11::xtest_available() {
                Backend::X11
            } else if uinput::available() {
                Backend::Uinput
            } else {
                Backend::None
            }
        }
    };
    match backend {
        Backend::Uinput => log::info!("[input] Simulated input goes through uinput"),
        Backend::X11 if is_wayland_session() => log::warn!(
            "[input] Simulated input goes through XTEST on a Wayland session: only XWayland windows will receive it. Grant access to /dev/uinput to reach every window."
        ),
        Backend::X11 => log::info!("[input] Simulated input goes through XTEST"),
        Backend::None => log::error!(
            "[input] No way to simulate input: no X server with XTEST and /dev/uinput is not writable"
        ),
    }
    backend
}

pub fn backend() -> Backend {
    *BACKEND.get_or_init(choose)
}

/// Picks the backend and creates the virtual devices ahead of the first run.
pub fn init() {
    backend();
}

/// Short human readable description for the UI and diagnostics.
pub fn describe() -> String {
    match backend() {
        Backend::Uinput => String::from("uinput"),
        Backend::X11 if is_wayland_session() => String::from("XTEST (XWayland windows only)"),
        Backend::X11 => String::from("XTEST"),
        Backend::None => String::from("unavailable"),
    }
}

// Telling our own input apart from the user's.
//
// * Polling X sees XTEST events in the same state it reports for real keys,
//   so the monitor asks `recently_injected` whether a change is ours.
// * evdev never sees our virtual devices directly, but an input remapper
//   (keyd, input-remapper, ...) that grabs every keyboard and mouse will
//   re-emit what we send from its own virtual device. Every uinput event is
//   therefore counted here, and the monitor calls `consume_echo` for events
//   that arrive from a virtual device.
static INJECT_CLOCK: OnceLock<Instant> = OnceLock::new();
static LAST_INJECTED_MS: OnceLock<[AtomicU64; 256]> = OnceLock::new();
static PENDING_ECHOES: OnceLock<[[AtomicU32; 2]; 256]> = OnceLock::new();
const INJECTED_ECHO_MS: u64 = 60;
/// How long a sent event may take to come back through a remapper.
const ECHO_WINDOW_MS: u64 = 150;

fn clock_ms() -> u64 {
    INJECT_CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

fn last_injected() -> &'static [AtomicU64; 256] {
    LAST_INJECTED_MS.get_or_init(|| std::array::from_fn(|_| AtomicU64::new(0)))
}

fn pending_echoes() -> &'static [[AtomicU32; 2]; 256] {
    PENDING_ECHOES.get_or_init(|| std::array::from_fn(|_| [AtomicU32::new(0), AtomicU32::new(0)]))
}

fn note(vk: u16, down: bool, expect_echo: bool) {
    let Some(slot) = last_injected().get(vk as usize) else {
        return;
    };
    let now = clock_ms();
    let previous = slot.swap(now, Ordering::Relaxed);
    if !expect_echo {
        return;
    }
    let pending = &pending_echoes()[vk as usize];
    if now.saturating_sub(previous) > ECHO_WINDOW_MS {
        // Nothing echoed the last burst; do not let the counts pile up.
        pending[0].store(0, Ordering::Relaxed);
        pending[1].store(0, Ordering::Relaxed);
    }
    pending[usize::from(down)].fetch_add(1, Ordering::Relaxed);
}

/// Whether an event seen on a virtual device is one of ours coming back.
/// Each sent event is matched at most once.
pub fn consume_echo(vk: u16, down: bool) -> bool {
    let Some(slot) = last_injected().get(vk as usize) else {
        return false;
    };
    let at = slot.load(Ordering::Relaxed);
    if at == 0 || clock_ms().saturating_sub(at) > ECHO_WINDOW_MS {
        return false;
    }
    let pending = &pending_echoes()[vk as usize][usize::from(down)];
    let mut count = pending.load(Ordering::Relaxed);
    while count > 0 {
        match pending.compare_exchange_weak(count, count - 1, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return true,
            Err(current) => count = current,
        }
    }
    false
}

pub fn recently_injected(vk: u16) -> bool {
    last_injected().get(vk as usize).is_some_and(|slot| {
        let at = slot.load(Ordering::Relaxed);
        at != 0 && clock_ms().saturating_sub(at) <= INJECTED_ECHO_MS
    })
}

fn x_button(vk: u16) -> Option<u8> {
    match vk {
        VK_LBUTTON => Some(1),
        VK_MBUTTON => Some(2),
        VK_RBUTTON => Some(3),
        VK_XBUTTON1 => Some(8),
        VK_XBUTTON2 => Some(9),
        _ => None,
    }
}

/// Presses or releases a virtual key, mouse buttons included.
pub fn send_vk(vk: u16, down: bool) -> bool {
    let Some(code) = keymap::vk_to_evdev(vk) else {
        return false;
    };
    let side_specific = match vk {
        VK_SHIFT => VK_LSHIFT,
        VK_CONTROL => VK_LCONTROL,
        VK_MENU => VK_LMENU,
        other => other,
    };
    let backend = backend();
    note(side_specific, down, backend == Backend::Uinput);
    match backend {
        Backend::Uinput => uinput::key(code, down),
        Backend::X11 => match x_button(vk) {
            Some(button) => x11::fake_button(button, down),
            None => x11::fake_key(code, down),
        },
        Backend::None => false,
    }
}

/// Moves the cursor to desktop coordinates.
pub fn move_to(x: i32, y: i32, bounds: Option<(i32, i32, i32, i32)>) -> bool {
    match backend() {
        Backend::X11 => x11::warp(x, y),
        Backend::Uinput => {
            let moved = bounds.is_some_and(|bounds| uinput::move_abs(x, y, bounds));
            // XWayland still honours a warp while one of its own windows is
            // under the cursor, which covers compositors that ignore the
            // absolute device.
            moved || x11::warp(x, y)
        }
        Backend::None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test uses its own virtual key so they can run in parallel.

    #[test]
    fn an_echo_is_matched_once_per_sent_event_and_direction() {
        let vk = 0xE8;
        note(vk, true, true);
        note(vk, false, true);
        assert!(consume_echo(vk, true));
        assert!(!consume_echo(vk, true), "only one press was sent");
        assert!(consume_echo(vk, false));
        assert!(!consume_echo(vk, false));
    }

    #[test]
    fn input_sent_through_xtest_expects_no_echo() {
        let vk = 0xE9;
        note(vk, true, false);
        assert!(!consume_echo(vk, true));
        assert!(recently_injected(vk));
    }

    #[test]
    fn keys_that_were_never_sent_are_never_echoes() {
        assert!(!consume_echo(0xEA, true));
        assert!(!recently_injected(0xEA));
    }
}
