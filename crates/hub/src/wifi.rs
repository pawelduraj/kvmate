use std::sync::Arc;

use kvmate_core::supervisor::Command;
use kvmate_core::wifi::wlan::{self, SavedNetwork};
use kvmate_core::wifi::{self, WifiMode, WifiSecrets, WifiSettings, WifiSetup};
use serde::Serialize;
use tauri::AppHandle;

use crate::state::AppState;

#[derive(Serialize)]
pub(crate) struct WifiInfo {
    mode: WifiMode,
    router_ssid: String,
    network_ssid: String,
    name: String,
    default_name: String,
    paired: bool,
}

#[tauri::command]
pub(crate) fn get_wifi(state: tauri::State<'_, Arc<AppState>>) -> WifiInfo {
    let wifi = state.config.lock().unwrap().wifi.clone();
    let mac = state.status.lock().unwrap().mac.clone().or_else(|| return wifi.board.clone());
    return WifiInfo {
        paired: wifi.secrets().is_some(),
        mode: wifi.mode,
        router_ssid: wifi.router_ssid,
        network_ssid: wifi.network_ssid,
        name: wifi.name,
        default_name: wifi::network_name(mac.as_deref()),
    };
}

#[tauri::command]
pub(crate) fn wifi_password(state: tauri::State<'_, Arc<AppState>>) -> Option<String> {
    let wifi = state.config.lock().unwrap().wifi.clone();
    return (wifi.mode == WifiMode::BoardNetwork)
        .then(|| return wifi.secrets())
        .flatten()
        .map(|s| return s.network_password);
}

#[tauri::command]
pub(crate) fn wifi_scan(state: tauri::State<'_, Arc<AppState>>) {
    state.command(Command::WifiScan);
}

#[tauri::command]
pub(crate) async fn wifi_saved(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<SavedNetwork>, String> {
    let own = state.config.lock().unwrap().wifi.network_ssid.clone();
    return Ok(wlan::saved_networks(&own));
}

#[tauri::command]
pub(crate) fn wifi_setup(
    mode: WifiMode,
    ssid: Option<String>,
    password: Option<String>,
    saved: Option<bool>,
    name: Option<String>,
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    return setup(&app, &state, mode, ssid, password, saved == Some(true), name);
}

pub(crate) fn setup(
    app: &AppHandle,
    state: &AppState,
    mode: WifiMode,
    ssid: Option<String>,
    password: Option<String>,
    saved: bool,
    name: Option<String>,
) -> Result<(), String> {
    if mode == WifiMode::Off {
        return forget(app, state).map(|_| ());
    }
    let status = state
        .board_on_cable()
        .ok_or("Connect the board with the “COM” cable — the board accepts Wi-Fi settings only over the cable.")?;
    let name = name
        .map(|n| return n.trim().to_string())
        .unwrap_or_else(|| return state.config.lock().unwrap().wifi.name.clone());
    let board_name = if name.is_empty() { wifi::network_name(status.mac.as_deref()) } else { name.clone() };
    let secrets = WifiSecrets::generate(mode == WifiMode::BoardNetwork);
    let (ssid, password) = match mode {
        WifiMode::BoardNetwork => (board_name.clone(), secrets.network_password.clone()),
        _ => {
            let ssid = ssid.unwrap_or_default().trim().to_string();
            let password = if saved {
                wlan::saved_password(&ssid).map_err(|e| format!("Not saved: {e}."))?.unwrap_or_default()
            } else {
                password.unwrap_or_default()
            };
            (ssid, password)
        }
    };
    if board_name.len() > kvmate_core::protocol::WIFI_SSID_MAX {
        return Err(
            "Not saved: the board's Wi-Fi name can be at most 32 bytes (letters outside ASCII take 2 or more).".into(),
        );
    }
    let hostname = wifi::host_name(&board_name);
    let setup = WifiSetup { mode, ssid: ssid.clone(), password, key: secrets.key, hostname };
    setup.validate().map_err(|e| format!("Not saved: {e}."))?;
    let protected = secrets.protect().ok_or("Can't protect the pairing key.")?;
    state.change_config(app, |c| {
        let old = std::mem::take(&mut c.wifi);
        c.wifi = WifiSettings {
            mode,
            router_ssid: if mode == WifiMode::Router { ssid.clone() } else { old.router_ssid },
            network_ssid: if mode == WifiMode::BoardNetwork { ssid.clone() } else { old.network_ssid },
            name,
            board: status.mac.clone(),
            secrets: Some(protected),
        };
    })?;
    state.command(Command::WifiApply(setup));
    return Ok(());
}

#[tauri::command]
pub(crate) fn wifi_forget(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<bool, String> {
    return forget(&app, &state);
}

pub(crate) fn forget(app: &AppHandle, state: &AppState) -> Result<bool, String> {
    let on_cable = state.board_on_cable().is_some();
    state.change_config(app, |c| {
        c.wifi = WifiSettings {
            router_ssid: std::mem::take(&mut c.wifi.router_ssid),
            name: std::mem::take(&mut c.wifi.name),
            ..WifiSettings::default()
        };
    })?;
    if on_cable {
        state.command(Command::WifiApply(WifiSetup::off()));
    }
    return Ok(on_cable);
}
