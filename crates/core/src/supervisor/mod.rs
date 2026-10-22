//! The core thread. Board events, captured input and GUI commands all arrive on one channel.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::AudioEngine;
use crate::audio::activity::{ActivityWatch, LocalActivity};
use crate::board::link::LinkEvent;
use crate::config::Config;
use crate::firmware::FirmwareImage;
use crate::input::{self, CaptureControl, InputEvent};
use crate::logfile::Level;
use crate::switcher::{Combo, Request, Switcher, Target};
use crate::wifi::WifiSetup;

mod commands;
mod connect;
mod flashing;
mod session;
mod status;
#[cfg(test)]
pub(crate) mod tests;
mod wifi;

use flashing::FlashUpdate;
use session::Session;
pub use status::*;

const RESCAN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub enum Mode {
    Auto { port: Option<String> },
    Demo,
}

pub struct Options {
    pub mode: Mode,
    pub config: Config,
    pub capture: bool,
    pub notes: Vec<String>,
    pub firmware: Option<FirmwareImage>,
}

pub trait Observer: Send + 'static {
    fn status_changed(&self, status: &BoardStatus);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    SetTarget(Target),
    ApplyConfig(Box<Config>),
    InstallFirmware { port: Option<String> },
    EraseFirmware { port: Option<String> },
    SendCombo(Combo),
    WifiApply(WifiSetup),
    WifiScan,
}

enum CoreEvent {
    Link(u64, LinkEvent),
    Input(InputEvent),
    Command(Command),
    Flash(FlashUpdate),
    LocalAudio(LocalActivity),
}

pub struct Supervisor {
    stop: Arc<AtomicBool>,
    tx: Sender<CoreEvent>,
    thread: Option<JoinHandle<()>>,
}

impl Supervisor {
    pub fn start(options: Options, observer: impl Observer) -> Supervisor {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread = {
            let stop = stop.clone();
            let tx = tx.clone();
            thread::Builder::new()
                .name("core".into())
                .spawn(move || Core::new(options, Box::new(observer), tx, rx, stop).run())
                .expect("spawn the core thread")
        };
        return Supervisor { stop, tx, thread: Some(thread) };
    }

    pub fn command(&self, command: Command) {
        let _ = self.tx.send(CoreEvent::Command(command));
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Core {
    mode: Mode,
    observer: Box<dyn Observer>,
    tx: Sender<CoreEvent>,
    rx: Receiver<CoreEvent>,
    stop: Arc<AtomicBool>,
    status: BoardStatus,
    switcher: Switcher,
    audio: AudioEngine,
    config: Config,
    control: Arc<CaptureControl>,
    live: bool,
    capture: Option<input::Capture>,
    activity: Option<ActivityWatch>,
    link_id: u64,
    firmware: Option<FirmwareImage>,
    pending_flash: Option<(String, FlashKind)>,
    flashing: Option<JoinHandle<()>>,
    demo_build: [u8; 8],
    wifi_miss_logged: bool,
    ports_seen: Vec<String>,
    port_errors: HashMap<String, String>,
    wifi_scan_sent: Instant,
}

impl Core {
    fn new(
        options: Options,
        observer: Box<dyn Observer>,
        tx: Sender<CoreEvent>,
        rx: Receiver<CoreEvent>,
        stop: Arc<AtomicBool>,
    ) -> Core {
        let demo = matches!(options.mode, Mode::Demo);
        let mut status = BoardStatus { demo, ..Default::default() };
        for note in options.notes {
            status.log(Level::Info, note);
        }
        if demo {
            status.log(Level::Info, "demo mode: simulated board; keyboard and mouse are not captured");
        }

        let control = CaptureControl::new(!demo, options.config.hotkey.usage());
        status.hotkey = options.config.hotkey.label().to_string();
        let capture = if options.capture {
            let sink_tx = tx.clone();
            let sink: input::InputSink = Arc::new(move |e| {
                let _ = sink_tx.send(CoreEvent::Input(e));
            });
            match input::start_capture(control.clone(), sink) {
                Ok(c) => Some(c),
                Err(e) => {
                    status.capture_error = Some(e.to_string());
                    status.log(Level::Error, format!("keyboard and mouse capture unavailable: {e}"));
                    None
                }
            }
        } else {
            status.capture_error = Some("off".into());
            None
        };

        let audio = AudioEngine::new(options.config.audio.clone());
        let activity = options.capture.then(|| {
            let tx = tx.clone();
            let a = &options.config.audio;
            return ActivityWatch::start(a.output_device.clone(), a.input_device.clone(), move |local| {
                let _ = tx.send(CoreEvent::LocalAudio(local));
            });
        });
        status.bundled_firmware = options.firmware.as_ref().map(|f| return f.info.clone());

        return Core {
            mode: options.mode,
            audio,
            switcher: Switcher::new(control.clone(), &options.config),
            config: options.config,
            observer,
            control,
            tx,
            rx,
            stop,
            status,
            live: options.capture,
            capture,
            activity,
            link_id: 0,
            firmware: options.firmware,
            pending_flash: None,
            flashing: None,
            demo_build: crate::board::sim::SIM_BUILD_ID,
            wifi_miss_logged: false,
            ports_seen: Vec::new(),
            port_errors: HashMap::new(),
            wifi_scan_sent: Instant::now(),
        };
    }

    fn stopped(&self) -> bool {
        return self.stop.load(Ordering::Relaxed);
    }

    fn publish(&mut self) {
        self.status.audio = self.audio.view();
        self.observer.status_changed(&self.status);
    }

    fn note(&mut self, level: Level, text: impl Into<String>) {
        self.status.log(level, text);
        self.publish();
    }

    fn run(mut self) {
        self.forget_board_network(&self.config.wifi);
        self.publish();
        while !self.stopped() {
            if self.pending_flash.is_none() && self.flashing.is_none() {
                self.scan();
            }
            if let Some((port, kind)) = self.pending_flash.take() {
                self.start_flash(port, kind);
            }
            let until = Instant::now() + RESCAN_INTERVAL;
            while (Instant::now() < until || self.flashing.is_some()) && !self.stopped() {
                if let Ok(event) = self.rx.recv_timeout(Duration::from_millis(50)) {
                    let _ = self.on_event(event, None);
                }
                if self.pending_flash.is_some() && self.flashing.is_none() {
                    break;
                }
            }
        }
        while self.flashing.is_some() {
            match self.rx.recv() {
                Ok(CoreEvent::Flash(update)) => self.on_flash(update),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        self.forget_board_network(&self.config.wifi);
    }

    fn on_event(&mut self, event: CoreEvent, session: Option<&mut Session>) -> ControlFlow<()> {
        let mut flow = ControlFlow::Continue(());
        match event {
            CoreEvent::Link(id, event) => match session {
                Some(s) if s.id == id => flow = self.on_link_event(event, s),
                _ => {}
            },
            CoreEvent::Input(InputEvent::Hotkey { back_here }) => {
                let request = if back_here { Request::Set(Target::Pc) } else { Request::Toggle };
                self.request(request, session.as_deref());
            }
            CoreEvent::Input(ev) => {
                if let Some(s) = session {
                    if let Some(t) = self.switcher.handle_input(ev, s.link.board()) {
                        self.target_changed(t);
                    }
                }
            }
            CoreEvent::Command(command) => flow = self.on_command(command, session.as_deref()),
            CoreEvent::Flash(update) => self.on_flash(update),
            CoreEvent::LocalAudio(local) => {
                self.audio.set_local(local);
                self.publish();
            }
        }
        if let Some(text) = self.switcher.take_notice() {
            self.note(Level::Warn, text);
        }
        return flow;
    }
}
