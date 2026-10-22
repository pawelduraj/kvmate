use std::time::{Duration, Instant};

use kvmate_protocol::{ConsumerState, KeyboardState, MouseReport, Msg};

use super::combos::local_combo;
use super::{Switcher, Target};
use crate::board::link::BoardSink;
use crate::input::InputEvent;
use crate::input::keymap::{
    KeyUsage, USAGE_DELETE, USAGE_END, USAGE_LEFT_ALT, USAGE_LEFT_CTRL, USAGE_RIGHT_ALT, USAGE_RIGHT_CTRL,
};

pub(super) const MOUSE_INTERVAL: Duration = Duration::from_millis(1);
const WHEEL_NOTCH: i32 = 120;
const BUTTONS_REPEAT_FOR: Duration = Duration::from_millis(600);

#[derive(Default)]
pub(super) struct HidState {
    pub(super) keys: KeyboardState,
    consumer: u16,
    buttons: u8,
    sent_buttons: u8,
    buttons_sent_at: Option<Instant>,
    dx: i32,
    dy: i32,
    wheel: i32,
    hwheel: i32,
    pub(super) mouse_dirty: bool,
}

impl Switcher {
    pub fn handle_input(&mut self, ev: InputEvent, board: Option<&dyn BoardSink>) -> Option<Target> {
        if self.target != Target::Laptop {
            return None;
        }
        let board = board?;
        let hid = &mut self.hid;
        match ev {
            InputEvent::Key { usage: KeyUsage::Keyboard(usage), down } => return self.key(usage, down, board),
            InputEvent::Key { usage: KeyUsage::Consumer(usage), down } => {
                let next = if down {
                    usage
                } else if hid.consumer == usage {
                    0
                } else {
                    hid.consumer
                };
                if next != hid.consumer {
                    hid.consumer = next;
                    board.send(Msg::ConsumerState(ConsumerState { usage: next }));
                }
                return None;
            }
            InputEvent::MouseMove { dx, dy } => {
                hid.dx += dx;
                hid.dy += dy;
            }
            InputEvent::MouseButton { button, down } => {
                let bit = 1u8 << button.min(7);
                hid.buttons = if down { hid.buttons | bit } else { hid.buttons & !bit };
            }
            InputEvent::Wheel { delta } => hid.wheel += delta,
            InputEvent::HWheel { delta } => hid.hwheel += delta,
            InputEvent::Hotkey { .. } => return None,
        }
        hid.mouse_dirty = true;
        if !matches!(ev, InputEvent::MouseMove { .. }) || self.last_mouse_send.elapsed() >= MOUSE_INTERVAL {
            self.flush_mouse(board);
        }
        return None;
    }

    fn key(&mut self, usage: u8, down: bool, board: &dyn BoardSink) -> Option<Target> {
        if let Some(combo) = local_combo(&self.hid.keys, usage).filter(|_| return down && self.local_combos) {
            return self.on_local_combo(combo, board);
        }
        let before = self.effective_keys();
        self.hid.keys.set(usage, down);
        let after = self.effective_keys();
        if after != before {
            board.send(Msg::KeyboardState(after));
        }
        return None;
    }

    pub(super) fn flush_mouse(&mut self, board: &dyn BoardSink) {
        let hid = &mut self.hid;
        if !hid.mouse_dirty {
            return;
        }
        let take = |acc: &mut i32, max: i32| {
            let v = (*acc).clamp(-max, max);
            *acc -= v;
            return v;
        };
        let notches = |acc: &mut i32| {
            let n = (*acc / WHEEL_NOTCH).clamp(-127, 127);
            *acc -= n * WHEEL_NOTCH;
            return n as i8;
        };
        let report = MouseReport {
            buttons: hid.buttons,
            move_x: take(&mut hid.dx, i16::MAX as i32) as i16,
            move_y: take(&mut hid.dy, i16::MAX as i32) as i16,
            wheel: notches(&mut hid.wheel),
            wheel_horizontal: notches(&mut hid.hwheel),
        };
        hid.mouse_dirty = hid.dx != 0 || hid.dy != 0;
        let empty = report.move_x == 0 && report.move_y == 0 && report.wheel == 0 && report.wheel_horizontal == 0;
        if empty && report.buttons == hid.sent_buttons {
            return;
        }
        if report.buttons != hid.sent_buttons {
            hid.buttons_sent_at = Some(Instant::now());
        }
        hid.sent_buttons = report.buttons;
        self.last_mouse_send = Instant::now();
        board.send(Msg::MouseReport(report));
    }

    pub(super) fn resend_state(&self, board: &dyn BoardSink) {
        board.send(Msg::KeyboardState(self.effective_keys()));
        board.send(Msg::ConsumerState(ConsumerState { usage: self.hid.consumer }));
        if self.hid.buttons_sent_at.is_some_and(|t| return t.elapsed() < BUTTONS_REPEAT_FOR) {
            let report =
                MouseReport { buttons: self.hid.sent_buttons, move_x: 0, move_y: 0, wheel: 0, wheel_horizontal: 0 };
            board.send(Msg::MouseReport(report));
        }
    }

    pub(super) fn effective_keys(&self) -> KeyboardState {
        let mut k = self.hid.keys;
        let ctrl = k.is_pressed(USAGE_LEFT_CTRL) || k.is_pressed(USAGE_RIGHT_CTRL);
        let alt = k.is_pressed(USAGE_LEFT_ALT) || k.is_pressed(USAGE_RIGHT_ALT);
        if self.ctrl_alt_end_as_delete && ctrl && alt && k.is_pressed(USAGE_END) {
            k.set(USAGE_END, false);
            k.set(USAGE_DELETE, true);
        }
        return k;
    }
}
