//! PC keyboard scan codes (set 1, as reported by Windows) -> USB HID usages.
//!
//! Scan codes describe key *positions*, so the controlled computer applies its own
//! keyboard layout, exactly as with a physical keyboard plugged into it.

use KeyUsage::{Consumer as C, Keyboard as K};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyUsage {
    Keyboard(u8),
    Consumer(u16),
}

pub const EXTENDED: u16 = 0xE000;

const KEYS: &[(u16, KeyUsage, &str)] = &[
    (0x01, K(0x29), "Escape"),
    (0x02, K(0x1E), "1"),
    (0x03, K(0x1F), "2"),
    (0x04, K(0x20), "3"),
    (0x05, K(0x21), "4"),
    (0x06, K(0x22), "5"),
    (0x07, K(0x23), "6"),
    (0x08, K(0x24), "7"),
    (0x09, K(0x25), "8"),
    (0x0A, K(0x26), "9"),
    (0x0B, K(0x27), "0"),
    (0x0C, K(0x2D), "-"),
    (0x0D, K(0x2E), "="),
    (0x0E, K(0x2A), "Backspace"),
    (0x0F, K(0x2B), "Tab"),
    (0x10, K(0x14), "Q"),
    (0x11, K(0x1A), "W"),
    (0x12, K(0x08), "E"),
    (0x13, K(0x15), "R"),
    (0x14, K(0x17), "T"),
    (0x15, K(0x1C), "Y"),
    (0x16, K(0x18), "U"),
    (0x17, K(0x0C), "I"),
    (0x18, K(0x12), "O"),
    (0x19, K(0x13), "P"),
    (0x1A, K(0x2F), "["),
    (0x1B, K(0x30), "]"),
    (0x1C, K(0x28), "Enter"),
    (0x1D, K(0xE0), "Left Ctrl"),
    (0x1E, K(0x04), "A"),
    (0x1F, K(0x16), "S"),
    (0x20, K(0x07), "D"),
    (0x21, K(0x09), "F"),
    (0x22, K(0x0A), "G"),
    (0x23, K(0x0B), "H"),
    (0x24, K(0x0D), "J"),
    (0x25, K(0x0E), "K"),
    (0x26, K(0x0F), "L"),
    (0x27, K(0x33), ";"),
    (0x28, K(0x34), "'"),
    (0x29, K(0x35), "`"),
    (0x2A, K(0xE1), "Left Shift"),
    (0x2B, K(0x31), "Backslash"),
    (0x2C, K(0x1D), "Z"),
    (0x2D, K(0x1B), "X"),
    (0x2E, K(0x06), "C"),
    (0x2F, K(0x19), "V"),
    (0x30, K(0x05), "B"),
    (0x31, K(0x11), "N"),
    (0x32, K(0x10), "M"),
    (0x33, K(0x36), ","),
    (0x34, K(0x37), "."),
    (0x35, K(0x38), "/"),
    (0x36, K(0xE5), "Right Shift"),
    (0x37, K(0x55), "Keypad *"),
    (0x38, K(0xE2), "Left Alt"),
    (0x39, K(0x2C), "Space"),
    (0x3A, K(0x39), "Caps Lock"),
    (0x3B, K(0x3A), "F1"),
    (0x3C, K(0x3B), "F2"),
    (0x3D, K(0x3C), "F3"),
    (0x3E, K(0x3D), "F4"),
    (0x3F, K(0x3E), "F5"),
    (0x40, K(0x3F), "F6"),
    (0x41, K(0x40), "F7"),
    (0x42, K(0x41), "F8"),
    (0x43, K(0x42), "F9"),
    (0x44, K(0x43), "F10"),
    (0x45, K(0x53), "Num Lock"),
    (0x46, K(0x47), "Scroll Lock"),
    (0x47, K(0x5F), "Keypad 7"),
    (0x48, K(0x60), "Keypad 8"),
    (0x49, K(0x61), "Keypad 9"),
    (0x4A, K(0x56), "Keypad -"),
    (0x4B, K(0x5C), "Keypad 4"),
    (0x4C, K(0x5D), "Keypad 5"),
    (0x4D, K(0x5E), "Keypad 6"),
    (0x4E, K(0x57), "Keypad +"),
    (0x4F, K(0x59), "Keypad 1"),
    (0x50, K(0x5A), "Keypad 2"),
    (0x51, K(0x5B), "Keypad 3"),
    (0x52, K(0x62), "Keypad 0"),
    (0x53, K(0x63), "Keypad ."),
    (0x54, K(0x46), "SysRq"),
    (0x56, K(0x64), "ISO key next to Left Shift"),
    (0x57, K(0x44), "F11"),
    (0x58, K(0x45), "F12"),
    (0x59, K(0x67), "Keypad ="),
    (0x64, K(0x68), "F13"),
    (0x65, K(0x69), "F14"),
    (0x66, K(0x6A), "F15"),
    (0x67, K(0x6B), "F16"),
    (0x68, K(0x6C), "F17"),
    (0x69, K(0x6D), "F18"),
    (0x6A, K(0x6E), "F19"),
    (0x6B, K(0x6F), "F20"),
    (0x6C, K(0x70), "F21"),
    (0x6D, K(0x71), "F22"),
    (0x6E, K(0x72), "F23"),
    (0x70, K(0x88), "Katakana/Hiragana"),
    (0x73, K(0x87), "Ro"),
    (0x76, K(0x73), "F24"),
    (0x79, K(0x8A), "Henkan"),
    (0x7B, K(0x8B), "Muhenkan"),
    (0x7D, K(0x89), "Yen"),
    (0x7E, K(0x85), "Keypad ,"),
    (EXTENDED | 0x1C, K(0x58), "Keypad Enter"),
    (EXTENDED | 0x1D, K(0xE4), "Right Ctrl"),
    (EXTENDED | 0x35, K(0x54), "Keypad /"),
    (EXTENDED | 0x37, K(0x46), "Print Screen"),
    (EXTENDED | 0x38, K(0xE6), "Right Alt / AltGr"),
    (EXTENDED | 0x45, K(0x53), "Num Lock"),
    (EXTENDED | 0x46, K(0x48), "Break"),
    (EXTENDED | 0x47, K(0x4A), "Home"),
    (EXTENDED | 0x48, K(0x52), "Up"),
    (EXTENDED | 0x49, K(0x4B), "Page Up"),
    (EXTENDED | 0x4B, K(0x50), "Left"),
    (EXTENDED | 0x4D, K(0x4F), "Right"),
    (EXTENDED | 0x4F, K(0x4D), "End"),
    (EXTENDED | 0x50, K(0x51), "Down"),
    (EXTENDED | 0x51, K(0x4E), "Page Down"),
    (EXTENDED | 0x52, K(0x49), "Insert"),
    (EXTENDED | 0x53, K(0x4C), "Delete"),
    (EXTENDED | 0x5B, K(0xE3), "Left Win"),
    (EXTENDED | 0x5C, K(0xE7), "Right Win"),
    (EXTENDED | 0x5D, K(0x65), "Menu"),
    (EXTENDED | 0x5E, K(0x66), "Power"),
    (EXTENDED | 0x10, C(0x00B6), "Previous track"),
    (EXTENDED | 0x19, C(0x00B5), "Next track"),
    (EXTENDED | 0x20, C(0x00E2), "Mute"),
    (EXTENDED | 0x21, C(0x0192), "Calculator"),
    (EXTENDED | 0x22, C(0x00CD), "Play/Pause"),
    (EXTENDED | 0x24, C(0x00B7), "Stop"),
    (EXTENDED | 0x2E, C(0x00EA), "Volume down"),
    (EXTENDED | 0x30, C(0x00E9), "Volume up"),
    (EXTENDED | 0x32, C(0x0223), "Browser home"),
    (EXTENDED | 0x65, C(0x0221), "Browser search"),
    (EXTENDED | 0x66, C(0x022A), "Browser favorites"),
    (EXTENDED | 0x67, C(0x0227), "Browser refresh"),
    (EXTENDED | 0x68, C(0x0226), "Browser stop"),
    (EXTENDED | 0x69, C(0x0225), "Browser forward"),
    (EXTENDED | 0x6A, C(0x0224), "Browser back"),
    (EXTENDED | 0x6B, C(0x0194), "My computer"),
    (EXTENDED | 0x6C, C(0x018A), "Mail"),
    (EXTENDED | 0x6D, C(0x0183), "Media select"),
];

pub fn set1_to_usage(scan: u16) -> Option<KeyUsage> {
    return KEYS.iter().find(|&&(code, _, _)| return code == scan).map(|&(_, usage, _)| return usage);
}

pub fn usage_name(usage: KeyUsage) -> Option<&'static str> {
    return KEYS.iter().find(|&&(_, u, _)| return u == usage).map(|&(_, _, name)| return name);
}

pub const USAGE_PAUSE: u8 = 0x48;
pub const USAGE_LEFT_CTRL: u8 = 0xE0;
pub const USAGE_LEFT_ALT: u8 = 0xE2;
pub const USAGE_RIGHT_CTRL: u8 = 0xE4;
pub const USAGE_RIGHT_SHIFT: u8 = 0xE5;
pub const USAGE_RIGHT_ALT: u8 = 0xE6;
pub const USAGE_LEFT_GUI: u8 = 0xE3;
pub const USAGE_RIGHT_GUI: u8 = 0xE7;
pub const USAGE_END: u8 = 0x4D;
pub const USAGE_DELETE: u8 = 0x4C;
pub const USAGE_KEYPAD_DELETE: u8 = 0x63;
pub const USAGE_L: u8 = 0x0F;
pub const USAGE_F24: u8 = 0x73;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits() {
        let letters = [
            (0x1E, 0x04),
            (0x30, 0x05),
            (0x2E, 0x06),
            (0x20, 0x07),
            (0x12, 0x08),
            (0x21, 0x09),
            (0x22, 0x0A),
            (0x23, 0x0B),
            (0x17, 0x0C),
            (0x24, 0x0D),
            (0x25, 0x0E),
            (0x26, 0x0F),
            (0x32, 0x10),
            (0x31, 0x11),
            (0x18, 0x12),
            (0x19, 0x13),
            (0x10, 0x14),
            (0x13, 0x15),
            (0x1F, 0x16),
            (0x14, 0x17),
            (0x16, 0x18),
            (0x2F, 0x19),
            (0x11, 0x1A),
            (0x2D, 0x1B),
            (0x15, 0x1C),
            (0x2C, 0x1D),
        ];
        let mut seen = [false; 26];
        for (scan, usage) in letters {
            assert_eq!(set1_to_usage(scan), Some(KeyUsage::Keyboard(usage)), "scan {scan:#x}");
            seen[(usage - 0x04) as usize] = true;
        }
        assert!(seen.iter().all(|&s| return s));
        for d in 0..10u16 {
            assert_eq!(set1_to_usage(0x02 + d), Some(KeyUsage::Keyboard(0x1E + d as u8)));
        }
    }

    #[test]
    fn function_and_navigation_keys() {
        assert_eq!(set1_to_usage(0x3B), Some(KeyUsage::Keyboard(0x3A)));
        assert_eq!(set1_to_usage(0x44), Some(KeyUsage::Keyboard(0x43)));
        assert_eq!(set1_to_usage(0x57), Some(KeyUsage::Keyboard(0x44)));
        assert_eq!(set1_to_usage(0x58), Some(KeyUsage::Keyboard(0x45)));
        assert_eq!(set1_to_usage(0x6E), Some(KeyUsage::Keyboard(0x72)));
        assert_eq!(set1_to_usage(0x76), Some(KeyUsage::Keyboard(0x73)));
        assert_eq!(set1_to_usage(EXTENDED | 0x4F), Some(KeyUsage::Keyboard(USAGE_END)));
        assert_eq!(set1_to_usage(EXTENDED | 0x53), Some(KeyUsage::Keyboard(USAGE_DELETE)));
        assert_eq!(set1_to_usage(EXTENDED | 0x38), Some(KeyUsage::Keyboard(USAGE_RIGHT_ALT)));
        assert_eq!(set1_to_usage(0x4F), Some(KeyUsage::Keyboard(0x59)));
    }

    #[test]
    fn media_keys_are_consumer_usages() {
        assert_eq!(set1_to_usage(EXTENDED | 0x30), Some(KeyUsage::Consumer(0xE9)));
        assert_eq!(set1_to_usage(EXTENDED | 0x20), Some(KeyUsage::Consumer(0xE2)));
    }

    #[test]
    fn unknown_codes() {
        assert_eq!(set1_to_usage(0x00), None);
        assert_eq!(set1_to_usage(EXTENDED | 0x01), None);
    }
}
