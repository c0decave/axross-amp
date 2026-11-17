//! Loudness after ITU-R BS.1770-4 / EBU R128: the K-weighting (a high
//! shelf and a high pass, at any rate), momentary (400 ms) and short-term
//! (3 s) loudness, the integrated loudness with its two gates and the
//! loudness range (LRA, EBU Tech 3342), the sample peak and the true peak
//! (4x oversampled by BS.1770's own filter). Counted from what is played, in 100 ms steps; the
//! integrated loudness and the range from histograms (0.1 LU a step), so
//! a track of any length costs the same.

use std::f64::consts::PI;

/// The histograms: -70..+5 LUFS in 0.1 LU steps.
const LOW: f64 = -70.0;
const STEPS: usize = 750;
const UNIT: f64 = 0.1;
/// 100 ms steps kept: 30 of them make the short-term loudness.
const BLOCKS: usize = 30;
/// Taps of each phase of the 4x oversampling filter.
pub(crate) const TAPS: usize = 12;

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        // (transposed direct form II)
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The two stages of the K-weighting at `rate` (BS.1770's filters, as
/// libebur128 designs them for any rate).
fn k_weighting(rate: u32) -> [Biquad; 2] {
    let fs = rate as f64;
    // the high shelf (the head's effect)
    let f0 = 1_681.974_450_955_533;
    let gain = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (PI * f0 / fs).tan();
    let vh = 10f64.powf(gain / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    // the high pass (RLB)
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let pass = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    [shelf, pass]
}

/// BS.1770-4's 4x oversampling filter (Annex 2): 48 taps in 4 phases.
// (the standard's values as it prints them: multiples of 2^-14, exact)
#[allow(clippy::excessive_precision)]
pub(crate) fn oversampling() -> [[f32; TAPS]; 4] {
    [
        [
            0.001_708_984_375,
            0.010_986_328_125,
            -0.019_653_320_312_5,
            0.033_203_125,
            -0.059_448_242_187_5,
            0.137_329_101_562_5,
            0.972_167_968_75,
            -0.102_294_921_875,
            0.047_607_421_875,
            -0.026_611_328_125,
            0.014_892_578_125,
            -0.008_300_781_25,
        ],
        [
            -0.029_174_804_687_5,
            0.029_296_875,
            -0.051_757_812_5,
            0.089_111_328_125,
            -0.166_503_906_25,
            0.465_087_890_625,
            0.779_785_156_25,
            -0.200_317_382_812_5,
            0.101_562_5,
            -0.058_227_539_062_5,
            0.033_081_054_687_5,
            -0.018_920_898_437_5,
        ],
        [
            -0.018_920_898_437_5,
            0.033_081_054_687_5,
            -0.058_227_539_062_5,
            0.101_562_5,
            -0.200_317_382_812_5,
            0.779_785_156_25,
            0.465_087_890_625,
            -0.166_503_906_25,
            0.089_111_328_125,
            -0.051_757_812_5,
            0.029_296_875,
            -0.029_174_804_687_5,
        ],
        [
            -0.008_300_781_25,
            0.014_892_578_125,
            -0.026_611_328_125,
            0.047_607_421_875,
            -0.102_294_921_875,
            0.972_167_968_75,
            0.137_329_101_562_5,
            -0.059_448_242_187_5,
            0.033_203_125,
            -0.019_653_320_312_5,
            0.010_986_328_125,
            0.001_708_984_375,
        ],
    ]
}

pub struct Meter {
    filters: [[Biquad; 2]; 2],
    /// Frames a 100 ms step, frames into it, its sum of squares (both
    /// channels, K-weighted).
    step: usize,
    into: usize,
    sum: f64,
    /// The last BLOCKS steps' mean squares (a ring) and how many there are.
    blocks: [f64; BLOCKS],
    at: usize,
    count: usize,
    /// Gating blocks a step: how many, their mean squares summed.
    integrated_hist: Vec<(u32, f64)>,
    range_hist: Vec<u32>,
    phases: [[f32; TAPS]; 4],
    history: [[f32; TAPS]; 2],
    hist_at: usize,
    /// LUFS (NEG_INFINITY before there is any).
    pub momentary: f32,
    pub short_term: f32,
    pub integrated: f32,
    /// The loudness range (LU).
    pub range: f32,
    /// The highest momentary and short-term loudness so far.
    pub momentary_max: f32,
    pub short_term_max: f32,
    /// dBFS and dBTP, the highest so far.
    pub sample_peak: f32,
    pub true_peak: f32,
    /// Seconds counted.
    pub seconds: f32,
}

impl Meter {
    pub fn new(rate: u32) -> Meter {
        let k = k_weighting(rate.max(8000));
        Meter {
            filters: [k, k],
            step: (rate.max(8000) / 10) as usize,
            into: 0,
            sum: 0.0,
            blocks: [0.0; BLOCKS],
            at: 0,
            count: 0,
            integrated_hist: vec![(0, 0.0); STEPS],
            range_hist: vec![0; STEPS],
            phases: oversampling(),
            history: [[0.0; TAPS]; 2],
            hist_at: 0,
            momentary: f32::NEG_INFINITY,
            short_term: f32::NEG_INFINITY,
            integrated: f32::NEG_INFINITY,
            range: 0.0,
            momentary_max: f32::NEG_INFINITY,
            short_term_max: f32::NEG_INFINITY,
            sample_peak: f32::NEG_INFINITY,
            true_peak: f32::NEG_INFINITY,
            seconds: 0.0,
        }
    }

    /// Everything counted from nothing (a new track).
    pub fn reset(&mut self) {
        let step = self.step;
        let filters = self.filters;
        let integrated = std::mem::take(&mut self.integrated_hist);
        let range = std::mem::take(&mut self.range_hist);
        *self = Meter {
            integrated_hist: integrated,
            range_hist: range,
            ..Meter::new(step as u32 * 10)
        };
        self.integrated_hist.fill((0, 0.0));
        self.range_hist.fill(0);
        self.filters = filters;
        for f in self.filters.iter_mut().flatten() {
            f.z = [0.0; 2];
        }
    }

    /// Frames played (interleaved stereo).
    pub fn push(&mut self, frames: &[f32]) {
        let mut peak = 0.0f32;
        let mut true_peak = 0.0f32;
        for f in frames.as_chunks::<2>().0 {
            let mut energy = 0.0;
            for c in 0..2 {
                let x = f[c] as f64;
                let shelved = self.filters[c][0].run(x);
                let y = self.filters[c][1].run(shelved);
                energy += y * y;
                peak = peak.max(f[c].abs());
                self.history[c][self.hist_at] = f[c];
            }
            // the true peak: the sound between the samples, 4x
            for c in 0..2 {
                for phase in &self.phases {
                    let mut v = 0.0f32;
                    for (t, tap) in phase.iter().enumerate() {
                        v += tap * self.history[c][(self.hist_at + TAPS - t) % TAPS];
                    }
                    true_peak = true_peak.max(v.abs());
                }
            }
            self.hist_at = (self.hist_at + 1) % TAPS;
            self.sum += energy;
            self.into += 1;
            if self.into == self.step {
                self.block();
            }
        }
        let db = |v: f32| {
            if v > 0.0 {
                20.0 * v.log10()
            } else {
                f32::NEG_INFINITY
            }
        };
        self.sample_peak = self.sample_peak.max(db(peak));
        self.true_peak = self
            .true_peak
            .max(db(true_peak.max(peak)))
            .max(self.sample_peak);
    }

    fn block(&mut self) {
        self.blocks[self.at] = self.sum / self.step as f64;
        self.at = (self.at + 1) % BLOCKS;
        self.count += 1;
        self.sum = 0.0;
        self.into = 0;
        self.seconds = self.count as f32 / 10.0;
        let mean = |n: usize| {
            let mut s = 0.0;
            for k in 1..=n {
                s += self.blocks[(self.at + BLOCKS - k) % BLOCKS];
            }
            s / n as f64
        };
        let lufs = |z: f64| {
            if z > 0.0 {
                -0.691 + 10.0 * z.log10()
            } else {
                f64::NEG_INFINITY
            }
        };
        if self.count >= 4 {
            let z = mean(4);
            let m = lufs(z);
            self.momentary = m as f32;
            self.momentary_max = self.momentary_max.max(self.momentary);
            // the gating blocks: 400 ms, every 100 ms (75 % overlap)
            if let Some(bin) = bin(m) {
                self.integrated_hist[bin].0 += 1;
                self.integrated_hist[bin].1 += z;
            }
            self.integrated = integrated(&self.integrated_hist) as f32;
        }
        if self.count >= BLOCKS {
            let s = lufs(mean(BLOCKS));
            self.short_term = s as f32;
            self.short_term_max = self.short_term_max.max(self.short_term);
            if let Some(bin) = bin(s) {
                self.range_hist[bin] += 1;
            }
            self.range = range(&self.range_hist) as f32;
        } else if self.count >= 4 {
            // (before 3 s: what there is)
            self.short_term = lufs(mean(self.count.min(BLOCKS))) as f32;
        }
    }
}

/// The histogram's step for a loudness at or above the absolute gate.
fn bin(lufs: f64) -> Option<usize> {
    if !lufs.is_finite() || lufs < LOW {
        return None;
    }
    Some((((lufs - LOW) / UNIT) as usize).min(STEPS - 1))
}

fn centre(bin: usize) -> f64 {
    LOW + (bin as f64 + 0.5) * UNIT
}

/// BS.1770's integrated loudness: the blocks above the absolute gate, and
/// of them those above 10 LU below their mean (exact: each step keeps its
/// blocks' sum).
fn integrated(hist: &[(u32, f64)]) -> f64 {
    let (mut sum, mut count) = (0.0, 0u64);
    for (n, z) in hist {
        sum += z;
        count += *n as u64;
    }
    if count == 0 {
        return f64::NEG_INFINITY;
    }
    let gate = -0.691 + 10.0 * (sum / count as f64).log10() - 10.0;
    let first = bin(gate).unwrap_or(0);
    let (mut sum, mut count) = (0.0, 0u64);
    for (n, z) in &hist[first..] {
        sum += z;
        count += *n as u64;
    }
    if count == 0 {
        return f64::NEG_INFINITY;
    }
    -0.691 + 10.0 * (sum / count as f64).log10()
}

/// The loudness of the blocks above the relative gate (`relative` LU below
/// the mean of all, which are above the absolute gate), and the first bin
/// above that gate.
fn gated(hist: &[u32], relative: f64) -> (f64, usize) {
    let power = |b: usize| 10f64.powf((centre(b) + 0.691) / 10.0);
    let (mut sum, mut count) = (0.0, 0u64);
    for (b, n) in hist.iter().enumerate() {
        sum += power(b) * *n as f64;
        count += *n as u64;
    }
    if count == 0 {
        return (f64::NEG_INFINITY, 0);
    }
    let gate = -0.691 + 10.0 * (sum / count as f64).log10() + relative;
    let first = bin(gate).unwrap_or(0);
    let (mut sum, mut count) = (0.0, 0u64);
    for (b, n) in hist.iter().enumerate().skip(first) {
        sum += power(b) * *n as f64;
        count += *n as u64;
    }
    if count == 0 {
        return (f64::NEG_INFINITY, first);
    }
    (-0.691 + 10.0 * (sum / count as f64).log10(), first)
}

/// EBU Tech 3342: the short-term values above -70 LUFS and above 20 LU
/// below their mean, from the 10th to the 95th percentile.
fn range(hist: &[u32]) -> f64 {
    let (_, first) = gated(hist, -20.0);
    let total: u64 = hist[first..].iter().map(|n| *n as u64).sum();
    if total == 0 {
        return 0.0;
    }
    let at = |share: f64| {
        let want = (total as f64 * share).floor() as u64;
        let mut seen = 0u64;
        for (b, n) in hist.iter().enumerate().skip(first) {
            seen += *n as u64;
            if seen > want {
                return centre(b);
            }
        }
        centre(STEPS - 1)
    };
    (at(0.95) - at(0.10)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f64, dbfs: f64, seconds: f64, both: bool) -> Vec<f32> {
        let amp = 10f64.powf(dbfs / 20.0);
        (0..(rate as f64 * seconds) as usize)
            .flat_map(|i| {
                let v = ((2.0 * PI * hz * i as f64 / rate as f64).sin() * amp) as f32;
                [v, if both { v } else { 0.0 }]
            })
            .collect()
    }

    #[test]
    fn a_1khz_sine_at_minus_18_dbfs_reads_minus_18_lufs() {
        // EBU Tech 3341's reference: a stereo 1 kHz sine at the level in
        // dBFS reads that many LUFS (the K-weighting adds ~0.7 dB at 1 kHz,
        // the -0.691 takes it away)
        for rate in [44_100, 48_000, 96_000] {
            let mut m = Meter::new(rate);
            let sound = tone(rate, 1000.0, -18.0, 5.0, true);
            for chunk in sound.chunks(1600) {
                m.push(chunk);
            }
            for (name, v) in [
                ("momentary", m.momentary),
                ("short-term", m.short_term),
                ("integrated", m.integrated),
            ] {
                assert!((v + 18.0).abs() < 0.1, "{rate} Hz: {name} {v}");
            }
            assert!(m.range < 0.2, "{}", m.range);
            assert!((m.sample_peak + 18.0).abs() < 0.05, "{}", m.sample_peak);
            assert!(
                m.true_peak >= m.sample_peak && m.true_peak < -17.8,
                "{}",
                m.true_peak
            );
        }
    }

    #[test]
    fn one_channel_is_3_db_quieter_and_the_gate_keeps_silence_out() {
        let mut m = Meter::new(48_000);
        m.push(&tone(48_000, 1000.0, -18.0, 4.0, false));
        assert!((m.integrated + 21.01).abs() < 0.1, "{}", m.integrated);
        // ten seconds of silence after it: only the blocks that still hold
        // some of the tone count (the absolute gate) - not the silence
        m.push(&vec![0.0; 48_000 * 2 * 10]);
        assert!((m.integrated + 21.01).abs() < 0.3, "{}", m.integrated);
        assert_eq!(m.momentary, f32::NEG_INFINITY);
    }

    #[test]
    fn a_loud_and_a_quiet_part_make_a_range() {
        // Tech 3342's idea: 20 s at -20 and 20 s at -30 dBFS: a range of 10 LU
        let mut m = Meter::new(48_000);
        m.push(&tone(48_000, 1000.0, -20.0, 20.0, true));
        m.push(&tone(48_000, 1000.0, -30.0, 20.0, true));
        assert!((m.range - 10.0).abs() < 0.6, "{}", m.range);
        m.reset();
        assert_eq!(m.integrated, f32::NEG_INFINITY);
        assert_eq!(m.seconds, 0.0);
    }

    #[test]
    fn an_intersample_peak_is_found() {
        // a sine at a quarter of the rate, sampled at 45 degrees: every
        // sample reads 0.707 of the true peak (+3 dB between them)
        let rate = 48_000;
        let sound: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * 0.25 * i as f32
                    + std::f32::consts::FRAC_PI_4)
                    .sin()
                    * 0.5;
                [v, v]
            })
            .collect();
        let mut m = Meter::new(rate);
        m.push(&sound);
        let sample = 20.0 * (0.5f32 * std::f32::consts::FRAC_1_SQRT_2).log10();
        assert!((m.sample_peak - sample).abs() < 0.1, "{}", m.sample_peak);
        assert!(
            m.true_peak > m.sample_peak + 2.0,
            "{} vs {}",
            m.true_peak,
            m.sample_peak
        );
    }
}

/// EBU Tech 3341 (loudness metering: the EBU mode) and Tech 3342 (loudness
/// range): their synthesised test signals and what a meter must read.
#[cfg(test)]
mod ebu {
    use super::*;

    /// A stereo 1 kHz sine at `dbfs` (peak) for `secs`, phase going on.
    fn sine(m: &mut Meter, rate: u32, dbfs: f64, secs: f64, at: &mut u64) {
        let amp = 10f64.powf(dbfs / 20.0);
        let frames = (secs * rate as f64).round() as u64;
        let mut chunk = Vec::with_capacity(9600);
        for i in 0..frames {
            let v = ((2.0 * PI * 1000.0 * (*at + i) as f64 / rate as f64).sin() * amp) as f32;
            chunk.push(v);
            chunk.push(v);
            if chunk.len() == 9600 {
                m.push(&chunk);
                chunk.clear();
            }
        }
        m.push(&chunk);
        *at += frames;
    }

    fn programme(parts: &[(f64, f64)]) -> Meter {
        let rate = 48_000;
        let mut m = Meter::new(rate);
        let mut at = 0;
        for (dbfs, secs) in parts {
            sine(&mut m, rate, *dbfs, *secs, &mut at);
        }
        m
    }

    #[test]
    fn tech_3341_cases_1_and_2_steady_sines() {
        for level in [-23.0, -33.0] {
            let m = programme(&[(level, 20.0)]);
            for (name, v) in [("M", m.momentary), ("S", m.short_term), ("I", m.integrated)] {
                assert!((v as f64 - level).abs() <= 0.1, "{level}: {name} {v}");
            }
        }
    }

    #[test]
    fn tech_3341_cases_3_to_5_the_gates() {
        // 3: the quieter parts are under the relative gate
        let m = programme(&[(-36.0, 10.0), (-23.0, 60.0), (-36.0, 10.0)]);
        assert!(
            (m.integrated + 23.0).abs() <= 0.1,
            "case 3: {}",
            m.integrated
        );
        // 4: and the quietest under the absolute one
        let m = programme(&[
            (-72.0, 10.0),
            (-36.0, 10.0),
            (-23.0, 60.0),
            (-36.0, 10.0),
            (-72.0, 10.0),
        ]);
        assert!(
            (m.integrated + 23.0).abs() <= 0.1,
            "case 4: {}",
            m.integrated
        );
        // 5: louder and quieter around the target average to it
        let m = programme(&[(-26.0, 20.0), (-20.0, 20.1), (-26.0, 20.0)]);
        assert!(
            (m.integrated + 23.0).abs() <= 0.1,
            "case 5: {}",
            m.integrated
        );
    }

    #[test]
    fn tech_3341_true_peak_cases_15_to_18() {
        // sines at -6 dBFS whose samples fall beside their peaks: the true
        // peak reads -6 dBTP (+0.2 / -0.4 dB)
        let rate = 48_000u32;
        for (case, divisor, phase_deg) in [
            (15, 4.0, 0.0),
            (16, 4.0, 45.0),
            (17, 6.0, 60.0),
            (18, 8.0, 67.5),
        ] {
            let hz = rate as f64 / divisor;
            let amp = 10f64.powf(-6.0 / 20.0);
            let phase = phase_deg * PI / 180.0;
            // (faded in over 50 ms: an abrupt start rings - Gibbs - above
            // the sine's own peak)
            let sound: Vec<f32> = (0..rate as usize)
                .flat_map(|i| {
                    let fade = 0.5 - 0.5 * (PI * (i as f64 / 2400.0).min(1.0)).cos();
                    let v = ((2.0 * PI * hz * i as f64 / rate as f64 + phase).sin() * amp * fade)
                        as f32;
                    [v, v]
                })
                .collect();
            let mut m = Meter::new(rate);
            m.push(&sound);
            assert!(
                (-6.4..=-5.8).contains(&m.true_peak),
                "case {case}: {} dBTP (samples peak at {} dBFS)",
                m.true_peak,
                m.sample_peak
            );
        }
    }

    #[test]
    fn tech_3342_loudness_range_cases_1_to_4() {
        for (case, parts, want) in [
            (1, vec![(-20.0, 20.0), (-30.0, 20.0)], 10.0),
            (2, vec![(-20.0, 20.0), (-15.0, 20.0)], 5.0),
            (3, vec![(-40.0, 20.0), (-20.0, 20.0)], 20.0),
            (
                4,
                vec![
                    (-50.0, 20.0),
                    (-35.0, 20.0),
                    (-20.0, 20.0),
                    (-35.0, 20.0),
                    (-50.0, 20.0),
                ],
                15.0,
            ),
        ] {
            let m = programme(&parts);
            assert!(
                (m.range - want).abs() <= 1.0,
                "case {case}: LRA {} LU, {want} wanted",
                m.range
            );
        }
    }
}
