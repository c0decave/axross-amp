//! The notes that sound: the spectrum's peaks at two resolutions - 32768
//! frames (1.5 Hz a bin at 48 kHz, a Hann window) below middle C, where
//! semitones lie a few hertz apart, and 8192 frames above it, where the
//! shorter window follows the melody - each peak at its note, and a peak
//! that is a harmonic (2nd to 8th) of a louder lower one counted as part of
//! that note, not as a note of its own.

use super::fine::{KEYS, note};
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub const LONG: usize = 32_768;
pub const SHORT: usize = 8_192;
/// Below this MIDI note the long window is read (C4, middle C).
const SPLIT: i32 = 60;
/// The most peaks taken from one spectrum.
const PEAKS: usize = 96;

struct Window {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    power: Vec<f32>,
}

impl Window {
    fn new(size: usize) -> Window {
        let fft = FftPlanner::new().plan_fft_forward(size);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        Window {
            fft,
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
                .collect(),
            buffer: vec![Complex::default(); size],
            scratch,
            power: vec![0.0; size / 2 + 1],
        }
    }

    /// The power spectrum of the last `size` frames of `mono` (dBFS: a
    /// full-scale sine reads 0).
    fn run(&mut self, mono: &[f32]) {
        let size = self.buffer.len();
        let start = mono.len().saturating_sub(size);
        let pad = size - (mono.len() - start);
        for i in 0..size {
            let s = if i >= pad { mono[start + i - pad] } else { 0.0 };
            self.buffer[i] = Complex::new(s * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let reference = (size as f32 * 0.25).powi(2);
        for (p, c) in self.power.iter_mut().zip(&self.buffer) {
            *p = c.norm_sqr() / reference;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Peak {
    hz: f32,
    db: f32,
    magnitude: f32,
    midi: i32,
    harmonic: bool,
}

pub struct Notes {
    long: Window,
    short: Window,
    peaks: Vec<Peak>,
    /// Each key's level now (0..1 over -80..-10 dBFS); a key that only
    /// carries another's harmonic stays dark.
    pub keys: [f32; KEYS],
    /// The pitch classes now (C..B), the loudest 1.
    pub chroma: [f32; 12],
    /// The pitch classes summed over the track (for its key).
    pub track_chroma: [f64; 12],
}

impl Default for Notes {
    fn default() -> Self {
        Self::new()
    }
}

impl Notes {
    pub fn new() -> Notes {
        Notes {
            long: Window::new(LONG),
            short: Window::new(SHORT),
            peaks: Vec::with_capacity(2 * PEAKS),
            keys: [0.0; KEYS],
            chroma: [0.0; 12],
            track_chroma: [0.0; 12],
        }
    }

    pub fn new_track(&mut self) {
        self.track_chroma = [0.0; 12];
    }

    /// The notes of the latest `mono` frames (at least LONG of them for the
    /// low ones) at `rate`.
    pub fn update(&mut self, mono: &[f32], rate: u32) {
        self.peaks.clear();
        self.long.run(mono);
        self.short.run(mono);
        let rate = rate as f32;
        collect(&self.long, rate, LONG, &mut self.peaks, |m| m < SPLIT);
        collect(&self.short, rate, SHORT, &mut self.peaks, |m| m >= SPLIT);
        // the harmonics of louder lower peaks are theirs, not notes; and a
        // peak close beside a much louder one is that one's window, not a note
        self.peaks.sort_unstable_by(|a, b| a.hz.total_cmp(&b.hz));
        for i in 0..self.peaks.len() {
            let p = self.peaks[i];
            let masked = self
                .peaks
                .iter()
                .any(|q| (q.hz / p.hz - 1.0).abs() < 0.06 && q.db > p.db + 15.0);
            self.peaks[i].harmonic = masked;
        }
        for i in 0..self.peaks.len() {
            let p = self.peaks[i];
            for j in 0..i {
                let q = self.peaks[j];
                // (a harmonic may read louder than its root while the long
                // window still fills with a new low note)
                if q.harmonic || q.db < p.db - 18.0 {
                    continue;
                }
                let ratio = p.hz / q.hz;
                let h = ratio.round();
                if (2.0..=8.0).contains(&h) && (ratio / h - 1.0).abs() < 0.025 {
                    self.peaks[i].harmonic = true;
                    break;
                }
            }
        }
        let mut keys = [0.0f32; KEYS];
        let mut classes = [0.0f32; 12];
        for p in &self.peaks {
            if p.harmonic {
                continue;
            }
            let k = (p.midi - 21) as usize;
            keys[k] = keys[k].max(((p.db + 80.0) / 70.0).clamp(0.0, 1.0));
            if p.midi >= 36 {
                classes[p.midi.rem_euclid(12) as usize] += p.magnitude;
            }
        }
        self.keys = keys;
        let loudest = classes.iter().cloned().fold(0.0, f32::max);
        for (c, v) in self.chroma.iter_mut().zip(classes) {
            *c = if loudest > 1e-6 { v / loudest } else { 0.0 };
        }
        for (t, v) in self.track_chroma.iter_mut().zip(classes) {
            *t += v as f64;
        }
    }
}

/// The peaks (above both neighbours, above -80 dBFS) of `w` whose notes
/// `wanted` takes, at most PEAKS of the loudest.
fn collect(w: &Window, rate: f32, size: usize, out: &mut Vec<Peak>, wanted: impl Fn(i32) -> bool) {
    let bin_hz = rate / size as f32;
    let first = ((super::fine::midi_hz(20.5) / bin_hz) as usize).max(2);
    let last = ((super::fine::midi_hz(108.5) / bin_hz) as usize).min(w.power.len() - 2);
    let start = out.len();
    let db = |k: usize| 10.0 * w.power[k].max(1e-12).log10();
    for k in first..=last {
        let p = w.power[k];
        if p < 1e-8 || p <= w.power[k - 1] || p < w.power[k + 1] {
            continue;
        }
        let (a, b, c) = (db(k - 1), db(k), db(k + 1));
        let d = a - 2.0 * b + c;
        let off = if d.abs() > 1e-9 {
            (0.5 * (a - c) / d).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let hz = (k as f32 + off) * bin_hz;
        let (midi, _) = note(hz);
        if !(21..=108).contains(&midi) || !wanted(midi) {
            continue;
        }
        let level = b - 0.25 * (a - c) * off;
        if level < -80.0 {
            continue;
        }
        let peak = Peak {
            hz,
            db: level,
            magnitude: 10f32.powf(level / 20.0),
            midi,
            harmonic: false,
        };
        if out.len() - start < PEAKS {
            out.push(peak);
        } else if let Some(quietest) =
            (start..out.len()).min_by(|x, y| out[*x].db.total_cmp(&out[*y].db))
            && out[quietest].db < peak.db
        {
            out[quietest] = peak;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::fine::midi_hz;

    /// A note with its harmonics (a plucked string's: each a little quieter).
    fn tone(midi: f32, frames: usize) -> Vec<f32> {
        let f = midi_hz(midi);
        (0..frames)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                (1..=6)
                    .map(|h| (t * f * h as f32 * std::f32::consts::TAU).sin() * 0.3 / h as f32)
                    .sum()
            })
            .collect()
    }

    fn lit(notes: &Notes) -> Vec<String> {
        (0..KEYS)
            .filter(|k| notes.keys[*k] > 0.5)
            .map(|k| crate::analyzer::fine::note_name(21 + k as i32))
            .collect()
    }

    #[test]
    fn a_low_note_with_its_harmonics_is_one_note() {
        let mut notes = Notes::new();
        // C2: its octave, fifth above and so on are its own harmonics
        notes.update(&tone(36.0, LONG), 48_000);
        assert_eq!(lit(&notes), ["C2"]);
        // a semitone apart, low down: E2 and F2 told apart
        let mut both = tone(40.0, LONG);
        for (a, b) in both.iter_mut().zip(tone(41.0, LONG)) {
            *a += b;
        }
        notes.update(&both, 48_000);
        assert_eq!(lit(&notes), ["E2", "F2"]);
    }

    #[test]
    fn a_chord_is_its_notes() {
        let mut notes = Notes::new();
        let mut chord = tone(57.0, LONG); // A3
        for m in [60.0, 64.0] {
            // C4, E4
            for (a, b) in chord.iter_mut().zip(tone(m, LONG)) {
                *a += b;
            }
        }
        notes.update(&chord, 48_000);
        assert_eq!(lit(&notes), ["A3", "C4", "E4"]);
        let mut top: Vec<usize> = (0..12).collect();
        top.sort_by(|a, b| notes.chroma[*b].total_cmp(&notes.chroma[*a]));
        let mut names: Vec<&str> = top[..3]
            .iter()
            .map(|c| crate::analyzer::fine::NOTE_NAMES[*c])
            .collect();
        names.sort();
        assert_eq!(names, ["A", "C", "E"]);
    }
}
