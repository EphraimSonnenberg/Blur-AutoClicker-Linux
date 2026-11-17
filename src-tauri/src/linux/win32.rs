//! The slice of Win32 the shared code is written against, for Linux.
//!
//! Names, signatures and constant values match `windows_sys` so the shared
//! modules can swap one import for the other. Functions are `unsafe` only to
//! keep call sites identical on both platforms.
//!
//! Low level hooks are emulated: a "hook" is a callback registered on the
//! current thread, and the message functions deliver input events from
//! `monitor` to it. Events cannot be swallowed the way a real hook can; the
//! pickers take an X11 button grab for that instead.

#![allow(
    non_snake_case,
    non_camel_case_types,
    dead_code,
    clippy::missing_safety_doc,
    clippy::upper_case_acronyms
)]

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

use super::monitor::{self, RawEvent};
use super::{keymap, x11};
use crate::engine::AUTOCLICKER_EXTRA_INFO;

pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type HHOOK = *mut std::ffi::c_void;
pub type HWND = *mut std::ffi::c_void;
pub type HOOKPROC = Option<unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT>;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: u32,
    pub scanCode: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MSLLHOOKSTRUCT {
    pub pt: POINT,
    pub mouseData: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

pub const WH_KEYBOARD_LL: i32 = 13;
pub const WH_MOUSE_LL: i32 = 14;

pub const WM_QUIT: u32 = 0x0012;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_RBUTTONDBLCLK: u32 = 0x0206;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const WM_XBUTTONDOWN: u32 = 0x020B;
pub const WM_XBUTTONUP: u32 = 0x020C;

pub const PM_NOREMOVE: u32 = 0x0000;
pub const QS_ALLINPUT: u32 = 0x04FF;
pub const MWMO_INPUTAVAILABLE: u32 = 0x0004;
pub const LLKHF_EXTENDED: u32 = 0x01;

pub const MAPVK_VSC_TO_VK_EX: u32 = 3;
pub const MAPVK_VK_TO_VSC_EX: u32 = 4;

pub const KEYEVENTF_KEYUP: u32 = 0x0002;

pub const MOUSEEVENTF_MOVE: u32 = 0x0001;
pub const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
pub const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
pub const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
pub const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
pub const MOUSEEVENTF_MIDDLEDOWN: u32 = 0x0020;
pub const MOUSEEVENTF_MIDDLEUP: u32 = 0x0040;

pub const VK_LBUTTON: u16 = 0x01;
pub const VK_RBUTTON: u16 = 0x02;
pub const VK_MBUTTON: u16 = 0x04;
pub const VK_XBUTTON1: u16 = 0x05;
pub const VK_XBUTTON2: u16 = 0x06;
pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_PAUSE: u16 = 0x13;
pub const VK_CAPITAL: u16 = 0x14;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_PRIOR: u16 = 0x21;
pub const VK_NEXT: u16 = 0x22;
pub const VK_END: u16 = 0x23;
pub const VK_HOME: u16 = 0x24;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
pub const VK_SNAPSHOT: u16 = 0x2C;
pub const VK_INSERT: u16 = 0x2D;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_APPS: u16 = 0x5D;
pub const VK_NUMPAD0: u16 = 0x60;
pub const VK_NUMPAD1: u16 = 0x61;
pub const VK_NUMPAD2: u16 = 0x62;
pub const VK_NUMPAD3: u16 = 0x63;
pub const VK_NUMPAD4: u16 = 0x64;
pub const VK_NUMPAD5: u16 = 0x65;
pub const VK_NUMPAD6: u16 = 0x66;
pub const VK_NUMPAD7: u16 = 0x67;
pub const VK_NUMPAD8: u16 = 0x68;
pub const VK_NUMPAD9: u16 = 0x69;
pub const VK_MULTIPLY: u16 = 0x6A;
pub const VK_ADD: u16 = 0x6B;
pub const VK_SUBTRACT: u16 = 0x6D;
pub const VK_DECIMAL: u16 = 0x6E;
pub const VK_DIVIDE: u16 = 0x6F;
pub const VK_F1: u16 = 0x70;
pub const VK_NUMLOCK: u16 = 0x90;
pub const VK_SCROLL: u16 = 0x91;
pub const VK_LSHIFT: u16 = 0xA0;
pub const VK_RSHIFT: u16 = 0xA1;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
pub const VK_OEM_1: u16 = 0xBA;
pub const VK_OEM_PLUS: u16 = 0xBB;
pub const VK_OEM_COMMA: u16 = 0xBC;
pub const VK_OEM_MINUS: u16 = 0xBD;
pub const VK_OEM_PERIOD: u16 = 0xBE;
pub const VK_OEM_2: u16 = 0xBF;
pub const VK_OEM_3: u16 = 0xC0;
pub const VK_OEM_4: u16 = 0xDB;
pub const VK_OEM_5: u16 = 0xDC;
pub const VK_OEM_6: u16 = 0xDD;
pub const VK_OEM_7: u16 = 0xDE;
pub const VK_OEM_102: u16 = 0xE2;

/// Double click window desktops default to.
const DOUBLE_CLICK_TIME_MS: u32 = 500;

#[derive(Clone, Copy)]
struct Hook {
    handle: usize,
    kind: i32,
    callback: unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT,
}

thread_local! {
    static THREAD_ID: Cell<u32> = const { Cell::new(0) };
    static HOOKS: RefCell<Vec<Hook>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Option<Receiver<RawEvent>>> = const { RefCell::new(None) };
}

static NEXT_THREAD_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_HOOK: AtomicUsize = AtomicUsize::new(1);
static QUIT: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

pub unsafe fn GetCurrentThreadId() -> u32 {
    THREAD_ID.with(|id| {
        if id.get() == 0 {
            id.set(NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed));
        }
        id.get()
    })
}

pub unsafe fn GetCurrentProcessId() -> u32 {
    std::process::id()
}

pub unsafe fn GetLastError() -> u32 {
    0
}

fn quit_pending(thread_id: u32, take: bool) -> bool {
    let mut quit = QUIT.lock().unwrap_or_else(|e| e.into_inner());
    let set = quit.get_or_insert_with(HashSet::new);
    if take {
        set.remove(&thread_id)
    } else {
        set.contains(&thread_id)
    }
}

pub unsafe fn PostThreadMessageW(thread_id: u32, message: u32, _w: WPARAM, _l: LPARAM) -> i32 {
    if message == WM_QUIT {
        QUIT.lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(HashSet::new)
            .insert(thread_id);
        monitor::wake(thread_id);
    }
    1
}

pub unsafe fn SetWindowsHookExW(kind: i32, callback: HOOKPROC, _module: HHOOK, _tid: u32) -> HHOOK {
    let Some(callback) = callback else {
        return std::ptr::null_mut();
    };
    let thread_id = GetCurrentThreadId();
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        if events.is_none() {
            *events = Some(monitor::subscribe(thread_id));
        }
    });
    let handle = NEXT_HOOK.fetch_add(1, Ordering::Relaxed);
    HOOKS.with(|hooks| {
        hooks.borrow_mut().push(Hook {
            handle,
            kind,
            callback,
        })
    });
    handle as HHOOK
}

pub unsafe fn UnhookWindowsHookEx(hook: HHOOK) -> i32 {
    let handle = hook as usize;
    let (removed, empty) = HOOKS.with(|hooks| {
        let mut hooks = hooks.borrow_mut();
        let before = hooks.len();
        hooks.retain(|hook| hook.handle != handle);
        (hooks.len() != before, hooks.is_empty())
    });
    if removed && empty {
        monitor::unsubscribe(GetCurrentThreadId());
        EVENTS.with(|events| events.borrow_mut().take());
    }
    i32::from(removed)
}

pub unsafe fn CallNextHookEx(_hook: HHOOK, _code: i32, _w: WPARAM, _l: LPARAM) -> LRESULT {
    0
}

fn hooks_of(kind: i32) -> Vec<Hook> {
    HOOKS.with(|hooks| {
        hooks
            .borrow()
            .iter()
            .filter(|hook| hook.kind == kind)
            .copied()
            .collect()
    })
}

fn mouse_message(vk: u16, down: bool) -> Option<(u32, u32)> {
    let message = match (vk, down) {
        (VK_LBUTTON, true) => (WM_LBUTTONDOWN, 0),
        (VK_LBUTTON, false) => (WM_LBUTTONUP, 0),
        (VK_RBUTTON, true) => (WM_RBUTTONDOWN, 0),
        (VK_RBUTTON, false) => (WM_RBUTTONUP, 0),
        (VK_MBUTTON, true) => (WM_MBUTTONDOWN, 0),
        (VK_MBUTTON, false) => (WM_MBUTTONUP, 0),
        (VK_XBUTTON1, true) => (WM_XBUTTONDOWN, 1 << 16),
        (VK_XBUTTON1, false) => (WM_XBUTTONUP, 1 << 16),
        (VK_XBUTTON2, true) => (WM_XBUTTONDOWN, 2 << 16),
        (VK_XBUTTON2, false) => (WM_XBUTTONUP, 2 << 16),
        _ => return None,
    };
    Some(message)
}

fn dispatch(event: RawEvent) {
    match event {
        RawEvent::Wake => {}
        RawEvent::Move { x, y } => {
            let info = MSLLHOOKSTRUCT {
                pt: POINT { x, y },
                ..Default::default()
            };
            for hook in hooks_of(WH_MOUSE_LL) {
                unsafe {
                    (hook.callback)(0, WM_MOUSEMOVE as WPARAM, &info as *const _ as LPARAM);
                }
            }
        }
        RawEvent::Key {
            vk,
            code,
            down,
            injected,
        } => {
            let extra = if injected { AUTOCLICKER_EXTRA_INFO } else { 0 };
            if let Some((message, mouse_data)) = mouse_message(vk, down) {
                let (x, y) = x11::cursor_pos().unwrap_or((0, 0));
                let info = MSLLHOOKSTRUCT {
                    pt: POINT { x, y },
                    mouseData: mouse_data,
                    dwExtraInfo: extra,
                    ..Default::default()
                };
                for hook in hooks_of(WH_MOUSE_LL) {
                    unsafe {
                        (hook.callback)(0, message as WPARAM, &info as *const _ as LPARAM);
                    }
                }
            } else {
                let info = KBDLLHOOKSTRUCT {
                    vkCode: vk as u32,
                    scanCode: code as u32,
                    dwExtraInfo: extra,
                    ..Default::default()
                };
                let message = if down { WM_KEYDOWN } else { WM_KEYUP };
                for hook in hooks_of(WH_KEYBOARD_LL) {
                    unsafe {
                        (hook.callback)(0, message as WPARAM, &info as *const _ as LPARAM);
                    }
                }
            }
        }
    }
}

/// Delivers queued input to this thread's hooks, waiting up to `timeout` for
/// the first event. Returns whether anything arrived.
fn pump(timeout: Duration) -> bool {
    let first = EVENTS.with(|events| match events.borrow().as_ref() {
        Some(rx) => match rx.recv_timeout(timeout) {
            Ok(event) => Some(Some(event)),
            Err(RecvTimeoutError::Timeout) => Some(None),
            Err(RecvTimeoutError::Disconnected) => None,
        },
        None => None,
    });
    let Some(first) = first else {
        // No hooks on this thread (or the monitor is gone): just wait.
        std::thread::sleep(timeout);
        return false;
    };
    let Some(first) = first else {
        return false;
    };
    dispatch(first);
    loop {
        let next = EVENTS.with(|events| events.borrow().as_ref().and_then(|rx| rx.try_recv().ok()));
        match next {
            Some(event) => dispatch(event),
            None => return true,
        }
    }
}

fn quit_message(msg: *mut MSG) {
    if !msg.is_null() {
        unsafe { (*msg).message = WM_QUIT };
    }
}

/// Blocks until input was delivered to this thread's hooks (returns 1) or
/// the thread was asked to quit (returns 0).
pub unsafe fn GetMessageW(msg: *mut MSG, _hwnd: HWND, _min: u32, _max: u32) -> i32 {
    let thread_id = GetCurrentThreadId();
    loop {
        if quit_pending(thread_id, true) {
            quit_message(msg);
            return 0;
        }
        if pump(Duration::from_millis(50)) {
            if quit_pending(thread_id, true) {
                quit_message(msg);
                return 0;
            }
            return 1;
        }
    }
}

pub unsafe fn PeekMessageW(msg: *mut MSG, _hwnd: HWND, _min: u32, _max: u32, remove: u32) -> i32 {
    let thread_id = GetCurrentThreadId();
    if quit_pending(thread_id, remove != PM_NOREMOVE) {
        quit_message(msg);
        return 1;
    }
    0
}

pub unsafe fn MsgWaitForMultipleObjectsEx(
    _count: u32,
    _handles: *const HHOOK,
    milliseconds: u32,
    _wake_mask: u32,
    _flags: u32,
) -> u32 {
    pump(Duration::from_millis(milliseconds as u64));
    0
}

pub unsafe fn GetAsyncKeyState(vk: i32) -> i16 {
    if monitor::is_down(vk) {
        0x8000u16 as i16
    } else {
        0
    }
}

/// Only the toggle bit of Caps Lock is meaningful here.
pub unsafe fn GetKeyState(vk: i32) -> i16 {
    if vk == VK_CAPITAL as i32 {
        return i16::from(x11::caps_lock_on().unwrap_or(false));
    }
    GetAsyncKeyState(vk)
}

pub unsafe fn MapVirtualKeyW(code: u32, map_type: u32) -> u32 {
    let Ok(code) = u16::try_from(code) else {
        return 0;
    };
    match map_type {
        MAPVK_VSC_TO_VK_EX => keymap::evdev_to_vk(code).unwrap_or(0) as u32,
        MAPVK_VK_TO_VSC_EX => keymap::vk_to_evdev(code).unwrap_or(0) as u32,
        _ => 0,
    }
}

pub unsafe fn VkKeyScanW(ch: u16) -> i16 {
    keymap::vk_key_scan(ch)
}

pub unsafe fn GetCursorPos(point: *mut POINT) -> i32 {
    match x11::cursor_pos() {
        Some((x, y)) if !point.is_null() => {
            *point = POINT { x, y };
            1
        }
        _ => 0,
    }
}

pub unsafe fn GetDoubleClickTime() -> u32 {
    DOUBLE_CLICK_TIME_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_thread_gets_its_own_stable_id() {
        let here = unsafe { GetCurrentThreadId() };
        assert_ne!(here, 0);
        assert_eq!(here, unsafe { GetCurrentThreadId() });
        let other = std::thread::spawn(|| unsafe { GetCurrentThreadId() })
            .join()
            .unwrap();
        assert_ne!(here, other);
    }

    #[test]
    fn a_posted_quit_ends_the_message_loop_of_that_thread_only() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || unsafe {
            tx.send(GetCurrentThreadId()).unwrap();
            let mut msg: MSG = std::mem::zeroed();
            let result = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            (result, msg.message)
        });
        let worker_id = rx.recv().unwrap();
        unsafe {
            PostThreadMessageW(worker_id, WM_QUIT, 0, 0);
            let mut msg: MSG = std::mem::zeroed();
            assert_eq!(PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 1), 0);
        }
        assert_eq!(worker.join().unwrap(), (0, WM_QUIT));
    }

    #[test]
    fn thumb_buttons_carry_their_index_like_windows_does() {
        assert_eq!(
            mouse_message(VK_XBUTTON1, true),
            Some((WM_XBUTTONDOWN, 1 << 16))
        );
        assert_eq!(
            mouse_message(VK_XBUTTON2, false),
            Some((WM_XBUTTONUP, 2 << 16))
        );
        assert_eq!(mouse_message(VK_SPACE, true), None);
    }
}
