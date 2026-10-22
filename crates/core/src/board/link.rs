//! The connection to one board over a [`Transport`]: frames, the handshake, heartbeats, and
//! noticing when the board goes silent.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use kvmate_protocol::{
    self as proto, AudioHeader, AudioStatus, ButtonEvent, DeviceInfo, Frame, FrameDecoder, Heartbeat, Msg, MsgType,
    RawData,
};

use crate::board::transport::{Reader, Transport, Writer, is_timeout};
use crate::logfile::{self, Level};

const HELLO_INTERVAL: Duration = Duration::from_millis(500);
const MAX_BATCH: usize = 16 * 1024;
const USB_FLAGS: u8 = proto::HEARTBEAT_FLAG_USB_MOUNTED | proto::HEARTBEAT_FLAG_USB_SUSPENDED;
const USB_FLAGS_UNSEEN: u8 = 0xFF;

#[derive(Clone, Debug, PartialEq)]
pub enum LinkEvent {
    Connected(DeviceInfo),
    Disconnected,
    Log { level: u8, text: String },
    Button(ButtonEvent),
    AudioStatus(AudioStatus),
    AudioPlayback(AudioHeader, Vec<u8>),
    Usb { mounted: bool, suspended: bool },
    WifiStatus(proto::WifiStatus),
    WifiScan(Vec<proto::WifiNetwork>),
    Failed(String),
}

pub type EventSink = Arc<dyn Fn(LinkEvent) + Send + Sync>;

pub trait BoardSink {
    fn send_raw(&self, msg_type: MsgType, payload: Vec<u8>);

    fn send(&self, msg: Msg<'_>) {
        self.send_raw(msg.msg_type(), msg.parts().concat());
    }
}

enum Command {
    Send(MsgType, Vec<u8>),
    Shutdown,
}

struct Shared {
    start: Instant,
    stop: AtomicBool,
    connected: AtomicBool,
    last_rx_ms: AtomicU64,
}

impl Shared {
    fn now_ms(&self) -> u64 {
        return self.start.elapsed().as_millis() as u64;
    }
}

pub struct Link {
    name: String,
    cmd_tx: Sender<Command>,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Link {
    pub fn start(transport: Box<dyn Transport>, events: EventSink) -> io::Result<Link> {
        let name = transport.name();
        let (reader, writer) = transport.split()?;
        let shared = Arc::new(Shared {
            start: Instant::now(),
            stop: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            last_rx_ms: AtomicU64::new(0),
        });
        let (cmd_tx, cmd_rx) = mpsc::channel();

        let writer_thread = {
            let shared = shared.clone();
            let events = events.clone();
            thread::Builder::new()
                .name(format!("link-tx {name}"))
                .spawn(move || writer_loop(writer, cmd_rx, &shared, &events))?
        };
        let reader_thread = {
            let shared = shared.clone();
            thread::Builder::new()
                .name(format!("link-rx {name}"))
                .spawn(move || reader_loop(reader, &shared, &events))?
        };

        return Ok(Link { name, cmd_tx, shared, threads: vec![writer_thread, reader_thread] });
    }

    pub fn name(&self) -> &str {
        return &self.name;
    }

    pub fn is_connected(&self) -> bool {
        return self.shared.connected.load(Ordering::Relaxed);
    }

    pub fn board(&self) -> Option<&dyn BoardSink> {
        return self.is_connected().then_some(self as &dyn BoardSink);
    }

    pub fn sender(&self) -> LinkSender {
        return LinkSender(self.cmd_tx.clone());
    }

    pub fn shutdown(self) {
        drop(self);
    }
}

impl BoardSink for Link {
    fn send_raw(&self, msg_type: MsgType, payload: Vec<u8>) {
        let _ = self.cmd_tx.send(Command::Send(msg_type, payload));
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[derive(Clone)]
pub struct LinkSender(Sender<Command>);

impl BoardSink for LinkSender {
    fn send_raw(&self, msg_type: MsgType, payload: Vec<u8>) {
        let _ = self.0.send(Command::Send(msg_type, payload));
    }
}

fn reader_loop(mut reader: Reader, shared: &Shared, events: &EventSink) {
    let mut decoder = FrameDecoder::new();
    let mut buf = [0u8; 4096];
    let mut usb_flags = USB_FLAGS_UNSEEN;
    while !shared.stop.load(Ordering::Relaxed) {
        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(e) if is_timeout(&e) => continue,
            Err(e) => {
                shared.stop.store(true, Ordering::Relaxed);
                events(LinkEvent::Failed(e.to_string()));
                break;
            }
        };
        decoder.push(&buf[..n], |frame| {
            shared.last_rx_ms.store(shared.now_ms(), Ordering::Relaxed);
            if let Some(event) = decode_event(&frame, shared, &mut usb_flags) {
                events(event);
            }
        });
    }
}

fn decode_event(frame: &Frame<'_>, shared: &Shared, usb_flags: &mut u8) -> Option<LinkEvent> {
    return match Msg::parse(frame)? {
        Msg::DeviceInfo(info) => (!shared.connected.swap(true, Ordering::Relaxed)).then(|| {
            *usb_flags = USB_FLAGS_UNSEEN;
            return LinkEvent::Connected(info);
        }),
        Msg::Heartbeat(hb) => {
            if hb.flags & proto::HEARTBEAT_FLAG_HELLO_ANSWERED == 0 {
                return shared.connected.swap(false, Ordering::Relaxed).then_some(LinkEvent::Disconnected);
            }
            let usb = hb.flags & USB_FLAGS;
            let changed = std::mem::replace(usb_flags, usb) != usb;
            (changed && shared.connected.load(Ordering::Relaxed)).then_some(LinkEvent::Usb {
                mounted: usb & proto::HEARTBEAT_FLAG_USB_MOUNTED != 0,
                suspended: usb & proto::HEARTBEAT_FLAG_USB_SUSPENDED != 0,
            })
        }
        Msg::Log(head, text) => {
            Some(LinkEvent::Log { level: head.level, text: String::from_utf8_lossy(text).into_owned() })
        }
        Msg::ButtonEvent(ev) => Some(LinkEvent::Button(ev)),
        Msg::AudioStatus(status) => Some(LinkEvent::AudioStatus(status)),
        Msg::AudioPlayback(head, pcm) => Some(LinkEvent::AudioPlayback(head, pcm.to_vec())),
        Msg::WifiStatus(status) => Some(LinkEvent::WifiStatus(status)),
        Msg::WifiScanResult(head, rest) => {
            let nets = rest
                .chunks_exact(proto::WifiNetwork::SIZE)
                .take(head.network_count as usize)
                .filter_map(proto::WifiNetwork::read_from)
                .collect();
            Some(LinkEvent::WifiScan(nets))
        }
        _ => None,
    };
}

fn writer_loop(mut writer: Writer, cmd_rx: Receiver<Command>, shared: &Shared, events: &EventSink) {
    let result = write_frames(&mut writer, &cmd_rx, shared, events);
    shared.stop.store(true, Ordering::Relaxed);
    if let Err(e) = result {
        events(LinkEvent::Failed(e.to_string()));
    }
}

fn write_frames(
    writer: &mut Writer,
    cmd_rx: &Receiver<Command>,
    shared: &Shared,
    events: &EventSink,
) -> io::Result<()> {
    let mut wire = vec![0u8; proto::MAX_ENCODED_LEN];
    let mut seq: u16 = 0;
    let mut last_hello: Option<Instant> = None;
    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = Duration::from_millis(proto::HEARTBEAT_INTERVAL_MS as u64);
    let mut out: Vec<u8> = Vec::with_capacity(MAX_BATCH + proto::MAX_ENCODED_LEN);

    let mut push = |out: &mut Vec<u8>, msg_type: MsgType, parts: &[&[u8]]| {
        match proto::encode_frame(msg_type, seq, parts, &mut wire) {
            Ok(n) => out.extend_from_slice(&wire[..n]),
            Err(e) => return logfile::write(Level::Error, &format!("{msg_type:?} not sent: {e:?}")),
        }
        seq = seq.wrapping_add(1);
    };

    loop {
        if shared.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let silent_ms = shared.now_ms().saturating_sub(shared.last_rx_ms.load(Ordering::Relaxed));
        if silent_ms > proto::LINK_TIMEOUT_MS as u64 && shared.connected.swap(false, Ordering::Relaxed) {
            events(LinkEvent::Disconnected);
        }

        if !shared.connected.load(Ordering::Relaxed) {
            if last_hello.is_none_or(|t| return t.elapsed() >= HELLO_INTERVAL) {
                push(&mut out, MsgType::Hello, &[]);
                last_hello = Some(Instant::now());
            }
        } else if last_heartbeat.elapsed() >= heartbeat_interval {
            let hb = Msg::Heartbeat(Heartbeat { flags: 0 });
            push(&mut out, hb.msg_type(), &hb.parts());
            last_heartbeat = Instant::now();
        }

        let mut shutdown = false;
        match cmd_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Command::Send(msg_type, payload)) => {
                push(&mut out, msg_type, &[&payload]);
                while out.len() < MAX_BATCH {
                    match cmd_rx.try_recv() {
                        Ok(Command::Send(msg_type, payload)) => push(&mut out, msg_type, &[&payload]),
                        Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => {
                            shutdown = true;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
            }
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => shutdown = true,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if shutdown {
            push(&mut out, MsgType::ReleaseAll, &[]);
            let _ = writer.write_all(&out);
            return Ok(());
        }
        if !out.is_empty() {
            writer.write_all(&out)?;
            out.clear();
        }
    }
}

#[cfg(test)]
pub(crate) fn channel_sink(tx: Sender<LinkEvent>) -> EventSink {
    return Arc::new(move |e| {
        let _ = tx.send(e);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::sim::{SimBoard, SimControl};

    fn wait_for(rx: &mpsc::Receiver<LinkEvent>, timeout: Duration, pred: impl Fn(&LinkEvent) -> bool) -> LinkEvent {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let ev = rx.recv_timeout(left).expect("timed out waiting for event");
            if pred(&ev) {
                return ev;
            }
        }
    }

    #[test]
    fn handshake_logs_and_buttons() {
        let (hub_end, board) = SimBoard::pair("sim");
        let (tx, rx) = mpsc::channel();
        let link = Link::start(Box::new(hub_end), channel_sink(tx)).unwrap();

        let ev = wait_for(&rx, Duration::from_secs(2), |e| matches!(e, LinkEvent::Connected(_)));
        let LinkEvent::Connected(info) = ev else { unreachable!() };
        assert_eq!({ info.firmware_version }, crate::hub_version());
        assert_eq!(info.selftest_passed, 1);
        assert!(link.is_connected());

        wait_for(
            &rx,
            Duration::from_secs(2),
            |e| matches!(e, LinkEvent::Log { text, .. } if text.contains("simulated")),
        );

        board.control(SimControl::PressButton(proto::BUTTON_EVENT_LONG));
        let ev = wait_for(&rx, Duration::from_secs(2), |e| matches!(e, LinkEvent::Button(_)));
        assert_eq!(ev, LinkEvent::Button(ButtonEvent { kind: proto::BUTTON_EVENT_LONG }));

        link.shutdown();
        board.stop();
    }

    #[test]
    fn detects_silence_and_board_restart() {
        let (hub_end, board) = SimBoard::pair("sim");
        let (tx, rx) = mpsc::channel();
        let link = Link::start(Box::new(hub_end), channel_sink(tx)).unwrap();
        wait_for(&rx, Duration::from_secs(2), |e| matches!(e, LinkEvent::Connected(_)));

        board.control(SimControl::Restart);
        wait_for(&rx, Duration::from_millis(proto::LINK_TIMEOUT_MS as u64 / 2), |e| {
            return *e == LinkEvent::Disconnected;
        });
        wait_for(&rx, Duration::from_secs(2), |e| matches!(e, LinkEvent::Connected(_)));

        board.control(SimControl::Mute(true));
        wait_for(&rx, Duration::from_secs(3), |e| return *e == LinkEvent::Disconnected);
        assert!(!link.is_connected());

        board.control(SimControl::Mute(false));
        wait_for(&rx, Duration::from_secs(2), |e| matches!(e, LinkEvent::Connected(_)));

        link.shutdown();
        assert!(board.stop().release_all_received, "hub must release all keys on shutdown");
    }

    #[test]
    fn audio_flows_both_ways() {
        let (hub_end, board) = SimBoard::pair("sim");
        let (tx, rx) = mpsc::channel();
        let link = Link::start(Box::new(hub_end), channel_sink(tx)).unwrap();
        wait_for(&rx, Duration::from_secs(2), |e| matches!(e, LinkEvent::Connected(_)));
        board.control(SimControl::Volume { volume: -10 * 256, mute: false });
        board.control(SimControl::Audio { playing: true, recording: true, clock_ppm: 0 });

        let mut next = 0u32;
        for _ in 0..25 {
            let ev = wait_for(&rx, Duration::from_secs(1), |e| matches!(e, LinkEvent::AudioPlayback(..)));
            let LinkEvent::AudioPlayback(head, pcm) = ev else { unreachable!() };
            assert_eq!({ head.first_frame_number }, next);
            assert_eq!(pcm.len(), 192 * 4);
            next += 192;
        }
        let ev =
            wait_for(&rx, Duration::from_secs(1), |e| matches!(e, LinkEvent::AudioStatus(s) if s.host_recording == 1));
        let LinkEvent::AudioStatus(status) = ev else { unreachable!() };
        assert_eq!({ status.host_volume }, -2560);
        assert_eq!(status.host_playing, 1);

        let sender = link.sender();
        let pcm = [0u8; 192 * 2];
        for i in 0..10u32 {
            sender.send(Msg::AudioCapture(AudioHeader { first_frame_number: i * 192 }, &pcm));
        }
        thread::sleep(Duration::from_millis(100));
        link.shutdown();
        let report = board.stop();
        assert_eq!(report.capture_frames_received, 1920);
        assert!(report.playback_frames_sent >= 25 * 192);
    }
}
