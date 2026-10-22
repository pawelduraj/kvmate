//! This computer's Wi-Fi card.
//!
//! The board's network is joined with a temporary profile: it is not added to the known
//! networks on purpose. Windows still keeps a copy while connected, so kvmate deletes it when
//! it exits or stops using that network ([`forget`]). A card that is already connected to
//! another network is left alone (it may carry this computer's internet).

use serde::Serialize;

use crate::platform::wlan::{check, delete_profile, profiles};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Adapter {
    Connected,
    NoAddress,
    Connecting,
    OtherNetwork(String),
    NoAdapter,
    Error(String),
}

impl Adapter {
    pub fn describe(&self, ssid: &str) -> String {
        match self {
            Adapter::Connected => return format!("Wi-Fi adapter connected to “{ssid}”"),
            Adapter::NoAddress => {
                return format!(
                    "Wi-Fi adapter connected to “{ssid}”, but the board gave it no address — update the board's firmware"
                );
            }
            Adapter::Connecting => return format!("Wi-Fi adapter connecting to “{ssid}”…"),
            Adapter::OtherNetwork(other) => {
                return format!(
                    "the Wi-Fi adapter is connected to “{other}” — kvmate won't take it over; disconnect it or choose “Via router”"
                );
            }
            Adapter::NoAdapter => return "this computer has no Wi-Fi adapter turned on — choose “Via router”".into(),
            Adapter::Error(e) => return format!("Wi-Fi adapter: {e}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SavedNetwork {
    pub ssid: String,
    pub usable: bool,
    pub connected: bool,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct ProfileInfo {
    pub(crate) ssid: String,
    pub(crate) password: Option<String>,
    pub(crate) usable: bool,
}

pub fn ensure_connected(ssid: &str, password: &str) -> Adapter {
    return check(ssid, password, true);
}

pub fn saved_networks(own: &str) -> Vec<SavedNetwork> {
    let mut list: Vec<SavedNetwork> = Vec::new();
    for (info, connected) in profiles() {
        if info.ssid == own || info.ssid.starts_with(super::DEFAULT_NETWORK_PREFIX) {
            continue;
        }
        match list.iter_mut().find(|n| return n.ssid == info.ssid) {
            Some(n) => {
                n.usable |= info.usable;
                n.connected |= connected;
            }
            None => list.push(SavedNetwork { ssid: info.ssid, usable: info.usable, connected }),
        }
    }
    list.sort_by_key(|n| return (!n.connected, !n.usable));
    return list;
}

pub fn saved_password(ssid: &str) -> Result<Option<String>, String> {
    let mut known = false;
    for (info, _) in profiles() {
        if info.ssid == ssid {
            if info.usable {
                return Ok(info.password);
            }
            known = true;
        }
    }
    return Err(if known {
        format!("Windows doesn't give the password of “{ssid}” (or it's a corporate network) — type it in")
    } else {
        format!("Windows doesn't know network “{ssid}”")
    });
}

pub fn forget(ssid: &str) {
    if !ssid.is_empty() {
        delete_profile(ssid);
    }
}
