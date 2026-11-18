//! axross's eighteen faces, drawn as its media_player.py draws them (the
//! same geometry, colours and constants; its comments say why), into
//! meshes: one or a few a frame, whatever the bar count.

// (bars, peaks and their places go in step: an index reads best)
#![allow(clippy::needless_range_loop)]

use super::{Canvas, Frame, Pen, Scheme, VizState, between, glow, mix, readable_on};
use crate::analyzer::Analyzer;
use egui::{Align2, Color32, FontId, Pos2, Rect, pos2, vec2};
use std::f32::consts::{PI, TAU};

/// The starfield: stars, their drift in silence and with full bass, the
/// nearest depth (axross's STAR_*).
const STAR_COUNT: usize = 300;
/// Stars of the starfield with trails (an option; axross's has 300 dots).
const STARS_DENSE: usize = 640;
const STAR_DRIFT: f32 = 0.15;
const STAR_BOOST: f32 = 1.2;
const STAR_NEAR: f32 = 0.04;
/// The LED equalizer (LED_*).
const LED_SEGMENTS: usize = 24;
const LED_MIN_PITCH: f32 = 6.0;
const LED_GAP: f32 = 0.28;
/// The particle columns (DOT_*, SPARK_*).
const DOT_SHARE: f32 = 0.62;
const DOT_SIZE: (f32, f32) = (2.0, 12.0);
const DOT_PITCH: f32 = 1.5;
const SPARK_ONSET: f32 = 0.08;
const SPARK_FLOOR: f32 = 0.25;
const SPARK_SPAWN: usize = 4;
const SPARK_MAX: usize = 160;
const SPARK_RISE: f32 = 0.5;
const SPARK_LIFE: f32 = 1.2;
/// The wave ribbon (RIBBON_*).
/// The ribbon's lines at most (fewer in a small picture).
const RIBBON_LINES: usize = 9;
const RIBBON_POINTS: usize = 120;
const RIBBON_TWISTS: f32 = 2.5;
const RIBBON_WIDTH: f32 = 0.7;
const RIBBON_COLOURS: usize = 8;
/// The ridgelines and the terrain (HISTORY_ROWS, TERRAIN_*).
const RIDGE_ROWS: usize = 28;
const TERRAIN_ROWS: usize = 22;
const TERRAIN_FAR: f32 = 6.0;
/// The ring (RING_*), the kaleidoscope, the bells.
const RING_TILT: f32 = 0.3;
const RING_FAR: f32 = 0.55;
const KALEIDO_FOLD: usize = 6;
const BELLS_MAX: usize = 32;
const BELL_POINTS: usize = 25;
/// The scopes: points of the stereo scope, the parts it fades in.
const LISSAJOUS_POINTS: usize = 512;
const LISSAJOUS_FADE: usize = 4;
/// Frames of the Lissajous figure kept, fading, behind the newest.
const LISSAJOUS_KEPT: usize = 6;
/// The width of the traces (axross: one pixel; here a little more, as
/// egui's lines are antialiased at their edges).
const TRACE: f32 = 1.2;

#[derive(Default)]
pub struct State {
    /// x, y (-1..1) and depth of each star.
    stars: Vec<[f32; 3]>,
    /// bar, height 0..1, age (s), rise a second.
    sparks: Vec<[f32; 4]>,
    /// The ring's bars, far ones first, for its bar count.
    ring_order: Vec<usize>,
    seed: u32,
    /// The Lissajous figures before (x, y -1..1), LISSAJOUS_KEPT of
    /// LISSAJOUS_POINTS, a ring; the next to write, how many there are.
    figures: Vec<[f32; 2]>,
    figure_at: usize,
    figures_kept: usize,
    /// The scopes' gain: their highest sample lately (falling slowly), so
    /// that quiet or near-mono music still fills them.
    scope_peak: f32,
    figure_peak: f32,
    /// The scope's shown height (guarded: noise in bursts made it flash),
    /// and its last loud wave (kept, fading, when the sound stops short).
    scope_guard: crate::analyzer::guard::Guard,
    scope_last: Vec<f32>,
    scope_last_height: f32,
    /// How bright the trace is drawn (eased: a dense wave dimmer).
    scope_bright: f32,
    /// The LED equalizer's lit segments a column, and the particles' lit
    /// dots (held: see `lit`).
    led_on: Vec<usize>,
    particles_on: Vec<usize>,
    /// Their columns' heights, falling no faster than `fallen` lets them.
    led_level: Vec<f32>,
    particles_level: Vec<f32>,
    /// The ribbon's height along it, eased; its twist (the onsets' phase,
    /// followed at Calm's pace, slower in the starkest schemes).
    ribbon_height: Vec<f32>,
    ribbon_phase: f32,
    /// The kaleidoscope's six groups and its peak, eased; its turn, and
    /// the onsets' phase it last followed.
    kaleido: [f32; 7],
    kaleido_turn: f32,
    kaleido_phase: f32,
}

/// A scope's gain for the highest sample lately `peak` (kept up to date,
/// falling over a few seconds): that peak at 90 % of the height, at most
/// 8 times louder, never smaller than as it is.
fn auto_gain(peak: &mut f32, now: f32, dt: f32) -> f32 {
    *peak = now.max(*peak * (-dt / 3.0).exp()).max(1e-4);
    (0.9 / *peak).clamp(1.0, 8.0)
}

impl State {
    fn random(&mut self) -> f32 {
        super::random(&mut self.seed)
    }

    /// The same stars every time (tests).
    pub fn reset_stars(&mut self, seed: u32) {
        self.reset_stars_n(seed, STAR_COUNT);
    }

    fn reset_stars_n(&mut self, seed: u32, count: usize) {
        self.seed = seed;
        self.stars.clear();
        for _ in 0..count {
            let x = self.random() * 2.0 - 1.0;
            let y = self.random() * 2.0 - 1.0;
            let z = STAR_NEAR + self.random() * (1.0 - STAR_NEAR);
            self.stars.push([x, y, z]);
        }
    }
}

/// The face's area in axross's terms: (0, 0) at its top left, `w` x `h`.
#[derive(Clone, Copy)]
struct Area {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
}

impl Area {
    fn of(rect: Rect) -> Area {
        Area {
            x0: rect.left(),
            y0: rect.top(),
            w: rect.width(),
            h: rect.height(),
        }
    }
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.x0 + x, self.y0 + y)
    }
    fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.p(x, y), vec2(w, h))
    }
}

pub fn draw(f: &Frame<'_>, key: &str, state: &mut VizState) {
    if key == "waterfall" && state.waterfall_hd {
        super::analysis::waterfall_hd(f, state);
        return;
    }
    let g = Area::of(f.rect);
    let (a, s) = (f.a, f.s);
    let trails = state.classic_plus;
    let cv = &mut state.canvas;
    let st = &mut state.classic;
    match key {
        "mirror" => mirror(f, g, cv),
        "radial" => radial(f, g, cv),
        "line" => line(f, g, cv),
        "led" => led(f, g, cv, st),
        "particles" => particles(f, g, cv, st),
        "polar" => polar(f, g, cv),
        "waterfall" => waterfall(f, g, cv),
        "ridges" => ridges(f, g, cv),
        "terrain" => terrain(f, g, cv),
        "ring" => ring(f, g, cv, st),
        "bells" => bells(f, g, cv),
        "ribbon" => ribbon(f, g, cv, st),
        "kaleidoscope" => kaleidoscope(f, g, cv, st),
        "scope" => scope(f, g, cv, st),
        "lissajous" => lissajous(f, g, cv, st, trails),
        "vu" => vu(f, g, cv),
        "starfield" => starfield(f, g, cv, st, trails),
        _ => spectrum(f.painter, g, a, s, cv),
    }
}

fn spectrum(painter: &egui::Painter, g: Area, a: &Analyzer, s: &Scheme, cv: &mut Canvas) {
    let n = a.bars.len().max(1);
    let width = g.w / n as f32;
    let bottom = g.h;
    let height = bottom - 4.0;
    let bar_width = (width - 2.0).max(1.0);
    let ground = s.background;
    cv.draw(painter, |pen| {
        // (behind the bars, between them, their colours a shade dimmer:
        // bars on a dark ground are a regular grating - a pattern hazard -
        // where they stand side by side; the shorter of two neighbours'
        // heights is shared)
        for i in 0..n {
            let x = i as f32 * width;
            let next = a.bars.get(i + 1).copied().unwrap_or(0.0);
            let shared = (a.bars[i].min(next) * height).max(0.0);
            let own = (a.bars[i] * height).max(2.0);
            pen.gradient_as(
                g.r(x, bottom - own, 1.0, own),
                s,
                g.y0 + bottom,
                g.y0,
                |c| between(c, ground),
            );
            if shared > 0.0 {
                pen.gradient_as(
                    g.r(x + width - 1.0, bottom - shared, 1.0, shared),
                    s,
                    g.y0 + bottom,
                    g.y0,
                    |c| between(c, ground),
                );
            }
        }
        for i in 0..n {
            let x = i as f32 * width + 1.0;
            let bar = (a.bars[i] * height).max(2.0);
            pen.gradient(g.r(x, bottom - bar, bar_width, bar), s, g.y0 + bottom, g.y0);
        }
        for i in 0..n {
            let x = i as f32 * width + 1.0;
            pen.rect(
                g.r(x, bottom - a.peaks[i] * height - 3.0, bar_width, 2.0),
                s.peak,
            );
        }
    });
}

fn mirror(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let width = g.w / n as f32;
    let mid = g.h / 2.0;
    let reach = mid - 3.0;
    let bar_width = (width - 2.0).max(1.0);
    let ground = s.background;
    cv.draw(f.painter, |pen| {
        // (between the bars, a shade dimmer: see the spectrum)
        for i in 0..n {
            let x = i as f32 * width;
            let next = a.bars.get(i + 1).copied().unwrap_or(0.0);
            for (len, at) in [
                ((a.bars[i] * reach).max(1.0), x),
                ((a.bars[i].min(next) * reach).max(0.0), x + width - 1.0),
            ] {
                if len > 0.0 {
                    let dim = |c| between(c, ground);
                    pen.gradient_as(g.r(at, mid - len, 1.0, len), s, g.y0 + mid, g.y0, dim);
                    pen.gradient_as(g.r(at, mid, 1.0, len), s, g.y0 + mid, g.y0 + g.h, dim);
                }
            }
        }
        for i in 0..n {
            let x = i as f32 * width + 1.0;
            let bar = (a.bars[i] * reach).max(1.0);
            pen.gradient(g.r(x, mid - bar, bar_width, bar), s, g.y0 + mid, g.y0);
            pen.gradient(g.r(x, mid, bar_width, bar), s, g.y0 + mid, g.y0 + g.h);
            let p = a.peaks[i] * reach;
            pen.rect(g.r(x, mid - p - 2.0, bar_width, 1.5), s.peak);
            pen.rect(g.r(x, mid + p + 0.5, bar_width, 1.5), s.peak);
        }
    });
}

fn radial(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let c = g.p(g.w / 2.0, g.h / 2.0);
    let radius = (g.w / 2.0).min(g.h / 2.0);
    let inner = radius * 0.22;
    let reach = (radius - inner - 2.0).max(1.0);
    let width = (TAU * inner / n as f32 * 0.6).max(1.5);
    cv.draw(f.painter, |pen| {
        for i in 0..n {
            let angle = TAU * i as f32 / n as f32 - PI / 2.0;
            let u = vec2(angle.cos(), angle.sin());
            let outer = inner + (a.bars[i] * reach).max(1.0);
            // (a flat-ended bar: a quad, its sides antialiased by a thin line)
            let side = vec2(-u.y, u.x) * width / 2.0;
            let colour = s.at(a.bars[i]);
            let (p0, p1) = (c + u * inner, c + u * outer);
            pen.quad([p0 - side, p0 + side, p1 + side, p1 - side], [colour; 4]);
            let tip = inner + a.peaks[i] * reach + 2.0;
            pen.rect(Rect::from_center_size(c + u * tip, vec2(2.0, 2.0)), s.peak);
        }
        pen.ring(c, inner - 2.0, 1.0, s.grid());
    });
}

/// The share of the line spectrum's axis at `hz` (40 Hz .. its top).
fn log_axis(hz: f32, rate: u32) -> f32 {
    let top = 16_000f32.min(rate as f32 / 2.0);
    (hz / 40.0).ln() / (top / 40.0).ln()
}

/// The area under the line in `pen.points` down to `base`, in the bars'
/// gradient (quiet at `quiet_y`, loud at `loud_y`), split at the
/// gradient's middle stop so it shows.
fn gradient_under(pen: &mut Pen<'_>, s: &Scheme, base: f32, quiet_y: f32, loud_y: f32) {
    let span = loud_y - quiet_y;
    let t = |y: f32| ((y - quiet_y) / span).clamp(0.0, 1.0);
    let stop = quiet_y + 0.6 * span;
    // (above the stop: y < stop, as the loud end is up)
    let strip = |pen: &mut Pen<'_>, p: Pos2, q: Pos2| {
        if p.y < stop && q.y < stop && base > stop {
            pen.quad(
                [
                    pos2(p.x, base),
                    pos2(q.x, base),
                    pos2(q.x, stop),
                    pos2(p.x, stop),
                ],
                [s.at(t(base)), s.at(t(base)), s.mid, s.mid],
            );
            pen.quad(
                [pos2(p.x, stop), pos2(q.x, stop), q, p],
                [s.mid, s.mid, s.at(t(q.y)), s.at(t(p.y))],
            );
        } else {
            pen.quad(
                [pos2(p.x, base), pos2(q.x, base), q, p],
                [s.at(t(base)), s.at(t(base)), s.at(t(q.y)), s.at(t(p.y))],
            );
        }
    };
    for w in 1..pen.points.len() {
        let (p, q) = (pen.points[w - 1], pen.points[w]);
        if (p.y - stop) * (q.y - stop) < 0.0 {
            // the edge crosses the stop: two strips, each on one side of it
            let k = (stop - p.y) / (q.y - p.y);
            let m = pos2(p.x + (q.x - p.x) * k, stop);
            strip(pen, p, m);
            strip(pen, m, q);
        } else {
            strip(pen, p, q);
        }
    }
}

fn line(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    let grid = [(100.0, "100"), (1000.0, "1k"), (10000.0, "10k")];
    let reach = h - 4.0;
    cv.draw(f.painter, |pen| {
        for (hz, _) in grid {
            let x = log_axis(hz, a.rate) * w;
            pen.rect(g.r(x - 0.5, 0.0, 1.0, h), s.grid());
        }
        pen.points.clear();
        pen.points.push(g.p(0.0, h - a.bars[0] * reach));
        for i in 0..n {
            pen.points
                .push(g.p((i as f32 + 0.5) / n as f32 * w, h - a.bars[i] * reach));
        }
        pen.points.push(g.p(w, h - a.bars[n - 1] * reach));
        gradient_under(pen, s, g.y0 + h, g.y0 + h, g.y0);
        // the edge in the colour of its height
        let top = pen.points.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        pen.line(TRACE, s.at(((g.y0 + h - top) / h).clamp(0.0, 1.0)));
        pen.points.clear();
        for i in 0..n {
            pen.points.push(g.p(
                (i as f32 + 0.5) / n as f32 * w,
                h - a.peaks[i] * reach - 2.0,
            ));
        }
        pen.line(TRACE, s.peak);
    });
    // over the curve: the axis stays readable
    for (hz, label) in grid {
        let x = log_axis(hz, a.rate) * w;
        f.painter.text(
            g.p(x + 3.0, 2.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(11.0),
            s.text(),
        );
    }
}

/// How many of `cells` a level lights, with a hysteresis of 0.35 of a
/// cell: a bar hovering at a cell's edge lit and darkened it again and
/// again (each a transition, counted strictly).
/// The columns' heights as drawn: rising as the bars rise, falling at most
/// so fast - in the starkest schemes slower (a column of lit cells going
/// dark row after row as the music stops sweeps a quarter of a third of
/// the screen in a tenth of a second: by the strict reading, a flash
/// again and again).
fn fallen(levels: &mut Vec<f32>, bars: &[f32], s: &Scheme, dt: f32) {
    if levels.len() != bars.len() {
        levels.clear();
        levels.extend_from_slice(bars);
    }
    // (0.8 of the height a second; white on black a third of that)
    let fall = 0.8 / super::scheme_calm(s).powi(2) * dt;
    for (l, b) in levels.iter_mut().zip(bars) {
        *l = b.max(*l - fall);
    }
}

fn lit(level: f32, cells: usize, before: usize) -> usize {
    let x = level * cells as f32;
    let now = (x + 0.5) as usize;
    let held = if now > before {
        if x > before as f32 + 0.85 {
            now
        } else {
            before
        }
    } else if now < before {
        if x < before as f32 - 0.15 {
            now
        } else {
            before
        }
    } else {
        now
    };
    held.min(cells)
}

fn led(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    let segments = ((h / LED_MIN_PITCH) as usize).clamp(3, LED_SEGMENTS);
    let pitch = ((h - 2.0) / segments as f32).max(1.0);
    let gap = (pitch * LED_GAP).round().max(1.0);
    let step = w / n as f32;
    let column = (step * 0.76).round().max(1.0);
    let ground = s.background;
    if st.led_on.len() != n {
        st.led_on = vec![0; n];
    }
    fallen(&mut st.led_level, &a.bars, s, f.dt);
    for i in 0..n {
        st.led_on[i] = lit(st.led_level[i], segments, st.led_on[i]);
    }
    let led_on = &st.led_on;
    cv.draw(f.painter, |pen| {
        // (behind the lit segments - between them and between lit columns
        // - their colours a shade dimmer: lit cells on a dark ground are a
        // regular grating both ways, a pattern hazard)
        for i in 0..n {
            let left = (i as f32 * step + step * 0.12).round();
            let on = led_on[i];
            let next = led_on.get(i + 1).copied().unwrap_or(0);
            let next_left = ((i + 1) as f32 * step + step * 0.12).round();
            for (from, to, k) in [
                (left, left + column, on),
                (left + column, next_left, on.min(next)),
            ] {
                // (a band a segment, each of its segment's colour a shade
                // dimmer: one gradient from the top's colour to the
                // bottom's ran under a ramp through yellow darker than the
                // segments by more than a tenth - in Intense, standing
                // stripes over 46 % of the picture)
                for j in 0..k.min(segments) {
                    let top = (h - 1.0 - (j + 1) as f32 * pitch).round();
                    let bottom = (h - 1.0 - j as f32 * pitch).round();
                    if bottom > top && to > from {
                        pen.rect(
                            g.r(from, top, to - from, bottom - top),
                            between(s.at((j as f32 + 0.5) / segments as f32), ground),
                        );
                    }
                }
            }
        }
        for i in 0..n {
            let left = (i as f32 * step + step * 0.12).round();
            let on = led_on[i];
            let high = ((a.peaks[i] * segments as f32 + 0.5) as usize).min(segments);
            for k in 0..segments {
                let top = (h - 1.0 - (k + 1) as f32 * pitch).round();
                let height = ((h - 1.0 - k as f32 * pitch - gap).round() - top).max(1.0);
                let lit = s.at((k as f32 + 0.5) / segments as f32);
                let colour = if high > 0 && k == high - 1 {
                    s.peak
                } else if k < on {
                    lit
                } else {
                    mix(s.background, lit, 0.18)
                };
                pen.rect(g.r(left, top, column, height), colour);
            }
        }
    });
}

/// A dot: a small polygon (no feather: thousands of them a frame).
fn disc(pen: &mut Pen<'_>, centre: Pos2, radius: f32, colour: Color32) {
    if radius < 1.6 {
        pen.rect(
            Rect::from_center_size(centre, vec2(radius, radius) * 2.0),
            colour,
        );
        return;
    }
    const SIDES: usize = 10;
    // (cos, sin) of k * 36 degrees
    const RING: [(f32, f32); SIDES] = [
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
    for k in 0..SIDES as u32 {
        pen.mesh
            .add_triangle(i0, i0 + 1 + k, i0 + 1 + (k + 1) % SIDES as u32);
    }
}

fn particles(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    let step = w / n as f32;
    let size = (step * DOT_SHARE).clamp(DOT_SIZE.0, DOT_SIZE.1);
    let pitch = (size * DOT_PITCH).max(3.0);
    let rows = (((h - 4.0) / pitch) as usize).max(1);
    // the sparks: the alive ones rise and age, new ones where a bar jumped
    for spark in st.sparks.iter_mut() {
        spark[1] += spark[3] * f.dt;
        spark[2] += f.dt;
    }
    st.sparks.retain(|k| k[2] < SPARK_LIFE && k[1] < 1.1);
    if a.ticks > 0 && a.rise.len() == n {
        let mut born = 0;
        // (from a random bar on: no side favoured when more jump than spawn)
        let first = (st.random() * n as f32) as usize;
        for k in 0..n {
            let i = (first + k) % n;
            if born < SPARK_SPAWN && a.rise[i] >= SPARK_ONSET && a.bars[i] >= SPARK_FLOOR {
                let jitter = st.random() * 0.6 - 0.3;
                let rise = SPARK_RISE * (0.6 + 0.8 * st.random());
                if st.sparks.len() == SPARK_MAX {
                    st.sparks.remove(0);
                }
                st.sparks.push([i as f32 + jitter, a.bars[i], 0.0, rise]);
                born += 1;
            }
        }
    }
    if st.sparks.capacity() < SPARK_MAX {
        st.sparks.reserve(SPARK_MAX - st.sparks.len());
    }
    if st.particles_on.len() != n {
        st.particles_on = vec![0; n];
    }
    fallen(&mut st.particles_level, &a.bars, s, f.dt);
    let levels = &st.particles_level;
    let sparks = &st.sparks;
    let st_on = &mut st.particles_on;
    cv.draw(f.painter, |pen| {
        for i in 0..n {
            let x = i as f32 * step + step / 2.0;
            st_on[i] = lit(levels[i], rows, st_on[i]);
            let on = st_on[i];
            let high = ((a.peaks[i] * rows as f32 + 0.5) as usize).min(rows);
            // (behind a lit column, a soft band of its colours: the dots on
            // it, not a grid of bright dots on black - a regular pattern of
            // high contrast is what a photosensitive eye cannot bear)
            // (the columns touch, on a band a shade dimmer than the dots -
            // the most contrast that is no grating: at 320 x 180 a band of
            // 75 % between gaps made stripes over 40 % of the picture)
            if on > 0 {
                let wide = step.max(size);
                for r in 0..on {
                    let y = h - 2.0 - (r + 1) as f32 * pitch;
                    let level = s.at((r as f32 + 0.5) / rows as f32);
                    pen.rect(
                        g.r(x - wide / 2.0, y, wide, pitch + 0.3),
                        between(level, s.background),
                    );
                }
            }
            for r in 0..rows {
                let level = s.at((r as f32 + 0.5) / rows as f32);
                let colour = if high > 0 && r == high - 1 {
                    s.peak
                } else if r < on {
                    level
                } else {
                    mix(s.background, level, 0.16)
                };
                let y = h - 2.0 - (r as f32 + 0.5) * pitch;
                disc(pen, g.p(x, y), size / 2.0, colour);
            }
        }
        let dot = (size * 0.7).max(2.0) / 2.0;
        for [bar, height, age, _] in sparks {
            let x = (bar + 0.5) * step;
            let y = h - 2.0 - height * rows as f32 * pitch;
            // (from loud to quiet, not from the peak's white: a sweep
            // through the bands sent up a swarm of white sparks that
            // brightened a third of the picture's top seven times a second)
            let colour = match age / SPARK_LIFE {
                t if t < 1.0 / 3.0 => s.high,
                t if t < 2.0 / 3.0 => s.mid,
                _ => mix(s.background, s.mid, 0.6),
            };
            disc(pen, g.p(x, y), dot, colour);
        }
    });
}

fn polar(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let c = g.p(g.w / 2.0, g.h / 2.0);
    let radius = (g.w / 2.0).min(g.h / 2.0);
    let inner = radius * 0.18;
    let reach = (radius - inner - 3.0).max(1.0);
    // the bars down the right half from the top, then mirrored up the left
    let spoke = |k: usize| {
        let (i, angle) = if k < n {
            (k, PI * (k as f32 + 0.5) / n as f32)
        } else {
            let j = 2 * n - 1 - k;
            (j, TAU - PI * (j as f32 + 0.5) / n as f32)
        };
        (i, vec2(angle.sin(), -angle.cos()))
    };
    // the radial gradient: quiet at the inner circle, middle at 0.6, loud at the edge
    let colour = |r: f32| {
        let t = r / radius;
        let stop = inner / radius;
        if t <= stop {
            s.low
        } else if t < 0.6 {
            mix(s.low, s.mid, (t - stop) / (0.6 - stop))
        } else {
            mix(s.mid, s.high, (t - 0.6) / 0.4)
        }
    };
    cv.draw(f.painter, |pen| {
        for k in 0..2 * n {
            let (i, u) = spoke(k);
            let (j, v) = spoke((k + 1) % (2 * n));
            let (ri, rj) = (inner + a.bars[i] * reach, inner + a.bars[j] * reach);
            // along each spoke: the centre, the inner circle, the middle stop, the edge
            let stops = |r: f32| [0.0, inner.min(r), (0.6 * radius).min(r), r];
            let (si, sj) = (stops(ri), stops(rj));
            for q in 0..3 {
                pen.quad(
                    [
                        c + u * si[q],
                        c + v * sj[q],
                        c + v * sj[q + 1],
                        c + u * si[q + 1],
                    ],
                    [
                        colour(si[q]),
                        colour(sj[q]),
                        colour(sj[q + 1]),
                        colour(si[q + 1]),
                    ],
                );
            }
        }
        pen.points.clear();
        for k in 0..2 * n {
            let (i, u) = spoke(k);
            pen.points.push(c + u * (inner + a.bars[i] * reach));
        }
        pen.outline(1.0, colour(radius * 0.8));
        pen.points.clear();
        for k in 0..2 * n {
            let (i, u) = spoke(k);
            pen.points.push(c + u * (inner + a.peaks[i] * reach + 2.0));
        }
        pen.outline(TRACE, s.peak);
    });
}

fn waterfall(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let rows = crate::analyzer::HISTORY;
    let cw = g.w / n as f32;
    let rh = g.h / rows as f32;
    // the newest row at the bottom; between two steps it moves up smoothly
    let shift = a.tick_fraction() * rh;
    // axross's colours: the background, the quiet colour at 0.15, the
    // middle at 0.6, the loud at 1
    let lut = |v: f32| {
        if v < 0.15 {
            mix(s.background, s.low, v / 0.15)
        } else if v < 0.6 {
            mix(s.low, s.mid, (v - 0.15) / 0.45)
        } else {
            mix(s.mid, s.high, (v - 0.6) / 0.4)
        }
    };
    let painter = f
        .painter
        .with_clip_rect(f.rect.intersect(f.painter.clip_rect()));
    cv.draw(&painter, |pen| {
        for age in 0..=rows {
            let y = g.h - (age + 1) as f32 * rh - shift;
            if y + rh < 0.0 {
                break;
            }
            // (the bars as guarded - turning no faster than the motion
            // allows - not as worked out: scrolled past a pixel, a row a
            // tick, a click nine times a second flickered nine times; and
            // their change from their slow mean halved, eased)
            let row = a.waterfall_row(age);
            for (i, v) in row.iter().enumerate() {
                if *v < 0.004 {
                    continue; // (the background)
                }
                pen.rect(g.r(i as f32 * cw, y, cw + 0.4, rh + 0.4), lut(*v));
            }
        }
    });
}

/// A colour per history row, back to front: the scheme's colours from
/// quiet (far) to loud (near), the far ones dimmed into the background;
/// the front row in the peak colour (axross's _depth_pens).
fn depth_colour(s: &Scheme, k: usize, rows: usize) -> Color32 {
    let depth = k as f32 / (rows - 1).max(1) as f32;
    if k == rows - 1 {
        s.peak
    } else {
        mix(s.background, s.at(depth), 0.3 + 0.6 * depth)
    }
}

fn ridges(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    cv.draw(f.painter, |pen| {
        // the sun behind: rays into the sky, the upper half of a disc in
        // stripes that thin out towards the horizon - larger with the bass
        if w >= 120.0 && h >= 120.0 {
            let (cx, cy) = (w / 2.0, h * 0.32);
            let radius = w.min(h) * 0.15 * (1.0 + 0.6 * a.bass);
            let far = w.hypot(h);
            for i in 1..36 {
                let angle = PI + PI * i as f32 / 36.0;
                let d = vec2(angle.cos(), angle.sin());
                pen.segment(
                    g.p(cx + d.x * radius * 1.2, cy + d.y * radius * 1.2),
                    g.p(cx + d.x * far, cy + d.y * far),
                    1.0,
                    s.grid(),
                );
            }
            let stripes = 9;
            let band = radius / stripes as f32;
            for i in 0..stripes {
                let top = cy - radius + i as f32 * band;
                let thick = band * (1.0 - 0.75 * i as f32 / stripes as f32);
                let middle = top + thick / 2.0;
                let half = (radius * radius - (cy - middle) * (cy - middle))
                    .max(0.0)
                    .sqrt();
                let at = |y: f32| {
                    mix(
                        s.high,
                        s.mid,
                        ((y - (cy - radius)) / radius).clamp(0.0, 1.0),
                    )
                };
                pen.vertical(
                    g.r(cx - half, top, 2.0 * half, thick),
                    at(top),
                    at(top + thick),
                );
            }
        }
    });
    // the ridges, back to front: each hides what is behind it
    let rows = RIDGE_ROWS;
    let top = h * 0.32;
    let bottom = h - 3.0;
    let spacing = (bottom - top) / (rows - 1) as f32;
    let reach = (h * 0.28).min(spacing * 7.0);
    let margin = w * 0.04;
    cv.draw(f.painter, |pen| {
        for k in 0..rows {
            let depth = k as f32 / (rows - 1) as f32;
            let half = w * 0.4 * (0.72 + 0.28 * depth);
            let base = top + k as f32 * spacing;
            let rise = reach * (0.75 + 0.25 * depth);
            let (left, right) = (w / 2.0 - half, w / 2.0 + half);
            let row = a.history_row(rows - 1 - k);
            pen.points.clear();
            pen.points.push(g.p(margin, base));
            pen.points.push(g.p(left, base));
            for (i, v) in row.iter().enumerate().take(n) {
                let u = (i as f32 + 0.5) / n as f32;
                let envelope = (PI * u).sin().max(0.0).powf(0.6);
                pen.points
                    .push(g.p(left + u * (right - left), base - v * envelope * rise));
            }
            pen.points.push(g.p(right, base));
            pen.points.push(g.p(w - margin, base));
            let background = s.background;
            pen.under(g.y0 + base, background, |_| background);
            pen.line(1.0, depth_colour(s, k, rows));
        }
    });
}

fn terrain(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    // (never saturated red: its scrolling lines and dots, in a pure red
    // scheme, flashed red seven times a second in Intense)
    let unred = f.s.unred();
    let (a, s) = (f.a, &unred);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    let rows = TERRAIN_ROWS;
    let horizon = h * 0.3;
    // row k (0: the farthest) as far away as `depth`
    let depth = |k: usize| TERRAIN_FAR + (1.0 - TERRAIN_FAR) * k as f32 / (rows - 1) as f32;
    let base = |k: usize| horizon + (h - 4.0 - horizon) / depth(k);
    let point = |k: usize, i: usize| {
        let u = (i as f32 + 0.5) / n as f32 - 0.5;
        let v = a.history_row(rows - 1 - k)[i];
        g.p(
            w / 2.0 + u * w * 1.15 / depth(k),
            base(k) - v * h * 0.5 / depth(k),
        )
    };
    cv.draw(f.painter, |pen| {
        for i in (0..n).step_by((n / 24).max(1)) {
            pen.points.clear();
            for k in 0..rows {
                pen.points.push(point(k, i));
            }
            pen.line(1.0, s.grid());
        }
        for k in 0..rows {
            pen.points.clear();
            for i in 0..n {
                pen.points.push(point(k, i));
            }
            pen.line(1.0, depth_colour(s, k, rows));
        }
        // dots on the front half where it is loud - on every other row,
        // their colour as loud as it is, a shade into the ground (bright
        // dots switched on and off at two levels, a row on at each step,
        // kept a quarter of the middle of the picture changing at once)
        // (every other row of the history, as it scrolls: the same rows)
        let head = a.history_index() + crate::analyzer::HISTORY;
        for k in (rows / 2..rows).filter(|k| (head - (rows - 1 - k)).is_multiple_of(2)) {
            let row = a.history_row(rows - 1 - k);
            for (i, v) in row.iter().enumerate().take(n) {
                if *v < 0.45 {
                    continue;
                }
                let loud = ((v - 0.45) / 0.4).clamp(0.0, 1.0);
                let colour = mix(s.background, mix(s.mid, s.high, loud), 0.4 + 0.3 * loud);
                pen.rect(Rect::from_center_size(point(k, i), vec2(2.2, 2.2)), colour);
            }
        }
        for (i, peak) in a.peaks.iter().enumerate() {
            let p = point(rows - 1, i);
            disc(
                pen,
                pos2(p.x, g.y0 + base(rows - 1) - peak * h * 0.5 - 4.0),
                1.75,
                s.peak,
            );
        }
    });
}

fn ring(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    let rx = (w * 0.42).min(h * 0.3 / RING_TILT).max(1.0);
    let ry = rx * RING_TILT;
    let (cx, cy) = (w / 2.0, h - ry - (h * 0.06).max(4.0));
    let reach = ((cy - ry - 4.0) / RING_FAR).max(1.0);
    let foot = |i: usize| {
        let angle = TAU * i as f32 / n as f32 + PI / 2.0;
        let (sin, cos) = angle.sin_cos();
        (
            g.p(cx + rx * cos, cy + ry * sin),
            RING_FAR + (1.0 - RING_FAR) * (sin + 1.0) / 2.0,
        )
    };
    if st.ring_order.len() != n {
        // far ones first (their feet higher up)
        st.ring_order.clear();
        st.ring_order.extend(0..n);
        st.ring_order
            .sort_by(|x, y| foot(*x).0.y.total_cmp(&foot(*y).0.y).then(x.cmp(y)));
    }
    let order = &st.ring_order;
    // (axross's bars, half as wide as their spacing; what lies between
    // them a shade dimmer than they - on the dark ground they made regular
    // stripes over 40 % of a small picture, a pattern hazard)
    let bar = TAU * rx / n as f32 * 0.5;
    let c = g.p(cx, cy);
    cv.draw(f.painter, |pen| {
        ellipse(pen, c, vec2(rx, ry), s.track());
        ellipse_line(pen, c, vec2(rx, ry), s.grid());
        ellipse(pen, c, vec2(rx, ry) * 0.55, s.background);
        ellipse_line(pen, c, vec2(rx, ry) * 0.55, s.grid());
        for &i in order {
            let (p, scale) = foot(i);
            let thick = (bar * scale).max(1.0);
            // quiet in front, loud at the back and round again; the far side
            // a third of the way into the background
            let colour = mix(
                readable_on(
                    s.at(1.0 - (2.0 * i as f32 - n as f32).abs() / n as f32),
                    s.background,
                    3.0,
                ),
                s.background,
                (1.0 - scale) / (1.0 - RING_FAR) / 3.0,
            );
            let rise = a.bars[i] * reach * scale;
            if rise >= 1.0 {
                pen.rect(
                    Rect::from_min_max(
                        pos2(p.x - thick * 1.1, p.y - rise),
                        pos2(p.x + thick * 1.1, p.y),
                    ),
                    between(colour, s.track()),
                );
                pen.rect(
                    Rect::from_min_max(
                        pos2(p.x - thick / 2.0, p.y - rise),
                        pos2(p.x + thick / 2.0, p.y),
                    ),
                    colour,
                );
            }
            let top = a.peaks[i] * reach * scale;
            if top >= 1.0 {
                pen.rect(
                    Rect::from_min_size(pos2(p.x - thick / 2.0, p.y - top - 3.0), vec2(thick, 2.0)),
                    s.peak,
                );
            }
        }
    });
}

fn ellipse_points(pen: &mut Pen<'_>, c: Pos2, r: egui::Vec2) {
    pen.points.clear();
    let k = 64;
    for i in 0..k {
        let t = TAU * i as f32 / k as f32;
        pen.points.push(c + vec2(r.x * t.cos(), r.y * t.sin()));
    }
}

fn ellipse(pen: &mut Pen<'_>, c: Pos2, r: egui::Vec2, colour: Color32) {
    ellipse_points(pen, c, r);
    pen.fan(c, colour, colour);
}

fn ellipse_line(pen: &mut Pen<'_>, c: Pos2, r: egui::Vec2, colour: Color32) {
    ellipse_points(pen, c, r);
    pen.outline(1.0, colour);
}

fn bells(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    let groups = n.min(BELLS_MAX);
    let sigma = w / groups as f32 * 0.5;
    let base = h - 3.0;
    let reach = (h - 8.0).max(1.0);
    let span = |gi: usize| {
        (
            gi * n / groups,
            ((gi + 1) * n / groups).max(gi * n / groups + 1),
        )
    };
    let mut loudest = [0.0f32; BELLS_MAX];
    let mut tops = [0.0f32; BELLS_MAX];
    for gi in 0..groups {
        let (from, to) = span(gi);
        loudest[gi] = a.bars[from..to].iter().cloned().fold(0.0, f32::max);
        tops[gi] = a.peaks[from..to].iter().cloned().fold(0.0, f32::max);
    }
    // the quiet ones behind, the loud in front
    let mut order = [0usize; BELLS_MAX];
    for (k, o) in order.iter_mut().enumerate() {
        *o = k;
    }
    order[..groups].sort_unstable_by(|x, y| loudest[*x].total_cmp(&loudest[*y]).then(x.cmp(y)));
    cv.draw(f.painter, |pen| {
        pen.rect(g.r(0.0, base, w, 1.0), s.grid());
        for (rank, &gi) in order[..groups].iter().enumerate() {
            let centre = (gi as f32 + 0.5) / groups as f32 * w;
            pen.points.clear();
            for p in 0..BELL_POINTS {
                let o = -3.0 + 6.0 * p as f32 / (BELL_POINTS - 1) as f32;
                pen.points.push(g.p(
                    centre + o * sigma,
                    base - loudest[gi] * reach * (-0.5 * o * o).exp(),
                ));
            }
            gradient_under(pen, s, g.y0 + base, g.y0 + h, g.y0);
            // (the outline of the three loudest only: every bell's, side
            // by side in a bright scheme, lay in moving stripes; four, in
            // Intense, for a frame)
            if rank + 3 >= groups {
                pen.line(1.0, s.at(loudest[gi]));
            }
        }
        for gi in 0..groups {
            let centre = (gi as f32 + 0.5) / groups as f32 * w;
            pen.rect(
                g.r(
                    centre - sigma * 0.8,
                    base - tops[gi] * reach - 3.0,
                    sigma * 1.6,
                    2.0,
                ),
                s.peak,
            );
        }
    });
}

fn ribbon(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let (w, h) = (g.w, g.h);
    // (followed at Calm's pace in Intense too, slower in the starkest
    // schemes: twisted at once by each onset, its pure red lines swept a
    // quarter of a field seven times a second; the phase wraps at 4 pi)
    let mut d = a.phase - st.ribbon_phase;
    let wrap = 4.0 * PI;
    while d > wrap / 2.0 {
        d -= wrap;
    }
    while d < -wrap / 2.0 {
        d += wrap;
    }
    let rate = a.motion.phase_follow().min(3.0) / super::scheme_calm(s);
    st.ribbon_phase = (st.ribbon_phase + d * (1.0 - (-f.dt * rate).exp())).rem_euclid(wrap);
    let phase = st.ribbon_phase;
    // the bands, smoothed over five (edges repeated)
    let at = |i: isize| a.bars[i.clamp(0, n as isize - 1) as usize];
    let smooth = |i: usize| {
        let i = i as isize;
        (at(i - 2) + 2.0 * at(i - 1) + 3.0 * at(i) + 2.0 * at(i + 1) + at(i + 2)) / 9.0
    };
    let level = |u: f32| {
        let x = u * (n - 1) as f32;
        let i = (x.floor() as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        smooth(i) + (smooth(j) - smooth(i)) * (x - i as f32)
    };
    // (its height along it eased over a quarter of a second, in Calm half
    // a second: the bundle of lines opening and closing with each onset
    // swept a quarter of a small picture seven times a second)
    if st.ribbon_height.len() != RIBBON_POINTS {
        st.ribbon_height = vec![0.2; RIBBON_POINTS];
    }
    let ease = 1.0 - (-f.dt / (0.35 * a.motion.turn() / 0.25)).exp();
    for (p, h) in st.ribbon_height.iter_mut().enumerate() {
        let u = p as f32 / (RIBBON_POINTS - 1) as f32;
        *h += (0.2 + 0.8 * level(u).max(0.0).sqrt() - *h) * ease;
    }
    let heights = &st.ribbon_height;
    // (in a small picture fewer lines: nine bright lines moving together
    // were a quarter of a field there - a flash - where at full screen they
    // are a tenth)
    let lines = match h {
        h if h < 260.0 => 5,
        h if h < 360.0 => 7,
        _ => RIBBON_LINES,
    };
    let y = |j: usize, p: usize| {
        let u = p as f32 / (RIBBON_POINTS - 1) as f32;
        let offset = -1.0 + 2.0 * j as f32 / (lines - 1) as f32;
        let amplitude = heights[p];
        let envelope = (PI * u).sin().max(0.0).powf(0.6);
        let theta = TAU * RIBBON_TWISTS * u + phase;
        let wave = 0.7 * theta.sin() + 0.3 * (TAU * 1.3 * u - 0.5 * phase).sin();
        let shape = (wave + RIBBON_WIDTH * offset * theta.cos()) / (1.0 + RIBBON_WIDTH);
        g.p(u * w, h / 2.0 - h * 0.46 * envelope * amplitude * shape)
    };
    let middle = lines / 2;
    cv.draw(f.painter, |pen| {
        for st in 0..RIBBON_COLOURS {
            let from = ((st * (RIBBON_POINTS - 1)) as f32 / RIBBON_COLOURS as f32).round() as usize;
            let to =
                (((st + 1) * (RIBBON_POINTS - 1)) as f32 / RIBBON_COLOURS as f32).round() as usize;
            let colour = s.at((st as f32 + 0.5) / RIBBON_COLOURS as f32);
            for j in 0..lines {
                if j == middle {
                    continue;
                }
                pen.points.clear();
                for p in from..=to {
                    pen.points.push(y(j, p));
                }
                pen.line(1.0, colour);
            }
        }
        pen.points.clear();
        for p in 0..RIBBON_POINTS {
            pen.points.push(y(middle, p));
        }
        pen.line(1.2, s.peak);
    });
}

fn kaleidoscope(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State) {
    let (a, s) = (f.a, f.s);
    let n = a.bars.len().max(1);
    let c = g.p(g.w / 2.0, g.h / 2.0);
    let radius = ((g.w / 2.0).min(g.h / 2.0) - 2.0).max(1.0);
    let alpha = PI / KALEIDO_FOLD as f32;
    // six groups of the bands (numpy's array_split), their means
    let mut groups = [0.0f32; 6];
    for (k, group) in groups.iter_mut().enumerate() {
        let size = |k: usize| n / 6 + usize::from(k < n % 6);
        let from: usize = (0..k).map(size).sum();
        let to = from + size(k);
        if to > from {
            *group = a.bars[from..to].iter().sum::<f32>() / (to - from) as f32;
        }
    }
    let peak = a.peaks.iter().cloned().fold(0.0f32, f32::max);
    // (eased over a fifth of a second, a third in Calm: its petals, dots
    // and diamonds, each following a group in its own time, made twelve
    // transitions a second together - all 24 copies at once)
    let ease = 1.0 - (-f.dt / (0.5 * a.motion.turn() / 0.25)).exp();
    for (k, v) in groups.iter().chain([peak].iter()).enumerate() {
        st.kaleido[k] += (v - st.kaleido[k]) * ease;
    }
    groups.copy_from_slice(&st.kaleido[..6]);
    let peak = st.kaleido[6];
    let polar = |r: f32, angle: f32| vec2(r * angle.cos(), r * angle.sin());
    // the petal: two cubic curves, sampled
    let length = radius * (0.35 + 0.6 * (groups[0] + groups[1]) / 2.0);
    let cubic = |p0: egui::Vec2, p1: egui::Vec2, p2: egui::Vec2, p3: egui::Vec2, t: f32| {
        let u = 1.0 - t;
        p0 * u * u * u + p1 * 3.0 * u * u * t + p2 * 3.0 * u * t * t + p3 * t * t * t
    };
    let start = polar(radius * 0.06, alpha * 0.5);
    let tip = polar(length, alpha * 0.45);
    const STEPS: usize = 14;
    let petal = |k: usize| {
        if k < STEPS {
            cubic(
                start,
                polar(length * 0.5, alpha * 1.7),
                polar(length * 0.95, alpha * 1.1),
                tip,
                k as f32 / STEPS as f32,
            )
        } else {
            cubic(
                tip,
                polar(length * 0.75, -alpha * 0.1),
                polar(length * 0.35, alpha * 0.05),
                start,
                (k - STEPS) as f32 / STEPS as f32,
            )
        }
    };
    let mut centroid = egui::Vec2::ZERO;
    for k in 0..2 * STEPS {
        centroid += petal(k);
    }
    centroid /= (2 * STEPS) as f32;
    // (it turns with the onsets, but no faster than a petal's width in
    // two-thirds of a second: turned at once by an onset, petals and gaps
    // changed places all round - a flash of half the picture)
    let mut step = (a.phase - st.kaleido_phase) * 0.5;
    if step < -PI {
        step += TAU; // (the phase wrapped, at 4 pi)
    }
    st.kaleido_phase = a.phase;
    // (as fast in Intense as in Calm: turned faster, petals and gaps
    // changed places more than six times a second, strictly counted)
    let most = alpha * 1.5 * f.dt * 0.25 / a.motion.turn().max(0.25);
    st.kaleido_turn = (st.kaleido_turn + step.clamp(-most, most)) % TAU;
    let turn = st.kaleido_turn;
    cv.draw(f.painter, |pen| {
        // layer by layer, each in all twelve copies (axross's order)
        for layer in 0..5 {
            for k in 0..KALEIDO_FOLD {
                for mirrored in [false, true] {
                    let angle = turn + k as f32 * TAU / KALEIDO_FOLD as f32;
                    let (sin, cos) = angle.sin_cos();
                    let place = |v: egui::Vec2| {
                        let v = if mirrored { vec2(v.x, -v.y) } else { v };
                        c + vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
                    };
                    match layer {
                        0 | 1 => {
                            let scale = if layer == 0 { 1.0 } else { 0.58 };
                            pen.points.clear();
                            for q in 0..2 * STEPS {
                                pen.points.push(place(petal(q) * scale));
                            }
                            let colour = if layer == 0 { s.low } else { s.track() };
                            pen.fan(place(centroid * scale), colour, colour);
                        }
                        2 => {
                            for (j, level) in groups[2..4].iter().enumerate() {
                                let centre = polar(
                                    radius * (0.3 + 0.18 * j as f32 + 0.25 * level),
                                    alpha * (0.3 + 0.4 * j as f32),
                                );
                                let size = radius * (0.03 + 0.09 * level);
                                pen.circle(place(centre), size, s.mid);
                            }
                        }
                        3 => {
                            for (j, level) in groups[4..6].iter().enumerate() {
                                let r = radius * (0.55 + 0.1 * j as f32 + 0.3 * level);
                                let reach = radius * (0.06 + 0.16 * level);
                                let angle = alpha * (0.85 - 0.5 * j as f32);
                                pen.points.clear();
                                for p in [
                                    polar(r - reach, angle),
                                    polar(r, angle + alpha * 0.12),
                                    polar(r + reach, angle),
                                    polar(r, angle - alpha * 0.12),
                                ] {
                                    pen.points.push(place(p));
                                }
                                pen.fan(place(polar(r, angle)), s.high, s.high);
                            }
                        }
                        _ => {
                            let mark = polar(radius * (0.16 + 0.12 * peak), alpha * 0.5);
                            pen.circle(place(mark), radius * 0.035, s.peak);
                        }
                    }
                }
            }
        }
    });
}

/// Points of the scope's trace kept (its last loud one, fading).
const SCOPE_KEPT: usize = 1024;

fn scope(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State) {
    let (a, s) = (f.a, f.s);
    let mid = g.h / 2.0;
    let n = a.mono.len();
    let loudest = a.mono.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let gain = auto_gain(&mut st.scope_peak, loudest, f.dt);
    // the wave's height as shown no faster to and fro than the flash guard
    // allows: rising, the wave is scaled down to it; when the sound stops
    // short, the last loud wave stays and fades (it does not vanish at once)
    let height = (loudest * gain).min(1.0);
    let shown_height = st.scope_guard.step(height, f.dt, a.motion);
    let scale = if height > 1e-4 {
        gain * (shown_height / height).min(1.0)
    } else {
        gain
    };
    if st.scope_last.len() != SCOPE_KEPT {
        st.scope_last = vec![0.0; SCOPE_KEPT];
        st.scope_last_height = 0.0;
    }
    let points = (g.w as usize).clamp(2, SCOPE_KEPT);
    let start = (1..n / 2)
        .find(|&i| a.mono[i - 1] <= 0.0 && a.mono[i] > 0.0)
        .unwrap_or(0);
    let wave = &a.mono[start.min(n)..(start + n / 2).min(n)];
    // (while the guard holds the height - the wave would come and go
    // faster than it allows - the last wave it let through is shown, as
    // tall as it says; a new one only once they agree)
    let fading = (shown_height - height).abs() > 0.03 && st.scope_last_height > 1e-3;
    if !fading && wave.len() >= 2 && height > 0.0 {
        for p in 0..points {
            let k = p * (wave.len() - 1) / (points - 1);
            st.scope_last[p] = (wave[k] * scale).clamp(-1.0, 1.0);
        }
        st.scope_last_height = shown_height.max(1e-3);
    }
    // (a wave as dense as noise, whose trace lies in stripes, drawn dimmer
    // - the held one too: moving stripes of high contrast are what a
    // photosensitive eye cannot bear; at 0.35 an Intense, noise-dense
    // trace still lay in moving stripes over 27 % of a small picture -
    // and eased, so that the trace's brightness does not flicker itself)
    // (crossings a point of the trace: the live wave's every sample, the
    // held one's a point each)
    let shown_wave: &[f32] = if fading {
        &st.scope_last[..points]
    } else {
        wave
    };
    let crossings = shown_wave
        .windows(2)
        .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
        .count() as f32
        / points.min(shown_wave.len()).max(1) as f32;
    // (a crossing every ten pixels or more often is a grating: a pure tone
    // drawn so, zigzag, lay in stripes over 46 % of a small picture)
    let want = (1.25 - crossings * 5.0).clamp(0.25, 1.0);
    // (dimmer at once, brighter again slowly)
    if st.scope_bright <= 0.0 || want < st.scope_bright {
        st.scope_bright = want;
    } else {
        st.scope_bright += (want - st.scope_bright) * (1.0 - (-f.dt / 0.5).exp());
    }
    let trace = mix(s.background, s.trace(), st.scope_bright);
    // (a wave as dense as noise: faint - in a bright scheme a quarter of
    // its colour still lay in stripes over 46 % of a small picture)
    let trace = if st.scope_bright < 0.45 || want < 0.45 {
        super::faint(trace, s.background)
    } else {
        trace
    };
    let kept = &st.scope_last;
    let kept_height = st.scope_last_height;
    cv.draw(f.painter, |pen| {
        pen.rect(g.r(0.0, mid - 0.5, g.w, 1.0), s.grid());
        if n < 4 || wave.len() < 2 {
            return;
        }
        // the held wave: the last let through, as tall as the guard says
        if fading {
            pen.points.clear();
            let k = shown_height / kept_height;
            for (p, v) in kept.iter().take(points).enumerate() {
                pen.points.push(g.p(
                    p as f32 / (points - 1) as f32 * (g.w - 1.0),
                    mid - v * k * (mid - 2.0),
                ));
            }
            pen.line(TRACE, trace);
            return;
        }
        // (here: from a rising zero crossing - the wave stands still -
        // half the samples, a point a pixel at most)
        let points = points.min(wave.len());
        pen.points.clear();
        for p in 0..points {
            let k = p * (wave.len() - 1) / (points - 1);
            let v = (wave[k] * scale).clamp(-1.0, 1.0);
            pen.points.push(g.p(
                p as f32 / (points - 1) as f32 * (g.w - 1.0),
                mid - v * (mid - 2.0),
            ));
        }
        pen.line(TRACE, trace);
    });
}

fn lissajous(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State, trails: bool) {
    let (a, s) = (f.a, f.s);
    let (cx, cy) = (g.w / 2.0, g.h / 2.0);
    let reach = cx.min(cy) - 3.0;
    let diagonal = reach * std::f32::consts::FRAC_1_SQRT_2;
    // the figure now (mid and side, -1..1), kept for the frames after
    let count = a.stereo.len();
    if st.figures.len() != LISSAJOUS_KEPT * LISSAJOUS_POINTS {
        st.figures = vec![[0.0; 2]; LISSAJOUS_KEPT * LISSAJOUS_POINTS];
        st.figures_kept = 0;
    }
    let loudest = a
        .stereo
        .iter()
        .fold(0.0f32, |m, [l, r]| m.max((l + r).abs()).max((l - r).abs()))
        * std::f32::consts::FRAC_1_SQRT_2;
    let gain = auto_gain(&mut st.figure_peak, loudest, f.dt);
    if count >= 2 && f.dt > 0.0 {
        let root = std::f32::consts::FRAC_1_SQRT_2 * gain;
        let at = st.figure_at * LISSAJOUS_POINTS;
        for k in 0..LISSAJOUS_POINTS {
            let [l, r] = a.stereo[k * (count - 1) / (LISSAJOUS_POINTS - 1)];
            st.figures[at + k] = [
                ((r - l) * root).clamp(-1.0, 1.0),
                ((l + r) * root).clamp(-1.0, 1.0),
            ];
        }
        st.figure_at = (st.figure_at + 1) % LISSAJOUS_KEPT;
        st.figures_kept = (st.figures_kept + 1).min(LISSAJOUS_KEPT);
    }
    let figures = &st.figures;
    let (newest, kept) = (
        (st.figure_at + LISSAJOUS_KEPT - 1) % LISSAJOUS_KEPT,
        st.figures_kept,
    );
    cv.draw(f.painter, |pen| {
        let grid = s.grid();
        pen.segment(g.p(cx - reach, cy), g.p(cx + reach, cy), 1.0, grid);
        pen.segment(g.p(cx, cy - reach), g.p(cx, cy + reach), 1.0, grid);
        for sign in [-1.0, 1.0] {
            pen.segment(
                g.p(cx - sign * diagonal, cy - diagonal),
                g.p(cx + sign * diagonal, cy + diagonal),
                1.0,
                grid,
            );
        }
        if kept == 0 {
            return;
        }
        // (as axross draws it: the newest alone; with trails, the ones
        // before too)
        let kept = if trails { kept } else { 1 };
        let point = |figure: usize, k: usize| {
            let [x, y] = figures[figure * LISSAJOUS_POINTS + k];
            g.p(cx + x * reach, cy - y * reach)
        };
        // like a phosphor screen: the figures before still glow, fading,
        // in the middle colour
        for age in (1..kept).rev() {
            let figure = (newest + LISSAJOUS_KEPT - age) % LISSAJOUS_KEPT;
            pen.points.clear();
            for k in 0..LISSAJOUS_POINTS {
                pen.points.push(point(figure, k));
            }
            let fade = 1.0 - age as f32 / LISSAJOUS_KEPT as f32;
            pen.line(1.0, mix(s.background, s.mid, 0.45 * fade * fade));
        }
        // the newest: its older part fading into the background, with a glow
        let size = (LISSAJOUS_POINTS - 1).div_ceil(LISSAJOUS_FADE);
        for part in 0..LISSAJOUS_FADE {
            let (start, stop) = (part * size, ((part + 1) * size + 1).min(LISSAJOUS_POINTS));
            pen.points.clear();
            for k in start..stop {
                pen.points.push(point(newest, k));
            }
            let k = (part + 1) as f32 / LISSAJOUS_FADE as f32;
            if trails {
                pen.line(4.0, glow(s.high, 0.14 * k));
            }
            pen.line(TRACE, mix(s.background, s.high, k));
        }
    });
    let font = FontId::proportional(12.0);
    let text = s.text();
    let p = f.painter;
    p.text(
        g.p(cx - diagonal - 3.0, cy - diagonal),
        Align2::RIGHT_TOP,
        "L",
        font.clone(),
        text,
    );
    p.text(
        g.p(cx + diagonal + 3.0, cy - diagonal),
        Align2::LEFT_TOP,
        "R",
        font.clone(),
        text,
    );
    p.text(g.p(cx + 4.0, cy - reach), Align2::LEFT_TOP, "M", font, text);
}

fn vu(f: &Frame<'_>, g: Area, cv: &mut Canvas) {
    let (a, s) = (f.a, f.s);
    let margin = 6.0;
    let label_width = 14.0;
    let left = margin + label_width;
    let width = (g.w - left - margin).max(1.0);
    let row = (g.h - 3.0 * margin) / 2.0;
    // quiet, middle at -15 dBFS (0.75), loud from -6 dBFS (0.9)
    let colour = |t: f32| {
        if t < 0.75 {
            mix(s.low, s.mid, t / 0.75)
        } else if t < 0.9 {
            mix(s.mid, s.high, (t - 0.75) / 0.15)
        } else {
            s.high
        }
    };
    cv.draw(f.painter, |pen| {
        for c in 0..2 {
            let top = margin + c as f32 * (row + margin);
            pen.rect(g.r(left, top, width, row), s.track());
            let level = a.vu[c];
            // the gradient over the whole track, shown up to the level
            let stops = [0.0f32, 0.75, 0.9, 1.0];
            for k in 0..3 {
                let (t0, t1) = (stops[k], stops[k + 1].min(level));
                if t1 <= t0 {
                    break;
                }
                pen.horizontal(
                    g.r(left + width * t0, top, width * (t1 - t0), row),
                    colour(t0),
                    colour(t1),
                );
            }
            let x = left + width * a.vu_peak[c];
            pen.rect(g.r(x - 1.0, top, 2.0, row), s.peak);
        }
        // a tick every 10 dB
        let mut db = 10.0;
        while db < crate::analyzer::VU_RANGE_DB {
            let x = left + width * (1.0 - db / crate::analyzer::VU_RANGE_DB);
            pen.rect(g.r(x, g.h - margin + 1.0, 1.0, margin - 2.0), s.grid());
            db += 10.0;
        }
    });
    for (c, name) in ["L", "R"].iter().enumerate() {
        let top = margin + c as f32 * (row + margin);
        f.painter.text(
            g.p(margin, top + row / 2.0),
            Align2::LEFT_CENTER,
            *name,
            FontId::proportional(13.0),
            s.peak,
        );
    }
}

fn starfield(f: &Frame<'_>, g: Area, cv: &mut Canvas, st: &mut State, trails: bool) {
    let (a, s) = (f.a, f.s);
    let count = if trails { STARS_DENSE } else { STAR_COUNT };
    if st.stars.len() != count {
        st.reset_stars_n(1, count);
    }
    // nearer, faster with the bass; one that leaves the picture starts afar
    let speed = STAR_DRIFT + STAR_BOOST * a.bass;
    for k in 0..count {
        let [x, y, z] = st.stars[k];
        let z = z - speed * f.dt;
        if z <= STAR_NEAR || x.abs() > z * 1.2 || y.abs() > z * 1.2 {
            let nx = st.random() * 2.0 - 1.0;
            let ny = st.random() * 2.0 - 1.0;
            st.stars[k] = [nx, ny, 1.0];
        } else {
            st.stars[k][2] = z;
        }
    }
    let (cx, cy) = (g.w / 2.0, g.h / 2.0);
    let stars = &st.stars;
    let colour_of = |z: f32| match z {
        d if d >= 0.75 => (s.low, 1.0),
        d if d >= 0.5 => (s.mid, 1.6),
        d if d >= 0.25 => (s.high, 2.2),
        _ => (s.peak, 3.0),
    };
    if !trails {
        // axross's: dots, larger and louder in colour as they come nearer
        cv.draw(f.painter, |pen| {
            for [x, y, z] in stars {
                let (colour, size) = colour_of(*z);
                let p = g.p(cx + x / z * cx, cy + y / z * cy);
                if f.rect.expand(2.0).contains(p) {
                    if size > 2.0 {
                        pen.circle(p, size / 2.0, colour);
                    } else {
                        pen.rect(Rect::from_center_size(p, vec2(size, size)), colour);
                    }
                }
            }
        });
        return;
    }
    // (how far a star's trail reaches back: further the faster they fly)
    let trail = 0.02 + speed * 0.1;
    // (never saturated red: in a pure red scheme thousands of streaks
    // sweeping out with the bass flashed red seven times a second)
    let colour_of = |z: f32| {
        let (c, size) = colour_of(z);
        (super::unred(c), size)
    };
    cv.draw(f.painter, |pen| {
        // a faint glow where they come from, swelling with the bass
        pen.radial(
            g.p(cx, cy),
            cx.min(cy) * (0.6 + 0.4 * a.bass),
            glow(super::unred(s.low), 0.06 + 0.12 * a.bass),
            Color32::TRANSPARENT,
        );
        for [x, y, z] in stars {
            let (colour, size) = colour_of(*z);
            let p = g.p(cx + x / z * cx, cy + y / z * cy);
            if !f.rect.expand(2.0).contains(p) {
                continue;
            }
            // its trail: where it was a moment ago, fading
            let back = z + trail;
            let q = g.p(cx + x / back * cx, cy + y / back * cy);
            if (p - q).length() > 1.0 {
                pen.quad(
                    [
                        q,
                        q,
                        p + (p - q).normalized().rot90() * size * 0.5,
                        p - (p - q).normalized().rot90() * size * 0.5,
                    ],
                    [Color32::TRANSPARENT, Color32::TRANSPARENT, colour, colour],
                );
            }
            if size > 2.0 {
                pen.radial(p, size * 3.0, glow(colour, 0.35), Color32::TRANSPARENT);
                pen.circle(p, size / 2.0, colour);
            } else {
                pen.rect(Rect::from_center_size(p, vec2(size, size)), colour);
            }
        }
    });
}
