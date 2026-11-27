//! Stand-in covers: a track that brings no cover is shown one of 100
//! pictures made here, in code - no image files, no crates beyond `image`
//! (already used for covers) - instead of the empty box with a note.
//!
//! Eleven families, each a function of its parameters: gradient meshes,
//! synthwave suns over a grid, Bauhaus compositions, ink and watercolour
//! blots, layered waves, night skies with nebulae, mosaics, calm op-art,
//! botanical silhouettes, records and their labels, and ridged landscapes.
//! A preset is a family with its parameters and one of twenty palettes
//! ([`PRESETS`] of them, pairwise unlike: `tests`); which one a track gets
//! is seeded from what it is ([`seed`]): an album - its name in its folder,
//! as the playlist's album heads have it - shares one cover, compilations
//! too; a track without an album gets its own from its file. The same
//! track always gets the same cover (the seed is FNV-1a, stable across
//! versions and machines; [`VERSION`] changes when the pictures do), the
//! seed also moving a preset's details a little, so two albums of one
//! preset differ.
//!
//! The pictures are full-bleed and mid-toned: they read on a light theme's
//! card and a dark one's. They are still pictures, made once a track on a
//! thread of their own ([`Covers`]), kept in memory (a few) and on disk
//! (`~/.cache/axross-amp/covers`, the newest [`DISK_KEPT`]): the 256 px one
//! (also the desktop's: MPRIS) and the 1024 px one (the cover as the
//! picture, full screen). A change of cover goes through the window's own
//! cover change - its turn, its crossfade, the slower one for saturated red
//! (none of these pictures has that much: `tests`).

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

/// How many presets there are.
pub const PRESETS: usize = 100;
/// The pictures' version: in the disk cache's names (another drawing,
/// other files).
pub const VERSION: u32 = 1;
/// The sides made: the small cover (the card, the desktop) and the large.
pub const SMALL: u32 = 256;
pub const LARGE: u32 = 1024;
/// Covers kept on disk (the newest; each a small and a large PNG).
pub const DISK_KEPT: usize = 64;

type Rgb = [f32; 3];

/// The families, by name (tests, the contact sheet).
pub const FAMILIES: [&str; 11] = [
    "gradient mesh",
    "synthwave",
    "bauhaus",
    "watercolour",
    "waves",
    "night sky",
    "mosaic",
    "op-art",
    "botanical",
    "record",
    "landscape",
];

/// The palettes: a dark, two middles, a light accent and a light (sRGB).
const PALETTES: [[u32; 5]; 20] = [
    [0x2b1a4a, 0x6b2d6b, 0xb4507a, 0xf08a4b, 0xf6d06f], // sunset
    [0x0b1d33, 0x12436b, 0x1f7a8c, 0x6ec6ca, 0xe0f2f1], // ocean
    [0x1b2e21, 0x2f5d3a, 0x6a994e, 0xa7c957, 0xf2e8cf], // forest
    [0x232136, 0x3e3a5c, 0x9a6a9a, 0xe6a4b4, 0xf6e3d4], // dusk rose
    [0x3d405b, 0x81b29a, 0xd9825f, 0xf2cc8f, 0xf4f1de], // terracotta
    [0x0d0221, 0x261447, 0x7b3fe4, 0xff6ec7, 0x2de2e6], // neon
    [0x2a3d45, 0x84a59d, 0xe8908a, 0xf6bd60, 0xf7ede2], // mint cream
    [0x2e3440, 0x4c566a, 0x88c0d0, 0xa3be8c, 0xeceff4], // nordic
    [0x3d2c2e, 0x8c5a3c, 0xd9a05b, 0xf2d7a7, 0xf7efe1], // desert
    [0x1f1d36, 0x3f3351, 0x864879, 0xe9a6a6, 0xf5e8c7], // lavender
    [0x04293a, 0x064663, 0x0f8b8d, 0xecb365, 0xf4f4f4], // teal and gold
    [0x2d0320, 0x5c1a4b, 0x9c3d77, 0xe07eaa, 0xf7c6d9], // berry
    [0x0a1128, 0x001f54, 0x034078, 0x1282a2, 0xfefcfb], // blue
    [0x283618, 0x606c38, 0xbc8a45, 0xdda15e, 0xfefae0], // olive
    [0x253237, 0x5c6b73, 0x9db4c0, 0xc2dfe3, 0xe0fbfc], // ice
    [0x003049, 0x0b6e8a, 0xe8913a, 0xfcbf49, 0xeae2b7], // reef
    [0x10002b, 0x3c096c, 0x7b2cbf, 0xc77dff, 0xe0aaff], // plum
    [0x3b2a1a, 0x7f4f24, 0xa68a64, 0xc2c5aa, 0xe8e4d4], // earth
    [0x33415c, 0x5c7aa8, 0x90e0ef, 0xc1fba4, 0xffef9f], // candy
    [0x0f0f1b, 0x1d1d3b, 0x3a3a6b, 0xc9a227, 0xf2e7c9], // midnight gold
];

fn rgb(c: u32) -> Rgb {
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

fn palette(i: usize) -> [Rgb; 5] {
    PALETTES[i % PALETTES.len()].map(rgb)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn scale(a: Rgb, k: f32) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of a pixel `px` wide a shape covers whose signed distance is
/// `d` (negative inside).
fn cover(d: f32, px: f32) -> f32 {
    (0.5 - d / px.max(1e-6)).clamp(0.0, 1.0)
}

/// FNV-1a, 64 bits: the same everywhere, in every version.
pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A small generator of numbers (splitmix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = splitmix(self.0);
        self.0
    }
    /// In [0, 1).
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
}

/// A lattice point's value in [0, 1).
fn lattice(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Smooth value noise in [0, 1).
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (xf, yf) = (x - xi, y - yi);
    let (u, v) = (xf * xf * (3.0 - 2.0 * xf), yf * yf * (3.0 - 2.0 * yf));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = lattice(xi, yi, seed);
    let b = lattice(xi + 1, yi, seed);
    let c = lattice(xi, yi + 1, seed);
    let d = lattice(xi + 1, yi + 1, seed);
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

/// Fractal noise (`octaves`), in about [0, 1).
fn fbm(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * noise(x * freq, y * freq, seed.wrapping_add(o * 101));
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

/// A picture: the colour at (u, v) in [0, 1]², `px` a pixel's width there.
type Paint = Box<dyn Fn(f32, f32, f32) -> Rgb + Send + Sync>;

/// Which family preset `p` is of, and which of its own it is.
pub fn family_of(p: usize) -> (usize, usize) {
    let p = p % PRESETS;
    // (nine of each of the first ten, ten landscapes)
    if p < 90 { (p / 9, p % 9) } else { (10, p - 90) }
}

/// The preset a seed gets.
pub fn preset_of(seed: u64) -> usize {
    (splitmix(seed ^ 0x5eed) % PRESETS as u64) as usize
}

/// A track's seed: its album in its folder (one album, one cover - as the
/// playlist's album heads group it), else its file (or stream).
pub fn seed(path: &str, album: &str) -> u64 {
    let album = album.trim();
    if album.is_empty() {
        return fnv(format!("track\u{1f}{path}").as_bytes());
    }
    let folder = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    fnv(format!("album\u{1f}{folder}\u{1f}{}", album.to_lowercase()).as_bytes())
}

/// The painter of preset `p`, its details moved by `seed`.
fn painter(p: usize, seed: u64) -> Paint {
    let (family, k) = family_of(p);
    // (the structure from the preset; small things from the seed)
    let mut r = Rng(splitmix(stir(p as u64)));
    let mut s = Rng(splitmix(seed ^ 0xa5a5_5a5a_dead_beef));
    let pal = palette(k * 7 + family * 3 + family / 4);
    match family {
        0 => mesh(pal, k, &mut r, &mut s),
        1 => synthwave(pal, k, &mut r, &mut s),
        2 => bauhaus(pal, k, &mut r, &mut s),
        3 => watercolour(pal, k, &mut r, &mut s),
        4 => waves(pal, k, &mut r, &mut s),
        5 => night(pal, k, &mut r, &mut s),
        6 => mosaic(pal, k, &mut r, &mut s),
        7 => opart(pal, k, &mut r, &mut s),
        8 => botanical(pal, k, &mut r, &mut s),
        9 => record(pal, k, &mut r, &mut s),
        _ => landscape(pal, k, &mut r, &mut s),
    }
}

/// (the preset's own number stirred: its structure)
const fn stir(p: u64) -> u64 {
    p.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ 0x00c0_ffee
}

/// Preset `preset` drawn `side` pixels square, its details moved by `seed`.
pub fn render(preset: usize, seed: u64, side: u32) -> image::RgbaImage {
    let side = side.max(1);
    let paint = painter(preset, seed);
    let px = 1.0 / side as f32;
    let mut out = image::RgbaImage::new(side, side);
    let grain_seed = (seed as u32) ^ 0x9e37;
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let (u, v) = ((x as f32 + 0.5) * px, (y as f32 + 0.5) * px);
        let c = tame_red(paint(u, v, px));
        // (a fine grain: no bands in slow gradients)
        let g = (lattice(x as i32, y as i32, grain_seed) - 0.5) * (1.6 / 255.0);
        let to = |v: f32| ((v + g).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        *pixel = image::Rgba([to(c[0]), to(c[1]), to(c[2]), 255]);
    }
    out
}

/// A colour of saturated red made less so: none of these pictures has
/// what the window's red guard slows a cover's change for (a red share of
/// 0.75, linear or as sent - viz::red_share). Some seeds of preset 5 had
/// 10 % of it (review of 0.17.1, L1).
fn tame_red(c: Rgb) -> Rgb {
    let share = |c: Rgb| {
        let lin = |v: f32| v.max(0.0).powf(2.2);
        let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
        let linear = if r + g + b > 0.0 {
            r / (r + g + b)
        } else {
            0.0
        };
        let sum = c[0] + c[1] + c[2];
        let gamma = if sum > 0.0 { c[0] / sum } else { 0.0 };
        linear.max(gamma)
    };
    if share(c) < 0.66 {
        return c;
    }
    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    for k in [0.2f32, 0.4, 0.6, 0.8, 1.0] {
        let t = mix(c, [l, l, l], k);
        if share(t) < 0.66 {
            return t;
        }
    }
    [l, l, l]
}

// ---------------------------------------------------------------- families

/// Soft colour fields that flow into one another.
fn mesh(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let n = 5 + r.pick(3);
    let start = r.pick(4);
    // (the lighter four colours; the darkest once at most, as a depth)
    let colours = [pal[1], pal[2], pal[3], pal[4]];
    let points: Vec<([f32; 2], Rgb)> = (0..n)
        .map(|i| {
            // (spread over the square: one point in each of its parts)
            let (cx, cy) = ((i % 3) as f32, (i / 3) as f32);
            let p = [
                ((cx + r.range(0.1, 0.9)) / 3.0 + s.range(-0.05, 0.05)).clamp(0.0, 1.0),
                ((cy + r.range(0.1, 0.9)) / 3.0 + s.range(-0.05, 0.05)).clamp(0.0, 1.0),
            ];
            let col = if i == n - 1 && k % 2 == 1 {
                mix(pal[0], pal[1], 0.5)
            } else {
                colours[(start + i * 3) % 4]
            };
            (p, col)
        })
        .collect();
    let sigma = r.range(0.16, 0.26);
    let warp = r.range(0.04, 0.16);
    let freq = r.range(1.2, 2.6);
    let ns = s.next() as u32;
    let glow = k.is_multiple_of(3);
    Box::new(move |u, v, _| {
        let wu = u + warp * (fbm(u * freq, v * freq, ns, 3) - 0.5) * 2.0;
        let wv = v + warp * (fbm(u * freq + 5.2, v * freq + 1.3, ns, 3) - 0.5) * 2.0;
        let (mut c, mut w) = ([0.0f32; 3], 0.0f32);
        for (p, col) in &points {
            let d2 = (wu - p[0]).powi(2) + (wv - p[1]).powi(2);
            let wi = (-d2 / (sigma * sigma)).exp() + 1e-4;
            c = [c[0] + col[0] * wi, c[1] + col[1] * wi, c[2] + col[2] * wi];
            w += wi;
        }
        let c = scale(c, 1.0 / w);
        // (blends grey toward their middle: the colour lifted back)
        let l = (c[0] + c[1] + c[2]) / 3.0;
        let c = [
            (l + (c[0] - l) * 1.35).clamp(0.0, 1.0),
            (l + (c[1] - l) * 1.35).clamp(0.0, 1.0),
            (l + (c[2] - l) * 1.35).clamp(0.0, 1.0),
        ];
        if glow {
            // (a soft light from above)
            mix(c, pal[4], 0.18 * (1.0 - v).powi(3))
        } else {
            c
        }
    })
}

/// A striped sun setting behind a perspective grid.
fn synthwave(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let horizon = r.range(0.58, 0.68);
    let sun_r = r.range(0.17, 0.25);
    let sun = [
        0.5 + s.range(-0.05, 0.05),
        horizon - sun_r * r.range(0.15, 0.55),
    ];
    let bands = 5 + r.pick(4);
    let mountains = k % 3 != 1;
    let stars = k.is_multiple_of(2);
    let ridge_seed = s.next() as u32;
    let star_seed = s.next() as u32;
    let lines_v = r.range(7.0, 12.0);
    let lines_h = r.range(5.0, 8.0);
    let (sky_top, sky_mid, glow, sun_top, sun_bottom, line) = (
        scale(pal[0], 0.8),
        pal[1],
        pal[2],
        pal[4],
        pal[3],
        mix(pal[3], pal[4], 0.3),
    );
    Box::new(move |u, v, px| {
        if v < horizon {
            // the sky, warmer toward the horizon
            let t = v / horizon;
            let mut c = if t < 0.6 {
                mix(sky_top, sky_mid, t / 0.6)
            } else {
                mix(sky_mid, glow, (t - 0.6) / 0.4)
            };
            if stars && t < 0.55 {
                let (gx, gy) = ((u * 60.0) as i32, (v * 60.0) as i32);
                let h = lattice(gx, gy, star_seed);
                if h < 0.12 {
                    let cx = (gx as f32 + lattice(gx, gy, star_seed + 1)) / 60.0;
                    let cy = (gy as f32 + lattice(gx, gy, star_seed + 2)) / 60.0;
                    let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
                    let rad = 0.0016f32.max(px * 0.7);
                    let b = (1.0 - t / 0.55) * (0.0016 / rad).powi(2) * (h / 0.12 * 0.6 + 0.4);
                    c = mix(c, sun_top, b * (-(d / rad).powi(2)).exp());
                }
            }
            // the sun: its lower half cut by widening gaps
            let d = ((u - sun[0]).powi(2) + (v - sun[1]).powi(2)).sqrt() - sun_r;
            let mut a = cover(d, px);
            let below = (v - (sun[1] - sun_r * 0.05)) / (sun_r * 1.05);
            if below > 0.0 {
                let band = below * bands as f32;
                let i = band.floor();
                let gap = 0.12 + 0.5 * (i / bands as f32);
                let f = band - i;
                let edge = px * bands as f32 / (sun_r * 1.05);
                a *= smooth(gap - edge, gap + edge, f);
            }
            if a > 0.0 {
                let st = ((v - (sun[1] - sun_r)) / (2.0 * sun_r)).clamp(0.0, 1.0);
                c = mix(c, mix(sun_top, sun_bottom, st), a);
            }
            // (a soft glow round it)
            c = mix(
                c,
                sun_bottom,
                0.25 * (-(d.max(0.0) / 0.12)).exp() * (1.0 - a),
            );
            if mountains {
                let h = horizon
                    - 0.13 * (fbm(u * 3.0, 0.5, ridge_seed, 4) * 1.4 - 0.25).max(0.0)
                    - 0.02;
                let m = cover(h - v, px);
                c = mix(c, mix(sky_top, sky_mid, 0.35), m);
            }
            c
        } else {
            // the ground: lines toward the vanishing point, and across
            let depth = (v - horizon).max(1e-4);
            let z = 0.18 / depth;
            let ground = mix(scale(pal[0], 0.6), sky_top, 0.3);
            let dz = 0.18 / (depth * depth) * px;
            let fz = (z * lines_h / 3.0).fract();
            let across = 1.0 - smooth(0.0, 0.04 + dz * lines_h, fz.min(1.0 - fz));
            let x = (u - 0.5) * z * lines_v;
            let fx = x.fract().abs();
            let dx = z * lines_v * px;
            let along = 1.0 - smooth(0.0, 0.03 * z + dx, fx.min(1.0 - fx));
            let fade = smooth(0.0, 0.12, depth) * 0.85 + 0.15;
            let l = across.max(along) * fade;
            let c = mix(ground, line, l * 0.85);
            // (the horizon's haze)
            mix(c, glow, (-(depth / 0.05)).exp() * 0.6)
        }
    })
}

/// Flat shapes in a grid: circles, halves, quarters, squares, triangles.
fn bauhaus(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let dark = k % 2 == 1;
    let paper = if dark {
        scale(pal[0], 0.9)
    } else {
        mix(pal[4], rgb(0xf3ece0), 0.6)
    };
    let ink = if dark { pal[4] } else { rgb(0x1d1d1f) };
    let g = 2 + r.pick(2);
    let margin = 0.07;
    let cells: Vec<(usize, usize, Rgb, Rgb)> = (0..g * g)
        .map(|_| {
            let a = pal[1 + r.pick(3)];
            let mut b = pal[1 + r.pick(4)];
            if b == a {
                b = paper;
            }
            (r.pick(7), r.pick(4), a, b)
        })
        .collect();
    let big = r.chance(0.5).then(|| {
        (
            [r.range(0.3, 0.7), r.range(0.3, 0.7)],
            r.range(0.22, 0.32),
            pal[1 + r.pick(3)],
        )
    });
    let lines = k.is_multiple_of(3);
    let jitter = s.range(-0.01, 0.01);
    Box::new(move |u, v, px| {
        let mut c = paper;
        let span = 1.0 - 2.0 * margin;
        let (gu, gv) = ((u - margin) / span, (v - margin) / span);
        if (0.0..1.0).contains(&gu) && (0.0..1.0).contains(&gv) {
            let (ci, cj) = ((gu * g as f32) as usize, (gv * g as f32) as usize);
            let (kind, turn, a, b) = cells[cj * g + ci];
            let cell = span / g as f32;
            let (mut x, mut y) = (gu * g as f32 - ci as f32, gv * g as f32 - cj as f32);
            for _ in 0..turn {
                (x, y) = (y, 1.0 - x);
            }
            let cpx = px / cell;
            let inset = 0.06;
            let d = match kind {
                0 => ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() - 0.42 - jitter,
                1 => (((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() - 0.44).max(0.5 - y),
                2 => (x.powi(2) + y.powi(2)).sqrt() - 0.94,
                3 => (x - 0.5).abs().max((y - 0.5).abs()) - 0.36,
                4 => (y - 0.92).max((x - 0.5).abs() * 1.8 + (0.1 - y) * 0.9 - 0.75 + 0.3),
                5 => (((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() - 0.3).abs() - 0.1,
                _ => {
                    // three bars
                    let bar = ((x - 0.18) * 3.0).fract();
                    if (0.12..0.88).contains(&x) && (0.1..0.9).contains(&y) {
                        (bar - 0.55) / 3.0
                    } else {
                        1.0
                    }
                }
            };
            // (the cell's ground, a little in from its edges)
            let inside = (x - 0.5).abs().max((y - 0.5).abs()) - (0.5 - inset);
            if kind % 2 == 0 {
                c = mix(c, b, cover(inside, cpx) * 0.9);
            }
            c = mix(c, a, cover(d.max(inside), cpx));
            if lines {
                let (lx, ly) = ((gu * g as f32).fract(), (gv * g as f32).fract());
                let edge = lx.min(1.0 - lx).min(ly.min(1.0 - ly)) * cell;
                c = mix(c, ink, cover(edge - 0.003, px) * 0.85);
            }
        }
        if let Some((p, rad, col)) = big {
            let d = ((u - p[0]).powi(2) + (v - p[1]).powi(2)).sqrt() - rad;
            // (multiplied over what is under it, as printed ink)
            let m = [c[0] * col[0], c[1] * col[1], c[2] * col[2]];
            c = mix(c, mix(m, col, 0.35), cover(d, px) * 0.85);
        }
        c
    })
}

/// Blots of watercolour - or ink - on paper, their edges darker.
fn watercolour(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let ink = k % 3 == 2;
    let paper = mix(pal[4], rgb(0xf6f1e7), 0.7);
    let n = 3 + r.pick(4);
    let blots: Vec<([f32; 2], f32, Rgb, u32, f32)> = (0..n)
        .map(|i| {
            (
                [r.range(0.2, 0.8) + s.range(-0.04, 0.04), r.range(0.2, 0.8)],
                r.range(0.14, 0.3),
                if ink && i % 3 != 2 {
                    mix(pal[0], pal[1], 0.3)
                } else {
                    [pal[2], pal[3], mix(pal[1], pal[4], 0.35), pal[3]][(i + k) % 4]
                },
                r.next() as u32,
                r.range(0.45, 0.7),
            )
        })
        .collect();
    let fiber = s.next() as u32;
    Box::new(move |u, v, px| {
        let tex = fbm(u * 40.0, v * 40.0, fiber, 2);
        let mut c = scale(paper, 0.97 + 0.03 * tex);
        for (p, rad, col, seed, a) in &blots {
            let (dx, dy) = (u - p[0], v - p[1]);
            let ang = dy.atan2(dx);
            let wobble = fbm(ang.cos() * 1.6 + 3.0, ang.sin() * 1.6 + 3.0, *seed, 4);
            let warp = fbm(u * 6.0, v * 6.0, seed + 7, 3) * 0.06;
            let d = (dx * dx + dy * dy).sqrt() - rad * (0.75 + 0.5 * wobble) - warp;
            let edge_w = if ink { 0.004 } else { 0.012 };
            let inside = smooth(edge_w + px, -edge_w - px, d);
            if inside <= 0.0 {
                continue;
            }
            // (pigment gathers at the edge; granulates inside)
            let rim = (-(d.abs() / 0.012)).exp();
            let gran = fbm(u * 90.0, v * 90.0, seed + 11, 2);
            let alpha = (a * (0.75 + 0.35 * rim + 0.2 * (gran - 0.5))).clamp(0.0, 0.95) * inside;
            let multiplied = [c[0] * col[0], c[1] * col[1], c[2] * col[2]];
            c = mix(c, mix(multiplied, *col, 0.6), alpha);
        }
        c
    })
}

/// Layers of waves, one behind another.
fn waves(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let layers = 5 + r.pick(4);
    let rows: Vec<(f32, f32, f32, f32, u32)> = (0..layers)
        .map(|i| {
            let base = 0.22 + 0.72 * (i as f32 + 0.5) / layers as f32;
            (
                base,
                r.range(0.015, 0.05),
                r.range(1.0, 3.0),
                s.range(0.0, TAU),
                r.next() as u32,
            )
        })
        .collect();
    let reverse = k % 2 == 1;
    let sun = k
        .is_multiple_of(3)
        .then(|| [r.range(0.25, 0.75), r.range(0.12, 0.2)]);
    let sky = if reverse {
        mix(pal[0], pal[1], 0.5)
    } else {
        mix(pal[4], pal[3], 0.3)
    };
    Box::new(move |u, v, px| {
        let mut c = mix(sky, if reverse { pal[0] } else { pal[4] }, 1.0 - v * 1.5);
        if let Some(p) = sun {
            let d = ((u - p[0]).powi(2) + (v - p[1]).powi(2)).sqrt() - 0.09;
            c = mix(c, if reverse { pal[4] } else { pal[3] }, cover(d, px) * 0.9);
        }
        for (i, (base, amp, freq, phase, seed)) in rows.iter().enumerate() {
            let y = base
                + amp * (TAU * freq * u + phase).sin()
                + 0.012 * (fbm(u * 4.0, *base * 9.0, *seed, 3) - 0.5);
            let t = (i as f32 + 1.0) / layers as f32;
            let col = if reverse {
                mix(pal[3], pal[1], t)
            } else {
                mix(pal[2], pal[0], t)
            };
            let below = v - y;
            let a = cover(-below, px);
            if a > 0.0 {
                // (each crest shades the layer behind it)
                let shade = 1.0 - 0.22 * (-(below.max(0.0) / 0.03)).exp();
                c = mix(c, scale(col, shade), a);
                // a light rim on the crest
                c = mix(
                    c,
                    mix(col, pal[4], 0.5),
                    cover(below.abs() - 0.0015, px) * 0.5,
                );
            }
        }
        c
    })
}

/// A night sky: a nebula and a planet, the Milky Way over hills, or an
/// aurora's curtains - and stars.
fn night(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let kind = k % 3;
    let top = scale(pal[0], 0.55);
    let bottom = mix(pal[0], pal[1], 0.6);
    let neb_seed = r.next() as u32;
    let neb_freq = r.range(1.5, 3.0);
    let (neb_a, neb_b) = (pal[2], pal[3]);
    let star_seed = s.next() as u32;
    let planet = (kind == 0).then(|| {
        (
            [r.range(0.3, 0.7), r.range(0.3, 0.6)],
            r.range(0.12, 0.22),
            pal[3 + r.pick(2)],
            r.range(0.0, TAU),
        )
    });
    // (the Milky Way: a band across, its tilt)
    let tilt = r.range(-0.9, 0.9);
    let band_at = r.range(0.35, 0.55);
    // (an aurora: curtains along a wavering line)
    let aurora_seed = r.next() as u32;
    let aurora = mix(pal[3 + k % 2], rgb(0x7cf0b4), [0.65, 0.25, 0.45][k % 3]);
    let horizon = (kind != 0).then(|| r.range(0.78, 0.88));
    let hills = mix(pal[0], rgb(0x000000), 0.55);
    Box::new(move |u, v, px| {
        let mut c = mix(top, bottom, v);
        let mut dense = 0.0;
        match kind {
            0 => {
                let n = fbm(
                    u * neb_freq + fbm(u * 2.0, v * 2.0, neb_seed + 3, 3),
                    v * neb_freq,
                    neb_seed,
                    5,
                );
                let m = smooth(0.42, 0.8, n);
                c = mix(c, mix(neb_a, neb_b, smooth(0.6, 0.9, n)), m * 0.7);
            }
            1 => {
                // the band: its glow, its dust lanes darker
                let (cs, sn) = (tilt.cos(), tilt.sin());
                let across = (u - 0.5) * sn + (v - band_at) * cs;
                let along = (u - 0.5) * cs - (v - band_at) * sn;
                let n = fbm(along * 4.0, across * 9.0, neb_seed, 5);
                let glow = (-(across / 0.16).powi(2)).exp() * (0.55 + 0.6 * n);
                let dust = smooth(0.55, 0.75, fbm(along * 7.0, across * 14.0, neb_seed + 2, 4))
                    * (-(across / 0.06).powi(2)).exp();
                c = mix(c, mix(neb_b, pal[4], 0.5), (glow * 0.6).min(0.8));
                c = mix(c, top, dust * 0.6);
                dense = glow;
            }
            _ => {
                // curtains: bright at their foot, fading up, swaying
                let sway = fbm(u * 2.5, 0.3, aurora_seed, 3);
                let foot = 0.52 + 0.14 * sway + 0.05 * (u * TAU * 0.9 + sway * 4.0).sin();
                let rays = fbm(u * 24.0 + sway * 6.0, 0.5, aurora_seed + 4, 2);
                let rise = (foot - v) / 0.45;
                let a = if (0.0..1.0).contains(&rise) {
                    (1.0 - rise).powf(1.6) * smooth(0.0, 0.04, rise + 0.03)
                } else {
                    0.0
                } * (0.35 + 0.75 * rays);
                c = mix(c, aurora, (a * 1.1).min(0.95));
                c = mix(
                    c,
                    mix(aurora, pal[2], 0.6),
                    a * smooth(0.5, 1.0, rise) * 0.6,
                );
            }
        }
        // stars on a lattice, a few bright (more within the band)
        for (cells, dens, size) in [(90.0f32, 0.2f32, 0.0012f32), (30.0, 0.12, 0.0026)] {
            let (gx, gy) = ((u * cells) as i32, (v * cells) as i32);
            let h = lattice(gx, gy, star_seed + cells as u32);
            let dens = dens * (1.0 + dense * 2.0);
            if h < dens {
                let cx = (gx as f32 + lattice(gx, gy, star_seed + 5)) / cells;
                let cy = (gy as f32 + lattice(gx, gy, star_seed + 6)) / cells;
                let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
                let rad = size.max(px * 0.7);
                let b = (size / rad).powi(2) * (0.5 + 0.5 * h / dens);
                c = mix(c, rgb(0xfdf6e3), b * (-(d / rad).powi(2)).exp());
            }
        }
        if let Some((p, rad, col, light)) = planet {
            let (dx, dy) = (u - p[0], v - p[1]);
            let d = (dx * dx + dy * dy).sqrt() - rad;
            let a = cover(d, px);
            if a > 0.0 {
                // (lit from one side: the terminator soft)
                let (lx, ly) = (light.cos(), light.sin());
                let z = (1.0 - ((dx * dx + dy * dy) / (rad * rad))).max(0.0).sqrt();
                let lit = ((dx / rad) * lx + (dy / rad) * ly + z * 0.6).clamp(0.0, 1.0);
                let bands = 0.85 + 0.15 * fbm(u * 3.0, v * 30.0, 9, 3);
                let surface = scale(col, bands);
                c = mix(c, mix(scale(col, 0.12), surface, smooth(0.0, 0.7, lit)), a);
            }
            c = mix(c, col, 0.18 * (-(d.max(0.0) / 0.05)).exp() * (1.0 - a));
        }
        if let Some(h) = horizon {
            let y = h - 0.05 * fbm(u * 4.0, 0.3, neb_seed + 9, 4);
            c = mix(c, hills, cover(y - v, px));
        }
        c
    })
}

/// Tiles: triangles, quarter-circle arcs, or quarter discs.
fn mosaic(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let n = (5 + r.pick(5)) as f32;
    let kind = k % 3;
    let grout = if k.is_multiple_of(2) {
        mix(pal[4], rgb(0xffffff), 0.4)
    } else {
        scale(pal[0], 0.7)
    };
    let tile_seed = s.next() as u32 ^ r.next() as u32;
    Box::new(move |u, v, px| {
        let (x, y) = (u * n, v * n);
        let (i, j) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - i as f32, y - j as f32);
        let tpx = px * n;
        let h = lattice(i, j, tile_seed);
        let col = |o: u32| pal[1 + (lattice(i, j, tile_seed + o) * 4.0) as usize % 4];
        let (a, b) = (col(1), col(2));
        let b = if a == b { pal[0] } else { b };
        let c = match kind {
            0 => {
                // split along a diagonal
                let d = if h < 0.5 { fx - fy } else { fx + fy - 1.0 };
                mix(a, b, cover(-d / std::f32::consts::SQRT_2, tpx))
            }
            1 => {
                // Truchet: two quarter-circle arcs
                let (cx, cy) = if h < 0.5 { (fx, fy) } else { (1.0 - fx, fy) };
                let d1 = ((cx).powi(2) + (cy).powi(2)).sqrt() - 0.5;
                let d2 = ((1.0 - cx).powi(2) + (1.0 - cy).powi(2)).sqrt() - 0.5;
                let band = d1.abs().min(d2.abs()) - 0.17;
                mix(a, b, cover(band, tpx))
            }
            _ => {
                // a quarter disc from a corner
                let corner = (h * 4.0) as u32;
                let (cx, cy) = match corner {
                    0 => (fx, fy),
                    1 => (1.0 - fx, fy),
                    2 => (fx, 1.0 - fy),
                    _ => (1.0 - fx, 1.0 - fy),
                };
                let d = (cx * cx + cy * cy).sqrt() - 0.85;
                let ring = (cx * cx + cy * cy).sqrt() - 0.45;
                let c = mix(a, b, cover(d, tpx));
                mix(c, pal[4], cover(ring, tpx) * 0.6)
            }
        };
        let edge = fx.min(1.0 - fx).min(fy.min(1.0 - fy));
        mix(c, grout, cover(edge - 0.035, tpx))
    })
}

/// Calm op-art: wide, uneven rings or bands in near tones.
fn opart(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let centre = [r.range(0.25, 0.75), r.range(0.25, 0.75)];
    let rings = r.range(6.0, 11.0);
    let power = r.range(0.6, 0.9);
    let warp_seed = s.next() as u32;
    let warp = r.range(0.02, 0.06);
    let bands = k % 3 == 2;
    // (three tones near one another: no harsh contrast)
    let base = pal[1 + k % 3];
    let tones = [
        mix(base, pal[0], 0.25),
        base,
        mix(base, pal[4], 0.3),
        mix(base, pal[3], 0.35),
    ];
    Box::new(move |u, v, px| {
        let w = warp * (fbm(u * 2.5, v * 2.5, warp_seed, 3) - 0.5) * 2.0;
        let (t, dt) = if bands {
            let y = v + w + 0.08 * (PI * 2.0 * u * 0.8).sin();
            let t = y.max(0.0).powf(power) * rings;
            (t, rings * px * 1.5)
        } else {
            let d = ((u - centre[0]).powi(2) + (v - centre[1]).powi(2)).sqrt() + w;
            let t = d.max(0.0).powf(power) * rings;
            (t, rings * power * px * d.max(0.02).powf(power - 1.0))
        };
        let i = t.floor();
        let f = t - i;
        let ia = (i as i64).rem_euclid(4) as usize;
        let ib = (ia + 1) % 4;
        // (the edge between two bands smoothed over a pixel)
        let a = smooth(1.0 - dt, 1.0, f);
        mix(tones[ia], tones[ib], a)
    })
}

/// Fronds against the light: stems and their leaves.
fn botanical(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let light = k.is_multiple_of(2);
    let (bg_top, bg_bottom) = if light {
        (mix(pal[4], pal[3], 0.2), mix(pal[4], pal[2], 0.25))
    } else {
        (scale(pal[0], 0.8), mix(pal[0], pal[1], 0.7))
    };
    struct Leaf {
        base: [f32; 2],
        dir: [f32; 2],
        len: f32,
        width: f32,
        lo: [f32; 2],
        hi: [f32; 2],
    }
    struct Frond {
        stem: Vec<[f32; 2]>,
        leaves: Vec<Leaf>,
        colour: Rgb,
        lo: [f32; 2],
        hi: [f32; 2],
    }
    let n = 3 + r.pick(3);
    let mut fronds = Vec::new();
    for f in 0..n {
        let x0 = (f as f32 + r.range(0.15, 0.85)) / n as f32 + s.range(-0.03, 0.03);
        let ang = -PI / 2.0 + r.range(-0.6, 0.6);
        let len = r.range(0.7, 1.05);
        let bend = r.range(-0.8, 0.8);
        let mut stem = Vec::new();
        for i in 0..=12 {
            let t = i as f32 / 12.0;
            let a = ang + bend * t * t;
            let prev = stem.last().copied().unwrap_or([x0, 1.04]);
            let step = len / 12.0;
            stem.push(if i == 0 {
                prev
            } else {
                [prev[0] + a.cos() * step, prev[1] + a.sin() * step]
            });
        }
        let mut leaves = Vec::new();
        let pairs = 5 + r.pick(4);
        let spread = r.range(0.6, 1.1);
        let leaf_len = r.range(0.1, 0.2);
        for j in 1..=pairs {
            let t = j as f32 / (pairs as f32 + 1.0);
            let at = ((t * 12.0) as usize).min(11);
            let (p, q) = (stem[at], stem[at + 1]);
            let along = (q[1] - p[1]).atan2(q[0] - p[0]);
            for side in [-1.0f32, 1.0] {
                let a = along + side * spread;
                let l = leaf_len * (1.0 - 0.6 * t);
                let dir = [a.cos(), a.sin()];
                let tip = [p[0] + dir[0] * l, p[1] + dir[1] * l];
                let w = l * 0.32;
                leaves.push(Leaf {
                    base: p,
                    dir,
                    len: l,
                    width: w,
                    lo: [p[0].min(tip[0]) - w, p[1].min(tip[1]) - w],
                    hi: [p[0].max(tip[0]) + w, p[1].max(tip[1]) + w],
                });
            }
        }
        let depth = f as f32 / n as f32;
        let colour = if light {
            mix(mix(pal[1], pal[0], 0.4), bg_bottom, 0.5 * (1.0 - depth))
        } else {
            mix(mix(pal[2], pal[3], 0.3), bg_bottom, 0.55 * (1.0 - depth))
        };
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in stem
            .iter()
            .chain(leaves.iter().flat_map(|l| [&l.lo, &l.hi]))
        {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        fronds.push(Frond {
            stem,
            leaves,
            colour,
            lo: [lo[0] - 0.01, lo[1] - 0.01],
            hi: [hi[0] + 0.01, hi[1] + 0.01],
        });
    }
    let sun = (k % 3 == 1).then(|| [r.range(0.2, 0.8), r.range(0.15, 0.35)]);
    Box::new(move |u, v, px| {
        let mut c = mix(bg_top, bg_bottom, v);
        if let Some(p) = sun {
            let d = ((u - p[0]).powi(2) + (v - p[1]).powi(2)).sqrt() - 0.14;
            c = mix(c, if light { pal[4] } else { pal[3] }, cover(d, px) * 0.8);
        }
        for f in &fronds {
            if u < f.lo[0] || u > f.hi[0] || v < f.lo[1] || v > f.hi[1] {
                continue;
            }
            let mut d = f32::MAX;
            for w in f.stem.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (bx, by) = (b[0] - a[0], b[1] - a[1]);
                let t = (((u - a[0]) * bx + (v - a[1]) * by) / (bx * bx + by * by)).clamp(0.0, 1.0);
                let (qx, qy) = (u - a[0] - bx * t, v - a[1] - by * t);
                d = d.min((qx * qx + qy * qy).sqrt() - 0.004);
            }
            for l in &f.leaves {
                if u < l.lo[0] || u > l.hi[0] || v < l.lo[1] || v > l.hi[1] {
                    continue;
                }
                let (dx, dy) = (u - l.base[0], v - l.base[1]);
                let along = dx * l.dir[0] + dy * l.dir[1];
                let across = -dx * l.dir[1] + dy * l.dir[0];
                let t = along / l.len;
                if !(0.0..=1.0).contains(&t) {
                    continue;
                }
                let half = l.width * (PI * t).sin().powf(0.8);
                d = d.min(across.abs() - half);
            }
            c = mix(c, f.colour, cover(d, px));
        }
        c
    })
}

/// A record: half out of its sleeve, alone on a ground cut in two, or
/// large and running off the edge - and its label.
fn record(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let layout = k % 3;
    let bg = if k.is_multiple_of(2) {
        mix(pal[4], pal[3], 0.25)
    } else {
        mix(pal[1], pal[0], 0.4)
    };
    let bg2 = pal[2 + k % 2];
    let sleeve_col = pal[1 + k % 3];
    let (disc, rad) = match layout {
        0 => (
            [r.range(0.58, 0.64) + s.range(-0.02, 0.02), 0.5],
            r.range(0.36, 0.42),
        ),
        1 => ([0.5 + s.range(-0.02, 0.02), 0.5], r.range(0.3, 0.36)),
        _ => ([r.range(0.62, 0.7), r.range(0.58, 0.68)], r.range(0.5, 0.6)),
    };
    let label = pal[2 + r.pick(3)];
    let label_b = pal[(r.pick(3) + 1) % 5];
    let design = (k + r.pick(2)) % 4;
    let sheen = r.range(0.0, PI);
    let split = r.range(-0.5, 0.5);
    Box::new(move |u, v, px| {
        let mut c = mix(bg, scale(bg, 0.85), v);
        match layout {
            0 => {
                let (l, t, sz) = (0.05, 0.5 - rad * 1.04, rad * 2.08);
                let d = (u - (l + sz / 2.0)).abs().max((v - (t + sz / 2.0)).abs()) - sz / 2.0;
                let a = cover(d, px);
                if a > 0.0 {
                    // (the sleeve's own motif: a circle and a band)
                    let mut sc = sleeve_col;
                    let cd = ((u - (l + sz * 0.38)).powi(2) + (v - (t + sz * 0.62)).powi(2)).sqrt()
                        - sz * 0.22;
                    sc = mix(sc, pal[4], cover(cd, px) * 0.7);
                    sc = mix(
                        sc,
                        pal[0],
                        cover((v - (t + sz * 0.2)).abs() - sz * 0.03, px) * 0.6,
                    );
                    c = mix(c, sc, a);
                }
            }
            1 => {
                // a ground cut in two, a slant
                c = mix(c, bg2, cover((u - 0.5) * split + (v - 0.5) - 0.12, px));
            }
            _ => {
                // a block of colour and a thin rule
                c = mix(c, bg2, cover((0.42 - v).max(u - 0.45), px));
                c = mix(c, pal[0], cover((v - 0.47).abs() - 0.004, px) * 0.7);
            }
        }
        let (dx, dy) = (u - disc[0], v - disc[1]);
        let dist = (dx * dx + dy * dy).sqrt();
        // (its shadow)
        c = mix(c, scale(c, 0.6), 0.5 * smooth(0.04, 0.0, dist - rad - 0.01));
        let a = cover(dist - rad, px);
        if a > 0.0 {
            let ang = dy.atan2(dx);
            let shine = 0.05 * ((2.0 * ang + sheen).cos()).abs().powf(3.0);
            let mut rc = mix(rgb(0x141416), rgb(0x2a2a2e), shine * 6.0);
            // (the gaps between its tracks, faint)
            for g in [0.55f32, 0.72, 0.86] {
                rc = mix(
                    rc,
                    rgb(0x232326),
                    cover((dist - rad * g).abs() - 0.0015, px) * 0.6,
                );
            }
            let lr = rad * 0.36;
            let la = cover(dist - lr, px);
            if la > 0.0 {
                let lc = match design {
                    0 => mix(label, label_b, cover(dy, px)),
                    1 => mix(label, label_b, cover(dist - lr * 0.55, px)),
                    2 => {
                        // a sunburst
                        let ray = ((ang * 8.0 / PI).fract() - 0.5).abs() - 0.25;
                        mix(label, label_b, cover(ray * dist * 0.4, px) * 0.8)
                    }
                    _ => mix(label, label_b, cover((dx * 0.7 + dy).abs() - lr * 0.18, px)),
                };
                rc = mix(rc, lc, la);
            }
            rc = mix(rc, bg, cover(dist - 0.008, px));
            c = mix(c, rc, a);
        }
        c
    })
}

/// Ridges fading into the distance under a sun or a moon.
fn landscape(pal: [Rgb; 5], k: usize, r: &mut Rng, s: &mut Rng) -> Paint {
    let ridges = 4 + r.pick(3);
    let rows: Vec<(f32, f32, f32, u32)> = (0..ridges)
        .map(|i| {
            (
                0.42 + 0.5 * i as f32 / ridges as f32 + r.range(-0.02, 0.02),
                r.range(0.08, 0.2),
                r.range(1.5, 4.0),
                s.next() as u32,
            )
        })
        .collect();
    let night = k % 3 == 1;
    // (the ridges' kind: jagged peaks, rolling hills, or stepped mesas)
    let style = (k / 3) % 3;
    let (sky_top, sky_low) = if night {
        (scale(pal[0], 0.7), pal[1 + k % 2])
    } else {
        (
            pal[1 + k % 3],
            mix(pal[3], pal[4], 0.3 + 0.2 * (k % 2) as f32),
        )
    };
    let sun = [r.range(0.2, 0.8), r.range(0.18, 0.36)];
    let sun_r = r.range(0.05, 0.1) * if style == 1 { 1.6 } else { 1.0 };
    let lake = k % 4 == 3;
    // (the nearest ridge: the darkest colour, tinted)
    let near = mix(pal[0], pal[1 + (k + 1) % 3], 0.3);
    let star_seed = s.next() as u32;
    Box::new(move |u, v0, px| {
        let reflect = lake && v0 > 0.86;
        let v = if reflect { 0.86 - (v0 - 0.86) } else { v0 };
        let mut c = mix(sky_top, sky_low, smooth(0.0, 0.6, v));
        let d = ((u - sun[0]).powi(2) + (v - sun[1]).powi(2)).sqrt() - sun_r;
        let disc = if night {
            pal[4]
        } else {
            mix(pal[4], pal[3], 0.3)
        };
        if night && v < 0.5 {
            let (gx, gy) = ((u * 70.0) as i32, (v * 70.0) as i32);
            let h = lattice(gx, gy, star_seed);
            if h < 0.15 {
                let cx = (gx as f32 + lattice(gx, gy, star_seed + 1)) / 70.0;
                let cy = (gy as f32 + lattice(gx, gy, star_seed + 2)) / 70.0;
                let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
                let rad = 0.0014f32.max(px * 0.7);
                let b = (0.0014 / rad).powi(2) * (1.0 - v * 2.0);
                c = mix(c, pal[4], b * (-(d / rad).powi(2)).exp());
            }
        }
        c = mix(c, disc, cover(d, px));
        c = mix(c, disc, 0.3 * (-(d.max(0.0) / 0.08)).exp());
        for (i, (base, amp, freq, seed)) in rows.iter().enumerate() {
            let t = (i as f32 + 1.0) / ridges as f32;
            let y = match style {
                0 => {
                    // jagged: ridged noise, sharp crests
                    // (few octaves: peaks, not a forest of needles)
                    let n = 1.0
                        - (fbm(u * freq * 0.7 + i as f32 * 3.1, 0.7, *seed, 3) * 2.0 - 1.0).abs();
                    let fine = 0.04 * fbm(u * freq * 4.0, 0.2, *seed + 1, 2);
                    base - amp * (1.3 * (n * n - 0.15).max(0.0) + fine)
                }
                1 => {
                    // rolling: few, soft waves
                    let n = fbm(u * freq * 0.5 + i as f32 * 3.1, 0.7, *seed, 2);
                    base - amp * 0.8 * n
                }
                _ => {
                    // mesas: flat tops, steep sides
                    let n = fbm(u * freq * 0.8 + i as f32 * 3.1, 0.7, *seed, 3);
                    let q = (n * 4.0).floor() + smooth(0.0, 0.3, (n * 4.0).fract());
                    base - amp * 0.35 * q
                }
            };
            let col = mix(sky_low, near, t.powf(0.8));
            let a = cover(y - v, px);
            if a > 0.0 {
                // (mist gathers at each ridge's foot)
                let mist = smooth(0.0, 0.12, v - y);
                c = mix(c, mix(col, sky_low, 0.25 * mist * (1.0 - t)), a);
            }
        }
        if reflect {
            let ripple = 0.03 * (fbm(u * 30.0, v0 * 120.0, 5, 2) - 0.5);
            c = mix(scale(c, 0.75 + ripple), sky_low, 0.15);
        }
        c
    })
}

// ---------------------------------------------------------------- the worker

/// A cover made for a track: the small one, its PNG, the large one.
pub fn art(preset: usize, seed: u64) -> crate::engine::Art {
    let large = render(preset, seed, LARGE);
    let small =
        image::imageops::resize(&large, SMALL, SMALL, image::imageops::FilterType::Triangle);
    art_of(&small, large)
}

fn art_of(small: &image::RgbaImage, large: image::RgbaImage) -> crate::engine::Art {
    let mut png = Vec::new();
    let _ = image::DynamicImage::ImageRgba8(small.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    crate::engine::Art {
        width: small.width(),
        height: small.height(),
        rgba: small.as_raw().clone(),
        png,
        large: Some((large.width(), large.height(), large.into_raw())),
    }
}

/// Where the covers are kept on disk.
pub fn cache_dir() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("axross-amp")
        .join("covers")
}

/// A cover from the disk (`dir`), or made and written there (the newest
/// [`DISK_KEPT`] kept).
pub fn art_cached(dir: &std::path::Path, preset: usize, seed: u64) -> crate::engine::Art {
    let name = |side: u32| dir.join(format!("v{VERSION}-{preset:03}-{seed:016x}-{side}.png"));
    let read = |side: u32| {
        image::open(name(side))
            .ok()
            .map(|i| i.to_rgba8())
            .filter(|i| i.width() == side && i.height() == side)
    };
    if let (Some(small), Some(large)) = (read(SMALL), read(LARGE)) {
        // (used: the newest again)
        let _ = std::fs::File::options()
            .append(true)
            .open(name(SMALL))
            .and_then(|f| f.set_modified(std::time::SystemTime::now()));
        return art_of(&small, large);
    }
    let made = art(preset, seed);
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(name(SMALL), &made.png);
        if let Some((w, h, rgba)) = &made.large
            && let Some(img) = image::RgbaImage::from_raw(*w, *h, rgba.clone())
        {
            let _ = img.save(name(LARGE));
        }
        prune(dir);
    }
    made
}

/// The covers on disk beyond the newest [`DISK_KEPT`] removed.
fn prune(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut small: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .ends_with(&format!("-{SMALL}.png"))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if small.len() <= DISK_KEPT {
        return;
    }
    small.sort_by_key(|e| std::cmp::Reverse(e.0));
    for (_, path) in small.into_iter().skip(DISK_KEPT) {
        let large = path
            .to_string_lossy()
            .replace(&format!("-{SMALL}.png"), &format!("-{LARGE}.png"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(large);
    }
}

/// Covers asked for by seed, made on a thread of their own (from the disk
/// where they are there), the last few kept in memory. Asking and polling
/// allocate nothing when nothing is new.
pub struct Covers {
    ask: crossbeam_channel::Sender<u64>,
    done: crossbeam_channel::Receiver<(u64, Arc<crate::engine::Art>)>,
    kept: Vec<(u64, Arc<crate::engine::Art>)>,
}

/// Covers kept in memory.
const KEPT: usize = 4;

impl Covers {
    /// The worker, writing to `dir` (None: memory only); `wake` when one
    /// is made.
    pub fn start(dir: Option<std::path::PathBuf>, wake: impl Fn() + Send + 'static) -> Covers {
        let (ask, asks) = crossbeam_channel::unbounded::<u64>();
        let (out, done) = crossbeam_channel::unbounded();
        let _ = std::thread::Builder::new()
            .name("amp-covers".into())
            .spawn(move || {
                while let Ok(first) = asks.recv() {
                    // (skipping through tracks: only the last asked)
                    let seed = asks.try_iter().last().unwrap_or(first);
                    let preset = preset_of(seed);
                    let art = match &dir {
                        Some(d) => art_cached(d, preset, seed),
                        None => art(preset, seed),
                    };
                    if out.send((seed, Arc::new(art))).is_err() {
                        return;
                    }
                    wake();
                }
            });
        Covers {
            ask,
            done,
            kept: Vec::new(),
        }
    }

    /// The cover for `seed`: at once where it is kept, else asked for
    /// (it comes by [`Covers::poll`]).
    pub fn get(&mut self, seed: u64) -> Option<Arc<crate::engine::Art>> {
        if let Some((_, art)) = self.kept.iter().find(|(s, _)| *s == seed) {
            return Some(art.clone());
        }
        let _ = self.ask.send(seed);
        None
    }

    /// A cover made since (its seed), kept.
    pub fn poll(&mut self) -> Option<(u64, Arc<crate::engine::Art>)> {
        let (seed, art) = self.done.try_recv().ok()?;
        if self.kept.len() >= KEPT {
            self.kept.remove(0);
        }
        self.kept.push((seed, art.clone()));
        Some((seed, art))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_gets_the_same_cover_every_time_and_an_album_one_cover() {
        let a = seed("/music/Album/01 One.flac", "The Album");
        assert_eq!(a, seed("/music/Album/01 One.flac", "The Album"));
        // (another track of the album, the name in other case)
        assert_eq!(a, seed("/music/Album/07 Seven.flac", "the album"));
        // the same name in another folder, a track without an album
        assert_ne!(a, seed("/other/Album/01 One.flac", "The Album"));
        assert_ne!(seed("/music/x/a.mp3", ""), seed("/music/x/b.mp3", ""));
        // (FNV-1a: the same on every machine and in every version)
        assert_eq!(fnv(b"a"), 0xaf63_dc4c_8601_ec8c);
        // the same picture, pixel for pixel
        for p in [0, 17, 45, 99] {
            let x = render(p, a, 64);
            let y = render(p, a, 64);
            assert_eq!(x.as_raw(), y.as_raw(), "preset {p}");
        }
        // every preset reached by some seed
        let mut hit = [false; PRESETS];
        for i in 0..20_000u64 {
            hit[preset_of(fnv(&i.to_le_bytes()))] = true;
        }
        assert!(hit.iter().all(|h| *h));
    }

    #[test]
    fn each_family_has_its_share() {
        let mut count = [0; FAMILIES.len()];
        for p in 0..PRESETS {
            count[family_of(p).0] += 1;
        }
        assert!(count.iter().all(|c| (9..=10).contains(c)), "{count:?}");
    }
}
