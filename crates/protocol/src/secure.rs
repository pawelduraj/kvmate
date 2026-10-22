//! Encrypted Wi-Fi datagrams. A data datagram carries whole frames, the same bytes as on the cable.
//! Both sides pick new nonces for every session, so it gets new keys and its counters can start at 0.

use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, Tag};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::RawData;

pub const WIFI_UDP_PORT: u16 = 47831;
pub const DGRAM_MAGIC: u32 = 0x544D_564B;
pub const DGRAM_HELLO: u8 = 1;
pub const DGRAM_REPLY: u8 = 2;
pub const DGRAM_DATA: u8 = 3;
pub const DGRAM_VERSION: u8 = 2;

pub const PAIRING_KEY_LEN: usize = 32;
pub const DGRAM_NONCE_LEN: usize = 16;
pub const DGRAM_TAG_LEN: usize = 16;
pub const DGRAM_MAX_PLAIN: usize = 1400;
pub const DGRAM_DATA_HEADER_LEN: usize = 13;
pub const DGRAM_MAX_LEN: usize = DGRAM_MAX_PLAIN + DGRAM_DATA_HEADER_LEN + DGRAM_TAG_LEN;
pub const DGRAM_HELLO_INTERVAL_MS: u32 = 500;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DgramHello {
    pub magic: u32,
    pub kind: u8,
    pub version: u8,
    pub key_id: [u8; 8],
    pub hub_nonce: [u8; DGRAM_NONCE_LEN],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DgramReply {
    pub magic: u32,
    pub kind: u8,
    pub hub_nonce: [u8; DGRAM_NONCE_LEN],
    pub board_nonce: [u8; DGRAM_NONCE_LEN],
    pub reply_mac: [u8; 32],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DgramDataHeader {
    pub kind: u8,
    pub session_id: u32,
    pub counter: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ReplayWindow {
    next: u64,
    seen: u64,
}

impl ReplayWindow {
    fn is_new(&self, counter: u64) -> bool {
        if counter >= self.next {
            return true;
        }
        let age = self.next - 1 - counter;
        return age < 64 && self.seen & (1 << age) == 0;
    }

    fn record_decrypted(&mut self, counter: u64) {
        if counter >= self.next {
            let shift = counter - self.next + 1;
            self.seen = if shift >= 64 { 0 } else { self.seen << shift };
            self.seen |= 1;
            self.next = counter + 1;
        } else {
            self.seen |= 1 << (self.next - 1 - counter);
        }
    }
}

type HmacSha256 = Hmac<Sha256>;

pub type PairingKey = [u8; PAIRING_KEY_LEN];

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for part in parts {
        mac.update(part);
    }
    return mac.finalize().into_bytes().into();
}

pub fn key_id(key: &PairingKey) -> [u8; 8] {
    return hmac(key, &[b"kvmate/key-id"])[..8].try_into().unwrap();
}

pub fn reply_mac(key: &PairingKey, hub_nonce: &[u8; 16], board_nonce: &[u8; 16]) -> [u8; 32] {
    return hmac(key, &[b"kvmate/reply", hub_nonce, board_nonce]);
}

struct SessionKeys {
    hub_to_board: [u8; 32],
    board_to_hub: [u8; 32],
}

fn session_keys(key: &PairingKey, hub_nonce: &[u8; 16], board_nonce: &[u8; 16]) -> SessionKeys {
    let mut salt = [0u8; 32];
    salt[..16].copy_from_slice(hub_nonce);
    salt[16..].copy_from_slice(board_nonce);
    let prk = hmac(&salt, &[key]);
    return SessionKeys {
        hub_to_board: hmac(&prk, &[b"kvmate/hub->board", &[1]]),
        board_to_hub: hmac(&prk, &[b"kvmate/board->hub", &[1]]),
    };
}

fn session_id(board_nonce: &[u8; 16]) -> u32 {
    return u32::from_le_bytes(board_nonce[..4].try_into().unwrap());
}

pub fn hello(key: &PairingKey, hub_nonce: [u8; 16]) -> DgramHello {
    return DgramHello {
        magic: DGRAM_MAGIC,
        kind: DGRAM_HELLO,
        version: DGRAM_VERSION,
        key_id: key_id(key),
        hub_nonce,
    };
}

pub fn reply(key: &PairingKey, hub_nonce: [u8; 16], board_nonce: [u8; 16]) -> DgramReply {
    return DgramReply {
        magic: DGRAM_MAGIC,
        kind: DGRAM_REPLY,
        hub_nonce,
        board_nonce,
        reply_mac: reply_mac(key, &hub_nonce, &board_nonce),
    };
}

pub fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    return a.iter().zip(b).fold(0u8, |acc, (x, y)| return acc | (x ^ y)) == 0;
}

fn nonce(counter: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&counter.to_le_bytes());
    return n;
}

pub struct Sealer {
    cipher: Aes256Gcm,
    session_id: u32,
    counter: u64,
}

impl Sealer {
    pub fn new(key: &[u8; 32], session_id: u32) -> Sealer {
        return Sealer { cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)), session_id, counter: 0 };
    }

    pub fn seal(&mut self, plain: &[u8], out: &mut [u8]) -> Option<usize> {
        let total = DgramDataHeader::SIZE + plain.len() + DGRAM_TAG_LEN;
        if plain.len() > DGRAM_MAX_PLAIN || out.len() < total {
            return None;
        }
        let header = DgramDataHeader { kind: DGRAM_DATA, session_id: self.session_id, counter: self.counter };
        self.counter += 1;
        let (head, rest) = out.split_at_mut(DgramDataHeader::SIZE);
        head.copy_from_slice(header.as_bytes());
        let (body, tag) = rest.split_at_mut(plain.len());
        body.copy_from_slice(plain);
        let n = nonce(header.counter);
        let t = self.cipher.encrypt_in_place_detached(Nonce::from_slice(&n), head, body).ok()?;
        tag[..DGRAM_TAG_LEN].copy_from_slice(&t);
        return Some(total);
    }
}

pub struct Opener {
    cipher: Aes256Gcm,
    session_id: u32,
    window: ReplayWindow,
}

impl Opener {
    pub fn new(key: &[u8; 32], session_id: u32) -> Opener {
        return Opener {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)),
            session_id,
            window: ReplayWindow::default(),
        };
    }

    pub fn open_in_place<'a>(&mut self, dgram: &'a mut [u8]) -> Option<&'a [u8]> {
        if dgram.len() < DgramDataHeader::SIZE + DGRAM_TAG_LEN {
            return None;
        }
        let header = DgramDataHeader::read_from(dgram)?;
        if header.kind != DGRAM_DATA || header.session_id != self.session_id || !self.window.is_new(header.counter) {
            return None;
        }
        let (head, rest) = dgram.split_at_mut(DgramDataHeader::SIZE);
        let body_len = rest.len() - DGRAM_TAG_LEN;
        let (body, tag) = rest.split_at_mut(body_len);
        let n = nonce(header.counter);
        let tag = Tag::clone_from_slice(tag);
        self.cipher.decrypt_in_place_detached(Nonce::from_slice(&n), head, body, &tag).ok()?;
        self.window.record_decrypted(header.counter);
        return Some(body);
    }
}

pub struct Session {
    pub tx: Sealer,
    pub rx: Opener,
}

impl Session {
    pub fn for_hub(key: &PairingKey, hub_nonce: &[u8; 16], board_nonce: &[u8; 16]) -> Session {
        let keys = session_keys(key, hub_nonce, board_nonce);
        let id = session_id(board_nonce);
        return Session { tx: Sealer::new(&keys.hub_to_board, id), rx: Opener::new(&keys.board_to_hub, id) };
    }

    pub fn for_board(key: &PairingKey, hub_nonce: &[u8; 16], board_nonce: &[u8; 16]) -> Session {
        let keys = session_keys(key, hub_nonce, board_nonce);
        let id = session_id(board_nonce);
        return Session { tx: Sealer::new(&keys.board_to_hub, id), rx: Opener::new(&keys.hub_to_board, id) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_window() {
        let mut w = ReplayWindow::default();
        for c in [0u64, 1, 2, 5, 4] {
            assert!(w.is_new(c), "{c}");
            w.record_decrypted(c);
        }
        assert!(!w.is_new(4) && !w.is_new(5) && !w.is_new(0));
        assert!(w.is_new(3), "a late datagram inside the window is still accepted");
        w.record_decrypted(100);
        assert!(!w.is_new(3), "too old after a jump");
        assert!(w.is_new(99) && w.is_new(37) && !w.is_new(36));
    }

    #[test]
    fn handshake_and_data() {
        let key: PairingKey = core::array::from_fn(|i| return i as u8);
        let (hn, bn) = ([1u8; 16], [2u8; 16]);
        let hello = hello(&key, hn);
        assert_eq!(hello.key_id, key_id(&key));
        let reply = reply(&key, hn, bn);
        assert!(constant_time_eq(&{ reply.reply_mac }, &reply_mac(&key, &hn, &bn)));
        assert!(!constant_time_eq(&{ reply.reply_mac }, &reply_mac(&[9; 32], &hn, &bn)));

        let mut hub = Session::for_hub(&key, &hn, &bn);
        let mut board = Session::for_board(&key, &hn, &bn);
        let mut buf = [0u8; DGRAM_MAX_LEN];

        let n = hub.tx.seal(b"\0frame\0", &mut buf).unwrap();
        let mut copy = buf;
        assert_eq!(board.rx.open_in_place(&mut buf[..n]), Some(&b"\0frame\0"[..]));
        assert_eq!(board.rx.open_in_place(&mut copy[..n]), None, "replayed");

        let n = board.tx.seal(b"reply", &mut buf).unwrap();
        buf[DgramDataHeader::SIZE] ^= 1;
        assert_eq!(hub.rx.open_in_place(&mut buf[..n]), None);
        let n = board.tx.seal(b"reply", &mut buf).unwrap();
        assert_eq!(hub.rx.open_in_place(&mut buf[..n]), Some(&b"reply"[..]));

        let n = hub.tx.seal(b"x", &mut buf).unwrap();
        assert_eq!(hub.rx.open_in_place(&mut buf[..n]), None);
        let mut other = Session::for_board(&key, &hn, &[3; 16]);
        let n = hub.tx.seal(b"x", &mut buf).unwrap();
        assert_eq!(other.rx.open_in_place(&mut buf[..n]), None);

        assert!(hub.tx.seal(&[0; DGRAM_MAX_PLAIN + 1], &mut [0; 2000]).is_none());
    }
}
