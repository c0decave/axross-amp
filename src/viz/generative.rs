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
use super::{Frame, Pen, VizState, mix, random};
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
    seed: u32,
    last_time: f64,
    clock: f32,
    flow: Vec<Particle>,
    stars: Vec<[f32; 4]>,
    galaxy_turn: f32,
    synth_scroll: f32,
    buildings: Vec<Building>,
    buildings_for: f32,
    /// age, strength, colour (0..1), radius speed
    rings: Vec<[f32; 4]>,
    /// x, y, vx, vy, age (in the face's units)
    sparks: Vec<[f32; 5]>,
    last_beat: u64,
    since_ring: f32,
    drops: [[f32; 4]; 8],
    next_drop: usize,
    since_drop: f32,
    morph: (usize, usize, f32),
    morph_turn: f32,
    spiro: (f32, f32, f32),
    spiro_turn: f32,
    arcs: [f32; 24],
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
            galaxy_turn: 0.0,
            synth_scroll: 0.0,
            buildings: Vec::new(),
            buildings_for: 0.0,
            rings: Vec::with_capacity(RINGS),
            sparks: Vec::with_capacity(SPARKS),
            last_beat: 0,
            since_ring: 0.0,
            drops: [[0.0; 4]; 8],
            next_drop: 0,
            since_drop: 0.0,
            morph: (3, 4, 1.0),
            morph_turn: 0.0,
            spiro: (0.4, 0.4, 0.6),
            spiro_turn: 0.0,
            arcs: [0.0; 24],
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
    let cv = &mut state.canvas;
    let st = &mut state.generative;
    let p = f.painter;
    match key {
        "flowfield" => cv.draw(p, |pen| flowfield(f, pen, st, dt)),
        "synthwave" => cv.draw(p, |pen| synthwave(f, pen, st, dt)),
        "galaxy" => cv.draw(p, |pen| galaxy(f, pen, st, dt)),
        "arcs" => cv.draw(p, |pen| arcs(f, pen, st, dt)),
        "skyline" => cv.draw(p, |pen| skyline(f, pen, st)),
        "isometric" => cv.draw(p, |pen| isometric(f, pen)),
        "helix" => cv.draw(p, |pen| helix(f, pen)),
        "spirograph" => cv.draw(p, |pen| spirograph(f, pen, st, dt)),
        "morph" => cv.draw(p, |pen| morph(f, pen, st, dt)),
        "mandala" => cv.draw(p, |pen| mandala(f, pen)),
        "beatrings" => cv.draw(p, |pen| beatrings(f, pen, st, dt)),
        _ => cv.draw(p, |pen| typography(f, pen, st, dt)),
    }
}

/// An additive colour (premultiplied, alpha 0): it lights what is below.
fn glow(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (c.r() as f32 * k) as u8,
        (c.g() as f32 * k) as u8,
        (c.b() as f32 * k) as u8,
        0,
    )
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

fn third(a: &Analyzer, k: usize) -> f32 {
    let n = a.bars.len().max(1);
    let (from, to) = (k * n / 3, ((k + 1) * n / 3).max(k * n / 3 + 1).min(n));
    a.bars[from..to].iter().sum::<f32>() / (to - from) as f32
}

// -- the shader faces ----------------------------------------------------------------

fn shaded(f: &Frame<'_>, shader: Shader, state: &mut VizState, dt: f32) {
    let st = &mut state.generative;
    let a = f.a;
    let mut picture = false;
    match shader {
        Shader::Ripples => {
            // a drop at each beat (and now and then without one)
            st.since_drop += dt;
            for d in st.drops.iter_mut() {
                d[2] += dt;
                if d[2] > 4.0 {
                    d[3] = 0.0;
                }
            }
            let beat = a.beat.count != st.last_beat;
            st.last_beat = a.beat.count;
            if beat || (st.since_drop > 1.1 && a.energy > 0.05) {
                st.since_drop = 0.0;
                let (x, y) = (0.15 + 0.7 * st.random(), 0.15 + 0.7 * st.random());
                let strength = if beat {
                    0.6 + 0.6 * a.beat.strength
                } else {
                    0.35
                };
                st.drops[st.next_drop] = [x, y, 0.0, strength];
                st.next_drop = (st.next_drop + 1) % st.drops.len();
            }
        }
        Shader::Reaction => {
            reaction(a, st, dt, &state.gpu.picture);
            picture = true;
        }
        _ => {}
    }
    let u = Uniforms::new(shader, a, f.s, &st.drops, 0.0);
    let clip = f.rect.expand(4.0);
    let (gpu, painter, rect) = (&state.gpu, f.painter, f.rect);
    state.canvas.draw(painter, |pen| {
        gpu::paint(painter, pen, gpu, clip, clip, u, picture)
    });
    let _ = rect;
}

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
            let (xx, yy) = ((x + dx + RD_W - r) % RD_W, (y + dy + RD_H - r) % RD_H);
            st.rd_v[yy * RD_W + xx] = 0.9;
            st.rd_u[yy * RD_W + xx] = 0.3;
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
    for (k, q) in st.flow.iter().enumerate() {
        // fading in as it is born and out as it ends
        let life = (q.age / 0.5).min(1.0) * ((q.life - q.age) / 0.8).clamp(0.0, 1.0);
        let hue =
            s.at(
                0.25 + 0.75 * ((k as f32 * 0.618).fract() * 0.6 + band(a, q.trail[q.head].y) * 0.4)
            );
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
    for k in 0..4 {
        pen.circle(c, radius * (1.15 + 0.12 * k as f32), glow(s.high, 0.07));
    }
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
        let hh = half((top + bottom) / 2.0);
        let t = (mid - (c.y - radius)) / (2.0 * radius);
        pen.rect(
            Rect::from_min_max(pos2(c.x - hh, top), pos2(c.x + hh, bottom)),
            mix(s.peak, s.high, t.clamp(0.0, 1.0)),
        );
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
    let line_colour = mix(s.background, s.mid, 0.85);
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
    for k in 0..16 {
        let z = k as f32 + 1.0 - st.synth_scroll;
        let y = horizon + (r.bottom() - horizon) * (1.0 / z).min(1.0) * 0.98;
        let fade = (1.0 / z).sqrt();
        pen.rect(
            Rect::from_x_y_ranges(r.x_range(), y - 0.6..=y + 0.6),
            mix(s.background, s.mid, fade * (0.6 + 0.4 * a.beat.pulse)),
        );
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
    // the core
    let core = size * (0.12 + 0.1 * a.bass + 0.05 * a.beat.pulse);
    for k in 0..6 {
        pen.circle(
            c,
            core * (1.0 + k as f32 * 0.6),
            glow(s.mid, 0.16 - k as f32 * 0.02),
        );
    }
    pen.circle(c, core * 0.45, glow(s.peak, 0.6));
    for [radius, angle, bright, tint] in &st.stars {
        // inner stars turn faster
        let turn = angle + st.galaxy_turn * (1.0 / (radius + 0.3));
        let level = band(a, *radius);
        let p = c + vec2(turn.cos(), turn.sin() * 0.55) * (radius * size);
        let k = bright * (0.35 + 0.9 * level);
        let colour = s.at(0.2 + 0.8 * (1.0 - radius) * 0.6 + tint * 0.3);
        let d = 1.0 + 1.6 * level * bright;
        pen.rect(Rect::from_center_size(p, vec2(d, d)), glow(colour, k));
    }
}

// -- arcs ----------------------------------------------------------------------------

fn arcs(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let outer = r.width().min(r.height()) / 2.0 - 4.0;
    let count = st.arcs.len();
    let spacing = outer * 0.85 / count as f32;
    for k in 0..count {
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        st.arcs[k] += dt * dir * (0.25 + 0.04 * k as f32) * (1.0 + 2.0 * a.energy + a.beat.pulse);
        let level = band(a, k as f32 / (count - 1) as f32);
        let radius = outer * 0.15 + spacing * k as f32;
        let sweep = (0.08 + 0.85 * level) * TAU * 0.8;
        let start = st.arcs[k];
        let width = spacing * 0.62;
        let colour = s.at(level);
        let steps = ((sweep * radius / 5.0) as usize).clamp(4, 160);
        // the track, faint
        pen.ring(c, radius, width * 0.3, mix(s.background, s.low, 0.12));
        for i in 0..steps {
            let t0 = start + sweep * i as f32 / steps as f32;
            let t1 = start + sweep * (i + 1) as f32 / steps as f32;
            let (u0, u1) = (vec2(t0.cos(), t0.sin()), vec2(t1.cos(), t1.sin()));
            // brighter towards its head
            let k0 = 0.35 + 0.65 * i as f32 / steps as f32;
            let k1 = 0.35 + 0.65 * (i + 1) as f32 / steps as f32;
            let (c0, c1) = (mix(s.background, colour, k0), mix(s.background, colour, k1));
            pen.quad(
                [
                    c + u0 * (radius - width / 2.0),
                    c + u1 * (radius - width / 2.0),
                    c + u1 * (radius + width / 2.0),
                    c + u0 * (radius + width / 2.0),
                ],
                [c0, c1, c1, c0],
            );
        }
        let end = start + sweep;
        pen.circle(c + vec2(end.cos(), end.sin()) * radius, width * 0.5, colour);
        pen.circle(
            c + vec2(end.cos(), end.sin()) * radius,
            width * 0.22,
            s.peak,
        );
    }
    pen.circle(c, outer * 0.08 * (1.0 + a.beat.pulse), s.at(a.energy));
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
    // the sky and the moon
    pen.vertical(
        Rect::from_min_max(r.min, pos2(r.right(), water)),
        s.background,
        mix(s.background, s.low, 0.35),
    );
    let moon = pos2(r.left() + w * 0.8, r.top() + h * 0.2);
    for k in 0..4 {
        pen.circle(
            moon,
            h * (0.06 + 0.035 * k as f32) * (1.0 + 0.3 * a.beat.pulse),
            glow(s.peak, 0.06),
        );
    }
    pen.circle(moon, h * 0.055, mix(s.peak, s.high, 0.15));
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
            if b.back || mirror {
                continue;
            }
            // the windows: more of them lit with the treble, flickering
            let (cols, rows) = (
                ((b.width - 4.0) / 6.0) as u32,
                ((height - 6.0) / 8.0) as u32,
            );
            for i in 0..cols {
                for j in 0..rows {
                    let lit = hash(b.seed * 977 + i * 31 + j, step / 3 + (i + j) % 5);
                    if lit < 0.25 + 0.6 * treble * level.max(0.3) {
                        let x = body.left() + 3.0 + i as f32 * 6.0;
                        let y = body.bottom() - 6.0 - (j + 1) as f32 * 8.0 + 3.0;
                        let c = if lit < 0.05 {
                            s.peak
                        } else {
                            s.at(0.55 + 0.45 * lit)
                        };
                        pen.rect(Rect::from_min_size(pos2(x, y), vec2(3.0, 4.0)), c);
                    }
                }
            }
        }
    };
    draw_city(pen, false);
    // the water: the city's reflection, broken into rippling lines
    pen.rect(
        Rect::from_min_max(pos2(r.left(), water), r.max),
        mix(s.background, s.low, 0.08),
    );
    draw_city(pen, true);
    let t = a.time as f32;
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
    pen.rect(
        Rect::from_x_y_ranges(r.x_range(), water - 0.5..=water + 0.5),
        mix(s.background, s.mid, 0.6),
    );
}

// -- isometric bars ------------------------------------------------------------------

fn isometric(f: &Frame<'_>, pen: &mut Pen<'_>) {
    let (a, s, r) = (f.a, f.s, f.rect);
    const COLS: usize = 16;
    const ROWS: usize = 14;
    let n = a.bars.len().max(1);
    let cell =
        (r.width() / (COLS + ROWS) as f32 * 1.9).min(r.height() / (COLS + ROWS) as f32 * 3.6);
    let (cw, ch) = (cell * 0.5, cell * 0.28);
    let origin = pos2(
        r.center().x + (COLS as f32 - ROWS as f32) * cw * 0.5 * 0.0,
        r.bottom() - (COLS + ROWS) as f32 * ch - 4.0,
    );
    let tall = r.height() * 0.45;
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

fn helix(f: &Frame<'_>, pen: &mut Pen<'_>) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let t = a.time as f32;
    const POINTS: usize = 90;
    let amp = r.height() * 0.3 * (0.85 + 0.25 * a.bass + 0.1 * a.beat.pulse);
    let cy = r.center().y;
    let spin = t * (0.6 + 1.5 * a.energy);
    let at = |k: usize, strand: f32| {
        let u = k as f32 / (POINTS - 1) as f32;
        let theta = u * TAU * 2.2 + spin + strand * PI;
        let wobble = (u * 7.0 + t * 0.7).sin() * r.height() * 0.03;
        (
            pos2(r.left() + u * r.width(), cy + theta.sin() * amp + wobble),
            theta.cos(),
        )
    };
    // the far halves first, then the rungs, then the near halves
    for near in [false, true] {
        for k in 0..POINTS - 1 {
            for strand in [0.0, 1.0] {
                let (p, z) = at(k, strand);
                let (q, z2) = at(k + 1, strand);
                if (z + z2 > 0.0) != near {
                    continue;
                }
                let depth = 0.5 + 0.5 * (z + z2) / 2.0;
                let colour = mix(
                    s.background,
                    if strand == 0.0 { s.mid } else { s.high },
                    0.3 + 0.7 * depth,
                );
                pen.segment(p, q, 2.0 + 3.0 * depth, colour);
            }
            if !near && k % 3 == 0 {
                let (p, z) = at(k, 0.0);
                let (q, _) = at(k, 1.0);
                let level = band(a, k as f32 / (POINTS - 1) as f32);
                let depth = 0.5 + 0.5 * z.abs();
                let mid = p + (q - p) * 0.5;
                pen.segment(
                    p,
                    mid,
                    1.0 + 3.0 * level,
                    mix(s.background, s.at(level), 0.4 + 0.6 * depth),
                );
                pen.segment(
                    mid,
                    q,
                    1.0 + 3.0 * level,
                    mix(s.background, s.at(1.0 - level * 0.5), 0.4 + 0.6 * depth),
                );
            }
        }
    }
    for k in (0..POINTS).step_by(3) {
        for strand in [0.0, 1.0] {
            let (p, z) = at(k, strand);
            if z > 0.0 {
                pen.circle(p, 2.0 + 3.0 * z, mix(s.background, s.peak, 0.5 + 0.5 * z));
            }
        }
    }
}

// -- the spirograph ------------------------------------------------------------------

fn spirograph(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    // the wheel's size changes now and then on the beat, smoothly
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
        if a.beat.count % 8 == 0 {
            st.spiro.1 = RATIOS[(a.beat.count / 8) as usize % RATIOS.len()];
        }
    }
    let follow = 1.0 - (-dt * 1.5).exp();
    st.spiro.0 += (st.spiro.1 - st.spiro.0) * follow;
    st.spiro.2 += ((0.45 + 0.5 * a.energy) - st.spiro.2) * follow;
    st.spiro_turn += dt * (0.1 + 0.4 * a.energy);
    let c = r.center();
    let size = r.width().min(r.height()) / 2.0 - 6.0;
    for layer in 0..3 {
        let ratio = st.spiro.0 * (1.0 + 0.07 * layer as f32);
        let d = st.spiro.2 * (1.0 - 0.15 * layer as f32);
        let scale = size / (1.0 - ratio + d).max(0.1);
        let turn =
            st.spiro_turn * (1.0 + 0.3 * layer as f32) * if layer % 2 == 0 { 1.0 } else { -1.0 };
        const STEPS: usize = 1400;
        let laps = 14.0;
        let chunks = 28;
        for chunk in 0..chunks {
            pen.points.clear();
            for i in chunk * STEPS / chunks..=((chunk + 1) * STEPS / chunks).min(STEPS - 1) {
                let t = i as f32 / STEPS as f32 * TAU * laps;
                let k = (1.0 - ratio) / ratio;
                let x = (1.0 - ratio) * t.cos() + d * (k * t).cos();
                let y = (1.0 - ratio) * t.sin() - d * (k * t).sin();
                let (sin, cos) = turn.sin_cos();
                pen.points
                    .push(c + vec2(x * cos - y * sin, x * sin + y * cos) * scale);
            }
            let level = band(a, chunk as f32 / chunks as f32);
            let colour = mix(
                s.background,
                s.at(0.2 + 0.8 * level),
                0.35 + 0.25 * (2 - layer) as f32 + 0.3 * a.beat.pulse,
            );
            pen.line(1.0 + 0.6 * (2 - layer) as f32, colour);
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
    let (a, s, r) = (f.a, f.s, f.rect);
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
    let size = r.width().min(r.height()) / 2.0 - 6.0;
    const COPIES: usize = 16;
    const POINTS: usize = 120;
    for copy in (0..COPIES).rev() {
        let k = copy as f32 / COPIES as f32;
        let scale = size * (1.0 - k * 0.92) * (1.0 + 0.06 * a.beat.pulse * (1.0 - k));
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
        let colour = mix(s.background, s.at(1.0 - k), 0.25 + 0.75 * (1.0 - k));
        pen.outline(1.2 + 1.5 * (1.0 - k), colour);
    }
}

// -- the mandala ---------------------------------------------------------------------

fn mandala(f: &Frame<'_>, pen: &mut Pen<'_>) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let size = r.width().min(r.height()) / 2.0 - 4.0;
    let t = a.time as f32;
    let (bass, middle, treble) = (third(a, 0), third(a, 1), third(a, 2));
    const FOLD: usize = 12;
    let petal =
        |pen: &mut Pen<'_>, angle: f32, inner: f32, outer: f32, width: f32, colour: Color32| {
            pen.points.clear();
            let steps = 10;
            for i in 0..=steps {
                let u = i as f32 / steps as f32;
                let rr = inner + (outer - inner) * u;
                let wdt = width * (PI * u).sin();
                pen.points
                    .push(c + Vec2::angled(angle + wdt / rr.max(1.0)) * rr);
            }
            for i in (1..steps).rev() {
                let u = i as f32 / steps as f32;
                let rr = inner + (outer - inner) * u;
                let wdt = width * (PI * u).sin();
                pen.points
                    .push(c + Vec2::angled(angle - wdt / rr.max(1.0)) * rr);
            }
            let mid = c + Vec2::angled(angle) * (inner + outer) / 2.0;
            pen.fan(mid, colour, colour);
        };
    // outer petals (treble), turning one way
    for k in 0..FOLD * 2 {
        let angle = k as f32 / (FOLD * 2) as f32 * TAU + t * 0.05;
        let level = band(a, 0.6 + 0.4 * (k % FOLD) as f32 / FOLD as f32);
        petal(
            pen,
            angle,
            size * 0.62,
            size * (0.75 + 0.25 * treble.max(level)),
            size * 0.06,
            mix(s.background, s.high, 0.3 + 0.6 * level),
        );
    }
    // a ring of dots
    for k in 0..FOLD * 3 {
        let angle = k as f32 / (FOLD * 3) as f32 * TAU - t * 0.12;
        pen.circle(
            c + Vec2::angled(angle) * size * 0.58,
            size * (0.012 + 0.02 * middle),
            s.peak.gamma_multiply(0.8),
        );
    }
    // middle petals (the middle), the other way
    for k in 0..FOLD {
        let angle = k as f32 / FOLD as f32 * TAU - t * 0.08;
        let level = band(a, 0.3 + 0.3 * k as f32 / FOLD as f32);
        petal(
            pen,
            angle,
            size * 0.25,
            size * (0.45 + 0.15 * level + 0.1 * middle),
            size * 0.1,
            mix(s.background, s.mid, 0.4 + 0.6 * level),
        );
        petal(
            pen,
            angle + PI / FOLD as f32,
            size * 0.3,
            size * (0.4 + 0.12 * level),
            size * 0.05,
            mix(s.background, s.low, 0.6 + 0.4 * level),
        );
    }
    // the centre (the bass, the beat)
    for k in 0..6 {
        let angle = k as f32 / 6.0 * TAU + t * 0.2;
        petal(
            pen,
            angle,
            size * 0.02,
            size * (0.16 + 0.12 * bass + 0.06 * a.beat.pulse),
            size * 0.07,
            s.at(0.5 + 0.5 * bass),
        );
    }
    pen.circle(c, size * (0.04 + 0.03 * a.beat.pulse), s.peak);
    pen.ring(c, size * 0.6, 1.0, mix(s.background, s.peak, 0.3));
    pen.ring(c, size * 0.22, 1.0, mix(s.background, s.peak, 0.3));
}

// -- rings on the beat ---------------------------------------------------------------

fn beatrings(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, dt: f32) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let reach = r.width().max(r.height()) * 0.6;
    st.since_ring += dt;
    let beat = a.beat.count != st.last_beat;
    st.last_beat = a.beat.count;
    if beat || (st.since_ring > 1.5 && a.energy > 0.15) {
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
        for _ in 0..(16.0 * strength) as usize {
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
    // the spectrum round the middle, faint
    let n = a.bars.len().max(1);
    let base = r.width().min(r.height()) * 0.14;
    for i in 0..n {
        let angle = i as f32 / n as f32 * TAU - PI / 2.0;
        let u = Vec2::angled(angle);
        let v = a.bars[i];
        streak(
            pen,
            c + u * base,
            c + u * (base + v * base * 1.6),
            2.0,
            mix(s.background, s.low, 0.2),
            mix(s.background, s.at(v), 0.5),
        );
    }
    for ring in &st.rings {
        let [age, strength, hue, speed] = *ring;
        let life = 1.0 - age / 2.2;
        let radius = base + age * reach * speed;
        let width = (2.0 + 10.0 * strength) * life;
        let colour = mix(s.background, s.at(0.3 + 0.7 * hue), life * life);
        pen.ring(c, radius, width, colour);
        pen.ring(
            c,
            radius,
            width * 3.0,
            glow(s.at(0.3 + 0.7 * hue), 0.12 * life),
        );
    }
    let size = r.width().min(r.height()) * 0.5;
    for [x, y, _, _, age] in &st.sparks {
        let life = 1.0 - age / 1.4;
        pen.circle(
            c + vec2(*x, *y) * size,
            1.0 + 2.0 * life,
            mix(s.background, s.peak, life),
        );
    }
    let core = base * (0.55 + 0.4 * a.bass + 0.35 * a.beat.pulse);
    pen.circle(c, core * 1.6, glow(s.at(a.energy), 0.25));
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
    if (beat && st.word_age > 0.3) || st.word_age > 1.6 {
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
    let treble = third(a, 2);
    // an echo of the word behind, large and faint
    let word = &st.words[st.word];
    let galley = f
        .painter
        .layout_no_wrap(word.clone(), font.clone(), Color32::WHITE);
    let fit = (r.width() * 0.82 / galley.size().x.max(1.0))
        .min(r.height() * 0.6 / galley.size().y.max(1.0));
    put_text(
        pen,
        &galley,
        uv,
        c,
        fit * 1.9,
        t * 0.05,
        mix(s.background, s.low, 0.12),
        0.0,
        0,
    );
    if st.previous_word != usize::MAX && st.word_age < 0.5 {
        let old = f.painter.layout_no_wrap(
            st.words[st.previous_word].clone(),
            font.clone(),
            Color32::WHITE,
        );
        let k = st.word_age / 0.5;
        let fit_old = (r.width() * 0.82 / old.size().x.max(1.0))
            .min(r.height() * 0.6 / old.size().y.max(1.0));
        put_text(
            pen,
            &old,
            uv,
            c + vec2(0.0, -k * r.height() * 0.2),
            fit_old * (1.0 + 1.5 * k),
            -0.2 * k,
            mix(s.background, s.mid, (1.0 - k) * 0.7),
            0.0,
            1,
        );
    }
    // the word: it comes in large, settles, swells with the beat; its
    // letters shake with the treble
    let arrive = (st.word_age / 0.22).min(1.0);
    let scale = fit
        * (1.0 + 0.6 * (1.0 - arrive) * (1.0 - arrive))
        * (1.0 + 0.12 * a.beat.pulse + 0.06 * a.bass);
    let tilt = (t * 0.7).sin() * 0.04 + (1.0 - arrive) * 0.15;
    let colour = mix(s.mid, s.peak, a.beat.pulse * 0.8);
    put_text(
        pen,
        &galley,
        uv,
        c,
        scale,
        tilt,
        colour,
        treble * base * 0.04,
        2,
    );
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
    let centre = galley.rect.center();
    let (sin, cos) = angle.sin_cos();
    let mut glyph = 0u32;
    for row in &galley.rows {
        let mesh = &row.row.visuals.mesh;
        let base = pen.mesh.vertices.len() as u32;
        for (k, v) in mesh.vertices.iter().enumerate() {
            if k % 4 == 0 {
                glyph += 1;
            }
            let jitter = if shake > 0.0 {
                vec2(hash(glyph, salt + 17) - 0.5, hash(glyph, salt + 29) - 0.5) * shake * 2.0
            } else {
                Vec2::ZERO
            };
            let p = (v.pos + row.pos.to_vec2() - centre.to_vec2()).to_vec2() * scale + jitter;
            let mut vertex = *v;
            vertex.pos = at + vec2(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
            vertex.color = colour;
            vertex.uv = pos2(v.uv.x * uv.x, v.uv.y * uv.y);
            pen.mesh.vertices.push(vertex);
        }
        for i in &mesh.indices {
            pen.mesh.indices.push(base + i);
        }
    }
}
