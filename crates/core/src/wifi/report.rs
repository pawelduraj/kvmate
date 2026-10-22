use std::net::Ipv4Addr;

use kvmate_protocol as proto;
use serde::Serialize;

use super::WifiMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiState {
    Off,
    Connecting,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiFail {
    NotFound,
    Auth,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiAuth {
    Open,
    Personal,
    Enterprise,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BoardWifi {
    pub mode: WifiMode,
    pub state: WifiState,
    pub fail: Option<WifiFail>,
    pub ssid: String,
    pub ip: Option<String>,
    pub rssi: Option<i8>,
    pub channel: u8,
    pub client: bool,
    pub session: bool,
}

impl BoardWifi {
    pub fn from_message(s: &proto::WifiStatus) -> BoardWifi {
        let ip = s.ip_address;
        return BoardWifi {
            mode: WifiMode::from_wire(s.mode),
            state: match s.state {
                proto::WIFI_STATE_CONNECTING => WifiState::Connecting,
                proto::WIFI_STATE_READY => WifiState::Ready,
                proto::WIFI_STATE_FAILED => WifiState::Failed,
                _ => WifiState::Off,
            },
            fail: match s.fail_reason {
                proto::WIFI_FAIL_NOT_FOUND => Some(WifiFail::NotFound),
                proto::WIFI_FAIL_AUTH => Some(WifiFail::Auth),
                proto::WIFI_FAIL_OTHER => Some(WifiFail::Other),
                _ => None,
            },
            ssid: String::from_utf8_lossy(proto::name_bytes(&s.ssid, s.ssid_len)).into_owned(),
            ip: (ip != [0; 4]).then(|| return Ipv4Addr::from(ip).to_string()),
            rssi: (s.rssi_dbm != 0).then_some(s.rssi_dbm),
            channel: s.channel,
            client: s.flags & proto::WIFI_FLAG_CLIENT != 0,
            session: s.flags & proto::WIFI_FLAG_HUB_SESSION != 0,
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NetworkView {
    pub ssid: String,
    pub rssi: i8,
    pub channel: u8,
    pub auth: WifiAuth,
}

impl NetworkView {
    pub fn from_message(n: &proto::WifiNetwork) -> NetworkView {
        return NetworkView {
            ssid: String::from_utf8_lossy(proto::name_bytes(&n.ssid, n.ssid_len)).into_owned(),
            rssi: n.rssi_dbm,
            channel: n.channel,
            auth: match n.auth {
                proto::WIFI_AUTH_OPEN => WifiAuth::Open,
                proto::WIFI_AUTH_ENTERPRISE => WifiAuth::Enterprise,
                _ => WifiAuth::Personal,
            },
        };
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WifiView {
    pub board: Option<BoardWifi>,
    pub networks: Vec<NetworkView>,
    pub scanning: bool,
    pub adapter: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_report() {
        let mut s = proto::WifiStatus {
            mode: proto::WIFI_MODE_STATION,
            state: proto::WIFI_STATE_READY,
            rssi_dbm: -52,
            channel: 6,
            ssid_len: 4,
            ip_address: [192, 168, 1, 57],
            flags: proto::WIFI_FLAG_HUB_SESSION,
            ..Default::default()
        };
        s.ssid[..4].copy_from_slice(b"Home");
        let v = BoardWifi::from_message(&s);
        assert_eq!(
            (v.mode, v.state, v.ssid.as_str(), v.ip.as_deref()),
            (WifiMode::Router, WifiState::Ready, "Home", Some("192.168.1.57"))
        );
        assert!(v.session && !v.client && v.fail.is_none());
        assert_eq!(
            serde_json::to_string(&(v.state, WifiFail::NotFound, WifiAuth::Enterprise)).unwrap(),
            r#"["ready","not_found","enterprise"]"#
        );
    }
}
