//! What the visualizations draw from, worked out once a frame for all of
//! them and without allocating: the spectrum as bars (axross's
//! `spectrum_bars`: logarithmic bands from 40 Hz to 16 kHz, the loudest
//! bin of each on a -70..0 dBFS scale, smoothed and with falling peaks as
//! its options say), their history, the levels of left and right, the raw
//! samples, the stereo picture (correlation, width, balance), the loudness
//! after EBU R128 ([`loudness`]), the beat and the tempo ([`beat`]) and,
//! for the faces that ask for it, a fine spectrum with notes, chroma and
//! mel bands ([`fine`]).
//!
//! Smoothing and peak fall are per second and the history moves on at a
//! fixed rate ([`TICK_HZ`]): how the picture moves does not depend on the
//! frame rate.

pub mod beat;
pub mod fine;
pub mod guard;
pub mod loudness;
pub mod notes;

use crate::state::VisualOptions;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// The frames the bars are worked out from (axross's 2048).
pub const FFT_SIZE: usize = 2048;
/// Rows of history kept (the waterfall shows all, the ridges the last 28).
pub const HISTORY: usize = 128;
/// The most bars (axross's option range is 8..=128).
pub const MAX_BARS: usize = 128;
/// The history (and what is counted per step: the beat's turn, the
/// sparks) moves on this many times a second - axross's default frame rate.
pub const TICK_HZ: f32 = 30.0;
/// The least smoothing and the fastest peak fall (% a second) the pictures
/// take, whatever the options (photosensitivity).
pub const SAFE_SMOOTHING: u32 = 75;
pub const SAFE_PEAK_FALL: u32 = 100;
/// Samples kept for the scopes.
pub const SAMPLES: usize = 2048;
/// axross's spectrum: 40 Hz to 16 kHz, -70..0 dBFS.
pub const LOW_HZ: f32 = 40.0;
pub const HIGH_HZ: f32 = 16_000.0;
const RANGE_DB: f32 = 70.0;
/// The VU meter's scale: -60..0 dBFS (axross's VU_RANGE_DB).
pub const VU_RANGE_DB: f32 = 60.0;
/// Below this the bass is counted (the starfield's speed, the sun).
pub const BASS_HZ: f32 = 250.0;

/// What a face needs beyond what is always worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    /// The fine spectrum, its notes, chroma and mel bands.
    pub fine: bool,
}

/// A band of the bars: the FFT bins `start..stop`, or (none) the
/// spectrum read at `centre` (in bins).
#[derive(Clone, Copy, Debug, Default)]
struct Band {
    start: usize,
    stop: usize,
    centre: f32,
}

pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mags: Vec<f32>,
    bands: Vec<Band>,
    bands_for: (usize, u32),
    /// This frame's bars as worked out (0..1), before smoothing.
    pub raw: Vec<f32>,
    /// The bars shown (smoothed) and their peaks.
    pub bars: Vec<f32>,
    pub peaks: Vec<f32>,
    /// Rows of history (raw bars for the waterfall, smoothed ones for the
    /// ridges and the terrain): a ring of HISTORY rows of MAX_BARS.
    raw_history: Vec<f32>,
    history: Vec<f32>,
    /// The waterfall's rows: each bar's slow mean (two seconds) and half
    /// its change from it, eased (a chirp's sweeps, scrolled, were a
    /// grating over 70 % of the picture), and those means.
    waterfall: Vec<f32>,
    waterfall_mean: Vec<f32>,
    head: usize,
    filled: usize,
    clock: f32,
    /// Ticks (of TICK_HZ) this frame made.
    pub ticks: u32,
    tick_raw: Vec<f32>,
    /// Each bar's rise at the last tick (where sparks start).
    pub rise: Vec<f32>,
    /// Turns with the onsets: the bars' mean rise a tick, times 3 (axross's
    /// FLUX_TURN), up to 4 pi; `phase` follows it smoothly.
    phase_target: f32,
    pub phase: f32,
    /// The latest samples: mono and left / right (oldest first).
    pub mono: Vec<f32>,
    pub stereo: Vec<[f32; 2]>,
    /// RMS and peak of left and right on the VU scale (0..1), as axross's
    /// VU meter shows them.
    pub vu: [f32; 2],
    pub vu_peak: [f32; 2],
    /// The bars below BASS_HZ, their mean (0..1).
    pub bass: f32,
    bass_count: usize,
    /// Loudness of the bars: the lowest third's mean, its rise (0..1).
    pub energy: f32,
    /// The middle and the highest third's means (0..1).
    pub middle: f32,
    pub treble: f32,
    /// How much calmer than the motion says the colours call for (1..1.6:
    /// the guards' turns that much further apart) - see
    /// `viz::scheme_calm`.
    pub calm: f32,
    /// Guards on the means too: each bar turns no faster than the motion
    /// allows, but a mean of many, each turning in its own time, turned
    /// as often as all of them together (the plasma, driven by them, made
    /// 11 - 17 transitions a second by the strict reading).
    mean_guards: [guard::Guard; 4],
    pub onset: f32,
    previous_energy: f32,
    /// Stereo: correlation (-1..1), width (side against all, 0..1),
    /// balance (-1 left .. 1 right) of the latest samples, smoothed.
    pub correlation: f32,
    pub width: f32,
    pub balance: f32,
    pub loudness: loudness::Meter,
    pub beat: beat::Beat,
    pub fine: fine::Fine,
    /// Seconds of sound analysed (the generative faces' clock: it stands
    /// while nothing plays).
    pub time: f64,
    /// The clock as the energy and the bass drive it (their sums over
    /// time): a face moving faster as it gets louder moves on by these -
    /// `time * (1 + energy)` jumped at each change of the energy, the
    /// further the longer it had played (a flyover a minute in leapt 50
    /// units at a beat).
    pub energy_flow: f64,
    pub bass_flow: f64,
    /// The device's rate the numbers are for.
    pub rate: u32,
    last_written: u64,
    /// The meters' thread's readings, when it runs (then loudness, beat and
    /// notes come from it, which hears every sample).
    external: Option<crate::meters::Readings>,
    /// How the pictures may move: what drives their brightness (the bars,
    /// the beat's pulse) turns no faster than the flash guard allows.
    pub motion: guard::Motion,
    guards: Vec<guard::Guard>,
    /// The bars as axross smooths them (the guard follows these); the VU
    /// meter's likewise.
    smooth: Vec<f32>,
    vu_smooth: [f32; 2],
    vu_guards: [guard::Guard; 2],
    pulse_guard: guard::Guard,
    raw_pulse: f32,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        // axross's window: a periodic Hann over the frames
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        let mut a = Analyzer {
            fft,
            window,
            buffer: vec![Complex::default(); FFT_SIZE],
            scratch,
            mags: vec![0.0; FFT_SIZE / 2 + 1],
            bands: Vec::with_capacity(MAX_BARS),
            bands_for: (0, 0),
            raw: Vec::with_capacity(MAX_BARS),
            bars: Vec::with_capacity(MAX_BARS),
            peaks: Vec::with_capacity(MAX_BARS),
            raw_history: vec![0.0; HISTORY * MAX_BARS],
            history: vec![0.0; HISTORY * MAX_BARS],
            waterfall: vec![0.0; HISTORY * MAX_BARS],
            waterfall_mean: vec![0.0; MAX_BARS],
            head: 0,
            filled: 0,
            clock: 0.0,
            ticks: 0,
            tick_raw: Vec::with_capacity(MAX_BARS),
            rise: Vec::with_capacity(MAX_BARS),
            phase_target: 0.0,
            phase: 0.0,
            mono: Vec::with_capacity(SAMPLES),
            stereo: Vec::with_capacity(SAMPLES),
            vu: [0.0; 2],
            vu_peak: [0.0; 2],
            bass: 0.0,
            bass_count: 1,
            energy: 0.0,
            onset: 0.0,
            previous_energy: 0.0,
            correlation: 0.0,
            width: 0.0,
            balance: 0.0,
            loudness: loudness::Meter::new(48_000),
            beat: beat::Beat::new(),
            fine: fine::Fine::new(),
            time: 0.0,
            energy_flow: 0.0,
            bass_flow: 0.0,
            rate: 48_000,
            last_written: 0,
            external: None,
            motion: guard::Motion::Calm,
            guards: vec![guard::Guard::default(); MAX_BARS],
            smooth: vec![0.0; MAX_BARS],
            vu_smooth: [0.0; 2],
            vu_guards: [guard::Guard::default(); 2],
            pulse_guard: guard::Guard::default(),
            middle: 0.0,
            treble: 0.0,
            calm: 1.0,
            mean_guards: [guard::Guard::default(); 4],
            raw_pulse: 0.0,
        };
        a.resize(32, 48_000);
        a
    }

    /// The bar count or the rate changed: the bands anew, the bars from 0.
    fn resize(&mut self, n: usize, rate: u32) {
        if self.bands_for == (n, rate) {
            return;
        }
        let size_changed = self.bands_for.0 != n;
        self.bands_for = (n, rate);
        let rate_f = rate as f64;
        let top = (HIGH_HZ as f64).min(rate_f / 2.0);
        let low = LOW_HZ as f64;
        let edge = |k: usize| low * (top / low).powf(k as f64 / n as f64);
        let bin_hz = rate_f / FFT_SIZE as f64;
        // the first bin at or above a frequency (numpy's searchsorted, left)
        let first_at = |hz: f64| ((hz / bin_hz).ceil().max(0.0) as usize).min(FFT_SIZE / 2 + 1);
        self.bands.clear();
        for k in 0..n {
            let (lo, hi) = (edge(k), edge(k + 1));
            self.bands.push(Band {
                start: first_at(lo),
                stop: first_at(hi),
                centre: ((lo * hi).sqrt() / bin_hz) as f32,
            });
        }
        let mut bass = 0;
        for k in 0..n {
            if ((edge(k) * edge(k + 1)).sqrt() as f32) < BASS_HZ {
                bass += 1;
            }
        }
        self.bass_count = bass.max(1);
        if size_changed {
            for v in [
                &mut self.raw,
                &mut self.bars,
                &mut self.peaks,
                &mut self.tick_raw,
                &mut self.rise,
            ] {
                v.clear();
                v.resize(n, 0.0);
            }
            self.raw_history.fill(0.0);
            self.history.fill(0.0);
            self.waterfall.fill(0.0);
            self.waterfall_mean.fill(0.0);
            self.filled = 0;
        }
    }

    /// One frame. `tap` holds the latest stereo samples (interleaved) and
    /// `written` the count of frames played up to its last one (what is new
    /// since the last frame is the difference); `dt` is the seconds since
    /// the last frame; `live` is false while nothing plays (the bars fall to
    /// rest, the clock stands).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        tap: &[f32],
        written: u64,
        rate: u32,
        options: &VisualOptions,
        dt: f32,
        live: bool,
        needs: Needs,
    ) {
        let rate = rate.clamp(8000, 384_000);
        let n = (options.bars as usize).clamp(8, MAX_BARS);
        self.resize(n, rate);
        if rate != self.rate {
            self.rate = rate;
            self.loudness = loudness::Meter::new(rate);
            self.beat = beat::Beat::new();
        }
        // (the music stopped: the picture falls to rest at the clock's pace
        // also at a frame every second or two - a slow renderer's window
        // held to its budget fell at a quarter of a second a frame, and
        // went on drawing long after the music: review 13)
        let dt = dt.clamp(0.0, if live { 0.25 } else { 2.0 });
        let frames = tap.len() / 2;
        let new = if written >= self.last_written {
            (written - self.last_written).min(frames as u64) as usize
        } else {
            frames.min(written as usize)
        };
        self.last_written = written;
        let live = live && frames >= 256;
        if live {
            let step = dt * self.motion.pace();
            self.time += step as f64;
            self.energy_flow += (step * self.energy.min(1.5)) as f64;
            self.bass_flow += (step * self.bass.min(1.5)) as f64;
            self.spectrum(tap);
        } else {
            self.raw.fill(0.0);
        }
        // axross's smoothing: a bar jumps up and eases down, keeping a share
        // of its height a 30th of a second
        // (the pictures keep at least SAFE_SMOOTHING and their peaks fall
        // at most SAFE_PEAK_FALL whatever the options say - axross's
        // values are kept in its file: no smoothing at all, 128 bars and
        // peaks falling at 300 % a second made the LED equalizer flash 11
        // times a second by the strict reading, in Calm)
        // (more bars, more of them turning out of step: with 128, at least
        // 85 %)
        let floor = SAFE_SMOOTHING + (options.bars.saturating_sub(64) * 10 / 64).min(10);
        let keep = (options.smoothing.clamp(floor, 95) as f32 / 100.0).powf(dt * 30.0);
        let fall = options.peak_fall.min(SAFE_PEAK_FALL) as f32 / 100.0 * dt;
        for i in 0..n {
            let value = self.raw[i];
            let level = self.smooth[i];
            let smoothed = if value > level {
                value
            } else {
                level * keep + value * (1.0 - keep)
            };
            self.smooth[i] = smoothed;
            // (the flash guard: no faster to and fro than it allows)
            self.bars[i] = self.guards[i].step(smoothed, dt / self.calm, self.motion);
            self.peaks[i] = self.bars[i].max(self.peaks[i] - fall);
        }
        let bass =
            self.bars[..self.bass_count.min(n)].iter().sum::<f32>() / self.bass_count.min(n) as f32;
        let third = (n / 3).max(1);
        let mean = |from: usize, to: usize| {
            let (from, to) = (from.min(n), to.min(n).max(from.min(n) + 1).min(n));
            if to > from {
                self.bars[from..to].iter().sum::<f32>() / (to - from) as f32
            } else {
                0.0
            }
        };
        let energy = mean(0, third);
        let (middle, treble) = (mean(n / 3, 2 * n / 3), mean(2 * n / 3, n));
        self.onset = ((energy - self.previous_energy).max(0.0) * 4.0 + self.onset * keep).min(1.0);
        self.previous_energy = energy;
        let motion = self.motion;
        let [g0, g1, g2, g3] = &mut self.mean_guards;
        let gdt = dt / self.calm;
        self.bass = g0.step(bass, gdt, motion);
        self.energy = g1.step(energy, gdt, motion);
        self.middle = g2.step(middle, gdt, motion);
        self.treble = g3.step(treble, gdt, motion);

        // the fixed steps: history, the onsets' turn
        self.ticks = 0;
        self.clock += dt;
        let step = 1.0 / TICK_HZ;
        if self.clock > 4.0 * step {
            self.clock = step; // (a long stall: no burst of steps)
        }
        while self.clock >= step {
            self.clock -= step;
            self.tick();
        }
        // the phase follows its target smoothly (no steps at 60 fps; in
        // Calm slowly)
        let follow = 1.0 - (-dt * self.motion.phase_follow()).exp();
        self.phase += (self.phase_target - self.phase) * follow;

        self.samples(tap, frames, live, dt);
        // (the beat's own pulse, then the guarded one the faces see)
        self.beat.pulse = self.raw_pulse;
        if let Some(r) = self.external {
            self.take(&r);
        } else if live && new > 0 {
            let fresh = &tap[(frames - new) * 2..];
            self.loudness.push(fresh);
            self.beat.push(tap, new, rate);
        }
        self.beat.frame(dt, live);
        self.raw_pulse = self.beat.pulse;
        self.beat.pulse = self
            .pulse_guard
            .step(self.raw_pulse, dt / self.calm, self.motion);
        self.fine.external = self.external.map(|r| (r.keys, r.chroma));
        self.fine.motion = self.motion;
        self.fine.calm = self.calm.max(1.0);
        if needs.fine && live {
            self.fine.update(tap, rate, dt);
        } else if needs.fine {
            self.fine.rest(dt);
        }
    }

    fn tick(&mut self) {
        self.ticks += 1;
        let n = self.raw.len();
        let mut flux = 0.0;
        for i in 0..n {
            let rise = (self.raw[i] - self.tick_raw[i]).max(0.0);
            self.rise[i] = rise;
            flux += rise;
            self.tick_raw[i] = self.raw[i];
        }
        let turn = 3.0 * flux / n.max(1) as f32;
        self.phase_target += turn;
        // (both wrap together, at 4 pi: the picture is the same there)
        if self.phase_target > 4.0 * std::f32::consts::PI {
            self.phase_target -= 4.0 * std::f32::consts::PI;
            self.phase -= 4.0 * std::f32::consts::PI;
        }
        self.head = (self.head + 1) % HISTORY;
        let row = self.head * MAX_BARS;
        self.raw_history[row..row + n].copy_from_slice(&self.raw);
        self.history[row..row + n].copy_from_slice(&self.bars);
        let before = (self.head + HISTORY - 1) % HISTORY * MAX_BARS;
        let slow = 1.0 - (-1.0 / (TICK_HZ * 2.0)).exp();
        for i in 0..n {
            let m = &mut self.waterfall_mean[i];
            *m += (self.bars[i] - *m) * slow;
            let shown = *m + (self.bars[i] - *m) * 0.5;
            let b = self.waterfall[before + i];
            self.waterfall[row + i] = b + (shown - b) * 0.25;
        }
        self.filled = (self.filled + 1).min(HISTORY);
    }

    /// The waterfall's row `age` steps back (calmer than the history).
    pub fn waterfall_row(&self, age: usize) -> &[f32] {
        let at = (self.head + HISTORY - age % HISTORY) % HISTORY * MAX_BARS;
        &self.waterfall[at..at + self.bars.len()]
    }

    /// The history row `age` steps back is the `history_index() - age`th
    /// (an even count of rows wraps round: its evenness holds).
    pub fn history_index(&self) -> usize {
        self.head
    }

    /// The history row `age` steps back (0: the newest), `bars` long.
    pub fn history_row(&self, age: usize) -> &[f32] {
        let at = (self.head + HISTORY - age % HISTORY) % HISTORY * MAX_BARS;
        &self.history[at..at + self.bars.len()]
    }

    /// As [`Analyzer::history_row`], the bars as they were worked out
    /// (unsmoothed: the waterfall).
    pub fn raw_row(&self, age: usize) -> &[f32] {
        let at = (self.head + HISTORY - age % HISTORY) % HISTORY * MAX_BARS;
        &self.raw_history[at..at + self.bars.len()]
    }

    /// How far (0..1) the next history step is: a scrolling picture moves
    /// smoothly in between.
    pub fn tick_fraction(&self) -> f32 {
        (self.clock * TICK_HZ).clamp(0.0, 1.0)
    }

    /// The bars of the latest FFT_SIZE frames: axross's `spectrum_bars`.
    fn spectrum(&mut self, tap: &[f32]) {
        let frames = tap.len() / 2;
        let take = frames.min(FFT_SIZE);
        let start = (frames - take) * 2;
        // (fewer frames: the oldest are silence)
        let pad = FFT_SIZE - take;
        for i in 0..FFT_SIZE {
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
        for (m, c) in self.mags.iter_mut().zip(&self.buffer) {
            *m = c.norm();
        }
        let reference = FFT_SIZE as f32 / 4.0; // a full-scale sine under the Hann window
        let last = self.mags.len() - 1;
        for (bar, band) in self.raw.iter_mut().zip(&self.bands) {
            let peak = if band.stop > band.start {
                self.mags[band.start..band.stop.min(last + 1)]
                    .iter()
                    .fold(0.0f32, |a, b| a.max(*b))
            } else {
                // between two bins: the spectrum read at the band's centre
                let at = band.centre.clamp(0.0, last as f32);
                let k = (at.floor() as usize).min(last);
                let t = at - k as f32;
                let next = (k + 1).min(last);
                self.mags[k] * (1.0 - t) + self.mags[next] * t
            };
            let db = if peak > 0.0 {
                20.0 * (peak / reference).log10()
            } else {
                -200.0
            };
            *bar = ((db + RANGE_DB) / RANGE_DB).clamp(0.0, 1.0);
        }
    }

    /// The scopes' samples, the VU meter, the stereo picture.
    fn samples(&mut self, tap: &[f32], frames: usize, live: bool, dt: f32) {
        self.mono.clear();
        self.stereo.clear();
        let take = if live { frames.min(SAMPLES) } else { 0 };
        let start = (frames - take) * 2;
        let (mut sum_l, mut sum_r, mut sum_lr) = (0.0f64, 0.0f64, 0.0f64);
        let (mut peak_l, mut peak_r) = (0.0f32, 0.0f32);
        let mut side = 0.0f64;
        for f in tap[start..].as_chunks::<2>().0 {
            let (l, r) = (f[0], f[1]);
            self.mono.push((l + r) * 0.5);
            self.stereo.push([l, r]);
            sum_l += (l * l) as f64;
            sum_r += (r * r) as f64;
            sum_lr += (l * r) as f64;
            side += ((l - r) * (l - r)) as f64 * 0.25;
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
        }
        let count = take.max(1) as f64;
        let scale = |v: f32| {
            if v <= 0.0 {
                0.0
            } else {
                ((20.0 * v.log10() + VU_RANGE_DB) / VU_RANGE_DB).clamp(0.0, 1.0)
            }
        };
        let rms = [
            scale((sum_l / count).sqrt() as f32),
            scale((sum_r / count).sqrt() as f32),
        ];
        let peaks = [scale(peak_l), scale(peak_r)];
        // axross's meter: up at once, down by 15 % of the way a 30th of a
        // second; its peak falls 0.01 a 30th
        let keep = 0.85f32.powf(dt * 30.0);
        for c in 0..2 {
            self.vu_smooth[c] = if rms[c] > self.vu_smooth[c] {
                rms[c]
            } else {
                self.vu_smooth[c] * keep + rms[c] * (1.0 - keep)
            };
            // (the meter's bars light half the picture: guarded)
            self.vu[c] = self.vu_guards[c].step(self.vu_smooth[c], dt / self.calm, self.motion);
            self.vu_peak[c] = peaks[c].max(self.vu_peak[c] - 0.3 * dt);
        }
        let follow = 1.0 - (-dt * 6.0).exp();
        let power = sum_l + sum_r;
        let (correlation, width, balance) = if power > 1e-9 * count {
            let mid = power * 0.5 - side; // (l+r)^2/4 summed = (l^2+r^2)/2 - (l-r)^2/4
            (
                (sum_lr / (sum_l * sum_r).sqrt().max(1e-12)).clamp(-1.0, 1.0) as f32,
                (side / (side + mid.max(0.0)).max(1e-12)).clamp(0.0, 1.0) as f32,
                ((sum_r - sum_l) / power).clamp(-1.0, 1.0) as f32,
            )
        } else {
            (0.0, 0.0, 0.0)
        };
        self.correlation += (correlation - self.correlation) * follow;
        self.width += (width - self.width) * follow;
        self.balance += (balance - self.balance) * follow;
    }

    /// From now on loudness, beat and notes are the meters' (each frame's
    /// latest readings given here before [`Analyzer::update`]).
    pub fn set_readings(&mut self, r: crate::meters::Readings) {
        self.external = Some(r);
    }

    /// The meters' readings into what the faces read.
    fn take(&mut self, r: &crate::meters::Readings) {
        let m = &mut self.loudness;
        m.momentary = r.momentary;
        m.short_term = r.short_term;
        m.integrated = r.integrated;
        m.range = r.range;
        m.momentary_max = r.momentary_max;
        m.short_term_max = r.short_term_max;
        m.sample_peak = r.sample_peak;
        m.true_peak = r.true_peak;
        m.seconds = r.seconds;
        let b = &mut self.beat;
        if r.beats != b.count {
            b.pulse = 1.0;
            b.since = 0.0;
        }
        b.count = r.beats;
        b.bpm = r.bpm;
        b.confidence = r.bpm_confidence;
        b.strength = r.beat_strength;
    }

    /// The track's key, as the meters found it over the whole track (root,
    /// minor, how sure); none without them.
    pub fn key(&self) -> Option<(usize, bool, f32)> {
        self.external.map(|r| r.key)
    }

    /// A new track: its loudness counted from nothing.
    pub fn new_track(&mut self) {
        self.loudness.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sine(rate: u32, hz: f32, amp: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * amp;
                [v, v]
            })
            .collect()
    }

    fn run(a: &mut Analyzer, tap: &[f32], written: u64, options: &VisualOptions) {
        a.update(
            tap,
            written,
            48_000,
            options,
            1.0 / 30.0,
            true,
            Needs::default(),
        );
    }

    #[test]
    fn a_tone_lights_its_band_and_silence_falls() {
        let tap = sine(48_000, 1000.0, 0.5, 4096);
        let mut a = Analyzer::new();
        let options = VisualOptions::default();
        // (a few frames: the flash guard lets the bars rise over 60 ms)
        for _ in 0..8 {
            run(&mut a, &tap, 4096, &options);
        }
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
        // -6 dBFS on axross's -70..0 scale
        assert!((loudest.1 - 64.0 / 70.0).abs() < 0.02, "{}", loudest.1);
        assert!(a.vu[0] > 0.8);
        for _ in 0..90 {
            a.update(
                &[],
                4096,
                48_000,
                &options,
                1.0 / 30.0,
                false,
                Needs::default(),
            );
        }
        assert!(a.bars.iter().all(|b| *b < 0.01));
        assert!(a.peaks.iter().all(|p| *p < 0.2));
    }

    /// At a frame every second or two (a slow renderer held to its budget)
    /// the picture falls to rest in the same seconds as at 30 frames a
    /// second, once the music has stopped (it fell a quarter of a second a
    /// frame and the window went on drawing for long: review 13).
    #[test]
    fn at_a_frame_a_second_silence_falls_in_as_many_seconds() {
        let tap = sine(48_000, 1000.0, 0.5, 4096);
        let mut a = Analyzer::new();
        let options = VisualOptions::default();
        for _ in 0..8 {
            run(&mut a, &tap, 4096, &options);
        }
        for _ in 0..3 {
            a.update(&[], 4096, 48_000, &options, 1.0, false, Needs::default());
        }
        assert!(a.bars.iter().all(|b| *b < 0.01), "{:?}", a.bars);
    }

    #[test]
    fn no_two_bars_read_the_same_bin() {
        // noise: every bar its own value (the lowest ones shared FFT bins)
        let mut seed = 3u32;
        let tap: Vec<f32> = (0..8192)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect();
        for bars in [32, 64, 128] {
            let mut a = Analyzer::new();
            let options = VisualOptions {
                bars,
                smoothing: 0,
                ..Default::default()
            };
            run(&mut a, &tap, 4096, &options);
            for pair in a.bars.windows(2) {
                assert_ne!(pair[0], pair[1], "{bars} bars: {:?}", &a.bars[..12]);
            }
        }
    }

    #[test]
    fn the_bar_count_follows_the_option() {
        let mut a = Analyzer::new();
        let options = VisualOptions {
            bars: 64,
            ..Default::default()
        };
        run(&mut a, &[0.0; 8192], 4096, &options);
        assert_eq!(a.bars.len(), 64);
        assert_eq!(a.history_row(0).len(), 64);
    }

    #[test]
    fn the_picture_moves_alike_at_any_frame_rate() {
        // the same second of sound at 20 and at 60 frames a second: the
        // bars fall as far, the history moves on as many rows
        let loud = sine(48_000, 440.0, 0.8, 4096);
        let options = VisualOptions::default();
        let mut results = Vec::new();
        for fps in [20u32, 60] {
            let mut a = Analyzer::new();
            run(&mut a, &loud, 4096, &options);
            let silent = vec![0.0; 8192];
            let mut written = 4096;
            for _ in 0..fps {
                written += 48_000 / fps as u64;
                a.update(
                    &silent,
                    written,
                    48_000,
                    &options,
                    1.0 / fps as f32,
                    true,
                    Needs::default(),
                );
            }
            let loudest = a.bars.iter().cloned().fold(0.0, f32::max);
            let rows = (0..HISTORY)
                .filter(|age| a.history_row(*age).iter().any(|v| *v > 0.0))
                .count();
            results.push((loudest, rows, a.filled));
        }
        assert!(
            (results[0].0 - results[1].0).abs() < 0.02,
            "the bars fell alike: {results:?}"
        );
        assert!(
            (results[0].2 as i64 - results[1].2 as i64).abs() <= 1,
            "{results:?}"
        );
    }

    #[test]
    fn mono_correlates_inverted_does_not() {
        let mut a = Analyzer::new();
        let options = VisualOptions::default();
        let mono = sine(48_000, 300.0, 0.5, 8192);
        for k in 0..60 {
            run(&mut a, &mono, 8192 * (k + 1), &options);
        }
        assert!(a.correlation > 0.99, "{}", a.correlation);
        assert!(a.width < 0.01, "{}", a.width);
        let inverted: Vec<f32> = mono.chunks(2).flat_map(|f| [f[0], -f[1]]).collect();
        for k in 0..60 {
            run(&mut a, &inverted, 8192 * (k + 61), &options);
        }
        assert!(a.correlation < -0.99, "{}", a.correlation);
        assert!(a.width > 0.99, "{}", a.width);
        let left: Vec<f32> = mono.chunks(2).flat_map(|f| [f[0], 0.0]).collect();
        for k in 0..60 {
            run(&mut a, &left, 8192 * (k + 121), &options);
        }
        assert!(a.balance < -0.99, "{}", a.balance);
    }

    #[test]
    fn a_frame_allocates_nothing() {
        // (counted by the allocator of tests/analysis_alloc.rs too; here:
        // the buffers keep their places)
        let mut a = Analyzer::new();
        let options = VisualOptions::default();
        let tap = sine(48_000, 300.0, 0.5, 8192);
        let needs = Needs { fine: true };
        a.update(&tap, 8192, 48_000, &options, 0.016, true, needs);
        let before = (a.bars.as_ptr(), a.mono.as_ptr(), a.fine.db.as_ptr());
        for k in 0..20 {
            a.update(&tap, 8192 + k * 800, 48_000, &options, 0.016, true, needs);
        }
        assert_eq!(
            before,
            (a.bars.as_ptr(), a.mono.as_ptr(), a.fine.db.as_ptr())
        );
    }
}
