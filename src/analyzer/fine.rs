//! The fine spectrum, for the faces that ask for it: 8192 frames under a
//! Blackman-Harris window (5.9 Hz a bin at 48 kHz, side lobes below
//! -92 dB), in dBFS, with a held peak curve; the strongest peak with its
//! note; the 88 keys of a piano and the twelve pitch classes (chroma); 64
//! mel bands; and their histories (a step each 30th of a second) for the
//! mel spectrogram, the chromagram and the piano roll.

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub const SIZE: usize = 8192;
pub const BINS: usize = SIZE / 2 + 1;
pub const KEYS: usize = 88;
pub const MELS: usize = 64;
/// Steps of history kept (mel, chroma, keys).
pub const STEPS: usize = 256;
/// dBFS shown at the bottom of the scales.
pub const FLOOR_DB: f32 = -96.0;
const STEP_HZ: f32 = 30.0;

pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The note nearest `hz`: (MIDI number, cents off it).
pub fn note(hz: f32) -> (i32, f32) {
    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
    let nearest = midi.round();
    (nearest as i32, (midi - nearest) * 100.0)
}

/// "A4" for MIDI 69.
pub fn note_name(midi: i32) -> String {
    format!(
        "{}{}",
        NOTE_NAMES[midi.rem_euclid(12) as usize],
        midi.div_euclid(12) - 1
    )
}

pub fn midi_hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// A mel band: its bins and their weights (a triangle).
#[derive(Clone, Debug, Default)]
struct MelBand {
    first: usize,
    weights: Vec<f32>,
}

pub struct Fine {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    power: Vec<f32>,
    /// dBFS a bin (a full-scale sine reads 0), falling 40 dB a second.
    pub db: Vec<f32>,
    /// The highest of `db` lately, falling 12 dB a second after a second held.
    pub held: Vec<f32>,
    held_for: Vec<f32>,
    /// Hz a bin.
    pub bin_hz: f32,
    /// The strongest peak above 30 Hz: Hz and dBFS (between the bins).
    pub peak_hz: f32,
    pub peak_db: f32,
    /// The 88 keys (A0..C8): their loudest bin, 0..1 over -80..-10 dBFS.
    pub keys: [f32; KEYS],
    /// The pitch classes (C..B), the loudest 1, smoothed.
    pub chroma: [f32; 12],
    /// The mel bands (30 Hz..16 kHz), 0..1 over -90..-10 dBFS.
    pub mel: [f32; MELS],
    mel_bands: Vec<MelBand>,
    built_for: u32,
    key_bins: Vec<(usize, usize)>,
    /// Histories, a row a step, a ring: STEPS rows.
    mel_history: Vec<[f32; MELS]>,
    chroma_history: Vec<[f32; 12]>,
    key_history: Vec<[f32; KEYS]>,
    head: usize,
    clock: f32,
    /// Steps of history this frame made.
    pub steps: u32,
    /// The keys and chroma from the meters' thread, when it runs.
    pub external: Option<([f32; KEYS], [f32; 12])>,
}

impl Default for Fine {
    fn default() -> Self {
        Self::new()
    }
}

impl Fine {
    pub fn new() -> Fine {
        let fft = FftPlanner::new().plan_fft_forward(SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window: Vec<f32> = (0..SIZE)
            .map(|i| {
                let x = std::f64::consts::TAU * i as f64 / SIZE as f64;
                (0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos()
                    - 0.01168 * (3.0 * x).cos()) as f32
            })
            .collect();
        Fine {
            fft,
            window,
            buffer: vec![Complex::default(); SIZE],
            scratch,
            power: vec![0.0; BINS],
            db: vec![FLOOR_DB; BINS],
            held: vec![FLOOR_DB; BINS],
            held_for: vec![0.0; BINS],
            bin_hz: 48_000.0 / SIZE as f32,
            peak_hz: 0.0,
            peak_db: FLOOR_DB,
            keys: [0.0; KEYS],
            chroma: [0.0; 12],
            mel: [0.0; MELS],
            mel_bands: Vec::new(),
            built_for: 0,
            key_bins: Vec::with_capacity(KEYS),
            mel_history: vec![[0.0; MELS]; STEPS],
            chroma_history: vec![[0.0; 12]; STEPS],
            key_history: vec![[0.0; KEYS]; STEPS],
            head: 0,
            clock: 0.0,
            steps: 0,
            external: None,
        }
    }

    fn build(&mut self, rate: u32) {
        if self.built_for == rate {
            return;
        }
        self.built_for = rate;
        self.bin_hz = rate as f32 / SIZE as f32;
        let bin = |hz: f32| ((hz / self.bin_hz).round() as usize).clamp(1, BINS - 1);
        // a key: the bins within half a semitone of it
        self.key_bins.clear();
        for k in 0..KEYS {
            let midi = 21.0 + k as f32;
            let lo = bin(midi_hz(midi - 0.5));
            let hi = bin(midi_hz(midi + 0.5)).max(lo);
            self.key_bins.push((lo, hi));
        }
        // the mel bands: triangles, each from the last one's centre to the next
        let mel = |hz: f32| 2595.0 * (1.0 + hz / 700.0).log10();
        let hz = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);
        let top = 16_000f32.min(rate as f32 * 0.48);
        let (low, high) = (mel(30.0), mel(top));
        let centres: Vec<f32> = (0..MELS + 2)
            .map(|k| hz(low + (high - low) * k as f32 / (MELS + 1) as f32) / self.bin_hz)
            .collect();
        self.mel_bands.clear();
        for m in 0..MELS {
            let (a, b, c) = (centres[m], centres[m + 1], centres[m + 2]);
            let first = (a.floor() as usize).max(1);
            let last = (c.ceil() as usize).min(BINS - 1).max(first);
            let mut weights: Vec<f32> = (first..=last)
                .map(|k| {
                    let k = k as f32;
                    if k <= b {
                        ((k - a) / (b - a).max(1e-6)).max(0.0)
                    } else {
                        ((c - k) / (c - b).max(1e-6)).max(0.0)
                    }
                })
                .collect();
            if weights.iter().all(|w| *w <= 0.0) {
                // narrower than a bin: the nearest one
                let near = (b.round() as usize).clamp(first, last);
                weights[near - first] = 1.0;
            }
            self.mel_bands.push(MelBand { first, weights });
        }
    }

    pub fn update(&mut self, tap: &[f32], rate: u32, dt: f32) {
        self.build(rate);
        let frames = tap.len() / 2;
        let take = frames.min(SIZE);
        let start = (frames - take) * 2;
        let pad = SIZE - take;
        for i in 0..SIZE {
            let s = if i >= pad {
                let j = start + (i - pad) * 2;
                (tap[j] + tap[j + 1]) * 0.5
            } else {
                0.0
            };
            self.buffer[i] = Complex::new(s * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        // a full-scale sine: |X| = SIZE/2 times the window's mean (0.35875)
        let reference = (SIZE as f32 * 0.5 * 0.35875).powi(2);
        let fall = 40.0 * dt;
        let held_fall = 12.0 * dt;
        let (mut best, mut best_bin) = (0.0f32, 0usize);
        let lowest = (30.0 / self.bin_hz).ceil() as usize;
        for k in 0..BINS {
            let p = self.buffer[k].norm_sqr() / reference;
            self.power[k] = p;
            let db = if p > 1e-12 {
                (10.0 * p.log10()).max(FLOOR_DB)
            } else {
                FLOOR_DB
            };
            self.db[k] = db.max(self.db[k] - fall);
            if self.db[k] >= self.held[k] {
                self.held[k] = self.db[k];
                self.held_for[k] = 0.0;
            } else {
                self.held_for[k] += dt;
                if self.held_for[k] > 1.0 {
                    self.held[k] = (self.held[k] - held_fall).max(self.db[k]);
                }
            }
            if k >= lowest && p > best {
                best = p;
                best_bin = k;
            }
        }
        if best_bin > 0 && best_bin + 1 < BINS {
            // between the bins: the top of a parabola through the three (in dB)
            let db = |k: usize| 10.0 * self.power[k].max(1e-12).log10();
            let (a, b, c) = (db(best_bin - 1), db(best_bin), db(best_bin + 1));
            let d = a - 2.0 * b + c;
            let off = if d.abs() > 1e-9 {
                (0.5 * (a - c) / d).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            self.peak_hz = (best_bin as f32 + off) * self.bin_hz;
            self.peak_db = b - 0.25 * (a - c) * off;
        } else {
            self.peak_hz = 0.0;
            self.peak_db = FLOOR_DB;
        }
        // the keys and the chroma: the meters' (every sample heard, two
        // resolutions, harmonics told apart) where they run, else this
        // spectrum's own peaks
        let (now, classes) = self.external.unwrap_or_else(|| self.own_notes());
        let fade = (-dt * 8.0).exp();
        for (key, v) in self.keys.iter_mut().zip(now) {
            *key = v.max(*key * fade);
        }
        let follow = 1.0 - (-dt * 10.0).exp();
        for (c, target) in self.chroma.iter_mut().zip(classes) {
            *c += (target - *c) * follow;
        }
        for (m, band) in self.mel_bands.iter().enumerate() {
            let mut p = 0.0;
            for (w, power) in band.weights.iter().zip(&self.power[band.first..]) {
                p += w * power;
            }
            let db = 10.0 * p.max(1e-12).log10();
            self.mel[m] = ((db + 90.0) / 80.0).clamp(0.0, 1.0);
        }
        self.step(dt);
    }

    /// The keys and the pitch classes (the loudest 1) of this spectrum's
    /// peaks (a bin above both neighbours), each at its note - not the bins
    /// near a note, which the window's main lobe lights for its neighbours.
    fn own_notes(&self) -> ([f32; KEYS], [f32; 12]) {
        let mut classes = [0.0f32; 12];
        let mut now = [0.0f32; KEYS];
        let (first, last) = (
            self.key_bins[0].0.max(1),
            self.key_bins[KEYS - 1].1.min(BINS - 2),
        );
        for k in first..=last {
            let p = self.power[k];
            if p <= 1e-8 || p <= self.power[k - 1] || p < self.power[k + 1] {
                continue;
            }
            let db = |i: usize| 10.0 * self.power[i].max(1e-12).log10();
            let (a, b, c) = (db(k - 1), db(k), db(k + 1));
            let d = a - 2.0 * b + c;
            let off = if d.abs() > 1e-9 {
                (0.5 * (a - c) / d).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let (midi, _) = note((k as f32 + off) * self.bin_hz);
            if !(21..=108).contains(&midi) {
                continue;
            }
            let level = ((b + 80.0) / 70.0).clamp(0.0, 1.0);
            let key = (midi - 21) as usize;
            now[key] = now[key].max(level);
            // (chroma from C2 up: below, mostly the roots of harmonics)
            if midi >= 36 {
                classes[midi.rem_euclid(12) as usize] += p.sqrt();
            }
        }
        let loudest = classes.iter().cloned().fold(0.0, f32::max);
        for c in classes.iter_mut() {
            *c = if loudest > 1e-6 { *c / loudest } else { 0.0 };
        }
        (now, classes)
    }

    /// Nothing plays: everything falls.
    pub fn rest(&mut self, dt: f32) {
        let fall = 40.0 * dt;
        for v in self.db.iter_mut().chain(self.held.iter_mut()) {
            *v = (*v - fall).max(FLOOR_DB);
        }
        let keep = (-dt * 8.0).exp();
        for v in self
            .keys
            .iter_mut()
            .chain(self.chroma.iter_mut())
            .chain(self.mel.iter_mut())
        {
            *v *= keep;
        }
        self.peak_db = FLOOR_DB;
        self.step(dt);
    }

    fn step(&mut self, dt: f32) {
        self.steps = 0;
        self.clock = (self.clock + dt).min(4.0 / STEP_HZ);
        while self.clock >= 1.0 / STEP_HZ {
            self.clock -= 1.0 / STEP_HZ;
            self.steps += 1;
            self.head = (self.head + 1) % STEPS;
            self.mel_history[self.head] = self.mel;
            self.chroma_history[self.head] = self.chroma;
            self.key_history[self.head] = self.keys;
        }
    }

    /// How far (0..1) the next step is.
    pub fn step_fraction(&self) -> f32 {
        (self.clock * STEP_HZ).clamp(0.0, 1.0)
    }

    pub fn mel_row(&self, age: usize) -> &[f32; MELS] {
        &self.mel_history[(self.head + STEPS - age % STEPS) % STEPS]
    }

    pub fn chroma_row(&self, age: usize) -> &[f32; 12] {
        &self.chroma_history[(self.head + STEPS - age % STEPS) % STEPS]
    }

    pub fn key_row(&self, age: usize) -> &[f32; KEYS] {
        &self.key_history[(self.head + STEPS - age % STEPS) % STEPS]
    }

    /// dBFS at `hz` (between bins: the louder neighbour).
    pub fn db_at(&self, hz: f32, held: bool) -> f32 {
        let of = if held { &self.held } else { &self.db };
        let k = hz / self.bin_hz;
        let i = (k.floor() as usize).min(BINS - 1);
        let j = (i + 1).min(BINS - 1);
        let t = k - i as f32;
        of[i] * (1.0 - t) + of[j] * t
    }

    /// The loudest dBFS of the bins from `lo` to `hi` Hz (a pixel's worth of
    /// the analyzer's curve: no peak falls between two pixels).
    pub fn db_span(&self, lo: f32, hi: f32, held: bool) -> f32 {
        let of = if held { &self.held } else { &self.db };
        let a = (lo / self.bin_hz).floor() as usize;
        let b = (hi / self.bin_hz).ceil() as usize;
        if b <= a + 1 {
            return self.db_at((lo + hi) * 0.5, held);
        }
        of[a.min(BINS - 1)..b.min(BINS)]
            .iter()
            .cloned()
            .fold(FLOOR_DB, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(rate: u32, notes: &[f32], frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f32 / rate as f32;
                let v: f32 = notes
                    .iter()
                    .map(|m| (std::f32::consts::TAU * midi_hz(*m) * t).sin() * 0.2)
                    .sum();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_c_major_chord_lights_c_e_and_g() {
        let mut fine = Fine::new();
        // C4 E4 G4
        let tap = chord(48_000, &[60.0, 64.0, 67.0], 8192);
        for _ in 0..30 {
            fine.update(&tap, 48_000, 1.0 / 30.0);
        }
        let mut order: Vec<usize> = (0..12).collect();
        order.sort_by(|a, b| fine.chroma[*b].total_cmp(&fine.chroma[*a]));
        let mut top: Vec<&str> = order[..3].iter().map(|c| NOTE_NAMES[*c]).collect();
        top.sort();
        assert_eq!(top, ["C", "E", "G"], "{:?}", fine.chroma);
        assert!(fine.chroma[order[3]] < 0.5, "{:?}", fine.chroma);
        // the keys: C4 is key 39 (A0 is 0)
        assert!(fine.keys[39] > 0.8 && fine.keys[43] > 0.8 && fine.keys[46] > 0.8);
        assert!(fine.keys[41] < 0.6, "{}", fine.keys[41]);
    }

    #[test]
    fn a_tone_reads_its_level_note_and_frequency() {
        let mut fine = Fine::new();
        let tap: Vec<f32> = (0..8192)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.5;
                [v, v]
            })
            .collect();
        fine.update(&tap, 48_000, 1.0 / 30.0);
        assert!((fine.peak_hz - 440.0).abs() < 1.0, "{}", fine.peak_hz);
        assert!((fine.peak_db + 6.02).abs() < 0.3, "{}", fine.peak_db);
        assert_eq!(note_name(note(fine.peak_hz).0), "A4");
        assert!((fine.db_at(440.0, false) + 6.0).abs() < 1.0);
        // far from it, the window keeps the floor
        assert!(
            fine.db_at(3000.0, false) < -90.0,
            "{}",
            fine.db_at(3000.0, false)
        );
        assert_eq!(note(261.63).0, 60);
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(21), "A0");
    }

    #[test]
    fn the_mel_bands_follow_the_frequency() {
        let mut fine = Fine::new();
        let mut last = 0;
        for hz in [100.0f32, 1000.0, 8000.0] {
            let tap: Vec<f32> = (0..8192)
                .flat_map(|i| {
                    let v = (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin() * 0.5;
                    [v, v]
                })
                .collect();
            fine.update(&tap, 48_000, 1.0);
            let loudest = (0..MELS)
                .max_by(|a, b| fine.mel[*a].total_cmp(&fine.mel[*b]))
                .unwrap();
            assert!(loudest > last, "{hz} Hz: band {loudest}");
            last = loudest;
        }
    }
}
