use std::ops::ControlFlow;

use super::session::Session;
use super::{Command, Core, FlashKind, TransportKind};
use crate::config::Config;
use crate::logfile::Level;
use crate::switcher::Request;
use crate::wifi::WifiMode;

impl Core {
    pub(super) fn on_command(&mut self, command: Command, session: Option<&Session>) -> ControlFlow<()> {
        let board = session.and_then(|s| return s.link.board());
        match command {
            Command::SetTarget(target) => self.request(Request::Set(target), session),
            Command::ApplyConfig(config) => return self.apply_config(*config, session),
            Command::InstallFirmware { port } => return self.flash_command(port, FlashKind::Install, session),
            Command::EraseFirmware { port } => return self.flash_command(port, FlashKind::Erase, session),
            Command::SendCombo(combo) => match session {
                Some(_) => self.switcher.send_combo(combo, board),
                None => self.note(Level::Warn, "no board: can't send the shortcut to the other computer"),
            },
            Command::WifiApply(setup) => self.wifi_apply(setup, session),
            Command::WifiScan => self.wifi_scan(session),
        }
        return ControlFlow::Continue(());
    }

    pub(super) fn request(&mut self, request: Request, session: Option<&Session>) {
        if let Some(t) = self.switcher.request(request, session.and_then(|s| return s.link.board())) {
            self.target_changed(t);
        }
    }

    fn apply_config(&mut self, config: Config, session: Option<&Session>) -> ControlFlow<()> {
        let board = session.and_then(|s| return s.link.board());
        let old = std::mem::replace(&mut self.config, config);
        let new = &self.config;
        let mut changed = Vec::new();
        if new.audio != old.audio {
            if let Some(a) = &self.activity {
                a.set_devices(new.audio.output_device.clone(), new.audio.input_device.clone());
            }
            self.audio.set_config(new.audio.clone());
            changed.push("sound");
        }
        let keyboard_led = (new.ctrl_alt_end_as_delete, new.led_gpio, new.led_level())
            != (old.ctrl_alt_end_as_delete, old.led_gpio, old.led_level());
        self.switcher.apply_settings(new, board);
        if keyboard_led {
            changed.push("keyboard and LED");
        }
        if new.usb != old.usb {
            changed.push("board names on the other computer");
        }
        if new.jiggle != old.jiggle {
            changed.push(if new.jiggle { "other computer kept awake" } else { "other computer may sleep" });
        }
        if new.hotkey != old.hotkey {
            self.control.set_hotkey(new.hotkey.usage());
            self.status.hotkey = new.hotkey.label().to_string();
            changed.push("shortcut");
        }
        if new.port != old.port {
            self.status.log(Level::Info, "the port change takes effect after restarting the app");
        }
        if new.wifi != old.wifi {
            if new.wifi.mode != WifiMode::BoardNetwork || new.wifi.network_ssid != old.wifi.network_ssid {
                self.forget_board_network(&old.wifi);
            }
            self.wifi_miss_logged = false;
            if new.wifi.mode != WifiMode::BoardNetwork {
                self.status.wifi.adapter = None;
            }
            changed.push("Wi-Fi");
        }
        let relink = new.wifi != old.wifi && session.is_some_and(|s| return s.kind == TransportKind::Wifi);
        if !changed.is_empty() {
            self.status.log(Level::Info, format!("settings changed: {}", changed.join(", ")));
        }
        self.publish();
        return if relink { ControlFlow::Break(()) } else { ControlFlow::Continue(()) };
    }
}
