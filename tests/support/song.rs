//! A little song of four sections for the faces' tests (review 18's): a
//! verse, a chorus with a melody panned to and fro, a quiet break, a louder
//! chorus - a kick at 120 a minute, a bass, a pad, hi-hats; nothing gated,
//! nothing that flashes. Thirteen seconds, stereo, 48 kHz. Also its bass
//! alone, its treble alone, and silence.
//!
//! And other music ([`other`], review 19's): a song of five parts at 100
//! beats a minute - an arpeggio alone, a groove, a build with a snare roll
//! and a noise sweep, a loud pumping drop, a tail - seventeen seconds;
//! drum and bass at 174; speech; loud noise with a kick; a steady tone.
#![allow(dead_code)]

use std::f64::consts::TAU;

struct Rng(u64);
impl Rng {
    fn n(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

/// `kind`: "song" (verse to 3 s, chorus to 6 s, break to 7.5 s, the louder
/// chorus after it), "bass", "treble" or "silence".
pub fn song(kind: &str) -> Vec<f32> {
    let n = 48_000 * 13;
    let mut r = Rng(12345);
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f64 / 48_000.0;
        let (l, rr) = match kind {
            "silence" => (0.0, 0.0),
            "bass" => {
                let tk = t % 0.5;
                let v =
                    (TAU * (45.0 + 60.0 * (-tk * 30.0).exp()) * tk).sin() * (-tk * 6.0).exp() * 0.9;
                (v, v)
            }
            "treble" => {
                let th = t % 0.25;
                let note = [4186.0, 5274.0, 6272.0, 7040.0][((t * 4.0) as usize) % 4];
                let v = (TAU * note * t).sin() * (-th * 10.0).exp() * 0.6
                    + r.n() * (-th * 60.0).exp() * 0.2;
                (v, v * 0.3)
            }
            _ => {
                let section = if t < 3.0 {
                    0
                } else if t < 6.0 {
                    1
                } else if t < 7.5 {
                    2
                } else {
                    3
                };
                let q = 0.5;
                let tk = t % q;
                let kick =
                    (TAU * (48.0 + 110.0 * (-tk * 30.0).exp()) * tk).sin() * (-tk * 9.0).exp();
                let ts = (t + q) % (2.0 * q);
                let snare = r.n() * (-ts * 22.0).exp() * 0.5;
                let th = t % (q / 2.0);
                let hat = r.n() * (-th * 90.0).exp() * 0.25;
                let (root, chord): (f64, [f64; 3]) = match section {
                    0 => (55.0, [220.0, 261.6, 329.6]),
                    1 => (43.65, [174.6, 220.0, 261.6]),
                    2 => (55.0, [440.0, 523.3, 659.3]),
                    _ => (65.4, [261.6, 329.6, 392.0]),
                };
                let bass = (TAU * root * t).sin() * 0.35 + (TAU * root * 2.0 * t).sin() * 0.12;
                let pad: f64 = chord
                    .iter()
                    .map(|f| (TAU * f * t).sin() + 0.4 * (TAU * f * 2.0 * t).sin())
                    .sum::<f64>()
                    * 0.07;
                let step = (t * 8.0) as usize;
                let mel = chord[step % 3] * [2.0, 4.0, 3.0, 4.0][step % 4];
                let tm = t % 0.125;
                let melody = (TAU * mel * t).sin() * (-tm * 14.0).exp() * 0.25;
                let pan = (t * 1.3).sin();
                match section {
                    0 => {
                        let v = kick * 0.7 + bass + pad + hat * 0.5;
                        (v, v)
                    }
                    1 => {
                        let v = kick * 0.8 + snare + bass + pad * 1.4 + hat;
                        (v + melody * (1.0 + pan), v + melody * (1.0 - pan))
                    }
                    2 => (
                        pad * 0.5 + melody * 0.3 * (1.0 + pan),
                        pad * 0.5 + melody * 0.3 * (1.0 - pan),
                    ),
                    _ => {
                        let v = kick * 0.9 + snare * 1.2 + bass + pad * 1.6 + hat;
                        (v + melody * (1.0 - pan), v * 0.8 + melody * (1.0 + pan))
                    }
                }
            }
        };
        out.push((l as f32).clamp(-1.0, 1.0));
        out.push((rr as f32).clamp(-1.0, 1.0));
    }
    out
}

/// `kind`: "groove" (the song of five parts), "dnb", "voice", "loud" or
/// "sine1k"; seventeen seconds each.
// (its numbers as the review's probe has them: the same music, to the sample)
#[allow(clippy::approx_constant)]
pub fn other(kind: &str) -> Vec<f32> {
    let n = 48_000 * 17;
    let mut r = Rng(987_654_321);
    let mut out = Vec::with_capacity(n * 2);
    let mut lp = 0.0f64;
    let mut lp2 = 0.0f64;
    for i in 0..n {
        let t = i as f64 / 48_000.0;
        let saw = |f: f64| 2.0 * ((t * f).fract()) - 1.0;
        let (l, rr) = match kind {
            "sine1k" => {
                let v = (TAU * 1000.0 * t).sin() * 0.5;
                (v, v)
            }
            "voice" => {
                // syllables 4-5 a second, formants, pauses
                let syl = (t * 4.3).fract();
                let word = ((t * 0.9).fract() < 0.75) as i32 as f64;
                let env = (syl * 3.14159).sin().max(0.0).powf(0.6) * word;
                let f0 = 120.0 + 25.0 * (t * 1.7).sin();
                let v: f64 = (1..18)
                    .map(|h| {
                        let f = f0 * h as f64;
                        let form = (-(f - 700.0).powi(2) / 90000.0).exp()
                            + 0.6 * (-(f - 1800.0).powi(2) / 250000.0).exp();
                        (TAU * f * t).sin() * form
                    })
                    .sum::<f64>()
                    * 0.22
                    * env;
                (v, v)
            }
            "loud" => {
                lp += 0.08 * (r.n() - lp);
                let tk = t % 0.4615;
                let kick =
                    (TAU * (50.0 + 120.0 * (-tk * 28.0).exp()) * tk).sin() * (-tk * 7.0).exp();
                let v =
                    (lp * 3.0 + kick * 0.9 + saw(110.0) * 0.3 + saw(164.8) * 0.25 + r.n() * 0.15)
                        .tanh();
                (v, (v * 0.9 + r.n() * 0.1).tanh())
            }
            "dnb" => {
                let beat = 60.0 / 174.0;
                let bar = (t / beat) % 4.0;
                let hit = |at: f64| {
                    let d = bar - at;
                    if d >= 0.0 { d * beat } else { 9.0 }
                };
                let k = hit(0.0).min(hit(2.5));
                let kick = (TAU * (55.0 + 150.0 * (-k * 40.0).exp()) * k).sin() * (-k * 12.0).exp();
                let s = hit(1.0).min(hit(3.0));
                let snare = (r.n() * 0.7 + (TAU * 190.0 * s).sin() * 0.5) * (-s * 18.0).exp();
                let th = (t / beat * 2.0).fract() * beat / 2.0;
                let hat = r.n() * (-th * 110.0).exp() * 0.3;
                let reese = (saw(55.0) + saw(55.6) + saw(54.5)) * 0.2;
                lp2 += (0.02 + 0.05 * (0.5 + 0.5 * (t * 0.7).sin())) * (reese - lp2);
                let v = kick * 0.9 + snare * 0.8 + hat + lp2 * 2.2;
                (v + hat * 0.3, v - hat * 0.3)
            }
            _ => {
                let beat = 0.6;
                let b = t / beat;
                let sec = if t < 3.0 {
                    0
                } else if t < 7.0 {
                    1
                } else if t < 9.5 {
                    2
                } else if t < 14.0 {
                    3
                } else {
                    4
                };
                let arp_notes = [293.7, 370.0, 440.0, 554.4, 587.3, 554.4, 440.0, 370.0];
                let st = (b * 4.0) as usize;
                let ta = (b * 4.0).fract() * beat / 4.0;
                let note = arp_notes[st % 8];
                let arp = ((TAU * note * t).sin()
                    + 0.5 * (TAU * note * 2.0 * t).sin()
                    + 0.25 * (TAU * note * 3.0 * t).sin())
                    * (-ta * 9.0).exp()
                    * 0.3;
                let pan = if st.is_multiple_of(2) { 0.85 } else { -0.85 };
                let tk = (b % 2.0) * beat;
                let kick =
                    (TAU * (46.0 + 130.0 * (-tk * 32.0).exp()) * tk).sin() * (-tk * 6.0).exp();
                let ts = ((b + 1.0) % 2.0) * beat;
                let snare =
                    (r.n() * 0.8 + (TAU * 200.0 * ts).sin() * 0.4) * (-ts * 16.0).exp() * 0.6;
                let th = (b * 4.0).fract() * beat / 4.0;
                let hat = r.n() * (-th * 120.0).exp() * if st % 4 == 2 { 0.35 } else { 0.18 };
                let bass_pat = [73.4, 0.0, 73.4, 87.3, 0.0, 110.0, 0.0, 65.4];
                let bn = bass_pat[(b * 2.0) as usize % 8];
                let tb = (b * 2.0).fract() * beat / 2.0;
                let bass = if bn > 0.0 {
                    ((TAU * bn * t).sin() + 0.3 * saw(bn)) * (-tb * 4.0).exp() * 0.45
                } else {
                    0.0
                };
                let off = ((b + 0.5) % 1.0) * beat;
                let stab = (saw(293.7) + saw(370.0) + saw(440.0)) * (-off * 14.0).exp() * 0.1;
                match sec {
                    0 => (arp * (1.0 + pan), arp * (1.0 - pan)),
                    1 => {
                        let v = kick * 0.8 + snare + hat + bass;
                        (
                            v + stab + arp * 0.4 * (1.0 + pan),
                            v + stab * 0.6 + arp * 0.4 * (1.0 - pan),
                        )
                    }
                    2 => {
                        let p = (t - 7.0) / 2.5;
                        let roll_rate = if p < 0.5 {
                            2.0
                        } else if p < 0.8 {
                            4.0
                        } else {
                            8.0
                        };
                        let tr = (b * roll_rate).fract() * beat / roll_rate;
                        let roll = r.n() * (-tr * 30.0).exp() * (0.3 + 0.5 * p);
                        lp += (0.02 + 0.5 * p * p) * (r.n() - lp);
                        let sweep = lp * (0.2 + 0.9 * p);
                        let v = roll + sweep + bass * (1.0 - p);
                        (v + arp * 0.3, v * 0.9 - arp * 0.3)
                    }
                    3 => {
                        let pump = 1.0 - (-(b.fract()) * 5.0).exp().mul_add(0.85, 0.0);
                        let chord =
                            (saw(146.8) + saw(220.0) + saw(293.7) + saw(370.0) + saw(440.9))
                                * 0.14
                                * pump;
                        let sub = (TAU * 55.0 * t).sin() * 0.5 * pump;
                        let tc = t - 9.5;
                        let crash = r.n() * (-tc * 1.2).exp() * 0.3;
                        let kick4 = {
                            let tk = b.fract() * beat;
                            (TAU * (46.0 + 130.0 * (-tk * 32.0).exp()) * tk).sin()
                                * (-tk * 6.0).exp()
                        };
                        let v = kick4 * 0.95 + snare * 1.2 + hat * 1.5 + sub + crash;
                        (
                            (v + chord * 1.3).tanh(),
                            (v * 0.9 + chord * 1.3 * (0.6 + 0.4 * (t * 2.0).sin())).tanh(),
                        )
                    }
                    _ => {
                        let d = (-(t - 14.0) * 1.5).exp();
                        let v = (saw(146.8) + saw(220.0) + saw(293.7)) * 0.12 * d;
                        (v + r.n() * 0.02 * d, v - r.n() * 0.02 * d)
                    }
                }
            }
        };
        out.push((l as f32).clamp(-1.0, 1.0));
        out.push((rr as f32).clamp(-1.0, 1.0));
    }
    out
}
