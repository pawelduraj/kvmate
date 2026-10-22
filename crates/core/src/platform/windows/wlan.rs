//! The Wi-Fi card through the WLAN API, and the XML of WLAN profiles.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WiFi::{
    WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST,
    WLAN_PROFILE_GET_PLAINTEXT_KEY, WLAN_PROFILE_INFO, WLAN_PROFILE_INFO_LIST, WlanCloseHandle, WlanConnect,
    WlanDeleteProfile, WlanEnumInterfaces, WlanFreeMemory, WlanGetProfile, WlanGetProfileList, WlanOpenHandle,
    WlanQueryInterface, dot11_BSS_type_infrastructure, wlan_connection_mode_temporary_profile,
    wlan_interface_state_associating, wlan_interface_state_authenticating, wlan_interface_state_connected,
    wlan_interface_state_discovering, wlan_intf_opcode_current_connection,
};
use windows_sys::core::GUID;

use super::wide;
use crate::wifi::BOARD_NETWORK_ADDRESS;
use crate::wifi::wlan::{Adapter, ProfileInfo};

fn xml_escape(s: &str) -> String {
    return s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
}

fn xml_unescape(s: &str) -> String {
    return s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
}

fn xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = start + xml[start..].find(&format!("</{tag}>"))?;
    return Some(&xml[start..end]);
}

fn parse_profile(xml: &str) -> Option<ProfileInfo> {
    let ssid = xml_unescape(xml_tag(xml_tag(xml, "SSID")?, "name")?);
    if xml_tag(xml, "connectionType").is_some_and(|t| return t != "ESS") {
        return None;
    }
    let auth = xml_tag(xml, "authentication").unwrap_or("open");
    let encryption = xml_tag(xml, "encryption").unwrap_or("none");
    let (password, usable) = match auth {
        "open" if encryption == "none" => (None, true),
        "WPA2PSK" | "WPA3SAE" => {
            let plain = xml_tag(xml, "protected") == Some("false");
            let key = xml_tag(xml, "keyMaterial").filter(|_| return plain).map(xml_unescape);
            let ok = key.as_ref().is_some_and(|k| return (8..=63).contains(&k.len()));
            (key, ok)
        }
        _ => (None, false),
    };
    return Some(ProfileInfo { ssid, password, usable });
}

fn profile_xml(ssid: &str, password: &str) -> String {
    let (ssid, password) = (xml_escape(ssid), xml_escape(password));
    return format!(
        r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{ssid}</name>
  <SSIDConfig><SSID><name>{ssid}</name></SSID></SSIDConfig>
  <connectionType>ESS</connectionType>
  <connectionMode>manual</connectionMode>
  <MSM><security>
    <authEncryption><authentication>WPA2PSK</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption>
    <sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{password}</keyMaterial></sharedKey>
  </security></MSM>
</WLANProfile>"#
    );
}

pub fn check(ssid: &str, password: &str, connect: bool) -> Adapter {
    let Some(wlan) = Wlan::open() else { return Adapter::NoAdapter };
    let mut other_network = None;
    let mut free = None;
    for iface in wlan.interfaces() {
        match iface.isState {
            s if s == wlan_interface_state_connected => match wlan.connected_ssid(&iface.InterfaceGuid) {
                None => return Adapter::Connected,
                Some(name) if name == ssid => {
                    if has_board_network_address() {
                        return Adapter::Connected;
                    } else {
                        return Adapter::NoAddress;
                    }
                }
                Some(name) => other_network = other_network.or(Some(name)),
            },
            s if s == wlan_interface_state_associating
                || s == wlan_interface_state_authenticating
                || s == wlan_interface_state_discovering =>
            {
                return Adapter::Connecting;
            }
            _ => free = free.or(Some(iface.InterfaceGuid)),
        }
    }
    match (free, other_network) {
        (Some(_), _) if !connect => {
            return Adapter::Error("Wi-Fi adapter free, not connecting (check only)".into());
        }
        (Some(guid), _) => match wlan.connect_temporary(&guid, &profile_xml(ssid, password)) {
            0 => return Adapter::Connecting,
            e => return Adapter::Error(format!("can't connect (error {e})")),
        },
        (None, Some(name)) => return Adapter::OtherNetwork(name),
        (None, None) => return Adapter::NoAdapter,
    }
}

fn has_board_network_address() -> bool {
    let net = BOARD_NETWORK_ADDRESS.octets();
    return if_addrs::get_if_addrs().unwrap_or_default().iter().any(|i| match i.addr {
        if_addrs::IfAddr::V4(ref v4) => return v4.ip.octets()[..3] == net[..3] && v4.ip != BOARD_NETWORK_ADDRESS,
        _ => return false,
    });
}

fn from_wide(s: &[u16]) -> String {
    return String::from_utf16_lossy(&s[..s.iter().position(|&c| return c == 0).unwrap_or(s.len())]);
}

struct Wlan(HANDLE);

impl Drop for Wlan {
    fn drop(&mut self) {
        // SAFETY: a handle from WlanOpenHandle.
        unsafe { WlanCloseHandle(self.0, null()) };
    }
}

// SAFETY (all methods): WLAN API calls with valid in/out pointers; returned memory is freed
// with WlanFreeMemory after copying out of it.
impl Wlan {
    fn open() -> Option<Wlan> {
        let mut version = 0u32;
        let mut raw: HANDLE = null_mut();
        return (unsafe { WlanOpenHandle(2, null(), &mut version, &mut raw) } == 0).then_some(Wlan(raw));
    }

    fn interfaces(&self) -> Vec<WLAN_INTERFACE_INFO> {
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
            if WlanEnumInterfaces(self.0, null(), &mut list) != 0 || list.is_null() {
                return Vec::new();
            }
            let count = (*list).dwNumberOfItems as usize;
            let first = std::ptr::addr_of!((*list).InterfaceInfo) as *const WLAN_INTERFACE_INFO;
            let out = (0..count).map(|i| return *first.add(i)).collect();
            WlanFreeMemory(list as _);
            return out;
        }
    }

    fn connected_ssid(&self, iface: &GUID) -> Option<String> {
        unsafe {
            let mut size = 0u32;
            let mut data: *mut core::ffi::c_void = null_mut();
            let r = WlanQueryInterface(
                self.0,
                iface,
                wlan_intf_opcode_current_connection,
                null(),
                &mut size,
                &mut data,
                null_mut(),
            );
            if r != 0 || data.is_null() {
                return None;
            }
            let ssid = (*(data as *const WLAN_CONNECTION_ATTRIBUTES)).wlanAssociationAttributes.dot11Ssid;
            let name = String::from_utf8_lossy(&ssid.ucSSID[..(ssid.uSSIDLength as usize).min(32)]).into_owned();
            WlanFreeMemory(data);
            return Some(name);
        }
    }

    fn profile_names(&self, iface: &GUID) -> Vec<String> {
        unsafe {
            let mut list: *mut WLAN_PROFILE_INFO_LIST = null_mut();
            if WlanGetProfileList(self.0, iface, null(), &mut list) != 0 || list.is_null() {
                return Vec::new();
            }
            let count = (*list).dwNumberOfItems as usize;
            let first = std::ptr::addr_of!((*list).ProfileInfo) as *const WLAN_PROFILE_INFO;
            let out = (0..count).map(|i| return from_wide(&(*first.add(i)).strProfileName)).collect();
            WlanFreeMemory(list as _);
            return out;
        }
    }

    fn profile_xml(&self, iface: &GUID, name: &str, with_key: bool) -> Option<String> {
        unsafe {
            let mut xml: *mut u16 = null_mut();
            let mut flags = if with_key { WLAN_PROFILE_GET_PLAINTEXT_KEY } else { 0 };
            let mut access = 0u32;
            if WlanGetProfile(self.0, iface, wide(name).as_ptr(), null(), &mut xml, &mut flags, &mut access) != 0
                || xml.is_null()
            {
                return None;
            }
            let len = (0..).take_while(|&i| return *xml.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(xml, len));
            WlanFreeMemory(xml as _);
            return Some(text);
        }
    }

    fn delete_profile(&self, iface: &GUID, name: &str) {
        unsafe { WlanDeleteProfile(self.0, iface, wide(name).as_ptr(), null()) };
    }

    fn connect_temporary(&self, iface: &GUID, xml: &str) -> u32 {
        let xml = wide(xml);
        let params = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_temporary_profile,
            strProfile: xml.as_ptr(),
            pDot11Ssid: null_mut(),
            pDesiredBssidList: null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        unsafe { return WlanConnect(self.0, iface, &params, null()) }
    }
}

pub fn profiles() -> Vec<(ProfileInfo, bool)> {
    let Some(wlan) = Wlan::open() else { return Vec::new() };
    let mut out = Vec::new();
    for iface in wlan.interfaces() {
        let current = (iface.isState == wlan_interface_state_connected)
            .then(|| return wlan.connected_ssid(&iface.InterfaceGuid))
            .flatten();
        for name in wlan.profile_names(&iface.InterfaceGuid) {
            if let Some(info) = wlan.profile_xml(&iface.InterfaceGuid, &name, true).as_deref().and_then(parse_profile) {
                let connected = current.as_deref() == Some(info.ssid.as_str());
                out.push((info, connected));
            }
        }
    }
    return out;
}

pub fn delete_profile(ssid: &str) {
    let Some(wlan) = Wlan::open() else { return };
    for iface in wlan.interfaces() {
        for name in wlan.profile_names(&iface.InterfaceGuid) {
            let ours = name == ssid
                || wlan
                    .profile_xml(&iface.InterfaceGuid, &name, false)
                    .as_deref()
                    .and_then(parse_profile)
                    .is_some_and(|i| return i.ssid == ssid);
            if ours {
                wlan.delete_profile(&iface.InterfaceGuid, &name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wifi::wlan::saved_networks;

    #[test]
    fn profile_is_escaped() {
        let xml = profile_xml("kvmate-7F3A", "a<b&c");
        assert!(xml.contains("<name>kvmate-7F3A</name>") && xml.contains("a&lt;b&amp;c"));
        assert!(xml.contains("WPA2PSK") && xml.contains("<connectionMode>manual</connectionMode>"));
    }

    #[test]
    fn own_profile_reads_back() {
        let info = parse_profile(&profile_xml("Café & Garden", "secret<pass>")).unwrap();
        assert_eq!(
            info,
            ProfileInfo { ssid: "Café & Garden".into(), password: Some("secret<pass>".into()), usable: true }
        );
    }

    #[test]
    fn profiles_the_board_cannot_use() {
        let protected =
            profile_xml("Home", "12345678").replace("<protected>false</protected>", "<protected>true</protected>");
        assert_eq!(parse_profile(&protected).unwrap().password, None);
        assert!(!parse_profile(&protected).unwrap().usable);
        let enterprise = profile_xml("Company", "x").replace("WPA2PSK", "WPA2");
        assert!(!parse_profile(&enterprise).unwrap().usable);
        let wpa1 = profile_xml("Old", "12345678").replace("WPA2PSK", "WPAPSK");
        assert!(!parse_profile(&wpa1).unwrap().usable);
        let hex_key = profile_xml("Hex", &"ab".repeat(32)).replace("passPhrase", "networkKey");
        assert!(!parse_profile(&hex_key).unwrap().usable);
        let open = r#"<WLANProfile><name>Cafe</name><SSIDConfig><SSID><hex>4B</hex><name>Cafe</name></SSID></SSIDConfig>
            <connectionType>ESS</connectionType><MSM><security><authEncryption><authentication>open</authentication>
            <encryption>none</encryption></authEncryption></security></MSM></WLANProfile>"#;
        assert_eq!(parse_profile(open).unwrap(), ProfileInfo { ssid: "Cafe".into(), password: None, usable: true });
    }

    #[test]
    #[ignore]
    fn live_wlan_state() {
        println!("{:?}", check("kvmate-0000", "x", false));
        for n in saved_networks("") {
            println!("saved: {n:?}");
        }
    }
}
