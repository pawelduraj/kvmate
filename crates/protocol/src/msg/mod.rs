mod audio;
mod board;
mod input;
mod session;
mod wifi;

pub use audio::*;
pub use board::*;
pub use input::*;
pub use session::*;
pub use wifi::*;

use crate::wire::{Error, Frame, RawData, encode_frame};

pub const HEARTBEAT_INTERVAL_MS: u32 = 250;
pub const LINK_TIMEOUT_MS: u32 = 1500;

pub fn name_bytes(bytes: &[u8], len: u8) -> &[u8] {
    return &bytes[..(len as usize).min(bytes.len())];
}

macro_rules! messages {
    ($($name:ident = $code:literal $(($payload:ident $(, $data:ty)?))?,)*) => {
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum MsgType {
            $($name = $code,)*
        }

        impl MsgType {
            pub const ALL: &[(MsgType, &str)] = &[$((MsgType::$name, stringify!($name)),)*];

            pub fn from_u8(value: u8) -> Option<MsgType> {
                return match value {
                    $($code => Some(MsgType::$name),)*
                    _ => None,
                };
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum Msg<'a> {
            $($name $(($payload $(, &'a $data)?))?,)*
        }

        impl<'a> Msg<'a> {
            pub fn msg_type(&self) -> MsgType {
                return match self {
                    $(Msg::$name { .. } => MsgType::$name,)*
                };
            }

            pub fn parse(frame: &Frame<'a>) -> Option<Msg<'a>> {
                return Some(match frame.msg_type()? {
                    $(MsgType::$name => parse_payload!(frame, $name $(, $payload $(, $data)?)?),)*
                });
            }

            pub fn parts(&self) -> [&[u8]; 2] {
                return payload_parts!(self; $($name $(($payload $(, $data)?))?;)*);
            }

            pub fn encode(&self, seq: u16, out: &mut [u8]) -> Result<usize, Error> {
                return encode_frame(self.msg_type(), seq, &self.parts(), out);
            }
        }
    };
}

macro_rules! parse_payload {
    ($frame:ident, $name:ident) => {
        Msg::$name
    };
    ($frame:ident, $name:ident, $payload:ident) => {
        Msg::$name($frame.payload_as::<$payload>()?)
    };
    ($frame:ident, $name:ident, $payload:ident, $data:ty) => {{
        let (head, data) = $frame.split_payload::<$payload>()?;
        Msg::$name(head, data)
    }};
}

macro_rules! payload_parts {
    ($msg:ident; $($entries:tt)*) => {
        payload_parts!(@arms $msg [] $($entries)*)
    };
    (@arms $msg:ident [$($arms:tt)*]) => {
        match $msg { $($arms)* }
    };
    (@arms $msg:ident [$($arms:tt)*] $name:ident; $($rest:tt)*) => {
        payload_parts!(@arms $msg [$($arms)* Msg::$name => [&[], &[]],] $($rest)*)
    };
    (@arms $msg:ident [$($arms:tt)*] $name:ident($payload:ident); $($rest:tt)*) => {
        payload_parts!(@arms $msg [$($arms)* Msg::$name(m) => [m.as_bytes(), &[]],] $($rest)*)
    };
    (@arms $msg:ident [$($arms:tt)*] $name:ident($payload:ident, $data:ty); $($rest:tt)*) => {
        payload_parts!(@arms $msg [$($arms)* Msg::$name(head, data) => [head.as_bytes(), *data],] $($rest)*)
    };
}

messages! {
    Hello = 0x01,
    DeviceInfo = 0x02 (DeviceInfo),
    Heartbeat = 0x03 (Heartbeat),

    KeyboardState = 0x10 (KeyboardState),
    MouseReport = 0x11 (MouseReport),
    ConsumerState = 0x12 (ConsumerState),
    ReleaseAll = 0x13,

    AudioPlayback = 0x20 (AudioHeader, [u8]),
    AudioCapture = 0x21 (AudioHeader, [u8]),
    AudioStatus = 0x22 (AudioStatus),

    ButtonEvent = 0x30 (ButtonEvent),
    SetLed = 0x31 (SetLed),
    DeviceConfig = 0x32 (DeviceConfig),
    UsbNames = 0x33 (UsbNames),

    Log = 0x3F (LogHeader, [u8]),

    WifiConfig = 0x40 (WifiConfig),
    WifiScan = 0x41,
    WifiScanResult = 0x42 (WifiScanHeader, [u8]),
    WifiStatus = 0x43 (WifiStatus),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameDecoder, MAX_ENCODED_LEN};

    #[test]
    fn msg_type_codes() {
        for &(t, _) in MsgType::ALL {
            assert_eq!(MsgType::from_u8(t as u8), Some(t));
        }
        assert_eq!(MsgType::from_u8(0xFF), None);
    }

    #[test]
    fn msg_roundtrip() {
        let mouse = MouseReport { buttons: 1, move_x: -3, move_y: 7, wheel: 0, wheel_horizontal: 0 };
        let head = LogHeader { level: LOG_WARN };
        for msg in [Msg::MouseReport(mouse), Msg::Log(head, b"low battery"), Msg::ReleaseAll] {
            let mut wire = [0u8; MAX_ENCODED_LEN];
            let n = msg.encode(5, &mut wire).unwrap();
            let mut seen = false;
            FrameDecoder::new().push(&wire[..n], |frame| {
                assert_eq!(Msg::parse(&frame), Some(msg));
                seen = true;
            });
            assert!(seen);
        }
    }
}
