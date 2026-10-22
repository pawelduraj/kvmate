//! Stand-in for the sound card streams in builds without the `audio-devices` feature.

use kvmate_protocol::{AudioHeader, AudioStatus};

use super::{AudioConfig, AudioView, DeviceList, StreamMode};
use crate::board::link::LinkSender;

#[derive(Default)]
pub(super) struct Devices;

impl Devices {
    pub(super) fn apply(&mut self, _: &AudioConfig, _: &AudioStatus, _: Option<&LinkSender>, _: bool) {}

    pub(super) fn on_board_report(&mut self, _: &AudioStatus) {}

    pub(super) fn push_playback(&mut self, _: &AudioHeader, _: &[u8]) {}

    pub(super) fn fill_view(&self, v: &mut AudioView) {
        for (mode, stream) in [(v.playback_mode, &mut v.playback), (v.capture_mode, &mut v.capture)] {
            if mode != StreamMode::Off {
                stream.error = Some("this build has no sound support".into());
            }
        }
    }

    pub(super) fn close(&mut self, _playback: bool, _capture: bool) {}
}

pub(super) fn list() -> DeviceList {
    return DeviceList::default();
}
