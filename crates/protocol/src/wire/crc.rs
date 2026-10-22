const TABLE: [u16; 256] = build_table();

const fn build_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    return table;
}

pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc = (crc << 8) ^ TABLE[((crc >> 8) as u8 ^ byte) as usize];
    }
    return crc;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_value() {
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29B1);
        assert_eq!(crc16_ccitt_false(&[]), 0xFFFF);
    }
}
