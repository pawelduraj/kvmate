//! A simulated board that speaks the real protocol over any [`Transport`], for tests and the demo
//! mode. Its audio clock can run a little fast or slow, to test how the hub corrects for that.

use std::f32::consts::PI;
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use kvmate_protocol::{
    self as proto, AudioHeader, AudioStatus, ButtonEvent, DeviceInfo, FrameDecoder, Heartbeat, LogHeader, Msg, MsgType,
    RawData,
};

use crate::board::memory::MemoryTransport;
use crate::board::transport::{Transport, Writer, is_timeout};

const PLAY_CHUNK_FRAMES: u64 = 192;
const MIC_START_FRAMES: u32 = 480;
const MIC_CAPACITY: u32 = 3072;
const STATUS_INTERVAL: Duration = Duration::from_millis(20);
pub const SIM_BUILD_ID: [u8; 8] = *b"simboard";

#[cfg_attr(not(test), allow(dead_code))]
pub enum SimControl {
    PressButton(u8),
    Mute(bool),
    Restart,
    Audio { playing: bool, recording: bool, clock_ppm: i32 },
    Volume { volume: i16, mute: bool },
}

#[derive(Debug, Default)]
pub struct SimReport {
    pub hellos: u32,
    pub keyboard_frames: u32,
    pub release_all_received: bool,
    pub playback_frames_sent: u64,
    pub capture_frames_received: u64,
    pub capture_underrun_frames: u64,
    pub capture_overflow_frames: u64,
    pub capture_fill_trace: Vec<(f32, u32)>,
}

pub struct SimBoard {
    ctl: Sender<Option<SimControl>>,
    thread: Option<JoinHandle<SimReport>>,
}

impl SimBoard {
    #[cfg(test)]
    pub fn spawn(transport: Box<dyn Transport>) -> SimBoard {
        return SimBoard::spawn_with_build(transport, SIM_BUILD_ID);
    }

    fn spawn_with_build(transport: Box<dyn Transport>, build_id: [u8; 8]) -> SimBoard {
        let (ctl, ctl_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("sim-board".into())
            .spawn(move || {
                let mut report = SimReport::default();
                let _ = run(transport, ctl_rx, build_id, &mut report);
                return report;
            })
            .expect("spawn simulated board");
        return SimBoard { ctl, thread: Some(thread) };
    }

    #[cfg(test)]
    pub fn pair(name: &str) -> (MemoryTransport, SimBoard) {
        return SimBoard::pair_with_build(name, SIM_BUILD_ID);
    }

    pub fn pair_with_build(name: &str, build_id: [u8; 8]) -> (MemoryTransport, SimBoard) {
        let (hub_end, board_end) = MemoryTransport::pair(name);
        let board_end = board_end.with_read_timeout(Duration::from_millis(1));
        return (hub_end, SimBoard::spawn_with_build(Box::new(board_end), build_id));
    }

    #[cfg(test)]
    pub fn control(&self, c: SimControl) {
        let _ = self.ctl.send(Some(c));
    }

    #[cfg(test)]
    pub fn stop(mut self) -> SimReport {
        let _ = self.ctl.send(None);
        return self.thread.take().and_then(|t| return t.join().ok()).unwrap_or_default();
    }
}

impl Drop for SimBoard {
    fn drop(&mut self) {
        let _ = self.ctl.send(None);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Audio {
    playing: bool,
    recording: bool,
    clock: f64,
    play_start: Instant,
    played: u64,
    phase: f32,
    rec_start: Instant,
    rec_last: Instant,
    rec_frac: f64,
    mic_fifo: u32,
    volume: i16,
    mute: bool,
    last_status: Instant,
}

impl Audio {
    fn new() -> Audio {
        let now = Instant::now();
        return Audio {
            playing: false,
            recording: false,
            clock: 1.0,
            play_start: now,
            played: 0,
            phase: 0.0,
            rec_start: now,
            rec_last: now,
            rec_frac: 0.0,
            mic_fifo: 0,
            volume: 0,
            mute: false,
            last_status: now,
        };
    }

    fn set(&mut self, playing: bool, recording: bool, clock_ppm: i32) {
        let now = Instant::now();
        self.clock = 1.0 + clock_ppm as f64 * 1e-6;
        if playing && !self.playing {
            self.play_start = now;
            self.played = 0;
        }
        if recording && !self.recording {
            self.rec_start = now;
            self.rec_last = now;
            self.rec_frac = 0.0;
            self.mic_fifo = MIC_START_FRAMES;
        }
        self.playing = playing;
        self.recording = recording;
    }

    fn on_capture(&mut self, frames: u32, report: &mut SimReport) {
        if !self.recording {
            return;
        }
        report.capture_frames_received += frames as u64;
        let room = MIC_CAPACITY - self.mic_fifo;
        report.capture_overflow_frames += frames.saturating_sub(room) as u64;
        self.mic_fifo += frames.min(room);
    }

    fn advance(&mut self, report: &mut SimReport) -> Vec<(AudioHeader, Vec<u8>)> {
        let now = Instant::now();
        if self.recording {
            self.rec_frac += now.duration_since(self.rec_last).as_secs_f64() * proto::AUDIO_RATE_HZ as f64 * self.clock;
            self.rec_last = now;
            let wanted = self.rec_frac as u32;
            self.rec_frac -= wanted as f64;
            report.capture_underrun_frames += wanted.saturating_sub(self.mic_fifo) as u64;
            self.mic_fifo = self.mic_fifo.saturating_sub(wanted);
        }
        let mut packets = Vec::new();
        if self.playing {
            let due = (self.play_start.elapsed().as_secs_f64() * proto::AUDIO_RATE_HZ as f64 * self.clock) as u64;
            while self.played + PLAY_CHUNK_FRAMES <= due {
                let head = AudioHeader { first_frame_number: self.played as u32 };
                let mut pcm = Vec::with_capacity(PLAY_CHUNK_FRAMES as usize * 4);
                for _ in 0..PLAY_CHUNK_FRAMES {
                    let s = ((self.phase.sin() * 3000.0) as i16).to_le_bytes();
                    self.phase = (self.phase + 2.0 * PI * 440.0 / proto::AUDIO_RATE_HZ as f32) % (2.0 * PI);
                    pcm.extend_from_slice(&s);
                    pcm.extend_from_slice(&s);
                }
                packets.push((head, pcm));
                self.played += PLAY_CHUNK_FRAMES;
                report.playback_frames_sent += PLAY_CHUNK_FRAMES;
            }
        }
        return packets;
    }

    fn status_due(&mut self) -> Option<AudioStatus> {
        if self.last_status.elapsed() < STATUS_INTERVAL {
            return None;
        }
        self.last_status += STATUS_INTERVAL;
        if self.last_status.elapsed() > STATUS_INTERVAL {
            self.last_status = Instant::now();
        }
        return Some(AudioStatus {
            host_playing: self.playing as u8,
            host_recording: self.recording as u8,
            mic_queued_frames: self.mic_fifo as u16,
            host_volume: self.volume,
            host_muted: self.mute as u8,
        });
    }
}

fn run(
    transport: Box<dyn Transport>,
    ctl: Receiver<Option<SimControl>>,
    build_id: [u8; 8],
    report: &mut SimReport,
) -> io::Result<()> {
    let (mut reader, mut writer) = transport.split()?;
    let mut decoder = FrameDecoder::new();
    let mut wire = vec![0u8; proto::MAX_ENCODED_LEN];
    let mut seq: u16 = 0;
    let mut last_heartbeat = Instant::now();
    let mut muted = false;
    let mut session = false;
    let mut audio = Audio::new();
    let mut buf = [0u8; 2048];
    let mut wifi = proto::WifiConfig::default();
    let mut last_wifi_status = Instant::now();

    let mut send = |writer: &mut Writer, msg: Msg<'_>| -> io::Result<()> {
        let n = msg.encode(seq, &mut wire).map_err(|e| return io::Error::other(format!("{e:?}")))?;
        seq = seq.wrapping_add(1);
        return writer.write_all(&wire[..n]);
    };

    loop {
        match ctl.try_recv() {
            Ok(Some(SimControl::PressButton(kind))) => {
                let ev = ButtonEvent { kind };
                send(&mut writer, Msg::ButtonEvent(ev))?;
            }
            Ok(Some(SimControl::Mute(m))) => muted = m,
            Ok(Some(SimControl::Restart)) => {
                session = false;
                audio = Audio::new();
            }
            Ok(Some(SimControl::Audio { playing, recording, clock_ppm })) => audio.set(playing, recording, clock_ppm),
            Ok(Some(SimControl::Volume { volume, mute })) => {
                audio.volume = volume;
                audio.mute = mute;
            }
            Ok(None) | Err(TryRecvError::Disconnected) => return Ok(()),
            Err(TryRecvError::Empty) => {}
        }

        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(e) if is_timeout(&e) => 0,
            Err(_) => return Ok(()),
        };
        let mut replies: Vec<MsgType> = Vec::new();
        decoder.push(&buf[..n], |frame| match Msg::parse(&frame) {
            Some(Msg::Hello) => {
                report.hellos += 1;
                replies.push(MsgType::DeviceInfo);
            }
            Some(Msg::KeyboardState(_)) => report.keyboard_frames += 1,
            Some(Msg::ReleaseAll) => report.release_all_received = true,
            Some(Msg::AudioCapture(_, pcm)) => {
                let frames = pcm.len() / (2 * proto::AUDIO_CAPTURE_CHANNELS as usize);
                audio.on_capture(frames as u32, report);
            }
            Some(Msg::WifiConfig(c)) => {
                wifi = c;
                replies.push(MsgType::WifiStatus);
            }
            Some(Msg::WifiScan) => replies.push(MsgType::WifiScanResult),
            _ => {}
        });
        if muted {
            continue;
        }
        for reply in replies {
            match reply {
                MsgType::DeviceInfo => {
                    send(&mut writer, Msg::DeviceInfo(device_info(build_id)))?;
                    session = true;
                    send(&mut writer, Msg::Log(LogHeader { level: proto::LOG_INFO }, b"simulated board ready"))?;
                    send(&mut writer, Msg::WifiStatus(sim_wifi_status(&wifi)))?;
                }
                MsgType::WifiStatus => send(&mut writer, Msg::WifiStatus(sim_wifi_status(&wifi)))?,
                MsgType::WifiScanResult => {
                    let (head, body) = scan_result();
                    send(&mut writer, Msg::WifiScanResult(head, &body))?;
                }
                _ => {}
            }
        }
        if session {
            for (head, pcm) in audio.advance(report) {
                send(&mut writer, Msg::AudioPlayback(head, &pcm))?;
            }
            if let Some(status) = audio.status_due() {
                if audio.recording {
                    report.capture_fill_trace.push((audio.rec_start.elapsed().as_secs_f32(), audio.mic_fifo));
                }
                send(&mut writer, Msg::AudioStatus(status))?;
            }
            if last_wifi_status.elapsed() >= Duration::from_millis(proto::WIFI_STATUS_INTERVAL_MS as u64) {
                send(&mut writer, Msg::WifiStatus(sim_wifi_status(&wifi)))?;
                last_wifi_status = Instant::now();
            }
        }
        if last_heartbeat.elapsed() >= Duration::from_millis(proto::HEARTBEAT_INTERVAL_MS as u64) {
            let session_flag = if session { proto::HEARTBEAT_FLAG_HELLO_ANSWERED } else { 0 };
            let flags = session_flag | proto::HEARTBEAT_FLAG_USB_MOUNTED;
            let hb = Heartbeat { flags };
            send(&mut writer, Msg::Heartbeat(hb))?;
            last_heartbeat = Instant::now();
        }
    }
}

fn device_info(build_id: [u8; 8]) -> DeviceInfo {
    return DeviceInfo {
        firmware_version: crate::hub_version(),
        mac_address: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
        selftest_passed: 1,
        firmware_build_id: build_id,
    };
}

fn scan_result() -> (proto::WifiScanHeader, Vec<u8>) {
    let nets = [sim_network(b"Home", -48, 6), sim_network(b"Home_5G_only", -61, 11), sim_network(b"Neighbour", -80, 1)];
    let head = proto::WifiScanHeader { network_count: nets.len() as u8 };
    return (head, nets.iter().flat_map(|n| return n.as_bytes().to_vec()).collect());
}

fn sim_network(ssid: &[u8], rssi: i8, channel: u8) -> proto::WifiNetwork {
    let mut n = proto::WifiNetwork {
        rssi_dbm: rssi,
        channel,
        auth: proto::WIFI_AUTH_PERSONAL,
        ssid_len: ssid.len() as u8,
        ssid: [0; 32],
    };
    n.ssid[..ssid.len()].copy_from_slice(ssid);
    return n;
}

fn sim_wifi_status(c: &proto::WifiConfig) -> proto::WifiStatus {
    let on = c.mode != proto::WIFI_MODE_OFF;
    return proto::WifiStatus {
        mode: c.mode,
        state: if on { proto::WIFI_STATE_READY } else { proto::WIFI_STATE_OFF },
        rssi_dbm: if on { -50 } else { 0 },
        channel: if on { 6 } else { 0 },
        ssid_len: c.ssid_len,
        ip_address: match c.mode {
            proto::WIFI_MODE_STATION => [192, 168, 1, 57],
            proto::WIFI_MODE_ACCESS_POINT => [192, 168, 4, 1],
            _ => [0; 4],
        },
        ssid: c.ssid,
        ..Default::default()
    };
}
