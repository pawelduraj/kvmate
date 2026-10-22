use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_LCONTROL, VK_PAUSE, VK_RSHIFT};
use windows_sys::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, LLKHF_EXTENDED};

use crate::input::keymap::{self, EXTENDED, KeyUsage};

const FAKE_ALTGR_CTRL_SCAN: u32 = 0x21D;

pub(super) fn key_usage(kb: &KBDLLHOOKSTRUCT) -> Option<KeyUsage> {
    if kb.vkCode == VK_LCONTROL as u32 && kb.scanCode == FAKE_ALTGR_CTRL_SCAN {
        return None;
    }
    if kb.vkCode == VK_PAUSE as u32 {
        return Some(KeyUsage::Keyboard(keymap::USAGE_PAUSE));
    }
    if let Some(usage) = media_vk_usage(kb.vkCode) {
        return Some(KeyUsage::Consumer(usage));
    }
    let extended = kb.flags & LLKHF_EXTENDED != 0 && kb.vkCode != VK_RSHIFT as u32;
    return keymap::set1_to_usage((kb.scanCode & 0xFF) as u16 | if extended { EXTENDED } else { 0 });
}

const MEDIA_KEYS: &[(u32, u16, &str)] = &[
    (0xA6, 0x0224, "Browser back"),
    (0xA7, 0x0225, "Browser forward"),
    (0xA8, 0x0227, "Browser refresh"),
    (0xA9, 0x0226, "Browser stop"),
    (0xAA, 0x0221, "Browser search"),
    (0xAB, 0x022A, "Browser favorites"),
    (0xAC, 0x0223, "Browser home"),
    (0xAD, 0x00E2, "Mute"),
    (0xAE, 0x00EA, "Volume down"),
    (0xAF, 0x00E9, "Volume up"),
    (0xB0, 0x00B5, "Next track"),
    (0xB1, 0x00B6, "Previous track"),
    (0xB2, 0x00B7, "Stop"),
    (0xB3, 0x00CD, "Play/Pause"),
    (0xB4, 0x018A, "Mail"),
    (0xB5, 0x0183, "Media select"),
    (0xB6, 0x0194, "My computer"),
    (0xB7, 0x0192, "Calculator"),
];

fn media_vk_usage(vk: u32) -> Option<u16> {
    return MEDIA_KEYS.iter().find(|&&(code, _, _)| return code == vk).map(|&(_, usage, _)| return usage);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keymap::{USAGE_PAUSE, USAGE_RIGHT_SHIFT};

    fn key(vk: u32, scan: u32, extended: bool) -> Option<KeyUsage> {
        let flags = if extended { LLKHF_EXTENDED } else { 0 };
        return key_usage(&KBDLLHOOKSTRUCT { vkCode: vk, scanCode: scan, flags, time: 0, dwExtraInfo: 0 });
    }

    #[test]
    fn keys_windows_reports_its_own_way() {
        assert_eq!(key(0x41, 0x1E, false), Some(KeyUsage::Keyboard(0x04)), "A");
        assert_eq!(key(VK_LCONTROL as u32, FAKE_ALTGR_CTRL_SCAN, false), None, "AltGr's Left Ctrl");
        assert_eq!(key(VK_RSHIFT as u32, 0x36, true), Some(KeyUsage::Keyboard(USAGE_RIGHT_SHIFT)));
        assert_eq!(key(VK_PAUSE as u32, 0x45, false), Some(KeyUsage::Keyboard(USAGE_PAUSE)));
        assert_eq!(key(0x90, 0x45, true), Some(KeyUsage::Keyboard(0x53)), "Num Lock");
        assert_eq!(key(0xAF, 0, false), Some(KeyUsage::Consumer(0x00E9)), "volume up without a scan code");
    }
}
