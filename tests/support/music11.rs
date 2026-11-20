//! Review 11's worst-case music (48 kHz stereo, interleaved) - its
//! harness's music11.rs, kept as it was (a test reads the faces through it).
#![allow(dead_code)]
use std::f64::consts::TAU;
pub const RATE: f64 = 48_000.0;

struct Rng(u64);
impl Rng {
    fn n(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

fn saw_chord(t: f64) -> f64 {
    [110.0, 164.8, 220.0, 329.6, 440.0, 880.0, 1760.0]
        .iter()
        .map(|f| 2.0 * ((t * f).fract()) - 1.0)
        .sum::<f64>()
        / 4.0
}

/// One part of `secs` seconds. kind: gate6 gate8 gate10 gateramp noise noisegate
/// sweep chirp8 beats kick8 antiphase silence_burst
pub fn part(kind: &str, secs: f64, seed: u64) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let mut r = Rng(seed | 1);
    let mut out = Vec::with_capacity(n * 2);
    let mut phase = 0.0f64;
    for i in 0..n {
        let t = i as f64 / RATE;
        let gate = |hz: f64| if (t * hz).fract() < 0.5 { 1.0 } else { 0.0 };
        let (l, rr) = match kind {
            "gate6" => {
                let v = saw_chord(t) * gate(6.0);
                (v, v)
            }
            "gate8" => {
                let v = saw_chord(t) * gate(8.0);
                (v, v)
            }
            "gate10" => {
                let v = saw_chord(t) * gate(10.0);
                (v, v)
            }
            "gateramp" => {
                // 6 -> 10 Hz
                let hz = 6.0 + 4.0 * t / secs;
                phase += hz / RATE;
                let g = if phase.fract() < 0.5 { 1.0 } else { 0.0 };
                let v = saw_chord(t) * g;
                (v, v)
            }
            "noise" => {
                let v = r.n() * 0.9;
                (v, r.n() * 0.9)
            }
            "noisegate" => {
                let g = gate(7.0);
                (r.n() * 0.95 * g, r.n() * 0.95 * g)
            }
            "sweep" => {
                // log sweep 30 Hz -> 16 kHz over the part
                let (f0, f1) = (30.0f64, 16000.0f64);
                let k = (f1 / f0).ln() / secs;
                let ph = f0 * ((k * t).exp() - 1.0) / k;
                let v = (TAU * ph).sin() * 0.9;
                (v, v)
            }
            "chirp8" => {
                // a fast sweep 100 Hz -> 8 kHz, eight times a second
                let tt = (t * 8.0).fract() / 8.0;
                let (f0, f1, d) = (100.0f64, 8000.0f64, 0.125f64);
                let k = (f1 / f0).ln() / d;
                let ph = f0 * ((k * tt).exp() - 1.0) / k;
                let v = (TAU * ph).sin() * 0.9;
                (v, v)
            }
            "beats" => {
                // 175 bpm: kick on quarters, snare on 2/4, hats on 16ths
                let q = 60.0 / 175.0;
                let tk = t % q;
                let kick =
                    (TAU * (50.0 + 120.0 * (-tk * 30.0).exp()) * tk).sin() * (-tk * 9.0).exp();
                let ts = (t + q) % (2.0 * q);
                let snare = r.n() * (-ts * 25.0).exp() * if ts < 2.0 * q { 0.7 } else { 0.0 };
                let th = t % (q / 4.0);
                let hat = r.n() * (-th * 120.0).exp() * 0.4;
                let bass = (TAU * 55.0 * t).sin() * 0.3;
                let v = (kick * 0.9 + snare + hat + bass).clamp(-1.0, 1.0);
                (v, v)
            }
            "kick8" => {
                // kicks eight times a second, full level
                let tk = t % 0.125;
                let v = (TAU * (45.0 + 150.0 * (-tk * 40.0).exp()) * tk).sin() * (-tk * 18.0).exp();
                (v, v)
            }
            "antiphase" => {
                // left on while right is off, 8 Hz
                let g = gate(8.0);
                let v = saw_chord(t);
                (v * g, v * (1.0 - g))
            }
            "silence_burst" => {
                // full-band bursts of 30 ms, 9 a second
                let g = if (t * 9.0).fract() < 0.27 { 1.0 } else { 0.0 };
                let v = (r.n() * 0.5 + saw_chord(t) * 0.5) * g;
                (v, v)
            }
            _ => panic!("music {kind}"),
        };
        out.push(l.clamp(-1.0, 1.0) as f32);
        out.push(rr.clamp(-1.0, 1.0) as f32);
    }
    out
}

pub const WORST: [&str; 8] = [
    "gate8",
    "noise",
    "sweep",
    "beats",
    "gateramp",
    "kick8",
    "antiphase",
    "silence_burst",
];

/// All the parts after one another, `each` seconds each.
pub fn worst(each: f64) -> Vec<f32> {
    let mut v = Vec::new();
    for (k, p) in WORST.iter().enumerate() {
        v.extend(part(p, each, 99 + k as u64));
    }
    v
}

pub fn write_wav(path: &std::path::Path, s: &[f32]) {
    let mut b = Vec::new();
    let data = (s.len() * 2) as u32;
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend((48000u32 * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    for x in s {
        b.extend(((x * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}
