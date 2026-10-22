pub fn encode(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    if dst.is_empty() {
        return None;
    }
    let mut code_idx = 0;
    let mut out = 1;
    let mut code: u8 = 1;
    for &byte in src {
        if byte == 0 {
            dst[code_idx] = code;
            code_idx = out;
            out += 1;
            code = 1;
        } else {
            *dst.get_mut(out)? = byte;
            out += 1;
            code += 1;
            if code == 0xFF {
                dst[code_idx] = code;
                code_idx = out;
                out += 1;
                code = 1;
            }
        }
        if code_idx >= dst.len() {
            return None;
        }
    }
    dst[code_idx] = code;
    return Some(out);
}

pub fn decode(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut i = 0;
    let mut out = 0;
    while i < src.len() {
        let code = src[i];
        if code == 0 {
            return None;
        }
        i += 1;
        for _ in 1..code {
            *dst.get_mut(out)? = *src.get(i)?;
            out += 1;
            i += 1;
        }
        if code < 0xFF && i < src.len() {
            *dst.get_mut(out)? = 0;
            out += 1;
        }
    }
    return Some(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(input: &[u8]) {
        let mut enc = [0u8; 2048];
        let mut dec = [0u8; 2048];
        let n = encode(input, &mut enc).unwrap();
        assert!(n <= input.len() + input.len() / 254 + 1, "worst-case COBS overhead");
        assert!(!enc[..n].contains(&0), "encoded data must not contain 0x00");
        let m = decode(&enc[..n], &mut dec).unwrap();
        assert_eq!(&dec[..m], input);
    }

    #[test]
    fn known_vectors() {
        let mut enc = [0u8; 16];
        let n = encode(&[0x11, 0x22, 0x00, 0x33], &mut enc).unwrap();
        assert_eq!(&enc[..n], &[0x03, 0x11, 0x22, 0x02, 0x33]);
        let n = encode(&[0x00], &mut enc).unwrap();
        assert_eq!(&enc[..n], &[0x01, 0x01]);
        let n = encode(&[], &mut enc).unwrap();
        assert_eq!(&enc[..n], &[0x01]);
    }

    #[test]
    fn roundtrips() {
        roundtrip(&[]);
        roundtrip(&[0]);
        roundtrip(&[0, 0, 0]);
        roundtrip(&[1, 2, 3, 0, 4, 5]);
        let long: [u8; 600] = core::array::from_fn(|i| return (i % 256) as u8);
        roundtrip(&long);
        let nonzero: [u8; 600] = core::array::from_fn(|i| return (i % 255) as u8 + 1);
        roundtrip(&nonzero);
        roundtrip(&nonzero[..254]);
        roundtrip(&nonzero[..255]);
    }

    #[test]
    fn rejects_malformed() {
        let mut dec = [0u8; 16];
        assert_eq!(decode(&[0x05, 0x11], &mut dec), None);
        assert_eq!(decode(&[0x00], &mut dec), None);
    }

    #[test]
    fn rejects_small_buffer() {
        let mut enc = [0u8; 3];
        assert_eq!(encode(&[1, 2, 3, 4], &mut enc), None);
    }
}
