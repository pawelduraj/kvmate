use crate::secure::{DGRAM_DATA_HEADER_LEN, DgramDataHeader, DgramHello, DgramReply};
use crate::*;

pub trait RawData: bytemuck::Pod {
    const SIZE: usize = core::mem::size_of::<Self>();

    fn as_bytes(&self) -> &[u8] {
        return bytemuck::bytes_of(self);
    }

    fn read_from(bytes: &[u8]) -> Option<Self> {
        return Some(bytemuck::pod_read_unaligned(bytes.get(..Self::SIZE)?));
    }
}

macro_rules! raw_data {
    ($($ty:ident = $size:expr),* $(,)?) => {
        $(
            impl RawData for $ty {}
            const _: () = assert!(core::mem::size_of::<$ty>() == $size);
        )*

        pub const WIRE_STRUCTS: &[(&str, usize)] = &[$((stringify!($ty), $size)),*];
    };
}

raw_data! {
    FrameHeader = HEADER_LEN,
    DeviceInfo = 18,
    Heartbeat = 1,
    KeyboardState = 32,
    MouseReport = 7,
    ConsumerState = 2,
    AudioHeader = 4,
    AudioStatus = 7,
    ButtonEvent = 1,
    SetLed = 3,
    DeviceConfig = 2,
    UsbNames = 66,
    LogHeader = 1,
    WifiConfig = 165,
    WifiStatus = 43,
    WifiScanHeader = 1,
    WifiNetwork = 36,
    DgramHello = 30,
    DgramReply = 69,
    DgramDataHeader = DGRAM_DATA_HEADER_LEN,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn little_endian_layout() {
        let m = MouseReport { buttons: 1, move_x: 0x0102, move_y: -2, wheel: 3, wheel_horizontal: -4 };
        assert_eq!(m.as_bytes(), &[1, 0x02, 0x01, 0xFE, 0xFF, 3, 0xFC]);
        assert_eq!(MouseReport::read_from(m.as_bytes()), Some(m));
        assert_eq!(MouseReport::read_from(&m.as_bytes()[..6]), None);
    }
}
