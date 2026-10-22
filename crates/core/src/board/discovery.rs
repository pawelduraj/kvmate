//! Finding the board by the USB IDs of its COM port.

use crate::platform;

const KNOWN_BRIDGES: &[(u16, u16, &str)] = &[
    (0x1A86, 0x55D3, "CH343"),
    (0x10C4, 0xEA60, "CP210x"),
    (0x1A86, 0x7523, "CH340"),
    (0x0403, 0x6001, "FT232R"),
    (0x0403, 0x6015, "FT231X"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortCandidate {
    pub name: String,
    pub bridge: &'static str,
}

pub fn candidates() -> Vec<PortCandidate> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };
    let mut found: Vec<(usize, PortCandidate)> = ports
        .into_iter()
        .filter_map(|p| {
            let serialport::SerialPortType::UsbPort(usb) = p.port_type else {
                return None;
            };
            let rank = KNOWN_BRIDGES.iter().position(|(v, d, _)| return *v == usb.vid && *d == usb.pid)?;
            return Some((rank, PortCandidate { name: p.port_name, bridge: KNOWN_BRIDGES[rank].2 }));
        })
        .collect();
    found.sort_by(|a, b| return a.0.cmp(&b.0).then_with(|| return a.1.name.cmp(&b.1.name)));
    return found.into_iter().map(|(_, c)| return c).collect();
}

const BOARD_USB_VID: u16 = 0x303A;
const BOARD_USB_PID: u16 = 0x4004;

pub fn board_usb_on_this_computer(mac: [u8; 6]) -> bool {
    let serial: String = mac.iter().map(|b| format!("{b:02X}")).collect();
    return platform::usb::device_present(BOARD_USB_VID, BOARD_USB_PID, &serial);
}
