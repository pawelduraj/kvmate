//! Sound card streams through cpal, in WASAPI shared mode on Windows. So the OS mixes our
//! playback with the hub's own sound, and other programs can use the microphone at the same time.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample};
use kvmate_protocol::{AudioHeader, AudioStatus, Msg};

use super::capture::CapturePath;
use super::playback::PlaybackPath;
use super::{AudioConfig, AudioView, DeviceList, StreamView, host_gain, pick_device};
use crate::board::link::{BoardSink, LinkSender};
use crate::lock;

const CLOSE_AFTER: Duration = Duration::from_secs(3);
const RETRY_AFTER: Duration = Duration::from_secs(3);
const OPEN_TIMEOUT: Duration = Duration::from_secs(2);

struct Open<P> {
    _stream: cpal::Stream,
    path: Arc<Mutex<P>>,
    name: String,
    channels: u16,
    rate: u32,
    opened: Instant,
    failed: Arc<OnceLock<String>>,
}

type Opened<P> = Result<Open<P>, String>;

struct Slot<P> {
    open: Option<Open<P>>,
    opening: Option<Receiver<Opened<P>>>,
    idle_since: Option<Instant>,
    retry_at: Option<Instant>,
    error: Option<String>,
}

impl<P> Default for Slot<P> {
    fn default() -> Self {
        return Slot { open: None, opening: None, idle_since: None, retry_at: None, error: None };
    }
}

impl<P: Send + 'static> Slot<P> {
    fn update(&mut self, wanted: bool, open: impl FnOnce() -> Opened<P> + Send + 'static) {
        let now = Instant::now();
        self.collect_opened(now);
        if let Some(o) = self.open.take_if(|o| return o.failed.get().is_some()) {
            let why = o.failed.get().map_or("", String::as_str);
            self.error = Some(format!("{}: the device stopped working ({why})", o.name));
            self.retry_at = (o.opened.elapsed() < RETRY_AFTER).then(|| return now + RETRY_AFTER);
        }
        if !wanted {
            match self.idle_since {
                None => self.idle_since = Some(now),
                Some(t) if now - t > CLOSE_AFTER => self.close(),
                Some(_) => {}
            }
            return;
        }
        self.idle_since = None;
        if self.open.is_some() || self.opening.is_some() || self.retry_at.is_some_and(|t| return now < t) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new().name("audio-open".into()).spawn(move || {
            let _ = tx.send(open());
        });
        match spawned {
            Ok(_) => self.opening = Some(rx),
            Err(e) => {
                self.error = Some(format!("can't open the device: {e}"));
                self.retry_at = Some(now + RETRY_AFTER);
            }
        }
    }

    fn collect_opened(&mut self, now: Instant) {
        let Some(rx) = &self.opening else { return };
        let opened = match rx.try_recv() {
            Err(TryRecvError::Empty) => return,
            Ok(opened) => opened,
            Err(TryRecvError::Disconnected) => Err("can't open the device".into()),
        };
        self.opening = None;
        match opened {
            Ok(o) => {
                self.open = Some(o);
                self.error = None;
                self.retry_at = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.retry_at = Some(now + RETRY_AFTER);
            }
        }
    }

    fn close(&mut self) {
        *self = Slot::default();
    }

    fn view(&self, v: &mut StreamView, details: impl Fn(&P) -> (Option<f32>, f32)) {
        v.error = self.error.clone();
        if let Some(o) = &self.open {
            v.device = Some(format!("{} ({} Hz, {} ch)", o.name, o.rate, o.channels));
            let (buffer_ms, drift_ppm) = details(&lock(&o.path));
            v.buffer_ms = buffer_ms;
            v.drift_ppm = Some(drift_ppm);
        }
    }
}

#[derive(Default)]
pub(super) struct Devices {
    playback: Slot<PlaybackPath>,
    capture: Slot<CapturePath>,
}

impl Devices {
    pub(super) fn apply(
        &mut self,
        config: &AudioConfig,
        status: &AudioStatus,
        link: Option<&LinkSender>,
        on_laptop: bool,
    ) {
        if config.playback.allowed(on_laptop) {
            let (device, buffer_ms) = (config.output_device.clone(), config.playback_buffer());
            self.playback.update(status.host_playing != 0, move || {
                return open(Output, device.as_deref(), |rate| return PlaybackPath::new(rate, buffer_ms));
            });
            if let Some(o) = &self.playback.open {
                lock(&o.path).set_gain(host_gain(status));
            }
        } else {
            self.playback.close();
        }

        if config.capture.allowed(on_laptop) {
            let active = status.host_recording != 0;
            let (device, buffer_ms, link) = (config.input_device.clone(), config.capture_buffer(), link.cloned());
            self.capture.update(active && link.is_some(), move || {
                let Some(link) = link else { return Err("no board".into()) };
                return open(Input(link), device.as_deref(), |rate| return CapturePath::new(rate, buffer_ms));
            });
            if let Some(o) = &self.capture.open {
                lock(&o.path).set_active(active);
            }
        } else {
            self.capture.close();
        }
    }

    pub(super) fn on_board_report(&mut self, status: &AudioStatus) {
        if let Some(o) = &self.capture.open {
            lock(&o.path).on_board_fill(status.mic_queued_frames, Instant::now());
        }
    }

    pub(super) fn push_playback(&mut self, head: &AudioHeader, pcm: &[u8]) {
        if let Some(o) = &self.playback.open {
            lock(&o.path).push(head, pcm, Instant::now());
        }
    }

    pub(super) fn fill_view(&self, v: &mut AudioView) {
        self.playback.view(&mut v.playback, |p| return (p.level_ms(), p.drift_ppm()));
        self.capture.view(&mut v.capture, |p| return (p.level_ms(), p.drift_ppm()));
    }

    pub(super) fn close(&mut self, playback: bool, capture: bool) {
        if playback {
            self.playback.close();
        }
        if capture {
            self.capture.close();
        }
    }
}

pub(super) fn list() -> DeviceList {
    fn names(devices: impl Iterator<Item = cpal::Device>) -> Vec<String> {
        return devices.map(|d| return d.to_string()).collect();
    }
    let host = cpal::default_host();
    return DeviceList {
        outputs: host.output_devices().map(names).unwrap_or_default(),
        inputs: host.input_devices().map(names).unwrap_or_default(),
        default_output: host.default_output_device().map(|d| return d.to_string()),
        default_input: host.default_input_device().map(|d| return d.to_string()),
    };
}

fn find_device(input: bool, wanted: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    let kind = if input { "microphone" } else { "sound output" };
    match wanted {
        None => {
            return if input { host.default_input_device() } else { host.default_output_device() }
                .ok_or_else(|| format!("no default device ({kind})"));
        }
        Some(wanted) => {
            let devices = if input { host.input_devices() } else { host.output_devices() }
                .map_err(|e| format!("can't list the devices ({kind}): {e}"))?;
            return pick_device(devices.map(|d| return (d.to_string(), d)), wanted)
                .ok_or_else(|| format!("device “{wanted}” not found ({kind})"));
        }
    }
}

fn open<D: Direction>(
    dir: D,
    wanted: Option<&str>,
    new_path: impl FnOnce(u32) -> D::Path,
) -> Result<Open<D::Path>, String> {
    let device = find_device(D::INPUT, wanted)?;
    let name = device.to_string();
    let supported = if D::INPUT { device.default_input_config() } else { device.default_output_config() }
        .map_err(|e| format!("{name}: {e}"))?;
    let config = supported.config();
    let path = Arc::new(Mutex::new(new_path(config.sample_rate)));
    let failed = Arc::new(OnceLock::new());
    let (p, f) = (path.clone(), failed.clone());
    let stream = match supported.sample_format() {
        SampleFormat::F32 => dir.build::<f32>(&device, &config, p, f),
        SampleFormat::I16 => dir.build::<i16>(&device, &config, p, f),
        SampleFormat::I32 => dir.build::<i32>(&device, &config, p, f),
        SampleFormat::U16 => dir.build::<u16>(&device, &config, p, f),
        SampleFormat::F64 => dir.build::<f64>(&device, &config, p, f),
        other => return Err(format!("{name}: unsupported sample format {other:?}")),
    }
    .map_err(|e| format!("{name}: {e}"))?;
    stream.play().map_err(|e| format!("{name}: {e}"))?;
    let (channels, rate, opened) = (config.channels, config.sample_rate, Instant::now());
    return Ok(Open { _stream: stream, path, name, channels, rate, opened, failed });
}

fn on_error(failed: Arc<OnceLock<String>>) -> impl FnMut(cpal::Error) + Send + 'static {
    return move |e: cpal::Error| {
        if !matches!(e.kind(), ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied) {
            let _ = failed.set(e.to_string());
        }
    };
}

trait Direction: Send + 'static {
    type Path: Send + 'static;
    const INPUT: bool;

    fn build<T>(
        self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        path: Arc<Mutex<Self::Path>>,
        failed: Arc<OnceLock<String>>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + FromSample<f32>,
        f32: FromSample<T>;
}

struct Output;

struct Input(LinkSender);

impl Direction for Output {
    type Path = PlaybackPath;
    const INPUT: bool = false;

    fn build<T>(
        self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        path: Arc<Mutex<PlaybackPath>>,
        failed: Arc<OnceLock<String>>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + FromSample<f32>,
        f32: FromSample<T>,
    {
        let channels = config.channels as usize;
        let mut scratch: Vec<f32> = Vec::new();
        return device.build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                lock(&path).render(&mut scratch, channels, Instant::now());
                for (dst, &src) in data.iter_mut().zip(&scratch) {
                    *dst = T::from_sample(src);
                }
            },
            on_error(failed),
            Some(OPEN_TIMEOUT),
        );
    }
}

impl Direction for Input {
    type Path = CapturePath;
    const INPUT: bool = true;

    fn build<T>(
        self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        path: Arc<Mutex<CapturePath>>,
        failed: Arc<OnceLock<String>>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + FromSample<f32>,
        f32: FromSample<T>,
    {
        let Input(link) = self;
        let channels = config.channels as usize;
        let mut scratch: Vec<f32> = Vec::new();
        return device.build_input_stream::<T, _, _>(
            *config,
            move |data: &[T], _| {
                scratch.clear();
                scratch.extend(data.iter().map(|&s| return s.to_sample::<f32>()));
                let mut p = lock(&path);
                p.push_input(&scratch, channels, Instant::now());
                while let Some((head, pcm)) = p.next_chunk() {
                    link.send(Msg::AudioCapture(head, &pcm));
                }
            },
            on_error(failed),
            Some(OPEN_TIMEOUT),
        );
    }
}
