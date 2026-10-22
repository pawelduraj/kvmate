//! Secrets shared with the board, kept out of plain sight in `config.json`: encrypted for the
//! current user where the system can do that (Windows), elsewhere stored as hex.

use kvmate_protocol::secure::PairingKey;

use super::random_bytes;
use crate::encoding::{hex, unhex};
use crate::platform;

#[derive(Clone, PartialEq, Eq)]
pub struct WifiSecrets {
    pub key: PairingKey,
    pub network_password: String,
}

impl std::fmt::Debug for WifiSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return f.write_str("WifiSecrets(..)");
    }
}

const PASSWORD_CHARS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
pub(crate) const PLAIN: &str = "plain";

impl WifiSecrets {
    pub fn generate(own_network: bool) -> WifiSecrets {
        let network_password = if own_network {
            random_bytes::<16>()
                .iter()
                .map(|b| return PASSWORD_CHARS[*b as usize % PASSWORD_CHARS.len()] as char)
                .collect()
        } else {
            String::new()
        };
        return WifiSecrets { key: random_bytes(), network_password };
    }

    fn to_bytes(&self) -> Vec<u8> {
        return [&self.key[..], self.network_password.as_bytes()].concat();
    }

    fn from_bytes(b: &[u8]) -> Option<WifiSecrets> {
        let key = b.get(..32)?.try_into().ok()?;
        let network_password = String::from_utf8(b[32..].to_vec()).ok()?;
        return Some(WifiSecrets { key, network_password });
    }

    pub fn protect(&self) -> Option<String> {
        return protect(&self.to_bytes());
    }

    pub(super) fn unprotect(text: &str) -> Option<WifiSecrets> {
        return unprotect(text).and_then(|b| return WifiSecrets::from_bytes(&b));
    }
}

pub fn protect(data: &[u8]) -> Option<String> {
    let sealed = platform::secret::protect(data)?;
    return Some(format!("{}:{}", platform::secret::SCHEME, hex(&sealed)));
}

pub fn unprotect(text: &str) -> Option<Vec<u8>> {
    let (scheme, digits) = text.split_once(':')?;
    let bytes = unhex(digits)?;
    if scheme == PLAIN {
        return Some(bytes);
    }
    if scheme == platform::secret::SCHEME {
        return platform::secret::unprotect(&bytes);
    }
    return None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wifi::{WifiMode, WifiSettings};

    #[test]
    fn secrets_roundtrip() {
        let s = WifiSecrets::generate(true);
        assert_eq!(s.network_password.len(), 16);
        assert!(s.network_password.bytes().all(|c| return PASSWORD_CHARS.contains(&c)));
        let settings = WifiSettings { mode: WifiMode::BoardNetwork, secrets: s.protect(), ..Default::default() };
        assert_eq!(settings.usable(), Some(s.clone()));
        assert_eq!(WifiSettings { mode: WifiMode::Off, ..settings.clone() }.usable(), None);
        assert_ne!(WifiSecrets::generate(false).key, s.key);
    }

    #[test]
    fn roundtrip() {
        let data = b"\x00\x01pairing key and password";
        let text = protect(data).unwrap();
        assert!(platform::secret::SCHEME == PLAIN || !text.contains(&hex(data)), "stored encrypted");
        assert_eq!(unprotect(&text).unwrap(), data);
        assert_eq!(unprotect(&format!("plain:{}", hex(data))).unwrap(), data);
        assert_eq!(unprotect("dpapi:zz"), None);
        assert_eq!(unprotect("something else"), None);
    }
}
