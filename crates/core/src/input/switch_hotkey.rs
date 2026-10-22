use std::time::{Duration, Instant};

use crate::input::keymap::KeyUsage;

pub struct SwitchHotkey {
    key: KeyUsage,
    window: Duration,
    held_since: Option<Instant>,
    clean: bool,
    last_tap: Option<Instant>,
}

impl SwitchHotkey {
    pub fn new(usage: u8) -> Self {
        return SwitchHotkey {
            key: KeyUsage::Keyboard(usage),
            window: Duration::from_millis(400),
            held_since: None,
            clean: true,
            last_tap: None,
        };
    }

    pub fn key(&self) -> KeyUsage {
        return self.key;
    }

    pub fn on_key(&mut self, usage: KeyUsage, down: bool, now: Instant) -> bool {
        if usage != self.key {
            if down {
                self.on_other_input();
            }
            return false;
        }
        if down {
            if self.held_since.is_none() {
                self.held_since = Some(now);
                self.clean = true;
            }
            return false;
        }
        let Some(pressed_at) = self.held_since.take() else {
            return false;
        };
        let is_tap = self.clean && now.duration_since(pressed_at) <= self.window;
        if !is_tap {
            self.last_tap = None;
            return false;
        }
        if self.last_tap.is_some_and(|t| return now.duration_since(t) <= self.window) {
            self.last_tap = None;
            return true;
        }
        self.last_tap = Some(now);
        return false;
    }

    pub fn on_other_input(&mut self) {
        self.clean = false;
        self.last_tap = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::keymap::USAGE_RIGHT_CTRL;

    const RCTRL: KeyUsage = KeyUsage::Keyboard(USAGE_RIGHT_CTRL);
    const C: KeyUsage = KeyUsage::Keyboard(0x06);

    fn at(base: Instant, ms: u64) -> Instant {
        return base + Duration::from_millis(ms);
    }

    #[test]
    fn double_tap_fires_on_second_release() {
        let t = Instant::now();
        let mut d = SwitchHotkey::new(USAGE_RIGHT_CTRL);
        assert!(!d.on_key(RCTRL, true, at(t, 0)));
        assert!(!d.on_key(RCTRL, false, at(t, 60)));
        assert!(!d.on_key(RCTRL, true, at(t, 150)));
        assert!(!d.on_key(RCTRL, true, at(t, 180)));
        assert!(d.on_key(RCTRL, false, at(t, 210)));
        assert!(!d.on_key(RCTRL, true, at(t, 300)));
        assert!(!d.on_key(RCTRL, false, at(t, 350)));
    }

    #[test]
    fn other_hotkey_key() {
        let t = Instant::now();
        let rshift = KeyUsage::Keyboard(crate::input::keymap::USAGE_RIGHT_SHIFT);
        let mut d = SwitchHotkey::new(crate::input::keymap::USAGE_RIGHT_SHIFT);
        for (ms, down) in [(0, true), (50, false), (100, true), (150, false)] {
            assert!(!d.on_key(RCTRL, down, at(t, ms)));
        }
        assert!(!d.on_key(rshift, true, at(t, 400)));
        assert!(!d.on_key(rshift, false, at(t, 450)));
        assert!(!d.on_key(rshift, true, at(t, 500)));
        assert!(d.on_key(rshift, false, at(t, 550)));
    }

    #[test]
    fn slow_taps_do_not_fire() {
        let t = Instant::now();
        let mut d = SwitchHotkey::new(USAGE_RIGHT_CTRL);
        d.on_key(RCTRL, true, at(t, 0));
        d.on_key(RCTRL, false, at(t, 50));
        d.on_key(RCTRL, true, at(t, 900));
        assert!(!d.on_key(RCTRL, false, at(t, 950)));
    }

    #[test]
    fn combos_and_long_presses_are_not_taps() {
        let t = Instant::now();
        let mut d = SwitchHotkey::new(USAGE_RIGHT_CTRL);
        d.on_key(RCTRL, true, at(t, 0));
        d.on_key(C, true, at(t, 20));
        d.on_key(C, false, at(t, 40));
        d.on_key(RCTRL, false, at(t, 60));
        d.on_key(RCTRL, true, at(t, 100));
        assert!(!d.on_key(RCTRL, false, at(t, 150)));
        let mut d = SwitchHotkey::new(USAGE_RIGHT_CTRL);
        d.on_key(RCTRL, true, at(t, 0));
        d.on_key(RCTRL, false, at(t, 50));
        d.on_key(RCTRL, true, at(t, 100));
        assert!(!d.on_key(RCTRL, false, at(t, 700)));
        let mut d = SwitchHotkey::new(USAGE_RIGHT_CTRL);
        d.on_key(RCTRL, true, at(t, 0));
        d.on_key(RCTRL, false, at(t, 50));
        d.on_other_input();
        d.on_key(RCTRL, true, at(t, 100));
        assert!(!d.on_key(RCTRL, false, at(t, 150)));
    }
}
