pub const WIFI_MODE_OFF: u8 = 0;
pub const WIFI_MODE_STATION: u8 = 1;
pub const WIFI_MODE_ACCESS_POINT: u8 = 2;

pub const WIFI_STATE_OFF: u8 = 0;
pub const WIFI_STATE_CONNECTING: u8 = 1;
pub const WIFI_STATE_READY: u8 = 2;
pub const WIFI_STATE_FAILED: u8 = 3;

pub const WIFI_FAIL_NONE: u8 = 0;
pub const WIFI_FAIL_NOT_FOUND: u8 = 1;
pub const WIFI_FAIL_AUTH: u8 = 2;
pub const WIFI_FAIL_OTHER: u8 = 3;

pub const WIFI_FLAG_CLIENT: u8 = 0x01;
pub const WIFI_FLAG_HUB_SESSION: u8 = 0x02;

pub const WIFI_AUTH_OPEN: u8 = 0;
pub const WIFI_AUTH_PERSONAL: u8 = 1;
pub const WIFI_AUTH_ENTERPRISE: u8 = 2;

pub const WIFI_SSID_MAX: usize = 32;
pub const WIFI_PASSWORD_MAX: usize = 64;
pub const WIFI_HOSTNAME_MAX: usize = 32;
pub const WIFI_SCAN_MAX: usize = 20;
pub const WIFI_STATUS_INTERVAL_MS: u32 = 2000;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WifiConfig {
    pub mode: u8,
    pub ssid_len: u8,
    pub password_len: u8,
    pub ap_channel: u8,
    pub ssid: [u8; WIFI_SSID_MAX],
    pub password: [u8; WIFI_PASSWORD_MAX],
    pub pairing_key: [u8; 32],
    pub hostname_len: u8,
    pub hostname: [u8; WIFI_HOSTNAME_MAX],
}

impl Default for WifiConfig {
    fn default() -> Self {
        return bytemuck::Zeroable::zeroed();
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WifiStatus {
    pub mode: u8,
    pub state: u8,
    pub fail_reason: u8,
    pub flags: u8,
    pub rssi_dbm: i8,
    pub channel: u8,
    pub ssid_len: u8,
    pub ip_address: [u8; 4],
    pub ssid: [u8; WIFI_SSID_MAX],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WifiScanHeader {
    pub network_count: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WifiNetwork {
    pub rssi_dbm: i8,
    pub channel: u8,
    pub auth: u8,
    pub ssid_len: u8,
    pub ssid: [u8; WIFI_SSID_MAX],
}
