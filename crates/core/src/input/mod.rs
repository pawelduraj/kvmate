//! Capturing the hub's keyboard and mouse. The platform layer reports raw events and,
//! while forwarding to the controlled computer, hides them from the local system.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::input::keymap::KeyUsage;

pub(crate) mod keymap;
mod switch_hotkey;
pub use switch_hotkey::SwitchHotkey;

pub use crate::platform::input::{Capture, SYSTEM_KEEPS_COMBOS, start_capture};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key { usage: KeyUsage, down: bool },
    MouseMove { dx: i32, dy: i32 },
    MouseButton { button: u8, down: bool },
    Wheel { delta: i32 },
    HWheel { delta: i32 },
    Hotkey { back_here: bool },
}

pub type InputSink = Arc<dyn Fn(InputEvent) + Send + Sync>;

pub struct CaptureControl {
    forwarding: AtomicBool,
    pub(crate) suppress: bool,
    hotkey: AtomicU8,
}

impl CaptureControl {
    pub fn new(suppress: bool, hotkey: u8) -> Arc<Self> {
        return Arc::new(CaptureControl {
            forwarding: AtomicBool::new(false),
            suppress,
            hotkey: AtomicU8::new(hotkey),
        });
    }

    pub fn set_hotkey(&self, usage: u8) {
        self.hotkey.store(usage, Ordering::Relaxed);
    }

    pub fn hotkey(&self) -> u8 {
        return self.hotkey.load(Ordering::Relaxed);
    }

    pub fn forwarding(&self) -> bool {
        return self.forwarding.load(Ordering::Relaxed);
    }

    pub(crate) fn set_forwarding(&self, on: bool) {
        self.forwarding.store(on, Ordering::Relaxed);
    }
}
