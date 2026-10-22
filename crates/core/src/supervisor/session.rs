use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use super::{Core, LinkState, Mode, TransportKind, UsbState};
use crate::board::discovery;
use crate::board::link::{Link, LinkEvent};
use crate::board::udp::PeerHandle;
use crate::logfile::Level;
use crate::switcher::Target;
use crate::wifi::{BoardWifi, NetworkView};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(2000);
const WIFI_HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(4000);
const CABLE_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const GIVE_UP_AFTER: Duration = Duration::from_secs(5);
const AUDIO_REFRESH: Duration = Duration::from_secs(1);
const WIFI_SCAN_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Session {
    pub(super) link: Link,
    pub(super) id: u64,
    pub(super) kind: TransportKind,
    peer: Option<PeerHandle>,
    opened: Instant,
    pub(super) answered: bool,
    silent_since: Option<Instant>,
    audio_published: Instant,
    cable_checked: Instant,
}

impl Session {
    pub(super) fn new(link: Link, id: u64, kind: TransportKind, peer: Option<PeerHandle>) -> Session {
        let now = Instant::now();
        return Session {
            link,
            id,
            kind,
            peer,
            opened: now,
            answered: false,
            silent_since: None,
            audio_published: now,
            cable_checked: now,
        };
    }
}

impl Core {
    pub(super) fn drive(&mut self, s: &mut Session) {
        while !self.stopped() {
            let timeout =
                if self.switcher.needs_tick() { Duration::from_millis(1) } else { Duration::from_millis(100) };
            let flow = match self.rx.recv_timeout(timeout) {
                Ok(event) => self.on_event(event, Some(&mut *s)),
                Err(_) => ControlFlow::Continue(()),
            };
            if flow.is_break() || self.housekeeping(s).is_break() {
                return;
            }
        }
    }

    pub(super) fn on_link_event(&mut self, event: LinkEvent, s: &mut Session) -> ControlFlow<()> {
        match event {
            LinkEvent::Connected(info) => {
                s.answered = true;
                s.silent_since = None;
                self.status.state = LinkState::Connected;
                self.status.set_device(&info);
                if let Some(peer) = &s.peer {
                    self.status.port = Some(match peer.get() {
                        Some(addr) => format!("Wi-Fi {}", addr.ip()),
                        None => "Wi-Fi".into(),
                    });
                }
                let fw = self.status.firmware.clone().unwrap_or_default();
                self.status.log(Level::Info, format!("{}: connected to the board (firmware {fw})", s.link.name()));
                if info.selftest_passed == 0 {
                    self.status.log(Level::Error, "the firmware's protocol self-test failed");
                }
                self.switcher.on_board_ready(&s.link);
                if !matches!(self.mode, Mode::Demo) {
                    let mac = info.mac_address;
                    self.switcher
                        .set_loopback_check(Box::new(move || return discovery::board_usb_on_this_computer(mac)));
                }
                self.audio.set_link(Some(s.link.sender()));
                self.publish();
            }
            LinkEvent::Disconnected => {
                s.silent_since = Some(Instant::now());
                self.status.state = LinkState::Connecting;
                self.status.usb = UsbState::Unknown;
                self.status.log(Level::Warn, format!("{}: the board doesn't answer, reconnecting", s.link.name()));
                self.board_lost();
                self.publish();
            }
            LinkEvent::Usb { mounted, suspended } => {
                self.switcher.on_usb(mounted || suspended);
                if mounted && self.switcher.board_usb_on_this_pc() {
                    self.status.log(Level::Warn, crate::switcher::LOOPBACK_REFUSAL);
                }
                self.status.usb = match (mounted, suspended) {
                    (_, true) => UsbState::Suspended,
                    (true, false) => UsbState::Mounted,
                    (false, false) => UsbState::Detached,
                };
                self.publish();
            }
            LinkEvent::Log { level, text } => self.note(Level::from_board(level), format!("[board] {text}")),
            LinkEvent::Button(ev) => {
                if let Some(t) = self.switcher.on_button(ev, s.link.board()) {
                    self.target_changed(t);
                }
            }
            LinkEvent::AudioStatus(status) => {
                if self.audio.on_status(status) {
                    s.audio_published = Instant::now();
                    self.publish();
                }
            }
            LinkEvent::AudioPlayback(head, pcm) => self.audio.on_playback(&head, &pcm),
            LinkEvent::WifiStatus(report) => self.on_board_wifi(BoardWifi::from_message(&report)),
            LinkEvent::WifiScan(networks) => {
                self.status.wifi.networks = networks.iter().map(NetworkView::from_message).collect();
                self.status.wifi.scanning = false;
                self.publish();
            }
            LinkEvent::Failed(e) => {
                self.status.log(Level::Error, format!("{}: {e}", s.link.name()));
                return ControlFlow::Break(());
            }
        }
        return ControlFlow::Continue(());
    }

    fn housekeeping(&mut self, s: &mut Session) -> ControlFlow<()> {
        self.switcher.tick(s.link.board());
        if self.status.wifi.scanning && self.wifi_scan_sent.elapsed() > WIFI_SCAN_TIMEOUT {
            self.status.wifi.scanning = false;
            self.note(Level::Warn, "the board didn't send the list of Wi-Fi networks");
        }
        if self.audio.busy() && s.audio_published.elapsed() >= AUDIO_REFRESH {
            s.audio_published = Instant::now();
            self.publish();
        }

        let handshake_timeout = match s.kind {
            TransportKind::Cable => HANDSHAKE_TIMEOUT,
            TransportKind::Wifi => WIFI_HANDSHAKE_TIMEOUT,
        };
        if !s.answered && s.opened.elapsed() > handshake_timeout {
            return ControlFlow::Break(());
        }
        if s.kind == TransportKind::Wifi && s.cable_checked.elapsed() >= CABLE_CHECK_INTERVAL {
            s.cable_checked = Instant::now();
            let ports = self.cable_ports();
            let plugged = ports.iter().any(|p| return !self.ports_seen.contains(p));
            self.ports_seen = ports;
            if plugged {
                if s.answered {
                    self.status.log(Level::Info, "board cable detected — switching from Wi-Fi to the cable");
                }
                return ControlFlow::Break(());
            }
        }
        if s.silent_since.is_some_and(|t| return t.elapsed() > GIVE_UP_AFTER) {
            return ControlFlow::Break(());
        }
        return ControlFlow::Continue(());
    }

    pub(super) fn board_lost(&mut self) {
        if let Some(t) = self.switcher.on_board_lost() {
            self.target_changed(t);
        }
        self.audio.set_link(None);
    }

    pub(super) fn target_changed(&mut self, target: Target) {
        self.status.target = target;
        let label = match target {
            Target::Pc => "this computer",
            Target::Laptop => "other computer",
        };
        self.status.log(Level::Info, format!("keyboard and mouse: {label}"));
        match (target, &self.capture) {
            (Target::Pc, _) => self.release_stuck_keys(),
            (Target::Laptop, Some(capture)) => capture.refresh(),
            (Target::Laptop, None) => {}
        }
        self.audio.set_target(target == Target::Laptop);
        self.publish();
    }

    fn release_stuck_keys(&mut self) {
        let Some(capture) = &self.capture else { return };
        match capture.release_stuck_keys() {
            Ok(keys) if keys.is_empty() => {}
            Ok(keys) => {
                self.status.log(Level::Warn, format!("released keys stuck on this computer: {}", keys.join(", ")))
            }
            Err(keys) => self
                .status
                .log(Level::Warn, format!("keys stuck on this computer ({}): press and release them", keys.join(", "))),
        }
    }
}
