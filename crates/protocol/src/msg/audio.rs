pub const AUDIO_RATE_HZ: u32 = 48000;
pub const AUDIO_PLAYBACK_CHANNELS: u8 = 2;
pub const AUDIO_CAPTURE_CHANNELS: u8 = 1;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AudioHeader {
    pub first_frame_number: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct AudioStatus {
    pub host_playing: u8,
    pub host_recording: u8,
    pub mic_queued_frames: u16,
    pub host_volume: i16,
    pub host_muted: u8,
}
