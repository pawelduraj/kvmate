//! Talking to the board over Wi-Fi instead of the cable.
//!
//! The cable always wins: Wi-Fi is tried only while no board answers on a serial port.
//! Settings go to the board over the cable only (`WifiConfig`), with a new pairing key. The
//! key and the password of the board's own network are kept here, readable only by this user
//! ([`secret`]). A router's password is stored on the board only.

mod report;
pub mod secret;
mod setup;
pub mod wlan;

pub use report::{BoardWifi, NetworkView, WifiAuth, WifiFail, WifiState, WifiView};
pub use secret::WifiSecrets;
pub use setup::{DEFAULT_NETWORK_PREFIX, WifiSetup, host_name, network_name};

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use kvmate_protocol as proto;
use serde::{Deserialize, Serialize};

use crate::board::udp::Targets;

pub const BOARD_NETWORK_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiMode {
    #[default]
    Off,
    Router,
    BoardNetwork,
}

impl WifiMode {
    pub fn wire(self) -> u8 {
        match self {
            WifiMode::Off => return proto::WIFI_MODE_OFF,
            WifiMode::Router => return proto::WIFI_MODE_STATION,
            WifiMode::BoardNetwork => return proto::WIFI_MODE_ACCESS_POINT,
        }
    }

    pub fn from_wire(mode: u8) -> WifiMode {
        match mode {
            proto::WIFI_MODE_STATION => return WifiMode::Router,
            proto::WIFI_MODE_ACCESS_POINT => return WifiMode::BoardNetwork,
            _ => return WifiMode::Off,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WifiSettings {
    pub mode: WifiMode,
    pub router_ssid: String,
    pub network_ssid: String,
    pub name: String,
    pub board: Option<String>,
    pub secrets: Option<String>,
}

impl WifiSettings {
    pub fn secrets(&self) -> Option<WifiSecrets> {
        return self.secrets.as_deref().and_then(WifiSecrets::unprotect);
    }

    pub fn usable(&self) -> Option<WifiSecrets> {
        if self.mode == WifiMode::Off {
            return None;
        }
        return self.secrets();
    }

    pub(crate) fn targets(&self) -> Targets {
        let mut fixed = Vec::new();
        if self.mode == WifiMode::BoardNetwork {
            fixed.push(SocketAddr::V4(SocketAddrV4::new(BOARD_NETWORK_ADDRESS, proto::secure::WIFI_UDP_PORT)));
        }
        return Targets { fixed, broadcast_port: Some(proto::secure::WIFI_UDP_PORT) };
    }
}

pub(crate) fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("system random generator");
    return b;
}
