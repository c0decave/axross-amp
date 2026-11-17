//! The beat: an onset strength (the spectral flux of log-compressed bands,
//! every 512 frames), a beat where it stands out from the last second, and
//! the tempo from the onsets' autocorrelation over the last six seconds.

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

const SIZE: usize = 1024;
const HOP: usize = 512;
const BANDS: usize = 24;
/// Onset strengths kept (a hop each): about 6 s at 48 kHz.
const KEEP: usize = 576;

pub struct Beat {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    edges: Vec<usize>,
    edges_for: u32,
    previous: [f32; BANDS],
    /// Frames since the last hop.
    since_hop: usize,
    /// Onset strengths, a ring.
    pub strengths: Vec<f32>,
    at: usize,
    hops: u64,
    since_beat_hops: u64,
    tempo_in: u32,
    acf: Vec<f32>,
    hop_secs: f32,
    /// 1 at a beat, falling to 0 in about a quarter of a second.
    pub pulse: f32,
    /// Beats so far, and the seconds since the last.
    pub count: u64,
    pub since: f32,
    /// Beats a minute (0 until there is one), and how sure (0..1).
    pub bpm: f32,
    pub confidence: f32,
    /// The latest onset strength, 0..1 against the recent loudest.
    pub strength: f32,
    /// The bands' levels (0..1, log-compressed) at the last hop: bass to treble.
    pub bands: [f32; BANDS],
}

impl Default for Beat {
    fn default() -> Self {
        Self::new()
    }
}

impl Beat {
    pub fn new() -> Beat {
        let fft = FftPlanner::new().plan_fft_forward(SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Beat {
            fft,
            window: (0..SIZE)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
                .collect(),
            buffer: vec![Complex::default(); SIZE],
            scratch,
            edges: Vec::with_capacity(BANDS + 1),
            edges_for: 0,
            previous: [0.0; BANDS],
            since_hop: 0,
            strengths: vec![0.0; KEEP],
            at: 0,
            hops: 0,
            since_beat_hops: 1000,
            tempo_in: 0,
            acf: Vec::with_capacity(KEEP),
            hop_secs: HOP as f32 / 48_000.0,
            pulse: 0.0,
            count: 0,
            since: 0.0,
            bpm: 0.0,
            confidence: 0.0,
            strength: 0.0,
            bands: [0.0; BANDS],
        }
    }

    /// `new` frames came at the end of `tap` (interleaved stereo).
    pub fn push(&mut self, tap: &[f32], new: usize, rate: u32) {
        if self.edges_for != rate {
            self.edges_for = rate;
            self.hop_secs = HOP as f32 / rate as f32;
            self.edges.clear();
            let bin = rate as f32 / SIZE as f32;
            let top = 16_000f32.min(rate as f32 * 0.45);
            for k in 0..=BANDS {
                let hz = 40.0 * (top / 40.0).powf(k as f32 / BANDS as f32);
                self.edges
                    .push(((hz / bin).round() as usize).clamp(1, SIZE / 2));
            }
            for k in 1..=BANDS {
                if self.edges[k] <= self.edges[k - 1] {
                    self.edges[k] = (self.edges[k - 1] + 1).min(SIZE / 2 + 1);
                }
            }
        }
        let frames = tap.len() / 2;
        let first = frames - new.min(frames);
        let mut end = first;
        // a hop each HOP frames: the window ending there
        loop {
            let wanted = HOP - self.since_hop;
            if end + wanted > frames {
                self.since_hop += frames - end;
                break;
            }
            end += wanted;
            self.since_hop = 0;
            if end >= SIZE {
                self.hop(&tap[(end - SIZE) * 2..end * 2]);
            }
        }
    }

    fn hop(&mut self, window: &[f32]) {
        for (i, f) in window.as_chunks::<2>().0.iter().enumerate() {
            self.buffer[i] = Complex::new((f[0] + f[1]) * 0.5 * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let mut flux = 0.0;
        for b in 0..BANDS {
            let (from, to) = (self.edges[b], self.edges[b + 1].max(self.edges[b] + 1));
            let mut power = 0.0;
            for c in &self.buffer[from..to.min(SIZE / 2 + 1)] {
                power += c.norm_sqr();
            }
            let level = (1.0 + 100.0 * (power / (to - from) as f32).sqrt() / SIZE as f32).ln();
            flux += (level - self.previous[b]).max(0.0);
            self.previous[b] = level;
            self.bands[b] = (level / 3.0).min(1.0);
        }
        self.strengths[self.at] = flux;
        self.at = (self.at + 1) % KEEP;
        self.hops += 1;
        self.since_beat_hops += 1;
        // a beat: well above the last second's onsets, not too soon after the last
        let second = ((1.0 / self.hop_secs) as usize).min(KEEP - 1);
        let (mut mean, mut square, mut loudest) = (0.0f32, 0.0f32, 0.0f32);
        for k in 1..=second {
            let v = self.strengths[(self.at + KEEP - 1 - k) % KEEP];
            mean += v;
            square += v * v;
        }
        for v in &self.strengths {
            loudest = loudest.max(*v);
        }
        mean /= second as f32;
        let deviation = (square / second as f32 - mean * mean).max(0.0).sqrt();
        self.strength = if loudest > 0.0 { flux / loudest } else { 0.0 };
        let gap = (0.28 / self.hop_secs) as u64;
        if flux > mean + 1.5 * deviation && flux > 0.05 && self.since_beat_hops >= gap {
            self.since_beat_hops = 0;
            self.count += 1;
            self.pulse = 1.0;
            self.since = 0.0;
        }
        self.tempo_in += 1;
        if self.tempo_in as f32 * self.hop_secs >= 0.5 && self.hops as usize >= KEEP / 2 {
            self.tempo_in = 0;
            self.tempo();
        }
    }

    /// The tempo: the lag (60..200 BPM) at which the onsets repeat best,
    /// leaning a little towards 120.
    fn tempo(&mut self) {
        let n = KEEP.min(self.hops as usize);
        let mean = self.strengths.iter().take(n).sum::<f32>() / n as f32;
        let at = |k: usize| self.strengths[(self.at + KEEP - n + k) % KEEP] - mean;
        let lag_for = |bpm: f32| 60.0 / bpm / self.hop_secs;
        let (from, to) = (lag_for(200.0) as usize, lag_for(60.0).ceil() as usize);
        self.acf.clear();
        let mut zero = 0.0;
        for k in 0..n {
            zero += at(k) * at(k);
        }
        if zero <= 1e-9 {
            return;
        }
        let (mut best, mut best_lag) = (f32::MIN, 0usize);
        for lag in from..=to.min(n / 2) {
            let mut sum = 0.0;
            for k in lag..n {
                sum += at(k) * at(k - lag);
            }
            let value = sum / zero;
            self.acf.push(value);
            let bpm = 60.0 / (lag as f32 * self.hop_secs);
            let lean = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp();
            if value * lean > best {
                best = value * lean;
                best_lag = lag;
            }
        }
        if best_lag == 0 || best <= 0.05 {
            self.confidence *= 0.8;
            return;
        }
        // between the lags: the top of a parabola through three
        let i = best_lag - from;
        let refine = if i > 0 && i + 1 < self.acf.len() {
            let (a, b, c) = (self.acf[i - 1], self.acf[i], self.acf[i + 1]);
            let d = a - 2.0 * b + c;
            if d.abs() > 1e-9 {
                (0.5 * (a - c) / d).clamp(-0.5, 0.5)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let bpm = 60.0 / ((best_lag as f32 + refine) * self.hop_secs);
        self.bpm = if self.bpm == 0.0 || (bpm - self.bpm).abs() > 8.0 {
            bpm
        } else {
            self.bpm * 0.7 + bpm * 0.3
        };
        self.confidence = best.clamp(0.0, 1.0);
    }

    /// The frame's time: the pulse falls.
    pub fn frame(&mut self, dt: f32, live: bool) {
        self.pulse *= (-dt * 7.0).exp();
        if live {
            self.since += dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicks (a short burst of noise) `bpm` times a minute.
    fn clicks(rate: u32, bpm: f32, seconds: f32) -> Vec<f32> {
        let every = (60.0 / bpm * rate as f32) as usize;
        let mut seed = 5u32;
        (0..(seconds * rate as f32) as usize)
            .flat_map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
                let v = if i % every < rate as usize / 100 {
                    noise * 0.8
                } else {
                    noise * 0.002
                };
                [v, v]
            })
            .collect()
    }

    #[test]
    fn clicks_at_128_bpm_are_beats_at_128_bpm() {
        let rate = 48_000;
        let sound = clicks(rate, 128.0, 12.0);
        let mut beat = Beat::new();
        // fed as a window would see it: 8192 frames, 800 new a frame
        let mut written = 0usize;
        let frames = sound.len() / 2;
        while written + 800 <= frames {
            written += 800;
            let from = written.saturating_sub(8192);
            beat.push(&sound[from * 2..written * 2], 800, rate);
            beat.frame(800.0 / rate as f32, true);
        }
        let expected = 12.0 * 128.0 / 60.0;
        assert!(
            (beat.count as f32 - expected).abs() <= 3.0,
            "{} beats, {expected} expected",
            beat.count
        );
        assert!((beat.bpm - 128.0).abs() < 3.0, "{} BPM", beat.bpm);
    }

    #[test]
    fn a_steady_tone_has_no_beat() {
        let rate = 48_000;
        let sound: Vec<f32> = (0..rate * 6)
            .flat_map(|i| {
                let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut beat = Beat::new();
        let mut written = 0usize;
        while written + 800 <= sound.len() / 2 {
            written += 800;
            let from = written.saturating_sub(8192);
            beat.push(&sound[from * 2..written * 2], 800, rate);
        }
        // (the tone's start may count as one)
        assert!(beat.count <= 1, "{}", beat.count);
    }
}
