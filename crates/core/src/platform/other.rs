//! Systems other than Windows. Input capture (Linux: evdev, macOS: CGEventTap), audio activity,
//! the Wi-Fi card and the USB check are not written yet.

pub(crate) mod input {
    use std::io;
    use std::sync::Arc;

    use crate::input::{CaptureControl, InputSink};

    pub const SYSTEM_KEEPS_COMBOS: bool = false;

    pub struct Capture;

    impl Capture {
        pub fn refresh(&self) {}

        pub fn release_stuck_keys(&self) -> Result<Vec<&'static str>, Vec<&'static str>> {
            return Ok(Vec::new());
        }
    }

    pub fn start_capture(_control: Arc<CaptureControl>, _sink: InputSink) -> io::Result<Capture> {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "input capture is not implemented on this platform yet",
        ));
    }
}

pub(crate) mod audio_activity {
    use crate::audio::activity::LocalActivity;

    pub struct Probe;

    impl Probe {
        pub fn new() -> Option<Probe> {
            return None;
        }

        pub fn resolve(&mut self, _output: Option<&str>, _input: Option<&str>) {}

        pub fn sample(&self) -> LocalActivity {
            return LocalActivity::default();
        }
    }
}

pub(crate) mod wlan {
    use crate::wifi::wlan::{Adapter, ProfileInfo};

    pub fn check(_ssid: &str, _password: &str, _connect: bool) -> Adapter {
        return Adapter::Error(
            "connecting the Wi-Fi adapter works on Windows only for now — join the board's network manually".into(),
        );
    }

    pub fn profiles() -> Vec<(ProfileInfo, bool)> {
        return Vec::new();
    }

    pub fn delete_profile(_ssid: &str) {}
}

pub(crate) mod secret {
    pub const SCHEME: &str = crate::wifi::secret::PLAIN;

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        return Some(data.to_vec());
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        return Some(data.to_vec());
    }
}

pub(crate) mod usb {
    pub fn device_present(_vid: u16, _pid: u16, _serial: &str) -> bool {
        return false;
    }
}

pub(crate) mod serial {
    use std::io;

    use crate::board::transport::{Reader, Writer};

    pub fn split(port: Box<dyn serialport::SerialPort>) -> io::Result<(Reader, Writer)> {
        let reader = port.try_clone().map_err(io::Error::from)?;
        return Ok((Box::new(reader), Box::new(port)));
    }
}
