//! A look-ahead true-peak limiter for a track that is made louder
//! (ReplayGain or its pre-amplification above unity): the sound is held
//! back [`LOOKAHEAD`] frames, so the gain can come down smoothly before a
//! peak instead of bending the peak itself. Its peaks are found between the
//! samples too (4x oversampled), and kept below [`CEILING_DB`] dBTP.
//!
//! A track played at unity gain never meets it: the decoder leaves the
//! samples untouched (bit for bit). Once it has started on a track it stays
//! in the path until a seek or the track's end (its delay must not come and
//! go mid-stream), with nothing to do where nothing peaks.

/// Frames the sound is held back (1.3 ms at 48 kHz).
pub const LOOKAHEAD: usize = 64;
/// The highest true peak let through, dBTP.
pub const CEILING_DB: f32 = -1.0;
/// Taps of each phase of the oversampling filter.
const TAPS: usize = 8;

pub struct Limiter {
    /// The frames held back (a ring) and each one's gain needed.
    delay: [[f32; 2]; LOOKAHEAD],
    need: [f32; LOOKAHEAD],
    at: usize,
    filled: usize,
    /// The last TAPS input frames, for the peaks between samples.
    history: [[f32; 2]; TAPS],
    hist_at: usize,
    phases: [[f32; TAPS]; 4],
    /// The gain now (falls before a peak, rises slowly after it).
    gain: f32,
    release: f32,
    ceiling: f32,
    /// Whether it is in the path (it started on this track).
    pub active: bool,
}

impl Limiter {
    pub fn new(rate: u32) -> Limiter {
        Limiter {
            delay: [[0.0; 2]; LOOKAHEAD],
            need: [1.0; LOOKAHEAD],
            at: 0,
            filled: 0,
            history: [[0.0; 2]; TAPS],
            hist_at: 0,
            phases: phases(),
            gain: 1.0,
            // back to unity with a time constant of 80 ms
            release: 1.0 - (-1.0 / (0.08 * rate.max(1) as f32)).exp(),
            // (aiming 0.05 dB lower: the peaks between samples are estimates)
            ceiling: 10f32.powf((CEILING_DB - 0.05) / 20.0),
            active: false,
        }
    }

    /// Forget everything (a seek, a new track): out of the path again.
    pub fn reset(&mut self) {
        self.delay = [[0.0; 2]; LOOKAHEAD];
        self.need = [1.0; LOOKAHEAD];
        self.at = 0;
        self.filled = 0;
        self.history = [[0.0; 2]; TAPS];
        self.hist_at = 0;
        self.gain = 1.0;
        self.active = false;
    }

    /// The highest of the frame's samples and the sound between it and the
    /// frames before (4x).
    fn peak(&mut self, l: f32, r: f32) -> f32 {
        self.history[self.hist_at] = [l, r];
        let mut peak = l.abs().max(r.abs());
        for phase in &self.phases {
            for c in 0..2 {
                let mut v = 0.0;
                for (t, tap) in phase.iter().enumerate() {
                    v += tap * self.history[(self.hist_at + TAPS - t) % TAPS][c];
                }
                peak = peak.max(v.abs());
            }
        }
        self.hist_at = (self.hist_at + 1) % TAPS;
        peak
    }

    /// Interleaved stereo in place: what comes out is LOOKAHEAD frames
    /// behind what went in (the first call gives that many fewer frames).
    pub fn process(&mut self, samples: &mut Vec<f32>) {
        self.active = true;
        let frames = samples.len() / 2;
        let mut written = 0;
        for k in 0..frames {
            let (l, r) = (samples[2 * k], samples[2 * k + 1]);
            let peak = self.peak(l, r);
            let need = if peak > self.ceiling {
                self.ceiling / peak
            } else {
                1.0
            };
            if let Some([ol, or]) = self.push([l, r], need) {
                samples[2 * written] = ol;
                samples[2 * written + 1] = or;
                written += 1;
            }
        }
        samples.truncate(written * 2);
    }

    /// A frame in, the frame LOOKAHEAD behind it out (once there is one),
    /// at the gain that keeps every peak ahead of it below the ceiling.
    fn push(&mut self, frame: [f32; 2], need: f32) -> Option<[f32; 2]> {
        let out = (self.filled == LOOKAHEAD).then(|| self.delay[self.at]);
        self.delay[self.at] = frame;
        self.need[self.at] = need;
        self.at = (self.at + 1) % LOOKAHEAD;
        self.filled = (self.filled + 1).min(LOOKAHEAD);
        let out = out?;
        // the lowest gain a peak in the next LOOKAHEAD frames needs: the gain
        // falls towards it evenly over the frames before it comes
        let mut lowest = 1.0f32;
        let mut ramp = 1.0f32;
        for k in 0..LOOKAHEAD {
            let n = self.need[(self.at + k) % LOOKAHEAD];
            if n < 1.0 {
                lowest = lowest.min(n);
                // (k frames before a peak needing n: as far down as a linear
                // fall from now reaching n there)
                // (the filter finds a peak between samples a few frames
                // after it: reached that much sooner)
                let steps = (k + 1).saturating_sub(TAPS / 2).max(1);
                let on_the_way = self.gain + (n - self.gain) * (1.0 / steps as f32);
                ramp = ramp.min(on_the_way);
            }
        }
        if lowest < 1.0 || self.gain < 1.0 {
            let target = ramp.min(1.0);
            self.gain = if target < self.gain {
                target
            } else {
                // rising again: slowly, never above what is ahead
                (self.gain + (1.0 - self.gain) * self.release).min(target)
            };
            if self.gain > 0.999_99 && lowest >= 1.0 {
                self.gain = 1.0;
            }
        }
        if self.gain == 1.0 {
            Some(out)
        } else {
            Some([out[0] * self.gain, out[1] * self.gain])
        }
    }

    /// The end of the track: the frames still held back.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if !self.active {
            return;
        }
        // (LOOKAHEAD frames of nothing push out the `filled` held back)
        for _ in 0..LOOKAHEAD {
            if let Some([l, r]) = self.push([0.0, 0.0], 1.0) {
                out.push(l);
                out.push(r);
            }
        }
        self.reset();
    }
}

/// A windowed sinc for 4x oversampling (the three phases between the
/// samples matter; the fourth is the sample itself).
fn phases() -> [[f32; TAPS]; 4] {
    let mut out = [[0.0f32; TAPS]; 4];
    let len = (TAPS * 4) as f64;
    for (p, phase) in out.iter_mut().enumerate() {
        let mut sum = 0.0;
        for (t, tap) in phase.iter_mut().enumerate() {
            let n = (t * 4 + p) as f64;
            let x = (n - (len - 1.0) / 2.0) / 4.0;
            let sinc = if x.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * n / (len - 1.0)).cos()
                + 0.08 * (4.0 * std::f64::consts::PI * n / (len - 1.0)).cos();
            *tap = (sinc * w) as f32;
            sum += sinc * w;
        }
        for tap in phase.iter_mut() {
            *tap /= sum as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f32 / 48_000.0;
                // (it begins over 10 ms, as music does)
                let v = amp
                    * (i as f32 / 480.0).min(1.0)
                    * (0.6 * (t * 220.0 * std::f32::consts::TAU).sin()
                        + 0.4 * (t * 3_300.0 * std::f32::consts::TAU).sin());
                [v, v * 0.9]
            })
            .collect()
    }

    #[test]
    fn peaks_stay_below_the_ceiling_without_a_hard_edge() {
        // 9 dB too loud
        let input = tone(48_000, 2.8);
        let mut limiter = Limiter::new(48_000);
        let mut out = input.clone();
        limiter.process(&mut out);
        limiter.flush(&mut out);
        assert_eq!(out.len(), input.len(), "every frame comes out");
        let ceiling = 10f32.powf(CEILING_DB / 20.0);
        let highest = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(highest <= ceiling + 1e-4, "{highest}");
        // not a clipper: the wave keeps its shape - each frame is its input
        // scaled (the held back frames come out in order), both sides alike,
        // and the gain moves smoothly
        let mut last = 1.0f32;
        for (frame, source) in out.chunks(2).zip(input.chunks(2)) {
            if source[0].abs() > 0.05 {
                let g0 = frame[0] / source[0];
                let g1 = frame[1] / source[1];
                assert!((g0 - g1).abs() < 1e-3, "both sides the same gain");
                assert!(g0 > 0.0 && g0 <= 1.0 + 1e-6, "{g0}");
                assert!((g0 - last).abs() < 0.05, "a jump in gain: {last} to {g0}");
                last = g0;
            }
        }
    }

    #[test]
    fn what_needs_nothing_passes_bit_for_bit() {
        let input = tone(10_000, 0.7);
        let mut limiter = Limiter::new(48_000);
        let mut out = input.clone();
        limiter.process(&mut out);
        limiter.flush(&mut out);
        assert_eq!(out, input);
    }
}
