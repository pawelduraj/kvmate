use std::sync::Arc;

use kvmate_core::audio::{self, AudioConfig, DeviceList};
use kvmate_core::config::{CloseAction, Hotkey, Theme, UsbNames};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::autostart;
use crate::state::AppState;

#[tauri::command]
pub(crate) async fn audio_devices() -> DeviceList {
    return audio::list_devices();
}

#[tauri::command]
pub(crate) fn get_audio_config(state: tauri::State<'_, Arc<AppState>>) -> AudioConfig {
    return state.config.lock().unwrap().audio.clone();
}

#[tauri::command]
pub(crate) fn set_audio_config(
    audio: AudioConfig,
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    return state.change_config(&app, |c| c.audio = audio);
}

#[derive(Serialize)]
struct HotkeyOption {
    id: Hotkey,
    label: &'static str,
}

#[derive(Serialize)]
pub(crate) struct GeneralSettings {
    autostart: bool,
    hotkey: Hotkey,
    ctrl_alt_end_as_delete: bool,
    led_on: bool,
    led_brightness: u8,
    jiggle: bool,
    theme: Theme,
    on_close: CloseAction,
    usb_name: String,
    usb_manufacturer: String,
    hotkeys: Vec<HotkeyOption>,
}

#[derive(Deserialize)]
pub(crate) struct SettingsChange {
    autostart: Option<bool>,
    hotkey: Option<Hotkey>,
    ctrl_alt_end_as_delete: Option<bool>,
    led_on: Option<bool>,
    led_brightness: Option<u8>,
    jiggle: Option<bool>,
    theme: Option<Theme>,
    on_close: Option<CloseAction>,
    usb_name: Option<String>,
    usb_manufacturer: Option<String>,
}

#[tauri::command]
pub(crate) fn get_settings(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> GeneralSettings {
    let config = state.config.lock().unwrap();
    return GeneralSettings {
        autostart: autostart::enabled(&app),
        hotkey: config.hotkey,
        ctrl_alt_end_as_delete: config.ctrl_alt_end_as_delete,
        led_on: config.led_on,
        led_brightness: config.led_brightness,
        jiggle: config.jiggle,
        theme: config.theme,
        on_close: config.on_close,
        usb_name: config.usb.name.clone(),
        usb_manufacturer: config.usb.manufacturer.clone(),
        hotkeys: Hotkey::ALL.into_iter().map(|h| return HotkeyOption { id: h, label: h.label() }).collect(),
    };
}

#[tauri::command]
pub(crate) fn set_settings(
    settings: SettingsChange,
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let usb_name = settings.usb_name.map(|s| return s.trim().to_string());
    let usb_manufacturer = settings.usb_manufacturer.map(|s| return s.trim().to_string());
    if let Some(name) = &usb_name {
        UsbNames::check(name).map_err(|e| format!("Device name: {e}."))?;
    }
    if let Some(name) = &usb_manufacturer {
        UsbNames::check(name).map_err(|e| format!("Manufacturer: {e}."))?;
    }
    if let Some(on) = settings.autostart {
        autostart::set(&app, on)?;
    }
    return state.change_config(&app, |c| {
        if let Some(name) = usb_name {
            c.usb.name = name;
        }
        if let Some(name) = usb_manufacturer {
            c.usb.manufacturer = name;
        }
        c.led_on = settings.led_on.unwrap_or(c.led_on);
        c.hotkey = settings.hotkey.unwrap_or(c.hotkey);
        c.ctrl_alt_end_as_delete = settings.ctrl_alt_end_as_delete.unwrap_or(c.ctrl_alt_end_as_delete);
        c.led_brightness = settings.led_brightness.unwrap_or(c.led_brightness);
        c.jiggle = settings.jiggle.unwrap_or(c.jiggle);
        c.theme = settings.theme.unwrap_or(c.theme);
        c.on_close = settings.on_close.unwrap_or(c.on_close);
    });
}
