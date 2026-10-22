//! Controlled computer -> hub headphones.
//!
//! The level kept at the target also counts the audio already resampled for the device and the
//! audio the board has produced since its last packet. Otherwise 4 ms packets and 10 ms device
//! callbacks would make the level seem to wander.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use kvmate_protocol::AudioHeader;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Adjustable, Async, FixedAsync, Resampler};

use super::RATE;
use super::drift::{self, DriftController};

const CHANNELS: usize = 2;
const CHUNK: usize = 96;
const MAX_GAP_FRAMES: u32 = RATE / 10;
const MAX_EXTRAPOLATION: Duration = Duration::from_millis(8);

pub struct PlaybackPath {
    out_rate: u32,
    target_frames: usize,
    jitter: VecDeque<f32>,
    expected_frame: Option<u32>,
    last_packet: Option<Instant>,
    resampler: Async<f32>,
    in_buf: Vec<f32>,
    out_buf: Vec<f32>,
    out_fifo: VecDeque<f32>,
    controller: DriftController,
    playing: bool,
    gain: f32,
    gain_target: f32,
    underruns: u32,
    lost_frames: u64,
}

impl PlaybackPath {
    pub fn new(out_rate: u32, buffer_ms: u32) -> PlaybackPath {
        let resampler = drift::resampler(out_rate as f64 / RATE as f64, CHUNK, CHANNELS, FixedAsync::Input);
        let out_max = resampler.output_frames_max();
        let target_frames = drift::target_frames(buffer_ms, CHUNK);
        return PlaybackPath {
            out_rate,
            target_frames,
            jitter: VecDeque::with_capacity(target_frames * CHANNELS * 4),
            expected_frame: None,
            last_packet: None,
            resampler,
            in_buf: vec![0.0; CHUNK * CHANNELS],
            out_buf: vec![0.0; out_max * CHANNELS],
            out_fifo: VecDeque::with_capacity(out_max * CHANNELS * 4),
            controller: DriftController::new(target_frames as f64, RATE as f64),
            playing: false,
            gain: 1.0,
            gain_target: 1.0,
            underruns: 0,
            lost_frames: 0,
        };
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain_target = gain;
        if !self.playing {
            self.gain = gain;
        }
    }

    fn buffered_frames(&self) -> f64 {
        return (self.jitter.len() / CHANNELS) as f64
            + (self.out_fifo.len() / CHANNELS) as f64 * RATE as f64 / self.out_rate as f64;
    }

    #[cfg(test)]
    fn buffered_ms(&self) -> f32 {
        return (self.buffered_frames() * 1000.0 / RATE as f64) as f32;
    }

    pub fn level_ms(&self) -> Option<f32> {
        return self.controller.level_ms().map(|ms| return ms as f32);
    }

    pub fn drift_ppm(&self) -> f32 {
        return self.controller.drift_ppm() as f32;
    }

    pub fn push(&mut self, head: &AudioHeader, pcm: &[u8], now: Instant) {
        let first = head.first_frame_number;
        if let Some(expected) = self.expected_frame {
            let gap = first.wrapping_sub(expected) as i32;
            if gap < 0 && gap > -(MAX_GAP_FRAMES as i32) {
                return;
            }
            if gap > 0 && gap < MAX_GAP_FRAMES as i32 {
                self.lost_frames += gap as u64;
                if self.playing {
                    self.jitter.extend(std::iter::repeat_n(0.0, gap as usize * CHANNELS));
                }
            } else if gap != 0 {
                self.restart();
            }
        }
        let frames = pcm.len() / (2 * CHANNELS);
        self.expected_frame = Some(first.wrapping_add(frames as u32));
        self.last_packet = Some(now);
        self.jitter.extend(
            pcm.chunks_exact(2)
                .take(frames * CHANNELS)
                .map(|b| return i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
        );
        if self.jitter.len() / CHANNELS > self.target_frames * 4 {
            let drop = self.jitter.len() / CHANNELS - self.target_frames;
            self.jitter.drain(..drop * CHANNELS);
        }
    }

    fn restart(&mut self) {
        self.jitter.clear();
        self.out_fifo.clear();
        self.resampler.reset();
        self.controller.reset_level();
        self.playing = false;
    }

    pub fn render(&mut self, out: &mut [f32], channels: usize, now: Instant) {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        while self.out_fifo.len() / CHANNELS < frames {
            if !self.playing {
                let queued = self.jitter.len() / CHANNELS;
                if queued < self.target_frames {
                    break;
                }
                self.jitter.drain(..(queued - self.target_frames) * CHANNELS);
                self.playing = true;
            }
            if self.jitter.len() / CHANNELS < CHUNK {
                self.underruns += 1;
                self.playing = false;
                self.controller.reset_level();
                break;
            }
            drift::take_into(&mut self.in_buf, &mut self.jitter);
            let input = InterleavedSlice::new(&self.in_buf[..], CHANNELS, CHUNK).expect("input size");
            let out_frames = self.resampler.output_frames_next();
            let mut output =
                InterleavedSlice::new_mut(&mut self.out_buf[..out_frames * CHANNELS], CHANNELS, out_frames)
                    .expect("output size");
            if let Ok((_, written)) = self.resampler.process_into_buffer(&input, &mut output, None) {
                self.out_fifo.extend(&self.out_buf[..written * CHANNELS]);
            }
        }

        let available = (self.out_fifo.len() / CHANNELS).min(frames);
        let gain_step = (self.gain_target - self.gain) / available.max(1) as f32;
        for frame in out.chunks_mut(channels).take(available) {
            self.gain += gain_step;
            let l = self.out_fifo.pop_front().unwrap_or(0.0) * self.gain;
            let r = self.out_fifo.pop_front().unwrap_or(0.0) * self.gain;
            match frame.len() {
                1 => frame[0] = 0.5 * (l + r),
                _ => {
                    frame[0] = l;
                    frame[1] = r;
                    frame[2..].fill(0.0);
                }
            }
        }
        out[available * channels..].fill(0.0);
        self.gain = self.gain_target;

        if self.playing {
            let dt = frames as f64 / self.out_rate as f64;
            let arriving = drift::seconds_since(self.last_packet, now, MAX_EXTRAPOLATION) * RATE as f64;
            let ratio = self.controller.update(self.buffered_frames() + arriving, dt);
            let _ = self.resampler.set_resample_ratio_relative(ratio, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(first_frame: u32, frames: usize, value: i16) -> (AudioHeader, Vec<u8>) {
        let pcm = (0..frames * CHANNELS).flat_map(|_| return value.to_le_bytes()).collect();
        return (AudioHeader { first_frame_number: first_frame }, pcm);
    }

    fn run(out_rate: u32, ppm: f64, seconds: f64) -> (PlaybackPath, f32, f32) {
        let t0 = Instant::now();
        let at = |t: f64| return t0 + Duration::from_secs_f64(t);
        let mut p = PlaybackPath::new(out_rate, 30);
        let packet_period = 192.0 / (RATE as f64 * (1.0 + ppm * 1e-6));
        let callback_period = 0.01;
        let (mut next_packet, mut next_callback) = (packet_period, callback_period);
        let mut frame_no: u32 = 0;
        let mut phase: f32 = 0.0;
        let mut out = vec![0.0f32; out_rate as usize / 100 * 2];
        let (mut min_ms, mut max_ms) = (f32::MAX, f32::MIN);
        loop {
            let t = next_packet.min(next_callback);
            if t > seconds {
                break;
            }
            if t == next_packet {
                let mut pcm = Vec::with_capacity(192 * 4);
                for _ in 0..192 {
                    let s = (phase.sin() * 8000.0) as i16;
                    phase += 2.0 * std::f32::consts::PI * 440.0 / RATE as f32;
                    pcm.extend_from_slice(&s.to_le_bytes());
                    pcm.extend_from_slice(&s.to_le_bytes());
                }
                p.push(&AudioHeader { first_frame_number: frame_no }, &pcm, at(t));
                frame_no = frame_no.wrapping_add(192);
                next_packet += packet_period;
            } else {
                p.render(&mut out, 2, at(t));
                if t > seconds / 2.0 {
                    min_ms = min_ms.min(p.buffered_ms());
                    max_ms = max_ms.max(p.buffered_ms());
                }
                next_callback += callback_period;
            }
        }
        return (p, min_ms, max_ms);
    }

    #[test]
    fn tracks_fast_board_clock_without_dropouts() {
        let (p, min_ms, max_ms) = run(48000, 250.0, 120.0);
        assert_eq!(p.underruns, 0);
        assert!(min_ms > 15.0 && max_ms < 45.0, "buffer {min_ms}..{max_ms} ms");
        assert!((p.drift_ppm() - 250.0).abs() < 20.0, "drift {}", p.drift_ppm());
    }

    #[test]
    fn tracks_slow_board_clock_at_44k1_output() {
        let (p, min_ms, max_ms) = run(44100, -180.0, 120.0);
        assert_eq!(p.underruns, 0);
        assert!(min_ms > 15.0 && max_ms < 45.0, "buffer {min_ms}..{max_ms} ms");
        assert!((p.drift_ppm() + 180.0).abs() < 20.0, "drift {}", p.drift_ppm());
    }

    #[test]
    fn lost_packet_is_filled_with_silence() {
        let mut p = PlaybackPath::new(48000, 30);
        p.playing = true;
        let now = Instant::now();
        let (h1, pcm) = packet(0, 192, 0x1010);
        let (h2, _) = packet(384, 192, 0x1010);
        p.push(&h1, &pcm, now);
        p.push(&h2, &pcm, now);
        assert_eq!(p.lost_frames, 192);
        assert_eq!(p.jitter.len() / CHANNELS, 192 * 3);
        assert!(p.jitter.range(192 * CHANNELS..384 * CHANNELS).all(|&s| return s == 0.0));
    }

    #[test]
    fn late_packet_is_dropped_and_a_gap_while_buffering_adds_no_delay() {
        let mut p = PlaybackPath::new(48000, 30);
        let now = Instant::now();
        let (h0, pcm) = packet(0, 192, 0x1010);
        let (h2, _) = packet(384, 192, 0x1010);
        let (h1, _) = packet(192, 192, 0x1010);
        p.push(&h0, &pcm, now);
        p.push(&h2, &pcm, now);
        p.push(&h1, &pcm, now);
        assert_eq!(p.jitter.len() / CHANNELS, 192 * 2);
        assert_eq!(p.expected_frame, Some(576), "the late packet didn't restart the stream");
    }

    #[test]
    fn playback_starts_at_the_target_after_a_burst() {
        let mut p = PlaybackPath::new(48000, 30);
        let now = Instant::now();
        for i in 0..50 {
            let (h, pcm) = packet(i * 192, 192, 0x1010);
            p.push(&h, &pcm, now);
        }
        let mut out = vec![0.0f32; 960];
        p.render(&mut out, 2, now);
        assert!(p.buffered_ms() < 31.0, "buffer {} ms", p.buffered_ms());
    }

    #[test]
    fn frame_numbers_wrap() {
        let mut p = PlaybackPath::new(48000, 30);
        let now = Instant::now();
        let (h1, pcm) = packet(u32::MAX - 95, 192, 0x1010);
        let (h2, _) = packet(96, 192, 0x1010);
        p.push(&h1, &pcm, now);
        p.push(&h2, &pcm, now);
        assert_eq!((p.jitter.len() / CHANNELS, p.lost_frames), (384, 0));
    }

    #[test]
    fn volume_changes_are_ramped() {
        let mut p = PlaybackPath::new(48000, 10);
        let now = Instant::now();
        let mut out = vec![0.0f32; 960];
        let mut render = |p: &mut PlaybackPath, n: u32| {
            let (h, pcm) = packet(n * 480, 480, 16384);
            p.push(&h, &pcm, now);
            p.render(&mut out, 2, now);
            return out.clone();
        };
        render(&mut p, 0);
        render(&mut p, 1);
        let full = render(&mut p, 2);
        assert!(full.iter().all(|&s| return (s - 0.5).abs() < 0.01), "full volume passes the signal");
        p.set_gain(0.0);
        let ramp = render(&mut p, 3);
        assert!(ramp[0] > 0.45 && ramp[958].abs() < 0.01, "ramped down, not cut");
        let muted = render(&mut p, 4);
        assert!(muted.iter().all(|&s| return s == 0.0), "muted");
    }
}
