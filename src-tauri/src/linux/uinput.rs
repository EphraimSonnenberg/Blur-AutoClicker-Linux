//! Kernel level injection through `/dev/uinput`.
//!
//! This is the only injection path that reaches native Wayland windows. It
//! needs write access to `/dev/uinput` (usually the `input` group, or a udev
//! rule).

use std::sync::{Mutex, OnceLock};

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{
    AbsInfo, AbsoluteAxisType, AttributeSet, EventType, InputEvent, Key, RelativeAxisType,
    UinputAbsSetup,
};

use super::keymap;

/// Our devices all start with this, so the input monitor can skip them and
/// never mistake an injected click for a physical one.
pub const DEVICE_NAME_PREFIX: &str = "blur-autoclicker";

const ABS_MAX: i32 = 65_535;

struct Devices {
    keyboard: VirtualDevice,
    mouse: VirtualDevice,
    pointer: Option<VirtualDevice>,
}

static DEVICES: OnceLock<Option<Mutex<Devices>>> = OnceLock::new();

fn mouse_buttons() -> AttributeSet<Key> {
    AttributeSet::from_iter([
        Key::new(keymap::BTN_LEFT),
        Key::new(keymap::BTN_RIGHT),
        Key::new(keymap::BTN_MIDDLE),
        Key::new(keymap::BTN_SIDE),
        Key::new(keymap::BTN_EXTRA),
    ])
}

fn build() -> std::io::Result<Devices> {
    let keys: AttributeSet<Key> = keymap::all_keyboard_codes().map(Key::new).collect();
    let keyboard = VirtualDeviceBuilder::new()?
        .name(&format!("{DEVICE_NAME_PREFIX}-keyboard"))
        .with_keys(&keys)?
        .build()?;

    let mouse = VirtualDeviceBuilder::new()?
        .name(&format!("{DEVICE_NAME_PREFIX}-mouse"))
        .with_keys(&mouse_buttons())?
        .with_relative_axes(&AttributeSet::from_iter([
            RelativeAxisType::REL_X,
            RelativeAxisType::REL_Y,
            RelativeAxisType::REL_WHEEL,
        ]))?
        .build()?;

    // An absolute device with mouse buttons is what virtual machine tablets
    // look like; compositors map its range onto the whole desktop.
    let axis = |axis| UinputAbsSetup::new(axis, AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0));
    let pointer_name = format!("{DEVICE_NAME_PREFIX}-pointer");
    let pointer = VirtualDeviceBuilder::new()
        .and_then(|b| b.name(&pointer_name).with_keys(&mouse_buttons()))
        .and_then(|b| b.with_absolute_axis(&axis(AbsoluteAxisType::ABS_X)))
        .and_then(|b| b.with_absolute_axis(&axis(AbsoluteAxisType::ABS_Y)))
        .and_then(|b| b.build());
    let pointer = match pointer {
        Ok(device) => Some(device),
        Err(e) => {
            log::warn!("[uinput] No absolute pointer device, cursor moves are unavailable: {e}");
            None
        }
    };

    Ok(Devices {
        keyboard,
        mouse,
        pointer,
    })
}

fn get() -> Option<&'static Mutex<Devices>> {
    DEVICES
        .get_or_init(|| match build() {
            Ok(devices) => {
                log::info!("[uinput] Virtual input devices created");
                Some(Mutex::new(devices))
            }
            Err(e) => {
                log::warn!(
                    "[uinput] /dev/uinput is not usable ({e}). Add your user to the 'input' group (sudo usermod -aG input $USER) and log in again."
                );
                None
            }
        })
        .as_ref()
}

/// Creates the devices if needed. Compositors take a moment to adopt a new
/// device, so call this early rather than on the first click.
pub fn available() -> bool {
    get().is_some()
}

fn sync() -> InputEvent {
    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0)
}

/// Presses or releases an evdev key or button code.
pub fn key(code: u16, down: bool) -> bool {
    let Some(devices) = get() else {
        return false;
    };
    let mut devices = devices.lock().unwrap_or_else(|e| e.into_inner());
    let event = InputEvent::new(EventType::KEY, code, i32::from(down));
    let device = if code >= keymap::BTN_LEFT {
        &mut devices.mouse
    } else {
        &mut devices.keyboard
    };
    device.emit(&[event, sync()]).is_ok()
}

/// Moves the cursor to `(x, y)` inside a desktop of the given bounds
/// `(left, top, width, height)`.
pub fn move_abs(x: i32, y: i32, bounds: (i32, i32, i32, i32)) -> bool {
    let Some(devices) = get() else {
        return false;
    };
    let (left, top, width, height) = bounds;
    if width <= 1 || height <= 1 {
        return false;
    }
    let scale = |value: i32, origin: i32, span: i32| -> i32 {
        let ratio = (value - origin) as f64 / (span - 1) as f64;
        (ratio.clamp(0.0, 1.0) * ABS_MAX as f64).round() as i32
    };
    let mut devices = devices.lock().unwrap_or_else(|e| e.into_inner());
    let Some(pointer) = devices.pointer.as_mut() else {
        return false;
    };
    pointer
        .emit(&[
            InputEvent::new(
                EventType::ABSOLUTE,
                AbsoluteAxisType::ABS_X.0,
                scale(x, left, width),
            ),
            InputEvent::new(
                EventType::ABSOLUTE,
                AbsoluteAxisType::ABS_Y.0,
                scale(y, top, height),
            ),
            sync(),
        ])
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn open_own_device(suffix: &str) -> Option<evdev::Device> {
        let wanted = format!("{DEVICE_NAME_PREFIX}-{suffix}");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let found = evdev::enumerate()
                .map(|(_, device)| device)
                .find(|device| device.name() == Some(wanted.as_str()));
            if found.is_some() {
                return found;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// Sends a key through the virtual keyboard and reads it back from the
    /// device node. The node is grabbed first, so the desktop never sees the
    /// key. Skips itself where uinput or evdev is not accessible (CI).
    #[test]
    fn injected_keys_arrive_on_the_virtual_keyboard() {
        if !available() {
            eprintln!("skipped: /dev/uinput is not accessible");
            return;
        }
        let Some(mut device) = open_own_device("keyboard") else {
            eprintln!("skipped: the virtual keyboard node is not readable");
            return;
        };
        if device.grab().is_err() {
            eprintln!("skipped: could not grab the virtual keyboard");
            return;
        }

        const KEY_F: u16 = 33;
        assert!(key(KEY_F, true));
        assert!(key(KEY_F, false));

        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while seen.len() < 2 && Instant::now() < deadline {
            for event in device.fetch_events().expect("virtual keyboard is readable") {
                if let evdev::InputEventKind::Key(pressed) = event.kind() {
                    seen.push((pressed.code(), event.value()));
                }
            }
        }
        let _ = device.ungrab();
        assert_eq!(seen, vec![(KEY_F, 1), (KEY_F, 0)]);
    }

    #[test]
    fn absolute_moves_reject_degenerate_desktops() {
        assert!(!move_abs(10, 10, (0, 0, 0, 0)));
        assert!(!move_abs(10, 10, (0, 0, 1, 1)));
    }
}
