//! Whether a USB device is plugged into this computer, from the device instance IDs.

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GETIDLIST_FILTER_ENUMERATOR, CM_GETIDLIST_FILTER_PRESENT, CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW,
    CR_SUCCESS,
};

use super::wide;

pub fn device_present(vid: u16, pid: u16, serial: &str) -> bool {
    let filter = wide("USB");
    let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
    let mut len = 0u32;
    // SAFETY: plain cfgmgr32 calls; the buffer is as long as the size call reported.
    let ids = unsafe {
        if CM_Get_Device_ID_List_SizeW(&mut len, filter.as_ptr(), flags) != CR_SUCCESS {
            return false;
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_ID_ListW(filter.as_ptr(), buf.as_mut_ptr(), len, flags) != CR_SUCCESS {
            return false;
        }
        buf
    };
    let wanted = format!(r"USB\VID_{vid:04X}&PID_{pid:04X}\{}", serial.to_ascii_uppercase());
    return ids
        .split(|&c| return c == 0)
        .filter(|id| return !id.is_empty())
        .any(|id| return String::from_utf16_lossy(id).to_ascii_uppercase().starts_with(&wanted));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn live_usb_device_present() {
        let serial = std::env::var("KVMATE_BRIDGE_SERIAL").expect("set KVMATE_BRIDGE_SERIAL");
        assert!(device_present(0x1A86, 0x55D3, &serial));
        assert!(!device_present(0x1A86, 0x55D3, "NOPE"));
        if let Ok(board) = std::env::var("KVMATE_BOARD_SERIAL") {
            println!("board USB on this PC: {}", device_present(0x303A, 0x4004, &board));
        }
    }
}
