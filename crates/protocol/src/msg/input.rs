#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct KeyboardState {
    pub usage_bitmap: [u8; 32],
}

impl KeyboardState {
    pub fn set(&mut self, usage: u8, pressed: bool) {
        let (byte, bit) = (usage as usize / 8, usage % 8);
        if pressed {
            self.usage_bitmap[byte] |= 1 << bit;
        } else {
            self.usage_bitmap[byte] &= !(1 << bit);
        }
    }

    pub fn is_pressed(&self, usage: u8) -> bool {
        return self.usage_bitmap[usage as usize / 8] & (1 << (usage % 8)) != 0;
    }

    pub fn is_empty(&self) -> bool {
        return self.usage_bitmap.iter().all(|&b| return b == 0);
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MouseReport {
    pub buttons: u8,
    pub move_x: i16,
    pub move_y: i16,
    pub wheel: i8,
    pub wheel_horizontal: i8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ConsumerState {
    pub usage: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_bitmap() {
        let mut k = KeyboardState::default();
        k.set(0x04, true);
        k.set(0xE0, true);
        k.set(0xE7, true);
        assert!(k.is_pressed(0x04));
        assert_eq!(k.usage_bitmap[0xE0 / 8], 0b1000_0001, "modifier byte in boot-protocol order");
        k.set(0x04, false);
        assert!(!k.is_pressed(0x04));
        k.set(0xE0, false);
        k.set(0xE7, false);
        assert!(k.is_empty());
    }
}
