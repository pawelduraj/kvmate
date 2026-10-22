//! Hub microphone -> controlled computer.
//!
//! The level kept at the target is the board's queue plus everything on its way there: samples
//! waiting here for a full chunk and samples the microphone recorded since its last callback.
//! Without those the level would seem to wander by several milliseconds, because chunks, 10 ms
//! device callbacks and 20 ms board reports don't line up. So the target is the whole microphone
//! delay kvmate adds.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use kvmate_protocol::AudioHeader;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Adjustable, Async, FixedAsync, Resampler};

use super::RATE;
use super::drift::{self, DriftController};

const CHUNK_FRAMES: usize = 192;
const MAX_EXTRAPOLATION: Duration = Duration::from_millis(40);

pub struct CapturePath {
    in_rate: u32,
    input: VecDeque<f32>,
    last_input: Option<Instant>,
    resampler: Async<f32>,
    in_buf: Vec<f32>,
    out_buf: Vec<f32>,
    next_frame: u32,
    controller: DriftController,
    last_report: Option<Instant>,
    active: bool,
}

impl CapturePath {
    pub fn new(in_rate: u32, target_ms: u32) -> CapturePath {
        let resampler = drift::resampler(RATE as f64 / in_rate as f64, CHUNK_FRAMES, 1, FixedAsync::Output);
        let in_max = resampler.input_frames_max();
        let target = drift::target_frames(target_ms, CHUNK_FRAMES);
        return CapturePath {
            in_rate,
            input: VecDeque::with_capacity(in_max * 4),
            last_input: None,
            resampler,
            in_buf: vec![0.0; in_max],
            out_buf: vec![0.0; CHUNK_FRAMES],
            next_frame: 0,
            controller: DriftController::new(target as f64, RATE as f64),
            last_report: None,
            active: true,
        };
    }

    pub fn drift_ppm(&self) -> f32 {
        return self.controller.drift_ppm() as f32;
    }

    pub fn level_ms(&self) -> Option<f32> {
        return self.controller.level_ms().map(|ms| return ms as f32);
    }

    pub fn set_active(&mut self, active: bool) {
        if active && !self.active {
            self.controller.reset_level();
            self.last_report = None;
        }
        if !active {
            self.input.clear();
        }
        self.active = active;
    }

    pub fn push_input(&mut self, data: &[f32], channels: usize, now: Instant) {
        if !self.active {
            return;
        }
        let channels = channels.max(1);
        let scale = 1.0 / channels as f32;
        self.input.extend(data.chunks_exact(channels).map(|f| return f.iter().sum::<f32>() * scale));
        self.last_input = Some(now);
    }

    pub fn on_board_fill(&mut self, fill_frames: u16, now: Instant) {
        if !self.active {
            return;
        }
        let dt = match self.last_report.replace(now) {
            Some(prev) => now.saturating_duration_since(prev).as_secs_f64(),
            None => 0.0,
        };
        let recording = drift::seconds_since(self.last_input, now, MAX_EXTRAPOLATION);
        let pending = (self.input.len() as f64 / self.in_rate as f64 + recording) * RATE as f64;
        let ratio = self.controller.update(fill_frames as f64 + pending, dt);
        let _ = self.resampler.set_resample_ratio_relative(ratio, true);
    }

    pub fn next_chunk(&mut self) -> Option<(AudioHeader, Vec<u8>)> {
        let need = self.resampler.input_frames_next();
        if self.input.len() < need {
            return None;
        }
        drift::take_into(&mut self.in_buf[..need], &mut self.input);
        let input = InterleavedSlice::new(&self.in_buf[..need], 1, need).ok()?;
        let mut output = InterleavedSlice::new_mut(&mut self.out_buf[..], 1, CHUNK_FRAMES).ok()?;
        let (_, written) = self.resampler.process_into_buffer(&input, &mut output, None).ok()?;

        let head = AudioHeader { first_frame_number: self.next_frame };
        self.next_frame = self.next_frame.wrapping_add(written as u32);
        let pcm = self.out_buf[..written]
            .iter()
            .flat_map(|&s| return ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        return Some((head, pcm));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mic_rate: u32, mic_ppm: f64, usb_ppm: f64, seconds: f64) -> (CapturePath, f64, f64) {
        let t0 = Instant::now();
        let at = |t: f64| return t0 + Duration::from_secs_f64(t);
        let mut c = CapturePath::new(mic_rate, 25);
        let mic_actual = mic_rate as f64 * (1.0 + mic_ppm * 1e-6);
        let usb_actual = RATE as f64 * (1.0 + usb_ppm * 1e-6);
        let mic_period = 0.01 * mic_rate as f64 / mic_actual;
        let usb_period = 0.001 * RATE as f64 / usb_actual;
        let report_period = 0.02 * RATE as f64 / usb_actual;
        let (mut next_mic, mut next_usb, mut next_report) = (mic_period, usb_period, report_period);
        let mic_frames = mic_rate as usize / 100;
        let mut board_fifo: f64 = 480.0;
        let (mut min, mut max) = (f64::MAX, f64::MIN);
        loop {
            let t = next_mic.min(next_usb).min(next_report);
            if t > seconds {
                break;
            }
            if t == next_mic {
                c.push_input(&vec![0.1; mic_frames * 2], 2, at(t));
                while let Some((_, pcm)) = c.next_chunk() {
                    board_fifo += (pcm.len() / 2) as f64;
                }
                next_mic += mic_period;
            } else if t == next_usb {
                board_fifo = (board_fifo - 48.0).max(0.0);
                next_usb += usb_period;
            } else {
                c.on_board_fill(board_fifo as u16, at(t));
                next_report += report_period;
            }
            if t > seconds / 2.0 {
                min = min.min(board_fifo);
                max = max.max(board_fifo);
            }
        }
        return (c, min, max);
    }

    #[test]
    fn keeps_board_level_with_drifting_clocks() {
        let (c, min, max) = run(48000, 120.0, -150.0, 120.0);
        assert!(min > 400.0 && max < 1300.0, "board queue {min}..{max} frames");
        assert!((c.drift_ppm() - 270.0).abs() < 20.0, "drift {}", c.drift_ppm());
    }

    #[test]
    fn keeps_board_level_with_44k1_microphone() {
        let (c, min, max) = run(44100, -90.0, 60.0, 120.0);
        assert!(min > 400.0 && max < 1300.0, "board queue {min}..{max} frames");
        assert!((c.drift_ppm() + 150.0).abs() < 20.0, "drift {}", c.drift_ppm());
    }

    #[test]
    fn chunks_are_numbered_and_stereo_is_mixed() {
        let mut c = CapturePath::new(44100, 20);
        let stereo: Vec<f32> = (0..44100 / 10).flat_map(|_| return [0.5, 0.0]).collect();
        c.push_input(&stereo, 2, Instant::now());
        let mut frames = 0;
        let mut last = Vec::new();
        while let Some((head, pcm)) = c.next_chunk() {
            assert_eq!({ head.first_frame_number } as usize, frames);
            frames += pcm.len() / 2;
            last = pcm;
        }
        assert!((4300..=4800).contains(&frames), "{frames} frames");
        let sample = i16::from_le_bytes([last[100], last[101]]);
        assert!((sample - 8192).abs() < 100, "mono mix of 0.5 and 0: {sample}");
    }

    #[test]
    fn nothing_is_sent_while_the_laptop_does_not_record() {
        let mut c = CapturePath::new(48000, 20);
        c.set_active(false);
        c.push_input(&[0.1; 4800], 1, Instant::now());
        c.on_board_fill(3000, Instant::now());
        assert!(c.next_chunk().is_none());
        assert!(c.level_ms().is_none(), "a queue nobody reads is not followed");
        c.set_active(true);
        c.push_input(&[0.1; 4800], 1, Instant::now());
        assert!(c.next_chunk().is_some());
    }
}
