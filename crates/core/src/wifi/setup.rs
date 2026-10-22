use kvmate_protocol::{self as proto, secure::PairingKey};

use super::WifiMode;

pub const DEFAULT_NETWORK_PREFIX: &str = "kvmate-";

pub fn network_name(mac: Option<&str>) -> String {
    let hex: String = mac.unwrap_or("").chars().filter(char::is_ascii_hexdigit).collect();
    let tail = if hex.len() >= 4 { hex[hex.len() - 4..].to_ascii_uppercase() } else { "0000".into() };
    return format!("{DEFAULT_NETWORK_PREFIX}{tail}");
}

pub fn host_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = match c.to_lowercase().next().unwrap_or(c) {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(proto::WIFI_HOSTNAME_MAX - 1);
    let out = out.trim_end_matches('-');
    if out.is_empty() { return "kvmate".into() } else { return out.into() }
}

#[derive(Clone, PartialEq, Eq)]
pub struct WifiSetup {
    pub mode: WifiMode,
    pub ssid: String,
    pub password: String,
    pub key: PairingKey,
    pub hostname: String,
}

impl std::fmt::Debug for WifiSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return f.debug_struct("WifiSetup").field("mode", &self.mode).field("ssid", &self.ssid).finish_non_exhaustive();
    }
}

impl WifiSetup {
    pub fn off() -> WifiSetup {
        return WifiSetup {
            mode: WifiMode::Off,
            ssid: String::new(),
            password: String::new(),
            key: [0; 32],
            hostname: String::new(),
        };
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.mode == WifiMode::Off {
            return Ok(());
        }
        let ssid = self.ssid.len();
        if ssid == 0 || ssid > proto::WIFI_SSID_MAX {
            return Err("the network name must be 1 to 32 bytes".into());
        }
        let pass = self.password.len();
        let open = pass == 0 && self.mode == WifiMode::Router;
        if !open && !(8..=63).contains(&pass) {
            return Err("the Wi-Fi password must be 8 to 63 characters".into());
        }
        return Ok(());
    }

    pub fn message(&self) -> proto::WifiConfig {
        let mut m = proto::WifiConfig { mode: self.mode.wire(), ..Default::default() };
        if self.mode == WifiMode::Off {
            return m;
        }
        m.ssid_len = copy_cut(&mut m.ssid, &self.ssid, proto::WIFI_SSID_MAX);
        m.password_len = copy_cut(&mut m.password, &self.password, proto::WIFI_PASSWORD_MAX - 1);
        m.pairing_key = self.key;
        m.hostname_len = copy_cut(&mut m.hostname, &self.hostname, proto::WIFI_HOSTNAME_MAX - 1);
        return m;
    }
}

fn copy_cut(dst: &mut [u8], text: &str, max: usize) -> u8 {
    let n = text.len().min(max);
    dst[..n].copy_from_slice(&text.as_bytes()[..n]);
    return n as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_names() {
        assert_eq!(network_name(Some("34:85:18:AB:7f:3a")), "kvmate-7F3A");
        assert_eq!(network_name(None), "kvmate-0000");
    }

    #[test]
    fn host_names() {
        assert_eq!(host_name("kvmate-7F3A"), "kvmate-7f3a");
        assert_eq!(host_name("  Łódź office (2)!"), "lodz-office-2");
        assert_eq!(host_name("Kraków Główny"), "krakow-glowny");
        assert_eq!(host_name("***"), "kvmate", "nothing usable left");
        assert_eq!(host_name(&"a-".repeat(40)).len(), 31);
        assert!(!host_name(&format!("{}-b", "a".repeat(30))).ends_with('-'));
    }

    #[test]
    fn setup_checks_and_message() {
        let key = [7u8; 32];
        let router = WifiSetup {
            mode: WifiMode::Router,
            ssid: "Home".into(),
            password: "secret-pass".into(),
            key,
            hostname: "office-1".into(),
        };
        assert!(router.validate().is_ok());
        assert!(WifiSetup { password: String::new(), ..router.clone() }.validate().is_ok(), "open network");
        assert!(WifiSetup { password: "short".into(), ..router.clone() }.validate().is_err());
        assert!(
            WifiSetup { mode: WifiMode::BoardNetwork, password: String::new(), ..router.clone() }.validate().is_err()
        );
        assert!(WifiSetup { ssid: "x".repeat(33), ..router.clone() }.validate().is_err());
        let m = router.message();
        assert_eq!((m.mode, m.ssid_len, m.password_len), (proto::WIFI_MODE_STATION, 4, 11));
        assert_eq!(&m.ssid[..4], b"Home");
        assert_eq!(proto::name_bytes(&m.hostname, m.hostname_len), b"office-1");
        assert_eq!({ m.pairing_key }, key);
        assert_eq!(WifiSetup::off().message(), proto::WifiConfig::default());
        assert!(!format!("{router:?}").contains("secret"), "no password in logs");
    }
}
