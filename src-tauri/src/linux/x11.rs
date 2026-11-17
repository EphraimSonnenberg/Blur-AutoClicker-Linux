//! X11 / XWayland access: pointer and keyboard queries, XTEST injection and
//! the window properties the process list needs.
//!
//! Everything here degrades to `None` / `false` when there is no X server, so
//! callers can run on a pure Wayland session without special casing.

use std::sync::OnceLock;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ButtonIndex, ConnectionExt as XprotoExt, EventMask, GrabMode, ModMask, Window,
};
use x11rb::protocol::xtest::ConnectionExt as XTestExt;
use x11rb::rust_connection::RustConnection;

const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;

/// X keycodes are evdev codes shifted by 8 on every Linux X server.
const EVDEV_KEYCODE_OFFSET: u16 = 8;

/// Largest window icon we are willing to decode, per side.
const MAX_ICON_SIDE: u32 = 256;

struct Atoms {
    net_active_window: u32,
    net_client_list: u32,
    net_wm_pid: u32,
    net_wm_name: u32,
    net_wm_icon: u32,
    utf8_string: u32,
}

struct State {
    conn: RustConnection,
    root: Window,
    screen: usize,
    xtest: bool,
    atoms: Atoms,
}

static STATE: OnceLock<Option<State>> = OnceLock::new();

fn connect() -> Option<State> {
    std::env::var_os("DISPLAY")?;
    let (conn, screen) = match x11rb::connect(None) {
        Ok(pair) => pair,
        Err(e) => {
            log::warn!("[x11] Could not connect to the X server: {e}");
            return None;
        }
    };
    let root = conn.setup().roots.get(screen)?.root;
    let atom = |name: &[u8]| -> u32 {
        conn.intern_atom(false, name)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.atom)
            .unwrap_or(0)
    };
    let atoms = Atoms {
        net_active_window: atom(b"_NET_ACTIVE_WINDOW"),
        net_client_list: atom(b"_NET_CLIENT_LIST"),
        net_wm_pid: atom(b"_NET_WM_PID"),
        net_wm_name: atom(b"_NET_WM_NAME"),
        net_wm_icon: atom(b"_NET_WM_ICON"),
        utf8_string: atom(b"UTF8_STRING"),
    };
    let xtest = conn
        .xtest_get_version(2, 2)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some();
    Some(State {
        conn,
        root,
        screen,
        xtest,
        atoms,
    })
}

fn get() -> Option<&'static State> {
    STATE.get_or_init(connect).as_ref()
}

pub fn available() -> bool {
    get().is_some()
}

pub fn xtest_available() -> bool {
    get().is_some_and(|state| state.xtest)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    /// Core modifier and button mask (`KeyButMask`).
    pub mask: u16,
}

impl Pointer {
    pub const LOCK: u16 = 1 << 1;
    pub const BUTTON1: u16 = 1 << 8;
    pub const BUTTON2: u16 = 1 << 9;
    pub const BUTTON3: u16 = 1 << 10;
}

pub fn pointer() -> Option<Pointer> {
    let state = get()?;
    let reply = state.conn.query_pointer(state.root).ok()?.reply().ok()?;
    Some(Pointer {
        x: reply.root_x as i32,
        y: reply.root_y as i32,
        mask: u16::from(reply.mask),
    })
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    pointer().map(|pointer| (pointer.x, pointer.y))
}

pub fn caps_lock_on() -> Option<bool> {
    pointer().map(|pointer| pointer.mask & Pointer::LOCK != 0)
}

/// Bit vector of pressed X keycodes, as `XQueryKeymap` returns it.
pub fn keymap() -> Option<[u8; 32]> {
    let state = get()?;
    let reply = state.conn.query_keymap().ok()?.reply().ok()?;
    Some(reply.keys)
}

pub fn keymap_has(keys: &[u8; 32], evdev_code: u16) -> bool {
    let keycode = (evdev_code + EVDEV_KEYCODE_OFFSET) as usize;
    keycode < 256 && keys[keycode / 8] & (1 << (keycode % 8)) != 0
}

/// `(left, top, width, height)` of the whole X screen.
pub fn virtual_screen() -> Option<(i32, i32, i32, i32)> {
    let state = get()?;
    let screen = state.conn.setup().roots.get(state.screen)?;
    Some((
        0,
        0,
        screen.width_in_pixels as i32,
        screen.height_in_pixels as i32,
    ))
}

pub fn monitor_rects() -> Option<Vec<(i32, i32, i32, i32)>> {
    use x11rb::protocol::randr::ConnectionExt as RandrExt;
    let state = get()?;
    let reply = state
        .conn
        .randr_get_monitors(state.root, true)
        .ok()?
        .reply()
        .ok()?;
    let mut rects: Vec<_> = reply
        .monitors
        .iter()
        .filter(|monitor| monitor.width > 0 && monitor.height > 0)
        .map(|monitor| {
            (
                monitor.x as i32,
                monitor.y as i32,
                monitor.width as i32,
                monitor.height as i32,
            )
        })
        .collect();
    if rects.is_empty() {
        return None;
    }
    rects.sort_by_key(|rect| (rect.1, rect.0));
    Some(rects)
}

pub fn warp(x: i32, y: i32) -> bool {
    let Some(state) = get() else {
        return false;
    };
    let clamp = |value: i32| value.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    state
        .conn
        .warp_pointer(x11rb::NONE, state.root, 0, 0, 0, 0, clamp(x), clamp(y))
        .is_ok()
        && state.conn.flush().is_ok()
}

fn fake_input(kind: u8, detail: u8) -> bool {
    let Some(state) = get() else {
        return false;
    };
    if !state.xtest {
        return false;
    }
    state
        .conn
        .xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, state.root, 0, 0, 0)
        .is_ok()
        && state.conn.flush().is_ok()
}

/// `button` is an X button number (1 left, 2 middle, 3 right, 8 / 9 thumb).
pub fn fake_button(button: u8, down: bool) -> bool {
    fake_input(if down { BUTTON_PRESS } else { BUTTON_RELEASE }, button)
}

pub fn fake_key(evdev_code: u16, down: bool) -> bool {
    let keycode = evdev_code + EVDEV_KEYCODE_OFFSET;
    if keycode > 255 {
        return false;
    }
    fake_input(if down { KEY_PRESS } else { KEY_RELEASE }, keycode as u8)
}

fn property(window: Window, atom: u32, kind: u32, max_words: u32) -> Option<Vec<u8>> {
    if atom == 0 {
        return None;
    }
    let state = get()?;
    let reply = state
        .conn
        .get_property(false, window, atom, kind, 0, max_words)
        .ok()?
        .reply()
        .ok()?;
    (reply.value_len > 0).then_some(reply.value)
}

fn property_u32(window: Window, atom: u32, kind: AtomEnum, max_words: u32) -> Option<Vec<u32>> {
    let state = get()?;
    if atom == 0 {
        return None;
    }
    let reply = state
        .conn
        .get_property(false, window, atom, kind, 0, max_words)
        .ok()?
        .reply()
        .ok()?;
    let values: Vec<u32> = reply.value32()?.collect();
    (!values.is_empty()).then_some(values)
}

/// The focused top level window, when the window manager publishes one. Under
/// XWayland this is `None` whenever a native Wayland window has focus.
pub fn active_window() -> Option<Window> {
    let state = get()?;
    let window = *property_u32(
        state.root,
        state.atoms.net_active_window,
        AtomEnum::WINDOW,
        1,
    )?
    .first()?;
    (window != 0).then_some(window)
}

pub fn client_windows() -> Vec<Window> {
    let Some(state) = get() else {
        return Vec::new();
    };
    property_u32(
        state.root,
        state.atoms.net_client_list,
        AtomEnum::WINDOW,
        4096,
    )
    .unwrap_or_default()
}

pub fn window_pid(window: Window) -> Option<u32> {
    let state = get()?;
    let pid = *property_u32(window, state.atoms.net_wm_pid, AtomEnum::CARDINAL, 1)?.first()?;
    (pid != 0).then_some(pid)
}

pub fn window_title(window: Window) -> Option<String> {
    let state = get()?;
    let utf8 = property(
        window,
        state.atoms.net_wm_name,
        state.atoms.utf8_string,
        1024,
    );
    let raw = utf8.or_else(|| {
        property(
            window,
            u32::from(AtomEnum::WM_NAME),
            u32::from(AtomEnum::ANY),
            1024,
        )
    })?;
    let title = String::from_utf8_lossy(&raw).trim().to_string();
    (!title.is_empty()).then_some(title)
}

/// The window icon closest to 32 px, as `(rgba, width, height)`.
pub fn window_icon(window: Window) -> Option<(Vec<u8>, u32, u32)> {
    let state = get()?;
    let words = property_u32(
        window,
        state.atoms.net_wm_icon,
        AtomEnum::CARDINAL,
        MAX_ICON_SIDE * MAX_ICON_SIDE + 2,
    )?;
    let mut best: Option<(u32, u32, &[u32])> = None;
    let mut rest = words.as_slice();
    while rest.len() > 2 {
        let (width, height) = (rest[0], rest[1]);
        if width == 0 || height == 0 || width > MAX_ICON_SIDE || height > MAX_ICON_SIDE {
            break;
        }
        let len = (width * height) as usize;
        if rest.len() < 2 + len {
            break;
        }
        let pixels = &rest[2..2 + len];
        let score = |w: u32| (w as i64 - 32).abs();
        if best.is_none_or(|(best_width, _, _)| score(width) < score(best_width)) {
            best = Some((width, height, pixels));
        }
        rest = &rest[2 + len..];
    }
    let (width, height, pixels) = best?;
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for argb in pixels {
        rgba.extend_from_slice(&[
            (argb >> 16) as u8,
            (argb >> 8) as u8,
            *argb as u8,
            (argb >> 24) as u8,
        ]);
    }
    Some((rgba, width, height))
}

/// Takes (or releases) the right mouse button for the whole screen so a pick
/// does not also open a context menu in whatever is underneath. Best effort:
/// it only covers X11 windows, and another client may already hold the grab.
pub fn grab_right_button(grab: bool) {
    let Some(state) = get() else {
        return;
    };
    if grab {
        let _ = state.conn.grab_button(
            false,
            state.root,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            ButtonIndex::M3,
            ModMask::ANY,
        );
    } else {
        let _ = state
            .conn
            .ungrab_button(ButtonIndex::M3, state.root, ModMask::ANY);
    }
    let _ = state.conn.flush();
}

/// Throws away queued events and errors (grabbed clicks, failed grabs) so
/// they do not pile up on a connection nobody reads.
pub fn drain_events() {
    let Some(state) = get() else {
        return;
    };
    while let Ok(Some(_)) = state.conn.poll_for_event() {}
}

/// Process that owns the window under the cursor, if it says so.
pub fn pointer_window_pid() -> Option<u32> {
    let state = get()?;
    let mut window = state.root;
    // Window managers reparent, so the property may sit a few levels down.
    for _ in 0..8 {
        let reply = state.conn.query_pointer(window).ok()?.reply().ok()?;
        let child = reply.child;
        if child == x11rb::NONE {
            return None;
        }
        if let Some(pid) = window_pid(child) {
            return Some(pid);
        }
        window = child;
    }
    None
}
