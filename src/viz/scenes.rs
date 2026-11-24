//! More pictures the sound moves: planets on their orbits, a plasma globe,
//! a fractal tree in the wind, a water surface the beat drops into, a
//! constellation, a strange attractor, bokeh, sand on a Chladni plate, a
//! contour map, a torus knot and a sunflower's seeds.
//!
//! The water, the attractor, the plate and the map are drawn by shaders
//! ([`super::gpu`]); the water's surface and the attractor's density are
//! worked out here, a picture each the shader shows. What each keeps is made
//! once and then only changed: a frame allocates nothing.

// (bars, peaks and their places go in step: an index reads best)
#![allow(clippy::needless_range_loop)]

use super::gpu::{self, Shader, Uniforms};
use super::{Frame, Pen, Scheme, VizState, faint, glow, luminance, mix, random};
use crate::analyzer::Analyzer;
use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use std::f32::consts::{PI, TAU};

/// The faces drawn here.
pub fn draws(key: &str) -> bool {
    matches!(
        key,
        "orbits"
            | "filaments"
            | "tree"
            | "water"
            | "constellation"
            | "attractor"
            | "bokeh"
            | "cymatics"
            | "contours"
            | "knot"
            | "sunflower"
    )
}

const PLANETS: usize = 7;
/// The plasma globe's filaments at most.
const FILAMENTS: usize = 13;
const TREE_DEPTH: usize = 9;
const PETALS: usize = 80;
const STARS: usize = 84;
const LIGHTS: usize = 46;
const KNOT_POINTS: usize = 300;
const SEEDS: usize = 520;
const RIPPLES: usize = 6;
/// The attractor's points a frame.
const ORBIT_STEPS: usize = 16_000;

/// The Chladni figures the plate shows, simplest first: modes n, m and
/// whether the two halves add (1) or take away (-1).
const MODES: [(f32, f32, f32); 10] = [
    (1.0, 3.0, 1.0),
    (2.0, 3.0, -1.0),
    (1.0, 4.0, -1.0),
    (2.0, 5.0, 1.0),
    (3.0, 4.0, -1.0),
    (3.0, 5.0, 1.0),
    (2.0, 7.0, -1.0),
    (4.0, 5.0, 1.0),
    (3.0, 7.0, -1.0),
    (5.0, 6.0, 1.0),
];

/// The attractor's shapes (Clifford's a, b, c, d), one after another: each a
/// strange one, as a test holds it to.
const SHAPES: [[f32; 4]; 8] = [
    [-1.4, 1.6, 1.0, 0.7],
    [1.5, -1.8, 1.6, 0.9],
    [-1.3, -1.3, -1.8, -1.9],
    [1.7, 1.7, 0.6, 1.2],
    [-2.0, -2.0, -1.2, 2.0],
    [-1.7, 1.3, -0.1, -1.2],
    [-1.9, -1.9, -1.9, -1.0],
    [2.0, 2.0, -1.3, -1.5],
];

/// The knot's windings (p, q: coprime).
const KNOTS: [(f32, f32); 6] = [
    (2.0, 3.0),
    (3.0, 5.0),
    (2.0, 5.0),
    (3.0, 4.0),
    (3.0, 7.0),
    (2.0, 7.0),
];

pub struct State {
    seed: u32,
    last_time: f64,
    clock: f32,
    last_beat: u64,
    /// Seconds since something came on the beat.
    since_beat: f32,
    orbit_turn: [f32; PLANETS],
    /// x, y (0..1 of the picture), drift, sway phase, age (s).
    petals: Vec<[f32; 5]>,
    /// Where the tree's twigs end (petals fall from them).
    tips: Vec<Pos2>,
    /// The water: its heights now and a step before, its grid, its clock.
    water: Vec<f32>,
    water_old: Vec<f32>,
    water_size: (usize, usize),
    water_clock: f32,
    /// x, y (0..1), drift x, drift y, band (0..1).
    stars: Vec<[f32; 5]>,
    /// The attractor's density, its grid, where its orbit is, its shape now,
    /// the one it goes to and how far, the loudest cell lately.
    density: Vec<f32>,
    density_size: (usize, usize),
    orbit: [f32; 2],
    /// The orbit of the shape that is coming.
    orbit_to: [f32; 2],
    shape: usize,
    shape_to: usize,
    shape_blend: f32,
    shape_beat: u64,
    shape_age: f32,
    density_top: f32,
    /// How much of the sound's bending the attractor's shape takes (0..1).
    bend: f32,
    /// Where its orbit goes (x from, to, y from, to), eased.
    bounds: [f32; 4],
    /// x, y (0..1), depth, band, drift phase, tint.
    lights: Vec<[f32; 6]>,
    /// The plate's figure, the one it goes to, how far; the sound's centre
    /// of weight and loudness, eased; seconds since the last change.
    mode: usize,
    mode_to: usize,
    mode_blend: f32,
    centroid: f32,
    plate_level: f32,
    mode_age: f32,
    /// The contour map's six hills (the bands', eased by the motion
    /// setting), and whether they have been set yet.
    hills: [f32; 6],
    hills_set: bool,
    /// The knot: its winding, the next, how far; its turn; its points
    /// (x, y, depth on the screen) and their order, far ones first.
    knot: usize,
    knot_to: usize,
    knot_blend: f32,
    knot_beat: u64,
    knot_age: f32,
    knot_turn: f32,
    knot_points: Vec<[f32; 3]>,
    knot_order: Vec<(f32, u16)>,
    /// The sunflower's turn and its ripples (age, strength).
    sun_turn: f32,
    ripples: [[f32; 2]; RIPPLES],
}

impl Default for State {
    fn default() -> Self {
        State {
            seed: 11,
            last_time: 0.0,
            clock: 0.0,
            last_beat: 0,
            since_beat: 10.0,
            orbit_turn: [0.0; PLANETS],
            petals: Vec::with_capacity(PETALS),
            tips: Vec::with_capacity(1 << TREE_DEPTH),
            water: Vec::new(),
            water_old: Vec::new(),
            water_size: (0, 0),
            water_clock: 0.0,
            stars: Vec::with_capacity(STARS),
            density: Vec::new(),
            density_size: (0, 0),
            orbit: [0.1, 0.1],
            orbit_to: [0.1, 0.1],
            shape: 0,
            shape_to: 0,
            shape_blend: 1.0,
            shape_beat: 0,
            shape_age: 0.0,
            density_top: 1.0,
            bend: 1.0,
            bounds: [-2.0, 2.0, -2.0, 2.0],
            lights: Vec::with_capacity(LIGHTS),
            mode: 1,
            mode_to: 1,
            mode_blend: 1.0,
            centroid: 0.3,
            plate_level: 0.0,
            hills: [0.0; 6],
            hills_set: false,
            mode_age: 0.0,
            knot: 0,
            knot_to: 0,
            knot_blend: 1.0,
            knot_beat: 0,
            knot_age: 0.0,
            knot_turn: 0.0,
            knot_points: Vec::with_capacity(KNOT_POINTS),
            knot_order: Vec::with_capacity(KNOT_POINTS),
            sun_turn: 0.0,
            ripples: [[9.0, 0.0]; RIPPLES],
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
        let st = &mut state.scenes;
        let dt = (f.a.time - st.last_time).clamp(0.0, 0.25) as f32;
        st.last_time = f.a.time;
        st.clock += dt;
        st.since_beat += dt;
        dt
    };
    if let Some(shader) = Shader::of(key) {
        return shaded(f, shader, state, dt);
    }
    let cv = &mut state.canvas;
    let st = &mut state.scenes;
    let p = f.painter;
    match key {
        "orbits" => cv.draw(p, |pen| orbits(f, pen, st, dt)),
        "filaments" => cv.draw(p, |pen| filaments(f, pen)),
        "tree" => cv.draw(p, |pen| tree(f, pen, st, dt)),
        "constellation" => cv.draw(p, |pen| constellation(f, pen, st, dt)),
        "bokeh" => cv.draw(p, |pen| bokeh(f, pen, st, dt)),
        "knot" => cv.draw(p, |pen| knot(f, pen, st, dt)),
        _ => cv.draw(p, |pen| sunflower(f, pen, st, dt)),
    }
}

// -- what the faces draw with --------------------------------------------------------

/// Whether the ground is dark (light is added to it) or light (colour is
/// laid over it).
pub(super) fn dark(s: &Scheme) -> bool {
    luminance(s.background) < 0.3
}

/// Light of `c`, `k` strong: added to a dark ground (a glow), laid over a
/// light one (a tint: added light would not show on it).
pub(super) fn shine(s: &Scheme, c: Color32, k: f32) -> Color32 {
    if dark(s) {
        glow(c, k)
    } else {
        c.gamma_multiply((k * 0.8).clamp(0.0, 1.0))
    }
}

/// A round dot (a polygon of ten sides; a square when tiny): cheaper than
/// a path's circle, thousands of them a frame.
pub(super) fn dot(pen: &mut Pen<'_>, centre: Pos2, radius: f32, colour: Color32) {
    if radius > 6.0 {
        // (a large one round and antialiased)
        pen.circle(centre, radius, colour);
        return;
    }
    if radius < 1.1 {
        pen.rect(
            Rect::from_center_size(centre, vec2(radius, radius) * 2.0),
            colour,
        );
        return;
    }
    // (cos, sin) of k * 36 degrees
    const RING: [(f32, f32); 10] = [
        (1.0, 0.0),
        (0.809_017, 0.587_785),
        (0.309_017, 0.951_057),
        (-0.309_017, 0.951_057),
        (-0.809_017, 0.587_785),
        (-1.0, 0.0),
        (-0.809_017, -0.587_785),
        (-0.309_017, -0.951_057),
        (0.309_017, -0.951_057),
        (0.809_017, -0.587_785),
    ];
    let i0 = pen.mesh.vertices.len() as u32;
    pen.mesh.colored_vertex(centre, colour);
    for (c, s) in RING {
        pen.mesh
            .colored_vertex(centre + vec2(c, s) * radius, colour);
    }
    for k in 0..10u32 {
        pen.mesh.add_triangle(i0, i0 + 1 + k, i0 + 1 + (k + 1) % 10);
    }
}

/// A soft dot: `inner` at the centre to nothing at `radius` (ten sides).
pub(super) fn soft(pen: &mut Pen<'_>, centre: Pos2, radius: f32, inner: Color32) {
    if radius < 0.5 {
        return;
    }
    let i0 = pen.mesh.vertices.len() as u32;
    pen.mesh.colored_vertex(centre, inner);
    for k in 0..12 {
        let a = k as f32 / 12.0 * TAU;
        pen.mesh.colored_vertex(
            centre + vec2(a.cos(), a.sin()) * radius,
            Color32::TRANSPARENT,
        );
    }
    for k in 0..12u32 {
        pen.mesh.add_triangle(i0, i0 + 1 + k, i0 + 1 + (k + 1) % 12);
    }
}

/// A thin quad from `a` to `b` (no feather: hundreds of them a frame).
pub(super) fn streak(pen: &mut Pen<'_>, a: Pos2, b: Pos2, width: f32, ca: Color32, cb: Color32) {
    let d = b - a;
    let len = d.length();
    if len < 1e-3 {
        return;
    }
    let n = vec2(-d.y, d.x) / len * (width / 2.0);
    pen.quad([a - n, b - n, b + n, a + n], [ca, cb, cb, ca]);
}

/// The bars at `x` (0..1), between them.
pub(super) fn band(a: &Analyzer, x: f32) -> f32 {
    let n = a.bars.len();
    if n == 0 {
        return 0.0;
    }
    let f = x.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = f.floor() as usize;
    let j = (i + 1).min(n - 1);
    a.bars[i] + (a.bars[j] - a.bars[i]) * (f - i as f32)
}

/// The mean of the bars from `x0` to `x1` (0..1).
pub(super) fn span(a: &Analyzer, x0: f32, x1: f32) -> f32 {
    let n = a.bars.len();
    if n == 0 {
        return 0.0;
    }
    let i0 = ((x0.clamp(0.0, 1.0) * n as f32) as usize).min(n - 1);
    let i1 = ((x1.clamp(0.0, 1.0) * n as f32).ceil() as usize).clamp(i0 + 1, n);
    a.bars[i0..i1].iter().sum::<f32>() / (i1 - i0) as f32
}

/// A fixed random number 0..1 for (k, salt).
pub(super) fn hash(k: u32, salt: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Smoothstep of 0..1.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A beat, no sooner than `gap` seconds after the last one taken.
fn beat(a: &Analyzer, st: &mut State, gap: f32) -> bool {
    if a.beat.count != st.last_beat {
        st.last_beat = a.beat.count;
        if st.since_beat >= gap {
            st.since_beat = 0.0;
            return true;
        }
    }
    false
}

// -- the shader faces ----------------------------------------------------------------

fn shaded(f: &Frame<'_>, shader: Shader, state: &mut VizState, dt: f32) {
    let a = f.a;
    let mut u = Uniforms::new(shader, a, f.s, 0.0);
    let mut picture = false;
    let aspect = f.rect.width() / f.rect.height().max(1.0);
    let st = &mut state.scenes;
    match shader {
        Shader::Water => {
            water(a, st, dt, aspect, state.gpu.picture(f.second));
            picture = true;
        }
        Shader::Attractor => {
            attractor(a, st, dt, aspect, state.gpu.picture(f.second));
            picture = true;
        }
        Shader::Cymatics => {
            plate(a, st, dt, f.rect.height());
            let (from, to) = (MODES[st.mode], MODES[st.mode_to]);
            u = u
                .with_extra(1, [from.0, from.1, to.0, to.1])
                .with_extra(2, [smooth(st.mode_blend), from.2, to.2, st.plate_level]);
        }
        Shader::Contours => {
            // the land rises and sinks with the bands - as fast as they
            // do in Intense, over a sixth of a second in Calm, over half
            // of one in Reduced (the hills followed the bands at once
            // whatever the setting: every contour line of the picture
            // moved as fast in Reduced as in Intense)
            use crate::analyzer::guard::Motion;
            let tau = match a.motion {
                Motion::Intense => 0.05,
                Motion::Calm => 0.16,
                Motion::Reduced => 0.5,
            };
            let ease = if st.hills_set {
                1.0 - (-dt.max(0.0) / tau).exp()
            } else {
                1.0
            };
            st.hills_set = true;
            for (k, hill) in st.hills.iter_mut().enumerate() {
                *hill += (u.band((k as f32 + 0.5) / 6.0) - *hill) * ease;
            }
            let h = st.hills;
            u = u
                .with_extra(1, [h[0], h[1], h[2], h[3]])
                .with_extra(2, [h[4], h[5], 0.0, 0.0]);
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

// -- the water -----------------------------------------------------------------------

/// A drop into the water at (x, y) (0..1): a dent `depth` deep, `r` cells round.
fn drop(st: &mut State, x: f32, y: f32, depth: f32, r: f32) {
    let (w, h) = st.water_size;
    let (cx, cy) = (x * w as f32, y * h as f32);
    let reach = r.ceil() as i32 + 1;
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let (xx, yy) = (cx as i32 + dx, cy as i32 + dy);
            if xx < 1 || yy < 1 || xx >= w as i32 - 1 || yy >= h as i32 - 1 {
                continue;
            }
            let d2 = (dx * dx + dy * dy) as f32 / (r * r);
            let k = (-d2 * 2.0).exp() * depth;
            st.water[yy as usize * w + xx as usize] -= k;
        }
    }
}

fn water(
    a: &Analyzer,
    st: &mut State,
    dt: f32,
    aspect: f32,
    picture: &std::sync::Mutex<gpu::Picture>,
) {
    let h = 90usize;
    let w = ((h as f32 * aspect).round() as usize).clamp(40, 320);
    if st.water_size != (w, h) {
        st.water_size = (w, h);
        st.water.clear();
        st.water.resize(w * h, 0.0);
        st.water_old.clear();
        st.water_old.resize(w * h, 0.0);
        for _ in 0..5 {
            let (x, y) = (0.1 + 0.8 * st.random(), 0.15 + 0.7 * st.random());
            drop(st, x, y, 0.5, 3.0);
        }
    }
    // a drop where the music is loudest on the beat (bass on the left,
    // treble on the right), not more than three a second
    if beat(a, st, 0.33) {
        // (a band chosen as loud as it is: the louder, the likelier)
        let n = a.bars.len().max(1);
        let total: f32 = a.bars.iter().map(|v| v * v).sum();
        let mut pick = st.random() * total;
        let mut at = 0;
        for (i, v) in a.bars.iter().enumerate() {
            pick -= v * v;
            if pick <= 0.0 {
                at = i;
                break;
            }
        }
        let x = 0.08 + 0.84 * (at as f32 + 0.5) / n as f32 + (st.random() - 0.5) * 0.08;
        let y = 0.2 + 0.6 * st.random();
        let depth = 0.35 + 0.35 * a.beat.strength.min(1.0);
        drop(st, x, y, depth, 3.5 + 2.0 * a.bass);
    } else if st.since_beat > 1.6 && a.energy > 0.08 {
        // (no beat: drops as the music moves)
        st.since_beat = 0.0;
        let (x, y) = (0.1 + 0.8 * st.random(), 0.15 + 0.7 * st.random());
        drop(st, x, y, 0.3, 3.5);
    }
    // 60 steps a second; a fine rain with the treble
    st.water_clock = (st.water_clock + dt).min(0.1);
    while st.water_clock >= 1.0 / 60.0 {
        st.water_clock -= 1.0 / 60.0;
        if st.random() < 0.1 + a.treble * 0.5 {
            let (x, y) = (st.random(), st.random());
            drop(st, x, y, 0.04 + 0.08 * a.treble, 1.4);
        }
        let (now, old) = (&st.water, &mut st.water_old);
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = y * w + x;
                let next = (now[i - 1] + now[i + 1] + now[i - w] + now[i + w]) * 0.5 - old[i];
                old[i] = next * 0.975;
            }
        }
        std::mem::swap(&mut st.water, &mut st.water_old);
    }
    if let Ok(mut pic) = picture.lock() {
        if pic.data.len() != w * h {
            pic.data.clear();
            pic.data.resize(w * h, 128);
        }
        pic.width = w as u32;
        pic.height = h as u32;
        for (d, v) in pic.data.iter_mut().zip(&st.water) {
            // (softly limited: a fresh drop is no flat plateau)
            *d = (128.0 + 127.0 * (v * 2.5).tanh()) as u8;
        }
        pic.version = pic.version.wrapping_add(1);
    }
}

// -- the strange attractor -----------------------------------------------------------

fn attractor(
    a: &Analyzer,
    st: &mut State,
    dt: f32,
    aspect: f32,
    picture: &std::sync::Mutex<gpu::Picture>,
) {
    let h = 150usize;
    let w = ((h as f32 * aspect).round() as usize).clamp(60, 420);
    if st.density_size != (w, h) {
        st.density_size = (w, h);
        st.density.clear();
        st.density.resize(w * h, 0.0);
    }
    // a new shape each phrase (32 beats; 30 s without a beat): the new
    // one's orbit drawn in over the old one's, over four seconds. (The
    // shapes' numbers were once led from one shape's to the next's: the
    // sets between two strange ones are mostly not strange - cycles of a
    // few points - and for seconds of every phrase, with any music, the
    // figure was hairlines and dots on an empty ground: review 19, M1.)
    st.shape_age += dt;
    if st.shape_blend >= 1.0 && (a.beat.count >= st.shape_beat + 32 || st.shape_age > 30.0) {
        st.shape = st.shape_to;
        st.orbit = st.orbit_to;
        st.shape_to = (st.shape_to + 1) % SHAPES.len();
        st.orbit_to = [0.1, 0.1];
        st.shape_blend = 0.0;
        st.shape_beat = a.beat.count;
        st.shape_age = 0.0;
    }
    st.shape_blend = (st.shape_blend + dt / 4.0).min(1.0);
    let b = if st.shape_to == st.shape {
        0.0
    } else {
        smooth(st.shape_blend)
    };
    // the sound breathes into it: the bass, the middle and the treble each
    // turn one of its numbers a little, and they drift by themselves - as
    // far as the orbit stays a strange one: every set of numbers drawn is
    // tried first (its exponent, how much it covers), and where it is not
    // strange the bend is halved, twice, then left out - the shapes
    // themselves are strange, each tested (review 18, M5; 19, M1)
    let t = st.clock;
    let turn = [
        0.05 * a.bass + 0.03 * (t * 0.11).sin(),
        0.035 * a.middle,
        0.045 * a.treble + 0.04 * (t * 0.07).cos(),
        0.03 * (t * 0.05).sin(),
    ];
    let mut most = 1.0f32;
    let mut numbers = |shape: usize, orbit: &mut [f32; 2]| -> [f32; 4] {
        if !orbit[0].is_finite() || !orbit[1].is_finite() {
            *orbit = [0.1, 0.1];
        }
        let (k, bend) = full_shape(shape, turn, st.bend, orbit);
        most = most.min(bend);
        k
    };
    let (mut from, mut to) = (st.orbit, st.orbit_to);
    let k_from = numbers(st.shape, &mut from);
    let k_to = if b > 0.0 {
        numbers(st.shape_to, &mut to)
    } else {
        k_from
    };
    // (bent less at once where a set was not strange, more again slowly)
    st.bend = if most < st.bend {
        most
    } else {
        (st.bend + dt / 3.0).min(1.0)
    };
    // the old hits fade (the figure's persistence)
    let fade = (-dt / 0.25).exp();
    for d in st.density.iter_mut() {
        *d *= fade;
    }
    // fitted into the grid by where its orbit went (eased: no jump as
    // its shape changes), its aspect kept
    let [x0, x1, y0, y1] = st.bounds;
    let (bx, by) = (((x1 - x0) * 0.5).max(0.2), ((y1 - y0) * 0.5).max(0.2));
    let fit = (w as f32 * 0.46 / bx).min(h as f32 * 0.45 / by);
    let (cx, cy) = (
        w as f32 * 0.5 - (x0 + x1) * 0.5 * fit,
        h as f32 * 0.5 - (y0 + y1) * 0.5 * fit,
    );
    let mut seen = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];
    // (the steps shared between the two orbits as the new one comes)
    let steps_to = (ORBIT_STEPS as f32 * b) as usize;
    for (k, orbit, steps) in [
        (k_from, &mut from, ORBIT_STEPS - steps_to),
        (k_to, &mut to, steps_to),
    ] {
        let [mut x, mut y] = *orbit;
        for _ in 0..steps {
            let nx = (k[0] * y).sin() + k[2] * (k[0] * x).cos();
            let ny = (k[1] * x).sin() + k[3] * (k[1] * y).cos();
            x = nx;
            y = ny;
            seen = [
                seen[0].min(x),
                seen[1].max(x),
                seen[2].min(y),
                seen[3].max(y),
            ];
            let (gx, gy) = (cx + x * fit, cy + y * fit);
            if gx >= 0.0 && gy >= 0.0 {
                let (ix, iy) = (gx as usize, gy as usize);
                if ix < w && iy < h {
                    st.density[iy * w + ix] += 1.0;
                }
            }
        }
        *orbit = [x, y];
    }
    st.orbit = from;
    st.orbit_to = to;
    let ease = 1.0 - (-dt / 0.6).exp();
    if seen.iter().all(|v| v.is_finite()) {
        for i in 0..4 {
            st.bounds[i] += (seen[i] - st.bounds[i]) * ease;
        }
    }
    // the brightest cells (eased): a log scale up to them - to the cells
    // brighter than all but one in two hundred of those the orbit visits,
    // not to the brightest of all: a few cells it comes back to again and
    // again took the scale, and the figure went dark
    let top = bright(&st.density);
    st.density_top += (top - st.density_top) * (1.0 - (-dt / 0.5).exp());
    let scale = 1.0 / (1.0 + st.density_top.max(1.0)).ln();
    if let Ok(mut pic) = picture.lock() {
        if pic.data.len() != w * h {
            pic.data.clear();
            pic.data.resize(w * h, 0);
        }
        pic.width = w as u32;
        pic.height = h as u32;
        for (p, d) in pic.data.iter_mut().zip(&st.density) {
            // (a curve: the threads of the orbit stand out from its haze)
            let v = ((1.0 + d).ln() * scale).clamp(0.0, 1.0);
            *p = (v * v * 255.0) as u8;
        }
        pic.version = pic.version.wrapping_add(1);
    }
}

/// The numbers shape `shape` is drawn with, bent by `turn` as far as
/// `bend` allows (then half, then a quarter of it, then not at all), and
/// how far it was: the first of them whose orbit from `orbit` on is
/// strange and covers at least seven tenths of what the shape itself
/// covers. Bent, or from where the orbit happened to be, a shape can be
/// another figure than the one chosen for it - a thin drawing of a dozen
/// curves, strange by its exponent and a twentieth of the picture: the
/// eighth shape for seconds in techno and metal (review 20, L7). Where
/// even the shape unbent is thin from where the orbit is, the orbit
/// begins anew where the shape was tested.
fn full_shape(shape: usize, turn: [f32; 4], bend: f32, orbit: &mut [f32; 2]) -> ([f32; 4], f32) {
    static COVERED: std::sync::OnceLock<[usize; SHAPES.len()]> = std::sync::OnceLock::new();
    let covered =
        COVERED.get_or_init(|| std::array::from_fn(|i| strangeness(SHAPES[i], 0.1, 0.1, 768).1));
    let least = (covered[shape] * 7 / 10).max(64);
    let full = |k: [f32; 4], orbit: &[f32; 2]| {
        let (wild, cells) = strangeness(k, orbit[0], orbit[1], 768);
        wild > 0.1 && cells >= least
    };
    let base = SHAPES[shape];
    for bend in [bend, bend * 0.5, bend * 0.25] {
        let k = std::array::from_fn(|i| base[i] + turn[i] * bend);
        if bend > 0.0 && full(k, orbit) {
            return (k, bend);
        }
    }
    if !full(base, orbit) {
        *orbit = [0.1, 0.1];
    }
    (base, 0.0)
}

/// Whether the orbit of the attractor with numbers `k` is a strange one
/// from (x, y) on - a figure, not a cycle of a few points: over 768 steps
/// the mean of the log of how far apart two orbits starting side by side
/// are a step later (its largest Lyapunov exponent) is well above zero, and
/// the orbit visits many cells of a coarse grid.
#[cfg(test)]
fn strange(k: [f32; 4], x: f32, y: f32) -> bool {
    let (wild, cells) = strangeness(k, x, y, 768);
    wild > 0.1 && cells >= 64
}

/// The orbit's exponent over `steps` steps, and how many cells of a grid
/// of 32 x 32 over (-3..3)² it visits.
fn strangeness(k: [f32; 4], mut x: f32, mut y: f32, steps: usize) -> (f32, usize) {
    let (mut vx, mut vy) = (0.6f32, 0.8f32);
    let mut sum = 0.0f32;
    let mut grid = [0u32; 32];
    for _ in 0..steps {
        let (j00, j01) = (-k[2] * k[0] * (k[0] * x).sin(), k[0] * (k[0] * y).cos());
        let (j10, j11) = (k[1] * (k[1] * x).cos(), -k[3] * k[1] * (k[1] * y).sin());
        let (wx, wy) = (j00 * vx + j01 * vy, j10 * vx + j11 * vy);
        let stretch = (wx * wx + wy * wy).sqrt().max(1e-12);
        sum += stretch.ln();
        (vx, vy) = (wx / stretch, wy / stretch);
        let nx = (k[0] * y).sin() + k[2] * (k[0] * x).cos();
        let ny = (k[1] * x).sin() + k[3] * (k[1] * y).cos();
        (x, y) = (nx, ny);
        let cell = |v: f32| (((v + 3.0) / 6.0 * 32.0) as usize).min(31);
        grid[cell(y)] |= 1 << cell(x);
    }
    (
        sum / steps as f32,
        grid.iter().map(|row| row.count_ones() as usize).sum(),
    )
}

/// The density that all but one in two hundred of the cells the orbit
/// visits lie under (at least 1).
fn bright(density: &[f32]) -> f32 {
    let most = density.iter().cloned().fold(1.0f32, f32::max);
    const BINS: usize = 64;
    let mut bins = [0u32; BINS];
    let scale = (BINS - 1) as f32 / (1.0 + most).ln();
    let mut visited = 0u32;
    for d in density {
        if *d >= 0.5 {
            bins[((1.0 + d).ln() * scale) as usize] += 1;
            visited += 1;
        }
    }
    let mut above = visited / 200;
    for (k, n) in bins.iter().enumerate().rev() {
        if *n > above {
            return (((k + 1) as f32 / scale).exp() - 1.0).clamp(1.0, most);
        }
        above -= n;
    }
    most
}

// -- the plate (cymatics) -----------------------------------------------------------

fn plate(a: &Analyzer, st: &mut State, dt: f32, tall: f32) {
    // the sound's centre of weight (0 the bass, 1 the treble), eased over
    // a second: higher, a finer figure
    let (mut sum, mut at) = (0.0, 0.0);
    let n = a.bars.len().max(1);
    for (i, v) in a.bars.iter().enumerate() {
        sum += v;
        at += v * i as f32;
    }
    let centre = if sum > 0.01 { at / sum / n as f32 } else { 0.3 };
    let ease = 1.0 - (-dt / 1.0).exp();
    st.centroid += (centre - st.centroid) * ease;
    st.plate_level += (a.energy.min(1.0) - st.plate_level) * (1.0 - (-dt / 0.4).exp());
    st.mode_age += dt;
    // (in a small picture the simpler figures only: many lines close
    // together there are stripes)
    let most = (((tall - 60.0) / 40.0).max(1.0) as usize).min(MODES.len() - 1);
    let want = (((st.centroid * 1.9 - 0.15).clamp(0.0, 1.0) * (MODES.len() - 1) as f32).round()
        as usize)
        .min(most);
    // (a new figure no sooner than two seconds after the last, eased over
    // one and a half: the sand slides into it)
    if st.mode_blend >= 1.0 && want != st.mode_to && st.mode_age > 2.0 {
        st.mode = st.mode_to;
        st.mode_to = if want > st.mode_to {
            st.mode_to + 1
        } else {
            st.mode_to - 1
        };
        st.mode_blend = 0.0;
        st.mode_age = 0.0;
    }
    if st.mode_blend < 1.0 {
        st.mode_blend = (st.mode_blend + dt / 1.5).min(1.0);
        if st.mode_blend >= 1.0 {
            st.mode = st.mode_to;
        }
    }
}

// -- the orbits ----------------------------------------------------------------------

fn orbits(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let calm = super::scheme_calm(s);
    let reach = (r.width() * 0.47).min(r.height() * 1.15);
    let tilt = 0.34;
    let small = (r.height() / 300.0).clamp(0.45, 2.5);
    // dust: far stars, still
    for k in 0..90u32 {
        let p = pos2(
            r.left() + hash(k, 3) * r.width(),
            r.top() + hash(k, 5) * r.height(),
        );
        dot(
            pen,
            p,
            0.6 + 0.8 * hash(k, 7),
            faint(mix(s.background, s.peak, 0.5), s.background),
        );
    }
    // the orbits, faint
    let orbit = faint(mix(s.background, s.low, 0.6), s.background);
    for k in 0..PLANETS {
        let radius = reach * (0.22 + 0.78 * (k + 1) as f32 / PLANETS as f32);
        pen.points.clear();
        for j in 0..96 {
            let t = j as f32 / 96.0 * TAU;
            pen.points
                .push(c + vec2(t.cos() * radius, t.sin() * radius * tilt));
        }
        pen.outline(1.0, orbit);
    }
    // each planet a band of the spectrum, the inner ones faster (Kepler:
    // their period as the orbit's size to the 1.5), all a little faster as
    // it gets louder
    let mut place = [(Pos2::ZERO, 0.0f32, 0.0f32, 0.0f32); PLANETS];
    for k in 0..PLANETS {
        let x = (k as f32 + 0.5) / PLANETS as f32;
        let level = span(a, x - 0.5 / PLANETS as f32, x + 0.5 / PLANETS as f32);
        let size = 0.22 + 0.78 * (k + 1) as f32 / PLANETS as f32;
        let speed = 0.35 / size.powf(1.5) * (0.6 + 0.6 * a.energy.min(1.0));
        st.orbit_turn[k] = (st.orbit_turn[k] + dt * speed) % (TAU * 64.0);
        let angle = st.orbit_turn[k] + k as f32 * 2.1;
        let radius = reach * size;
        let p = c + vec2(angle.cos() * radius, angle.sin() * radius * tilt);
        place[k] = (p, angle.sin(), level, angle);
    }
    let draw_planet = |pen: &mut Pen<'_>, k: usize| {
        let (p, depth, level, angle) = place[k];
        let radius = reach * (0.22 + 0.78 * (k + 1) as f32 / PLANETS as f32);
        let scale = 0.85 + 0.25 * depth;
        let colour = s.at(0.25 + 0.75 * level);
        // the trail behind it along its orbit
        let mut last = p;
        for j in 1..=18 {
            let t = angle - j as f32 * 0.035 * (1.0 + 2.0 / (k + 1) as f32);
            let q = c + vec2(t.cos() * radius, t.sin() * radius * tilt);
            let fade = (1.0 - j as f32 / 19.0).powi(2);
            streak(
                pen,
                last,
                q,
                (2.0 + 4.0 * level) * small * scale * fade,
                shine(s, colour, 0.35 * fade),
                shine(s, colour, 0.3 * fade),
            );
            last = q;
        }
        let body = (3.0 + 4.0 * k as f32 / PLANETS as f32 + 7.0 * level / calm) * small * scale;
        soft(pen, p, body * 3.0, shine(s, colour, 0.18 + 0.25 * level));
        dot(
            pen,
            p,
            body,
            mix(s.background, colour, 0.55 + 0.45 * scale.min(1.0)),
        );
        // lit from the sun: a lighter cap on its side towards it
        let towards = (c - p).normalized() * body * 0.35;
        dot(pen, p + towards, body * 0.62, mix(colour, s.peak, 0.35));
        if k == 4 {
            // (a ringed one)
            pen.points.clear();
            for j in 0..40 {
                let t = j as f32 / 40.0 * TAU;
                pen.points
                    .push(p + vec2(t.cos() * body * 2.1, t.sin() * body * 0.55));
            }
            pen.outline(1.2 * small, mix(s.background, s.high, 0.7));
        }
        if k >= 5 {
            // a moon, its size the treble's
            let m = st_angle(angle, k);
            let q = p + vec2(m.cos() * body * 2.6, m.sin() * body * 1.0);
            dot(
                pen,
                q,
                (1.2 + 2.5 * a.treble) * small,
                mix(s.background, s.peak, 0.8),
            );
        }
    };
    // behind the sun first, then the sun, then those in front
    for k in 0..PLANETS {
        if place[k].1 < 0.0 {
            draw_planet(pen, k);
        }
    }
    let sun = reach * 0.08 * (1.0 + 0.25 * a.bass / calm);
    soft(
        pen,
        c,
        sun * 5.0,
        shine(s, s.high, 0.22 + 0.12 * a.energy.min(1.0)),
    );
    soft(pen, c, sun * 2.2, shine(s, s.peak, 0.4));
    dot(pen, c, sun, mix(s.high, s.peak, 0.55));
    for k in 0..PLANETS {
        if place[k].1 >= 0.0 {
            draw_planet(pen, k);
        }
    }
}

/// A moon's angle round its planet.
fn st_angle(angle: f32, k: usize) -> f32 {
    angle * (3.0 + k as f32) + k as f32
}

// -- the plasma globe ----------------------------------------------------------------

fn filaments(f: &Frame<'_>, pen: &mut Pen<'_>) {
    let unred = f.s.unred();
    let (a, s, r) = (f.a, &unred, f.rect);
    let c = r.center();
    let outer = (r.width().min(r.height()) * 0.47).max(8.0);
    let inner = outer * 0.17;
    let t = a.time as f32;
    let calm = super::scheme_calm(s);
    // the glass: a faint body, its rim, a highlight
    soft(pen, c, outer * 1.15, shine(s, s.low, 0.12));
    pen.ring(
        c,
        outer,
        1.5,
        faint(mix(s.background, s.mid, 0.6), s.background),
    );
    pen.points.clear();
    for j in 0..24 {
        let u = -2.6 + j as f32 / 23.0 * 0.9;
        pen.points.push(c + Vec2::angled(u) * outer * 0.9);
    }
    pen.line(outer * 0.04, shine(s, s.peak, 0.18));
    // the filaments: as many as it is loud, each one a band of the
    // spectrum, wandering; forked once on their way out. Each has a place
    // of its own round the electrode, the golden angle on from the last
    // (any number of them is spread all round), and the last one grows out
    // and draws back as the loudness passes: spaced by their number, all of
    // them jumped - up to 40 degrees - whenever it changed (review 18, L1)
    const GOLDEN: f32 = 2.399_963_2;
    let many = 7.0 + 6.0 * a.energy.min(1.0);
    for i in 0..FILAMENTS {
        let fi = i as f32;
        let there = smooth((many - fi).clamp(0.0, 1.0));
        if there <= 0.0 {
            continue;
        }
        let outer_glass = outer;
        // (growing out: its tip on its way to the glass)
        let outer = inner + (outer - inner) * (0.25 + 0.75 * there);
        let level = band(a, fi / 12.0);
        let strength = (0.25 + 0.75 * level) / calm.sqrt() * there;
        let base = fi * GOLDEN
            + 0.7 * (t * (0.11 + 0.03 * (i % 4) as f32) + fi * 1.7).sin()
            + 0.25 * (t * 0.05 + fi).cos();
        let colour = s.at(0.3 + 0.7 * level);
        let wiggle = outer * (0.05 + 0.03 * a.beat.pulse);
        pen.points.clear();
        let mut fork = Pos2::ZERO;
        let mut fork_dir = 0.0;
        for k in 0..=24 {
            let u = k as f32 / 24.0;
            let along = inner + (outer - inner) * u;
            let env = (PI * u).sin().max(0.0).powf(0.8);
            let side = env
                * wiggle
                * ((u * 7.0 + t * 1.7 + fi * 2.3).sin()
                    + 0.55 * (u * 13.0 - t * 2.3 + fi).sin()
                    + 0.3 * (u * 23.0 + t * 3.1 + fi * 0.7).sin());
            let dir = base + 0.12 * (t * 0.4 + fi).sin() * u;
            let p = c + Vec2::angled(dir) * along + Vec2::angled(dir + PI / 2.0) * side;
            if k == 14 {
                fork = p;
                fork_dir = dir;
            }
            pen.points.push(p);
        }
        let tip = *pen.points.last().unwrap_or(&c);
        pen.line(4.5 + 4.0 * level, shine(s, colour, 0.07 * strength));
        pen.line(1.8 + 1.5 * level, shine(s, colour, 0.25 * strength));
        pen.line(0.9, mix(colour, s.peak, 0.5 * strength.min(1.0)));
        // the fork: a shorter branch off at an angle
        pen.points.clear();
        let turn = if i % 2 == 0 { 0.32 } else { -0.32 };
        for k in 0..=9 {
            let u = k as f32 / 9.0;
            let dir = fork_dir + turn * u;
            let side = (u * 9.0 + t * 2.1 + fi).sin() * wiggle * 0.5 * (PI * u).sin();
            pen.points.push(
                fork + Vec2::angled(dir) * (outer - inner) * 0.36 * u
                    + Vec2::angled(dir + PI / 2.0) * side,
            );
        }
        pen.line(2.0 + 1.5 * level, shine(s, colour, 0.12 * strength));
        pen.line(
            0.7,
            shine(s, mix(colour, s.peak, 0.4), 0.7 * strength.min(1.0)),
        );
        // where it meets the glass
        soft(
            pen,
            tip,
            outer_glass * (0.05 + 0.07 * level) * there,
            shine(s, colour, 0.45 * strength),
        );
    }
    // the electrode
    soft(pen, c, inner * 2.6, shine(s, s.high, 0.45 / calm.sqrt()));
    dot(pen, c, inner, mix(s.mid, s.peak, 0.35));
    dot(
        pen,
        c - vec2(inner, inner) * 0.3,
        inner * 0.45,
        mix(s.high, s.peak, 0.7),
    );
}

// -- the fractal tree ----------------------------------------------------------------

fn tree(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let t = st.clock;
    let calm = super::scheme_calm(s);
    let ground = r.bottom() - r.height() * 0.06;
    // the ground (a hill's soft light) and a halo behind the crown
    pen.vertical(
        Rect::from_x_y_ranges(r.x_range(), ground - r.height() * 0.12..=r.bottom()),
        Color32::TRANSPARENT,
        shine(s, s.low, 0.22),
    );
    let crown = pos2(r.center().x, r.top() + r.height() * 0.42);
    soft(
        pen,
        crown,
        r.height() * 0.6,
        shine(s, s.mid, 0.08 + 0.05 * a.energy.min(1.0)),
    );
    // the wind: the bass's, swaying the twigs more than the trunk
    let wind = 0.03 + 0.16 * a.bass / calm;
    let spread = 0.34 + 0.14 * a.middle;
    let length = r.height() * 0.215;
    let trunk = (length * 0.13).max(2.0);
    // (depth first: a stack of branches to grow; their tips, left to right)
    #[derive(Clone, Copy)]
    struct Branch {
        at: Pos2,
        angle: f32,
        length: f32,
        depth: u8,
        id: u32,
    }
    let mut stack = [Branch {
        at: Pos2::ZERO,
        angle: 0.0,
        length: 0.0,
        depth: 0,
        id: 0,
    }; TREE_DEPTH + 2];
    let mut top = 1;
    stack[0] = Branch {
        at: pos2(r.center().x, ground),
        angle: -PI / 2.0,
        length: length * 0.85,
        depth: 0,
        id: 1,
    };
    st.tips.clear();
    let bark = mix(s.background, s.low, 0.75);
    let twig = mix(s.background, s.mid, 0.8);
    while top > 0 {
        top -= 1;
        let b = stack[top];
        let d = b.depth as f32 / TREE_DEPTH as f32;
        let sway = wind * (t * 1.3 + b.depth as f32 * 0.55 + hash(b.id, 3) * 2.0).sin() * d
            + 0.015 * (t * 0.37).sin();
        let angle = b.angle + sway;
        let end = b.at + Vec2::angled(angle) * b.length;
        // (tapered, its joint rounded)
        let width = (trunk * 0.68f32.powi(b.depth as i32)).max(0.7);
        let colour = mix(bark, twig, d);
        let along = (end - b.at).normalized();
        let n = vec2(-along.y, along.x);
        let (w0, w1) = (width * 0.5, (width * 0.68).max(0.7) * 0.5);
        pen.quad(
            [b.at - n * w0, end - n * w1, end + n * w1, b.at + n * w0],
            [colour; 4],
        );
        if width > 1.5 {
            dot(pen, end, w1, colour);
        }
        if b.depth as usize == TREE_DEPTH {
            st.tips.push(end);
            continue;
        }
        for (side, salt) in [(-1.0, 5u32), (1.0, 7)] {
            if top < stack.len() {
                // (each fork its own: one side wider, one twig longer)
                let wobble = (hash(b.id, salt) - 0.5) * 0.45;
                let wide = 0.65 + 0.7 * hash(b.id, salt + 2);
                stack[top] = Branch {
                    at: end,
                    angle: angle + side * spread * wide * (1.3 - 0.6 * d) + wobble,
                    length: b.length * (0.68 + 0.14 * hash(b.id, salt + 4)),
                    depth: b.depth + 1,
                    id: b.id * 2 + (side > 0.0) as u32,
                };
                top += 1;
            }
        }
    }
    // the blossoms at the tips: across the spectrum, left to right
    let small = (r.height() / 300.0).clamp(0.5, 2.5);
    for (k, p) in st.tips.iter().enumerate() {
        let x = (p.x - r.left()) / r.width();
        let level = band(a, x * 0.9 + 0.05 * hash(k as u32, 9));
        let colour = s.at(0.35 + 0.65 * level);
        let size = (1.2 + 3.2 * level / calm) * small;
        if level > 0.3 {
            soft(pen, *p, size * 3.5, shine(s, colour, 0.12 * level));
        }
        dot(pen, *p, size, mix(colour, s.peak, 0.25 * level));
    }
    // petals fall on the beat
    if beat(a, st, 0.5) && !st.tips.is_empty() {
        for _ in 0..3 {
            let k = (st.random() * st.tips.len() as f32) as usize % st.tips.len();
            let p = st.tips[k];
            if st.petals.len() == PETALS {
                st.petals.remove(0);
            }
            let drift = (st.random() - 0.5) * 0.06;
            let phase = st.random() * TAU;
            st.petals.push([
                (p.x - r.left()) / r.width(),
                (p.y - r.top()) / r.height(),
                drift,
                phase,
                0.0,
            ]);
        }
    }
    for p in st.petals.iter_mut() {
        p[4] += dt;
        p[1] += dt * 0.09;
        p[0] += dt * (p[2] + 0.04 * (p[4] * 2.0 + p[3]).sin());
    }
    st.petals.retain(|p| p[4] < 6.0 && p[1] < 0.95);
    for p in &st.petals {
        let at = pos2(r.left() + p[0] * r.width(), r.top() + p[1] * r.height());
        let fade = (1.0 - p[4] / 6.0).min(p[4] * 4.0).clamp(0.0, 1.0);
        let wide = (2.6 + 0.8 * (p[4] * 3.0 + p[3]).sin()) * small;
        dot(
            pen,
            at,
            wide,
            mix(s.background, mix(s.high, s.peak, 0.3), 0.8 * fade),
        );
    }
}

// -- the constellation ---------------------------------------------------------------

fn constellation(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    if st.stars.len() != STARS {
        st.stars.clear();
        for _ in 0..STARS {
            let (x, y) = (st.random(), st.random());
            let angle = st.random() * TAU;
            let speed = 0.006 + 0.012 * st.random();
            let b = st.random();
            st.stars
                .push([x, y, angle.cos() * speed, angle.sin() * speed, b]);
        }
    }
    let calm = super::scheme_calm(s);
    let t = st.clock;
    // a faint nebula drifting behind
    for k in 0..3u32 {
        let p = pos2(
            r.left() + r.width() * (0.25 + 0.5 * hash(k, 1) + 0.12 * (t * 0.03 + k as f32).sin()),
            r.top() + r.height() * (0.3 + 0.4 * hash(k, 2)),
        );
        let colour = [s.low, s.mid, s.high][k as usize];
        soft(
            pen,
            p,
            r.height() * (0.5 + 0.2 * hash(k, 3)),
            shine(s, colour, 0.07),
        );
    }
    let speed = 0.6 + 0.8 * a.energy.min(1.0);
    for star in st.stars.iter_mut() {
        star[0] = (star[0] + star[2] * dt * speed).rem_euclid(1.0);
        star[1] = (star[1] + star[3] * dt * speed).rem_euclid(1.0);
    }
    // (the stars' places with a margin round the picture: none pops in at
    // an edge)
    let place = |star: &[f32; 5]| {
        pos2(
            r.left() - 20.0 + star[0] * (r.width() + 40.0),
            r.top() - 20.0 + star[1] * (r.height() + 40.0),
        )
    };
    // (a star's reach by the picture's size alone, and fewer stars in a
    // small picture: with 30 points of it whatever the size, nearly every
    // star at 240 x 135 was joined to every neighbour - a tangle; review
    // 18, L6)
    let reach = r.width().min(r.height()) * (0.3 + 0.05 * a.energy.min(1.0));
    let small = (r.height() / 300.0).clamp(0.5, 2.5);
    let shown = ((r.area() / (620.0 * 300.0)).sqrt().clamp(0.5, 1.0) * STARS as f32) as usize;
    // the links: near stars joined, the nearer and the louder their bands
    // the stronger
    for i in 0..shown {
        let p = place(&st.stars[i]);
        let li = band(a, st.stars[i][4]);
        for j in i + 1..shown {
            let q = place(&st.stars[j]);
            let d = (q - p).length();
            if d >= reach {
                continue;
            }
            let lj = band(a, st.stars[j][4]);
            let near = 1.0 - d / reach;
            let k = near * near * (0.25 + 0.75 * (li + lj) * 0.5) / calm;
            let colour = s.at(0.3 + 0.6 * (li + lj) * 0.5);
            // (light added: over the nebula a faint link is no dark line)
            pen.segment(
                p,
                q,
                (1.0 + 0.6 * near) * small.clamp(0.7, 1.0),
                shine(s, colour, (0.9 * k).min(0.9)),
            );
        }
    }
    // the stars: their size their band's; the loudest with a halo
    for i in 0..shown {
        let p = place(&st.stars[i]);
        let level = band(a, st.stars[i][4]);
        let colour = s.at(0.4 + 0.6 * level);
        let size = (1.3 + 2.8 * level / calm) * small;
        if level > 0.45 {
            soft(pen, p, size * 4.0, shine(s, colour, 0.25 * level));
        }
        dot(pen, p, size, mix(colour, s.peak, 0.4 * level));
    }
}

// -- bokeh ---------------------------------------------------------------------------

fn bokeh(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    if st.lights.len() != LIGHTS {
        st.lights.clear();
        for k in 0..LIGHTS {
            let depth = 0.25 + 0.75 * st.random();
            let (x, y) = (st.random(), st.random());
            let b = (k as f32 + st.random()) / LIGHTS as f32;
            let phase = st.random() * TAU;
            let tint = st.random();
            st.lights.push([x, y, depth, b, phase, tint]);
        }
        // far ones first
        st.lights.sort_by(|p, q| p[2].total_cmp(&q[2]));
    }
    let calm = super::scheme_calm(s);
    // the ground: darker below, as a street at night out of focus
    pen.vertical(r, mix(s.background, s.low, 0.12), s.background);
    let t = st.clock;
    let side = r.width().min(r.height());
    for light in st.lights.iter_mut() {
        // (drifting up, the near ones faster)
        light[1] -= dt * (0.01 + 0.03 * light[2]) * (0.7 + 0.6 * a.energy.min(1.0));
        if light[1] < -0.15 {
            light[1] += 1.3;
        }
    }
    for light in &st.lights {
        let [x, y, depth, b, phase, tint] = *light;
        let level = band(a, b);
        let at = pos2(
            r.left() + (x + 0.03 * (t * 0.2 + phase).sin() * depth) * r.width(),
            r.top() + y * r.height(),
        );
        let radius = side * (0.03 + 0.085 * depth) * (0.85 + 0.3 * level);
        let colour = s.at(0.25 + 0.6 * tint + 0.15 * level);
        let k = (0.1 + 0.28 * level / calm) * (0.55 + 0.45 * depth);
        if depth < 0.4 {
            // (the farthest further out of focus: round, soft)
            soft(pen, at, radius * 1.7, shine(s, colour, k * 0.9));
            continue;
        }
        // a hexagon (six blades of the lens' iris), brighter at its rim,
        // its edge soft
        let i0 = pen.mesh.vertices.len() as u32;
        let turn = 0.3;
        pen.mesh.colored_vertex(at, shine(s, colour, k * 0.55));
        for j in 0..6 {
            let u = Vec2::angled(turn + j as f32 / 6.0 * TAU);
            pen.mesh
                .colored_vertex(at + u * radius * 0.82, shine(s, colour, k * 0.75));
            pen.mesh
                .colored_vertex(at + u * radius, shine(s, colour, k * 1.25));
            pen.mesh.colored_vertex(
                at + u * (radius + 1.0 + 0.08 * radius),
                Color32::TRANSPARENT,
            );
        }
        for j in 0..6u32 {
            let (a0, b0, c0) = (i0 + 1 + 3 * j, i0 + 2 + 3 * j, i0 + 3 + 3 * j);
            let n = (j + 1) % 6;
            let (a1, b1, c1) = (i0 + 1 + 3 * n, i0 + 2 + 3 * n, i0 + 3 + 3 * n);
            pen.mesh.add_triangle(i0, a0, a1);
            pen.mesh.add_triangle(a0, b0, b1);
            pen.mesh.add_triangle(a0, b1, a1);
            pen.mesh.add_triangle(b0, c0, c1);
            pen.mesh.add_triangle(b0, c1, b1);
        }
    }
    // a few lights in focus, sharp and small, drifting slowly
    let small = (r.height() / 300.0).clamp(0.5, 2.5);
    for k in 0..14u32 {
        let x = (hash(k, 21) + t * 0.004 * (0.5 + hash(k, 22))).rem_euclid(1.0);
        let y = (hash(k, 23) - t * 0.01 * (0.5 + hash(k, 24))).rem_euclid(1.0);
        let at = pos2(r.left() + x * r.width(), r.top() + y * r.height());
        let level = band(a, hash(k, 25));
        let colour = mix(s.at(0.5 + 0.5 * level), s.peak, 0.4);
        soft(pen, at, 7.0 * small, shine(s, colour, 0.2 + 0.25 * level));
        if dark(s) {
            // (on a light ground a point of light would be a speck)
            dot(pen, at, (1.0 + 0.8 * level) * small, colour);
        }
    }
}

// -- the torus knot ------------------------------------------------------------------

fn knot(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let unred = f.s.unred();
    let (a, s, r) = (f.a, &unred, f.rect);
    // a new winding each phrase (16 beats; 20 s without a beat), the knot
    // turning into it over three seconds
    st.knot_age += dt;
    if st.knot_blend >= 1.0 && (a.beat.count >= st.knot_beat + 16 || st.knot_age > 20.0) {
        st.knot = st.knot_to;
        st.knot_to = (st.knot_to + 1) % KNOTS.len();
        st.knot_blend = 0.0;
        st.knot_beat = a.beat.count;
        st.knot_age = 0.0;
    }
    st.knot_blend = (st.knot_blend + dt / 3.0).min(1.0);
    let b = smooth(st.knot_blend);
    st.knot_turn = (st.knot_turn + dt * (0.25 + 0.35 * a.energy.min(1.0))) % (TAU * 100.0);
    let (spin, pitch) = (st.knot_turn, 0.5 + 0.25 * (st.clock * 0.13).sin());
    let yaw = 0.25 * (st.clock * 0.09).sin();
    let c = r.center();
    let size = r.width().min(r.height()) * 0.15;
    let calm = super::scheme_calm(s);
    let at = |pq: (f32, f32), u: f32| {
        let (p, q) = pq;
        let ring = (q * u).cos() + 2.0;
        [ring * (p * u).cos(), ring * (p * u).sin(), -(q * u).sin()]
    };
    st.knot_points.clear();
    for k in 0..KNOT_POINTS {
        let u = k as f32 / KNOT_POINTS as f32 * TAU;
        let (p0, p1) = (at(KNOTS[st.knot], u), at(KNOTS[st.knot_to], u));
        let mut v = [0.0; 3];
        for i in 0..3 {
            v[i] = p0[i] + (p1[i] - p0[i]) * b;
        }
        // turned in its own plane, tipped towards the viewer, swaying
        let (ss, cs) = spin.sin_cos();
        let (x, y) = (v[0] * cs - v[1] * ss, v[0] * ss + v[1] * cs);
        let (sp, cp) = pitch.sin_cos();
        let (y, z) = (y * cp - v[2] * sp, y * sp + v[2] * cp);
        let (sy, cy) = yaw.sin_cos();
        let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
        let near = 1.0 / (1.0 - z * 0.12);
        st.knot_points
            .push([c.x + x * size * near, c.y + y * size * near, z]);
    }
    st.knot_order.clear();
    for k in 0..KNOT_POINTS {
        let z = (st.knot_points[k][2] + st.knot_points[(k + 1) % KNOT_POINTS][2]) * 0.5;
        st.knot_order.push((z, k as u16));
    }
    st.knot_order.sort_unstable_by(|x, y| x.0.total_cmp(&y.0));
    let tube = size * (0.22 + 0.12 * a.bass / calm);
    soft(
        pen,
        c,
        size * 4.0,
        shine(s, s.low, 0.12 + 0.06 * a.energy.min(1.0)),
    );
    for &(z, k) in &st.knot_order {
        let k = k as usize;
        let (p, q) = (st.knot_points[k], st.knot_points[(k + 1) % KNOT_POINTS]);
        let (p, q) = (pos2(p[0], p[1]), pos2(q[0], q[1]));
        let depth = ((z + 1.0) * 0.5).clamp(0.0, 1.0);
        // the colour along it: the spectrum there and back
        let x = 1.0 - (2.0 * k as f32 / KNOT_POINTS as f32 - 1.0).abs();
        let level = band(a, x);
        let colour = s.at(0.2 + 0.8 * level);
        let width = tube * (0.55 + 0.45 * depth) * (0.85 + 0.35 * level);
        let body = mix(s.background, colour, 0.3 + 0.7 * depth);
        dot(pen, p, width * 0.5, body);
        streak(pen, p, q, width, body, body);
        let shine_on = mix(body, s.peak, 0.45 * depth);
        let lift = vec2(-0.18, -0.22) * width;
        streak(pen, p + lift, q + lift, width * 0.28, shine_on, shine_on);
    }
}

// -- the sunflower -------------------------------------------------------------------

fn sunflower(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let outer = r.width().min(r.height()) * 0.31;
    let calm = super::scheme_calm(s);
    st.sun_turn = (st.sun_turn + dt * (0.04 + 0.12 * a.energy.min(1.0))) % (TAU * 100.0);
    // a ripple out from the middle on the beat (at most one in 0.6 s: a
    // ripple passing is a rise and a fall)
    if beat(a, st, 0.6) {
        let strength = 0.4 + 0.6 * a.beat.strength.min(1.0);
        if let Some(slot) = st.ripples.iter_mut().max_by(|x, y| x[0].total_cmp(&y[0])) {
            *slot = [0.0, strength];
        }
    }
    for ripple in st.ripples.iter_mut() {
        ripple[0] += dt;
    }
    // the petals behind: 34 (a Fibonacci number), their length the treble's
    let petals = 34;
    for k in 0..petals {
        let x = 0.5 + 0.5 * k as f32 / petals as f32;
        let level = band(a, x);
        let angle = k as f32 / petals as f32 * TAU + st.sun_turn * 0.3;
        let long = outer * (0.32 + 0.3 * level / calm);
        let u = Vec2::angled(angle);
        let side = Vec2::angled(angle + PI / 2.0);
        let base = c + u * outer * 0.92;
        let wide = outer * 0.11;
        pen.points.clear();
        for j in 0..=10 {
            let v = j as f32 / 10.0;
            let w = (PI * v).sin().max(0.0).powf(0.7) * wide;
            pen.points.push(base + u * long * v + side * w);
        }
        for j in (1..10).rev() {
            let v = j as f32 / 10.0;
            let w = (PI * v).sin().max(0.0).powf(0.7) * wide;
            pen.points.push(base + u * long * v - side * w);
        }
        let tint = mix(s.background, s.at(0.55 + 0.45 * level), 0.55 + 0.3 * level);
        pen.fan(
            base + u * long * 0.4,
            mix(tint, s.peak, 0.15),
            mix(s.background, tint, 0.7),
        );
    }
    // the seeds: the golden angle apart, the inner ones the bass, the
    // outer the treble
    const GOLDEN: f32 = 2.399_963;
    let seed = outer / (SEEDS as f32).sqrt();
    dot(pen, c, outer * 1.02, mix(s.background, s.low, 0.35));
    for k in 0..SEEDS {
        let along = ((k as f32 + 0.5) / SEEDS as f32).sqrt();
        let radius = outer * along;
        let angle = k as f32 * GOLDEN + st.sun_turn;
        let p = c + Vec2::angled(angle) * radius;
        let level = band(a, along);
        let mut swell = 0.0;
        for [age, strength] in st.ripples {
            let front = age * outer * 0.9;
            let d = (radius - front) / (outer * 0.09);
            swell += strength * (-d * d).exp() * (1.0 - age / 1.6).max(0.0);
        }
        // (nearly touching: their spirals - 21 one way, 34 the other - show)
        let size = seed * (0.66 + 0.2 * level / calm + 0.1 * swell) * (0.8 + 0.3 * along);
        let colour = s.at(0.15 + 0.7 * level + 0.15 * swell.min(1.0));
        dot(
            pen,
            p,
            size,
            mix(mix(s.background, s.low, 0.5), colour, 0.45 + 0.55 * along),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_of_the_attractor_is_a_strange_one() {
        // (as the face asks it of every set of numbers it draws: window
        // after window along the orbit, and over a long stretch of it)
        for (n, k) in SHAPES.iter().enumerate() {
            let (wild, cells) = strangeness(*k, 0.1, 0.1, 40_000);
            assert!(
                wild > 0.15 && cells >= 120,
                "shape {n} {k:?}: exponent {wild}, {cells} cells of 1024"
            );
            let (mut x, mut y) = (0.1f32, 0.1f32);
            for window in 0..200 {
                assert!(
                    strange(*k, x, y),
                    "shape {n} {k:?}: not strange in window {window} from ({x}, {y}): {:?}",
                    strangeness(*k, x, y, 768)
                );
                // (and full, by the face's own rule: the orbit is not
                // begun anew anywhere along the shape's own figure)
                let mut orbit = [x, y];
                full_shape(n, [0.0; 4], 0.0, &mut orbit);
                assert_eq!(
                    orbit,
                    [x, y],
                    "shape {n}: taken for thin in window {window}"
                );
                for _ in 0..997 {
                    (x, y) = (
                        (k[0] * y).sin() + k[2] * (k[0] * x).cos(),
                        (k[1] * x).sin() + k[3] * (k[1] * y).cos(),
                    );
                }
            }
        }
    }

    #[test]
    fn a_shape_bent_or_begun_anywhere_is_still_the_full_figure() {
        // (review 20, L7: the eighth shape, bent, was for seconds a thin
        // drawing a third of the picture wide - strange by its exponent,
        // a twentieth of the picture inked.) Every shape, bent every way
        // the sound can bend it, from a thousand places: what is drawn
        // covers seven tenths of what the shape covers by itself.
        let mut seed = 12_345u32;
        let mut random = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut thin = 0;
        for (n, k) in SHAPES.iter().enumerate() {
            let alone = strangeness(*k, 0.1, 0.1, 768).1;
            for _ in 0..1000 {
                let turn = [
                    0.08 * (random() * 2.0 - 1.0),
                    0.035 * random(),
                    0.085 * (random() * 2.0 - 1.0),
                    0.03 * (random() * 2.0 - 1.0),
                ];
                let mut orbit = [random() * 4.0 - 2.0, random() * 4.0 - 2.0];
                let (drawn, _) = full_shape(n, turn, 1.0, &mut orbit);
                let cells = strangeness(drawn, orbit[0], orbit[1], 768).1;
                assert!(
                    cells >= alone * 7 / 10,
                    "shape {n} as {drawn:?} from {orbit:?}: {cells} cells, {alone} alone"
                );
                // (and what the rule before this one let through)
                let bent: [f32; 4] = std::array::from_fn(|i| k[i] + turn[i]);
                let (wild, cells) = strangeness(bent, orbit[0], orbit[1], 768);
                thin += (wild > 0.1 && cells >= 64 && cells < alone * 7 / 10) as usize;
            }
        }
        println!("{thin} of 8000 bent shapes were strange and thin");
    }

    #[test]
    fn a_cycle_is_not_taken_for_a_strange_orbit() {
        // (numbers between two shapes, as they were led from one to the
        // next: a cycle of a few points)
        let mut cycles = 0;
        for step in 1..40 {
            let b = step as f32 / 40.0;
            let (p, q) = (SHAPES[0], SHAPES[5]);
            let k: [f32; 4] = std::array::from_fn(|i| p[i] + (q[i] - p[i]) * b);
            // (settled on what it is drawn to)
            let (mut x, mut y) = (0.1f32, 0.1f32);
            for _ in 0..20_000 {
                (x, y) = (
                    (k[0] * y).sin() + k[2] * (k[0] * x).cos(),
                    (k[1] * x).sin() + k[3] * (k[1] * y).cos(),
                );
            }
            let (wild, cells) = strangeness(k, x, y, 20_000);
            let figure = wild > 0.1 && cells >= 64;
            assert_eq!(
                strange(k, x, y),
                figure,
                "{k:?}: exponent {wild}, {cells} cells over a long stretch"
            );
            cycles += !figure as usize;
        }
        assert!(cycles > 0, "no cycle between two shapes: nothing was tried");
    }
}
