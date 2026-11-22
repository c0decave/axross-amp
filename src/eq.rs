//! The ten-band equalizer: the bands, presets and settings of axross's
//! (`core/audio_eq.py`, `~/.config/axross/audio.json`) and its curve -
//! the gains interpolated between the bands over the log of the frequency,
//! flat beyond them - by biquad filters (RBJ's audio EQ cookbook) whose
//! gains are fitted to that curve, run on the decoded samples.

/// Centre frequencies of the ten bands (Hz) - as axross's.
pub const BANDS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
/// The most a band boosts or cuts (dB).
pub const MAX_GAIN_DB: f32 = 12.0;
/// The name of gains that match no preset.
pub const CUSTOM: &str = "Custom";

/// The presets, in axross's order and with its values.
pub const PRESETS: [(&str, [f32; 10]); 6] = [
    ("Flat", [0.0; 10]),
    (
        "Bass boost",
        [8.0, 7.0, 5.0, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ),
    (
        "Treble boost",
        [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 7.0, 8.0],
    ),
    (
        "Vocal",
        [-3.0, -2.0, -1.0, 1.0, 4.0, 5.0, 4.0, 2.0, 0.0, -1.0],
    ),
    (
        "Loudness",
        [6.0, 5.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.0, 4.0, 5.0],
    ),
    (
        "Rock",
        [5.0, 4.0, 2.0, -1.0, -2.0, -1.0, 2.0, 4.0, 5.0, 5.0],
    ),
];

/// The preset whose gains these are, or [`CUSTOM`].
pub fn preset_of(gains: &[f32; 10]) -> &'static str {
    PRESETS
        .iter()
        .find(|(_, values)| values.iter().zip(gains).all(|(a, b)| (a - b).abs() < 0.05))
        .map(|(name, _)| *name)
        .unwrap_or(CUSTOM)
}

/// The gains of a preset by name.
pub fn preset(name: &str) -> Option<[f32; 10]> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, g)| *g)
}

/// Gains clamped to ±[`MAX_GAIN_DB`]; anything not a number is 0.
pub fn clamp(gains: [f32; 10]) -> [f32; 10] {
    gains.map(|g| {
        if g.is_finite() {
            g.clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
        } else {
            0.0
        }
    })
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// One of the filters the curve is made of (RBJ's audio EQ cookbook).
#[derive(Clone, Copy)]
enum Kind {
    LowShelf,
    Peak,
    HighShelf,
}

/// The filters: a low shelf below the first band, a peak at each band
/// between and one halfway between each two, a high shelf above the last -
/// their gains fitted to axross's curve ([`target_db`]).
const SHELF_LOW: f64 = 44.0;
const SHELF_HIGH: f64 = 11_314.0;
const PEAK_Q: f64 = 2.0;

fn layout() -> Vec<(Kind, f64)> {
    let mut out = vec![(Kind::LowShelf, SHELF_LOW)];
    for f in &BANDS[1..9] {
        out.push((Kind::Peak, *f as f64));
    }
    out.push((Kind::HighShelf, SHELF_HIGH));
    for pair in BANDS.windows(2) {
        out.push((Kind::Peak, (pair[0] as f64 * pair[1] as f64).sqrt()));
    }
    out
}

/// The coefficients (b0, b1, b2, a1, a2), normalised, in f64.
fn design(kind: Kind, freq: f64, gain_db: f64, rate: f64) -> [f64; 5] {
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = std::f64::consts::TAU * freq / rate;
    let (sin, cos) = w0.sin_cos();
    let (b, d) = match kind {
        Kind::Peak => {
            let alpha = sin / (2.0 * PEAK_Q);
            (
                [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a],
                [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
            )
        }
        Kind::LowShelf | Kind::HighShelf => {
            // (shelf slope 1)
            let alpha = sin / 2.0 * 2f64.sqrt();
            let sa = 2.0 * a.sqrt() * alpha;
            let (ap, am) = (a + 1.0, a - 1.0);
            if matches!(kind, Kind::LowShelf) {
                (
                    [
                        a * (ap - am * cos + sa),
                        2.0 * a * (am - ap * cos),
                        a * (ap - am * cos - sa),
                    ],
                    [
                        ap + am * cos + sa,
                        -2.0 * (am + ap * cos),
                        ap + am * cos - sa,
                    ],
                )
            } else {
                (
                    [
                        a * (ap + am * cos + sa),
                        -2.0 * a * (am + ap * cos),
                        a * (ap + am * cos - sa),
                    ],
                    [
                        ap - am * cos + sa,
                        2.0 * (am - ap * cos),
                        ap - am * cos - sa,
                    ],
                )
            }
        }
    };
    [
        b[0] / d[0],
        b[1] / d[0],
        b[2] / d[0],
        d[1] / d[0],
        d[2] / d[0],
    ]
}

/// A filter's response at `freq` (dB).
fn response_db(c: &[f64; 5], freq: f64, rate: f64) -> f64 {
    let w = std::f64::consts::TAU * freq / rate;
    let (s1, c1) = w.sin_cos();
    let (s2, c2) = (2.0 * w).sin_cos();
    // H = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2), z^-1 = e^-jw
    let (nr, ni) = (c[0] + c[1] * c1 + c[2] * c2, -(c[1] * s1 + c[2] * s2));
    let (dr, di) = (1.0 + c[3] * c1 + c[4] * c2, -(c[3] * s1 + c[4] * s2));
    10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
}

/// axross's equalizer curve (core/audio_eq.py `_gain_curve`): the band
/// gains interpolated over the log of the frequency, flat beyond the outer
/// bands (dB).
pub fn target_db(gains: &[f32; 10], freq: f64) -> f64 {
    let x = freq.max(BANDS[0] as f64).log10();
    let logs = BANDS.map(|b| (b as f64).log10());
    if x >= logs[9] {
        return gains[9] as f64;
    }
    let i = logs.windows(2).position(|w| x < w[1]).unwrap_or(8);
    let t = (x - logs[i]) / (logs[i + 1] - logs[i]);
    gains[i] as f64 + (gains[i + 1] as f64 - gains[i] as f64) * t
}

/// The frequencies the fit is made at (20 Hz to 20 kHz, or what the rate carries).
fn grid(rate: f64) -> Vec<f64> {
    let top = 20_000f64.min(rate * 0.47);
    let n = 240;
    (0..n)
        .map(|i| 20.0 * (top / 20.0).powf(i as f64 / (n - 1) as f64))
        .collect()
}

/// `m x = v` by Gaussian elimination (m square, regular).
fn solve(mut m: Vec<Vec<f64>>, mut v: Vec<f64>) -> Vec<f64> {
    let n = v.len();
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))
            .unwrap_or(col);
        m.swap(col, pivot);
        v.swap(col, pivot);
        let d = m[col][col];
        if d.abs() < 1e-12 {
            continue;
        }
        for row in col + 1..n {
            let k = m[row][col] / d;
            if k != 0.0 {
                for j in col..n {
                    m[row][j] -= k * m[col][j];
                }
                v[row] -= k * v[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|j| m[row][j] * x[j]).sum();
        x[row] = if m[row][row].abs() < 1e-12 {
            0.0
        } else {
            (v[row] - s) / m[row][row]
        };
    }
    x
}

/// The filters' gains and the level (dB) whose sum follows axross's curve
/// for `gains` at `rate` - a least-squares fit over [`grid`], refined
/// (a peak's shape changes a little with its gain).
fn fit(gains: &[f32; 10], rate: f64) -> (Vec<(Kind, f64, f64)>, f64) {
    let filters: Vec<(Kind, f64)> = layout()
        .into_iter()
        .filter(|(_, f)| *f < rate * 0.45)
        .collect();
    let freqs = grid(rate);
    let target: Vec<f64> = freqs.iter().map(|f| target_db(gains, *f)).collect();
    // each filter's shape at a unit of gain, and the level (a column of 1)
    const UNIT: f64 = 6.0;
    let mut columns: Vec<Vec<f64>> = filters
        .iter()
        .map(|(kind, f0)| {
            let c = design(*kind, *f0, UNIT, rate);
            freqs
                .iter()
                .map(|f| response_db(&c, *f, rate) / UNIT)
                .collect()
        })
        .collect();
    columns.push(vec![1.0; freqs.len()]);
    let n = columns.len();
    let mut normal = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            normal[i][j] = columns[i]
                .iter()
                .zip(&columns[j])
                .map(|(a, b)| a * b)
                .sum::<f64>()
                + if i == j { 1e-3 } else { 0.0 };
        }
    }
    let project = |error: &[f64]| -> Vec<f64> {
        columns
            .iter()
            .map(|c| c.iter().zip(error).map(|(a, b)| a * b).sum())
            .collect()
    };
    let mut p = solve(normal.clone(), project(&target));
    for _ in 0..4 {
        let made: Vec<[f64; 5]> = filters
            .iter()
            .zip(&p)
            .map(|((kind, f0), g)| design(*kind, *f0, *g, rate))
            .collect();
        let error: Vec<f64> = freqs
            .iter()
            .zip(&target)
            .map(|(f, t)| t - p[n - 1] - made.iter().map(|c| response_db(c, *f, rate)).sum::<f64>())
            .collect();
        let step = solve(normal.clone(), project(&error));
        for (g, d) in p.iter_mut().zip(step) {
            *g = (*g + d).clamp(-30.0, 30.0);
        }
    }
    let level = p[n - 1];
    (
        filters
            .iter()
            .zip(&p)
            .map(|((kind, f0), g)| (*kind, *f0, *g))
            .collect(),
        level,
    )
}

/// Transposed direct form II state of one filter on one channel.
#[derive(Clone, Copy, Default)]
struct State {
    z1: f32,
    z2: f32,
}

/// The equalizer for one stream of interleaved stereo samples: axross's
/// curve (the gains between the bands interpolated, flat beyond them) by
/// shelves and peaks whose gains are fitted to it - within about half a
/// decibel of axross's for its presets, without its latency, and no level
/// taken off (the output's limiter holds a boost's peaks).
pub struct Equalizer {
    filters: Vec<Biquad>,
    states: Vec<[State; 2]>,
    level: f32,
    gains: [f32; 10],
    rate: f32,
    /// Whether it raises any frequency above 0 dB (its peaks then held by
    /// the output's limiter): cuts alone can, by the fit's ripple.
    raises: bool,
    pub enabled: bool,
}

impl Equalizer {
    pub fn new(rate: u32) -> Self {
        Equalizer {
            filters: Vec::new(),
            states: Vec::new(),
            level: 1.0,
            gains: [0.0; 10],
            rate: rate as f32,
            raises: false,
            enabled: true,
        }
    }

    /// New gains (dB); the filters are made anew, their state kept (the
    /// same filters at every setting: no click on a change).
    pub fn set_gains(&mut self, gains: [f32; 10]) {
        self.gains = clamp(gains);
        self.rebuild();
    }

    fn rebuild(&mut self) {
        if self.gains.iter().all(|g| g.abs() < 0.01) {
            // flat: no filter at all (the samples bit for bit)
            self.filters.clear();
            self.states.clear();
            self.level = 1.0;
            self.raises = false;
            return;
        }
        let (filters, level) = fit(&self.gains, self.rate as f64);
        self.filters = filters
            .into_iter()
            .map(|(kind, f0, g)| {
                let c = design(kind, f0, g, self.rate as f64);
                Biquad {
                    b0: c[0] as f32,
                    b1: c[1] as f32,
                    b2: c[2] as f32,
                    a1: c[3] as f32,
                    a2: c[4] as f32,
                }
            })
            .collect();
        self.states
            .resize(self.filters.len(), [State::default(); 2]);
        self.level = 10f64.powf(level / 20.0) as f32;
        let rate = self.rate as f64;
        self.raises = grid(rate)
            .into_iter()
            .chain([rate * 0.49])
            .any(|f| self.response_db(f) > 0.0);
    }

    /// Whether any frequency comes out louder than it went in (review 14:
    /// a setting of cuts only raised some by up to 0.9 dB, and with no
    /// band above 0 the output's limiter held nothing - a master at full
    /// scale was clipped).
    pub fn raises(&self) -> bool {
        self.enabled && self.raises
    }

    /// The equalizer's response at `freq` (dB), as set now.
    pub fn response_db(&self, freq: f64) -> f64 {
        if self.filters.is_empty() {
            return 0.0;
        }
        let rate = self.rate as f64;
        20.0 * (self.level as f64).log10()
            + self
                .filters
                .iter()
                .map(|b| {
                    let c = [b.b0, b.b1, b.b2, b.a1, b.a2].map(|v| v as f64);
                    response_db(&c, freq, rate)
                })
                .sum::<f64>()
    }

    /// Filter interleaved stereo samples in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.enabled || self.filters.is_empty() {
            return;
        }
        for frame in samples.as_chunks_mut::<2>().0 {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * self.level;
                for (filter, state) in self.filters.iter().zip(self.states.iter_mut()) {
                    let s = &mut state[channel];
                    let y = filter.b0 * x + s.z1;
                    s.z1 = filter.b1 * x - filter.a1 * y + s.z2;
                    s.z2 = filter.b2 * x - filter.a2 * y;
                    x = y;
                }
                *sample = x;
            }
        }
        // (a sample that was no number - one is enough - kept the filters at
        // NaN for good: they start afresh)
        if self
            .states
            .iter()
            .any(|s| !(s[0].z1 + s[0].z2 + s[1].z1 + s[1].z2).is_finite())
        {
            self.reset();
            for s in samples.iter_mut() {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
        }
    }

    /// Forget the filters' memory (after a seek or a new track).
    pub fn reset(&mut self) {
        for state in &mut self.states {
            *state = [State::default(); 2];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn tone(freq: f32, rate: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / rate).sin() * 0.25;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn presets_are_found_by_their_gains() {
        assert_eq!(preset_of(&[0.0; 10]), "Flat");
        assert_eq!(preset_of(&PRESETS[1].1), "Bass boost");
        assert_eq!(preset_of(&[1.0; 10]), CUSTOM);
        assert_eq!(
            clamp([99.0, f32::NAN, -99.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])[..3],
            [12.0, 0.0, -12.0]
        );
    }

    #[test]
    fn a_boosted_band_raises_its_tone_and_leaves_others() {
        let rate = 48_000.0;
        let mut eq = Equalizer::new(48_000);
        let mut gains = [0.0; 10];
        gains[5] = 12.0; // 1 kHz
        eq.set_gains(gains);
        let mut at_band = tone(1000.0, rate, 48_000);
        let before = rms(&at_band);
        eq.process(&mut at_band);
        eq.reset();
        let mut far = tone(60.0, rate, 48_000);
        let far_before = rms(&far);
        eq.process(&mut far);
        // +12 dB at the band, as axross; far away nothing (no level taken
        // off for the boost: the output's limiter holds its peaks)
        let gain_band = 20.0 * (rms(&at_band[9600..]) / before).log10();
        let gain_far = 20.0 * (rms(&far[9600..]) / far_before).log10();
        assert!((gain_band - 12.0).abs() < 0.6, "band {gain_band}");
        assert!(gain_far.abs() < 0.6, "far {gain_far}");
    }

    /// The curve is axross's (core/audio_eq.py): the gains interpolated
    /// between the bands over the log of the frequency, flat beyond them,
    /// no level taken off - review 13 (M8): Bass boost lowered everything
    /// above 1 kHz by about 8 dB, Treble boost gave -7.4 dB at 20 kHz, all
    /// bands at +6 dB rippled by 1.5 dB.
    #[test]
    fn the_curve_is_axross_s_within_half_a_decibel() {
        let freqs: Vec<f64> = (0..200)
            .map(|i| 20.0 * 1000f64.powf(i as f64 / 199.0))
            .collect();
        let mut cases: Vec<(String, [f32; 10])> =
            PRESETS.iter().map(|(n, g)| (n.to_string(), *g)).collect();
        cases.push(("all +6".into(), [6.0; 10]));
        cases.push(("all -12".into(), [-12.0; 10]));
        cases.push(("500 Hz +12".into(), {
            let mut g = [0.0; 10];
            g[4] = 12.0;
            g
        }));
        for rate in [44_100u32, 48_000, 96_000] {
            for (name, gains) in &cases {
                let mut eq = Equalizer::new(rate);
                eq.set_gains(*gains);
                let worst = freqs
                    .iter()
                    .map(|f| (eq.response_db(*f) - target_db(gains, *f)).abs())
                    .fold(0.0, f64::max);
                assert!(worst < 0.5, "{name} at {rate}: {worst:.2} dB off");
            }
            let mut bass = Equalizer::new(rate);
            bass.set_gains(PRESETS[1].1);
            for f in [1000.0, 4000.0, 20_000.0] {
                assert!(bass.response_db(f).abs() < 0.5, "bass boost at {f}");
            }
            let mut treble = Equalizer::new(rate);
            treble.set_gains(PRESETS[2].1);
            assert!((treble.response_db(20_000.0) - 8.0).abs() < 0.5);
        }
        // any gains: close, as far as octave bands can follow
        let mut seed = 7u32;
        for _ in 0..40 {
            let gains = [0; 10].map(|_: i32| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 24.0 - 12.0
            });
            let mut eq = Equalizer::new(48_000);
            eq.set_gains(gains);
            let errors: Vec<f64> = freqs
                .iter()
                .map(|f| (eq.response_db(*f) - target_db(&gains, *f)).abs())
                .collect();
            let mean = errors.iter().sum::<f64>() / errors.len() as f64;
            let worst = errors.iter().cloned().fold(0.0, f64::max);
            assert!(
                mean < 0.5 && worst < 3.0,
                "{gains:?}: mean {mean:.2}, worst {worst:.2}"
            );
        }
    }

    /// The response measured on tones is the one computed.
    #[test]
    fn tones_are_raised_as_the_curve_says() {
        let rate = 44_100.0;
        let mut eq = Equalizer::new(44_100);
        eq.set_gains(PRESETS[4].1); // Loudness
        for f in [40.0, 300.0, 1500.0, 9000.0, 18_000.0] {
            eq.reset();
            let mut t = tone(f, rate, 44_100);
            let before = rms(&t[8820..]);
            eq.process(&mut t);
            let gain = 20.0 * (rms(&t[8820..]) / before).log10();
            let want = target_db(&PRESETS[4].1, f as f64);
            assert!(
                (gain as f64 - want).abs() < 0.6,
                "{f} Hz: {gain} for {want}"
            );
        }
    }

    /// One NaN or infinity does not keep the filters at NaN (review 13, M1).
    #[test]
    fn a_nan_is_forgotten() {
        let mut eq = Equalizer::new(48_000);
        eq.set_gains(PRESETS[1].1);
        let mut t = tone(100.0, 48_000.0, 4800);
        t[100] = f32::NAN;
        eq.process(&mut t);
        let mut after = tone(100.0, 48_000.0, 4800);
        eq.process(&mut after);
        assert!(after.iter().all(|s| s.is_finite()));
        assert!(rms(&after) > 0.1);
    }

    #[test]
    fn flat_changes_nothing() {
        let mut eq = Equalizer::new(44_100);
        let mut samples = tone(440.0, 44_100.0, 1000);
        let copy = samples.clone();
        eq.process(&mut samples);
        assert_eq!(samples, copy);
    }
}
