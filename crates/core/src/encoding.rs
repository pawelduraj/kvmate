pub fn hex(bytes: &[u8]) -> String {
    return bytes.iter().map(|b| format!("{b:02x}")).collect();
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.bytes().all(|b| return b.is_ascii_hexdigit()) {
        return None;
    }
    return (0..s.len()).step_by(2).map(|i| return u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(hex(&[0x00, 0xab, 0x7f]), "00ab7f");
        assert_eq!(unhex("00ab7f"), Some(vec![0x00, 0xab, 0x7f]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
        assert_eq!(unhex("+f"), None);
    }
}
