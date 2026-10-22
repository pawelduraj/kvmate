pub const BUTTON_EVENT_SHORT: u8 = 1;
pub const BUTTON_EVENT_LONG: u8 = 2;

pub const LOG_ERROR: u8 = 1;
pub const LOG_WARN: u8 = 2;
pub const LOG_INFO: u8 = 3;
pub const LOG_DEBUG: u8 = 4;

pub const USB_NAME_MAX: usize = 31;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ButtonEvent {
    pub kind: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SetLed {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DeviceConfig {
    pub led_pin: u8,
    pub led_brightness: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UsbNames {
    pub manufacturer_len: u8,
    pub product_len: u8,
    pub manufacturer: [u8; 32],
    pub product: [u8; 32],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LogHeader {
    pub level: u8,
}
