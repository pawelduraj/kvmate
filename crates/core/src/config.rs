//! User configuration, stored as JSON by the application.

use kvmate_protocol as proto;
use serde::{Deserialize, Serialize};

use crate::audio::AudioConfig;
use crate::input::keymap::{USAGE_LEFT_CTRL, USAGE_RIGHT_CTRL, USAGE_RIGHT_SHIFT};
use crate::wifi::WifiSettings;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: Option<String>,
    pub hotkey: Hotkey,
    pub led_gpio: u8,
    pub led_on: bool,
    pub led_brightness: u8,
    pub ctrl_alt_end_as_delete: bool,
    pub audio: AudioConfig,
    pub jiggle: bool,
    pub theme: Theme,
    pub on_close: CloseAction,
    pub wifi: WifiSettings,
    pub usb: UsbNames,
}

impl Default for Config {
    fn default() -> Self {
        return Config {
            port: None,
            hotkey: Hotkey::DoubleRightCtrl,
            led_gpio: 48,
            led_on: true,
            led_brightness: 41,
            ctrl_alt_end_as_delete: true,
            audio: AudioConfig::default(),
            jiggle: false,
            theme: Theme::Auto,
            on_close: CloseAction::Tray,
            wifi: WifiSettings::default(),
            usb: UsbNames::default(),
        };
    }
}

pub const DEFAULT_USB_NAME: &str = "kvmate";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsbNames {
    pub name: String,
    pub manufacturer: String,
}

impl Default for UsbNames {
    fn default() -> Self {
        return UsbNames { name: DEFAULT_USB_NAME.into(), manufacturer: DEFAULT_USB_NAME.into() };
    }
}

impl UsbNames {
    pub fn check(name: &str) -> Result<(), String> {
        let len = name.chars().count();
        if len == 0 || len > proto::USB_NAME_MAX {
            return Err(format!("the name must be 1 to {} characters", proto::USB_NAME_MAX));
        }
        if !name.chars().all(|c| return (' '..='~').contains(&c)) {
            return Err(
                "only letters without accents, digits, spaces and simple symbols — USB can't carry others".into()
            );
        }
        return Ok(());
    }

    pub fn message(&self) -> proto::UsbNames {
        fn field(text: &str, out: &mut [u8; 32]) -> u8 {
            if UsbNames::check(text).is_err() {
                return 0;
            }
            out[..text.len()].copy_from_slice(text.as_bytes());
            return text.len() as u8;
        }
        let mut m = proto::UsbNames::default();
        m.manufacturer_len = field(&self.manufacturer, &mut m.manufacturer);
        m.product_len = field(&self.name, &mut m.product);
        return m;
    }
}

pub const LED_LEVELS: [u8; 10] = [3, 10, 23, 41, 64, 92, 125, 163, 207, 255];

impl Config {
    pub fn led_level(&self) -> u8 {
        if self.led_on { return self.led_brightness } else { return 0 }
    }

    pub fn led_step(&self) -> usize {
        let b = self.led_brightness as i32;
        return (0..LED_LEVELS.len()).min_by_key(|&i| return (LED_LEVELS[i] as i32 - b).abs()).unwrap_or(0) + 1;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseAction {
    #[default]
    Tray,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hotkey {
    DoubleRightCtrl,
    DoubleLeftCtrl,
    DoubleRightShift,
}

impl Hotkey {
    pub const ALL: [Hotkey; 3] = [Hotkey::DoubleRightCtrl, Hotkey::DoubleLeftCtrl, Hotkey::DoubleRightShift];

    pub fn label(self) -> &'static str {
        match self {
            Hotkey::DoubleRightCtrl => return "2× Right Ctrl",
            Hotkey::DoubleLeftCtrl => return "2× Left Ctrl",
            Hotkey::DoubleRightShift => return "2× Right Shift",
        }
    }

    pub fn usage(self) -> u8 {
        match self {
            Hotkey::DoubleRightCtrl => return USAGE_RIGHT_CTRL,
            Hotkey::DoubleLeftCtrl => return USAGE_LEFT_CTRL,
            Hotkey::DoubleRightShift => return USAGE_RIGHT_SHIFT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_get_defaults() {
        let cfg: Config = serde_json::from_str(r#"{ "led_gpio": 38 }"#).unwrap();
        assert_eq!(cfg.led_gpio, 38);
        assert_eq!(cfg.hotkey, Hotkey::DoubleRightCtrl);
        assert!(cfg.ctrl_alt_end_as_delete);
        assert_eq!((cfg.theme, cfg.on_close), (Theme::Auto, CloseAction::Tray));
    }

    #[test]
    fn usb_names() {
        assert!(UsbNames::check("USB Audio (office) 2").is_ok());
        assert!(UsbNames::check("").is_err());
        assert!(UsbNames::check(&"x".repeat(32)).is_err());
        assert!(UsbNames::check("Café").is_err(), "USB strings carry ASCII only");
        let m = UsbNames { name: "Headset".into(), manufacturer: "Zürich".into() }.message();
        assert_eq!(proto::name_bytes(&m.product, m.product_len), b"Headset");
        assert_eq!(m.manufacturer_len, 0, "invalid: the board keeps its default");
        let json = r#"{"usb": {"name": "Office"}}"#;
        let usb = serde_json::from_str::<Config>(json).unwrap().usb;
        assert_eq!((usb.name.as_str(), usb.manufacturer.as_str()), ("Office", DEFAULT_USB_NAME));
    }
}
