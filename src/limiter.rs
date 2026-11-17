//! A look-ahead true-peak limiter: the sound is held back about 4 ms, so
//! the gain comes down smoothly before a peak (over those 4 ms) instead of
//! bending the peak itself, and rises again over 80 ms. Peaks are found
//! between the samples too - 8x oversampled by a windowed sinc (BS.1770's
//! 4x filter reads dense, clipped masters up to 0.8 dB low) - and kept
//! below the ceiling. Where it holds nothing (only delays) it neither
//! looks for peaks nor scans what is ahead: its cost is a copy.
//!
//! The decoder puts one in the way of a track ReplayGain makes louder
//! (ceiling [`CEILING_DB`]); the player one after the resampler, where a
//! track is played at another rate than its own (its intersample peaks
//! become samples: [`RESAMPLED_DB`]). A track at unity gain at its own rate
//! meets neither: its samples go out bit for bit. Once one has started it
//! stays in the path until a seek or the end (its delay must not come and
//! go mid-stream), with nothing to do where nothing peaks.

/// The detector's taps a phase (a windowed sinc) and its phases.
const TAPS: usize = 64;
const PHASES: usize = 8;

/// The 8x interpolator: for each fraction k/8 between two samples, the
/// taps over the TAPS samples round it (a Kaiser-like window).
fn oversampling() -> [[f32; TAPS]; PHASES - 1] {
    let mut out = [[0.0f32; TAPS]; PHASES - 1];
    for (k, phase) in out.iter_mut().enumerate() {
        let f = (k + 1) as f64 / PHASES as f64;
        let mut sum = 0.0f64;
        for (t, tap) in phase.iter_mut().enumerate() {
            // (sample t is TAPS/2 - 1 - t + f before the point wanted)
            let d = t as f64 - (TAPS / 2 - 1) as f64 - f;
            let sinc = if d.abs() < 1e-9 {
                1.0
            } else {
                (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d)
            };
            let x = d / (TAPS as f64 / 2.0);
            let window = if x.abs() < 1.0 {
                0.42 + 0.5 * (std::f64::consts::PI * x).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * x).cos()
            } else {
                0.0
            };
            *tap = (sinc * window) as f32;
            sum += sinc * window;
        }
        for tap in phase.iter_mut() {
            *tap = (*tap as f64 / sum) as f32;
        }
    }
    out
}

/// The most frames held back (4 ms at 256 kHz).
const MOST: usize = 1024;
/// Seconds held back: what the gain takes to come down.
const AHEAD_SECS: f32 = 0.004;
/// The highest true peak let through a track made louder, dBTP.
pub const CEILING_DB: f32 = -1.0;
/// And through what the resampler made of a track, dBTP.
pub const RESAMPLED_DB: f32 = -0.3;

pub struct Limiter {
    /// The frames held back (a ring) and each one's gain needed.
    delay: Box<[[f32; 2]; MOST]>,
    need: Box<[f32; MOST]>,
    ahead: usize,
    at: usize,
    filled: usize,
    /// The last TAPS input frames, for the peaks between samples.
    history: [[f32; 2]; TAPS],
    hist_at: usize,
    phases: [[f32; TAPS]; PHASES - 1],
    /// Frames ahead whose gain must come down (none: nothing to scan),
    /// and how many times what is ahead was scanned (tests).
    pending: usize,
    pub scans: u64,
    /// The gain now (falls before a peak, rises slowly after it).
    gain: f32,
    release: f32,
    ceiling: f32,
    /// Whether it is in the path (it started on this track).
    pub active: bool,
}

impl Limiter {
    /// One that keeps true peaks below `ceiling_db` dBTP at `rate`.
    pub fn with_ceiling(rate: u32, ceiling_db: f32) -> Limiter {
        let rate = rate.max(8000);
        Limiter {
            delay: Box::new([[0.0; 2]; MOST]),
            need: Box::new([1.0; MOST]),
            ahead: ((rate as f32 * AHEAD_SECS) as usize).clamp(16, MOST),
            at: 0,
            filled: 0,
            history: [[0.0; 2]; TAPS],
            hist_at: 0,
            phases: oversampling(),
            pending: 0,
            scans: 0,
            gain: 1.0,
            // back to unity with a time constant of 80 ms
            release: 1.0 - (-1.0 / (0.08 * rate as f32)).exp(),
            // (aiming 0.5 dB lower: what an 8x look still misses, and
            // what a gain that moves adds between the samples)
            ceiling: 10f32.powf((ceiling_db - 0.5) / 20.0),
            active: false,
        }
    }

    /// A track made louder's: [`CEILING_DB`].
    pub fn new(rate: u32) -> Limiter {
        Limiter::with_ceiling(rate, CEILING_DB)
    }

    /// Forget everything (a seek, a new track): out of the path again.
    pub fn reset(&mut self) {
        self.delay.fill([0.0; 2]);
        self.need.fill(1.0);
        self.at = 0;
        self.filled = 0;
        self.history = [[0.0; 2]; TAPS];
        self.hist_at = 0;
        self.pending = 0;
        self.gain = 1.0;
        self.active = false;
    }

    /// Frames it holds back.
    pub fn delay_frames(&self) -> usize {
        self.ahead
    }

    /// The frame into the detector's history; with `look`, the highest of
    /// the sound between the frames round it (8x), the samples' too.
    fn peak(&mut self, l: f32, r: f32, look: bool) -> f32 {
        self.history[self.hist_at] = [l, r];
        self.hist_at = (self.hist_at + 1) % TAPS;
        if !look {
            return 0.0;
        }
        // (the point looked at lies TAPS/2 frames back: the detector's
        // delay, well within the look-ahead)
        let mut peak = 0.0f32;
        for phase in &self.phases {
            let mut v = [0.0f32; 2];
            for (t, tap) in phase.iter().enumerate() {
                let [x, y] = self.history[(self.hist_at + t) % TAPS];
                v[0] += tap * x;
                v[1] += tap * y;
            }
            peak = peak.max(v[0].abs()).max(v[1].abs());
        }
        let [x, y] = self.history[(self.hist_at + TAPS / 2) % TAPS];
        peak.max(x.abs()).max(y.abs()).max(l.abs()).max(r.abs())
    }

    /// Interleaved stereo in place: what comes out is the look-ahead
    /// behind what went in (the first call gives that many fewer frames).
    pub fn process(&mut self, samples: &mut Vec<f32>) {
        self.process_holding(samples, true);
    }

    /// As [`Limiter::process`], the peaks held only while `hold` (else the
    /// sound goes through as it is, only delayed - bit for bit once the
    /// gain is back at unity): in the path all along, so that its delay
    /// does not come and go, holding where the sound may go over.
    pub fn process_holding(&mut self, samples: &mut Vec<f32>, hold: bool) {
        self.active = true;
        let frames = samples.len() / 2;
        let mut written = 0;
        for k in 0..frames {
            let (l, r) = (samples[2 * k], samples[2 * k + 1]);
            let peak = self.peak(l, r, hold);
            let need = if hold && peak > self.ceiling {
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

    /// A frame in, the frame the look-ahead behind it out (once there is
    /// one), at the gain that keeps every peak ahead of it below the ceiling.
    fn push(&mut self, frame: [f32; 2], need: f32) -> Option<[f32; 2]> {
        let n = self.ahead;
        let out = (self.filled == n).then(|| self.delay[self.at]);
        if self.filled == n && self.need[self.at] < 1.0 {
            self.pending -= 1;
        }
        if need < 1.0 {
            self.pending += 1;
        }
        self.delay[self.at] = frame;
        self.need[self.at] = need;
        self.at = (self.at + 1) % n;
        self.filled = (self.filled + 1).min(n);
        let out = out?;
        // (nothing ahead to come down for, at unity: nothing to scan)
        if self.pending == 0 && self.gain == 1.0 {
            return Some(out);
        }
        self.scans += 1;
        // the gain falls evenly towards what each peak ahead needs, to be
        // there when it comes (the filter finds a peak between samples a
        // few frames after it: reached that much sooner)
        let mut lowest = 1.0f32;
        let mut ramp = 1.0f32;
        for k in 0..n {
            let wanted = self.need[(self.at + k) % n];
            if wanted < 1.0 {
                lowest = lowest.min(wanted);
                let steps = (k + 1).saturating_sub(TAPS / 2).max(1);
                ramp = ramp.min(self.gain + (wanted - self.gain) / steps as f32);
            }
        }
        if lowest < 1.0 || self.gain < 1.0 {
            self.gain = if ramp < self.gain {
                ramp
            } else {
                // rising again: slowly, never above what is ahead
                (self.gain + (1.0 - self.gain) * self.release).min(ramp)
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
        // (that many frames of nothing push out those held back)
        for _ in 0..self.ahead {
            if let Some([l, r]) = self.push([0.0, 0.0], 1.0) {
                out.push(l);
                out.push(r);
            }
        }
        self.reset();
    }
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
    fn where_it_holds_nothing_it_scans_nothing() {
        // in the path all along, it must cost next to nothing where it
        // only delays: no look for peaks, no scan of what is ahead (at
        // 192 kHz a scan a frame was 150 million steps a second)
        let mut limiter = Limiter::with_ceiling(192_000, RESAMPLED_DB);
        let mut quiet = tone(192_000, 0.5);
        limiter.process_holding(&mut quiet, true);
        let mut loud = tone(192_000, 2.0);
        limiter.process_holding(&mut loud, false);
        assert_eq!(limiter.scans, 0, "it scanned with nothing to hold");
        // and when it does hold, it works as it should
        let mut loud = tone(192_000, 2.0);
        limiter.process_holding(&mut loud, true);
        assert!(limiter.scans > 0);
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
        // and between the samples too, as a BS.1770 meter reads it
        let mut meter = crate::analyzer::loudness::Meter::new(48_000);
        meter.push(&out);
        assert!(
            meter.true_peak <= CEILING_DB + 0.1,
            "{} dBTP",
            meter.true_peak
        );
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
                // (over 4 ms: no step of more than 2 % a frame)
                assert!((g0 - last).abs() < 0.02, "a jump in gain: {last} to {g0}");
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

#[cfg(test)]
mod true_peaks {
    use super::*;

    #[test]
    fn high_tones_peaking_between_their_samples_are_held_too() {
        // 12 and 18 kHz at 48 kHz, the samples beside the peaks, 6 dB too
        // loud: a meter after BS.1770 reads them below the ceiling (the
        // old 8-tap detector read them 0.4-0.5 dB low and let them over)
        for hz in [12_000.0f32, 18_000.0] {
            let input: Vec<f32> = (0..48_000)
                .flat_map(|i| {
                    let fade = (i as f32 / 480.0).min(1.0);
                    let v =
                        (std::f32::consts::TAU * hz * i as f32 / 48_000.0 + 0.6).sin() * 1.8 * fade;
                    [v, v]
                })
                .collect();
            let mut limiter = Limiter::new(48_000);
            let mut out = input.clone();
            limiter.process(&mut out);
            limiter.flush(&mut out);
            let mut meter = crate::analyzer::loudness::Meter::new(48_000);
            meter.push(&out);
            assert!(
                meter.true_peak <= CEILING_DB + 0.1,
                "{hz} Hz: {} dBTP",
                meter.true_peak
            );
            assert!(
                meter.true_peak > CEILING_DB - 1.0,
                "{hz} Hz: {} dBTP",
                meter.true_peak
            );
        }
    }
}
