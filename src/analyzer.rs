//! What the visualizations draw from: the spectrum of the sound heard as
//! bars (logarithmic frequency, smoothed, with falling peaks - axross's
//! options: bars, smoothing and peak fall per second), its recent history,
//! the levels of left and right, the raw samples and the loudness.

use crate::state::VisualOptions;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::collections::VecDeque;
use std::sync::Arc;

pub const FFT_SIZE: usize = 2048;
/// Rows of spectrum history (waterfall, ridges).
pub const HISTORY: usize = 96;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;
const FLOOR_DB: f32 = -72.0;

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    pub bars: Vec<f32>,
    pub peaks: Vec<f32>,
    pub history: VecDeque<Vec<f32>>,
    /// The most recent samples, mono (scope) and stereo pairs (stereo scope).
    pub mono: Vec<f32>,
    pub stereo: Vec<[f32; 2]>,
    /// Levels (0..1, smoothed) of left and right, and their peaks.
    pub level: [f32; 2],
    pub level_peak: [f32; 2],
    /// How loud and how much it changed (0..1): starfield speed, beats.
    pub energy: f32,
    pub onset: f32,
    previous_energy: f32,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()
            })
            .collect();
        Analyzer {
            fft,
            window,
            spectrum: vec![Complex::default(); FFT_SIZE],
            bars: vec![0.0; 32],
            peaks: vec![0.0; 32],
            history: VecDeque::with_capacity(HISTORY),
            mono: Vec::new(),
            stereo: Vec::new(),
            level: [0.0; 2],
            level_peak: [0.0; 2],
            energy: 0.0,
            onset: 0.0,
            previous_energy: 0.0,
        }
    }

    /// One frame: `tap` holds the latest stereo samples (interleaved),
    /// `dt` the seconds since the last frame; `live` is false while
    /// paused (everything falls to rest).
    pub fn update(&mut self, tap: &[f32], rate: u32, options: &VisualOptions, dt: f32, live: bool) {
        let n = options.bars.clamp(8, 128) as usize;
        if self.bars.len() != n {
            self.bars = vec![0.0; n];
            self.peaks = vec![0.0; n];
            self.history.clear();
        }
        let frames = tap.len() / 2;
        let fresh = if live && frames >= 256 {
            self.spectrum_bars(tap, rate, n)
        } else {
            vec![0.0; n]
        };
        // smoothing: the share of a bar kept a frame at 30 fps, as a decay per second
        let keep = (options.smoothing as f32 / 100.0).powf(dt * 30.0);
        let fall = options.peak_fall as f32 / 100.0 * dt;
        for i in 0..n {
            let decayed = self.bars[i] * keep;
            self.bars[i] = fresh[i].max(decayed);
            self.peaks[i] = if self.bars[i] >= self.peaks[i] {
                self.bars[i]
            } else {
                (self.peaks[i] - fall).max(0.0)
            };
        }
        if self.history.len() >= HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.bars.clone());

        let take = frames.min(1024);
        self.mono.clear();
        self.stereo.clear();
        if live {
            let start = (frames - take) * 2;
            for f in tap[start..].as_chunks::<2>().0 {
                self.mono.push((f[0] + f[1]) * 0.5);
                self.stereo.push([f[0], f[1]]);
            }
        }
        for c in 0..2 {
            let rms = if take > 0 {
                (self.stereo.iter().map(|f| f[c] * f[c]).sum::<f32>() / take as f32).sqrt()
            } else {
                0.0
            };
            let db = 20.0 * rms.max(1e-6).log10();
            let value = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
            self.level[c] = value.max(self.level[c] * keep);
            self.level_peak[c] = if self.level[c] >= self.level_peak[c] {
                self.level[c]
            } else {
                (self.level_peak[c] - fall).max(0.0)
            };
        }
        let energy = self.bars.iter().take(n / 3).sum::<f32>() / (n / 3).max(1) as f32;
        self.onset = (energy - self.previous_energy).max(0.0) * 4.0 + self.onset * keep;
        self.onset = self.onset.min(1.0);
        self.previous_energy = energy;
        self.energy = energy;
    }

    /// The bars (0..1) of the latest FFT_SIZE samples: the loudest bin of
    /// each logarithmic band, in dB above the floor.
    fn spectrum_bars(&mut self, tap: &[f32], rate: u32, n: usize) -> Vec<f32> {
        let frames = tap.len() / 2;
        let take = frames.min(FFT_SIZE);
        let start = (frames - take) * 2;
        for i in 0..FFT_SIZE {
            let s = if i < take {
                let j = start + i * 2;
                (tap[j] + tap[j + 1]) * 0.5
            } else {
                0.0
            };
            self.spectrum[i] = Complex::new(s * self.window[i], 0.0);
        }
        self.fft.process(&mut self.spectrum);
        let rate = rate.max(8000) as f32;
        let bin_hz = rate / FFT_SIZE as f32;
        let high = HIGH_HZ.min(rate * 0.48);
        let scale = 2.0 / (FFT_SIZE as f32 * 0.5); // (a full-scale sine reads about 0 dB through the Hann window)
        (0..n)
            .map(|b| {
                let lo = LOW_HZ * (high / LOW_HZ).powf(b as f32 / n as f32);
                let hi = LOW_HZ * (high / LOW_HZ).powf((b + 1) as f32 / n as f32);
                let first = ((lo / bin_hz) as usize).max(1);
                let last = ((hi / bin_hz) as usize).max(first).min(FFT_SIZE / 2 - 1);
                let mag = (first..=last)
                    .map(|k| self.spectrum[k].norm())
                    .fold(0.0f32, f32::max)
                    * scale;
                let db = 20.0 * mag.max(1e-9).log10();
                ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_lights_its_band_and_silence_falls() {
        let rate = 48_000;
        let tap: Vec<f32> = (0..4096)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut a = Analyzer::new();
        let options = VisualOptions::default();
        a.update(&tap, rate, &options, 1.0 / 30.0, true);
        let loudest = a
            .bars
            .iter()
            .cloned()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        // 1 kHz on a log scale from 40 Hz to 16 kHz: a bit past the middle
        let expected = (32.0 * (1000.0f32 / 40.0).ln() / (16_000.0f32 / 40.0).ln()) as usize;
        assert!(
            (loudest.0 as i64 - expected as i64).abs() <= 1,
            "{} vs {expected}",
            loudest.0
        );
        assert!(loudest.1 > 0.8);
        assert!(a.level[0] > 0.7);
        for _ in 0..60 {
            a.update(&[], rate, &options, 1.0 / 30.0, false);
        }
        assert!(a.bars.iter().all(|b| *b < 0.01));
        assert!(a.peaks.iter().all(|p| *p < 0.2));
    }

    #[test]
    fn the_bar_count_follows_the_option() {
        let mut a = Analyzer::new();
        let options = VisualOptions {
            bars: 64,
            ..Default::default()
        };
        a.update(&[0.0; 8192], 44_100, &options, 0.03, true);
        assert_eq!(a.bars.len(), 64);
        assert_eq!(a.history.len(), 1);
    }
}
