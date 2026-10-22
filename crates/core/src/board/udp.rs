//! Encrypted UDP transport to the board (datagram format: `kvmate_protocol::secure`).
//!
//! The link layer writes and reads the same frame bytes as on the cable. The reader also runs
//! the handshake. It runs it again whenever the board has been silent for a second (it
//! restarted, or it was out of reach).

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kvmate_protocol::RawData;
use kvmate_protocol::secure::{self, DgramReply, PairingKey, Session};

use crate::board::transport::{READ_TIMEOUT, Reader, Transport, Writer, is_timeout};
use crate::lock;
use crate::wifi::random_bytes;

const POLL: Duration = Duration::from_millis(10);
const SILENCE: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug, Default)]
pub struct Targets {
    pub fixed: Vec<SocketAddr>,
    pub broadcast_port: Option<u16>,
}

impl Targets {
    fn addresses(&self) -> Vec<SocketAddr> {
        let mut out = self.fixed.clone();
        if let Some(port) = self.broadcast_port {
            for iface in if_addrs::get_if_addrs().unwrap_or_default() {
                if let if_addrs::IfAddr::V4(v4) = iface.addr {
                    if let Some(b) = v4.broadcast.filter(|_| return !v4.ip.is_loopback()) {
                        out.push(SocketAddr::V4(SocketAddrV4::new(b, port)));
                    }
                }
            }
            out.push(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, port)));
        }
        out.dedup();
        return out;
    }
}

struct State {
    session: Option<Session>,
    peer: Option<SocketAddr>,
    hub_nonce: Option<[u8; 16]>,
    last_hello: Option<Instant>,
    last_valid: Instant,
}

type Shared = Arc<Mutex<State>>;

pub struct UdpTransport {
    socket: UdpSocket,
    key: PairingKey,
    targets: Targets,
    state: Shared,
}

impl UdpTransport {
    pub fn open(key: PairingKey, targets: Targets) -> io::Result<UdpTransport> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(POLL))?;
        let state = Arc::new(Mutex::new(State {
            session: None,
            peer: None,
            hub_nonce: None,
            last_hello: None,
            last_valid: Instant::now(),
        }));
        return Ok(UdpTransport { socket, key, targets, state });
    }

    pub fn peer_handle(&self) -> PeerHandle {
        return PeerHandle(self.state.clone());
    }
}

#[derive(Clone)]
pub struct PeerHandle(Shared);

impl PeerHandle {
    pub fn get(&self) -> Option<SocketAddr> {
        return lock(&self.0).peer;
    }
}

impl Transport for UdpTransport {
    fn name(&self) -> String {
        return "Wi-Fi".into();
    }

    fn split(self: Box<Self>) -> io::Result<(Reader, Writer)> {
        let writer = UdpWriter { socket: self.socket.try_clone()?, state: self.state.clone() };
        let reader = UdpReader {
            socket: self.socket,
            key: self.key,
            targets: self.targets,
            state: self.state,
            pending: Vec::new(),
            pos: 0,
            dgram: vec![0; secure::DGRAM_MAX_LEN + 64],
        };
        return Ok((Box::new(reader), Box::new(writer)));
    }
}

struct UdpReader {
    socket: UdpSocket,
    key: PairingKey,
    targets: Targets,
    state: Shared,
    pending: Vec<u8>,
    pos: usize,
    dgram: Vec<u8>,
}

impl UdpReader {
    fn maybe_hello(&mut self) {
        let hello = {
            let mut st = lock(&self.state);
            let needed = st.session.is_none() || st.last_valid.elapsed() > SILENCE;
            let due = st
                .last_hello
                .is_none_or(|t| return t.elapsed() >= Duration::from_millis(secure::DGRAM_HELLO_INTERVAL_MS as u64));
            if !(needed && due) {
                return;
            }
            let nonce = random_bytes::<16>();
            st.hub_nonce = Some(nonce);
            st.last_hello = Some(Instant::now());
            secure::hello(&self.key, nonce)
        };
        for addr in self.targets.addresses() {
            let _ = self.socket.send_to(hello.as_bytes(), addr);
        }
    }

    fn handle(&mut self, n: usize, from: SocketAddr) {
        let dgram = &mut self.dgram[..n];
        let mut st = lock(&self.state);
        if dgram[0] == secure::DGRAM_DATA {
            let Some(session) = st.session.as_mut() else { return };
            let Some(plain) = session.rx.open_in_place(dgram) else { return };
            self.pending.clear();
            self.pending.extend_from_slice(plain);
            self.pos = 0;
            st.last_valid = Instant::now();
            st.peer = Some(from);
            return;
        }
        let Some(reply) = DgramReply::read_from(dgram) else { return };
        let (hub_nonce, board_nonce, mac) = (reply.hub_nonce, reply.board_nonce, reply.reply_mac);
        let reply_window = Duration::from_millis(2 * secure::DGRAM_HELLO_INTERVAL_MS as u64);
        if reply.magic != secure::DGRAM_MAGIC
            || reply.kind != secure::DGRAM_REPLY
            || st.hub_nonce != Some(hub_nonce)
            || st.last_hello.is_none_or(|t| return t.elapsed() > reply_window)
            || !secure::constant_time_eq(&mac, &secure::reply_mac(&self.key, &hub_nonce, &board_nonce))
        {
            return;
        }
        st.session = Some(Session::for_hub(&self.key, &hub_nonce, &board_nonce));
        st.hub_nonce = None;
        st.peer = Some(from);
        st.last_valid = Instant::now();
    }
}

impl Read for UdpReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + READ_TIMEOUT;
        loop {
            if self.pos < self.pending.len() {
                let n = buf.len().min(self.pending.len() - self.pos);
                buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            self.maybe_hello();
            match self.socket.recv_from(&mut self.dgram) {
                Ok((n, from)) if n > 0 => self.handle(n, from),
                Ok(_) => {}
                Err(e) if is_timeout(&e) => {}
                Err(_) => std::thread::sleep(POLL),
            }
            if self.pos >= self.pending.len() && Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
    }
}

struct UdpWriter {
    socket: UdpSocket,
    state: Shared,
}

const _: () = assert!(kvmate_protocol::MAX_ENCODED_LEN <= secure::DGRAM_MAX_PLAIN);

fn chunks(buf: &[u8], max: usize) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let (mut start, mut frame_start) = (0, 0);
    for i in 1..=buf.len() {
        let boundary = i == buf.len() || (buf[i - 1] == 0 && buf[i] == 0 && i - frame_start >= 2);
        if !boundary {
            continue;
        }
        if i - start > max && frame_start > start {
            out.push(&buf[start..frame_start]);
            start = frame_start;
        }
        frame_start = i;
    }
    if start < buf.len() {
        out.push(&buf[start..]);
    }
    return out;
}

fn seal_all(tx: &mut secure::Sealer, buf: &[u8]) -> Vec<Vec<u8>> {
    let mut dgram = [0u8; secure::DGRAM_MAX_LEN];
    let sealed = chunks(buf, secure::DGRAM_MAX_PLAIN).into_iter().filter_map(|chunk| {
        return tx.seal(chunk, &mut dgram).map(|n| return dgram[..n].to_vec());
    });
    return sealed.collect();
}

impl Write for UdpWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (peer, dgrams) = {
            let mut st = lock(&self.state);
            let Some(peer) = st.peer else { return Ok(buf.len()) };
            let Some(session) = st.session.as_mut() else { return Ok(buf.len()) };
            (peer, seal_all(&mut session.tx, buf))
        };
        for dgram in dgrams {
            let _ = self.socket.send_to(&dgram, peer);
        }
        return Ok(buf.len());
    }

    fn flush(&mut self) -> io::Result<()> {
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread::{self, JoinHandle};

    use kvmate_protocol::secure::DgramHello;
    use kvmate_protocol::{KeyboardState, Msg};

    use super::*;
    use crate::board::link::{BoardSink, Link, LinkEvent, channel_sink};
    use crate::board::memory::MemoryTransport;
    use crate::board::sim::{SimBoard, SimControl};

    #[test]
    fn frame_chunks() {
        let f = |len: usize| -> Vec<u8> { return [vec![0], vec![1; len], vec![0]].concat() };
        let buf = [f(5), f(3), f(10)].concat();
        assert_eq!(chunks(&buf, 100), vec![&buf[..]]);
        let parts = chunks(&buf, 14);
        assert_eq!(parts, vec![&buf[..12], &buf[12..]], "two frames fit, the third goes alone");
        assert_eq!(chunks(&buf, 5).len(), 3, "a frame larger than the limit still goes whole");
        assert!(chunks(&[], 10).is_empty());
    }

    fn wait_for(rx: &mpsc::Receiver<LinkEvent>, pred: impl Fn(&LinkEvent) -> bool) -> LinkEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ev = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("event");
            if pred(&ev) {
                return ev;
            }
        }
    }

    #[test]
    fn link_over_udp() {
        let key: PairingKey = random_bytes();
        let (hub_end, board_end) = MemoryTransport::pair("sim");
        let board = SimBoard::spawn(Box::new(board_end.with_read_timeout(Duration::from_millis(1))));
        let udp_board = UdpBoard::spawn(key, Box::new(hub_end.with_read_timeout(Duration::from_millis(1)))).unwrap();

        let wrong =
            UdpTransport::open(random_bytes(), Targets { fixed: vec![udp_board.addr], broadcast_port: None }).unwrap();
        let (tx, rx) = mpsc::channel();
        let link = Link::start(Box::new(wrong), channel_sink(tx)).unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(1200)).is_err(), "no events without the key");
        link.shutdown();

        let transport = UdpTransport::open(key, Targets { fixed: vec![udp_board.addr], broadcast_port: None }).unwrap();
        let peer = transport.peer_handle();
        let (tx, rx) = mpsc::channel();
        let link = Link::start(Box::new(transport), channel_sink(tx)).unwrap();
        wait_for(&rx, |e| matches!(e, LinkEvent::Connected(_)));
        assert_eq!(peer.get(), Some(udp_board.addr));

        board.control(SimControl::Audio { playing: true, recording: false, clock_ppm: 0 });
        wait_for(&rx, |e| matches!(e, LinkEvent::AudioPlayback(..)));
        let mut keys = KeyboardState::default();
        keys.set(0x04, true);
        link.send(Msg::KeyboardState(keys));
        std::thread::sleep(Duration::from_millis(200));
        link.shutdown();
        drop(udp_board);
        let report = board.stop();
        assert!(report.keyboard_frames >= 1 && report.release_all_received, "{report:?}");
        assert!(report.hellos >= 1);
    }

    pub struct UdpBoard {
        pub addr: SocketAddr,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl UdpBoard {
        pub fn spawn(key: PairingKey, inner: Box<dyn Transport>) -> io::Result<UdpBoard> {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
            socket.set_read_timeout(Some(Duration::from_millis(1)))?;
            let addr = socket.local_addr()?;
            let (reader, writer) = inner.split()?;
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let stop = stop.clone();
                thread::spawn(move || serve(&socket, &key, reader, writer, &stop))
            };
            return Ok(UdpBoard { addr, stop, thread: Some(thread) });
        }
    }

    impl Drop for UdpBoard {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    struct HubSession {
        session: Session,
        hub_nonce: [u8; 16],
        board_nonce: [u8; 16],
        peer: SocketAddr,
    }

    fn serve(socket: &UdpSocket, key: &PairingKey, mut reader: Reader, mut writer: Writer, stop: &AtomicBool) {
        let mut hub: Option<HubSession> = None;
        let mut dgram = vec![0u8; secure::DGRAM_MAX_LEN + 64];
        let mut buf = vec![0u8; 4096];
        while !stop.load(Ordering::Relaxed) {
            if let Ok((n, from)) = socket.recv_from(&mut dgram) {
                if dgram[0] == secure::DGRAM_DATA {
                    if let Some(h) = hub.as_mut() {
                        if let Some(plain) = h.session.rx.open_in_place(&mut dgram[..n]) {
                            h.peer = from;
                            let _ = writer.write_all(plain);
                        }
                    }
                } else if let Some(hello) = DgramHello::read_from(&dgram[..n]) {
                    if hello.key_id != secure::key_id(key) {
                        continue;
                    }
                    let hub_nonce = hello.hub_nonce;
                    let board_nonce = match &hub {
                        Some(h) if h.hub_nonce == hub_nonce => h.board_nonce,
                        _ => {
                            let board_nonce = random_bytes::<16>();
                            let session = Session::for_board(key, &hub_nonce, &board_nonce);
                            hub = Some(HubSession { session, hub_nonce, board_nonce, peer: from });
                            board_nonce
                        }
                    };
                    let reply = secure::reply(key, hub_nonce, board_nonce);
                    let _ = socket.send_to(reply.as_bytes(), from);
                }
            }
            match reader.read(&mut buf) {
                Ok(n) if n > 0 => {
                    if let Some(h) = hub.as_mut() {
                        for d in seal_all(&mut h.session.tx, &buf[..n]) {
                            let _ = socket.send_to(&d, h.peer);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if is_timeout(&e) => {}
                Err(_) => break,
            }
        }
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            if let (Ok((n, _)), Some(h)) = (socket.recv_from(&mut dgram), hub.as_mut()) {
                if dgram[0] == secure::DGRAM_DATA {
                    if let Some(plain) = h.session.rx.open_in_place(&mut dgram[..n]) {
                        let _ = writer.write_all(plain);
                    }
                }
            }
        }
    }
}
