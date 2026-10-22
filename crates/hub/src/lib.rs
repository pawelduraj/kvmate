//! The window is created when opened and destroyed when closed. In the background only this
//! process and its tray icon remain, with no WebView processes.

mod autostart;
mod config_file;
mod firmware;
mod panel;
mod platform;
mod settings;
mod state;
mod tray;
mod wifi;
mod window;

use std::sync::Arc;

use kvmate_core::supervisor::{BoardStatus, Command, Mode, Options, Supervisor};
use kvmate_core::switcher::{Combo, Target};
use tauri::{AppHandle, RunEvent};

use state::{AppState, GuiObserver};

#[tauri::command]
fn get_status(state: tauri::State<'_, Arc<AppState>>) -> BoardStatus {
    return state.status.lock().unwrap().clone();
}

#[tauri::command]
fn set_target(target: Target, state: tauri::State<'_, Arc<AppState>>) {
    state.command(Command::SetTarget(target));
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn send_combo(combo: Combo, state: tauri::State<'_, Arc<AppState>>) {
    state.command(Command::SendCombo(combo));
}

fn mode_from_args(config_port: Option<String>) -> Mode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| return a == "--demo") {
        return Mode::Demo;
    }
    let port = args.iter().position(|a| return a == "--port").and_then(|i| return args.get(i + 1)).cloned();
    return Mode::Auto { port: port.or(config_port) };
}

pub fn run() {
    let state = Arc::new(AppState::default());

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::open(app, None)))
        .plugin(tauri_plugin_autostart::Builder::new().app_name("kvmate").build())
        .manage(state.clone())
        .manage(panel::PanelState::default())
        .on_window_event(window::on_event)
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_target,
            config_file::config_location,
            settings::audio_devices,
            settings::get_audio_config,
            settings::set_audio_config,
            settings::get_settings,
            settings::set_settings,
            firmware::install_firmware,
            firmware::erase_firmware,
            window::open_main,
            quit_app,
            send_combo,
            panel::panel_fit,
            panel::panel_hide,
            wifi::get_wifi,
            wifi::wifi_password,
            wifi::wifi_scan,
            wifi::wifi_saved,
            wifi::wifi_setup,
            wifi::wifi_forget
        ])
        .setup({
            let state = state.clone();
            move |app| {
                tray::create(app.handle())?;
                if let Some(dir) = config_file::config_path(app.handle()).as_deref().and_then(|p| return p.parent()) {
                    kvmate_core::logfile::init(dir);
                }
                let (config, note) = config_file::load(app.handle());
                *state.config.lock().unwrap() = config.clone();
                tray::sync_config(app.handle(), &config);
                let (firmware, firmware_note) = firmware::bundled_firmware();
                let options = Options {
                    mode: mode_from_args(config.port.clone()),
                    config,
                    capture: true,
                    notes: note.into_iter().chain(firmware_note).collect(),
                    firmware,
                };
                let observer = GuiObserver { app: app.handle().clone(), state: state.clone() };
                *state.supervisor.lock().unwrap() = Some(Supervisor::start(options, observer));
                return Ok(());
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build the kvmate application");

    app.run(move |_app, event| match event {
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => drop(state.supervisor.lock().unwrap().take()),
        _ => {}
    });
}
