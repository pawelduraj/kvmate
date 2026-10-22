//! Whether this computer itself plays to the headphones or records from the microphone
//! (other programs, not kvmate).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::platform::audio_activity::Probe;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalActivity {
    pub playing: bool,
    pub recording: bool,
}

const POLL: Duration = Duration::from_millis(250);
const HOLD: Duration = Duration::from_millis(1500);
const RESOLVE_EVERY: Duration = Duration::from_secs(5);

type DeviceNames = (Option<String>, Option<String>);

pub struct ActivityWatch {
    devices: Arc<Mutex<DeviceNames>>,
    stop: Arc<AtomicBool>,
}

impl ActivityWatch {
    pub fn start(
        output: Option<String>,
        input: Option<String>,
        on_change: impl Fn(LocalActivity) + Send + 'static,
    ) -> ActivityWatch {
        let devices = Arc::new(Mutex::new((output, input)));
        let stop = Arc::new(AtomicBool::new(false));
        let (d, s) = (devices.clone(), stop.clone());
        let _ = thread::Builder::new().name("audio-activity".into()).spawn(move || watch(d, s, on_change));
        return ActivityWatch { devices, stop };
    }

    pub fn set_devices(&self, output: Option<String>, input: Option<String>) {
        *self.devices.lock().unwrap() = (output, input);
    }
}

impl Drop for ActivityWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn watch(devices: Arc<Mutex<DeviceNames>>, stop: Arc<AtomicBool>, on_change: impl Fn(LocalActivity)) {
    let Some(mut probe) = Probe::new() else { return };
    let mut shown = LocalActivity::default();
    let mut last_play: Option<Instant> = None;
    let mut last_rec: Option<Instant> = None;
    let mut resolved: Option<(DeviceNames, Instant)> = None;
    while !stop.load(Ordering::Relaxed) {
        let wanted = devices.lock().unwrap().clone();
        if resolved.as_ref().is_none_or(|(d, at)| return *d != wanted || at.elapsed() >= RESOLVE_EVERY) {
            probe.resolve(wanted.0.as_deref(), wanted.1.as_deref());
            resolved = Some((wanted, Instant::now()));
        }
        let now = Instant::now();
        let sample = probe.sample();
        if sample.playing {
            last_play = Some(now);
        }
        if sample.recording {
            last_rec = Some(now);
        }
        let held = |t: Option<Instant>| return t.is_some_and(|t| return now.duration_since(t) < HOLD);
        let current = LocalActivity { playing: held(last_play), recording: held(last_rec) };
        if current != shown {
            shown = current;
            on_change(current);
        }
        thread::sleep(POLL);
    }
}
