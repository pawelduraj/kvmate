use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kvmate_core::config::Config;
use kvmate_core::supervisor::{BoardStatus, Command, LinkState, Observer, Supervisor, TransportKind};
use tauri::{AppHandle, Emitter};

use crate::{config_file, tray, window};

#[derive(Default)]
pub(crate) struct AppState {
    pub(crate) status: Mutex<BoardStatus>,
    pub(crate) config: Mutex<Config>,
    pub(crate) supervisor: Mutex<Option<Supervisor>>,
    tray_queued: AtomicBool,
}

impl AppState {
    pub(crate) fn command(&self, command: Command) {
        if let Some(sup) = self.supervisor.lock().unwrap().as_ref() {
            sup.command(command);
        }
    }

    pub(crate) fn change_config(&self, app: &AppHandle, change: impl FnOnce(&mut Config)) -> Result<(), String> {
        let config = {
            let mut config = self.config.lock().unwrap();
            change(&mut config);
            config.clone()
        };
        self.command(Command::ApplyConfig(Box::new(config.clone())));
        tray::sync_config(app, &config);
        window::set_theme(app, config.theme);
        let _ = app.emit("config", ());
        return config_file::save(app, &config);
    }

    pub(crate) fn board_on_cable(&self) -> Option<BoardStatus> {
        let status = self.status.lock().unwrap().clone();
        return (status.state == LinkState::Connected && status.transport == TransportKind::Cable).then_some(status);
    }
}

pub(crate) struct GuiObserver {
    pub(crate) app: AppHandle,
    pub(crate) state: Arc<AppState>,
}

impl Observer for GuiObserver {
    fn status_changed(&self, status: &BoardStatus) {
        *self.state.status.lock().unwrap() = status.clone();
        let _ = self.app.emit("status", status);
        if self.state.tray_queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let (app, state) = (self.app.clone(), self.state.clone());
        let _ = self.app.run_on_main_thread(move || {
            state.tray_queued.store(false, Ordering::Release);
            let status = state.status.lock().unwrap().clone();
            tray::sync_status(&app, &status);
        });
    }
}
