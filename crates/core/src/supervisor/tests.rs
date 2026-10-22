use super::*;
use crate::firmware::FlashStage;
use crate::wifi::WifiMode;
use std::sync::Mutex;

struct Collect(Arc<Mutex<Vec<BoardStatus>>>);

impl Observer for Collect {
    fn status_changed(&self, status: &BoardStatus) {
        self.0.lock().unwrap().push(status.clone());
    }
}

pub(crate) fn start_demo(capture: bool, firmware: Option<FirmwareImage>) -> (Supervisor, Arc<Mutex<Vec<BoardStatus>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let options = Options { mode: Mode::Demo, config: Config::default(), capture, notes: Vec::new(), firmware };
    return (Supervisor::start(options, Collect(seen.clone())), seen);
}

pub(crate) fn wait_until(seen: &Mutex<Vec<BoardStatus>>, what: &str, pred: impl Fn(&BoardStatus) -> bool) {
    wait_until_for(seen, what, Duration::from_secs(3), pred);
}

fn wait_until_for(seen: &Mutex<Vec<BoardStatus>>, what: &str, timeout: Duration, pred: impl Fn(&BoardStatus) -> bool) {
    let deadline = Instant::now() + timeout;
    while !seen.lock().unwrap().iter().any(&pred) {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn demo_mode_connects_and_switches() {
    let (sup, seen) = start_demo(false, None);

    wait_until(&seen, "connected", |s| {
        return s.state == LinkState::Connected
            && s.demo
            && s.versions_match == Some(true)
            && s.selftest_ok == Some(true);
    });
    wait_until(&seen, "usb mounted", |s| return s.usb == UsbState::Mounted);

    sup.command(Command::SetTarget(Target::Laptop));
    wait_until(&seen, "target laptop", |s| return s.target == Target::Laptop);
    sup.command(Command::SetTarget(Target::Pc));
    wait_until(&seen, "back to pc", |s| {
        return s.target == Target::Pc && s.logs.iter().any(|l| return l.text.contains("this computer"));
    });
    drop(sup);
}

#[test]
fn settings_apply_without_restart() {
    let (sup, seen) = start_demo(false, None);
    wait_until(&seen, "connected", |s| return s.state == LinkState::Connected && s.hotkey == "2× Right Ctrl");

    let config = Config { hotkey: crate::config::Hotkey::DoubleLeftCtrl, led_brightness: 99, ..Config::default() };
    sup.command(Command::ApplyConfig(Box::new(config.clone())));
    wait_until(&seen, "new hotkey shown", |s| {
        return s.hotkey == "2× Left Ctrl"
            && s.logs.iter().any(|l| return l.text.contains("shortcut") && l.text.contains("LED"));
    });
    sup.command(Command::ApplyConfig(Box::new(Config { jiggle: true, ..config })));
    wait_until(&seen, "jiggler on", |s| {
        return s.logs.iter().any(|l| return l.text == "settings changed: other computer kept awake");
    });
    drop(sup);
}

#[test]
fn wifi_setup_over_the_cable() {
    let (sup, seen) = start_demo(false, None);
    wait_until(&seen, "connected, Wi-Fi off", |s| {
        return s.state == LinkState::Connected
            && s.wifi.board.as_ref().is_some_and(|w| return w.mode == WifiMode::Off);
    });

    sup.command(Command::WifiScan);
    wait_until(&seen, "networks found", |s| {
        return !s.wifi.scanning && s.wifi.networks.iter().any(|n| return n.ssid == "Home");
    });

    let setup = WifiSetup {
        mode: WifiMode::Router,
        ssid: "Home".into(),
        password: "secret-pass".into(),
        key: [5; 32],
        hostname: "kvmate".into(),
    };
    sup.command(Command::WifiApply(setup));
    wait_until(&seen, "board joined the network", |s| {
        return s.wifi.board.as_ref().is_some_and(|w| {
            return w.mode == WifiMode::Router && w.state == crate::wifi::WifiState::Ready && w.ssid == "Home";
        }) && s.logs.iter().any(|l| return l.text.contains("connected to network “Home”"));
    });
    drop(sup);
}

#[test]
fn demo_mode_firmware_update() {
    let image: &'static [u8] = Box::leak(crate::firmware::tests::fake_image(9, "0.1.0").into_boxed_slice());
    let firmware = FirmwareImage::new(image).unwrap().unwrap();
    let bundled = crate::encoding::hex(&firmware.info.build_id);
    let (sup, seen) = start_demo(false, Some(firmware));

    wait_until(&seen, "connected, other build", |s| {
        return s.state == LinkState::Connected && s.firmware_current == Some(false);
    });
    sup.command(Command::InstallFirmware { port: None });
    wait_until(&seen, "writing", |s| {
        return s.flash.as_ref().is_some_and(|f| return f.stage == FlashStage::Writing && f.kind == FlashKind::Install);
    });
    wait_until_for(&seen, "done", Duration::from_secs(8), |s| {
        return s.last_flash.as_ref().is_some_and(|o| return o.ok);
    });
    wait_until_for(&seen, "current", Duration::from_secs(5), |s| {
        return s.state == LinkState::Connected
            && s.firmware_current == Some(true)
            && s.build_id.as_deref() == Some(bundled.as_str());
    });
    drop(sup);
}
