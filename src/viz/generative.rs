//! Pictures the sound moves: some drawn by shaders ([`super::gpu`]: plasma,
//! fire, metaballs, aurora, tunnel, Julia set, reaction-diffusion, ripples,
//! moiré, smoke), the others from meshes here - a flow field, a synthwave
//! landscape, a galaxy, arcs, a skyline, isometric bars, a DNA helix, a
//! spirograph, a morphing polygon, a mandala, rings on the beat and the
//! title's words on it.
//!
//! What each keeps (particles, stars, buildings, the reaction's grid) is
//! made once and then only changed: a frame allocates nothing.

// (bars, peaks and their places go in step: an index reads best)
#![allow(clippy::needless_range_loop)]

use super::gpu::{self, Shader, Uniforms};
use super::{Frame, Pen, VizState, glow, mix, random};
use crate::analyzer::Analyzer;
use egui::{Color32, FontId, Pos2, Rect, Vec2, pos2, vec2};
use std::f32::consts::{PI, TAU};

const FLOW_PARTICLES: usize = 900;
const TRAIL: usize = 10;
const GALAXY_STARS: usize = 2600;
const RINGS: usize = 24;
const SPARKS: usize = 240;
/// The reaction-diffusion's grid.
const RD_W: usize = 128;
const RD_H: usize = 72;

#[derive(Clone, Copy, Default)]
struct Particle {
    trail: [Pos2; TRAIL],
    head: usize,
    age: f32,
    life: f32,
}

#[derive(Clone, Copy, Default)]
struct Building {
    x: f32,
    width: f32,
    height: f32,
    seed: u32,
    back: bool,
}

pub struct State {
    /// The typography's rows behind the word: how far they have run.
    marquee: f32,
    /// The feedback face's preset, the one before it, how far the change
    /// is (0..1), the beat it came on and the seconds since.
    preset: usize,
    preset_before: usize,
    preset_blend: f32,
    preset_beat: u64,
    preset_age: f32,
    /// The bands the feedback face's waves are drawn from, eased.
    bands: [f32; 32],
    seed: u32,
    last_time: f64,
    clock: f32,
    flow: Vec<Particle>,
    stars: Vec<[f32; 4]>,
    /// The beat rings' spokes, eased (48 at most).
    spokes: Vec<f32>,
    galaxy_turn: f32,
    /// The helix's turn.
    helix_spin: f32,
    synth_scroll: f32,
    buildings: Vec<Building>,
    buildings_for: f32,
    /// age, strength, colour (0..1), radius speed
    rings: Vec<[f32; 4]>,
    /// x, y, vx, vy, age (in the face's units)
    sparks: Vec<[f32; 5]>,
    last_beat: u64,
    since_ring: f32,
    morph: (usize, usize, f32),
    morph_turn: f32,
    spiro: (f32, f32, f32),
    spiro_turn: f32,
    /// The wheel the spirograph fades from, and how far (1: done).
    spiro_from: f32,
    spiro_fade: f32,
    rd_u: Vec<f32>,
    rd_v: Vec<f32>,
    rd_u2: Vec<f32>,
    rd_v2: Vec<f32>,
    rd_clock: f32,
    title: String,
    words: Vec<String>,
    word: usize,
    word_age: f32,
    previous_word: usize,
}

impl Default for State {
    fn default() -> Self {
        State {
            seed: 7,
            last_time: 0.0,
            clock: 0.0,
            flow: Vec::new(),
            stars: Vec::new(),
            spokes: Vec::new(),
            galaxy_turn: 0.0,
            helix_spin: 0.0,
            synth_scroll: 0.0,
            buildings: Vec::new(),
            buildings_for: 0.0,
            rings: Vec::with_capacity(RINGS),
            sparks: Vec::with_capacity(SPARKS),
            last_beat: 0,
            since_ring: 0.0,
            marquee: 0.0,
            preset: 0,
            preset_before: 0,
            preset_blend: 1.0,
            preset_beat: 0,
            preset_age: 0.0,
            bands: [0.0; 32],
            morph: (3, 4, 1.0),
            morph_turn: 0.0,
            spiro: (0.4, 0.4, 0.6),
            spiro_turn: 0.0,
            spiro_from: 0.4,
            spiro_fade: 1.0,
            rd_u: Vec::new(),
            rd_v: Vec::new(),
            rd_u2: Vec::new(),
            rd_v2: Vec::new(),
            rd_clock: 0.0,
            title: String::new(),
            words: Vec::new(),
            word: 0,
            word_age: 0.0,
            previous_word: usize::MAX,
        }
    }
}

impl State {
    fn random(&mut self) -> f32 {
        random(&mut self.seed)
    }
}

pub fn draw(f: &Frame<'_>, key: &str, state: &mut VizState) {
    // the faces' clock: seconds of sound (it stands while nothing plays)
    let dt = {
        let st = &mut state.generative;
        let dt = (f.a.time - st.last_time).clamp(0.0, 0.25) as f32;
        st.last_time = f.a.time;
        dt
    };
    if let Some(shader) = Shader::of(key) {
        return shaded(f, shader, state, dt);
    }
    let thumb = state.cover_thumb.clone();
    let cv = &mut state.canvas;
    let st = &mut state.generative;
    let p = f.painter;
    match key {
        "cover" => cover(f, cv, st, thumb.as_deref()),
        "flowfield" => cv.draw(p, |pen| flowfield(f, pen, st, dt)),
        "synthwave" => cv.draw(p, |pen| synthwave(f, pen, st, dt)),
        "galaxy" => cv.draw(p, |pen| galaxy(f, pen, st, dt)),
        "skyline" => cv.draw(p, |pen| skyline(f, pen, st)),
        "isometric" => cv.draw(p, |pen| isometric(f, pen)),
        "helix" => cv.draw(p, |pen| helix(f, pen, st, dt)),
        "spirograph" => cv.draw(p, |pen| spirograph(f, pen, st, dt)),
        "morph" => cv.draw(p, |pen| morph(f, pen, st, dt)),
        "beatrings" => {
            cv.draw(p, |pen| beatrings(f, pen, st, dt));
            // the tempo the rings keep
            if f.a.beat.bpm > 0.0 && f.a.beat.confidence > 0.25 {
                p.text(
                    f.rect.left_bottom() + vec2(4.0, -4.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{:.0} BPM", f.a.beat.bpm),
                    FontId::monospace(12.0),
                    f.s.text(),
                );
            }
        }
        _ => cv.draw(p, |pen| typography(f, pen, st, dt)),
    }
}

/// A thin line from `a` to `b`, its colour from `ca` to `cb` (no feather:
/// thousands of them a frame).
fn streak(pen: &mut Pen<'_>, a: Pos2, b: Pos2, width: f32, ca: Color32, cb: Color32) {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return;
    }
    let n = vec2(-d.y, d.x) / len * (width / 2.0);
    pen.quad([a - n, b - n, b + n, a + n], [ca, cb, cb, ca]);
}

/// The bars at `x` (0..1), between them.
fn band(a: &Analyzer, x: f32) -> f32 {
    let n = a.bars.len().max(1);
    let f = x.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = f.floor() as usize;
    let j = (i + 1).min(n - 1);
    a.bars[i] + (a.bars[j] - a.bars[i]) * (f - i as f32)
}

/// A third of the bars' mean - the low, middle or high - as the analyzer
/// guards it (turning no faster than the motion allows).
fn third(a: &Analyzer, k: usize) -> f32 {
    match k {
        0 => a.energy,
        1 => a.middle,
        _ => a.treble,
    }
}

// -- the shader faces ----------------------------------------------------------------

fn shaded(f: &Frame<'_>, shader: Shader, state: &mut VizState, dt: f32) {
    let st = &mut state.generative;
    let a = f.a;
    let mut picture = false;
    let mut u = Uniforms::new(shader, a, f.s, 0.0);
    match shader {
        Shader::Reaction => {
            reaction(a, st, dt, state.gpu.picture(f.second));
            picture = true;
        }
        Shader::Feedback => {
            // a new preset each phrase (16 beats; 20 s without a beat),
            // blended into over 2.5 s
            st.preset_age += dt;
            let phrase = a.beat.count >= st.preset_beat + 16;
            if (phrase || st.preset_age > 20.0) && st.preset_blend >= 1.0 {
                st.preset_before = st.preset;
                st.preset = (st.preset + 1 + (st.random() * (PRESETS.len() - 1) as f32) as usize)
                    % PRESETS.len();
                st.preset_blend = 0.0;
                st.preset_beat = a.beat.count;
                st.preset_age = 0.0;
            }
            st.preset_blend = (st.preset_blend + dt / 2.5).min(1.0);
            let b = st.preset_blend * st.preset_blend * (3.0 - 2.0 * st.preset_blend);
            let (p0, p1) = (&PRESETS[st.preset_before], &PRESETS[st.preset]);
            let lerp = |x: f32, y: f32| x + (y - x) * b;
            // the feedback's motion, a 60th of a second's worth scaled to
            // this frame's (the same look at any frame rate): a turn that
            // swings, a zoom the bass pulls, a swirl with the middle
            let t = a.time as f32;
            let k = (dt * 60.0).clamp(0.0, 4.0);
            // (the beat's turn and the bass's pull half what they were, the
            // waves' bands eased over 0.3 s: through a chord gated at 2 to
            // 5 Hz the face read 7 by the strict reading - review 13)
            let turn = (lerp(p0.turn, p1.turn)
                + lerp(p0.swing, p1.swing) * (t * 0.13).sin()
                + 0.01 * a.beat.pulse)
                * k;
            let zoom = 1.0 - (lerp(p0.zoom, p1.zoom) + 0.5 * lerp(p0.pull, p1.pull) * a.bass) * k;
            let ease = 1.0 - (-dt / 0.3).exp();
            for (i, b) in st.bands.iter_mut().enumerate() {
                let v = &mut u.v[10 + i / 4][i % 4];
                *b += (*v - *b) * ease;
                *v = *b;
            }
            let swirl = (lerp(p0.swirl, p1.swirl) + 0.004 * third(a, 1)) * k;
            // (never 1: what it drew fades, however loud)
            let decay = (lerp(p0.decay, p1.decay) + 0.02 * a.energy.min(1.0))
                .min(0.982)
                .powf(k);
            u = u
                .with_motion([turn, zoom, swirl, decay])
                .with_extra(1, [p0.wave, p1.wave, b, lerp(p0.warp, p1.warp)]);
            u = u.with_extra(
                2,
                [lerp(p0.hue, p1.hue), lerp(p0.mirror, p1.mirror), 0.0, 0.0],
            );
        }
        _ => {}
    }
    let u = u.with_opacity(f.opacity);
    let clip = f.rect.expand(4.0);
    let (gpu, painter, second, live) = (&mut state.gpu, f.painter, f.second, f.dt > 0.0);
    state.canvas.draw(painter, |pen| {
        gpu::paint(painter, pen, gpu, clip, clip, u, picture, second, live)
    });
}

pub fn preset(st: &State) -> usize {
    st.preset
}

/// The feedback face's looks: how many.
pub const LOOKS: usize = PRESETS.len();

/// Shows the feedback face's look `k` at once, the next not before a
/// phrase (tests).
pub fn set_preset(st: &mut State, k: usize) {
    st.preset = k % PRESETS.len();
    st.preset_before = st.preset;
    st.preset_blend = 1.0;
    st.preset_age = 0.0;
}

/// A look of the feedback face (eight looks of its own - not MilkDrop's presets): its
/// motion (a 60th of a second's), the wave it draws, how its swirl folds,
/// how fast its colours turn, and whether it is mirrored.
struct Preset {
    turn: f32,
    swing: f32,
    zoom: f32,
    pull: f32,
    swirl: f32,
    decay: f32,
    /// 0 a ring of the bands, 1 a scope's line, 2 a spiral, 3 orbits, 4 a
    /// rose, 5 spokes
    wave: f32,
    warp: f32,
    hue: f32,
    /// 0 none, 1 left on right, 2 in four
    mirror: f32,
}

const PRESETS: [Preset; 8] = [
    // a halo drawn inwards
    Preset {
        turn: 0.004,
        swing: 0.012,
        zoom: 0.012,
        pull: 0.02,
        swirl: 0.0015,
        decay: 0.962,
        wave: 0.0,
        warp: 6.0,
        hue: 0.07,
        mirror: 0.0,
    },
    // a scope's line flowing outwards
    Preset {
        turn: -0.002,
        swing: 0.006,
        zoom: -0.008,
        pull: -0.01,
        swirl: 0.003,
        decay: 0.958,
        wave: 1.0,
        warp: 3.0,
        hue: 0.12,
        mirror: 0.0,
    },
    // a vortex: spiral arms turning in
    Preset {
        turn: 0.02,
        swing: 0.004,
        zoom: 0.018,
        pull: 0.015,
        swirl: 0.001,
        decay: 0.966,
        wave: 2.0,
        warp: 9.0,
        hue: 0.05,
        mirror: 0.0,
    },
    // orbits in a mirror
    Preset {
        turn: 0.006,
        swing: 0.01,
        zoom: 0.006,
        pull: 0.02,
        swirl: 0.006,
        decay: 0.962,
        wave: 3.0,
        warp: 4.0,
        hue: 0.1,
        mirror: 1.0,
    },
    // a bloom: a ring pulled in hard, mirrored, swirling finely
    Preset {
        turn: 0.0,
        swing: 0.008,
        zoom: 0.025,
        pull: 0.03,
        swirl: 0.007,
        decay: 0.958,
        wave: 0.0,
        warp: 12.0,
        hue: 0.15,
        mirror: 1.0,
    },
    // a rose in four mirrors, its petals left as lace
    Preset {
        turn: 0.008,
        swing: 0.006,
        zoom: 0.01,
        pull: 0.015,
        swirl: 0.004,
        decay: 0.968,
        wave: 4.0,
        warp: 7.0,
        hue: 0.08,
        mirror: 2.0,
    },
    // spokes of the bands, turning out into a fan
    Preset {
        turn: 0.014,
        swing: 0.008,
        zoom: -0.006,
        pull: -0.008,
        swirl: 0.002,
        decay: 0.965,
        wave: 5.0,
        warp: 5.0,
        hue: 0.06,
        mirror: 0.0,
    },
    // a halo folded in four, dense and fine
    Preset {
        turn: -0.006,
        swing: 0.01,
        zoom: 0.015,
        pull: 0.02,
        swirl: 0.005,
        decay: 0.968,
        wave: 0.0,
        warp: 10.0,
        hue: 0.11,
        mirror: 2.0,
    },
];

/// Gray-Scott reaction-diffusion on a small grid: spots and stripes that
/// grow, fed a little faster by the sound, a new seed at each beat.
fn reaction(a: &Analyzer, st: &mut State, dt: f32, picture: &std::sync::Mutex<gpu::Picture>) {
    let cells = RD_W * RD_H;
    if st.rd_u.len() != cells {
        st.rd_u = vec![1.0; cells];
        st.rd_v = vec![0.0; cells];
        st.rd_u2 = vec![1.0; cells];
        st.rd_v2 = vec![0.0; cells];
        for _ in 0..14 {
            let (x, y) = (
                (st.random() * RD_W as f32) as usize,
                (st.random() * RD_H as f32) as usize,
            );
            seed(st, x, y, 4);
        }
    }
    if a.beat.count != st.last_beat {
        st.last_beat = a.beat.count;
        let (x, y) = (
            (st.random() * RD_W as f32) as usize,
            (st.random() * RD_H as f32) as usize,
        );
        seed(st, x, y, 3);
    }
    let feed = 0.030 + 0.016 * a.energy.min(1.0);
    let kill = 0.059 + 0.003 * third(a, 2);
    st.rd_clock = (st.rd_clock + dt).min(0.2);
    // 6 steps a 30th of a second
    while st.rd_clock >= 1.0 / 180.0 {
        st.rd_clock -= 1.0 / 180.0;
        let (u, v) = (&st.rd_u, &st.rd_v);
        for y in 0..RD_H {
            let row = y * RD_W;
            let up = (y + RD_H - 1) % RD_H * RD_W;
            let down = (y + 1) % RD_H * RD_W;
            for x in 0..RD_W {
                let left = (x + RD_W - 1) % RD_W;
                let right = (x + 1) % RD_W;
                let i = row + x;
                let lap = |g: &[f32]| {
                    0.2 * (g[row + left] + g[row + right] + g[up + x] + g[down + x])
                        + 0.05 * (g[up + left] + g[up + right] + g[down + left] + g[down + right])
                        - g[i]
                };
                let (a, b) = (u[i], v[i]);
                let abb = a * b * b;
                st.rd_u2[i] = (a + lap(u) - abb + feed * (1.0 - a)).clamp(0.0, 1.0);
                st.rd_v2[i] = (b + 0.5 * lap(v) + abb - (feed + kill) * b).clamp(0.0, 1.0);
            }
        }
        std::mem::swap(&mut st.rd_u, &mut st.rd_u2);
        std::mem::swap(&mut st.rd_v, &mut st.rd_v2);
    }
    if let Ok(mut pic) = picture.lock() {
        if pic.data.len() != cells {
            pic.data.clear();
            pic.data.resize(cells, 0);
            pic.width = RD_W as u32;
            pic.height = RD_H as u32;
        }
        for (d, v) in pic.data.iter_mut().zip(&st.rd_v) {
            *d = (v * 2.5 * 255.0).min(255.0) as u8;
        }
        pic.version = pic.version.wrapping_add(1);
    }
}

fn seed(st: &mut State, x: usize, y: usize, r: usize) {
    for dy in 0..=2 * r {
        for dx in 0..=2 * r {
            // (a round seed: a square one stays a square for a while)
            let (ox, oy) = (dx as i32 - r as i32, dy as i32 - r as i32);
            if ox * ox + oy * oy > (r * r) as i32 {
                continue;
            }
            let (xx, yy) = ((x + dx + RD_W - r) % RD_W, (y + dy + RD_H - r) % RD_H);
            st.rd_v[yy * RD_W + xx] = 0.9;
            st.rd_u[yy * RD_W + xx] = 0.3;
        }
    }
}

// -- the cover -----------------------------------------------------------------------

/// The cover's side in the cover face of `r` (its inner rect).
/// How patterned a cover is (0..1): how much its small picture's
/// neighbouring cells differ in luminance.
fn patterned(t: &super::Thumb) -> f32 {
    let n = t.n;
    let y = |i: usize| {
        let c = t.rgb[i];
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    };
    let mut sum = 0.0;
    for row in 0..n {
        for col in 0..n - 1 {
            sum += (y(row * n + col) - y(row * n + col + 1)).abs();
            sum += (y(col * n + row) - y((col + 1) * n + row)).abs();
        }
    }
    (sum / (2 * n * (n - 1)) as f32 * 4.0).min(1.0)
}

pub fn cover_side(r: Rect) -> f32 {
    // (at most 30 % of the picture: a cover of stripes or checks, standing,
    // stays well under BT.1702's 40 %)
    (r.height() * 0.84)
        .min(r.width() * 0.62)
        .min((r.width() * r.height() * 0.3).sqrt())
}

/// The cover large, as the picture: dimmed behind it, a glow round it that
/// swells with the bass, a ring at each beat, the spectrum faint below.
fn cover(f: &Frame<'_>, cv: &mut super::Canvas, st: &mut State, thumb: Option<&super::Thumb>) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let p = f.painter;
    let side = cover_side(r);
    let c = r.center();
    let art = Rect::from_center_size(c, vec2(side, side));
    let beat = a.beat.count != st.last_beat;
    st.last_beat = a.beat.count;
    if beat {
        st.since_ring = 0.0;
    }
    st.since_ring += f.dt;
    if let Some((texture, size)) = f.cover {
        // the cover over the whole picture, dark: the room it plays in
        let aspect = size.x / size.y.max(1.0);
        let wide = r.width() / r.height().max(1.0);
        let uv = if aspect > wide {
            let k = wide / aspect;
            Rect::from_min_max(pos2(0.5 - k / 2.0, 0.0), pos2(0.5 + k / 2.0, 1.0))
        } else {
            let k = aspect / wide;
            Rect::from_min_max(pos2(0.0, 0.5 - k / 2.0), pos2(1.0, 0.5 + k / 2.0))
        };
        match thumb {
            // (blurred: the cover made small, between its cells - a cover
            // of checks, drawn large under a glow that swells with the
            // bass, lay in stripes over the whole picture)
            Some(t) => cv.draw(p, |pen| {
                const N: usize = 12;
                let t = t.smaller(4);
                let i = pen.mesh.vertices.len() as u32;
                for y in 0..=N {
                    for x in 0..=N {
                        let (fx, fy) = (x as f32 / N as f32, y as f32 / N as f32);
                        let c = t.at_srgb(uv.left() + fx * uv.width(), uv.top() + fy * uv.height());
                        let k = 64.0 / 255.0;
                        let byte = |v: f32| (v * k * 255.0).round() as u8;
                        pen.mesh.colored_vertex(
                            pos2(r.left() + fx * r.width(), r.top() + fy * r.height()),
                            Color32::from_rgb(byte(c[0]), byte(c[1]), byte(c[2])),
                        );
                    }
                }
                for y in 0..N as u32 {
                    for x in 0..N as u32 {
                        let a = i + y * (N as u32 + 1) + x;
                        let b = a + N as u32 + 1;
                        pen.mesh.add_triangle(a, a + 1, b + 1);
                        pen.mesh.add_triangle(a, b + 1, b);
                    }
                }
            }),
            None => {
                p.image(texture, r, uv, Color32::from_gray(64));
            }
        }
    }
    cv.draw(p, |pen| {
        pen.rect(r, Color32::from_rgba_premultiplied(0, 0, 0, 90));
        // (the glow swells with the bass - less over a patterned cover)
        let pattern = thumb.map_or(0.0, patterned);
        let swell = 1.0 + (0.18 * a.bass + 0.12 * a.beat.pulse) * (1.0 - 0.8 * pattern);
        pen.radial(
            c,
            side * 0.78 * swell,
            glow(s.mid, 0.55),
            Color32::TRANSPARENT,
        );
        pen.radial(
            c,
            side * 0.62,
            glow(s.high, 0.25 * a.energy.min(1.0)),
            Color32::TRANSPARENT,
        );
        // a ring that leaves the cover at each beat
        let k = (st.since_ring / 0.9).min(1.0);
        if k < 1.0 {
            pen.ring(
                c,
                side * (0.55 + 0.4 * k),
                1.0 + 2.0 * (1.0 - k),
                glow(mix(s.mid, s.peak, 0.3), (1.0 - k) * 0.6),
            );
        }
        // the spectrum, faint, along the bottom
        let n = a.bars.len().max(1);
        for i in 0..n {
            let x = r.left() + (i as f32 + 0.5) / n as f32 * r.width();
            let v = a.bars[i];
            pen.rect(
                Rect::from_min_max(
                    pos2(x - 1.5, r.bottom() - v * r.height() * 0.18),
                    pos2(x + 1.5, r.bottom()),
                ),
                glow(s.at(v), 0.35),
            );
        }
    });
    // (a soft shadow under it)
    p.add(
        egui::epaint::Shadow {
            offset: [0, 10],
            blur: 32,
            spread: 2,
            color: Color32::from_black_alpha(170),
        }
        .as_shape(art, egui::CornerRadius::same(12)),
    );
    match f.cover {
        Some((texture, _)) => {
            p.add(
                egui::epaint::RectShape::filled(art, egui::CornerRadius::same(12), Color32::WHITE)
                    .with_texture(texture, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
            );
        }
        None => {
            // no cover: a record of the scheme's colours, its light turning
            // slowly (not a giant note on a flat square)
            let t = f.a.time as f32;
            cv.draw(p, |pen| {
                pen.vertical(
                    art,
                    mix(s.background, s.low, 0.30),
                    mix(s.background, s.mid, 0.18),
                );
                pen.circle(c, side * 0.47, mix(s.background, Color32::BLACK, 0.35));
                for k in 0..16 {
                    pen.ring(
                        c,
                        side * (0.17 + k as f32 * 0.018),
                        1.0,
                        glow(s.peak, 0.05 + 0.02 * (k % 3) as f32),
                    );
                }
                let sheen = c + Vec2::angled(t * 0.3) * side * 0.22;
                pen.radial(sheen, side * 0.35, glow(s.high, 0.16), Color32::TRANSPARENT);
                pen.circle(c, side * 0.14, mix(s.mid, s.high, 0.35));
                pen.circle(c, side * 0.015, s.background);
            });
            p.text(
                c + vec2(0.0, -side * 0.06),
                egui::Align2::CENTER_CENTER,
                "♫",
                FontId::proportional(side * 0.08),
                mix(s.mid, s.background, 0.6),
            );
        }
    }
}

// -- the flow field ------------------------------------------------------------------

fn flowfield(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let aspect = r.width() / r.height().max(1.0);
    if st.flow.len() != FLOW_PARTICLES {
        st.flow.clear();
        for _ in 0..FLOW_PARTICLES {
            let p = pos2(st.random() * aspect, st.random());
            let life = 2.0 + 4.0 * st.random();
            let age = st.random() * life;
            st.flow.push(Particle {
                trail: [p; TRAIL],
                head: 0,
                age,
                life,
            });
        }
    }
    let t = a.time as f32;
    let bass = a.bass;
    let speed = 0.05 + 0.35 * a.energy + 0.4 * a.beat.pulse;
    st.clock += dt;
    let record = st.clock >= 1.0 / 30.0;
    if record {
        st.clock %= 1.0 / 30.0;
    }
    for k in 0..FLOW_PARTICLES {
        let mut q = st.flow[k];
        let head = q.trail[q.head];
        let angle = (head.x * 2.1 + t * 0.21).sin() * 1.9
            + (head.y * 2.7 - t * 0.17).cos() * 2.1
            + ((head.x + head.y) * 5.0 + t * 0.6).sin() * (0.4 + 1.2 * bass);
        let next = head + vec2(angle.cos(), angle.sin()) * speed * dt;
        q.age += dt;
        let gone = next.x < -0.02 || next.x > aspect + 0.02 || next.y < -0.02 || next.y > 1.02;
        if gone || q.age > q.life {
            let p = pos2(st.random() * aspect, st.random());
            q = Particle {
                trail: [p; TRAIL],
                head: 0,
                age: 0.0,
                life: 2.0 + 4.0 * st.random(),
            };
        } else if record {
            q.head = (q.head + 1) % TRAIL;
            q.trail[q.head] = next;
        } else {
            q.trail[q.head] = next;
        }
        st.flow[k] = q;
    }
    let place = |p: Pos2| r.min + p.to_vec2() * r.height();
    // (in the starkest schemes - white on black - the lines a shade under
    // full: thousands of white hairs on black lie in rows the eye reads as
    // a moving grating)
    let reach = 1.0 - 0.65 * s.stark();
    // (at a slow renderer's rate a frame's step of each hair is long and
    // straight: the hairs lay in rows, standing stripes over 40 % of the
    // picture at 12 frames a second - there a shade fainter)
    let slow = if dt > 0.0 {
        (1.0 / 30.0 / dt).min(1.0)
    } else {
        1.0
    };
    let reach = reach * (0.6 + 0.4 * slow);
    for (k, q) in st.flow.iter().enumerate() {
        // fading in as it is born and out as it ends
        let life = (q.age / 0.5).min(1.0) * ((q.life - q.age) / 0.8).clamp(0.0, 1.0) * reach;
        let hue =
            s.at(
                0.25 + 0.75 * ((k as f32 * 0.618).fract() * 0.6 + band(a, q.trail[q.head].y) * 0.4)
            );
        // (in the starkest schemes faint: hundreds of hairs, black on
        // white, flowed in rows - moving stripes over a quarter of it)
        let hue = if s.stark() > 0.5 {
            super::faint(hue, s.background)
        } else {
            hue
        };
        for j in 1..TRAIL {
            let i0 = (q.head + TRAIL - j) % TRAIL;
            let i1 = (q.head + TRAIL - j + 1) % TRAIL;
            let k0 = (1.0 - j as f32 / TRAIL as f32) * life;
            let k1 = (1.0 - (j - 1) as f32 / TRAIL as f32) * life;
            streak(
                pen,
                place(q.trail[i0]),
                place(q.trail[i1]),
                1.4,
                mix(s.background, hue, k0),
                mix(s.background, hue, k1),
            );
        }
    }
}

// -- synthwave -----------------------------------------------------------------------

fn synthwave(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let (w, h) = (r.width(), r.height());
    let horizon = r.top() + h * 0.56;
    let sky = Rect::from_min_max(r.min, pos2(r.right(), horizon));
    pen.vertical(sky, s.background, mix(s.background, s.high, 0.28));
    // stars
    for k in 0..70u32 {
        let x = r.left() + hash(k, 1) * w;
        let y = r.top() + hash(k, 2) * (horizon - r.top()) * 0.8;
        let twinkle = 0.4
            + 0.6
                * ((a.time as f32) * (1.0 + hash(k, 3) * 3.0) + k as f32)
                    .sin()
                    .abs();
        pen.rect(
            Rect::from_center_size(pos2(x, y), vec2(1.5, 1.5)),
            mix(s.background, s.peak, twinkle * 0.7),
        );
    }
    // the sun: stripes cut into its lower half, growing with the bass
    let radius = w.min(h) * 0.22 * (1.0 + 0.18 * a.bass + 0.08 * a.beat.pulse);
    let c = pos2(r.center().x, horizon - radius * 0.38);
    pen.radial(c, radius * 1.7, glow(s.high, 0.45), Color32::TRANSPARENT);
    let stripes = 22;
    for i in 0..stripes {
        let y0 = c.y - radius + 2.0 * radius * i as f32 / stripes as f32;
        let y1 = c.y - radius + 2.0 * radius * (i + 1) as f32 / stripes as f32;
        if y0 >= horizon {
            break;
        }
        let mid = (y0 + y1) / 2.0;
        // the gaps widen towards the bottom and move down
        let gap = if mid > c.y - radius * 0.1 {
            let k = (mid - (c.y - radius * 0.1)) / (radius * 1.1);
            (k * 0.8 + 0.1 * ((a.time as f32) * 2.0).sin().abs()).min(0.9)
        } else {
            0.0
        };
        let top = y0;
        let bottom = (y1 - (y1 - y0) * gap).min(horizon);
        let half = |y: f32| (radius * radius - (y - c.y) * (y - c.y)).max(0.0).sqrt();
        let colour = |y: f32| {
            let t = (y - (c.y - radius)) / (2.0 * radius);
            mix(s.peak, s.high, t.clamp(0.0, 1.0))
        };
        // (the round edge followed in slices of a pixel or two, each a
        // trapezoid with a pixel's soft edge: no steps)
        let slices = ((bottom - top) / 2.0).ceil().max(1.0) as usize;
        for k in 0..slices {
            let ya = top + (bottom - top) * k as f32 / slices as f32;
            let yb = top + (bottom - top) * (k + 1) as f32 / slices as f32;
            let (wa, wb) = (half(ya), half(yb));
            let (ca, cb) = (colour(ya), colour(yb));
            pen.quad(
                [
                    pos2(c.x - wa, ya),
                    pos2(c.x + wa, ya),
                    pos2(c.x + wb, yb),
                    pos2(c.x - wb, yb),
                ],
                [ca, ca, cb, cb],
            );
            for side in [-1.0f32, 1.0] {
                pen.quad(
                    [
                        pos2(c.x + side * wa, ya),
                        pos2(c.x + side * (wa + 1.0), ya),
                        pos2(c.x + side * (wb + 1.0), yb),
                        pos2(c.x + side * wb, yb),
                    ],
                    [ca, Color32::TRANSPARENT, Color32::TRANSPARENT, cb],
                );
            }
        }
    }
    // mountains from the spectrum, mirrored round the middle
    let n = 48;
    pen.points.clear();
    for k in 0..=n {
        let u = k as f32 / n as f32;
        let v = band(a, (2.0 * u - 1.0).abs() * 0.9);
        let ridge = (u * 23.0).sin().abs() * 0.3 + 0.7;
        pen.points
            .push(pos2(r.left() + u * w, horizon - v * ridge * h * 0.2 - 2.0));
    }
    let dark = mix(s.background, s.low, 0.22);
    pen.under(horizon, dark, |_| dark);
    pen.line(1.5, mix(s.background, s.low, 0.9));
    // the floor: lines to the vanishing point, rows coming nearer
    let floor = Rect::from_min_max(pos2(r.left(), horizon), r.max);
    pen.vertical(floor, mix(s.background, s.mid, 0.12), s.background);
    st.synth_scroll = (st.synth_scroll + dt * (0.5 + 2.5 * a.bass + a.beat.pulse)) % 1.0;
    // (the lines to the vanishing point faint: 29 of them, side by side,
    // under rows scrolling over them, were a moving grating)
    let line_colour = super::faint(mix(s.background, s.mid, 0.85), s.background);
    let vanish = pos2(r.center().x, horizon);
    for k in -14..=14 {
        let x = r.center().x + k as f32 * w * 0.16;
        pen.segment(
            vanish + (pos2(x, r.bottom()) - vanish) * 0.02,
            pos2(x, r.bottom()),
            1.0,
            line_colour,
        );
    }
    // the rows coming nearer: antialiased lines (rects at a fraction of a
    // pixel flickered), thicker and brighter as they come, with a glow
    for k in 0..16 {
        let z = k as f32 + 1.0 - st.synth_scroll;
        let y = horizon + (r.bottom() - horizon) * (1.0 / z).min(1.0) * 0.98;
        let near = (1.0 / z).sqrt();
        let colour = mix(s.background, s.mid, near * (0.6 + 0.4 * a.beat.pulse));
        // (the far rows, packed together near the horizon, faint: they lay
        // in a grating there, scrolling)
        let colour = if near < 0.7 {
            super::faint(colour, s.background)
        } else {
            colour
        };
        let (left, right) = (pos2(r.left(), y), pos2(r.right(), y));
        pen.segment(left, right, 4.0 * near, glow(s.mid, 0.12 * near));
        pen.segment(left, right, 0.6 + 1.2 * near, colour);
    }
    pen.rect(
        Rect::from_x_y_ranges(r.x_range(), horizon - 1.0..=horizon + 1.0),
        s.high,
    );
}

/// A fixed random number 0..1 for (k, salt).
fn hash(k: u32, salt: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

// -- the galaxy ----------------------------------------------------------------------

fn galaxy(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    if st.stars.len() != GALAXY_STARS {
        st.stars.clear();
        for k in 0..GALAXY_STARS {
            let arm = (k % 3) as f32;
            let radius = st.random().powf(0.7);
            let spread = (st.random() - 0.5) * (0.5 / (radius + 0.25));
            let angle = arm * TAU / 3.0 + radius * 5.0 + spread;
            let bright = 0.3 + 0.7 * st.random();
            let tint = st.random();
            st.stars.push([radius, angle, bright, tint]);
        }
    }
    st.galaxy_turn += dt * (0.06 + 0.5 * a.energy + 0.3 * a.beat.pulse);
    let c = r.center();
    let size = r.width().min(r.height() * 1.6) * 0.48;
    // the core, swelling a little with the bass (less in the starkest
    // schemes: its white heart grown and shrunk by a kick at 240 BPM, and
    // the inner stars with it, were seven changes a second in Intense)
    let calm = super::scheme_calm(s);
    let core = size * (0.13 + (0.06 * a.bass + 0.02 * a.beat.pulse) / calm);
    pen.radial(c, core * 4.0, glow(s.mid, 0.55), Color32::TRANSPARENT);
    pen.radial(c, core * 1.2, glow(s.peak, 0.8), Color32::TRANSPARENT);
    for [radius, angle, bright, tint] in &st.stars {
        // inner stars turn faster
        let turn = angle + st.galaxy_turn * (1.0 / (radius + 0.3));
        let level = band(a, *radius);
        let p = c + vec2(turn.cos(), turn.sin() * 0.55) * (radius * size);
        let k = bright * (0.4 + 0.8 * level / calm);
        let colour = s.at(0.2 + 0.8 * (1.0 - radius) * 0.6 + tint * 0.3);
        let d = 1.0 + 1.6 * level * bright;
        pen.rect(Rect::from_center_size(p, vec2(d, d)), glow(colour, k));
    }
}

// -- the skyline ---------------------------------------------------------------------

fn skyline(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let (w, h) = (r.width(), r.height());
    let water = r.top() + h * 0.78;
    if (st.buildings_for - w).abs() > 1.0 || st.buildings.is_empty() {
        st.buildings_for = w;
        st.buildings.clear();
        for back in [true, false] {
            let mut x = -10.0;
            let mut k = 0u32;
            while x < w + 10.0 {
                let width = (16.0 + 40.0 * hash(k, back as u32 + 7)) * (w / 620.0).clamp(0.6, 2.5);
                st.buildings.push(Building {
                    x,
                    width,
                    height: 0.25 + 0.5 * hash(k, back as u32 + 11),
                    seed: k * 31 + back as u32,
                    back,
                });
                x += width + if back { 0.0 } else { 3.0 + 8.0 * hash(k, 13) };
                k += 1;
            }
        }
    }
    // the sky, its stars and the moon
    pen.vertical(
        Rect::from_min_max(r.min, pos2(r.right(), water)),
        s.background,
        mix(s.background, s.low, 0.35),
    );
    let t = a.time as f32;
    for k in 0..90u32 {
        let x = r.left() + hash(k, 21) * w;
        let y = r.top() + hash(k, 22) * (water - r.top()) * 0.6;
        let twinkle = 0.5 + 0.5 * (t * (0.7 + 2.0 * hash(k, 23)) + k as f32).sin();
        pen.rect(
            Rect::from_center_size(pos2(x, y), vec2(1.2, 1.2)),
            mix(s.background, s.peak, 0.15 + 0.45 * twinkle * hash(k, 24)),
        );
    }
    let moon = pos2(r.left() + w * 0.8, r.top() + h * 0.2);
    pen.radial(
        moon,
        h * 0.2 * (1.0 + 0.3 * a.beat.pulse),
        glow(s.peak, 0.3),
        Color32::TRANSPARENT,
    );
    pen.circle(moon, h * 0.055, mix(s.peak, s.high, 0.15));
    // searchlights from behind the city, sweeping, brighter as it gets loud
    for k in 0..3u32 {
        let foot = pos2(r.left() + w * (0.2 + 0.3 * k as f32), water);
        let angle = (t * (0.25 + 0.1 * k as f32) + k as f32 * 2.1).sin() * 0.55;
        let reach = h * 1.1;
        let dir = vec2(angle.sin(), -angle.cos());
        let side = vec2(-dir.y, dir.x) * reach * 0.09;
        let tip = foot + dir * reach;
        // (soft: a bright core in a wider haze, fading out as it goes)
        let strength = 0.04 + 0.12 * a.energy.min(1.0) + 0.06 * a.beat.pulse;
        for (wide, k) in [(1.0f32, 0.35), (0.55, 0.45), (0.22, 0.7)] {
            let beam = glow(mix(s.high, s.peak, 0.3), strength * k);
            pen.quad(
                [foot, foot, tip + side * wide, tip - side * wide],
                [beam, beam, Color32::TRANSPARENT, Color32::TRANSPARENT],
            );
        }
    }
    let step = (a.time * 6.0) as u32;
    let treble = third(a, 2);
    let draw_city = |pen: &mut Pen<'_>, mirror: bool| {
        for b in &st.buildings {
            let level = band(a, ((b.x + b.width / 2.0) / w).clamp(0.0, 1.0));
            let height = h * 0.62 * b.height * if b.back { 1.15 } else { 0.55 + 0.6 * level };
            let (top, bottom) = if mirror {
                (water, water + height * 0.35)
            } else {
                (water - height, water)
            };
            let body = Rect::from_min_max(
                pos2(r.left() + b.x, top),
                pos2(r.left() + b.x + b.width, bottom),
            );
            let colour = if b.back {
                mix(s.background, s.low, 0.18)
            } else {
                mix(s.background, s.low, 0.32)
            };
            let colour = if mirror {
                mix(s.background, colour, 0.5)
            } else {
                colour
            };
            pen.rect(body, colour);
            if b.back {
                // a few dim windows far off
                if !mirror {
                    for j in 0..((height - 10.0) / 9.0).max(0.0) as u32 {
                        let lit = hash(b.seed * 131 + j, step / 12);
                        if lit < 0.3 {
                            let x = body.left() + 2.0 + lit * (b.width - 6.0).max(0.0) * 3.0;
                            let y = body.top() + 6.0 + j as f32 * 9.0;
                            pen.rect(
                                Rect::from_min_size(
                                    pos2(x.min(body.right() - 4.0), y),
                                    vec2(2.0, 3.0),
                                ),
                                mix(s.background, s.high, 0.35),
                            );
                        }
                    }
                }
                continue;
            }
            // the windows: more of them lit with the treble, flickering
            let (cols, rows) = (
                ((b.width - 4.0) / 5.0) as u32,
                ((height - 6.0) / 7.0) as u32,
            );
            for i in 0..cols {
                for j in 0..rows {
                    let lit = hash(b.seed * 977 + i * 31 + j, step / 3 + (i + j) % 5);
                    if lit < 0.25 + 0.6 * treble * level.max(0.3) {
                        let x = body.left() + 3.0 + i as f32 * 5.0;
                        let c = if lit < 0.05 {
                            s.peak
                        } else {
                            s.at(0.55 + 0.45 * lit)
                        };
                        if mirror {
                            // (in the water: a streak that shivers)
                            let y = body.top() + 2.0 + (j as f32 * 7.0) * 0.35;
                            if y < body.bottom() {
                                let shift = (y * 0.5 + t * 3.0).sin() * 1.5;
                                pen.rect(
                                    Rect::from_min_size(pos2(x + shift, y), vec2(2.5, 1.5)),
                                    glow(c, 0.45),
                                );
                            }
                        } else {
                            let y = body.bottom() - 6.0 - (j + 1) as f32 * 7.0 + 3.0;
                            pen.rect(Rect::from_min_size(pos2(x, y), vec2(2.5, 3.5)), c);
                        }
                    }
                }
            }
            if mirror {
                continue;
            }
            // the roof's neon, lit by its band
            let roof = s.at(0.4 + 0.6 * level);
            let (l, rr) = (
                pos2(body.left(), body.top()),
                pos2(body.right(), body.top()),
            );
            pen.segment(l, rr, 6.0, glow(roof, 0.12 + 0.3 * level));
            pen.segment(l, rr, 1.5, mix(s.background, roof, 0.4 + 0.6 * level));
            // a mast on some, its light blinking on the beat
            if b.seed % 3 == 0 && b.width > 22.0 {
                let foot = pos2(body.center().x, body.top());
                let tip = foot - vec2(0.0, 8.0 + 10.0 * hash(b.seed, 3));
                pen.segment(foot, tip, 1.0, mix(s.background, s.low, 0.6));
                let blink = if (a.beat.count + b.seed as u64).is_multiple_of(2) {
                    a.beat.pulse
                } else {
                    0.15
                };
                pen.radial(
                    tip,
                    6.0,
                    glow(s.high, 0.2 + 0.7 * blink),
                    Color32::TRANSPARENT,
                );
                pen.circle(tip, 1.4, mix(s.background, s.high, 0.4 + 0.6 * blink));
            }
        }
    };
    draw_city(pen, false);
    // the road along the water: lights coming and going
    let speed = 40.0 + 220.0 * a.energy.min(1.0);
    for k in 0..36u32 {
        let way = if k % 2 == 0 { 1.0 } else { -1.0 };
        let x = (hash(k, 31) * w + way * t * speed * (0.7 + 0.6 * hash(k, 32))).rem_euclid(w);
        let c = if way > 0.0 { s.peak } else { s.high };
        let y = water - 2.0 + if way > 0.0 { 0.0 } else { 1.5 };
        pen.rect(
            Rect::from_min_size(pos2(r.left() + x, y), vec2(3.0, 1.2)),
            mix(s.background, c, 0.8),
        );
    }
    // the water: the city's reflection, broken into rippling lines
    pen.rect(
        Rect::from_min_max(pos2(r.left(), water), r.max),
        mix(s.background, s.low, 0.08),
    );
    draw_city(pen, true);
    let mut y = water + 2.0;
    let mut k = 0;
    while y < r.bottom() {
        let shift = (y * 0.3 + t * 2.0).sin() * 6.0;
        let len = w * (0.1 + 0.2 * hash(k, 3));
        let x = r.left() + (hash(k, 5) * w + shift + t * 10.0 * hash(k, 9)) % w;
        pen.rect(
            Rect::from_min_size(pos2(x, y), vec2(len, 1.0)),
            mix(s.background, s.mid, 0.25 + 0.3 * a.energy),
        );
        y += 4.0 + 3.0 * hash(k, 7);
        k += 1;
    }
}

// -- isometric bars ------------------------------------------------------------------

fn isometric(f: &Frame<'_>, pen: &mut Pen<'_>) {
    let (a, s, r) = (f.a, f.s, f.rect);
    const COLS: usize = 16;
    const ROWS: usize = 14;
    let n = a.bars.len().max(1);
    // the whole of it within the picture: the floor's width, and its depth
    // with the tallest box on it
    let tall = r.height() * 0.34;
    let span = (COLS + ROWS) as f32;
    let cell = (r.width() * 0.94 / (span * 0.5)).min((r.height() * 0.94 - tall) / (span * 0.2));
    let (cw, ch) = (cell * 0.5, cell * 0.2);
    let origin = pos2(
        r.center().x - (COLS as f32 - ROWS as f32) * cw * 0.5,
        r.bottom() - 4.0 - span * ch,
    );
    // back (far, old) to front: each box over those behind it
    for sum in 0..COLS + ROWS - 1 {
        for i in 0..COLS {
            if sum < i || sum - i >= ROWS {
                continue;
            }
            let j = sum - i;
            let row = a.history_row((ROWS - 1 - j) * 2);
            let from = i * n / COLS;
            let to = ((i + 1) * n / COLS).max(from + 1).min(n);
            let v = row[from..to].iter().cloned().fold(0.0, f32::max);
            let base = origin + vec2((i as f32 - j as f32) * cw, (i + j) as f32 * ch);
            let height = 2.0 + v * tall * (0.5 + 0.5 * j as f32 / ROWS as f32);
            let top = base - vec2(0.0, height);
            let colour = s.at(v);
            let fade = 0.35 + 0.65 * j as f32 / (ROWS - 1) as f32;
            let lit = |k: f32| mix(s.background, colour, (k * fade).min(1.0));
            let (l, rr, front, back) = (
                vec2(-cw, ch),
                vec2(cw, ch),
                vec2(0.0, 2.0 * ch),
                vec2(0.0, 0.0),
            );
            // the left and right faces, then the top
            pen.quad(
                [top + l, top + front, base + front, base + l],
                [lit(0.55); 4],
            );
            pen.quad(
                [top + front, top + rr, base + rr, base + front],
                [lit(0.8); 4],
            );
            pen.quad(
                [top + back, top + rr, top + front, top + l],
                [lit(1.0), lit(1.0), lit(1.15), lit(1.0)],
            );
        }
    }
}

// -- the DNA helix -------------------------------------------------------------------

fn helix(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    // (never saturated red: its lines, in a pure red scheme, flashed red
    // eight or nine times a second in Intense)
    let unred = f.s.unred();
    let (a, s, r) = (f.a, &unred, f.rect);
    let t = a.time as f32;
    const POINTS: usize = 140;
    let amp = r.height() * 0.36 * (0.85 + 0.2 * a.bass + 0.1 * a.beat.pulse);
    let cy = r.center().y;
    // (turning faster as it gets louder, by a turn kept from frame to frame:
    // `time * (0.5 + 1.4 * energy)` leapt at each change of the energy,
    // the further the longer it had played)
    st.helix_spin = (st.helix_spin + dt * (0.5 + 1.4 * a.energy)) % (TAU * 1000.0);
    let spin = st.helix_spin;
    // dust drifting behind, lit by the treble
    let treble = third(a, 2);
    for k in 0..90u32 {
        let x = r.left() + fract(hash(k, 41) + t * 0.01 * (0.5 + hash(k, 43))) * r.width();
        let y = r.top() + hash(k, 47) * r.height();
        let size = 0.8 + 1.8 * hash(k, 53);
        pen.circle(
            pos2(x, y),
            size,
            glow(s.low, 0.15 + 0.4 * treble * hash(k, 59)),
        );
    }
    let at = |k: usize, strand: f32| {
        let u = k as f32 / (POINTS - 1) as f32;
        // (1.7 turns across: at 2.4 a row crossed the strands ten times
        // at even steps - a standing grating over half the picture)
        let theta = u * TAU * 1.7 + spin + strand * PI;
        // (a slow sway along it, as if seen from a turning camera)
        let wobble = (u * 5.0 + t * 0.5).sin() * r.height() * 0.04;
        (
            pos2(r.left() + u * r.width(), cy + theta.sin() * amp + wobble),
            theta.cos(),
        )
    };
    // far parts first; each part of a strand a glowing tube, lit in front
    for near in [false, true] {
        for k in 0..POINTS - 1 {
            if k % 4 == 0 {
                let (p, z) = at(k, 0.0);
                let (q, _) = at(k, 1.0);
                if (z > 0.0) == near {
                    // the base pair: two halves, each its band's colour
                    let level = band(a, k as f32 / (POINTS - 1) as f32);
                    let depth = 0.35 + 0.65 * (0.5 + 0.5 * z.abs());
                    let mid = p + (q - p) * 0.5;
                    let w = 2.0 + 4.0 * level;
                    // (faint: 35 rungs side by side, turning, were a
                    // moving grating)
                    let (c1, c2) = (
                        super::faint(s.at(0.3 + 0.7 * level), s.background),
                        super::faint(s.at(1.0 - 0.6 * level), s.background),
                    );
                    // (no glow round them, and half as strong: lying over
                    // the strands' haze, 35 glowing rungs were a standing
                    // grating over half the picture in the editor's own
                    // colours)
                    pen.segment(p, mid, w * 0.6, mix(s.background, c1, depth * 0.5));
                    pen.segment(mid, q, w * 0.6, mix(s.background, c2, depth * 0.5));
                }
            }
            for strand in [0.0, 1.0] {
                let (p, z) = at(k, strand);
                let (q, z2) = at(k + 1, strand);
                if (z + z2 > 0.0) != near {
                    continue;
                }
                let depth = 0.5 + 0.25 * (z + z2);
                let hue = if strand == 0.0 { s.mid } else { s.high };
                let width = 3.0 + 6.0 * depth;
                pen.segment(p, q, width * 2.6, glow(hue, 0.1 * depth));
                pen.segment(p, q, width, mix(s.background, hue, 0.25 + 0.75 * depth));
                pen.segment(p, q, width * 0.3, mix(hue, s.peak, 0.55 * depth));
            }
        }
    }
    // the bases at the strands, in front
    for k in (0..POINTS).step_by(4) {
        for strand in [0.0, 1.0] {
            let (p, z) = at(k, strand);
            if z > 0.0 {
                let level = band(a, k as f32 / (POINTS - 1) as f32);
                pen.radial(
                    p,
                    6.0 + 10.0 * level * z,
                    glow(s.peak, 0.35 * z),
                    Color32::TRANSPARENT,
                );
                pen.circle(p, 2.0 + 3.0 * z, mix(s.at(level), s.peak, 0.5 * z));
            }
        }
    }
}

/// The fractional part.
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// The line in `pen.points` with a glow round it: wide and faint, less wide,
/// then the line itself.
fn glowing_line(pen: &mut Pen<'_>, width: f32, colour: Color32, strength: f32) {
    pen.line(width * 6.0, glow(colour, 0.08 * strength));
    pen.line(width * 2.5, glow(colour, 0.22 * strength));
    pen.line(width, colour);
}

// -- the spirograph ------------------------------------------------------------------

fn spirograph(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    // the wheel's size changes now and then on the beat: the curve of the
    // new one fades in over the old in two seconds (it once followed the
    // size itself - the far laps of a dense curve reshuffle at the least
    // change of it, and the picture flickered 25 times a second)
    const RATIOS: [f32; 8] = [
        3.0 / 7.0,
        2.0 / 5.0,
        5.0 / 8.0,
        3.0 / 8.0,
        4.0 / 9.0,
        5.0 / 12.0,
        2.0 / 7.0,
        7.0 / 12.0,
    ];
    if a.beat.count != st.last_beat {
        st.last_beat = a.beat.count;
        let next = RATIOS[(a.beat.count / 8) as usize % RATIOS.len()];
        if a.beat.count % 8 == 0 && next != st.spiro.1 && st.spiro_fade >= 1.0 {
            st.spiro_from = st.spiro.1;
            st.spiro.1 = next;
            st.spiro_fade = 0.0;
        }
    }
    st.spiro_fade = (st.spiro_fade + dt / 2.0).min(1.0);
    st.spiro.0 = st.spiro.1;
    // (its pen and its turn slow: the dense curve, turning or changing
    // shape, lit and darkened its pixels together, nine or ten times a
    // second by the strict reading)
    let follow = 1.0 - (-dt * 0.4).exp();
    st.spiro.2 += ((0.45 + 0.5 * a.energy) - st.spiro.2) * follow;
    st.spiro_turn += dt * (0.04 + 0.1 * a.energy);
    let c = r.center();
    // as large as the picture: the curves and, behind, two larger copies
    // that run off its sides
    let size = r.height() / 2.0 - 4.0;
    let fade = st.spiro_fade;
    let wheels = [(st.spiro_from, 1.0 - fade), (st.spiro.1, fade)];
    for (wheel, weight) in wheels {
        if weight < 0.01 {
            continue;
        }
        for layer in 0..5usize {
            let big = match layer {
                0 => 2.4,
                1 => 1.7,
                _ => 1.0,
            };
            let inner = layer.saturating_sub(2);
            let ratio = wheel * (1.0 + 0.07 * inner as f32);
            let d = st.spiro.2 * (1.0 - 0.15 * inner as f32);
            let scale = size * big / (1.0 - ratio + d).max(0.1);
            let turn = st.spiro_turn
                * (1.0 + 0.3 * inner as f32)
                * if layer % 2 == 0 { 1.0 } else { -1.0 };
            // (while two wheels show, each with half the points: the
            // frame costs what one did)
            let steps: usize = if fade < 1.0 { 490 } else { 980 };
            let laps = 14.0;
            let chunks = 28;
            let faint = if layer < 2 { 0.35 } else { 1.0 } * weight;
            for chunk in 0..chunks {
                pen.points.clear();
                for i in chunk * steps / chunks..=((chunk + 1) * steps / chunks).min(steps - 1) {
                    let t = i as f32 / steps as f32 * TAU * laps;
                    let k = (1.0 - ratio) / ratio;
                    let x = (1.0 - ratio) * t.cos() + d * (k * t).cos();
                    let y = (1.0 - ratio) * t.sin() - d * (k * t).sin();
                    let (sin, cos) = turn.sin_cos();
                    pen.points
                        .push(c + vec2(x * cos - y * sin, x * sin + y * cos) * scale);
                }
                let level = band(a, chunk as f32 / chunks as f32);
                // (the band's level picks the colour; how bright follows the
                // guarded energy alone - each band's and the beat's own,
                // twenty-eight chunks each in its own time, were more than
                // six rises a second, strictly counted)
                let colour = mix(
                    s.background,
                    s.at(0.2 + 0.8 * level),
                    // (in the starkest schemes a shade into the ground: black
                    // laps on white were moving stripes)
                    (0.5 + 0.2 * (4 - layer) as f32 / 4.0 + 0.15 * a.energy)
                        * faint
                        * (1.0 - 0.5 * s.stark()),
                );
                glowing_line(
                    pen,
                    1.0 + 0.5 * inner as f32,
                    colour,
                    faint * (0.8 + 0.2 * a.energy),
                );
            }
        }
    }
}

// -- the polygon morph ---------------------------------------------------------------

/// The radius of a regular `sides`-gon (a star for 0) at `theta`.
fn polygon_radius(sides: usize, theta: f32) -> f32 {
    if sides == 0 {
        // a five-pointed star
        let k = (theta.rem_euclid(TAU / 5.0)) / (TAU / 5.0);
        return 1.0 - 0.55 * (1.0 - (2.0 * k - 1.0).abs());
    }
    let n = sides as f32;
    let seg = TAU / n;
    (PI / n).cos() / ((theta.rem_euclid(seg)) - PI / n).cos()
}

fn morph(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    // (never saturated red: its lines, in a pure red scheme, flashed red
    // eight or nine times a second in Intense)
    let unred = f.s.unred();
    let (a, s, r) = (f.a, &unred, f.rect);
    const SHAPES: [usize; 7] = [3, 4, 5, 6, 0, 8, 3];
    if a.beat.count != st.last_beat {
        st.last_beat = a.beat.count;
        if st.morph.2 >= 1.0 && a.beat.count % 2 == 0 {
            st.morph = (
                st.morph.1,
                SHAPES[(a.beat.count as usize / 2) % SHAPES.len()],
                0.0,
            );
        }
    }
    st.morph.2 = (st.morph.2 + dt / 0.6).min(1.0);
    st.morph_turn += dt * (0.15 + 0.8 * a.energy);
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let t = ease(st.morph.2);
    let c = r.center();
    let size = r.height() / 2.0 - 4.0;
    // (fewer copies, the outer ones fainter: closely packed outlines that
    // move are a pattern a photosensitive eye cannot bear)
    const COPIES: usize = 15;
    const POINTS: usize = 120;
    // copies from the middle out past the picture's sides (k < 0: outside)
    for copy in (0..COPIES).rev() {
        let k = copy as f32 / 10.0 - 0.5;
        let scale = size * (1.0 - k * 0.92) * (1.0 + 0.06 * a.beat.pulse * (1.0 - k.abs()));
        let turn = st.morph_turn - k * (0.6 + 1.2 * a.energy);
        pen.points.clear();
        for i in 0..POINTS {
            let theta = i as f32 / POINTS as f32 * TAU;
            let r0 = polygon_radius(st.morph.0, theta);
            let r1 = polygon_radius(st.morph.1, theta);
            let wobble = 1.0 + 0.12 * band(a, i as f32 / POINTS as f32) * (1.0 - k);
            let rr = (r0 + (r1 - r0) * t) * wobble * scale;
            let angle = theta + turn - PI / 2.0;
            pen.points.push(c + vec2(angle.cos(), angle.sin()) * rr);
        }
        let kk = k.clamp(0.0, 1.0);
        let colour = mix(
            s.background,
            s.at(1.0 - kk),
            (0.25 + 0.75 * (1.0 - k.abs())).clamp(0.1, 1.0),
        );
        // (four copies as they are, the rest faint: fifteen outlines,
        // nested, were a grating over half the picture)
        let colour = if (3..=6).contains(&copy) {
            colour
        } else {
            super::faint(colour, s.background)
        };
        let width = 1.0 + 1.6 * (1.0 - k.abs()).max(0.0);
        pen.outline(width * 6.0, glow(colour, 0.06));
        pen.outline(width * 2.5, glow(colour, 0.18));
        pen.outline(width, colour);
    }
}

// -- rings on the beat ---------------------------------------------------------------

fn beatrings(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let reach = r.width().max(r.height()) * 0.6;
    st.since_ring += dt;
    let beat = a.beat.count != st.last_beat;
    st.last_beat = a.beat.count;
    // (a new ring no sooner than twice the motion's turn after the last:
    // every beat of a fast song, a ring passing each pixel lit and darkened
    // it more than six times a second)
    let gap = 2.0 * a.motion.turn();
    if (beat && st.since_ring >= gap) || (st.since_ring > 1.5 && a.energy > 0.15) {
        st.since_ring = 0.0;
        let strength = if beat {
            0.5 + 0.5 * a.beat.strength
        } else {
            0.3
        };
        if st.rings.len() == RINGS {
            st.rings.remove(0);
        }
        let hue = st.random();
        let speed = 0.5 + 0.3 * st.random();
        st.rings.push([0.0, strength, hue, speed]);
        for _ in 0..(28.0 * strength) as usize {
            if st.sparks.len() == SPARKS {
                st.sparks.remove(0);
            }
            let angle = st.random() * TAU;
            let speed = 0.25 + 0.5 * st.random();
            st.sparks
                .push([0.0, 0.0, angle.cos() * speed, angle.sin() * speed, 0.0]);
        }
    }
    for ring in st.rings.iter_mut() {
        ring[0] += dt;
    }
    st.rings.retain(|g| g[0] < 2.2);
    for spark in st.sparks.iter_mut() {
        spark[0] += spark[2] * dt;
        spark[1] += spark[3] * dt;
        spark[4] += dt;
    }
    st.sparks.retain(|k| k[4] < 1.4);
    let base = r.width().min(r.height()) * 0.14;
    let t = a.time as f32;
    // a sonar's grid behind, turning slowly, brighter on the beat
    let grid = glow(s.low, 0.07 + 0.08 * a.beat.pulse);
    for k in 1..=7 {
        pen.ring(c, base * k as f32 * 0.9, 1.0, grid);
    }
    for k in 0..24 {
        let u = Vec2::angled(k as f32 / 24.0 * TAU + t * 0.05);
        pen.segment(c + u * base * 0.9, c + u * reach, 1.0, grid);
    }
    // the spectrum round the middle, mirrored left and right - at most 48
    // spokes a side, each the mean of its bars (128 bars' spokes lay over
    // each other as a disc, its edge each bar's jitter: in black and white
    // at the Options' extremes 7 transitions a second by the strict
    // reading)
    let bars = a.bars.len().max(1);
    let n = bars.min(48);
    for i in 0..n {
        let (from, to) = (i * bars / n, ((i + 1) * bars / n).max(i * bars / n + 1));
        let to = to.min(a.bars.len());
        let raw = if from < to {
            a.bars[from..to].iter().sum::<f32>() / (to - from) as f32
        } else {
            0.0
        };
        // (eased: their tips' jitter, the band of the disc's edge flickering
        // as a whole, was 7 by the strict reading with the Options at their
        // extremes, Intense)
        if st.spokes.len() != n {
            st.spokes.clear();
            st.spokes.resize(n, raw);
        }
        let ease = 1.0 - (-dt / (0.12 * a.motion.turn() / 0.25)).exp();
        st.spokes[i] += (raw - st.spokes[i]) * ease;
        let v = st.spokes[i];
        for side in [1.0f32, -1.0] {
            let angle = -PI / 2.0 + side * (i as f32 + 0.5) / n as f32 * PI;
            let u = Vec2::angled(angle);
            streak(
                pen,
                c + u * base,
                c + u * (base + v * base * 1.8),
                2.5,
                mix(s.background, s.low, 0.35),
                mix(s.background, s.at(v), 0.55),
            );
        }
    }
    // each ring rippled by the spectrum round it as it goes out
    for ring in &st.rings {
        let [age, strength, hue, speed] = *ring;
        let life = 1.0 - age / 2.2;
        let radius = base + age * reach * speed;
        let width = (1.5 + 8.0 * strength) * life;
        // (never saturated red: rings that come and go on the beat in a
        // red scheme's orange were red flashes, seven a second)
        let colour = mix(
            s.background,
            super::unred(s.at(0.3 + 0.7 * hue)),
            life * life,
        );
        pen.points.clear();
        for k in 0..120 {
            let angle = k as f32 / 120.0 * TAU;
            let fold = (angle / PI - 1.0).abs();
            let ripple = band(a, 1.0 - fold) * base * 0.35 * life * strength;
            pen.points
                .push(c + Vec2::angled(angle - PI / 2.0) * (radius + ripple));
        }
        pen.outline(
            width * 3.0,
            glow(super::unred(s.at(0.3 + 0.7 * hue)), 0.12 * life),
        );
        pen.outline(width, colour);
    }
    // sparks with their trails
    let size = r.width().min(r.height()) * 0.5;
    for [x, y, vx, vy, age] in &st.sparks {
        let life = 1.0 - age / 1.4;
        let at = c + vec2(*x, *y) * size;
        let tail = at - vec2(*vx, *vy) * size * 0.15;
        streak(
            pen,
            tail,
            at,
            1.0 + 1.5 * life,
            Color32::TRANSPARENT,
            mix(s.background, s.peak, life),
        );
    }
    let core = base * (0.6 + 0.45 * a.bass);
    pen.radial(
        c,
        core * 2.4,
        glow(s.at(a.energy), 0.5),
        Color32::TRANSPARENT,
    );
    pen.circle(c, core, s.at(0.4 + 0.6 * a.energy));
    pen.circle(c, core * 0.4, mix(s.at(a.energy), s.peak, 0.6));
}

// -- kinetic typography --------------------------------------------------------------

fn typography(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    if st.title != f.title || st.words.is_empty() {
        st.title.clear();
        st.title.push_str(f.title);
        st.words.clear();
        for word in f
            .title
            .split_whitespace()
            .filter(|w| w.chars().any(char::is_alphanumeric))
        {
            st.words.push(word.to_owned());
        }
        if st.words.is_empty() {
            st.words.extend(["axross", "amp"].map(str::to_owned));
        }
        st.word = 0;
        st.word_age = 0.0;
        st.previous_word = usize::MAX;
    }
    st.word_age += dt;
    let beat = a.beat.count != st.last_beat;
    st.last_beat = a.beat.count;
    // (a word a beat at most every 0.75 s: a big word coming and the last
    // one going is a rise and a fall over much of the picture - at every
    // beat of a fast song, more than three of them a second)
    if (beat && st.word_age > 0.75) || st.word_age > 2.0 {
        st.previous_word = st.word;
        st.word = (st.word + 1) % st.words.len();
        st.word_age = 0.0;
    }
    // the size the words are laid out in: a few steps, not every size
    // (each is a set of glyphs in the font's picture)
    let base = ((r.height() * 0.35 / 16.0).round() * 16.0).clamp(32.0, 160.0);
    let font = FontId::proportional(base);
    // (a glyph's place in the font's picture is in its pixels)
    let atlas = f.painter.ctx().fonts(|fonts| fonts.font_image_size());
    let uv = vec2(1.0 / atlas[0].max(1) as f32, 1.0 / atlas[1].max(1) as f32);
    let c = r.center();
    let t = a.time as f32;
    // behind: the whole title in rows running both ways, faint, faster
    // as it gets louder
    st.marquee += dt * (18.0 + 90.0 * a.energy.min(1.0));
    let small = FontId::proportional((base * 0.5).round().max(16.0));
    let line = f
        .painter
        .layout_no_wrap(format!("{}  ·  ", f.title), small, Color32::WHITE);
    let lw = line.size().x.max(40.0);
    // (whole rows only: none cut by the picture's edge, and none over the
    // spectrum's line at its foot - they lay across it)
    let pitch = line.size().y * 1.3;
    let room = (r.height() - 16.0).max(pitch);
    let rows = ((room / pitch).floor() as i32).max(1);
    let top = r.top() + room / 2.0 - (rows as f32 - 1.0) * pitch / 2.0;
    for row in 0..rows {
        let y = top + row as f32 * pitch;
        let way = if row % 2 == 0 { 1.0 } else { -1.0 };
        let shift =
            (st.marquee * way * (0.6 + 0.25 * row as f32) + row as f32 * 97.0).rem_euclid(lw);
        let colour = glow(
            if row % 2 == 0 { s.low } else { s.mid },
            0.07 + 0.06 * a.beat.pulse,
        );
        let mut x = r.left() - lw + shift + lw / 2.0;
        while x - lw / 2.0 < r.right() {
            put_text(pen, &line, uv, pos2(x, y), 1.0, 0.0, colour, 0.0, 0);
            x += lw;
        }
    }
    let fit = |g: &egui::Galley| {
        (r.width() * 0.84 / g.size().x.max(1.0)).min(r.height() * 0.5 / g.size().y.max(1.0))
    };
    // the word before goes up and out as the new one comes up from below,
    // a letter after another
    let travel = (st.word_age / 0.4).min(1.0);
    let ease = travel * travel * (3.0 - 2.0 * travel);
    let word = &st.words[st.word];
    let galley = f
        .painter
        .layout_no_wrap(word.clone(), font.clone(), Color32::WHITE);
    let scale = fit(&galley) * (1.0 + 0.08 * a.beat.pulse + 0.04 * a.bass);
    let height = galley.size().y * scale;
    if st.previous_word != usize::MAX && travel < 1.0 {
        let old = f.painter.layout_no_wrap(
            st.words[st.previous_word].clone(),
            font.clone(),
            Color32::WHITE,
        );
        // (up by half its height, gone by half the way: it went up and out
        // of the picture, cut by its edge, and lay over the new word)
        let at = c - vec2(0.0, ease * height * 0.45);
        let colour = mix(s.background, s.mid, (1.0 - 2.0 * ease).max(0.0));
        put_text(pen, &old, uv, at, fit(&old), 0.0, colour, 0.0, 1);
    }
    let tilt = (t * 0.6).sin() * 0.025;
    let letters = galley
        .rows
        .iter()
        .map(|row| row.row.visuals.mesh.vertices.len() / 4)
        .sum::<usize>()
        .max(1) as f32;
    let age = st.word_age;
    let pulse = a.beat.pulse;
    let arrive = move |glyph: u32| {
        let k = ((age - (glyph as f32 - 1.0) * 0.05) / 0.35).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        let bounce = pulse * (glyph as f32 * 1.3 + t * 6.0).sin() * 0.06;
        (vec2(0.0, ((1.0 - k) * 0.3 - bounce) * height), k)
    };
    // a glow behind, then the word: quiet to loud along it, its peak on
    // the beat
    // (a halo all round - copies a little off in eight directions - not
    // a copy a little larger, which stood off like a misplaced shadow)
    for k in 0..8 {
        let off = Vec2::angled(k as f32 / 8.0 * TAU) * (2.0 + 1.5 * pulse);
        put_glyphs(pen, &galley, uv, c, scale, tilt, |glyph| {
            let (lift, a) = arrive(glyph);
            (lift + off, glow(s.mid, 0.07 * a))
        });
    }
    put_glyphs(pen, &galley, uv, c, scale, tilt, |glyph| {
        let (lift, k) = arrive(glyph);
        let along = (glyph as f32 - 1.0) / (letters - 1.0).max(1.0);
        let colour = mix(mix(s.high, s.peak, 0.35 * along), s.peak, pulse * 0.3);
        (lift, mix(s.background, colour, k))
    });
    // a bar under it, as wide as it is loud - below its letters' feet
    // (and their descenders), once the word has come
    let wide = galley.size().x * scale * (0.25 + 0.75 * a.energy.min(1.0)) * ease;
    let y = c.y + height * 0.62;
    let (l, rr) = (pos2(c.x - wide / 2.0, y), pos2(c.x + wide / 2.0, y));
    pen.segment(l, rr, 10.0, glow(s.mid, 0.15 + 0.25 * pulse));
    pen.segment(l, rr, 3.0, mix(s.mid, s.peak, pulse * 0.6));
    // the spectrum as a line under it
    let y = r.bottom() - 6.0;
    let n = a.bars.len().max(1);
    for i in 0..n {
        let x = r.left() + (i as f32 + 0.5) / n as f32 * r.width();
        let v = a.bars[i];
        pen.rect(
            Rect::from_min_max(pos2(x - 1.0, y - v * r.height() * 0.12), pos2(x + 1.0, y)),
            mix(s.background, s.at(v), 0.7),
        );
    }
}

/// A laid out word's glyphs into the mesh: centred on `at`, scaled,
/// turned, coloured; each glyph shaken by up to `shake` (points).
#[allow(clippy::too_many_arguments)]
fn put_text(
    pen: &mut Pen<'_>,
    galley: &egui::Galley,
    uv: Vec2,
    at: Pos2,
    scale: f32,
    angle: f32,
    colour: Color32,
    shake: f32,
    salt: u32,
) {
    put_glyphs(pen, galley, uv, at, scale, angle, |glyph| {
        let jitter = if shake > 0.0 {
            vec2(hash(glyph, salt + 17) - 0.5, hash(glyph, salt + 29) - 0.5) * shake * 2.0
        } else {
            Vec2::ZERO
        };
        (jitter, colour)
    });
}

/// A laid out word's glyphs into the mesh, each glyph (counted from 1)
/// moved and coloured by `each`.
fn put_glyphs(
    pen: &mut Pen<'_>,
    galley: &egui::Galley,
    uv: Vec2,
    at: Pos2,
    scale: f32,
    angle: f32,
    each: impl Fn(u32) -> (Vec2, Color32),
) {
    let centre = galley.rect.center();
    let (sin, cos) = angle.sin_cos();
    let mut glyph = 0u32;
    let mut now = (Vec2::ZERO, Color32::TRANSPARENT);
    for row in &galley.rows {
        let mesh = &row.row.visuals.mesh;
        let base = pen.mesh.vertices.len() as u32;
        for (k, v) in mesh.vertices.iter().enumerate() {
            if k % 4 == 0 {
                glyph += 1;
                now = each(glyph);
            }
            let p = (v.pos + row.pos.to_vec2() - centre.to_vec2()).to_vec2() * scale + now.0;
            let mut vertex = *v;
            vertex.pos = at + vec2(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
            vertex.color = now.1;
            vertex.uv = pos2(v.uv.x * uv.x, v.uv.y * uv.y);
            pen.mesh.vertices.push(vertex);
        }
        for i in &mesh.indices {
            pen.mesh.indices.push(base + i);
        }
    }
}
