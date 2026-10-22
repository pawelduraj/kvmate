use std::ops::ControlFlow;
use std::panic::{self, AssertUnwindSafe};
use std::thread;
use std::time::{Duration, Instant};

use super::session::Session;
use super::{Core, CoreEvent, FlashKind, FlashOutcome, FlashView, Mode, TransportKind};
use crate::firmware::{self, FlashJob, FlashProgress, FlashStage};
use crate::logfile::Level;

const FLASH_REPORT_INTERVAL: Duration = Duration::from_millis(100);

pub(super) enum FlashUpdate {
    Progress(FlashStage, Option<f32>),
    Done(Result<String, String>),
}

impl Core {
    pub(super) fn flash_command(
        &mut self,
        port: Option<String>,
        kind: FlashKind,
        session: Option<&Session>,
    ) -> ControlFlow<()> {
        if self.flashing.is_some() {
            self.note(Level::Warn, "a firmware installation is already running");
        } else if kind == FlashKind::Install && self.firmware.is_none() {
            self.note(Level::Error, "this build of the app has no firmware");
        } else if let Some(port) = port.or_else(|| {
            let s = session.filter(|s| return s.answered && s.kind == TransportKind::Cable);
            return s.map(|s| return s.link.name().to_owned());
        }) {
            self.pending_flash = Some((port, kind));
            return ControlFlow::Break(());
        } else if session.is_some_and(|s| return s.answered) {
            self.note(Level::Warn, "the firmware is installed over the “COM” cable only");
        } else {
            self.note(Level::Warn, "no board: choose the port to install the firmware on");
        }
        return ControlFlow::Continue(());
    }

    pub(super) fn start_flash(&mut self, port: String, kind: FlashKind) {
        let action = match kind {
            FlashKind::Install => "installing firmware",
            FlashKind::Erase => "removing firmware (erasing flash)",
        };
        self.status.log(Level::Info, format!("{port}: {action}"));
        self.status.flash = Some(FlashView { port: port.clone(), kind, stage: FlashStage::Connecting, progress: None });
        self.status.last_flash = None;
        self.publish();

        let tx = self.tx.clone();
        let demo = matches!(self.mode, Mode::Demo);
        let job = match kind {
            FlashKind::Install => FlashJob::Install(self.firmware.as_ref().map(|f| return f.bytes).unwrap_or_default()),
            FlashKind::Erase => FlashJob::Erase,
        };
        if demo && kind == FlashKind::Install {
            if let Some(f) = &self.firmware {
                self.demo_build = f.info.build_id;
            }
        }
        let thread = thread::Builder::new().name("flash".into()).spawn(move || {
            let mut last: Option<(FlashStage, Instant)> = None;
            let mut report = |stage: FlashStage, progress: Option<f32>| {
                let due = match last {
                    Some((s, t)) => s != stage || t.elapsed() >= FLASH_REPORT_INTERVAL,
                    None => true,
                };
                if due {
                    last = Some((stage, Instant::now()));
                    let _ = tx.send(CoreEvent::Flash(FlashUpdate::Progress(stage, progress)));
                }
            };
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                return if demo { simulate_flash(job, &mut report) } else { firmware::flash(&port, job, &mut report) };
            }))
            .unwrap_or_else(|_| return Err(format!("{port}: the installation crashed")));
            let _ = tx.send(CoreEvent::Flash(FlashUpdate::Done(result)));
        });
        match thread {
            Ok(t) => self.flashing = Some(t),
            Err(e) => self.flash_done(Err(format!("can't start the installation: {e}"))),
        }
    }

    pub(super) fn on_flash(&mut self, update: FlashUpdate) {
        match update {
            FlashUpdate::Progress(stage, progress) => {
                if let Some(f) = &mut self.status.flash {
                    f.stage = stage;
                    f.progress = progress;
                    self.publish();
                }
            }
            FlashUpdate::Done(result) => self.flash_done(result),
        }
    }

    fn flash_done(&mut self, result: Result<String, String>) {
        if let Some(t) = self.flashing.take() {
            let _ = t.join();
        }
        self.status.flash = None;
        let (ok, text) = match result {
            Ok(text) => (true, text),
            Err(text) => (false, text),
        };
        self.status.log(if ok { Level::Info } else { Level::Error }, text.clone());
        self.status.last_flash = Some(FlashOutcome { ok, text });
        self.publish();
    }
}

fn simulate_flash(job: FlashJob, report: FlashProgress<'_>) -> Result<String, String> {
    report(FlashStage::Connecting, None);
    thread::sleep(Duration::from_millis(600));
    match job {
        FlashJob::Install(image) => {
            for i in 0..=20 {
                report(FlashStage::Writing, Some(i as f32 / 20.0));
                thread::sleep(Duration::from_millis(100));
            }
            report(FlashStage::Verifying, None);
            thread::sleep(Duration::from_millis(400));
            report(FlashStage::Restarting, None);
            return Ok(format!("demo mode: simulated firmware installation ({} kB)", image.len() / 1024));
        }
        FlashJob::Erase => {
            report(FlashStage::Erasing, None);
            thread::sleep(Duration::from_millis(1500));
            report(FlashStage::Restarting, None);
            return Ok("demo mode: simulated flash erase".into());
        }
    }
}
