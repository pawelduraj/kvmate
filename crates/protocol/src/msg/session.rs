#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DeviceInfo {
    pub firmware_version: [u8; 3],
    pub mac_address: [u8; 6],
    pub selftest_passed: u8,
    pub firmware_build_id: [u8; 8],
}

pub const HEARTBEAT_FLAG_HELLO_ANSWERED: u8 = 0x01;
pub const HEARTBEAT_FLAG_USB_MOUNTED: u8 = 0x02;
pub const HEARTBEAT_FLAG_USB_SUSPENDED: u8 = 0x04;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Heartbeat {
    pub flags: u8,
}
