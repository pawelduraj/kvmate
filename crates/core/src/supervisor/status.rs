use std::collections::VecDeque;

use kvmate_protocol::DeviceInfo;
use serde::Serialize;

use crate::audio::AudioView;
use crate::firmware::{FlashStage, ImageInfo};
use crate::logfile::Level;
use crate::switcher::Target;
use crate::wifi::WifiView;

const MAX_LOG_LINES: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkState {
    #[default]
    Searching,
    Connecting,
    Connected,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    #[default]
    Cable,
    Wifi,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UsbState {
    #[default]
    Unknown,
    Detached,
    Mounted,
    Suspended,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SilentPort {
    pub port: String,
    pub bridge: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlashKind {
    Install,
    Erase,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FlashView {
    pub port: String,
    pub kind: FlashKind,
    pub stage: FlashStage,
    pub progress: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FlashOutcome {
    pub ok: bool,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BoardStatus {
    pub state: LinkState,
    pub demo: bool,
    pub port: Option<String>,
    pub transport: TransportKind,
    pub wifi: WifiView,
    pub bridge: Option<String>,
    pub firmware: Option<String>,
    pub mac: Option<String>,
    pub build_id: Option<String>,
    pub firmware_current: Option<bool>,
    pub selftest_ok: Option<bool>,
    pub versions_match: Option<bool>,
    pub usb: UsbState,
    pub target: Target,
    pub hotkey: String,
    pub capture_error: Option<String>,
    pub audio: AudioView,
    pub bundled_firmware: Option<ImageInfo>,
    pub silent_ports: Vec<SilentPort>,
    pub flash: Option<FlashView>,
    pub last_flash: Option<FlashOutcome>,
    pub logs: VecDeque<LogLine>,
    pub log_total: u64,
}

impl BoardStatus {
    pub(super) fn log(&mut self, level: Level, text: impl Into<String>) {
        if self.logs.len() >= MAX_LOG_LINES {
            self.logs.pop_front();
        }
        let text = text.into();
        crate::logfile::write(level, &text);
        self.logs.push_back(LogLine { level, text });
        self.log_total += 1;
    }

    pub(super) fn set_device(&mut self, info: &DeviceInfo) {
        let build = info.firmware_build_id;
        self.build_id = Some(crate::encoding::hex(&build));
        self.firmware_current = self.bundled_firmware.as_ref().map(|f| return f.build_id == build);
        let v = info.firmware_version;
        self.firmware = Some(format!("{}.{}.{}", v[0], v[1], v[2]));
        self.versions_match = Some(v == crate::hub_version());
        let m = info.mac_address;
        self.mac = Some(format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", m[0], m[1], m[2], m[3], m[4], m[5]));
        self.selftest_ok = Some(info.selftest_passed != 0);
    }

    pub(super) fn clear_device(&mut self) {
        self.port = None;
        self.bridge = None;
        self.firmware = None;
        self.mac = None;
        self.build_id = None;
        self.firmware_current = None;
        self.selftest_ok = None;
        self.versions_match = None;
        self.usb = UsbState::Unknown;
        self.transport = TransportKind::Cable;
        self.wifi.board = None;
        self.wifi.networks.clear();
        self.wifi.scanning = false;
    }
}
