use super::stuck_keys::key_input;
use super::*;
use crate::input::keymap;
use crate::supervisor::LinkState;
use crate::supervisor::tests::{start_demo, wait_until};
use std::time::Duration;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: valid out pointer.
    unsafe { GetCursorPos(&mut p) };
    return (p.x, p.y);
}

fn inject(input: INPUT) {
    // SAFETY: one fully initialised INPUT.
    unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) };
}

fn mouse_move(dx: i32) -> INPUT {
    return INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 },
        },
    };
}

fn right_ctrl(up: bool) -> INPUT {
    let flags = KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | if up { KEYEVENTF_KEYUP } else { 0 };
    return INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: 0x1D, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
}

fn key_scan(scan: u16, up: bool) -> INPUT {
    let flags = KEYEVENTF_SCANCODE | if up { KEYEVENTF_KEYUP } else { 0 };
    return INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
}

#[test]
#[ignore]
fn live_release_stuck_keys() {
    let capture = start_capture(CaptureControl::new(false, crate::input::keymap::USAGE_RIGHT_CTRL), Arc::new(|_| {}))
        .expect("capture");
    // SAFETY: plain Win32 call.
    let windows_down = |vk: VIRTUAL_KEY| return unsafe { GetAsyncKeyState(vk as i32) } < 0;
    inject(key_input(VK_LCONTROL, true));
    inject(key_input(VK_LMENU, true));
    inject(key_scan(0x2A, false));
    thread::sleep(Duration::from_millis(100));
    let before = [VK_LCONTROL, VK_LMENU, VK_LSHIFT].map(windows_down);
    let released = capture.release_stuck_keys();
    thread::sleep(Duration::from_millis(100));
    let after = [VK_LCONTROL, VK_LMENU].map(windows_down);
    inject(key_scan(0x2A, true));
    inject(key_input(VK_LMENU, false));
    inject(key_input(VK_LCONTROL, false));
    drop(capture);
    println!("held before {before:?}, released {released:?}, still held after {after:?}");
    assert_eq!(before, [true; 3], "injected presses did not reach Windows");
    assert_eq!(released, Ok(vec!["Left Ctrl", "Left Alt"]));
    assert_eq!(after, [false; 2]);
}

#[test]
#[ignore]
fn live_double_tap_toggles_once() {
    let control = CaptureControl::new(false, crate::input::keymap::USAGE_RIGHT_CTRL);
    let (tx, rx) = mpsc::channel();
    let sink: InputSink = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    let capture = start_capture(control.clone(), sink).expect("capture");
    control.set_forwarding(true);
    for _ in 0..2 {
        inject(right_ctrl(false));
        thread::sleep(Duration::from_millis(40));
        inject(right_ctrl(true));
        thread::sleep(Duration::from_millis(80));
    }
    thread::sleep(Duration::from_millis(200));
    control.set_forwarding(false);
    drop(capture);
    let events: Vec<InputEvent> = rx.try_iter().collect();
    println!("events: {events:?}");
    assert_eq!(events.iter().filter(|e| return **e == InputEvent::Hotkey { back_here: true }).count(), 1);
}

#[test]
#[ignore]
fn live_shift_keys() {
    let control = CaptureControl::new(false, crate::input::keymap::USAGE_RIGHT_CTRL);
    let (tx, rx) = mpsc::channel();
    let sink: InputSink = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    let capture = start_capture(control.clone(), sink).expect("capture");
    control.set_forwarding(true);
    for scan in [0x2A, 0x36] {
        inject(key_scan(scan, false));
        thread::sleep(Duration::from_millis(30));
        inject(key_scan(scan, true));
        thread::sleep(Duration::from_millis(30));
    }
    thread::sleep(Duration::from_millis(200));
    control.set_forwarding(false);
    drop(capture);
    let events: Vec<InputEvent> = rx.try_iter().collect();
    println!("events: {events:?}");
    for usage in [0xE1, keymap::USAGE_RIGHT_SHIFT] {
        for down in [true, false] {
            let ev = InputEvent::Key { usage: KeyUsage::Keyboard(usage), down };
            assert!(events.contains(&ev), "{ev:?} not reported");
        }
    }
}

#[test]
#[ignore]
fn live_capture_hides_input_and_reports_raw_mouse() {
    let control = CaptureControl::new(true, crate::input::keymap::USAGE_RIGHT_CTRL);
    let (tx, rx) = mpsc::channel();
    let sink: InputSink = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    let capture = start_capture(control.clone(), sink).expect("capture");

    let before = cursor();
    control.set_forwarding(true);
    thread::sleep(Duration::from_millis(50));
    for _ in 0..4 {
        inject(mouse_move(25));
    }
    inject(key_scan(0x76, false));
    inject(key_scan(0x76, true));
    thread::sleep(Duration::from_millis(250));
    let after = cursor();
    control.set_forwarding(false);
    drop(capture);

    let events: Vec<InputEvent> = rx.try_iter().collect();
    let dx: i32 =
        events.iter().map(|e| if let InputEvent::MouseMove { dx, .. } = e { return *dx } else { return 0 }).sum();
    let f24 = KeyUsage::Keyboard(0x73);
    println!("events: {events:?}");
    println!("raw mouse dx sum: {dx}, cursor before {before:?} after {after:?}");
    assert!(events.contains(&InputEvent::Key { usage: f24, down: true }), "F24 down not reported");
    assert!(events.contains(&InputEvent::Key { usage: f24, down: false }), "F24 up not reported");
    assert!(dx > 0, "no Raw Input movement while the hook hid the mouse");
    assert_eq!(before, after, "cursor moved: mouse was not hidden from Windows");
}

#[test]
#[ignore]
fn live_core_hotkey_switches_once() {
    let (sup, seen) = start_demo(true, None);
    wait_until(&seen, "connected", |s| return s.state == LinkState::Connected);

    for _ in 0..2 {
        inject(right_ctrl(false));
        thread::sleep(Duration::from_millis(40));
        inject(right_ctrl(true));
        thread::sleep(Duration::from_millis(80));
    }
    thread::sleep(Duration::from_millis(500));
    drop(sup);

    let last = seen.lock().unwrap().last().cloned().unwrap();
    println!("capture_error: {:?}", last.capture_error);
    for line in &last.logs {
        println!("log [{:?}] {}", line.level, line.text);
    }
    let switches: Vec<&str> = last
        .logs
        .iter()
        .filter(|l| return l.text.starts_with("keyboard and mouse"))
        .map(|l| return l.text.as_str())
        .collect();
    println!("switch log: {switches:?}");
    assert_eq!(switches, ["keyboard and mouse: other computer", "keyboard and mouse: this computer"]);
}
