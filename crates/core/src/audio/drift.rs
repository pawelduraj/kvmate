//! Keeps an audio buffer at its target by playing a tiny bit faster or slower. It settles in
//! about 30 s without overshooting; a 250 ppm clock difference moves the buffer by under 1 ms.
//! The level is averaged over 1 s first, so the small ups and downs from packets and device
//! callbacks change the speed by only a few ppm.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use rubato::{Async, FixedAsync, SincInterpolationParameters, WindowFunction};

use super::RATE;

const MAX_CORRECTION: f64 = 0.005;

const OMEGA_N: f64 = 0.15;
const ZETA: f64 = 1.0;
const KP: f64 = 2.0 * ZETA * OMEGA_N;
const KI: f64 = OMEGA_N * OMEGA_N;
const SMOOTHING_S: f64 = 1.0;

pub struct DriftController {
    target: f64,
    rate: f64,
    smoothed: Option<f64>,
    integral: f64,
}

impl DriftController {
    pub fn new(target: f64, rate: f64) -> Self {
        return DriftController { target, rate, smoothed: None, integral: 0.0 };
    }

    pub fn update(&mut self, level: f64, dt: f64) -> f64 {
        let alpha = (dt / SMOOTHING_S).clamp(0.0, 1.0);
        let smoothed = match self.smoothed {
            Some(s) => s + alpha * (level - s),
            None => level,
        };
        self.smoothed = Some(smoothed);
        let error = (smoothed - self.target) / self.rate;
        let integral = (self.integral + error * dt).clamp(-MAX_CORRECTION / KI, MAX_CORRECTION / KI);
        let wanted = KP * error + KI * integral;
        if wanted.abs() <= MAX_CORRECTION || wanted.signum() != error.signum() {
            self.integral = integral;
        }
        let correction = (KP * error + KI * self.integral).clamp(-MAX_CORRECTION, MAX_CORRECTION);
        return 1.0 - correction;
    }

    pub fn drift_ppm(&self) -> f64 {
        return KI * self.integral * 1e6;
    }

    pub fn level_ms(&self) -> Option<f64> {
        return self.smoothed.map(|s| return s * 1000.0 / self.rate);
    }

    pub fn reset_level(&mut self) {
        self.smoothed = None;
    }
}

pub(super) fn resampler(ratio: f64, chunk: usize, channels: usize, fixed: FixedAsync) -> Async<f32> {
    return Async::new_sinc(
        ratio,
        1.0 + 2.0 * MAX_CORRECTION,
        &SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2),
        chunk,
        channels,
        fixed,
    )
    .expect("valid resampler parameters");
}

pub(super) fn target_frames(ms: u32, chunk: usize) -> usize {
    return (RATE as usize * ms as usize / 1000).max(chunk * 2);
}

pub(super) fn seconds_since(last: Option<Instant>, now: Instant, cap: Duration) -> f64 {
    return last.map_or(0.0, |t| return now.saturating_duration_since(t).min(cap).as_secs_f64());
}

pub(super) fn take_into(dst: &mut [f32], queue: &mut VecDeque<f32>) {
    let n = dst.len();
    for (d, s) in dst.iter_mut().zip(queue.drain(..n)) {
        *d = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulate(producer: f64, consumer: f64, seconds: f64) -> (DriftController, f64, f64, f64) {
        let target = 1440.0;
        let mut c = DriftController::new(target, 48000.0);
        let mut level = target;
        let dt = 0.01;
        let mut ratio = 1.0;
        let (mut min, mut max) = (f64::MAX, f64::MIN);
        let steps = (seconds / dt) as usize;
        for i in 0..steps {
            level += producer * dt - consumer * dt / ratio;
            ratio = c.update(level, dt);
            if i > steps / 2 {
                min = min.min(level);
                max = max.max(level);
            }
        }
        return (c, min, max, (1.0 - ratio) * 1e6);
    }

    #[test]
    fn converges_on_fast_producer() {
        let (c, min, max, correction) = simulate(48000.0 * (1.0 + 300e-6), 48000.0, 120.0);
        assert!((c.drift_ppm() - 300.0).abs() < 5.0, "drift {}", c.drift_ppm());
        assert!((correction - 300.0).abs() < 5.0, "correction {correction}");
        assert!(min > 1400.0 && max < 1480.0, "level {min}..{max}");
    }

    #[test]
    fn converges_on_slow_producer() {
        let (c, min, max, _) = simulate(48000.0 * (1.0 - 200e-6), 48000.0, 120.0);
        assert!((c.drift_ppm() + 200.0).abs() < 5.0, "drift {}", c.drift_ppm());
        assert!(min > 1400.0 && max < 1480.0, "level {min}..{max}");
    }

    #[test]
    fn correction_is_bounded() {
        let mut c = DriftController::new(1000.0, 48000.0);
        let mut ratio = 1.0;
        for _ in 0..10_000 {
            ratio = c.update(100_000.0, 0.01);
        }
        assert!((1.0 - ratio) * 1e6 <= MAX_CORRECTION * 1e6 + 1e-6);
        assert!(c.drift_ppm() <= MAX_CORRECTION * 1e6 + 1e-6);
    }

    #[test]
    fn a_burst_does_not_wind_up_the_learned_drift() {
        let mut c = DriftController::new(1440.0, 48000.0);
        for _ in 0..1000 {
            c.update(1440.0 + 3600.0, 0.01);
        }
        assert!(c.drift_ppm().abs() < 100.0, "drift {}", c.drift_ppm());
        c.reset_level();
        assert!(c.level_ms().is_none(), "the level is measured again, the drift is kept");
    }
}
