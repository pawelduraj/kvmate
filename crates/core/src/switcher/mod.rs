//! Which computer gets the keyboard and mouse. Platform-independent; the capture layer only
//! follows `CaptureControl`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kvmate_protocol::{self as proto, ButtonEvent, DeviceConfig, MouseReport, Msg, SetLed};
use serde::{Deserialize, Serialize};

use crate::board::link::BoardSink;
use crate::config::Config;
use crate::input::{CaptureControl, SYSTEM_KEEPS_COMBOS};

mod combos;
mod hid;
#[cfg(test)]
mod tests;

pub use combos::Combo;
#[cfg(test)]
use combos::{LOCK_NOTE, SECURE_ATTENTION_NOTE};
use hid::{HidState, MOUSE_INTERVAL};

const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(300);
const JIGGLE_INTERVAL: Duration = Duration::from_secs(30);
const STATE_REFRESH: Duration = Duration::from_millis(200);
const RELEASE_REPEAT_FOR: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    #[default]
    Pc,
    Laptop,
}

impl Target {
    pub fn other(self) -> Target {
        match self {
            Target::Pc => return Target::Laptop,
            Target::Laptop => return Target::Pc,
        }
    }
}

pub type LoopbackCheck = Box<dyn Fn() -> bool + Send>;

pub(crate) const LOOPBACK_REFUSAL: &str =
    "the board's “USB” port is connected to this computer, not the other one: switching would loop the mouse";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Toggle,
    Set(Target),
}

pub struct Switcher {
    control: Arc<CaptureControl>,
    ctrl_alt_end_as_delete: bool,
    led_gpio: u8,
    led_brightness: u8,
    usb_names: proto::UsbNames,
    target: Target,
    board_ready: bool,
    laptop_usb: bool,
    notice: Option<&'static str>,
    loopback: Option<LoopbackCheck>,
    hid: HidState,
    last_mouse_send: Instant,
    last_switch: Option<Instant>,
    last_refresh: Instant,
    jiggle: bool,
    last_jiggle: Instant,
    local_combos: bool,
    release_at: Option<Instant>,
}

impl Switcher {
    pub fn new(control: Arc<CaptureControl>, config: &Config) -> Switcher {
        control.set_forwarding(false);
        return Switcher {
            control,
            ctrl_alt_end_as_delete: config.ctrl_alt_end_as_delete,
            led_gpio: config.led_gpio,
            led_brightness: config.led_level(),
            usb_names: config.usb.message(),
            target: Target::Pc,
            board_ready: false,
            laptop_usb: false,
            notice: None,
            loopback: None,
            hid: HidState::default(),
            last_mouse_send: Instant::now(),
            last_switch: None,
            last_refresh: Instant::now(),
            jiggle: config.jiggle,
            last_jiggle: Instant::now(),
            local_combos: SYSTEM_KEEPS_COMBOS,
            release_at: None,
        };
    }

    pub fn target(&self) -> Target {
        return self.target;
    }

    pub fn apply_settings(&mut self, config: &Config, board: Option<&dyn BoardSink>) {
        self.ctrl_alt_end_as_delete = config.ctrl_alt_end_as_delete;
        self.jiggle = config.jiggle;
        let led_changed = (self.led_gpio, self.led_brightness) != (config.led_gpio, config.led_level());
        self.led_gpio = config.led_gpio;
        self.led_brightness = config.led_level();
        let usb_names = config.usb.message();
        let usb_changed = usb_names != self.usb_names;
        self.usb_names = usb_names;
        let Some(board) = board.filter(|_| return self.board_ready) else { return };
        if led_changed {
            self.send_setup(board);
        } else if usb_changed {
            board.send(Msg::UsbNames(self.usb_names));
        }
    }

    pub fn on_board_ready(&mut self, board: &dyn BoardSink) {
        self.board_ready = true;
        self.send_setup(board);
    }

    fn send_setup(&self, board: &dyn BoardSink) {
        let config = DeviceConfig { led_pin: self.led_gpio, led_brightness: self.led_brightness };
        board.send(Msg::DeviceConfig(config));
        board.send(Msg::UsbNames(self.usb_names));
        self.show_target(board);
    }

    pub fn on_board_lost(&mut self) -> Option<Target> {
        self.board_ready = false;
        self.laptop_usb = false;
        self.release_at = None;
        if self.target == Target::Laptop {
            self.target = Target::Pc;
            self.control.set_forwarding(false);
            self.hid = HidState::default();
            return Some(Target::Pc);
        }
        return None;
    }

    pub fn on_usb(&mut self, mounted: bool) {
        self.laptop_usb = mounted;
    }

    pub fn set_loopback_check(&mut self, check: LoopbackCheck) {
        self.loopback = Some(check);
    }

    pub fn board_usb_on_this_pc(&self) -> bool {
        return self.loopback.as_ref().is_some_and(|on_this_pc| return on_this_pc());
    }

    pub fn take_notice(&mut self) -> Option<&'static str> {
        return self.notice.take();
    }

    pub fn on_button(&mut self, ev: ButtonEvent, board: Option<&dyn BoardSink>) -> Option<Target> {
        match ev.kind {
            proto::BUTTON_EVENT_SHORT => return self.request(Request::Toggle, board),
            proto::BUTTON_EVENT_LONG => return self.request(Request::Set(Target::Pc), board),
            _ => return None,
        }
    }

    pub fn request(&mut self, req: Request, board: Option<&dyn BoardSink>) -> Option<Target> {
        let wanted = match req {
            Request::Toggle if self.last_switch.is_some_and(|t| return t.elapsed() < TOGGLE_DEBOUNCE) => return None,
            Request::Toggle => self.target.other(),
            Request::Set(t) => t,
        };
        return self.set_target(wanted, board);
    }

    fn set_target(&mut self, wanted: Target, board: Option<&dyn BoardSink>) -> Option<Target> {
        if wanted == self.target {
            return None;
        }
        if wanted == Target::Laptop {
            if !(self.board_ready && board.is_some()) {
                self.notice = Some("no board: can't switch to the other computer");
                return None;
            }
            if let Some(why) = self.laptop_refusal() {
                self.notice = Some(why);
                return None;
            }
        }
        if self.target == Target::Laptop && self.release_at.is_none() {
            if let Some(board) = board {
                board.send(Msg::ReleaseAll);
            }
        }
        self.hid = HidState::default();
        self.target = wanted;
        let now = Instant::now();
        self.last_switch = Some(now);
        self.last_refresh = now;
        self.last_jiggle = now;
        self.control.set_forwarding(wanted == Target::Laptop);
        if let Some(board) = board {
            self.show_target(board);
        }
        return Some(wanted);
    }

    fn laptop_refusal(&self) -> Option<&'static str> {
        if !self.laptop_usb {
            return Some("the other computer hasn't detected the board (“USB” port): the keyboard and mouse stay here");
        } else if self.board_usb_on_this_pc() {
            return Some(LOOPBACK_REFUSAL);
        } else {
            return None;
        }
    }

    fn show_target(&self, board: &dyn BoardSink) {
        let led = match self.target {
            Target::Pc => SetLed { red: 0, green: 90, blue: 255 },
            Target::Laptop => SetLed { red: 0, green: 255, blue: 70 },
        };
        board.send(Msg::SetLed(led));
    }

    pub fn needs_tick(&self) -> bool {
        return self.hid.mouse_dirty || self.release_at.is_some();
    }

    pub fn tick(&mut self, board: Option<&dyn BoardSink>) {
        if let Some(board) = board {
            if self.hid.mouse_dirty && self.last_mouse_send.elapsed() >= MOUSE_INTERVAL {
                self.flush_mouse(board);
            }
            if self.release_at.is_some_and(|t| return Instant::now() >= t) {
                self.release_at = None;
                if self.target == Target::Laptop {
                    board.send(Msg::KeyboardState(self.effective_keys()));
                } else {
                    board.send(Msg::ReleaseAll);
                }
            }
            if self.release_at.is_none() && self.last_refresh.elapsed() >= STATE_REFRESH {
                if self.target == Target::Laptop {
                    self.last_refresh = Instant::now();
                    self.resend_state(board);
                } else if self.last_switch.is_some_and(|t| return t.elapsed() < RELEASE_REPEAT_FOR) {
                    self.last_refresh = Instant::now();
                    board.send(Msg::ReleaseAll);
                }
            }
            self.maybe_jiggle(Instant::now(), board);
        }
    }

    fn maybe_jiggle(&mut self, now: Instant, board: &dyn BoardSink) {
        if !self.jiggle || self.target != Target::Pc || now.duration_since(self.last_jiggle) < JIGGLE_INTERVAL {
            return;
        }
        self.last_jiggle = now;
        let report = MouseReport { buttons: 0, move_x: 0, move_y: 0, wheel: 0, wheel_horizontal: 0 };
        board.send(Msg::MouseReport(report));
    }
}
