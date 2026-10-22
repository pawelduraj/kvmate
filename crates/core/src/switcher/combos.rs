use std::time::{Duration, Instant};

use kvmate_protocol::{KeyboardState, Msg};
use serde::Deserialize;

use super::{Switcher, Target};
use crate::board::link::BoardSink;
use crate::input::keymap::{
    USAGE_DELETE, USAGE_F24, USAGE_KEYPAD_DELETE, USAGE_L, USAGE_LEFT_ALT, USAGE_LEFT_CTRL, USAGE_LEFT_GUI,
    USAGE_RIGHT_ALT, USAGE_RIGHT_CTRL, USAGE_RIGHT_GUI,
};

pub(super) const RELEASE_DELAY: Duration = Duration::from_millis(40);
pub(super) const MASK_KEY: u8 = USAGE_F24;

pub(super) const SECURE_ATTENTION_NOTE: &str = "Ctrl+Alt+Del is always handled by this computer (Windows doesn't let it be \
     captured) — on the other one press Ctrl+Alt+End or choose it in the tray menu";
pub(super) const LOCK_NOTE: &str = "Win+L always locks this computer (Windows doesn't let it be captured) — the keyboard \
     and mouse are back here; lock the other computer from the tray menu";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Combo {
    CtrlAltDel,
    Lock,
}

impl Combo {
    fn usages(self) -> &'static [u8] {
        match self {
            Combo::CtrlAltDel => return &[USAGE_LEFT_CTRL, USAGE_LEFT_ALT, USAGE_DELETE],
            Combo::Lock => return &[USAGE_LEFT_GUI, USAGE_L],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalCombo {
    SecureAttention,
    Lock,
}

pub(super) fn local_combo(keys: &KeyboardState, usage: u8) -> Option<LocalCombo> {
    let ctrl = keys.is_pressed(USAGE_LEFT_CTRL) || keys.is_pressed(USAGE_RIGHT_CTRL);
    let alt = keys.is_pressed(USAGE_LEFT_ALT) || keys.is_pressed(USAGE_RIGHT_ALT);
    let win = keys.is_pressed(USAGE_LEFT_GUI) || keys.is_pressed(USAGE_RIGHT_GUI);
    match usage {
        USAGE_DELETE | USAGE_KEYPAD_DELETE if ctrl && alt => return Some(LocalCombo::SecureAttention),
        USAGE_L if win => return Some(LocalCombo::Lock),
        _ => return None,
    }
}

impl Switcher {
    pub(super) fn on_local_combo(&mut self, combo: LocalCombo, board: &dyn BoardSink) -> Option<Target> {
        let mut masked = self.effective_keys();
        masked.set(MASK_KEY, true);
        board.send(Msg::KeyboardState(masked));
        self.hid.keys = KeyboardState::default();
        self.release_at = Some(Instant::now() + RELEASE_DELAY);
        match combo {
            LocalCombo::SecureAttention => {
                self.notice = Some(SECURE_ATTENTION_NOTE);
                return None;
            }
            LocalCombo::Lock => {
                self.notice = Some(LOCK_NOTE);
                return self.set_target(Target::Pc, Some(board));
            }
        }
    }

    pub fn send_combo(&mut self, combo: Combo, board: Option<&dyn BoardSink>) {
        let Some(board) = board.filter(|_| return self.board_ready && self.laptop_usb) else {
            self.notice = Some("the other computer hasn't detected the board: can't send the shortcut");
            return;
        };
        let mut keys = self.effective_keys();
        for &usage in combo.usages() {
            keys.set(usage, true);
        }
        board.send(Msg::KeyboardState(keys));
        self.release_at = Some(Instant::now() + RELEASE_DELAY);
    }
}
