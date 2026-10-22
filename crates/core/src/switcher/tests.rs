use super::combos::{MASK_KEY, RELEASE_DELAY};
use super::*;
use crate::input::InputEvent;
use crate::input::keymap::{
    KeyUsage, USAGE_DELETE, USAGE_END, USAGE_L, USAGE_LEFT_ALT, USAGE_LEFT_CTRL, USAGE_LEFT_GUI,
};
use kvmate_protocol::{KeyboardState, MsgType, RawData};
use std::cell::RefCell;

#[derive(Default)]
struct Recorder(RefCell<Vec<(MsgType, Vec<u8>)>>);

impl BoardSink for Recorder {
    fn send_raw(&self, msg_type: MsgType, payload: Vec<u8>) {
        self.0.borrow_mut().push((msg_type, payload));
    }
}

impl Recorder {
    fn take(&self) -> Vec<(MsgType, Vec<u8>)> {
        return std::mem::take(&mut self.0.borrow_mut());
    }
    fn of<T: RawData>(&self, t: MsgType) -> Vec<T> {
        return self
            .0
            .borrow()
            .iter()
            .filter(|(m, _)| return *m == t)
            .filter_map(|(_, p)| return T::read_from(p))
            .collect();
    }
}

fn key(usage: u8, down: bool) -> InputEvent {
    return InputEvent::Key { usage: KeyUsage::Keyboard(usage), down };
}

fn ready() -> (Switcher, Arc<CaptureControl>, Recorder) {
    let control = CaptureControl::new(true, crate::input::keymap::USAGE_RIGHT_CTRL);
    let mut sw = Switcher::new(control.clone(), &Config::default());
    let rec = Recorder::default();
    sw.on_board_ready(&rec);
    sw.on_usb(true);
    rec.take();
    return (sw, control, rec);
}

#[test]
fn new_led_settings_reach_the_board() {
    let (mut sw, _, rec) = ready();
    let config = Config { led_brightness: 200, ctrl_alt_end_as_delete: false, ..Config::default() };
    sw.apply_settings(&config, Some(&rec));
    let sent = rec.of::<DeviceConfig>(MsgType::DeviceConfig);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].led_brightness, 200);
    assert!(!sw.ctrl_alt_end_as_delete);
    rec.take();
    sw.apply_settings(&config, Some(&rec));
    assert!(rec.take().is_empty());
    let off = Config { led_on: false, ..config.clone() };
    sw.apply_settings(&off, Some(&rec));
    assert_eq!(rec.of::<DeviceConfig>(MsgType::DeviceConfig)[0].led_brightness, 0);
    rec.take();
    sw.apply_settings(&config, Some(&rec));
    assert_eq!(rec.of::<DeviceConfig>(MsgType::DeviceConfig)[0].led_brightness, 200);
}

#[test]
fn usb_names_reach_the_board() {
    use crate::config::UsbNames;
    let control = CaptureControl::new(true, crate::input::keymap::USAGE_RIGHT_CTRL);
    let mut sw = Switcher::new(control, &Config::default());
    let rec = Recorder::default();
    sw.on_board_ready(&rec);
    let sent = rec.of::<proto::UsbNames>(MsgType::UsbNames);
    assert_eq!(sent.len(), 1, "after every handshake");
    assert_eq!(proto::name_bytes(&sent[0].product, sent[0].product_len), b"kvmate");
    rec.take();

    let usb = UsbNames { name: "USB Headset".into(), ..UsbNames::default() };
    let config = Config { usb, ..Config::default() };
    sw.apply_settings(&config, Some(&rec));
    let sent = rec.of::<proto::UsbNames>(MsgType::UsbNames);
    assert_eq!(sent.len(), 1);
    assert_eq!(proto::name_bytes(&sent[0].product, sent[0].product_len), b"USB Headset");
    assert_eq!(rec.of::<DeviceConfig>(MsgType::DeviceConfig).len(), 0, "only the names");
    rec.take();
    sw.apply_settings(&config, Some(&rec));
    assert!(rec.take().is_empty(), "unchanged: nothing to send");
}

#[test]
fn cannot_switch_without_board() {
    let control = CaptureControl::new(true, crate::input::keymap::USAGE_RIGHT_CTRL);
    let mut sw = Switcher::new(control.clone(), &Config::default());
    assert_eq!(sw.request(Request::Toggle, None), None);
    assert_eq!(sw.target(), Target::Pc);
    assert!(!control.forwarding());
    assert!(sw.take_notice().is_some(), "a refused switch says why");
}

#[test]
fn cannot_switch_until_laptop_sees_the_board() {
    let (mut sw, control, rec) = ready();
    sw.on_usb(false);
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), None);
    assert!(!control.forwarding());
    assert!(sw.take_notice().is_some());
    assert!(sw.take_notice().is_none());

    sw.on_usb(true);
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), Some(Target::Laptop));
    sw.on_usb(false);
    assert_eq!(sw.target(), Target::Laptop);
    assert_eq!(sw.request(Request::Set(Target::Pc), Some(&rec)), Some(Target::Pc));
}

#[test]
fn cannot_switch_when_board_usb_is_on_this_pc() {
    let (mut sw, control, rec) = ready();
    sw.set_loopback_check(Box::new(|| return true));
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), None);
    assert!(!control.forwarding());
    assert_eq!(sw.take_notice(), Some(LOOPBACK_REFUSAL));
    sw.set_loopback_check(Box::new(|| return false));
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), Some(Target::Laptop));
}

fn jiggling() -> (Switcher, Recorder) {
    let config = Config { jiggle: true, ..Config::default() };
    let mut sw = Switcher::new(CaptureControl::new(true, crate::input::keymap::USAGE_RIGHT_CTRL), &config);
    let rec = Recorder::default();
    sw.on_board_ready(&rec);
    sw.on_usb(true);
    rec.take();
    return (sw, rec);
}

#[test]
fn jiggle_nudges_laptop_when_idle_on_pc() {
    let (mut sw, rec) = jiggling();
    let t0 = Instant::now();
    sw.maybe_jiggle(t0, &rec);
    assert!(rec.take().is_empty(), "not due yet");
    sw.maybe_jiggle(t0 + JIGGLE_INTERVAL + Duration::from_millis(1), &rec);
    let reports = rec.of::<MouseReport>(MsgType::MouseReport);
    assert_eq!(reports.len(), 1);
    let r = reports[0];
    assert_eq!((r.move_x, r.move_y, r.wheel, r.wheel_horizontal, r.buttons), (0, 0, 0, 0, 0), "0 px, no buttons");
}

#[test]
fn no_jiggle_while_controlling_laptop_or_when_off() {
    let (mut sw, rec) = jiggling();
    sw.request(Request::Toggle, Some(&rec));
    rec.take();
    sw.maybe_jiggle(Instant::now() + JIGGLE_INTERVAL * 2, &rec);
    assert!(rec.take().is_empty(), "no jiggle while using the laptop");

    let (mut off, _c, rec) = ready();
    off.maybe_jiggle(Instant::now() + JIGGLE_INTERVAL * 2, &rec);
    assert!(rec.take().is_empty(), "no jiggle when disabled");
}

#[test]
fn toggle_forwards_keys_and_release_on_return() {
    let (mut sw, control, rec) = ready();
    sw.handle_input(key(0x04, true), Some(&rec));
    assert!(rec.take().is_empty());

    assert_eq!(sw.request(Request::Toggle, Some(&rec)), Some(Target::Laptop));
    assert!(control.forwarding());
    assert_eq!(rec.of::<SetLed>(MsgType::SetLed).len(), 1);
    rec.take();

    sw.handle_input(key(0xE1, true), Some(&rec));
    sw.handle_input(key(0x04, true), Some(&rec));
    sw.handle_input(key(0x04, true), Some(&rec));
    let states = rec.of::<KeyboardState>(MsgType::KeyboardState);
    assert_eq!(states.len(), 2);
    assert!(states[1].is_pressed(0x04) && states[1].is_pressed(0xE1));
    rec.take();

    assert_eq!(sw.request(Request::Set(Target::Pc), Some(&rec)), Some(Target::Pc));
    let msgs = rec.take();
    assert_eq!(msgs[0].0, MsgType::ReleaseAll);
    assert!(!control.forwarding());
}

#[test]
fn ctrl_alt_end_becomes_delete() {
    let (mut sw, _, rec) = ready();
    sw.request(Request::Toggle, Some(&rec));
    rec.take();
    for e in [key(0xE0, true), key(0xE2, true), key(USAGE_END, true)] {
        sw.handle_input(e, Some(&rec));
    }
    let last = *rec.of::<KeyboardState>(MsgType::KeyboardState).last().unwrap();
    assert!(last.is_pressed(USAGE_DELETE) && !last.is_pressed(USAGE_END));
}

#[test]
fn mouse_batching_buttons_and_wheel() {
    let (mut sw, _, rec) = ready();
    sw.request(Request::Toggle, Some(&rec));
    rec.take();
    std::thread::sleep(MOUSE_INTERVAL);
    sw.handle_input(InputEvent::MouseMove { dx: 5, dy: -3 }, Some(&rec));
    sw.handle_input(InputEvent::MouseMove { dx: 7, dy: 1 }, Some(&rec));
    sw.handle_input(InputEvent::MouseButton { button: 0, down: true }, Some(&rec));
    sw.handle_input(InputEvent::Wheel { delta: 60 }, Some(&rec));
    sw.handle_input(InputEvent::Wheel { delta: 60 }, Some(&rec));
    let reports = rec.of::<MouseReport>(MsgType::MouseReport);
    let total_dx: i32 = reports.iter().map(|r| return r.move_x as i32).sum();
    let total_dy: i32 = reports.iter().map(|r| return r.move_y as i32).sum();
    assert_eq!((total_dx, total_dy), (12, -2));
    assert_eq!(reports.iter().map(|r| return r.wheel as i32).sum::<i32>(), 1);
    assert_eq!({ reports.last().unwrap().buttons }, 1);
    assert!(!sw.needs_tick());
}

#[test]
fn duplicated_toggles_are_debounced() {
    let (mut sw, _, rec) = ready();
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), Some(Target::Laptop));
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), None);
    assert_eq!(sw.target(), Target::Laptop);
    assert_eq!(sw.request(Request::Set(Target::Pc), Some(&rec)), Some(Target::Pc));
    std::thread::sleep(TOGGLE_DEBOUNCE);
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), Some(Target::Laptop));
}

#[test]
fn board_loss_returns_input_to_pc() {
    let (mut sw, control, rec) = ready();
    sw.request(Request::Toggle, Some(&rec));
    assert!(control.forwarding());
    assert_eq!(sw.on_board_lost(), Some(Target::Pc));
    assert!(!control.forwarding());
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), None);
}

#[test]
fn boot_button_long_press_is_emergency_return() {
    let (mut sw, _, rec) = ready();
    let short = ButtonEvent { kind: proto::BUTTON_EVENT_SHORT };
    let long = ButtonEvent { kind: proto::BUTTON_EVENT_LONG };
    assert_eq!(sw.on_button(short, Some(&rec)), Some(Target::Laptop));
    assert_eq!(sw.on_button(long, Some(&rec)), Some(Target::Pc));
    assert_eq!(sw.on_button(long, Some(&rec)), None);
}

fn on_laptop() -> (Switcher, Recorder) {
    let (mut sw, _, rec) = ready();
    sw.local_combos = true;
    assert_eq!(sw.request(Request::Toggle, Some(&rec)), Some(Target::Laptop));
    rec.take();
    return (sw, rec);
}

fn wait_for_release(sw: &mut Switcher, rec: &Recorder) {
    assert!(sw.needs_tick());
    std::thread::sleep(RELEASE_DELAY + Duration::from_millis(10));
    sw.tick(Some(rec));
    assert!(!sw.needs_tick());
}

fn last_message(rec: &Recorder) -> MsgType {
    return rec.0.borrow().last().unwrap().0;
}

#[test]
fn ctrl_alt_del_is_not_completed_on_the_laptop() {
    let (mut sw, rec) = on_laptop();
    sw.handle_input(key(USAGE_LEFT_CTRL, true), Some(&rec));
    sw.handle_input(key(USAGE_LEFT_ALT, true), Some(&rec));
    assert_eq!(sw.handle_input(key(USAGE_DELETE, true), Some(&rec)), None);
    wait_for_release(&mut sw, &rec);

    let states = rec.of::<KeyboardState>(MsgType::KeyboardState);
    assert!(states.iter().all(|k| return !k.is_pressed(USAGE_DELETE)), "Delete must not reach the laptop");
    let masked = states[states.len() - 2];
    assert!(masked.is_pressed(USAGE_LEFT_CTRL) && masked.is_pressed(USAGE_LEFT_ALT) && masked.is_pressed(MASK_KEY));
    assert!(states.last().unwrap().is_empty());
    assert_eq!(sw.take_notice(), Some(SECURE_ATTENTION_NOTE));
    assert_eq!(sw.target(), Target::Laptop);

    sw.handle_input(key(0x04, true), Some(&rec));
    let last = *rec.of::<KeyboardState>(MsgType::KeyboardState).last().unwrap();
    assert!(last.is_pressed(0x04) && !last.is_pressed(USAGE_LEFT_CTRL) && !last.is_pressed(USAGE_LEFT_ALT));
}

#[test]
fn win_l_locks_this_computer_and_brings_input_back() {
    let (mut sw, rec) = on_laptop();
    sw.handle_input(key(USAGE_LEFT_GUI, true), Some(&rec));
    assert_eq!(sw.handle_input(key(USAGE_L, true), Some(&rec)), Some(Target::Pc));
    assert!(!sw.control.forwarding());
    let states = rec.of::<KeyboardState>(MsgType::KeyboardState);
    assert!(states.iter().all(|k| return !k.is_pressed(USAGE_L)), "L must not reach the laptop");
    assert!(states.last().unwrap().is_pressed(USAGE_LEFT_GUI) && states.last().unwrap().is_pressed(MASK_KEY));
    assert!(rec.0.borrow().iter().all(|(m, _)| return *m != MsgType::ReleaseAll));
    wait_for_release(&mut sw, &rec);
    assert_eq!(last_message(&rec), MsgType::ReleaseAll);
    assert_eq!(sw.take_notice(), Some(LOCK_NOTE));
}

#[test]
fn combos_pass_through_where_the_os_does_not_keep_them() {
    let (mut sw, rec) = on_laptop();
    sw.local_combos = false;
    sw.handle_input(key(USAGE_LEFT_GUI, true), Some(&rec));
    sw.handle_input(key(USAGE_L, true), Some(&rec));
    let last = *rec.of::<KeyboardState>(MsgType::KeyboardState).last().unwrap();
    assert!(last.is_pressed(USAGE_LEFT_GUI) && last.is_pressed(USAGE_L));
    assert_eq!(sw.target(), Target::Laptop);
}

#[test]
fn combo_from_the_menu_is_pressed_then_released() {
    let (mut sw, _, rec) = ready();
    sw.send_combo(Combo::Lock, Some(&rec));
    let pressed = rec.of::<KeyboardState>(MsgType::KeyboardState);
    assert_eq!(pressed.len(), 1);
    assert!(pressed[0].is_pressed(USAGE_LEFT_GUI) && pressed[0].is_pressed(USAGE_L));
    wait_for_release(&mut sw, &rec);
    assert_eq!(last_message(&rec), MsgType::ReleaseAll);
    assert_eq!(sw.target(), Target::Pc);

    sw.on_usb(false);
    sw.send_combo(Combo::CtrlAltDel, Some(&rec));
    assert!(sw.take_notice().is_some());
}

#[test]
fn combo_names_from_the_gui() {
    assert_eq!(serde_json::from_str::<Combo>("\"ctrl_alt_del\"").unwrap(), Combo::CtrlAltDel);
    assert_eq!(serde_json::from_str::<Combo>("\"lock\"").unwrap(), Combo::Lock);
}

#[test]
fn a_pending_release_ends_with_the_board() {
    let (mut sw, rec) = on_laptop();
    sw.handle_input(key(USAGE_LEFT_GUI, true), Some(&rec));
    sw.handle_input(key(USAGE_L, true), Some(&rec));
    assert!(sw.needs_tick());
    sw.on_board_lost();
    assert!(!sw.needs_tick(), "nothing is left for a later board");
}

#[test]
fn a_combo_sent_from_the_tray_while_on_the_laptop_keeps_the_held_keys() {
    let (mut sw, rec) = on_laptop();
    sw.handle_input(key(0x04, true), Some(&rec));
    sw.send_combo(Combo::CtrlAltDel, Some(&rec));
    let pressed = *rec.of::<KeyboardState>(MsgType::KeyboardState).last().unwrap();
    assert!(pressed.is_pressed(USAGE_DELETE) && pressed.is_pressed(0x04));
    wait_for_release(&mut sw, &rec);
    let after = *rec.of::<KeyboardState>(MsgType::KeyboardState).last().unwrap();
    assert!(after.is_pressed(0x04) && !after.is_pressed(USAGE_DELETE));
}

#[test]
fn the_state_goes_out_again_in_case_a_datagram_was_lost() {
    let (mut sw, rec) = on_laptop();
    sw.handle_input(key(0x04, true), Some(&rec));
    sw.handle_input(InputEvent::MouseButton { button: 0, down: true }, Some(&rec));
    rec.take();
    sw.last_refresh -= STATE_REFRESH;
    sw.tick(Some(&rec));
    assert!(rec.of::<KeyboardState>(MsgType::KeyboardState)[0].is_pressed(0x04));
    assert_eq!(rec.of::<MouseReport>(MsgType::MouseReport)[0].buttons, 1);

    rec.take();
    sw.tick(Some(&rec));
    assert!(rec.take().is_empty(), "not more often than STATE_REFRESH");

    assert_eq!(sw.request(Request::Set(Target::Pc), Some(&rec)), Some(Target::Pc));
    rec.take();
    sw.last_refresh -= STATE_REFRESH;
    sw.tick(Some(&rec));
    assert_eq!(last_message(&rec), MsgType::ReleaseAll, "and the release after leaving");
}
