//! Audio between the hub and the controlled computer.
//!
//! - Playback: the controlled computer's audio arrives from the board (48 kHz stereo) and is
//!   played on the hub's headphones.
//! - Capture: the hub's microphone goes to the board (48 kHz mono).
//!
//! Devices are opened only while the controlled computer actually plays or records, so the OS
//! does not show the microphone as permanently in use.

pub mod activity;
pub mod capture;
mod config;
pub mod drift;
pub mod playback;

#[cfg(feature = "audio-devices")]
mod device;
#[cfg(not(feature = "audio-devices"))]
mod no_device;
#[cfg(not(feature = "audio-devices"))]
use no_device as device;

pub use config::{AudioConfig, StreamMode};

use kvmate_protocol::{AudioHeader, AudioStatus};
use serde::Serialize;

use crate::board::link::LinkSender;

pub const RATE: u32 = kvmate_protocol::AUDIO_RATE_HZ;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StreamView {
    pub active: bool,
    pub peer_active: bool,
    pub device: Option<String>,
    pub buffer_ms: Option<f32>,
    pub drift_ppm: Option<f32>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AudioView {
    pub playback_mode: StreamMode,
    pub capture_mode: StreamMode,
    pub playback: StreamView,
    pub capture: StreamView,
    pub host_volume_percent: Option<u8>,
    pub here_playing: bool,
    pub here_recording: bool,
}

impl AudioView {
    fn discrete(&self) -> impl PartialEq + use<> {
        let s = |v: &StreamView| return (v.active, v.peer_active, v.device.clone(), v.error.clone());
        return (
            self.playback_mode,
            self.capture_mode,
            s(&self.playback),
            s(&self.capture),
            self.host_volume_percent,
            (self.here_playing, self.here_recording),
        );
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DeviceList {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
}

pub fn list_devices() -> DeviceList {
    return device::list();
}

pub(crate) fn pick_device<D>(devices: impl Iterator<Item = (String, D)>, wanted: &str) -> Option<D> {
    let wanted = wanted.to_lowercase();
    let mut partial = None;
    for (name, device) in devices {
        let name = name.to_lowercase();
        if name == wanted {
            return Some(device);
        }
        if partial.is_none() && name.contains(&wanted) {
            partial = Some(device);
        }
    }
    return partial;
}

const HOST_VOLUME_MIN_DB: f32 = -60.0;

pub fn slider_percent(db: f32) -> u8 {
    if db <= HOST_VOLUME_MIN_DB + 0.01 {
        return 0;
    }
    return (100.0 * 10f32.powf(db.min(0.0) / 34.6)).round().clamp(1.0, 100.0) as u8;
}

fn host_volume_db(status: &AudioStatus) -> Option<f32> {
    return (status.host_muted == 0).then(|| return status.host_volume as f32 / 256.0);
}

pub fn host_gain(status: &AudioStatus) -> f32 {
    return host_volume_db(status).map_or(0.0, |db| return 10f32.powf(db / 20.0));
}

pub struct AudioEngine {
    config: AudioConfig,
    status: AudioStatus,
    link: Option<LinkSender>,
    on_laptop: bool,
    local: activity::LocalActivity,
    devices: device::Devices,
}

impl AudioEngine {
    pub fn new(config: AudioConfig) -> AudioEngine {
        return AudioEngine {
            config,
            status: AudioStatus::default(),
            link: None,
            on_laptop: false,
            local: activity::LocalActivity::default(),
            devices: Default::default(),
        };
    }

    pub fn set_local(&mut self, local: activity::LocalActivity) {
        self.local = local;
    }

    pub fn view(&self) -> AudioView {
        let mut v = AudioView {
            playback_mode: self.config.playback,
            capture_mode: self.config.capture,
            host_volume_percent: host_volume_db(&self.status).map(slider_percent),
            here_playing: self.local.playing,
            here_recording: self.local.recording,
            ..Default::default()
        };
        v.playback.peer_active = self.status.host_playing != 0;
        v.capture.peer_active = self.status.host_recording != 0;
        v.playback.active = v.playback.peer_active && self.config.playback.allowed(self.on_laptop);
        v.capture.active = v.capture.peer_active && self.config.capture.allowed(self.on_laptop);
        self.devices.fill_view(&mut v);
        return v;
    }

    pub fn set_config(&mut self, config: AudioConfig) {
        let old = &self.config;
        let playback =
            (&config.output_device, config.playback_buffer_ms) != (&old.output_device, old.playback_buffer_ms);
        let capture = (&config.input_device, config.capture_buffer_ms) != (&old.input_device, old.capture_buffer_ms);
        self.devices.close(playback, capture);
        self.config = config;
        self.apply();
    }

    pub fn set_target(&mut self, on_laptop: bool) {
        if self.on_laptop != on_laptop {
            self.on_laptop = on_laptop;
            self.apply();
        }
    }

    fn apply(&mut self) {
        self.devices.apply(&self.config, &self.status, self.link.as_ref(), self.on_laptop);
    }

    pub fn busy(&self) -> bool {
        return self.status.host_playing != 0 || self.status.host_recording != 0;
    }

    pub fn set_link(&mut self, link: Option<LinkSender>) {
        if link.is_none() {
            self.stop();
        }
        self.link = link;
    }

    pub fn on_status(&mut self, status: AudioStatus) -> bool {
        let before = self.view().discrete();
        self.status = status;
        self.apply();
        self.devices.on_board_report(&status);
        return self.view().discrete() != before;
    }

    pub fn on_playback(&mut self, head: &AudioHeader, pcm: &[u8]) {
        self.devices.push_playback(head, pcm);
    }

    fn stop(&mut self) {
        self.devices.close(true, true);
        self.status = AudioStatus::default();
    }
}

#[cfg(test)]
mod volume_tests {
    use super::*;

    #[test]
    fn volume_like_the_windows_slider() {
        assert_eq!(slider_percent(0.0), 100);
        assert_eq!(slider_percent(-10.42), 50);
        assert_eq!(slider_percent(-60.0), 0);
        assert!(slider_percent(-59.0) >= 1);
        assert!(slider_percent(-20.0) < slider_percent(-10.0));
    }

    #[test]
    fn device_by_name() {
        let devices =
            || return ["Headset 2 (USB)", "Headset", "Speakers"].into_iter().map(|n| return (n.to_string(), n));
        assert_eq!(pick_device(devices(), "headset"), Some("Headset"));
        assert_eq!(pick_device(devices(), "usb"), Some("Headset 2 (USB)"));
        assert_eq!(pick_device(devices(), "hdmi"), None);
    }
}

#[cfg(all(test, feature = "audio-devices"))]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::board::link::{Link, LinkEvent, channel_sink};
    use crate::board::sim::{SimBoard, SimControl};

    #[test]
    #[ignore]
    fn live_audio_devices_with_sim_board() {
        let (hub_end, board) = SimBoard::pair("sim");
        let (tx, rx) = mpsc::channel();
        let link = Link::start(Box::new(hub_end), channel_sink(tx)).unwrap();
        let mut engine = AudioEngine::new(AudioConfig::default());
        board.control(SimControl::Volume { volume: 0, mute: true });

        let seconds = 20;
        let start = Instant::now();
        let mut started = false;
        while start.elapsed() < Duration::from_secs(seconds) {
            let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) else { continue };
            match ev {
                LinkEvent::Connected(_) => {
                    engine.set_link(Some(link.sender()));
                    if !started {
                        board.control(SimControl::Audio { playing: true, recording: true, clock_ppm: 300 });
                        started = true;
                    }
                }
                LinkEvent::AudioStatus(s) => {
                    if engine.on_status(s) {
                        println!("{:?}", engine.view());
                    }
                }
                LinkEvent::AudioPlayback(head, pcm) => engine.on_playback(&head, &pcm),
                _ => {}
            }
        }
        let view = engine.view();
        println!("{view:#?}");
        engine.stop();
        link.shutdown();
        let report = board.stop();
        let late: Vec<_> = report.capture_fill_trace.iter().filter(|(t, _)| return *t > seconds as f32 / 2.0).collect();
        let min = late.iter().map(|(_, f)| return *f).min().unwrap_or(0);
        let max = late.iter().map(|(_, f)| return *f).max().unwrap_or(0);
        let last_empty = report
            .capture_fill_trace
            .iter()
            .filter(|(_, f)| return *f == 0)
            .map(|(t, _)| return *t)
            .fold(0.0, f32::max);
        println!(
            "board microphone queue (second half): {min}..{max} frames, underrun {} frames (last empty at {last_empty:.2} s), overflow {}",
            report.capture_underrun_frames, report.capture_overflow_frames
        );

        assert_eq!(view.playback.error, None);
        assert_eq!(view.capture.error, None);
        assert!(view.playback.device.is_some() && view.capture.device.is_some());
        let buffer = view.playback.buffer_ms.unwrap();
        assert!((15.0..60.0).contains(&buffer), "playback buffer {buffer} ms");
        assert!(min > 100 && max < 2400, "board microphone queue {min}..{max}");
        assert!(last_empty < 1.0, "microphone ran dry after the start-up");
    }
}
