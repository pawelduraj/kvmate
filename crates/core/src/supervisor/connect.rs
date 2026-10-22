use std::sync::Arc;

use kvmate_protocol as proto;

use super::session::Session;
use super::{Core, CoreEvent, LinkState, Mode, SilentPort, TransportKind};
use crate::board::discovery;
use crate::board::link::{EventSink, Link};
use crate::board::serial::SerialTransport;
use crate::board::sim::SimBoard;
use crate::board::transport::Transport;
use crate::board::udp::PeerHandle;
use crate::logfile::Level;

pub(super) enum Attempt {
    NotOpened,
    Opened { answered: bool },
}

impl Core {
    pub(super) fn scan(&mut self) {
        let attempts: Vec<(String, Option<&'static str>)> = match &self.mode {
            Mode::Demo => vec![("demo".into(), None)],
            Mode::Auto { port: Some(p) } => vec![(p.clone(), None)],
            Mode::Auto { port: None } => {
                discovery::candidates().into_iter().map(|c| return (c.name, Some(c.bridge))).collect()
            }
        };
        self.ports_seen = self.cable_ports();
        let before = self.status.silent_ports.clone();
        self.status.silent_ports.retain(|s| return attempts.iter().any(|(p, _)| return *p == s.port));
        self.port_errors.retain(|port, _| return attempts.iter().any(|(p, _)| return p == port));
        for (port, bridge) in attempts {
            if self.stopped() || self.pending_flash.is_some() {
                break;
            }
            let attempt = self.try_port(&port, bridge);
            if self.stopped() || self.pending_flash.is_some() {
                break;
            }
            match attempt {
                Attempt::Opened { answered: false } if !matches!(self.mode, Mode::Demo) => {
                    if !self.status.silent_ports.iter().any(|s| return s.port == port) {
                        self.status.silent_ports.push(SilentPort { port, bridge: bridge.map(str::to_owned) });
                    }
                }
                Attempt::Opened { answered: true } => self.status.silent_ports.retain(|s| return s.port != port),
                _ => {}
            }
        }
        if self.status.silent_ports != before {
            self.publish();
        }
        if matches!(self.mode, Mode::Auto { .. }) && !self.stopped() && self.pending_flash.is_none() {
            if let Some(secrets) = self.config.wifi.usable() {
                self.try_wifi(secrets);
            }
        }
    }

    pub(super) fn cable_ports(&self) -> Vec<String> {
        match &self.mode {
            Mode::Demo => return Vec::new(),
            Mode::Auto { port: None } => return discovery::candidates().into_iter().map(|c| return c.name).collect(),
            Mode::Auto { port: Some(p) } => {
                let present = serialport::available_ports()
                    .is_ok_and(|ports| return ports.iter().any(|i| return i.port_name == *p));
                return if present { vec![p.clone()] } else { Vec::new() };
            }
        }
    }

    fn try_port(&mut self, port: &str, bridge: Option<&'static str>) -> Attempt {
        self.status.state = LinkState::Connecting;
        self.status.port = Some(port.to_owned());
        self.status.bridge = bridge.map(str::to_owned);
        self.status.transport = TransportKind::Cable;
        self.publish();

        let (transport, _board): (Box<dyn Transport>, _) = match &self.mode {
            Mode::Demo => {
                let (hub_end, board) = SimBoard::pair_with_build("demo", self.demo_build);
                (Box::new(hub_end), Some(board))
            }
            Mode::Auto { .. } => match SerialTransport::open(port, proto::DEFAULT_BAUD) {
                Ok(t) => {
                    self.port_errors.remove(port);
                    (Box::new(t), None)
                }
                Err(e) => {
                    let e = e.to_string();
                    if self.port_errors.get(port) != Some(&e) {
                        self.status.log(Level::Warn, format!("{port}: can't open the port ({e})"));
                        self.port_errors.insert(port.to_owned(), e);
                    }
                    return Attempt::NotOpened;
                }
            },
        };
        return self.run_link(transport, TransportKind::Cable, None, port);
    }

    pub(super) fn run_link(
        &mut self,
        transport: Box<dyn Transport>,
        kind: TransportKind,
        peer: Option<PeerHandle>,
        label: &str,
    ) -> Attempt {
        self.link_id += 1;
        let id = self.link_id;
        let tx = self.tx.clone();
        let sink: EventSink = Arc::new(move |e| {
            let _ = tx.send(CoreEvent::Link(id, e));
        });
        let attempt = match Link::start(transport, sink) {
            Ok(link) => {
                let mut session = Session::new(link, id, kind, peer);
                self.drive(&mut session);
                session.link.shutdown();
                Attempt::Opened { answered: session.answered }
            }
            Err(e) => {
                self.status.log(Level::Error, format!("{label}: {e}"));
                Attempt::NotOpened
            }
        };
        self.board_lost();
        self.status.state = LinkState::Searching;
        self.status.clear_device();
        self.publish();
        return attempt;
    }
}
