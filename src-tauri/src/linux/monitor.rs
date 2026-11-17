//! Global keyboard and mouse button state.
//!
//! Two sources, tried in this order:
//!
//! * **evdev** - reads `/dev/input/event*` directly. Works on X11 and Wayland
//!   alike and never sees our own injected input, but needs read access
//!   (usually the `input` group).
//! * **X11 polling** - `XQueryKeymap` / `XQueryPointer` on a timer. Needs no
//!   permissions, but on Wayland it only sees input while an XWayland window
//!   has focus, and it cannot tell injected input from physical input except
//!   by timing.
//!
//! State changes are published to subscribers, which is what the emulated
//! low level hooks in `win32` are built on.

use std::collections::{HashMap, HashSet};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use super::win32::*;
use super::{inject, keymap, uinput, x11};

const TICK: Duration = Duration::from_millis(8);
const RESCAN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEvent {
    Key {
        vk: u16,
        code: u16,
        down: bool,
        injected: bool,
    },
    Move {
        x: i32,
        y: i32,
    },
    /// Nothing happened; a waiting thread should re-check its quit flag.
    Wake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Source {
    Starting = 0,
    Evdev = 1,
    X11 = 2,
    None = 3,
}

static SOURCE: AtomicU8 = AtomicU8::new(Source::Starting as u8);
static KEY_STATE: OnceLock<[AtomicBool; 256]> = OnceLock::new();
static SUBSCRIBERS: Mutex<Vec<(u32, Sender<RawEvent>)>> = Mutex::new(Vec::new());
static START: Once = Once::new();

fn key_state() -> &'static [AtomicBool; 256] {
    KEY_STATE.get_or_init(|| std::array::from_fn(|_| AtomicBool::new(false)))
}

pub fn source() -> Source {
    match SOURCE.load(Ordering::Relaxed) {
        1 => Source::Evdev,
        2 => Source::X11,
        3 => Source::None,
        _ => Source::Starting,
    }
}

/// Short human readable description for the UI and diagnostics.
pub fn describe() -> String {
    match source() {
        Source::Evdev => String::from("evdev"),
        Source::X11 if inject::is_wayland_session() => {
            String::from("X11 polling (XWayland windows only)")
        }
        Source::X11 => String::from("X11 polling"),
        Source::None => String::from("unavailable"),
        Source::Starting => String::from("starting"),
    }
}

/// Whether `vk` is held. The side-less modifiers match either side.
pub fn is_down(vk: i32) -> bool {
    ensure_started();
    let held = |vk: u16| key_state()[vk as usize].load(Ordering::Relaxed);
    match u16::try_from(vk) {
        Ok(VK_SHIFT) => held(VK_LSHIFT) || held(VK_RSHIFT),
        Ok(VK_CONTROL) => held(VK_LCONTROL) || held(VK_RCONTROL),
        Ok(VK_MENU) => held(VK_LMENU) || held(VK_RMENU),
        Ok(vk) if vk < 256 => held(vk),
        _ => false,
    }
}

pub fn ensure_started() {
    START.call_once(|| {
        std::thread::Builder::new()
            .name(String::from("input-monitor"))
            .spawn(run)
            .map(|_| ())
            .unwrap_or_else(|e| {
                log::error!("[input] Could not start the input monitor: {e}");
                SOURCE.store(Source::None as u8, Ordering::Relaxed);
            });
    });
}

pub fn subscribe(thread_id: u32) -> Receiver<RawEvent> {
    ensure_started();
    let (tx, rx) = mpsc::channel();
    let mut subscribers = SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner());
    subscribers.retain(|(id, _)| *id != thread_id);
    subscribers.push((thread_id, tx));
    rx
}

pub fn unsubscribe(thread_id: u32) {
    SUBSCRIBERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(id, _)| *id != thread_id);
}

pub fn wake(thread_id: u32) {
    let subscribers = SUBSCRIBERS.lock().unwrap_or_else(|e| e.into_inner());
    for (id, tx) in subscribers.iter() {
        if *id == thread_id {
            let _ = tx.send(RawEvent::Wake);
        }
    }
}

fn has_subscribers() -> bool {
    !SUBSCRIBERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
}

fn publish(event: RawEvent) {
    SUBSCRIBERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(_, tx)| tx.send(event).is_ok());
}

struct OpenDevice {
    device: evdev::Device,
    held: HashSet<u16>,
    /// Created through uinput by some other program. Only these can replay
    /// input we injected (see `inject::consume_echo`).
    is_virtual: bool,
}

fn is_virtual_device(event_node: &std::path::Path) -> bool {
    let Some(name) = event_node.file_name() else {
        return false;
    };
    std::fs::canonicalize(std::path::Path::new("/sys/class/input").join(name))
        .is_ok_and(|resolved| resolved.starts_with("/sys/devices/virtual"))
}

#[derive(Default)]
struct Evdev {
    devices: HashMap<PathBuf, OpenDevice>,
    denied: bool,
}

fn is_input_device(device: &evdev::Device) -> bool {
    if device
        .name()
        .is_some_and(|name| name.starts_with(uinput::DEVICE_NAME_PREFIX))
    {
        return false;
    }
    device.supported_keys().is_some_and(|keys| {
        keys.contains(evdev::Key::KEY_A)
            || keys.contains(evdev::Key::KEY_ENTER)
            || keys.contains(evdev::Key::BTN_LEFT)
    })
}

impl Evdev {
    fn rescan(&mut self) {
        let mut denied = false;
        let Ok(entries) = std::fs::read_dir("/dev/input") else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_event_node = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("event"));
            if !is_event_node || self.devices.contains_key(&path) {
                continue;
            }
            match evdev::Device::open(&path) {
                Ok(device) => {
                    if !is_input_device(&device) {
                        continue;
                    }
                    unsafe {
                        let fd = device.as_raw_fd();
                        let flags = libc::fcntl(fd, libc::F_GETFL);
                        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                    }
                    log::debug!(
                        "[input] Watching {} ({})",
                        path.display(),
                        device.name().unwrap_or("unnamed")
                    );
                    let is_virtual = is_virtual_device(&path);
                    self.devices.insert(
                        path,
                        OpenDevice {
                            device,
                            held: HashSet::new(),
                            is_virtual,
                        },
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
                Err(_) => {}
            }
        }
        self.denied = denied && self.devices.is_empty();
    }

    fn held_anywhere(&self, vk: u16) -> bool {
        self.devices.values().any(|open| open.held.contains(&vk))
    }

    /// Waits up to one tick for input and applies it.
    fn pump(&mut self) {
        let mut fds: Vec<libc::pollfd> = self
            .devices
            .values()
            .map(|open| libc::pollfd {
                fd: open.device.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, TICK.as_millis() as _) };
        if ready <= 0 {
            return;
        }

        let mut changes: Vec<(u16, u16, bool)> = Vec::new();
        let mut gone: Vec<PathBuf> = Vec::new();
        for (path, open) in self.devices.iter_mut() {
            match open.device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        let evdev::InputEventKind::Key(key) = event.kind() else {
                            continue;
                        };
                        let Some(vk) = keymap::evdev_to_vk(key.code()) else {
                            continue;
                        };
                        let down = match event.value() {
                            0 => false,
                            1 => true,
                            _ => continue,
                        };
                        if open.is_virtual && inject::consume_echo(vk, down) {
                            continue;
                        }
                        match event.value() {
                            0 => {
                                open.held.remove(&vk);
                                changes.push((vk, key.code(), false));
                            }
                            1 => {
                                open.held.insert(vk);
                                changes.push((vk, key.code(), true));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => gone.push(path.clone()),
            }
        }

        for path in gone {
            if let Some(open) = self.devices.remove(&path) {
                log::debug!("[input] Lost {}", path.display());
                for vk in open.held {
                    changes.push((vk, keymap::vk_to_evdev(vk).unwrap_or(0), false));
                }
            }
        }

        for (vk, code, down) in changes {
            let now_down = if down { true } else { self.held_anywhere(vk) };
            let slot = &key_state()[vk as usize];
            if slot.swap(now_down, Ordering::Relaxed) != now_down {
                publish(RawEvent::Key {
                    vk,
                    code,
                    down: now_down,
                    injected: false,
                });
            }
        }
    }
}

/// One X11 polling pass: diff the server's idea of what is held against ours.
fn poll_x11(tracked: &[(u16, u16)]) {
    let keys = x11::keymap();
    let pointer = x11::pointer();
    for (vk, code) in tracked {
        let down = if keymap::is_mouse_vk(*vk) {
            let Some(pointer) = pointer else { continue };
            let bit = match *vk {
                VK_LBUTTON => x11::Pointer::BUTTON1,
                VK_MBUTTON => x11::Pointer::BUTTON2,
                VK_RBUTTON => x11::Pointer::BUTTON3,
                // The core protocol has no state bits for the thumb buttons.
                _ => continue,
            };
            pointer.mask & bit != 0
        } else {
            let Some(keys) = keys.as_ref() else { continue };
            x11::keymap_has(keys, *code)
        };
        let slot = &key_state()[*vk as usize];
        if slot.swap(down, Ordering::Relaxed) != down {
            publish(RawEvent::Key {
                vk: *vk,
                code: *code,
                down,
                injected: inject::recently_injected(*vk),
            });
        }
    }
}

fn x11_tracked_keys() -> Vec<(u16, u16)> {
    let mut seen = HashSet::new();
    let mut tracked = Vec::new();
    for vk in 1u16..256 {
        if let Some(code) = keymap::vk_to_evdev(vk) {
            if seen.insert(vk) {
                tracked.push((vk, code));
            }
        }
    }
    tracked
}

fn run() {
    let mut evdev = Evdev::default();
    evdev.rescan();
    let mut last_scan = Instant::now();
    let tracked = x11_tracked_keys();
    let mut last_cursor: Option<(i32, i32)> = None;
    let mut announced = Source::Starting;

    loop {
        if last_scan.elapsed() >= RESCAN_INTERVAL {
            evdev.rescan();
            last_scan = Instant::now();
        }

        let current = if !evdev.devices.is_empty() {
            Source::Evdev
        } else if x11::available() {
            Source::X11
        } else {
            Source::None
        };
        if current != announced {
            announced = current;
            SOURCE.store(current as u8, Ordering::Relaxed);
            match current {
                Source::Evdev => log::info!(
                    "[input] Reading hotkeys from {} evdev device(s)",
                    evdev.devices.len()
                ),
                Source::X11 => log::warn!(
                    "[input] Reading hotkeys by polling X11{}{}",
                    if evdev.denied {
                        " because /dev/input is not readable (add your user to the 'input' group)"
                    } else {
                        ""
                    },
                    if inject::is_wayland_session() {
                        "; on Wayland this only works while an XWayland window has focus"
                    } else {
                        ""
                    }
                ),
                Source::None => log::error!(
                    "[input] Hotkeys are unavailable: /dev/input is not readable and there is no X server. Add your user to the 'input' group and log in again."
                ),
                Source::Starting => {}
            }
        }

        match current {
            Source::Evdev => evdev.pump(),
            Source::X11 => {
                std::thread::sleep(TICK);
                poll_x11(&tracked);
            }
            _ => std::thread::sleep(Duration::from_millis(250)),
        }

        if has_subscribers() {
            x11::drain_events();
            let cursor = x11::cursor_pos();
            if cursor.is_some() && cursor != last_cursor {
                last_cursor = cursor;
                if let Some((x, y)) = cursor {
                    publish(RawEvent::Move { x, y });
                }
            }
        }
    }
}
