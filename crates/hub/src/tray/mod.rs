//! The tray menu has everything the window has, because on Linux it is all the tray offers
//! (AppIndicator doesn't report clicks). Only things that need typing or a confirmation open the window.

mod devices;
mod icon;
mod sync;

use std::fmt::Display;
use std::sync::{Arc, Mutex};

use kvmate_core::audio::StreamMode;
use kvmate_core::config::{CloseAction, Config, Hotkey, LED_LEVELS, Theme};
use kvmate_core::logfile::{self, Level};
use kvmate_core::supervisor::{BoardStatus, Command, LinkState, TransportKind, UsbState};
use kvmate_core::switcher::{Combo, Target};
use kvmate_core::wifi::WifiMode;
use tauri::menu::{CheckMenuItem, IconMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::state::AppState;
use crate::{autostart, window};
use devices::DeviceItems;
use icon::{Colour, icon_for, menu_icon, tray_size};
use sync::StatusView;

pub(crate) use sync::{sync_config, sync_status};

const TRAY_ID: &str = "main";
const MODES: [(StreamMode, &str); 3] =
    [(StreamMode::Always, "always"), (StreamMode::WhenLaptop, "when_laptop"), (StreamMode::Off, "off")];
const WIFI_MODES: [WifiMode; 3] = [WifiMode::Off, WifiMode::BoardNetwork, WifiMode::Router];
const THEMES: [(Theme, &str, &str); 3] =
    [(Theme::Auto, "auto", "System"), (Theme::Light, "light", "Light"), (Theme::Dark, "dark", "Dark")];
const CLOSE: [(CloseAction, &str, &str); 2] =
    [(CloseAction::Tray, "tray", "Keep kvmate running in the tray"), (CloseAction::Quit, "quit", "Quit kvmate")];

struct TrayMenu {
    header: MenuItem<Wry>,
    here: CheckMenuItem<Wry>,
    other: CheckMenuItem<Wry>,
    hotkeys: Vec<CheckMenuItem<Wry>>,
    cad_end: CheckMenuItem<Wry>,
    playback: Vec<CheckMenuItem<Wry>>,
    capture: Vec<CheckMenuItem<Wry>>,
    outputs: Mutex<DeviceItems>,
    inputs: Mutex<DeviceItems>,
    volume: MenuItem<Wry>,
    board: MenuItem<Wry>,
    wifi: [CheckMenuItem<Wry>; 3],
    wifi_hint: MenuItem<Wry>,
    firmware: MenuItem<Wry>,
    led_on: CheckMenuItem<Wry>,
    led_levels: Vec<CheckMenuItem<Wry>>,
    other_line: MenuItem<Wry>,
    jiggle: CheckMenuItem<Wry>,
    autostart: CheckMenuItem<Wry>,
    themes: Vec<CheckMenuItem<Wry>>,
    close: Vec<CheckMenuItem<Wry>>,
    shown: Mutex<Option<StatusView>>,
}

fn other_text(hotkey: &str) -> String {
    if cfg!(windows) {
        return format!("Controlling the other computer\t{hotkey}");
    } else {
        return format!("Controlling the other computer ({hotkey})");
    }
}

fn check(app: &AppHandle, id: &str, text: &str) -> tauri::Result<CheckMenuItem<Wry>> {
    return CheckMenuItem::with_id(app, id, text, true, false, None::<&str>);
}

fn choices<I: Display, T: AsRef<str>>(
    app: &AppHandle,
    prefix: &str,
    values: impl IntoIterator<Item = (I, T)>,
) -> tauri::Result<Vec<CheckMenuItem<Wry>>> {
    return values.into_iter().map(|(id, text)| return check(app, &format!("{prefix}_{id}"), text.as_ref())).collect();
}

fn item(app: &AppHandle, id: &str, text: &str) -> tauri::Result<MenuItem<Wry>> {
    return MenuItem::with_id(app, id, text, true, None::<&str>);
}

fn label(app: &AppHandle, text: &str) -> tauri::Result<MenuItem<Wry>> {
    return MenuItem::new(app, text, false, None::<&str>);
}

fn as_entries(items: &[CheckMenuItem<Wry>]) -> impl Iterator<Item = &dyn IsMenuItem<Wry>> {
    return items.iter().map(|i| return i as &dyn IsMenuItem<Wry>);
}

fn sound_menu(
    app: &AppHandle,
    title: &str,
    heading: &str,
    modes: &[CheckMenuItem<Wry>],
    tail: &[&dyn IsMenuItem<Wry>],
) -> tauri::Result<(Submenu<Wry>, usize)> {
    let heading = label(app, heading)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let devices_heading = label(app, title)?;
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&heading];
    items.extend(as_entries(modes));
    items.extend([&sep as &dyn IsMenuItem<Wry>, &devices_heading]);
    let first_device = items.len();
    items.extend_from_slice(tail);
    return Ok((Submenu::with_items(app, title, true, &items)?, first_device));
}

pub(crate) fn create(app: &AppHandle) -> tauri::Result<()> {
    let sep = || return PredefinedMenuItem::separator(app);
    let open = IconMenuItem::with_id(app, "open", "Open kvmate", true, menu_icon(app), None::<&str>)?;
    let header = label(app, "kvmate · looking for the board")?;
    let here = check(app, "target_here", "Controlling this computer")?;
    let other = check(app, "target_other", &other_text("2× Right Ctrl"))?;

    let hotkeys = choices(app, "hotkey", Hotkey::ALL.iter().enumerate().map(|(i, h)| return (i, h.label())))?;
    let cad_end = check(app, "cad_end", "Ctrl+Alt+End works as Ctrl+Alt+Del")?;
    let mut km_items: Vec<&dyn IsMenuItem<Wry>> = vec![];
    let km_label = label(app, "Switch shortcut")?;
    km_items.push(&km_label);
    km_items.extend(as_entries(&hotkeys));
    let km_sep = sep()?;
    km_items.push(&km_sep);
    km_items.push(&cad_end);
    let km_menu = Submenu::with_items(app, "Keyboard and mouse", true, &km_items)?;

    let modes = |labels: [&'static str; 3]| return MODES.map(|(_, id)| return id).into_iter().zip(labels);
    let play_labels = ["Always — hear both computers", "While controlling the other computer", "Never"];
    let playback = choices(app, "playback", modes(play_labels))?;
    let capture = choices(app, "capture", modes(["Always", "While controlling the other computer", "Never"]))?;
    let volume = label(app, "Volume on the other computer: —")?;
    let (play_menu, play_first) =
        sound_menu(app, "Headphones", "Other computer's sound", &playback, &[&sep()?, &volume])?;
    let (mic_menu, mic_first) = sound_menu(app, "Microphone", "Microphone for the other computer", &capture, &[])?;

    let board = label(app, "looking for the board")?;
    let wifi = [
        check(app, "wifi_off", "Off — cable only")?,
        check(app, "wifi_own", "Board's own network")?,
        check(app, "wifi_router", "Via router…")?,
    ];
    let wifi_hint = label(app, "")?;
    let firmware = item(app, "fw_update", "Update firmware")?;
    let led_on = check(app, "led_on", "On")?;
    let led_levels = choices(app, "led", (1..=LED_LEVELS.len()).map(|n| return (n, format!("Brightness {n} of 10"))))?;
    let mut led_items: Vec<&dyn IsMenuItem<Wry>> = vec![&led_on];
    let led_sep = sep()?;
    led_items.push(&led_sep);
    led_items.extend(as_entries(&led_levels));
    let led_menu = Submenu::with_items(app, "LED", true, &led_items)?;
    let board_menu = Submenu::with_items(
        app,
        "Board",
        true,
        &[
            &board,
            &sep()?,
            &label(app, "Board Wi-Fi (wireless)")?,
            &wifi[0],
            &wifi[1],
            &wifi[2],
            &wifi_hint,
            &sep()?,
            &firmware,
            &led_menu,
            &sep()?,
            &item(app, "open_board", "Connection, firmware and log…")?,
        ],
    )?;

    let other_line = label(app, "no board")?;
    let jiggle = check(app, "jiggle", "Keep it awake (while you control this computer)")?;
    let other_menu = Submenu::with_items(
        app,
        "Other computer",
        true,
        &[
            &other_line,
            &sep()?,
            &jiggle,
            &sep()?,
            &item(app, "send_cad", "Send Ctrl+Alt+Del")?,
            &item(app, "send_lock", "Lock it (Win+L)")?,
        ],
    )?;

    let autostart = check(app, "autostart", "Start kvmate at sign-in")?;
    let themes = choices(app, "theme", THEMES.map(|(_, id, text)| return (id, text)))?;
    let close = choices(app, "close", CLOSE.map(|(_, id, text)| return (id, text)))?;
    let theme_menu = Submenu::with_items(app, "Theme", true, &as_entries(&themes).collect::<Vec<_>>())?;
    let close_menu = Submenu::with_items(app, "When the window closes", true, &as_entries(&close).collect::<Vec<_>>())?;
    let here_menu = Submenu::with_items(app, "This computer", true, &[&autostart, &theme_menu, &close_menu])?;

    let menu = Menu::with_items(
        app,
        &[
            &open,
            &header,
            &sep()?,
            &here,
            &other,
            &sep()?,
            &km_menu,
            &play_menu,
            &mic_menu,
            &board_menu,
            &other_menu,
            &here_menu,
            &sep()?,
            &item(app, "quit", "Quit")?,
        ],
    )?;
    app.manage(TrayMenu {
        header,
        here,
        other,
        hotkeys,
        cad_end,
        playback,
        capture,
        outputs: DeviceItems::new(play_menu, play_first),
        inputs: DeviceItems::new(mic_menu, mic_first),
        volume,
        board,
        wifi,
        wifi_hint,
        firmware,
        led_on,
        led_levels,
        other_line,
        jiggle,
        autostart,
        themes,
        close,
        shown: Mutex::new(None),
    });

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_for(Colour::Searching, tray_size(app)))
        .tooltip("kvmate: looking for the board")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left, button_state: MouseButtonState::Up, rect, ..
            } = event
            {
                crate::panel::toggle(tray.app_handle(), rect);
            }
        })
        .build(app)?;

    return devices::watch(app);
}

fn on_menu(app: &AppHandle, id: &str) {
    let state = app.state::<Arc<AppState>>();
    let log = |result: Result<(), String>| {
        if let Err(e) = result {
            logfile::write(Level::Error, &e);
        }
    };
    let change = |f: &dyn Fn(&mut Config)| log(state.change_config(app, f));
    let menu = app.state::<TrayMenu>();
    match id {
        "open" => window::open(app, None),
        "quit" => app.exit(0),
        "target_here" => state.command(Command::SetTarget(Target::Pc)),
        "target_other" => state.command(Command::SetTarget(Target::Laptop)),
        "send_cad" => state.command(Command::SendCombo(Combo::CtrlAltDel)),
        "send_lock" => state.command(Command::SendCombo(Combo::Lock)),
        "cad_end" => change(&|c| c.ctrl_alt_end_as_delete = !c.ctrl_alt_end_as_delete),
        "jiggle" => change(&|c| c.jiggle = !c.jiggle),
        "open_board" => window::open(app, Some("board-conn")),
        "fw_update" => {
            let silent: Vec<String> = {
                let status = state.status.lock().unwrap();
                let connected = status.state == LinkState::Connected;
                if connected { Vec::new() } else { status.silent_ports.iter().map(|p| return p.port.clone()).collect() }
            };
            match silent.as_slice() {
                [] => state.command(Command::InstallFirmware { port: None }),
                [port] => state.command(Command::InstallFirmware { port: Some(port.clone()) }),
                _ => window::open(app, Some("board-conn")),
            }
        }
        "led_on" => change(&|c| {
            c.led_on = !c.led_on;
            if c.led_on && c.led_brightness == 0 {
                c.led_brightness = LED_LEVELS[4];
            }
        }),
        "autostart" => {
            let result = autostart::set(app, !autostart::enabled(app));
            if result.is_ok() {
                let _ = app.emit("config", ());
            }
            log(result);
        }
        "wifi_off" => {
            if state.config.lock().unwrap().wifi.mode != WifiMode::Off {
                log(crate::wifi::forget(app, &state).map(|_| ()));
            }
        }
        "wifi_own" => {
            if state.config.lock().unwrap().wifi.mode != WifiMode::BoardNetwork {
                let result = crate::wifi::setup(app, &state, WifiMode::BoardNetwork, None, None, false, None);
                if result.is_err() {
                    window::open(app, Some("board-conn"));
                }
                log(result);
            }
        }
        "wifi_router" => window::open(app, Some("board-conn")),
        _ if id.starts_with("out_dev:") => {
            if let Some(name) = devices::device_of(&menu.outputs, id) {
                change(&|c| c.audio.output_device = name.clone());
            }
        }
        _ if id.starts_with("in_dev:") => {
            if let Some(name) = devices::device_of(&menu.inputs, id) {
                change(&|c| c.audio.input_device = name.clone());
            }
        }
        _ => match id.split_once('_') {
            Some(("hotkey", index)) => {
                if let Some(hotkey) = index.parse().ok().and_then(|i: usize| return Hotkey::ALL.get(i).copied()) {
                    change(&|c| c.hotkey = hotkey);
                }
            }
            Some(("playback", name)) => {
                if let Some(&(mode, _)) = MODES.iter().find(|m| return m.1 == name) {
                    change(&|c| c.audio.playback = mode);
                }
            }
            Some(("capture", name)) => {
                if let Some(&(mode, _)) = MODES.iter().find(|m| return m.1 == name) {
                    change(&|c| c.audio.capture = mode);
                }
            }
            Some(("led", step)) => {
                if let Some(level) =
                    step.parse().ok().and_then(|s: usize| return LED_LEVELS.get(s.checked_sub(1)?).copied())
                {
                    change(&|c| {
                        c.led_on = true;
                        c.led_brightness = level;
                    });
                }
            }
            Some(("theme", name)) => {
                if let Some(&(theme, ..)) = THEMES.iter().find(|t| return t.1 == name) {
                    change(&|c| c.theme = theme);
                }
            }
            Some(("close", name)) => {
                if let Some(&(action, ..)) = CLOSE.iter().find(|a| return a.1 == name) {
                    change(&|c| c.on_close = action);
                }
            }
            _ => {}
        },
    }
    *menu.shown.lock().unwrap() = None;
    let status = state.status.lock().unwrap().clone();
    sync_status(app, &status);
    let config = state.config.lock().unwrap().clone();
    sync_config(app, &config);
}

fn summary(status: &BoardStatus) -> String {
    let demo = if status.demo { " (demo)" } else { "" };
    let text = match status.state {
        _ if status.flash.is_some() => "installing firmware".to_string(),
        LinkState::Searching if !status.silent_ports.is_empty() => "board without kvmate firmware".to_string(),
        LinkState::Searching => "looking for the board".to_string(),
        LinkState::Connecting if status.transport == TransportKind::Wifi => "connecting via Wi-Fi".to_string(),
        LinkState::Connecting => format!("connecting ({})", status.port.as_deref().unwrap_or("?")),
        LinkState::Connected if status.usb == UsbState::Detached => {
            "the other computer doesn't see the board".to_string()
        }
        LinkState::Connected => match status.target {
            Target::Pc => "controlling this computer".to_string(),
            Target::Laptop => "controlling the other computer".to_string(),
        },
    };
    return format!("{text}{demo}");
}
