pub(crate) mod audio_activity;
pub(crate) mod input;
pub(crate) mod secret;
pub(crate) mod serial;
pub(crate) mod usb;
pub(crate) mod wlan;

fn wide(s: &str) -> Vec<u16> {
    return s.encode_utf16().chain(Some(0)).collect();
}
