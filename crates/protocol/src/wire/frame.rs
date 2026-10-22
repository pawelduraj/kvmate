use super::{RawData, cobs, crc};
use crate::MsgType;

pub const PROTOCOL_VERSION: u8 = 2;
pub const DEFAULT_BAUD: u32 = 3000000;

pub const HEADER_LEN: usize = 6;
pub const CRC_LEN: usize = 2;
pub const MAX_PAYLOAD: usize = 1024;
pub const MAX_FRAME_LEN: usize = HEADER_LEN + MAX_PAYLOAD + CRC_LEN;
pub const MAX_ENCODED_LEN: usize = MAX_FRAME_LEN + MAX_FRAME_LEN / 254 + 1 + 2;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrameHeader {
    pub protocol_version: u8,
    pub msg_type: u8,
    pub sequence: u16,
    pub payload_len: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    PayloadTooLarge,
    BufferTooSmall,
    TooShort,
    Crc,
    Version(u8),
    Length,
}

#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    pub header: FrameHeader,
    pub payload: &'a [u8],
}

impl<'a> Frame<'a> {
    pub fn msg_type(&self) -> Option<MsgType> {
        return MsgType::from_u8(self.header.msg_type);
    }

    pub fn payload_as<T: RawData>(&self) -> Option<T> {
        return T::read_from(self.payload);
    }

    pub fn split_payload<T: RawData>(&self) -> Option<(T, &'a [u8])> {
        let head = T::read_from(self.payload)?;
        return Some((head, &self.payload[T::SIZE..]));
    }
}

pub fn encode_frame(msg_type: MsgType, seq: u16, parts: &[&[u8]], out: &mut [u8]) -> Result<usize, Error> {
    let payload_len: usize = parts.iter().map(|p| return p.len()).sum();
    if payload_len > MAX_PAYLOAD {
        return Err(Error::PayloadTooLarge);
    }
    let mut raw = [0u8; MAX_FRAME_LEN];
    let header = FrameHeader {
        protocol_version: PROTOCOL_VERSION,
        msg_type: msg_type as u8,
        sequence: seq,
        payload_len: payload_len as u16,
    };
    raw[..HEADER_LEN].copy_from_slice(header.as_bytes());
    let mut pos = HEADER_LEN;
    for part in parts {
        raw[pos..pos + part.len()].copy_from_slice(part);
        pos += part.len();
    }
    let crc = crc::crc16_ccitt_false(&raw[..pos]);
    raw[pos..pos + CRC_LEN].copy_from_slice(&crc.to_le_bytes());
    pos += CRC_LEN;

    if out.len() < 2 {
        return Err(Error::BufferTooSmall);
    }
    out[0] = 0;
    let last = out.len() - 1;
    let n = cobs::encode(&raw[..pos], &mut out[1..last]).ok_or(Error::BufferTooSmall)?;
    out[1 + n] = 0;
    return Ok(n + 2);
}

pub fn parse_frame(raw: &[u8]) -> Result<Frame<'_>, Error> {
    if raw.len() < HEADER_LEN + CRC_LEN {
        return Err(Error::TooShort);
    }
    let body_len = raw.len() - CRC_LEN;
    let expected = u16::from_le_bytes([raw[body_len], raw[body_len + 1]]);
    if crc::crc16_ccitt_false(&raw[..body_len]) != expected {
        return Err(Error::Crc);
    }
    let header = FrameHeader::read_from(raw).ok_or(Error::TooShort)?;
    if header.protocol_version != PROTOCOL_VERSION {
        return Err(Error::Version(header.protocol_version));
    }
    if header.payload_len as usize != body_len - HEADER_LEN {
        return Err(Error::Length);
    }
    return Ok(Frame { header, payload: &raw[HEADER_LEN..body_len] });
}

pub struct FrameDecoder {
    buf: [u8; MAX_ENCODED_LEN],
    len: usize,
    overflow: bool,
    raw: [u8; MAX_FRAME_LEN],
}

impl Default for FrameDecoder {
    fn default() -> Self {
        return Self::new();
    }
}

impl FrameDecoder {
    pub const fn new() -> Self {
        return Self { buf: [0; MAX_ENCODED_LEN], len: 0, overflow: false, raw: [0; MAX_FRAME_LEN] };
    }

    pub fn push(&mut self, data: &[u8], mut on_frame: impl FnMut(Frame<'_>)) {
        for &byte in data {
            if byte != 0 {
                if self.len < self.buf.len() {
                    self.buf[self.len] = byte;
                    self.len += 1;
                } else {
                    self.overflow = true;
                }
                continue;
            }
            let len = core::mem::replace(&mut self.len, 0);
            if core::mem::replace(&mut self.overflow, false) || len == 0 {
                continue;
            }
            let Some(n) = cobs::decode(&self.buf[..len], &mut self.raw) else { continue };
            if let Ok(frame) = parse_frame(&self.raw[..n]) {
                on_frame(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Heartbeat, KeyboardState, MouseReport};

    fn decode_all(wire: &[u8]) -> usize {
        let mut count = 0;
        FrameDecoder::new().push(wire, |_| count += 1);
        return count;
    }

    #[test]
    fn message_roundtrip() {
        let mouse = MouseReport { buttons: 0b101, move_x: -300, move_y: 1200, wheel: -1, wheel_horizontal: 0 };
        let mut wire = [0u8; MAX_ENCODED_LEN];
        let n = encode_frame(MsgType::MouseReport, 42, &[mouse.as_bytes()], &mut wire).unwrap();
        assert_eq!(wire[0], 0);
        assert_eq!(wire[n - 1], 0);

        let mut got = None;
        FrameDecoder::new().push(&wire[..n], |f| {
            assert_eq!(f.msg_type(), Some(MsgType::MouseReport));
            assert_eq!({ f.header.sequence }, 42);
            got = f.payload_as::<MouseReport>();
        });
        assert_eq!(got, Some(mouse));
    }

    #[test]
    fn resyncs_after_garbage_and_split_reads() {
        let mut wire = [0u8; 256];
        let mut pos = 0;
        let garbage = b"ESP-ROM:esp32s3-20210327\r\nrst:0x1 (POWERON)";
        wire[..garbage.len()].copy_from_slice(garbage);
        pos += garbage.len();
        let heartbeat = Heartbeat { flags: 0 };
        pos += encode_frame(MsgType::Heartbeat, 1, &[heartbeat.as_bytes()], &mut wire[pos..]).unwrap();
        let mut keys = KeyboardState::default();
        keys.set(0x04, true);
        keys.set(0xE1, true);
        pos += encode_frame(MsgType::KeyboardState, 2, &[keys.as_bytes()], &mut wire[pos..]).unwrap();

        let mut dec = FrameDecoder::new();
        let mut seqs = [0u16; 4];
        let mut count = 0;
        for byte in &wire[..pos] {
            dec.push(core::slice::from_ref(byte), |f| {
                seqs[count] = f.header.sequence;
                count += 1;
            });
        }
        assert_eq!(&seqs[..count], &[1, 2]);
    }

    #[test]
    fn rejects_corrupted_frame() {
        let mut wire = [0u8; 64];
        let n = encode_frame(MsgType::Heartbeat, 7, &[Heartbeat::default().as_bytes()], &mut wire).unwrap();
        assert_eq!(decode_all(&wire[..n]), 1);
        wire[5] ^= 0x40;
        assert_eq!(decode_all(&wire[..n]), 0);
    }

    #[test]
    fn variable_length_payload() {
        let mut wire = [0u8; MAX_ENCODED_LEN];
        let head = crate::LogHeader { level: 3 };
        let n = encode_frame(MsgType::Log, 9, &[head.as_bytes(), b"hello"], &mut wire).unwrap();
        let mut dec = FrameDecoder::new();
        let mut ok = false;
        dec.push(&wire[..n], |f| {
            let (h, text) = f.split_payload::<crate::LogHeader>().unwrap();
            assert_eq!(h.level, 3);
            assert_eq!(text, b"hello");
            ok = true;
        });
        assert!(ok);
    }

    #[test]
    fn max_payload_fits() {
        let payload = [0xA5u8; MAX_PAYLOAD];
        let mut wire = [0u8; MAX_ENCODED_LEN];
        let n = encode_frame(MsgType::AudioPlayback, 0, &[&payload], &mut wire).unwrap();
        assert!(n <= MAX_ENCODED_LEN);
        assert_eq!(encode_frame(MsgType::AudioPlayback, 0, &[&payload, &[1]], &mut wire), Err(Error::PayloadTooLarge));
    }

    #[test]
    fn rejects_wrong_version() {
        let mut raw = [0u8; HEADER_LEN + CRC_LEN];
        let header = FrameHeader { protocol_version: 99, msg_type: MsgType::Hello as u8, sequence: 0, payload_len: 0 };
        raw[..HEADER_LEN].copy_from_slice(header.as_bytes());
        let crc = crc::crc16_ccitt_false(&raw[..HEADER_LEN]);
        raw[HEADER_LEN..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(parse_frame(&raw).unwrap_err(), Error::Version(99));
    }
}
