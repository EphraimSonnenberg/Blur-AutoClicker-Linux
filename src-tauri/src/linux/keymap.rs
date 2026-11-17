//! Windows virtual key <-> evdev key code tables.
//!
//! evdev codes are physical positions (they equal PC set 1 scan codes for the
//! main block), so this is a US-layout mapping, the same assumption Windows
//! makes when the app falls back from `MapVirtualKeyW`.

use super::win32::*;

pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_SIDE: u16 = 0x113;
pub const BTN_EXTRA: u16 = 0x114;
pub const BTN_FORWARD: u16 = 0x115;
pub const BTN_BACK: u16 = 0x116;

pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_CAPSLOCK: u16 = 58;

/// (virtual key, evdev code). First match wins in both directions.
const TABLE: &[(u16, u16)] = &[
    (VK_ESCAPE, 1),
    (0x31, 2),
    (0x32, 3),
    (0x33, 4),
    (0x34, 5),
    (0x35, 6),
    (0x36, 7),
    (0x37, 8),
    (0x38, 9),
    (0x39, 10),
    (0x30, 11),
    (VK_OEM_MINUS, 12),
    (VK_OEM_PLUS, 13),
    (VK_BACK, 14),
    (VK_TAB, 15),
    (b'Q' as u16, 16),
    (b'W' as u16, 17),
    (b'E' as u16, 18),
    (b'R' as u16, 19),
    (b'T' as u16, 20),
    (b'Y' as u16, 21),
    (b'U' as u16, 22),
    (b'I' as u16, 23),
    (b'O' as u16, 24),
    (b'P' as u16, 25),
    (VK_OEM_4, 26),
    (VK_OEM_6, 27),
    (VK_RETURN, 28),
    (VK_LCONTROL, 29),
    (b'A' as u16, 30),
    (b'S' as u16, 31),
    (b'D' as u16, 32),
    (b'F' as u16, 33),
    (b'G' as u16, 34),
    (b'H' as u16, 35),
    (b'J' as u16, 36),
    (b'K' as u16, 37),
    (b'L' as u16, 38),
    (VK_OEM_1, 39),
    (VK_OEM_7, 40),
    (VK_OEM_3, 41),
    (VK_LSHIFT, KEY_LEFTSHIFT),
    (VK_OEM_5, 43),
    (b'Z' as u16, 44),
    (b'X' as u16, 45),
    (b'C' as u16, 46),
    (b'V' as u16, 47),
    (b'B' as u16, 48),
    (b'N' as u16, 49),
    (b'M' as u16, 50),
    (VK_OEM_COMMA, 51),
    (VK_OEM_PERIOD, 52),
    (VK_OEM_2, 53),
    (VK_RSHIFT, 54),
    (VK_MULTIPLY, 55),
    (VK_LMENU, 56),
    (VK_SPACE, 57),
    (VK_CAPITAL, KEY_CAPSLOCK),
    (VK_F1, 59),
    (VK_F1 + 1, 60),
    (VK_F1 + 2, 61),
    (VK_F1 + 3, 62),
    (VK_F1 + 4, 63),
    (VK_F1 + 5, 64),
    (VK_F1 + 6, 65),
    (VK_F1 + 7, 66),
    (VK_F1 + 8, 67),
    (VK_F1 + 9, 68),
    (VK_NUMLOCK, 69),
    (VK_SCROLL, 70),
    (VK_NUMPAD7, 71),
    (VK_NUMPAD8, 72),
    (VK_NUMPAD9, 73),
    (VK_SUBTRACT, 74),
    (VK_NUMPAD4, 75),
    (VK_NUMPAD5, 76),
    (VK_NUMPAD6, 77),
    (VK_ADD, 78),
    (VK_NUMPAD1, 79),
    (VK_NUMPAD2, 80),
    (VK_NUMPAD3, 81),
    (VK_NUMPAD0, 82),
    (VK_DECIMAL, 83),
    (VK_OEM_102, 86),
    (VK_F1 + 10, 87),
    (VK_F1 + 11, 88),
    // Windows reports numpad Enter as VK_RETURN (extended); keep that.
    (VK_RETURN, 96),
    (VK_RCONTROL, 97),
    (VK_DIVIDE, 98),
    (VK_SNAPSHOT, 99),
    (VK_RMENU, 100),
    (VK_HOME, 102),
    (VK_UP, 103),
    (VK_PRIOR, 104),
    (VK_LEFT, 105),
    (VK_RIGHT, 106),
    (VK_END, 107),
    (VK_DOWN, 108),
    (VK_NEXT, 109),
    (VK_INSERT, 110),
    (VK_DELETE, 111),
    (VK_PAUSE, 119),
    (VK_LWIN, 125),
    (VK_RWIN, 126),
    (VK_APPS, 127),
    (VK_F1 + 12, 183),
    (VK_F1 + 13, 184),
    (VK_F1 + 14, 185),
    (VK_F1 + 15, 186),
    (VK_F1 + 16, 187),
    (VK_F1 + 17, 188),
    (VK_F1 + 18, 189),
    (VK_F1 + 19, 190),
    (VK_F1 + 20, 191),
    (VK_F1 + 21, 192),
    (VK_F1 + 22, 193),
    (VK_F1 + 23, 194),
    (VK_LBUTTON, BTN_LEFT),
    (VK_RBUTTON, BTN_RIGHT),
    (VK_MBUTTON, BTN_MIDDLE),
    (VK_XBUTTON1, BTN_SIDE),
    (VK_XBUTTON2, BTN_EXTRA),
    // Some mice report the thumb buttons as BACK / FORWARD instead.
    (VK_XBUTTON1, BTN_BACK),
    (VK_XBUTTON2, BTN_FORWARD),
];

/// evdev code for a virtual key. The side-less modifiers map to the left key,
/// which is what Windows injects for them too.
pub fn vk_to_evdev(vk: u16) -> Option<u16> {
    let vk = match vk {
        VK_SHIFT => VK_LSHIFT,
        VK_CONTROL => VK_LCONTROL,
        VK_MENU => VK_LMENU,
        other => other,
    };
    TABLE.iter().find(|(v, _)| *v == vk).map(|(_, code)| *code)
}

pub fn evdev_to_vk(code: u16) -> Option<u16> {
    TABLE.iter().find(|(_, c)| *c == code).map(|(vk, _)| *vk)
}

pub fn is_mouse_vk(vk: u16) -> bool {
    matches!(
        vk,
        VK_LBUTTON | VK_RBUTTON | VK_MBUTTON | VK_XBUTTON1 | VK_XBUTTON2
    )
}

/// Every evdev key code the table can produce, for building virtual devices.
pub fn all_keyboard_codes() -> impl Iterator<Item = u16> {
    TABLE.iter().map(|(_, code)| *code).filter(|c| *c < 0x100)
}

/// `VkKeyScanW` for a US layout: low byte is the virtual key, high byte is
/// non-zero when the character needs Shift. `-1` when there is no key.
pub fn vk_key_scan(ch: u16) -> i16 {
    let Some(ch) = char::from_u32(ch as u32) else {
        return -1;
    };
    const SHIFT: i16 = 0x100;
    let plain = |vk: u16| vk as i16;
    let shifted = |vk: u16| vk as i16 | SHIFT;
    match ch {
        'a'..='z' => plain(ch.to_ascii_uppercase() as u16),
        'A'..='Z' => shifted(ch as u16),
        '0'..='9' => plain(ch as u16),
        ' ' => plain(VK_SPACE),
        '`' => plain(VK_OEM_3),
        '~' => shifted(VK_OEM_3),
        '-' => plain(VK_OEM_MINUS),
        '_' => shifted(VK_OEM_MINUS),
        '=' => plain(VK_OEM_PLUS),
        '+' => shifted(VK_OEM_PLUS),
        '[' => plain(VK_OEM_4),
        '{' => shifted(VK_OEM_4),
        ']' => plain(VK_OEM_6),
        '}' => shifted(VK_OEM_6),
        '\\' => plain(VK_OEM_5),
        '|' => shifted(VK_OEM_5),
        ';' => plain(VK_OEM_1),
        ':' => shifted(VK_OEM_1),
        '\'' => plain(VK_OEM_7),
        '"' => shifted(VK_OEM_7),
        ',' => plain(VK_OEM_COMMA),
        '<' => shifted(VK_OEM_COMMA),
        '.' => plain(VK_OEM_PERIOD),
        '>' => shifted(VK_OEM_PERIOD),
        '/' => plain(VK_OEM_2),
        '?' => shifted(VK_OEM_2),
        '!' => shifted(0x31),
        '@' => shifted(0x32),
        '#' => shifted(0x33),
        '$' => shifted(0x34),
        '%' => shifted(0x35),
        '^' => shifted(0x36),
        '&' => shifted(0x37),
        '*' => shifted(0x38),
        '(' => shifted(0x39),
        ')' => shifted(0x30),
        _ => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_modifiers_round_trip() {
        for vk in (b'A' as u16..=b'Z' as u16).chain(b'0' as u16..=b'9' as u16) {
            let code = vk_to_evdev(vk).expect("alphanumeric keys are mapped");
            assert_eq!(evdev_to_vk(code), Some(vk));
        }
        for vk in [
            VK_LSHIFT,
            VK_RSHIFT,
            VK_LCONTROL,
            VK_RCONTROL,
            VK_LMENU,
            VK_RMENU,
            VK_LWIN,
            VK_RWIN,
        ] {
            let code = vk_to_evdev(vk).expect("modifiers are mapped");
            assert_eq!(evdev_to_vk(code), Some(vk));
        }
    }

    #[test]
    fn generic_modifiers_inject_the_left_key() {
        assert_eq!(vk_to_evdev(VK_SHIFT), vk_to_evdev(VK_LSHIFT));
        assert_eq!(vk_to_evdev(VK_CONTROL), vk_to_evdev(VK_LCONTROL));
        assert_eq!(vk_to_evdev(VK_MENU), vk_to_evdev(VK_LMENU));
    }

    #[test]
    fn both_thumb_button_conventions_map_to_the_x_buttons() {
        assert_eq!(evdev_to_vk(BTN_SIDE), Some(VK_XBUTTON1));
        assert_eq!(evdev_to_vk(BTN_BACK), Some(VK_XBUTTON1));
        assert_eq!(evdev_to_vk(BTN_EXTRA), Some(VK_XBUTTON2));
        assert_eq!(evdev_to_vk(BTN_FORWARD), Some(VK_XBUTTON2));
        assert_eq!(vk_to_evdev(VK_XBUTTON1), Some(BTN_SIDE));
    }

    #[test]
    fn numpad_enter_reads_as_return_but_return_injects_the_main_key() {
        assert_eq!(evdev_to_vk(96), Some(VK_RETURN));
        assert_eq!(vk_to_evdev(VK_RETURN), Some(28));
    }

    #[test]
    fn key_scan_flags_shifted_characters() {
        assert_eq!(vk_key_scan('a' as u16), b'A' as i16);
        assert_eq!(vk_key_scan(';' as u16), VK_OEM_1 as i16);
        assert_ne!(vk_key_scan(':' as u16) >> 8, 0);
        assert_eq!(vk_key_scan('é' as u16), -1);
    }
}
