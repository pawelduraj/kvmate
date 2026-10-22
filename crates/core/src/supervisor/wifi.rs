use std::time::Instant;

use kvmate_protocol::Msg;

use super::connect::Attempt;
use super::session::Session;
use super::{Core, LinkState, Mode, TransportKind};
use crate::board::link::BoardSink;
use crate::board::udp::UdpTransport;
use crate::logfile::Level;
use crate::wifi::wlan::{self, Adapter};
use crate::wifi::{BoardWifi, WifiFail, WifiMode, WifiSecrets, WifiSettings, WifiSetup, WifiState};

const NO_BOARD: &str = "no board: connect it with the “COM” cable to set up Wi-Fi";

impl Core {
    pub(super) fn forget_board_network(&self, wifi: &WifiSettings) {
        if wifi.mode == WifiMode::BoardNetwork && !matches!(self.mode, Mode::Demo) && self.live {
            wlan::forget(&wifi.network_ssid);
        }
    }

    pub(super) fn try_wifi(&mut self, secrets: WifiSecrets) {
        let settings = self.config.wifi.clone();
        if settings.mode == WifiMode::BoardNetwork {
            let adapter = wlan::ensure_connected(&settings.network_ssid, &secrets.network_password);
            let text = adapter.describe(&settings.network_ssid);
            let blocked = matches!(adapter, Adapter::OtherNetwork(_) | Adapter::NoAdapter | Adapter::Error(_));
            if self.status.wifi.adapter.as_deref() != Some(text.as_str()) {
                if blocked || adapter == Adapter::NoAddress {
                    self.status.log(Level::Warn, text.clone());
                }
                self.status.wifi.adapter = Some(text);
                self.publish();
            }
            if blocked {
                return;
            }
        } else if self.status.wifi.adapter.take().is_some() {
            self.publish();
        }
        let transport = match UdpTransport::open(secrets.key, settings.targets()) {
            Ok(t) => t,
            Err(e) => {
                self.note(Level::Warn, format!("Wi-Fi: {e}"));
                return;
            }
        };
        let peer = transport.peer_handle();
        self.status.state = LinkState::Connecting;
        self.status.port = None;
        self.status.bridge = None;
        self.status.transport = TransportKind::Wifi;
        self.publish();
        match self.run_link(Box::new(transport), TransportKind::Wifi, Some(peer), "Wi-Fi") {
            Attempt::Opened { answered: true } => self.wifi_miss_logged = false,
            Attempt::Opened { answered: false }
                if !self.wifi_miss_logged && !self.stopped() && self.config.wifi == settings =>
            {
                let ssid = match settings.mode {
                    WifiMode::BoardNetwork => &settings.network_ssid,
                    _ => &settings.router_ssid,
                };
                self.note(
                    Level::Warn,
                    format!("Wi-Fi: the board doesn't answer on network “{ssid}” — still looking (cable and Wi-Fi)"),
                );
                self.wifi_miss_logged = true;
            }
            _ => {}
        }
    }

    pub(super) fn wifi_apply(&mut self, setup: WifiSetup, session: Option<&Session>) {
        let Some(s) = session else {
            self.note(Level::Warn, NO_BOARD);
            return;
        };
        if s.kind == TransportKind::Wifi {
            self.status.log(Level::Warn, "the board takes Wi-Fi settings over the cable only — connect its “COM” port");
        } else if s.link.is_connected() {
            s.link.send(Msg::WifiConfig(setup.message()));
            self.status.log(
                Level::Info,
                match setup.mode {
                    WifiMode::Off => "Wi-Fi: the board deletes its Wi-Fi settings".to_string(),
                    WifiMode::Router => format!("Wi-Fi: network “{}” saved on the board", setup.ssid),
                    WifiMode::BoardNetwork => format!("Wi-Fi: the board starts its own network “{}”", setup.ssid),
                },
            );
        } else {
            self.status.log(Level::Warn, "no board: can't save the Wi-Fi settings");
        }
        self.publish();
    }

    pub(super) fn wifi_scan(&mut self, session: Option<&Session>) {
        let Some(s) = session else {
            self.note(Level::Warn, NO_BOARD);
            return;
        };
        if s.link.is_connected() {
            s.link.send(Msg::WifiScan);
            self.status.wifi.scanning = true;
            self.wifi_scan_sent = Instant::now();
        } else {
            self.status.log(Level::Warn, "no board: can't look for Wi-Fi networks");
        }
        self.publish();
    }

    pub(super) fn on_board_wifi(&mut self, report: BoardWifi) {
        let before = self.status.wifi.board.as_ref().map(|b| return (b.mode, b.state, b.fail));
        let now = (report.mode, report.state, report.fail);
        if before != Some(now) && report.mode != WifiMode::Off {
            let ssid = &report.ssid;
            let text = match (report.state, report.fail) {
                (WifiState::Ready, _) if report.mode == WifiMode::Router => Some((
                    Level::Info,
                    format!("board connected to network “{ssid}” ({})", report.ip.as_deref().unwrap_or("?")),
                )),
                (WifiState::Ready, _) => Some((Level::Info, format!("board network “{ssid}” is up"))),
                (WifiState::Failed, Some(WifiFail::NotFound)) => {
                    Some((Level::Warn, format!("the board can't see network “{ssid}” (range? 2.4 GHz only)")))
                }
                (WifiState::Failed, Some(WifiFail::Auth)) => {
                    Some((Level::Warn, format!("the board can't connect to “{ssid}”: wrong password?")))
                }
                (WifiState::Failed, _) => Some((Level::Warn, format!("the board can't connect to network “{ssid}”"))),
                _ => None,
            };
            if let Some((level, text)) = text {
                self.status.log(level, text);
            }
        }
        if self.status.wifi.board.as_ref() != Some(&report) {
            self.status.wifi.board = Some(report);
            self.publish();
        }
    }
}
