//! The ten-band equalizer: the bands, presets and settings of axross's
//! (`core/audio_eq.py`, `~/.config/axross/audio.json`), as peaking biquad
//! filters (RBJ's audio EQ cookbook) run on the decoded samples.

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

impl Biquad {
    /// A peaking filter of `gain_db` at `freq` (RBJ cookbook), Q of about an
    /// octave's width.
    fn peaking(freq: f32, gain_db: f32, rate: f32) -> Option<Self> {
        if gain_db.abs() < 0.01 || freq >= rate * 0.45 {
            return None; // flat, or above what this rate carries
        }
        let q = std::f32::consts::SQRT_2;
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Some(Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        })
    }
}

/// Transposed direct form II state of one filter on one channel.
#[derive(Clone, Copy, Default)]
struct State {
    z1: f32,
    z2: f32,
}

/// The equalizer for one stream of interleaved stereo samples.
pub struct Equalizer {
    filters: Vec<Biquad>,
    states: Vec<[State; 2]>,
    preamp: f32,
    gains: [f32; 10],
    rate: f32,
    pub enabled: bool,
}

impl Equalizer {
    pub fn new(rate: u32) -> Self {
        Equalizer {
            filters: Vec::new(),
            states: Vec::new(),
            preamp: 1.0,
            gains: [0.0; 10],
            rate: rate as f32,
            enabled: true,
        }
    }

    /// New gains (dB); the filters are made anew, their state kept where a
    /// band stays (no click on a small change).
    pub fn set_gains(&mut self, gains: [f32; 10]) {
        self.gains = clamp(gains);
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.filters = BANDS
            .iter()
            .zip(self.gains)
            .filter_map(|(f, g)| Biquad::peaking(*f, g, self.rate))
            .collect();
        self.states
            .resize(self.filters.len(), [State::default(); 2]);
        // Headroom: as much as the largest boost, so a boost does not clip.
        let boost = self.gains.iter().cloned().fold(0.0f32, f32::max);
        self.preamp = 10f32.powf(-boost / 20.0);
    }

    /// Filter interleaved stereo samples in place.
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.enabled || self.filters.is_empty() {
            return;
        }
        for frame in samples.as_chunks_mut::<2>().0 {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * self.preamp;
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
        // +12 dB at the band minus 12 dB of headroom: about level; far away: about -12 dB
        let gain_band = 20.0 * (rms(&at_band[9600..]) / before).log10();
        let gain_far = 20.0 * (rms(&far[9600..]) / far_before).log10();
        assert!(gain_band.abs() < 1.5, "band {gain_band}");
        assert!(gain_far < -9.0, "far {gain_far}");
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
