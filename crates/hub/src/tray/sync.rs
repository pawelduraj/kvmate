use kvmate_core::config::{Config, Hotkey};
use kvmate_core::supervisor::{BoardStatus, LinkState, TransportKind, UsbState};
use kvmate_core::switcher::Target;
use kvmate_core::wifi::WifiMode;
use tauri::{AppHandle, Manager};

use super::icon::{Colour, icon_for, tray_size};
use super::{CLOSE, MODES, THEMES, TRAY_ID, TrayMenu, WIFI_MODES, devices, other_text, summary};
use crate::autostart;

#[derive(Clone, PartialEq, Default)]
pub(super) struct StatusView {
    colour: Colour,
    icon_size: u32,
    tooltip: String,
    header: String,
    on_other: bool,
    other_enabled: bool,
    other_text: String,
    board: String,
    wifi_enabled: bool,
    wifi_hint: String,
    firmware: String,
    firmware_enabled: bool,
    other_line: String,
    volume: String,
}

pub(crate) fn sync_config(app: &AppHandle, config: &Config) {
    let Some(menu) = app.try_state::<TrayMenu>() else { return };
    for (i, h) in Hotkey::ALL.iter().enumerate() {
        let _ = menu.hotkeys[i].set_checked(config.hotkey == *h);
    }
    let _ = menu.cad_end.set_checked(config.ctrl_alt_end_as_delete);
    for (i, (mode, _)) in MODES.iter().enumerate() {
        let _ = menu.playback[i].set_checked(config.audio.playback == *mode);
        let _ = menu.capture[i].set_checked(config.audio.capture == *mode);
    }
    devices::sync(&menu.outputs, &config.audio.output_device);
    devices::sync(&menu.inputs, &config.audio.input_device);

    for (entry, mode) in menu.wifi.iter().zip(WIFI_MODES) {
        let _ = entry.set_checked(config.wifi.mode == mode);
    }
    let router = if config.wifi.mode == WifiMode::Router && !config.wifi.router_ssid.is_empty() {
        format!("Via router: “{}”…", config.wifi.router_ssid)
    } else {
        "Via router…".into()
    };
    let _ = menu.wifi[2].set_text(router);
    let _ = menu.led_on.set_checked(config.led_on && config.led_brightness > 0);
    let step = config.led_step();
    for (i, level) in menu.led_levels.iter().enumerate() {
        let _ = level.set_checked(config.led_on && i + 1 == step);
    }
    let _ = menu.jiggle.set_checked(config.jiggle);
    let _ = menu.autostart.set_checked(autostart::enabled(app));
    for (i, (theme, _, _)) in THEMES.iter().enumerate() {
        let _ = menu.themes[i].set_checked(config.theme == *theme);
    }
    for (i, (action, _, _)) in CLOSE.iter().enumerate() {
        let _ = menu.close[i].set_checked(config.on_close == *action);
    }
}

fn status_view(status: &BoardStatus, icon_size: u32) -> StatusView {
    let connected = status.state == LinkState::Connected;
    let on_cable = connected && status.transport == TransportKind::Cable;
    let problem = status.selftest_ok == Some(false) || status.versions_match == Some(false);
    let colour = match (status.state, status.target) {
        _ if problem => Colour::Problem,
        (LinkState::Searching, _) => Colour::Searching,
        (LinkState::Connecting, _) => Colour::Connecting,
        (LinkState::Connected, Target::Pc) => Colour::Pc,
        (LinkState::Connected, Target::Laptop) => Colour::Laptop,
    };
    let summary = summary(status);
    let board = if !connected {
        summary.clone()
    } else if status.transport == TransportKind::Wifi {
        format!("connected via Wi-Fi · firmware {}", status.firmware.as_deref().unwrap_or("?"))
    } else {
        format!(
            "connected by cable ({}) · firmware {}",
            status.port.as_deref().unwrap_or("?"),
            status.firmware.as_deref().unwrap_or("?")
        )
    };
    let (firmware, firmware_enabled) = match () {
        _ if status.flash.is_some() => ("Installing firmware…".to_string(), false),
        _ if status.bundled_firmware.is_none() => ("Firmware: not in this build".to_string(), false),
        _ if !connected && status.silent_ports.len() == 1 => ("Install kvmate firmware".to_string(), true),
        _ if !connected && !status.silent_ports.is_empty() => ("Install kvmate firmware…".to_string(), true),
        _ if !on_cable => ("Firmware: updates over the “COM” cable only".to_string(), false),
        _ if status.firmware_current == Some(true) => ("Firmware up to date — reinstall".to_string(), true),
        _ => ("Update firmware".to_string(), true),
    };
    let other_line = match (connected, status.usb) {
        (false, _) => "no board",
        (true, UsbState::Mounted) => "sees the board",
        (true, UsbState::Suspended) => "asleep — a key press wakes it",
        (true, UsbState::Detached) => "doesn't see the board — check the “USB” cable",
        (true, UsbState::Unknown) => "—",
    };
    let a = &status.audio;
    let volume = match (a.playback.active, a.host_volume_percent) {
        (true, Some(p)) => format!("Volume on the other computer: {p}%"),
        (true, None) => "Other computer: muted".into(),
        (false, _) => "Volume on the other computer: — (nothing playing)".into(),
    };
    return StatusView {
        colour,
        icon_size,
        tooltip: format!("kvmate: {summary}"),
        header: format!("kvmate · {summary}"),
        on_other: status.target == Target::Laptop,
        other_enabled: other_reachable(status) || status.target == Target::Laptop,
        other_text: other_text(if status.hotkey.is_empty() { "2× Right Ctrl" } else { &status.hotkey }),
        board,
        wifi_enabled: on_cable,
        wifi_hint: if on_cable {
            String::new()
        } else {
            "Wi-Fi changes only with the board on the “COM” cable".into()
        },
        firmware,
        firmware_enabled,
        other_line: other_line.into(),
        volume,
    };
}

pub(crate) fn sync_status(app: &AppHandle, status: &BoardStatus) {
    let Some(menu) = app.try_state::<TrayMenu>() else { return };
    let view = status_view(status, tray_size(app));
    let mut shown = menu.shown.lock().unwrap();
    let old = shown.clone().unwrap_or_default();
    let all = shown.is_none();
    if !all && old == view {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if all || old.colour != view.colour || old.icon_size != view.icon_size {
            let _ = tray.set_icon(Some(icon_for(view.colour, view.icon_size)));
        }
        if all || old.tooltip != view.tooltip {
            let _ = tray.set_tooltip(Some(&view.tooltip));
        }
    }
    if all || old.header != view.header {
        let _ = menu.header.set_text(&view.header);
    }
    if all || old.on_other != view.on_other {
        let _ = menu.here.set_checked(!view.on_other);
        let _ = menu.other.set_checked(view.on_other);
    }
    if all || old.other_enabled != view.other_enabled {
        let _ = menu.other.set_enabled(view.other_enabled);
    }
    if all || old.other_text != view.other_text {
        let _ = menu.other.set_text(&view.other_text);
    }
    if all || old.board != view.board {
        let _ = menu.board.set_text(&view.board);
    }
    if all || old.wifi_enabled != view.wifi_enabled || old.wifi_hint != view.wifi_hint {
        for entry in &menu.wifi {
            let _ = entry.set_enabled(view.wifi_enabled);
        }
        let _ = menu.wifi_hint.set_text(&view.wifi_hint);
    }
    if all || old.firmware != view.firmware || old.firmware_enabled != view.firmware_enabled {
        let _ = menu.firmware.set_text(&view.firmware);
        let _ = menu.firmware.set_enabled(view.firmware_enabled);
    }
    if all || old.other_line != view.other_line {
        let _ = menu.other_line.set_text(&view.other_line);
    }
    if all || old.volume != view.volume {
        let _ = menu.volume.set_text(&view.volume);
    }
    *shown = Some(view);
}

fn other_reachable(status: &BoardStatus) -> bool {
    return status.state == LinkState::Connected && matches!(status.usb, UsbState::Mounted | UsbState::Suspended);
}
