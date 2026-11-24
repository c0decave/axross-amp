//! Spectra, scopes and instruments of the sound drawn anew: the waveform
//! round a circle, balls the bands throw up and that fall back, the
//! spectrum's history on a turning globe, a honeycomb equalizer, the notes
//! on a spiral of pitch, a record whose grooves are the spectrum, a radar
//! that sweeps it, a pendulum wave the bands swing and a rain of glyphs.
//!
//! What each keeps is made once and then only changed: a frame allocates
//! nothing of its own.

// (bars, peaks and their places go in step: an index reads best)
#![allow(clippy::needless_range_loop)]

use super::scenes::{band, dot, hash, shine, soft, span, streak};
use super::{Frame, Pen, VizState, faint, mix};
use crate::analyzer::Analyzer;
use crate::analyzer::guard::Guard;
use egui::{Color32, FontId, Pos2, Rect, Vec2, pos2, vec2};
use std::f32::consts::{PI, TAU};

/// Points round the circular waveform, and its echoes.
const WAVE_POINTS: usize = 240;
const ECHOES: usize = 6;
/// The bouncing balls at most.
const CAPS: usize = 48;
/// The honeycomb's levels from its middle outwards.
const COMB: usize = 24;
/// The globe: its bands round and its rows of history.
const GLOBE_LON: usize = 40;
const GLOBE_LAT: usize = 18;
/// The radar's blips.
const BLIPS: usize = 960;
/// The pendulums at most.
const PENDULUMS: usize = 24;
/// The glyphs of the rain (no space: a glyph a quad).
const GLYPHS: &str = "0123456789ABCDEFXZ$#@%&*+=<>:";

pub struct State {
    last_time: f64,
    clock: f32,
    last_beat: u64,
    since_beat: f32,
    /// The circular waveform: its left and right as shown (eased), its
    /// gain's peak, its height guarded, its echoes (a ring) and their ages.
    wave_l: [f32; WAVE_POINTS],
    wave_r: [f32; WAVE_POINTS],
    wave_peak: f32,
    wave_guard: Guard,
    echoes: [[f32; WAVE_POINTS]; ECHOES],
    echo_age: [f32; ECHOES],
    echo_next: usize,
    since_echo: f32,
    /// The bouncing balls: each band's level as drawn, its ball's height
    /// and speed (heights, heights a second), how much it is squashed.
    cap_level: [f32; CAPS],
    caps: [[f32; 2]; CAPS],
    squash: [f32; CAPS],
    /// The honeycomb's levels from its middle outwards, eased.
    comb: [f32; COMB],
    /// The globe's turn, the record's, the radar's sweep.
    globe_turn: f32,
    vinyl_turn: f32,
    sweep: f32,
    /// The radar's pings: the beat last answered, each ping's age (s).
    ping_beat: u64,
    pings: [f32; 3],
    /// angle, radius (0..1), level, age (s).
    blips: Vec<[f32; 4]>,
    /// The honeycomb's ripple: its age (s), how strong.
    ripple: [f32; 2],
    /// The pitch spiral's keys as shown (eased).
    keys: [f32; 88],
    /// The pendulums' swing, eased.
    swing: [f32; PENDULUMS],
    /// The rain's columns: head (rows), speed (rows a second), length.
    drops: Vec<[f32; 3]>,
    seed: u32,
}

impl Default for State {
    fn default() -> Self {
        State {
            last_time: 0.0,
            clock: 0.0,
            last_beat: 0,
            since_beat: 10.0,
            wave_l: [0.0; WAVE_POINTS],
            wave_r: [0.0; WAVE_POINTS],
            wave_peak: 0.1,
            wave_guard: Guard::default(),
            echoes: [[0.0; WAVE_POINTS]; ECHOES],
            echo_age: [9.0; ECHOES],
            echo_next: 0,
            since_echo: 0.0,
            cap_level: [0.0; CAPS],
            caps: [[0.0; 2]; CAPS],
            squash: [0.0; CAPS],
            comb: [0.0; COMB],
            globe_turn: 0.0,
            vinyl_turn: 0.0,
            sweep: 0.0,
            ping_beat: 0,
            pings: [9.0; 3],
            blips: Vec::with_capacity(BLIPS),
            ripple: [9.0, 0.0],
            keys: [0.0; 88],
            swing: [0.0; PENDULUMS],
            drops: Vec::new(),
            seed: 5,
        }
    }
}

impl State {
    fn random(&mut self) -> f32 {
        super::random(&mut self.seed)
    }
}

/// A frame's two steps of time (s): the music's own - what follows the
/// sound (a level's rise and fall) keeps its pace in every mode - and the
/// pictures', slower in Calm and slower still where the desktop asks for
/// reduced motion: what moves by itself (a pendulum's swing, the rain's
/// fall, a sweep, a throw, a turn) moves at that one. (All of it went by
/// the music's: these nine faces moved as fast in Reduced as in Intense -
/// review 18, M2.)
#[derive(Clone, Copy)]
struct Step {
    music: f32,
    motion: f32,
    /// How much longer a shape takes to follow the sound than in Intense
    /// (1 there; a line redrawn as fast as the sound changes is motion too).
    rest: f32,
}

pub fn draw(f: &Frame<'_>, key: &str, state: &mut VizState) {
    // seconds of sound (they stand while nothing plays)
    let dt = {
        let st = &mut state.spectral;
        let pace = f.a.motion.pace();
        let motion = (f.a.time - st.last_time).clamp(0.0, 0.25) as f32;
        st.last_time = f.a.time;
        st.clock += motion;
        let music = motion / pace;
        st.since_beat += music;
        Step {
            music,
            motion,
            rest: match f.a.motion {
                crate::analyzer::guard::Motion::Intense => 1.0,
                crate::analyzer::guard::Motion::Calm => 1.0 / pace,
                crate::analyzer::guard::Motion::Reduced => 1.0 / (pace * pace),
            },
        }
    };
    let cv = &mut state.canvas;
    let st = &mut state.spectral;
    let p = f.painter;
    match key {
        "circlewave" => cv.draw(p, |pen| circlewave(f, pen, st, dt)),
        "bouncing" => cv.draw(p, |pen| bouncing(f, pen, st, dt)),
        "globe" => cv.draw(p, |pen| globe(f, pen, st, dt)),
        "honeycomb" => cv.draw(p, |pen| honeycomb(f, pen, st, dt)),
        "spiral" => cv.draw(p, |pen| {
            spiral(f, pen, st, dt);
            spiral_labels(f, pen);
        }),
        "vinyl" => cv.draw(p, |pen| vinyl(f, pen, st, dt)),
        "radar" => cv.draw(p, |pen| radar(f, pen, st, dt)),
        "pendulum" => cv.draw(p, |pen| pendulum(f, pen, st, dt)),
        _ => cv.draw(p, |pen| rain(f, pen, st, dt)),
    }
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

/// How much smaller the marks are in a small picture (1 at 300 high).
fn small(r: Rect) -> f32 {
    (r.height() / 300.0).clamp(0.5, 2.5)
}

// -- the circular waveform -----------------------------------------------------------

fn circlewave(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let dt = step.music;
    let unred = f.s.unred();
    let (a, s, r) = (f.a, &unred, f.rect);
    let calm = super::scheme_calm(s);
    let c = r.center();
    let side = r.width().min(r.height());
    // (on a light ground as it was - smaller, its gain by the samples, its
    // line of one weight: livelier there, in black on white at 240 x 135
    // it read six transitions a second and was veiled a fifth of a song)
    let lively = super::scenes::dark(s);
    let base = side * if lively { 0.3 } else { 0.29 } * (1.0 + 0.06 * a.bass / calm);
    let reach = side * if lively { 0.16 } else { 0.15 };
    // the latest 1024 frames from a rise through zero (a steady tone stands
    // still), the seam where they close blended
    let n = a.stereo.len();
    let window = n.min(1024);
    let latest = n - window;
    let from = latest.saturating_sub(600);
    let start = (from.max(1)..latest.max(1))
        .find(|&i| {
            let (p, q) = (a.stereo[i - 1], a.stereo[i]);
            p[0] + p[1] <= 0.0 && q[0] + q[1] > 0.0
        })
        .unwrap_or(latest);
    // (eased over a tenth of a second: its every wiggle anew each frame
    // swept a field again and again by the strict reading)
    // (longer in Calm, half a second in Reduced: the line's own motion)
    let ease = 1.0 - (-dt / (0.1 * step.rest)).exp();
    let blend = WAVE_POINTS / 12;
    // (each point the mean of the samples round it, then smoothed along
    // the circle: a jagged line, hundreds of zigzags, was a grating)
    let at = |k: usize| -> [f32; 2] {
        if window == 0 {
            return [0.0, 0.0];
        }
        let span = (window / WAVE_POINTS).max(1);
        let from = start + k * window / WAVE_POINTS;
        let (mut l, mut r) = (0.0, 0.0);
        for i in from.saturating_sub(span / 2)..(from + span + span / 2).min(n) {
            l += a.stereo[i][0];
            r += a.stereo[i][1];
        }
        let count = (2 * span).max(1) as f32;
        [l / count, r / count]
    };
    let mut target = [[0.0f32; 2]; WAVE_POINTS];
    let first = at(0);
    for k in 0..WAVE_POINTS {
        let mut v = at(k);
        if k + blend >= WAVE_POINTS {
            let w = (k + blend - WAVE_POINTS) as f32 / blend as f32;
            v = [v[0] + (first[0] - v[0]) * w, v[1] + (first[1] - v[1]) * w];
        }
        target[k] = v;
    }
    for _ in 0..3 {
        let mut before = target[WAVE_POINTS - 1];
        let first = target[0];
        for k in 0..WAVE_POINTS {
            let next = if k + 1 < WAVE_POINTS {
                target[k + 1]
            } else {
                first
            };
            let here = target[k];
            target[k] = [
                (before[0] + 2.0 * here[0] + next[0]) * 0.25,
                (before[1] + 2.0 * here[1] + next[1]) * 0.25,
            ];
            before = here;
        }
    }
    // its gain by the line as it is drawn - the samples' means, smoothed -
    // not by the samples: in a loud, dense passage the samples' peak is
    // several times the line's, and the ring was nearly a plain circle
    // where the music was loudest (a quiet arpeggio moved it more)
    let loudest = if lively {
        target
            .iter()
            .fold(0.0f32, |m, v| m.max(v[0].abs()).max(v[1].abs()))
            * 1.4
    } else {
        a.stereo[start..(start + window).min(n)]
            .iter()
            .fold(0.0f32, |m, v| m.max(v[0].abs()).max(v[1].abs()))
    };
    st.wave_peak = loudest.max(st.wave_peak * (-dt / 3.0).exp()).max(1e-4);
    let gain = (0.9 / st.wave_peak).clamp(1.0, if lively { 16.0 } else { 8.0 });
    // (its height no faster to and fro than the guard allows)
    let height = (loudest * gain).min(1.0);
    let shown = st.wave_guard.step(height, dt / calm, a.motion);
    let scale = if height > 1e-4 {
        gain * (shown / height).min(1.0)
    } else {
        gain
    };
    for k in 0..WAVE_POINTS {
        let v = target[k];
        st.wave_l[k] += ((v[0] * scale * 1.4).clamp(-1.0, 1.0) - st.wave_l[k]) * ease;
        st.wave_r[k] += ((v[1] * scale * 1.4).clamp(-1.0, 1.0) - st.wave_r[k]) * ease;
    }
    // echoes of the left's line going out, faint, a few a second
    st.since_echo += step.motion;
    if st.since_echo > 0.16 && dt > 0.0 {
        st.since_echo = 0.0;
        st.echoes[st.echo_next] = st.wave_l;
        st.echo_age[st.echo_next] = 0.0;
        st.echo_next = (st.echo_next + 1) % ECHOES;
    }
    for age in st.echo_age.iter_mut() {
        *age += step.motion;
    }
    let point = |k: usize, radius: f32| {
        let angle = k as f32 / WAVE_POINTS as f32 * TAU - PI / 2.0;
        c + Vec2::angled(angle) * radius
    };
    soft(
        pen,
        c,
        base * 1.9,
        shine(s, s.low, 0.1 + 0.06 * a.energy.min(1.0)),
    );
    // (no echoes on a light ground or in a small picture: rings close
    // together there read as stripes)
    let echoes = super::scenes::dark(s) && r.height() >= 200.0;
    for e in 0..ECHOES {
        let age = st.echo_age[e];
        if age > 0.9 || !echoes {
            continue;
        }
        let life = 1.0 - age / 0.9;
        let grow = 1.0 + age * 0.55;
        pen.points.clear();
        for k in 0..WAVE_POINTS {
            pen.points
                .push(point(k, (base + st.echoes[e][k] * reach) * grow));
        }
        pen.outline(
            1.0,
            faint(mix(s.background, s.mid, life * 0.8), s.background),
        );
    }
    // the band between the two channels: its colour how far apart they are
    let i0 = pen.mesh.vertices.len() as u32;
    for k in 0..WAVE_POINTS {
        let (l, rr) = (st.wave_l[k], st.wave_r[k]);
        let apart = ((l - rr).abs() * 1.5).min(1.0);
        let outer = point(k, base + l * reach);
        let inner = point(k, base * 0.78 + rr * reach * 0.8);
        let colour = shine(s, s.at(0.3 + 0.7 * apart), 0.24 + 0.12 * apart);
        pen.mesh.colored_vertex(outer, colour);
        pen.mesh.colored_vertex(inner, colour);
    }
    for k in 0..WAVE_POINTS as u32 {
        let j = (k + 1) % WAVE_POINTS as u32;
        pen.mesh
            .add_triangle(i0 + 2 * k, i0 + 2 * k + 1, i0 + 2 * j + 1);
        pen.mesh
            .add_triangle(i0 + 2 * k, i0 + 2 * j + 1, i0 + 2 * j);
    }
    // left outside, right inside
    let small = small(r);
    pen.points.clear();
    for k in 0..WAVE_POINTS {
        pen.points.push(point(k, base + st.wave_l[k] * reach));
    }
    // (the louder, the heavier the line)
    let weight = if lively {
        1.0 + 0.7 * a.energy.min(1.0)
    } else {
        1.0
    };
    pen.outline(5.0 * small * weight, shine(s, s.high, 0.15));
    pen.outline(1.6 * small * weight, mix(s.high, s.peak, 0.3));
    pen.points.clear();
    for k in 0..WAVE_POINTS {
        pen.points
            .push(point(k, base * 0.78 + st.wave_r[k] * reach * 0.8));
    }
    pen.outline(4.0 * small * weight, shine(s, s.mid, 0.12));
    pen.outline(1.3 * small * weight, s.mid);
    // the heart: the loudness
    let heart = base * (0.42 + 0.08 * a.energy.min(1.0) / calm);
    soft(pen, c, heart * 1.4, shine(s, s.at(a.energy), 0.3));
    dot(
        pen,
        c,
        heart * 0.55,
        mix(s.background, s.at(0.3 + 0.7 * a.energy), 0.8),
    );
}

// -- the bouncing balls --------------------------------------------------------------

/// An oval (a fan of fourteen sides).
fn oval(pen: &mut Pen<'_>, centre: Pos2, rx: f32, ry: f32, colour: Color32) {
    let i0 = pen.mesh.vertices.len() as u32;
    pen.mesh.colored_vertex(centre, colour);
    for k in 0..14 {
        let a = k as f32 / 14.0 * TAU;
        pen.mesh
            .colored_vertex(centre + vec2(a.cos() * rx, a.sin() * ry), colour);
    }
    for k in 0..14u32 {
        pen.mesh.add_triangle(i0, i0 + 1 + k, i0 + 1 + (k + 1) % 14);
    }
}

/// A row of balls, one a band, on a floor of pads: a pad kicks its ball as
/// high as its band is loud, the ball falls back, lands - squashed a
/// moment - and is kicked again or bounces out. No bars: a fountain of
/// light over each pad, faint, says how loud the band is now; the balls,
/// their trails and their reflections in the floor are the picture. (It was
/// the Spectrum's bars with caps thrown from them - and thrown to the
/// picture's top whatever the bar's height, where they stood in a row:
/// review 18, L2.)
fn bouncing(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let small = small(r);
    let n = (r.width() / (26.0 * small.max(0.6))).clamp(8.0, a.bars.len().clamp(8, CAPS) as f32)
        as usize;
    let floor = r.top() + r.height() * 0.8;
    let tall = r.height() * 0.7;
    let width = r.width() / n as f32;
    let radius = (width * 0.3).clamp(2.0, 12.0 * small);
    let calm = super::scheme_calm(s);
    // (heights a second, a second: a ball thrown the whole height is back
    // in a second and a quarter)
    const G: f32 = 5.0;
    let dark = super::scenes::dark(s);
    // the floor: a sheen under it, its line
    pen.vertical(
        Rect::from_min_max(pos2(r.left(), floor), r.right_bottom()),
        mix(s.background, s.low, if dark { 0.12 } else { 0.06 }),
        s.background,
    );
    let fall = 1.2 * step.music;
    for i in 0..n {
        let v = span(a, i as f32 / n as f32, (i + 1) as f32 / n as f32);
        // (the band as drawn: up at once, down no faster than the whole
        // height in a second)
        let level = v.max(st.cap_level[i] - fall);
        st.cap_level[i] = level;
        // the ball: as high as its band is loud when it is kicked - by
        // the speed that takes it there, never past the picture's top
        let [mut h, mut speed] = st.caps[i];
        let dt = step.motion;
        speed -= G * dt;
        h += speed * dt;
        st.squash[i] *= (-dt / 0.09).exp();
        if h <= 0.0 {
            let landed = -speed;
            h = 0.0;
            let reach = (level * 0.95).min(0.95);
            if reach > 0.05 {
                // (kicked again)
                speed = (2.0 * G * reach).sqrt();
                st.squash[i] = st.squash[i].max((landed / 3.0).min(1.0));
            } else if landed > 0.5 {
                // (no sound in its band: it bounces out)
                speed = landed * 0.45;
                st.squash[i] = (landed / 3.0).min(1.0);
            } else {
                speed = 0.0;
            }
        }
        st.caps[i] = [h, speed];
    }
    for i in 0..n {
        let level = st.cap_level[i];
        let [h, speed] = st.caps[i];
        let x = r.left() + (i as f32 + 0.5) * width;
        let colour = s.at(0.25 + 0.75 * level);
        // the pad, lit as its band is loud, and the light over it
        let pad = Rect::from_min_max(
            pos2(x - width * 0.36, floor - 2.5 * small),
            pos2(x + width * 0.36, floor),
        );
        let up = level * tall * 0.95;
        if up > 2.0 {
            pen.vertical(
                Rect::from_min_max(
                    pos2(x - width * 0.3, floor - up),
                    pos2(x + width * 0.3, floor),
                ),
                shine(s, colour, 0.0),
                shine(s, colour, (0.1 + 0.16 * level) / calm),
            );
        }
        pen.rect(pad, mix(s.background, colour, 0.35 + 0.55 * level));
        // the ball: stretched as it flies, squashed as it lands
        let fast = (speed.abs() / 3.0).min(1.0);
        let squash = st.squash[i];
        let (rx, ry) = (
            radius * (1.0 + 0.4 * squash - 0.12 * fast),
            radius * (1.0 - 0.4 * squash + 0.18 * fast),
        );
        let y = floor - 2.5 * small - ry - h * tall;
        // its reflection in the floor, fainter the higher it is
        let deep = r.bottom() - floor;
        let mirror = floor + (ry + h * tall) * 0.3 + 2.0;
        if mirror + ry * 0.5 < r.bottom() && deep > 8.0 {
            oval(
                pen,
                pos2(x, mirror),
                rx,
                ry * 0.5,
                mix(s.background, colour, 0.3 * (1.0 - 0.7 * h.min(1.0))),
            );
        }
        // its trail: where it was, by the way it flies
        for j in (1..=4).rev() {
            let back = j as f32 * 0.028;
            let was = h - speed * back - 0.5 * G * back * back;
            if was <= 0.0 || (was - h).abs() * tall < 1.0 {
                continue;
            }
            let k = 1.0 - j as f32 / 5.0;
            oval(
                pen,
                pos2(x, floor - 2.5 * small - ry - was * tall),
                rx * (0.55 + 0.4 * k),
                ry * (0.55 + 0.4 * k),
                shine(s, colour, 0.22 * k),
            );
        }
        soft(
            pen,
            pos2(x, y),
            radius * 2.6,
            shine(s, colour, (0.1 + 0.22 * level) / calm),
        );
        oval(pen, pos2(x, y), rx, ry, mix(s.background, colour, 0.92));
        oval(
            pen,
            pos2(x - rx * 0.3, y - ry * 0.3),
            rx * 0.38,
            ry * 0.38,
            mix(colour, s.peak, 0.55),
        );
    }
    pen.rect(
        Rect::from_x_y_ranges(r.x_range(), floor..=floor + 1.5),
        s.grid(),
    );
}

// -- the spectrum globe --------------------------------------------------------------

fn globe(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let dt = step.motion;
    let (a, s, r) = (f.a, f.s, f.rect);
    let c = r.center();
    let radius = r.width().min(r.height()) * 0.3;
    let calm = super::scheme_calm(s);
    st.globe_turn = (st.globe_turn + dt * (0.22 + 0.2 * a.energy.min(1.0))) % (TAU * 100.0);
    let tilt: f32 = 0.38;
    let (st_, ct) = tilt.sin_cos();
    soft(pen, c, radius * 1.7, shine(s, s.low, 0.14));
    // (on a light ground the sphere has a body, shaded, and its marks are
    // inked: pale dots on a pale ground were nearly gone in a quiet
    // passage - review 18, L6)
    let light = !super::scenes::dark(s);
    if light {
        dot(pen, c, radius * 1.02, mix(s.background, s.low, 0.1));
        soft(
            pen,
            c + vec2(radius, radius) * 0.25,
            radius * 0.95,
            mix(s.background, s.mid, 0.12),
        );
    }
    let ink = |c: Color32| {
        if light { mix(c, s.peak, 0.4) } else { c }
    };
    pen.ring(
        c,
        radius * 1.02,
        1.0,
        faint(mix(s.background, s.mid, 0.5), s.background),
    );
    let small = small(r);
    // (latitude rows: the newest at the top, two seconds down to the
    // bottom; bands round, the bass and the treble meeting at the back)
    let project = |lat: usize, lon: usize, level: f32| -> (Pos2, f32) {
        let phi = (0.82 - 1.64 * lat as f32 / (GLOBE_LAT - 1) as f32) * PI / 2.0;
        let lambda = lon as f32 / GLOBE_LON as f32 * TAU + st.globe_turn;
        let rho = radius * (1.0 + 0.42 * level / calm);
        let (x, y, z) = (
            rho * phi.cos() * lambda.sin(),
            rho * phi.sin(),
            rho * phi.cos() * lambda.cos(),
        );
        // tipped towards the viewer
        let (y, z) = (y * ct - z * st_, y * st_ + z * ct);
        let near = 1.0 / (1.0 - z / (radius * 5.0));
        (
            pos2(c.x + x * near, c.y - y * near),
            (z / rho.max(1e-3) + 1.0) * 0.5,
        )
    };
    let level = |lat: usize, lon: usize| {
        let row = a.history_row(lat * 3);
        let n = row.len();
        if n == 0 {
            return 0.0;
        }
        let x = 1.0 - (2.0 * lon as f32 / GLOBE_LON as f32 - 1.0).abs();
        let at = x * (n - 1) as f32;
        let i = at as usize;
        let j = (i + 1).min(n - 1);
        row[i] + (row[j] - row[i]) * (at - i as f32)
    };
    for front in [false, true] {
        for lat in 0..GLOBE_LAT {
            let mut last = None;
            // (round and once more to the first: the row's line closes; the
            // first dot and spike drawn once)
            for step in 0..=GLOBE_LON {
                let lon = step % GLOBE_LON;
                let v = level(lat, lon);
                let (p, depth) = project(lat, lon, v);
                if (depth >= 0.5) == front {
                    if let Some((q, d)) = last
                        && front
                    {
                        let k: f32 = (depth + d) * 0.5;
                        streak(
                            pen,
                            q,
                            p,
                            0.9 * small,
                            faint(mix(s.background, s.mid, k), s.background),
                            faint(mix(s.background, s.mid, k), s.background),
                        );
                    }
                    if step < GLOBE_LON {
                        let colour = ink(s.at(0.2 + 0.8 * v));
                        if front && v > 0.04 {
                            // (a spike from the sphere out to it)
                            let (root, _) = project(lat, lon, 0.0);
                            streak(
                                pen,
                                root,
                                p,
                                (0.8 + 1.4 * v) * small,
                                mix(s.background, colour, 0.15 * depth),
                                mix(s.background, colour, 0.7 * depth),
                            );
                        }
                        let size =
                            (if light { 1.1 } else { 0.8 } + 2.6 * v) * (0.5 + 0.6 * depth) * small;
                        let seen = if light { 0.45 } else { 0.25 };
                        dot(
                            pen,
                            p,
                            size,
                            mix(s.background, colour, seen + (1.0 - seen) * depth),
                        );
                    }
                    last = Some((p, depth));
                } else {
                    last = None;
                }
            }
        }
    }
}

// -- the honeycomb equalizer ---------------------------------------------------------

fn honeycomb(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let calm = super::scheme_calm(s);
    // (fewer, larger cells in a small picture, and every cell's level eased
    // over a seventh of a second: each cell following its bars frame by
    // frame, the face read 4 - 5 transitions a second on ordinary music
    // at 240 x 135 and leant on the guard's veil in drum and bass - review
    // 18, L5)
    let least = r.width().min(r.height());
    let size = (least / if least < 200.0 { 6.5 } else { 9.0 }).clamp(9.0, 42.0);
    let ease = 1.0 - (-step.music / (0.15 * step.rest)).exp();
    for (k, shown) in st.comb.iter_mut().enumerate() {
        let x = k as f32 / (COMB - 1) as f32;
        *shown += (span(a, x - 0.05, x + 0.05) - *shown) * ease;
    }
    let comb = st.comb;
    let level_at = |x: f32| {
        let at = x.clamp(0.0, 1.0) * (COMB - 1) as f32;
        let i = (at as usize).min(COMB - 2);
        comb[i] + (comb[i + 1] - comb[i]) * (at - i as f32)
    };
    let (dx, dy) = (size * 1.5, size * 3f32.sqrt());
    let c = r.center();
    let reach = (r.width().powi(2) + r.height().powi(2)).sqrt() * 0.5;
    if beat(a, st, 0.5) {
        st.ripple = [0.0, 0.5 + 0.5 * a.beat.strength.min(1.0)];
    }
    st.ripple[0] += step.motion;
    let front = st.ripple[0] * reach * 1.3;
    let fade = (1.0 - st.ripple[0] / 0.9).max(0.0) * st.ripple[1];
    let cols = (r.width() / dx / 2.0).ceil() as i32 + 1;
    let rows = (r.height() / dy / 2.0).ceil() as i32 + 1;
    let hex = |pen: &mut Pen<'_>, at: Pos2, radius: f32, colour: Color32| {
        let i0 = pen.mesh.vertices.len() as u32;
        pen.mesh.colored_vertex(at, colour);
        for k in 0..6 {
            pen.mesh
                .colored_vertex(at + Vec2::angled(k as f32 * PI / 3.0) * radius, colour);
        }
        for k in 0..6u32 {
            pen.mesh.add_triangle(i0, i0 + 1 + k, i0 + 1 + (k + 1) % 6);
        }
    };
    let light = !super::scenes::dark(s);
    for col in -cols..=cols {
        for row in -rows..=rows {
            let at = c + vec2(
                col as f32 * dx,
                (row as f32 + if col % 2 != 0 { 0.5 } else { 0.0 }) * dy,
            );
            if !r.expand(size).contains(at) {
                continue;
            }
            // the bass in the middle, the treble outwards; neighbours a
            // little apart
            let d = (at - c).length() / reach;
            // (the mean of the bars round it: neighbours alike, light
            // and dark never alternating cell by cell - a grating)
            let x = (d * 1.05).clamp(0.0, 1.0);
            let level = level_at(x);
            let near = (-(((at - c).length() - front) / (size * 1.6)).powi(2)).exp() * fade;
            let fill = (0.3 + 0.62 * level / calm.sqrt() + 0.1 * near).min(0.9);
            let colour = s.at(level * 0.9 + 0.1 * near);
            // (each cell one colour, its heart - as large as it is loud -
            // lighter by less than a tenth of the luminance, its walls
            // faint: light hearts on dark cells, cell after cell, were a
            // grating)
            let k = (0.3 + 0.6 * level / calm) * if light { 0.6 } else { 1.0 };
            let ground = mix(s.background, colour, k);
            hex(pen, at, size * 1.01, ground);
            let heart = faint(mix(ground, mix(colour, s.peak, 0.3), 0.8), ground);
            hex(pen, at, size * fill, heart);
            // (the walls the hearts' way from the ground, never the other:
            // light hearts and dark walls in turn were a grating)
            let wall = mix(ground, heart, 0.6);
            for k in 0..3 {
                let p = at + Vec2::angled(k as f32 * PI / 3.0) * size;
                let q = at + Vec2::angled((k + 1) as f32 * PI / 3.0) * size;
                streak(pen, p, q, 1.0, wall, wall);
            }
        }
    }
}

// -- the pitch spiral ----------------------------------------------------------------

/// Where key `k` (0 is A0) lies on the spiral of pitch: its pitch class's
/// ray, an octave a turn outwards.
fn spiral_at(r: Rect, k: f32) -> Pos2 {
    let (c, inner, outer) = spiral_frame(r);
    let class = (k + 9.0) / 12.0; // (A0 is the 9th class from C)
    let angle = class * TAU - PI / 2.0;
    let radius = inner + (outer - inner) * k / 87.0;
    c + Vec2::angled(angle) * radius
}

fn spiral_frame(r: Rect) -> (Pos2, f32, f32) {
    let c = r.center();
    // (the names stand 19 points outside it, 11 points tall: 30 points of
    // room, or the lowest one is cut by the picture's edge - review 18, L4)
    let outer = (r.width().min(r.height()) * 0.5 - 30.0).max(10.0);
    (c, outer * 0.14, outer)
}

fn spiral(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let calm = super::scheme_calm(s);
    let (c, _, outer) = spiral_frame(r);
    let ease = 1.0 - (-step.music / (0.12 * step.rest)).exp();
    for (shown, v) in st.keys.iter_mut().zip(a.fine.keys) {
        *shown += (v - *shown) * ease;
    }
    soft(pen, c, outer * 1.2, shine(s, s.low, 0.1));
    // the twelve rays, faint, and the pitch classes' ring
    for k in 0..12 {
        let angle = k as f32 / 12.0 * TAU - PI / 2.0;
        let u = Vec2::angled(angle);
        pen.segment(c + u * outer * 0.1, c + u * outer * 1.02, 1.0, s.track());
        // (the ring: each class's arc as loud as it is)
        let level = a.fine.chroma[k];
        pen.points.clear();
        for j in 0..=8 {
            let t = angle - PI / 12.0 * 0.85 + j as f32 / 8.0 * PI / 6.0 * 0.85;
            pen.points.push(c + Vec2::angled(t) * (outer + 8.0));
        }
        pen.line(
            3.0 + 3.0 * level,
            mix(s.background, s.at(level), 0.25 + 0.6 * level),
        );
    }
    // the spiral itself
    pen.points.clear();
    for j in 0..=88 * 4 {
        pen.points.push(spiral_at(r, j as f32 / 4.0));
    }
    // (faint: its seven turns side by side are rings close together)
    pen.line(1.0, mix(s.track(), s.grid(), 0.5));
    let small = small(r);
    // the keys: dots on it, as loud as they are; the loud ones glowing
    for k in 0..88 {
        let level = st.keys[k];
        let p = spiral_at(r, k as f32);
        let colour = s.at(0.2 + 0.8 * level);
        if level > 0.35 {
            soft(
                pen,
                p,
                // (breathing with the beat: a held chord is no still picture)
                (6.0 + 14.0 * level) * small * (1.0 + 0.3 * a.beat.pulse),
                shine(s, colour, 0.35 * level / calm),
            );
        }
        dot(
            pen,
            p,
            (1.0 + 4.5 * level / calm.sqrt()) * small * (1.0 + 0.3 * level * a.beat.pulse),
            mix(s.background, colour, 0.35 + 0.65 * level),
        );
    }
}

/// The letters the pitch classes' names are written with.
const LETTERS: &str = "ABCDEFG#m";

/// The pitch classes' names round the spiral, the key in the middle:
/// written into the face's own mesh from one line of letters laid out a
/// size (egui keeps it) - twelve texts and a `format!` laid out each frame
/// were 40 allocations a frame, three times the other faces' (review 18,
/// L4).
fn spiral_labels(f: &Frame<'_>, pen: &mut Pen<'_>) {
    let (a, s, r) = (f.a, f.s, f.rect);
    let (c, _, outer) = spiral_frame(r);
    if outer < 30.0 {
        return;
    }
    let atlas = f.painter.ctx().fonts(|fonts| fonts.font_image_size());
    let uv = vec2(1.0 / atlas[0].max(1) as f32, 1.0 / atlas[1].max(1) as f32);
    // (the letters laid out once a size, not once a name)
    let letters = |size: f32| {
        f.painter.layout_no_wrap(
            LETTERS.to_owned(),
            FontId::proportional(size),
            Color32::WHITE,
        )
    };
    let small = letters(11.0);
    for (k, name) in crate::analyzer::fine::NOTE_NAMES.iter().enumerate() {
        let angle = k as f32 / 12.0 * TAU - PI / 2.0;
        let at = c + Vec2::angled(angle) * (outer + 19.0);
        let colour = if a.fine.chroma[k] > 0.7 {
            s.peak
        } else {
            s.text()
        };
        word(pen, &small, uv, at, colour, name, false);
    }
    if let Some((class, minor, _)) = a.key() {
        let name = crate::analyzer::fine::NOTE_NAMES[class % 12];
        word(pen, &letters(13.0), uv, c, s.text(), name, minor);
    }
}

/// `name` (of [`LETTERS`], as laid out in `galley`; an "m" after it when
/// `minor`), centred on `at`, into the pen's mesh.
fn word(
    pen: &mut Pen<'_>,
    galley: &egui::Galley,
    uv: Vec2,
    at: Pos2,
    colour: Color32,
    name: &str,
    minor: bool,
) {
    let Some(row) = galley.rows.first() else {
        return;
    };
    let mesh = &row.row.visuals.mesh;
    let glyphs = &row.row.glyphs;
    // (a glyph a quad, in the letters' order)
    if mesh.vertices.len() != glyphs.len() * 4 || mesh.indices.len() != glyphs.len() * 6 {
        return;
    }
    let letters = || name.chars().chain(minor.then_some('m'));
    let place = |ch: char| LETTERS.chars().position(|l| l == ch);
    let wide: f32 = letters()
        .filter_map(place)
        .map(|g| glyphs[g].advance_width)
        .sum();
    let tall = row.row.size.y;
    let mut x = at.x - wide / 2.0;
    for g in letters().filter_map(place) {
        let glyph = &glyphs[g];
        // (the glyph's quad moved from its place in the line to this one)
        let shift = vec2(x - glyph.pos.x, at.y - tall / 2.0);
        let base = pen.mesh.vertices.len() as u32;
        for v in &mesh.vertices[g * 4..g * 4 + 4] {
            let mut vertex = *v;
            vertex.pos = (v.pos + shift).round();
            vertex.color = colour;
            vertex.uv = pos2(v.uv.x * uv.x, v.uv.y * uv.y);
            pen.mesh.vertices.push(vertex);
        }
        for i in &mesh.indices[g * 6..g * 6 + 6] {
            pen.mesh.indices.push(base + i - (g * 4) as u32);
        }
        x += glyph.advance_width;
    }
}

// -- the spectral vinyl --------------------------------------------------------------

fn vinyl(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let dt = step.motion;
    let (a, s, r) = (f.a, f.s, f.rect);
    let radius = (r.height() * 0.46).min(r.width() * 0.36);
    let c = pos2(r.center().x - radius * 0.14, r.center().y);
    st.vinyl_turn = (st.vinyl_turn + dt * 0.35 * TAU) % (TAU * 100.0);
    let turn = st.vinyl_turn;
    // the record: near black whatever the scheme, its grooves the spectrum's
    // last two and a half seconds (the newest outside), round it the
    // bass to the treble and back
    let black = mix(s.background, Color32::from_rgb(8, 8, 10), 0.75);
    soft(
        pen,
        c + vec2(3.0, 5.0),
        radius * 1.08,
        Color32::from_black_alpha(90),
    );
    dot(pen, c, radius, black);
    const GROOVES: usize = 34;
    const SEGMENTS: usize = 72;
    let (outer, inner) = (radius * 0.96, radius * 0.38);
    let pitch = (outer - inner) / GROOVES as f32;
    for g in 0..GROOVES {
        let row = a.history_row(g * 2);
        let n = row.len().max(1);
        let r0 = outer - g as f32 * pitch;
        let r1 = r0 - pitch;
        let i0 = pen.mesh.vertices.len() as u32;
        for k in 0..=SEGMENTS {
            let x = 1.0 - (2.0 * k as f32 / SEGMENTS as f32 - 1.0).abs();
            let at = x * (n - 1) as f32;
            let (i, t) = (at as usize, at.fract());
            let v = if row.is_empty() {
                0.0
            } else {
                row[i.min(n - 1)] * (1.0 - t) + row[(i + 1).min(n - 1)] * t
            };
            let colour = mix(black, s.at(v), 0.1 + 0.5 * v);
            let u = Vec2::angled(k as f32 / SEGMENTS as f32 * TAU + turn);
            pen.mesh.colored_vertex(c + u * r0, colour);
            pen.mesh.colored_vertex(c + u * r1, colour);
        }
        for k in 0..SEGMENTS as u32 {
            let (p, q) = (i0 + 2 * k, i0 + 2 * k + 2);
            pen.mesh.add_triangle(p, p + 1, q + 1);
            pen.mesh.add_triangle(p, q + 1, q);
        }
    }
    // the tracks' gaps: a faint ring every seventh groove (fine grooves
    // side by side, light and dark, were a grating)
    for g in (7..GROOVES).step_by(7) {
        let radius = outer - g as f32 * pitch;
        pen.ring(c, radius, 1.0, mix(black, s.background, 0.3));
    }
    // the light on it: two still wedges of sheen (the lamp does not turn)
    for side in [0.0, PI] {
        let i0 = pen.mesh.vertices.len() as u32;
        pen.mesh.colored_vertex(c, Color32::TRANSPARENT);
        for k in 0..=8 {
            let angle = -PI / 4.0 + side - 0.28 + k as f32 / 8.0 * 0.56;
            let edge = 1.0 - (k as f32 / 8.0 * 2.0 - 1.0).abs();
            pen.mesh.colored_vertex(
                c + Vec2::angled(angle) * radius * 0.97,
                glow(s.peak, 0.07 * edge),
            );
        }
        for k in 0..8u32 {
            pen.mesh.add_triangle(i0, i0 + 1 + k, i0 + 2 + k);
        }
    }
    // the label: the middle colour, a mark on it turning, the hole
    let label = inner * 0.86;
    let calm = super::scheme_calm(s);
    dot(pen, c, label, mix(s.mid, s.background, 0.25));
    dot(
        pen,
        c,
        label * (0.62 + 0.05 * a.bass / calm),
        mix(s.high, s.mid, 0.5),
    );
    let mark = Vec2::angled(turn);
    pen.segment(
        c + mark * label * 0.25,
        c + mark * label * 0.9,
        2.0,
        mix(s.mid, s.peak, 0.5),
    );
    dot(pen, c, label * 0.09, s.background);
    // the tonearm, at rest on the outer groove, nodding with the bass
    let pivot = pos2(c.x + radius * 1.18, c.y - radius * 0.72);
    let stylus = c + Vec2::angled(-0.18) * (outer - pitch * 1.5) + vec2(0.0, a.bass * 1.2);
    let elbow = pivot + (stylus - pivot) * 0.7 + vec2(radius * 0.08, 0.0);
    let arm = mix(s.background, s.text(), 0.85);
    dot(pen, pivot, radius * 0.1, mix(s.background, s.text(), 0.45));
    dot(pen, pivot, radius * 0.05, arm);
    pen.segment(pivot, elbow, 3.0, arm);
    pen.segment(elbow, stylus, 3.0, arm);
    let head = (stylus - elbow).normalized();
    pen.segment(
        stylus - head * 10.0,
        stylus + head * 2.0,
        7.0,
        mix(arm, s.peak, 0.3),
    );
}

/// An additive colour (see `super::glow`).
fn glow(c: Color32, k: f32) -> Color32 {
    super::glow(c, k)
}

// -- the radar -----------------------------------------------------------------------

fn radar(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let dt = step.motion;
    let unred = f.s.unred();
    let (a, s, r) = (f.a, &unred, f.rect);
    let c = r.center();
    let radius = r.width().min(r.height()) * 0.46;
    let period = 3.2;
    let before = st.sweep;
    // (the beam a little faster the louder it is - it swept the same with
    // music and without: review 20, L8)
    st.sweep += dt / period * TAU * (0.8 + 0.45 * a.energy.min(1.0));
    // a ping on the beat: a ring going out from the middle (no sooner
    // than a third of a second after the last)
    for age in st.pings.iter_mut() {
        *age += dt;
    }
    if a.beat.count != st.ping_beat {
        st.ping_beat = a.beat.count;
        if st.pings.iter().all(|age| *age > 0.33)
            && let Some(oldest) = st.pings.iter_mut().max_by(|p, q| p.total_cmp(q))
        {
            *oldest = 0.0;
        }
    }
    // what the beam passed this frame: the spectrum there (the bass at
    // the top, round to the treble), an echo as far out as it is loud
    let steps = (((st.sweep - before) / (TAU / 360.0)).ceil() as usize).min(90);
    for k in 1..=steps {
        let angle = before + (st.sweep - before) * k as f32 / steps as f32;
        let x = (angle / TAU).rem_euclid(1.0);
        let level = band(a, x);
        if level > 0.1 {
            if st.blips.len() == BLIPS {
                st.blips.remove(0);
            }
            st.blips.push([angle, level, level, 0.0]);
        }
    }
    st.sweep %= TAU * 1000.0;
    for b in st.blips.iter_mut() {
        b[3] += dt;
    }
    st.blips.retain(|b| b[3] < period * 0.95);
    // the scope's face: range rings, bearings, a faint disc
    // (on a light ground the scope's face is a shade deeper and its echoes
    // are inked: pale marks on a pale ground - review 18, L6)
    let light = !super::scenes::dark(s);
    dot(
        pen,
        c,
        radius,
        mix(s.background, s.low, if light { 0.2 } else { 0.1 }),
    );
    let line = if light { s.grid() } else { s.track() };
    for k in 1..=4 {
        pen.ring(c, radius * k as f32 / 4.0, 1.0, line);
    }
    for k in 0..36 {
        let u = Vec2::angled(k as f32 / 36.0 * TAU);
        let long = if k % 3 == 0 { 0.06 } else { 0.03 };
        pen.segment(c + u * radius * (1.0 - long), c + u * radius, 1.0, s.grid());
    }
    pen.segment(c - vec2(radius, 0.0), c + vec2(radius, 0.0), 1.0, line);
    pen.segment(c - vec2(0.0, radius), c + vec2(0.0, radius), 1.0, line);
    // (the pings: thin rings, fading as they reach the rim in 0.9 s)
    for age in st.pings {
        let out = age / 0.9;
        if out < 1.0 {
            pen.ring(
                c,
                radius * (0.08 + 0.92 * out),
                1.5 * small(r),
                shine(s, s.high, if light { 0.6 } else { 0.45 } * (1.0 - out)),
            );
        }
    }
    // the afterglow behind the beam
    let beam = st.sweep - PI / 2.0;
    let i0 = pen.mesh.vertices.len() as u32;
    pen.mesh.colored_vertex(c, shine(s, s.mid, 0.0));
    const TRAIL: usize = 28;
    for k in 0..=TRAIL {
        let t = k as f32 / TRAIL as f32;
        let angle = beam - 1.1 * (1.0 - t);
        pen.mesh.colored_vertex(
            c + Vec2::angled(angle) * radius,
            shine(s, s.mid, if light { 0.42 } else { 0.28 } * t * t),
        );
    }
    for k in 0..TRAIL as u32 {
        pen.mesh.add_triangle(i0, i0 + 1 + k, i0 + 2 + k);
    }
    pen.segment(
        c,
        c + Vec2::angled(beam) * radius,
        1.5,
        shine(s, s.high, 0.8),
    );
    // the echoes, fading over a turn
    let small = small(r);
    for b in &st.blips {
        let [angle, distance, level, age] = *b;
        let life = (1.0 - age / (period * 0.95)).powf(1.5);
        let at = c + Vec2::angled(angle - PI / 2.0) * radius * (0.12 + 0.85 * distance);
        let colour = if light {
            mix(s.at(0.3 + 0.7 * level), s.peak, 0.45)
        } else {
            s.at(0.3 + 0.7 * level)
        };
        // (the return painted in from the middle, faint, then its echo)
        let root = c + Vec2::angled(angle - PI / 2.0) * radius * 0.12;
        streak(
            pen,
            root,
            at,
            2.2 * small,
            shine(s, colour, 0.02 * life),
            shine(s, colour, if light { 0.3 } else { 0.12 } * life),
        );
        dot(
            pen,
            at,
            (if light { 1.6 } else { 1.2 } + 1.8 * level) * small,
            shine(
                s,
                colour,
                (life * (0.45 + 0.55 * level) * if light { 1.25 } else { 1.0 }).min(1.0),
            ),
        );
    }
    soft(
        pen,
        c,
        radius * 0.08,
        shine(s, s.high, 0.3 + 0.2 * a.beat.pulse),
    );
}

// -- the pendulum wave ---------------------------------------------------------------

fn pendulum(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let dt = step.music;
    let (a, s, r) = (f.a, f.s, f.rect);
    let n = ((r.width() / 26.0) as usize).clamp(8, PENDULUMS);
    let t = st.clock;
    let calm = super::scheme_calm(s);
    let top = r.top() + r.height() * 0.08;
    let reach = r.height() * 0.8;
    let width = r.width() / n as f32;
    let small = small(r);
    let ease = 1.0 - (-dt / 0.3).exp();
    // the beam they hang from
    pen.rect(
        Rect::from_min_max(
            pos2(r.left() + width * 0.2, top - 3.0),
            pos2(r.right() - width * 0.2, top + 2.0),
        ),
        mix(s.background, s.text(), 0.4),
    );
    // each pendulum a band; their periods a whole cycle of thirty seconds
    // apart (20, 21, ... swings in it: the wave turns into snakes and
    // back), their swing as loud as their band
    let mut place = [(Pos2::ZERO, 0.0f32, 0.0f32, 0usize); PENDULUMS];
    for k in 0..n {
        let level = span(a, k as f32 / n as f32, (k + 1) as f32 / n as f32);
        st.swing[k] += (0.25 + 0.75 * level - st.swing[k]) * ease;
        let period = 30.0 / (20.0 + k as f32);
        let phase = (TAU * t / period).cos() * st.swing[k];
        let x = r.left() + (k as f32 + 0.5) * width;
        let anchor = pos2(x, top);
        let bob = pos2(x, top + reach * (0.55 + 0.38 * phase));
        place[k] = (bob, phase, level, k);
        pen.segment(
            anchor,
            bob,
            1.0,
            faint(mix(s.background, s.text(), 0.5), s.background),
        );
    }
    // far ones (swung away: higher) first
    place[..n].sort_unstable_by(|p, q| p.1.total_cmp(&q.1));
    for &(bob, phase, level, k) in &place[..n] {
        let colour = s.at(0.25 + 0.75 * level);
        let size = (width * 0.36).min(18.0 * small) * (0.8 + 0.25 * phase);
        // (its light on the floor below)
        let pool = pos2(bob.x, r.bottom() - 6.0 * small);
        soft(pen, pool, size * 2.4, shine(s, colour, 0.06 + 0.12 * level));
        // (its blur: where it was a moment before)
        let period = 30.0 / (20.0 + k as f32);
        for j in 1..=4 {
            let then = t - j as f32 * 0.035;
            let ph = (TAU * then / period).cos() * st.swing[k];
            let p = pos2(bob.x, top + reach * (0.55 + 0.38 * ph));
            dot(
                pen,
                p,
                size * 0.9,
                shine(s, colour, 0.12 * (1.0 - j as f32 / 5.0)),
            );
        }
        soft(
            pen,
            bob,
            size * 2.6,
            shine(s, colour, (0.12 + 0.25 * level) / calm),
        );
        dot(
            pen,
            bob,
            size,
            mix(s.background, colour, 0.8 + 0.2 * phase.max(0.0)),
        );
        dot(
            pen,
            bob - vec2(size, size) * 0.3,
            size * 0.35,
            mix(colour, s.peak, 0.55),
        );
    }
}

// -- the spectrum rain ---------------------------------------------------------------

fn rain(f: &Frame<'_>, pen: &mut Pen<'_>, st: &mut State, step: Step) {
    let dt = step.motion;
    let (a, s, r) = (f.a, f.s, f.rect);
    let size = ((r.height() / 20.0 / 2.0).round() * 2.0).clamp(12.0, 22.0);
    let font = FontId::monospace(size);
    let galley = f
        .painter
        .layout_no_wrap(GLYPHS.to_owned(), font, Color32::WHITE);
    let atlas = f.painter.ctx().fonts(|fonts| fonts.font_image_size());
    let uv = vec2(1.0 / atlas[0].max(1) as f32, 1.0 / atlas[1].max(1) as f32);
    let Some(row) = galley.rows.first() else {
        return;
    };
    let mesh = &row.row.visuals.mesh;
    let glyphs = mesh.vertices.len() / 4;
    if glyphs == 0 {
        return;
    }
    let pitch_x = size * 0.95;
    let pitch_y = size * 1.1;
    let cols = ((r.width() / pitch_x) as usize).max(1);
    let rows = (r.height() / pitch_y).ceil() as usize + 1;
    if st.drops.len() != cols {
        st.drops.clear();
        for _ in 0..cols {
            let head = st.random() * rows as f32;
            let length = 6.0 + 12.0 * st.random();
            st.drops.push([head, 0.0, length]);
        }
    }
    let calm = super::scheme_calm(s);
    let left = r.left() + (r.width() - cols as f32 * pitch_x) / 2.0;
    // the spectrum the rain lands on, faint, at the foot
    for col in 0..cols {
        let level = band(a, (col as f32 + 0.5) / cols as f32);
        let x = left + col as f32 * pitch_x;
        let h = level * r.height() * 0.22;
        pen.vertical(
            Rect::from_min_max(pos2(x, r.bottom() - h), pos2(x + pitch_x, r.bottom())),
            shine(s, s.at(level), 0.0),
            shine(s, s.at(level), 0.18),
        );
    }
    let t = st.clock;
    for col in 0..cols {
        let level = band(a, (col as f32 + 0.5) / cols as f32);
        // each column falling as fast as its band is loud
        let speed = 3.0 + 13.0 * level;
        let drop = &mut st.drops[col];
        drop[0] += speed * dt;
        if drop[0] - drop[2] > rows as f32 {
            drop[0] = -hash(col as u32, (t * 10.0) as u32) * rows as f32 * 0.4;
            drop[2] = 6.0 + 12.0 * hash(col as u32, (t * 7.0) as u32 + 1);
        }
        let [head, _, length] = *drop;
        let x = left + (col as f32 + 0.5) * pitch_x;
        let first = (head - length).floor().max(0.0) as usize;
        let last = (head.floor() as usize).min(rows.saturating_sub(1));
        if head < 0.0 {
            continue;
        }
        for row in first..=last {
            let behind = head - row as f32;
            let k = (1.0 - behind / length).clamp(0.0, 1.0);
            // (a glyph changes about once a second, each in its own time)
            let changes = (t * 0.9 + hash(col as u32 * 977 + row as u32, 1) * 9.0) as u32;
            let g =
                (hash(col as u32 * 31 + row as u32 * 7, changes) * glyphs as f32) as usize % glyphs;
            let at = pos2(x, r.top() + (row as f32 + 0.5) * pitch_y);
            let colour = if behind < 1.0 {
                mix(s.high, s.peak, 0.6 / calm)
            } else {
                mix(
                    s.background,
                    s.at(0.2 + 0.6 * level),
                    0.2 + 0.75 * k.powf(1.5),
                )
            };
            if behind < 1.0 {
                soft(pen, at, size * 1.1, shine(s, s.high, 0.18));
            }
            // the glyph, mirrored (as if seen from behind the glass)
            let base = pen.mesh.vertices.len() as u32;
            let quad = &mesh.vertices[g * 4..g * 4 + 4];
            let mut centre = Vec2::ZERO;
            for v in quad {
                centre += v.pos.to_vec2();
            }
            centre /= 4.0;
            for v in quad {
                let mut vertex = *v;
                let d = v.pos.to_vec2() - centre;
                vertex.pos = at + vec2(-d.x, d.y);
                vertex.color = colour;
                vertex.uv = pos2(v.uv.x * uv.x, v.uv.y * uv.y);
                pen.mesh.vertices.push(vertex);
            }
            let first_index = g * 6;
            for i in &mesh.indices[first_index..first_index + 6] {
                pen.mesh.indices.push(base + i - (g * 4) as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pitch_spirals_names_have_room_within_the_picture() {
        // (19 points outside the spiral, 11 points tall: with 22 points of
        // room the lowest, "F#", was cut by the picture's edge - review 18,
        // L4)
        for (w, h) in [
            (620.0, 300.0),
            (240.0, 135.0),
            (300.0, 300.0),
            (1920.0, 1080.0),
        ] {
            let r = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
            let (c, _, outer) = spiral_frame(r);
            let reach = outer + 19.0 + 11.0 / 2.0;
            assert!(
                c.y + reach <= r.bottom() - 3.0 && c.y - reach >= r.top() + 3.0,
                "{w} x {h}: a name reaches {reach} points from the middle, the picture {}",
                h / 2.0
            );
        }
    }
}
