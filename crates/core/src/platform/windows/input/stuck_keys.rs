//! Modifiers left pressed on this computer while input was hidden, and letting go of them.

use std::mem::size_of;
use std::sync::atomic::{AtomicU8, Ordering};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN,
};

use super::Capture;

pub(super) const INJECTED_TAG: usize = 0x6B76_6D74;
const MASK_VK: VIRTUAL_KEY = 0xE8;
const MODIFIERS: [(VIRTUAL_KEY, &str); 8] = [
    (VK_LCONTROL, "Left Ctrl"),
    (VK_RCONTROL, "Right Ctrl"),
    (VK_LSHIFT, "Left Shift"),
    (VK_RSHIFT, "Right Shift"),
    (VK_LMENU, "Left Alt"),
    (VK_RMENU, "Right Alt"),
    (VK_LWIN, "Left Win"),
    (VK_RWIN, "Right Win"),
];

impl Capture {
    pub fn release_stuck_keys(&self) -> Result<Vec<&'static str>, Vec<&'static str>> {
        // SAFETY: plain Win32 call.
        let stuck = stuck_modifiers(&self.held, |vk| return unsafe { GetAsyncKeyState(vk as i32) } < 0);
        if stuck.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<VIRTUAL_KEY> = stuck.iter().map(|&(vk, _)| return vk).collect();
        let inputs: Vec<INPUT> =
            release_sequence(&keys).into_iter().map(|(vk, down)| return key_input(vk, down)).collect();
        // SAFETY: fully initialised INPUTs; count and size match the slice.
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        let names = stuck.into_iter().map(|(_, name)| return name).collect();
        if sent as usize == inputs.len() { return Ok(names) } else { return Err(names) }
    }
}

pub(super) fn modifier_bit(vk: u32) -> Option<u8> {
    return MODIFIERS.iter().position(|&(m, _)| return m as u32 == vk).map(|i| return 1 << i);
}

fn stuck_modifiers(held: &AtomicU8, windows_down: impl Fn(VIRTUAL_KEY) -> bool) -> Vec<(VIRTUAL_KEY, &'static str)> {
    return MODIFIERS
        .iter()
        .enumerate()
        .filter(|&(i, &(vk, _))| return windows_down(vk) && held.load(Ordering::Relaxed) & (1 << i) == 0)
        .map(|(_, &m)| return m)
        .collect();
}

fn release_sequence(keys: &[VIRTUAL_KEY]) -> Vec<(VIRTUAL_KEY, bool)> {
    let mut seq = Vec::new();
    if keys.iter().any(|&vk| matches!(vk, VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN)) {
        seq.extend([(MASK_VK, true), (MASK_VK, false)]);
    }
    seq.extend(keys.iter().map(|&vk| return (vk, false)));
    return seq;
}

pub(super) fn key_input(vk: VIRTUAL_KEY, down: bool) -> INPUT {
    // SAFETY: plain Win32 call.
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    if matches!(vk, VK_RCONTROL | VK_RMENU | VK_LWIN | VK_RWIN) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    return INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: INJECTED_TAG },
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuck_modifiers_skip_keys_the_hook_saw_pressed() {
        let held = AtomicU8::new(modifier_bit(VK_RCONTROL as u32).unwrap());
        let windows = [VK_LCONTROL, VK_RCONTROL, VK_LMENU];
        let stuck = stuck_modifiers(&held, |vk| return windows.contains(&vk));
        assert_eq!(stuck, [(VK_LCONTROL, "Left Ctrl"), (VK_LMENU, "Left Alt")]);
        assert!(stuck_modifiers(&AtomicU8::new(0), |_| return false).is_empty());
    }

    #[test]
    fn lone_alt_or_win_release_is_masked() {
        assert_eq!(release_sequence(&[VK_LCONTROL, VK_RSHIFT]), [(VK_LCONTROL, false), (VK_RSHIFT, false)]);
        assert_eq!(
            release_sequence(&[VK_LCONTROL, VK_RWIN]),
            [(MASK_VK, true), (MASK_VK, false), (VK_LCONTROL, false), (VK_RWIN, false)]
        );
    }
}
