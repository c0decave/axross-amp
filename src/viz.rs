//! The visualizations, drawn with egui's painter (on the GPU) from the
//! analyzer: the faces of axross's player under the same keys, in its colour
//! schemes. A click on the picture shows the next one.

use crate::analyzer::Analyzer;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, epaint::Mesh, pos2, vec2};
use std::f32::consts::{PI, TAU};

/// The faces, in the order a click steps through them: (key, name).
pub const MODES: [(&str, &str); 18] = [
    ("spectrum", "Spectrum"),
    ("mirror", "Mirrored spectrum"),
    ("radial", "Radial spectrum"),
    ("line", "Line spectrum"),
    ("led", "LED equalizer"),
    ("particles", "Particle columns"),
    ("polar", "Polar spectrum"),
    ("waterfall", "Waterfall"),
    ("ridges", "Ridgelines"),
    ("terrain", "Spectrum terrain"),
    ("ring", "3D ring"),
    ("bells", "Bell curves"),
    ("ribbon", "Wave ribbon"),
    ("kaleidoscope", "Kaleidoscope"),
    ("scope", "Oscilloscope"),
    ("lissajous", "Stereo scope"),
    ("vu", "VU meter"),
    ("starfield", "Starfield"),
];

/// The key a saved name stands for here (a face of axross's not drawn here
/// yet shows the spectrum).
pub fn known(key: &str) -> &'static str {
    MODES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(k, _)| *k)
        .unwrap_or("spectrum")
}

pub fn next(key: &str) -> &'static str {
    let i = MODES.iter().position(|(k, _)| *k == key).unwrap_or(0);
    MODES[(i + 1) % MODES.len()].0
}

pub fn name(key: &str) -> &'static str {
    MODES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, n)| *n)
        .unwrap_or("Spectrum")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheme {
    pub background: Color32,
    pub low: Color32,
    pub mid: Color32,
    pub high: Color32,
    pub peak: Color32,
}

fn hex(text: &str) -> Color32 {
    let v = u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap_or(0);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The named schemes - axross's COLOUR_SCHEMES.
pub const SCHEMES: [(&str, &str, [&str; 5]); 5] = [
    (
        "classic",
        "Classic",
        ["#101418", "#2ecc71", "#f1c40f", "#e74c3c", "#ecf0f1"],
    ),
    (
        "fire",
        "Fire",
        ["#140805", "#8b1a00", "#ff6a00", "#ffd84d", "#fff6e0"],
    ),
    (
        "ice",
        "Ice",
        ["#06111c", "#1b4f8a", "#3fa9f5", "#bdeaff", "#ffffff"],
    ),
    (
        "neon",
        "Neon",
        ["#0b0016", "#00e5ff", "#ff2bd6", "#faff00", "#ffffff"],
    ),
    (
        "mono",
        "Mono",
        ["#0c0c0c", "#555555", "#a0a0a0", "#e8e8e8", "#ffffff"],
    ),
];

pub fn scheme(key: &str, custom: &std::collections::BTreeMap<String, String>) -> Scheme {
    if key == "custom" {
        let get = |k: &str, d: &str| hex(custom.get(k).map(String::as_str).unwrap_or(d));
        let low = get("low", "#1e90ff");
        let high = get("high", "#ff1493");
        return Scheme {
            background: get("background", "#000000"),
            low,
            mid: lerp(low, high, 0.5),
            high,
            peak: get("peak", "#ffffff"),
        };
    }
    let colours = SCHEMES
        .iter()
        .find(|(k, _, _)| *k == key)
        .unwrap_or(&SCHEMES[0])
        .2;
    Scheme {
        background: hex(colours[0]),
        low: hex(colours[1]),
        mid: hex(colours[2]),
        high: hex(colours[3]),
        peak: hex(colours[4]),
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

impl Scheme {
    /// The colour of a level 0..1: quiet, middle, loud.
    pub fn at(&self, v: f32) -> Color32 {
        if v < 0.5 {
            lerp(self.low, self.mid, v * 2.0)
        } else {
            lerp(self.mid, self.high, (v - 0.5) * 2.0)
        }
    }
}

/// What a face keeps between frames: the starfield's stars, the particle
/// columns' sparks, the turn the beat gives the ribbon and the kaleidoscope.
#[derive(Default)]
pub struct VizState {
    stars: Vec<[f32; 3]>,
    seed: u32,
    /// (bar, height 0..1, age s, rise a second)
    sparks: Vec<[f32; 4]>,
    previous: Vec<f32>,
    /// Radians: turns by the spectral flux (the bars' rise), axross's FLUX_TURN.
    pub phase: f32,
}

impl VizState {
    fn random(&mut self) -> f32 {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn gradient_rect(mesh: &mut Mesh, rect: Rect, bottom: Color32, top: Color32) {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

pub fn draw(
    painter: &Painter,
    rect: Rect,
    a: &Analyzer,
    s: &Scheme,
    mode: &str,
    state: &mut VizState,
    dt: f32,
) {
    painter.rect_filled(rect, 8.0, s.background);
    let rect = rect.shrink(10.0);
    // the beat turns the ribbon and the kaleidoscope: the bars' mean rise
    let rise: Vec<f32> = if state.previous.len() == a.bars.len() {
        a.bars
            .iter()
            .zip(&state.previous)
            .map(|(b, p)| (b - p).max(0.0))
            .collect()
    } else {
        vec![0.0; a.bars.len()]
    };
    let flux = rise.iter().sum::<f32>() / rise.len().max(1) as f32;
    state.phase = (state.phase + 3.0 * flux) % (4.0 * PI);
    state.previous.clone_from(&a.bars);
    match mode {
        "particles" => particles(painter, rect, a, s, state, &rise, dt),
        "terrain" => terrain(painter, rect, a, s),
        "ring" => ring(painter, rect, a, s),
        "bells" => bells(painter, rect, a, s),
        "ribbon" => ribbon(painter, rect, a, s, state.phase),
        "kaleidoscope" => kaleidoscope(painter, rect, a, s, state.phase),
        "mirror" => mirror(painter, rect, a, s),
        "radial" => radial(painter, rect, a, s, false),
        "polar" => radial(painter, rect, a, s, true),
        "line" => line(painter, rect, a, s),
        "led" => led(painter, rect, a, s),
        "waterfall" => waterfall(painter, rect, a, s),
        "ridges" => ridges(painter, rect, a, s),
        "scope" => scope(painter, rect, a, s),
        "lissajous" => lissajous(painter, rect, a, s),
        "vu" => vu(painter, rect, a, s),
        "starfield" => starfield(painter, rect, a, s, state, dt),
        _ => spectrum(painter, rect, a, s),
    }
}

fn spectrum(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let n = a.bars.len().max(1);
    let slot = rect.width() / n as f32;
    let gap = (slot * 0.18).clamp(1.0, 4.0);
    let mut mesh = Mesh::default();
    for (i, (bar, peak)) in a.bars.iter().zip(&a.peaks).enumerate() {
        let x = rect.left() + i as f32 * slot;
        let h = bar * rect.height();
        let r = Rect::from_min_max(
            pos2(x + gap / 2.0, rect.bottom() - h),
            pos2(x + slot - gap / 2.0, rect.bottom()),
        );
        if h > 0.5 {
            gradient_rect(&mut mesh, r, s.low, s.at(*bar));
        }
        let py = rect.bottom() - peak * rect.height();
        painter.rect_filled(
            Rect::from_min_max(pos2(r.left(), py - 3.0), pos2(r.right(), py)),
            1.0,
            s.peak,
        );
    }
    painter.add(Shape::mesh(mesh));
}

fn mirror(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let n = a.bars.len().max(1);
    let slot = rect.width() / n as f32;
    let gap = (slot * 0.18).clamp(1.0, 4.0);
    let mid = rect.center().y;
    let mut mesh = Mesh::default();
    for (i, bar) in a.bars.iter().enumerate() {
        let x = rect.left() + i as f32 * slot;
        let h = bar * rect.height() / 2.0;
        if h < 0.5 {
            continue;
        }
        let up = Rect::from_min_max(
            pos2(x + gap / 2.0, mid - h),
            pos2(x + slot - gap / 2.0, mid),
        );
        let down = Rect::from_min_max(
            pos2(x + gap / 2.0, mid),
            pos2(x + slot - gap / 2.0, mid + h),
        );
        gradient_rect(&mut mesh, up, s.low, s.at(*bar));
        gradient_rect(
            &mut mesh,
            down,
            s.at(*bar).gamma_multiply(0.55),
            s.low.gamma_multiply(0.55),
        );
    }
    painter.add(Shape::mesh(mesh));
}

fn radial(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme, closed: bool) {
    let c = rect.center();
    let base = rect.width().min(rect.height()) * 0.2;
    let reach = rect.width().min(rect.height()) * 0.3;
    let n = a.bars.len().max(1);
    if closed {
        // a polygon through every bar, mirrored left and right
        let mut points = Vec::with_capacity(n * 2);
        for k in 0..n * 2 {
            let i = if k < n { k } else { 2 * n - 1 - k };
            let angle = -PI / 2.0 + k as f32 / (n * 2) as f32 * TAU;
            let r = base + a.bars[i] * reach;
            points.push(c + vec2(angle.cos(), angle.sin()) * r);
        }
        let loud = a.bars.iter().cloned().fold(0.0, f32::max);
        // the fill as a fan from the centre: the outline is not convex
        let mut mesh = Mesh::default();
        let fill = s.at(loud).gamma_multiply(0.25);
        mesh.colored_vertex(c, fill.gamma_multiply(0.3));
        for p in &points {
            mesh.colored_vertex(*p, fill);
        }
        let count = points.len() as u32;
        for k in 0..count {
            mesh.add_triangle(0, 1 + k, 1 + (k + 1) % count);
        }
        painter.add(Shape::mesh(mesh));
        points.push(points[0]);
        painter.add(Shape::line(points, Stroke::new(2.5, s.at(loud))));
        painter.circle_stroke(c, base, Stroke::new(1.5, s.low));
        return;
    }
    for (k, bar) in a.bars.iter().chain(a.bars.iter().rev()).enumerate() {
        let angle = -PI / 2.0 + k as f32 / (n * 2) as f32 * TAU;
        let dir = vec2(angle.cos(), angle.sin());
        let width = (TAU * base / (n * 2) as f32 * 0.6).clamp(1.5, 8.0);
        painter.line_segment(
            [c + dir * base, c + dir * (base + 2.0 + bar * reach)],
            Stroke::new(width, s.at(*bar)),
        );
    }
    painter.circle_filled(c, base * 0.92, s.background);
    painter.circle_stroke(
        c,
        base * (0.85 + a.onset * 0.1),
        Stroke::new(2.0, s.peak.gamma_multiply(0.6)),
    );
}

fn line(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let n = a.bars.len().max(2);
    let points: Vec<Pos2> = a
        .bars
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                rect.left() + i as f32 / (n - 1) as f32 * rect.width(),
                rect.bottom() - v * rect.height(),
            )
        })
        .collect();
    let mut mesh = Mesh::default();
    for w in 0..points.len().saturating_sub(1) {
        let (p, q) = (points[w], points[w + 1]);
        let i = mesh.vertices.len() as u32;
        let fade = s.low.gamma_multiply(0.05);
        mesh.colored_vertex(pos2(p.x, rect.bottom()), fade);
        mesh.colored_vertex(pos2(q.x, rect.bottom()), fade);
        mesh.colored_vertex(q, s.at(a.bars[w + 1]).gamma_multiply(0.5));
        mesh.colored_vertex(p, s.at(a.bars[w]).gamma_multiply(0.5));
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(Shape::mesh(mesh));
    let loud = a.bars.iter().cloned().fold(0.0, f32::max);
    painter.add(Shape::line(points, Stroke::new(2.5, s.at(loud))));
}

fn led(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let n = a.bars.len().max(1);
    let rows = 16usize;
    let slot = rect.width() / n as f32;
    let cell = rect.height() / rows as f32;
    for (i, (bar, peak)) in a.bars.iter().zip(&a.peaks).enumerate() {
        let lit = (bar * rows as f32).round() as usize;
        let peak_row = ((peak * rows as f32).round() as usize).min(rows);
        for r in 0..rows {
            let level = (r as f32 + 0.5) / rows as f32;
            let on = r < lit || (r + 1 == peak_row && peak_row > 0);
            let colour = if on {
                s.at(level)
            } else {
                s.at(level).gamma_multiply(0.08)
            };
            let x = rect.left() + i as f32 * slot;
            let y = rect.bottom() - (r + 1) as f32 * cell;
            painter.rect_filled(
                Rect::from_min_max(pos2(x + 1.5, y + 1.5), pos2(x + slot - 1.5, y + cell - 1.5)),
                1.5,
                colour,
            );
        }
    }
}

fn waterfall(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let rows = a.history.len().max(1);
    let n = a.bars.len().max(1);
    let cw = rect.width() / n as f32;
    let rh = rect.height() / crate::analyzer::HISTORY as f32;
    let mut mesh = Mesh::default();
    for (r, row) in a.history.iter().enumerate() {
        // the newest row at the bottom, older ones rising
        let y = rect.bottom() - (rows - r) as f32 * rh;
        for (i, v) in row.iter().enumerate() {
            if *v < 0.02 {
                continue;
            }
            let x = rect.left() + i as f32 * cw;
            let c = s.at(*v).gamma_multiply((0.25 + v * 0.75).min(1.0));
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, y), vec2(cw + 0.5, rh + 0.5)), c);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn ridges(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let lines = 24usize;
    let take: Vec<&Vec<f32>> = a.history.iter().rev().step_by(3).take(lines).collect();
    let spacing = rect.height() / (lines + 6) as f32;
    for (k, row) in take.iter().enumerate().rev() {
        let base = rect.bottom() - k as f32 * spacing;
        let n = row.len().max(2);
        let mut points: Vec<Pos2> = row
            .iter()
            .enumerate()
            .map(|(i, v)| {
                pos2(
                    rect.left() + i as f32 / (n - 1) as f32 * rect.width(),
                    base - v * spacing * 6.0,
                )
            })
            .collect();
        let fade = 1.0 - k as f32 / lines as f32;
        // the ridge hides what is behind it
        let mut fill = points.clone();
        fill.push(pos2(rect.right(), base));
        fill.push(pos2(rect.left(), base));
        let mut mesh = Mesh::default();
        for w in 0..points.len() - 1 {
            let i = mesh.vertices.len() as u32;
            mesh.colored_vertex(pos2(points[w].x, base), s.background);
            mesh.colored_vertex(pos2(points[w + 1].x, base), s.background);
            mesh.colored_vertex(points[w + 1], s.background);
            mesh.colored_vertex(points[w], s.background);
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        painter.add(Shape::mesh(mesh));
        let loud = row.iter().cloned().fold(0.0, f32::max);
        points.dedup();
        painter.add(Shape::line(
            points,
            Stroke::new(1.6, s.at(loud).gamma_multiply(0.35 + 0.65 * fade)),
        ));
    }
}

fn scope(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let mid = rect.center().y;
    painter.line_segment(
        [pos2(rect.left(), mid), pos2(rect.right(), mid)],
        Stroke::new(1.0, s.low.gamma_multiply(0.3)),
    );
    let n = a.mono.len();
    if n < 2 {
        return;
    }
    // start at a rising zero crossing: the wave stands still
    let start = (1..n / 2)
        .find(|&i| a.mono[i - 1] <= 0.0 && a.mono[i] > 0.0)
        .unwrap_or(0);
    let shown = &a.mono[start..(start + n / 2).min(n)];
    let points: Vec<Pos2> = shown
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                rect.left() + i as f32 / shown.len().max(2) as f32 * rect.width(),
                mid - v.clamp(-1.0, 1.0) * rect.height() * 0.45,
            )
        })
        .collect();
    painter.add(Shape::line(
        points.clone(),
        Stroke::new(5.0, s.mid.gamma_multiply(0.18)),
    ));
    painter.add(Shape::line(
        points,
        Stroke::new(2.0, s.at(a.energy.max(0.3))),
    ));
}

fn lissajous(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.45;
    painter.circle_stroke(c, r, Stroke::new(1.0, s.low.gamma_multiply(0.25)));
    painter.line_segment(
        [c - vec2(r, 0.0), c + vec2(r, 0.0)],
        Stroke::new(1.0, s.low.gamma_multiply(0.2)),
    );
    painter.line_segment(
        [c - vec2(0.0, r), c + vec2(0.0, r)],
        Stroke::new(1.0, s.low.gamma_multiply(0.2)),
    );
    // rotated 45°: mono is vertical, wide stereo spreads out
    let points: Vec<Pos2> = a
        .stereo
        .iter()
        .map(|[l, rr]| {
            let x = (l - rr) * std::f32::consts::FRAC_1_SQRT_2;
            let y = (l + rr) * std::f32::consts::FRAC_1_SQRT_2;
            c + vec2(x.clamp(-1.0, 1.0), -y.clamp(-1.0, 1.0)) * r
        })
        .collect();
    if points.len() > 1 {
        painter.add(Shape::line(
            points,
            Stroke::new(1.3, s.mid.gamma_multiply(0.8)),
        ));
    }
}

fn vu(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let half = rect.width() / 2.0;
    for (c, label) in ["L", "R"].iter().enumerate() {
        let area = Rect::from_min_size(
            pos2(rect.left() + c as f32 * half, rect.top()),
            vec2(half, rect.height()),
        )
        .shrink(10.0);
        let pivot = pos2(area.center().x, area.bottom() - 6.0);
        let radius = (area.width() / 2.0).min(area.height() - 12.0);
        // the scale: -48 .. 0 dB over 100 degrees
        let mut arc = Vec::new();
        for k in 0..=40 {
            let t = k as f32 / 40.0;
            let angle = PI + (PI - 1.745) / 2.0 + t * 1.745;
            arc.push(pivot + vec2(angle.cos(), angle.sin()) * radius);
            if k % 5 == 0 {
                let inner = pivot + vec2(angle.cos(), angle.sin()) * (radius - 10.0);
                painter.line_segment([inner, arc[k]], Stroke::new(2.0, s.at(t)));
            }
        }
        painter.add(Shape::line(
            arc,
            Stroke::new(3.0, s.low.gamma_multiply(0.6)),
        ));
        let needle = |v: f32| PI + (PI - 1.745) / 2.0 + v * 1.745;
        let pa = needle(a.level_peak[c]);
        painter.circle_filled(
            pivot + vec2(pa.cos(), pa.sin()) * (radius - 4.0),
            3.0,
            s.peak,
        );
        let angle = needle(a.level[c]);
        painter.line_segment(
            [
                pivot,
                pivot + vec2(angle.cos(), angle.sin()) * (radius - 2.0),
            ],
            Stroke::new(2.5, s.at(a.level[c])),
        );
        painter.circle_filled(pivot, 6.0, s.mid);
        painter.text(
            pos2(area.center().x, area.top() + 12.0),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(14.0),
            s.peak.gamma_multiply(0.7),
        );
    }
}

/// A colour between `a` and `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    lerp(a, b, t)
}

/// The bars as columns of dots, the unlit ones dimly there, the peak a dot
/// in the peak colour; sparks rise from a band that jumps and fade as they
/// age (axross's particle columns).
fn particles(
    painter: &Painter,
    rect: Rect,
    a: &Analyzer,
    s: &Scheme,
    state: &mut VizState,
    rise: &[f32],
    dt: f32,
) {
    let n = a.bars.len().max(1);
    let step = rect.width() / n as f32;
    let size = (step * 0.62).clamp(2.0, 12.0);
    let pitch = (size * 1.5).max(3.0);
    let rows = ((rect.height() - 4.0) / pitch).max(1.0) as usize;
    let unlit = mix(s.background, s.low, 0.18);
    let mut mesh = Mesh::default();
    for (i, (level, peak)) in a.bars.iter().zip(&a.peaks).enumerate() {
        let x = rect.left() + i as f32 * step + (step - size) / 2.0;
        let on = ((level * rows as f32) + 0.5) as usize;
        let high = ((peak * rows as f32) + 0.5) as usize;
        for r in 0..rows {
            let y = rect.bottom() - 2.0 - (r + 1) as f32 * pitch;
            let colour = if r < on {
                s.at(r as f32 / rows as f32)
            } else if high > 0 && r + 1 == high {
                s.peak
            } else {
                unlit
            };
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, y), vec2(size, size)), colour);
        }
    }
    painter.add(Shape::mesh(mesh));
    // the sparks: the alive ones rise and age, new ones where a bar jumped
    let life = 1.2;
    for spark in &mut state.sparks {
        spark[1] += spark[3] * dt;
        spark[2] += dt;
    }
    state.sparks.retain(|k| k[2] < life && k[1] < 1.1);
    let mut born = 0;
    for (i, r) in rise.iter().enumerate() {
        if born >= 6 {
            break;
        }
        if *r >= 0.08 && a.bars[i] >= 0.25 {
            let jitter = state.random() * 0.6 - 0.3;
            let speed = 0.5 * (0.6 + 0.8 * state.random());
            state
                .sparks
                .push([i as f32 + jitter, a.bars[i], 0.0, speed]);
            born += 1;
        }
    }
    let excess = state.sparks.len().saturating_sub(160);
    state.sparks.drain(..excess);
    let dot = (size * 0.7).max(2.0);
    let mut mesh = Mesh::default();
    for [bar, height, age, _] in &state.sparks {
        let x = rect.left() + (bar + 0.5) * step;
        let y = rect.bottom() - 2.0 - height * rows as f32 * pitch;
        let colour = match age / life {
            t if t < 1.0 / 3.0 => s.peak,
            t if t < 2.0 / 3.0 => s.high,
            _ => s.mid,
        };
        mesh.add_colored_rect(Rect::from_center_size(pos2(x, y), vec2(dot, dot)), colour);
    }
    painter.add(Shape::mesh(mesh));
}

/// A wireframe landscape of the spectrum's history receding to the
/// horizon, dots on its front half, the peaks above the front row.
fn terrain(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let rows = 22.min(a.history.len());
    if rows < 2 {
        return;
    }
    let history: Vec<&Vec<f32>> = a.history.iter().skip(a.history.len() - rows).collect();
    let n = a.bars.len().max(1);
    let (w, h) = (rect.width(), rect.height());
    let horizon = h * 0.3;
    let point = |k: usize, i: usize| {
        // the farthest row 6 times as far away as the nearest
        let depth = 6.0 - 5.0 * k as f32 / (rows - 1) as f32;
        let u = (i as f32 + 0.5) / n as f32 - 0.5;
        let x = rect.left() + w / 2.0 + u * w * 1.15 / depth;
        let base = rect.top() + horizon + (h - 4.0 - horizon) / depth;
        let v = history[k].get(i).copied().unwrap_or(0.0);
        (pos2(x, base - v * h * 0.5 / depth), base)
    };
    let grid = mix(s.background, s.low, 0.25);
    for i in (0..n).step_by((n / 24).max(1)) {
        let line: Vec<Pos2> = (0..rows).map(|k| point(k, i).0).collect();
        painter.add(Shape::line(line, Stroke::new(1.0, grid)));
    }
    for k in 0..rows {
        let depth = k as f32 / (rows - 1) as f32;
        let colour = if k == rows - 1 {
            s.peak
        } else {
            mix(s.background, s.at(depth), 0.3 + 0.6 * depth)
        };
        let line: Vec<Pos2> = (0..n).map(|i| point(k, i).0).collect();
        painter.add(Shape::line(line, Stroke::new(1.0, colour)));
    }
    let mut mesh = Mesh::default();
    for (k, row) in history.iter().enumerate().skip(rows / 2) {
        for (i, v) in row.iter().copied().enumerate().take(n) {
            let colour = if v >= 0.65 {
                s.high
            } else if v >= 0.35 {
                s.mid
            } else {
                continue;
            };
            mesh.add_colored_rect(
                Rect::from_center_size(point(k, i).0, vec2(2.5, 2.5)),
                colour,
            );
        }
    }
    for (i, peak) in a.peaks.iter().enumerate() {
        let (p, base) = point(rows - 1, i);
        let y = base - peak * h * 0.5 - 4.0;
        mesh.add_colored_rect(Rect::from_center_size(pos2(p.x, y), vec2(3.5, 3.5)), s.peak);
    }
    painter.add(Shape::mesh(mesh));
}

/// The bars standing on a ring seen at a tilt, the far ones smaller and
/// drawn first; the peaks as ticks (axross's 3D ring).
fn ring(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    const TILT: f32 = 0.3;
    const FAR: f32 = 0.55;
    let n = a.bars.len().max(1);
    let (w, h) = (rect.width(), rect.height());
    let rx = (w * 0.42).min(h * 0.3 / TILT).max(1.0);
    let ry = rx * TILT;
    let c = pos2(rect.center().x, rect.bottom() - ry - (h * 0.06).max(4.0));
    let reach = ((c.y - rect.top()) - ry - 4.0).max(1.0) / FAR;
    let track = mix(s.background, s.low, 0.2);
    painter.add(Shape::ellipse_filled(c, vec2(rx, ry), track));
    painter.add(Shape::ellipse_filled(c, vec2(rx, ry) * 0.55, s.background));
    let feet: Vec<(f32, Pos2, f32)> = (0..n)
        .map(|i| {
            let angle = TAU * i as f32 / n as f32 + PI / 2.0;
            let (sin, cos) = angle.sin_cos();
            let scale = FAR + (1.0 - FAR) * (sin + 1.0) / 2.0;
            (sin, c + vec2(rx * cos, ry * sin), scale)
        })
        .collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|x, y| feet[*x].1.y.total_cmp(&feet[*y].1.y));
    let bar = TAU * rx / n as f32 * 0.5;
    let mut mesh = Mesh::default();
    for i in order {
        let (_, foot, scale) = feet[i];
        let thick = (bar * scale).max(1.0);
        // quiet in front, loud at the back, the far side dimmed
        let colour = mix(
            s.at(1.0 - (2.0 * i as f32 - n as f32).abs() / n as f32),
            s.background,
            (1.0 - scale) / (1.0 - FAR) / 3.0,
        );
        let rise = a.bars[i] * reach * scale;
        if rise >= 1.0 {
            mesh.add_colored_rect(
                Rect::from_min_max(
                    pos2(foot.x - thick / 2.0, foot.y - rise),
                    pos2(foot.x + thick / 2.0, foot.y),
                ),
                colour,
            );
        }
        let top = a.peaks[i] * reach * scale;
        if top >= 1.0 {
            mesh.add_colored_rect(
                Rect::from_min_size(
                    pos2(foot.x - thick / 2.0, foot.y - top - 3.0),
                    vec2(thick, 2.0),
                ),
                s.peak,
            );
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A gaussian bump per group of bars, the louder ones in front, filled
/// from the quiet to the loud colour; the peaks as ticks.
fn bells(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme) {
    let n = a.bars.len().max(1);
    let groups = n.min(32);
    let (w, h) = (rect.width(), rect.height());
    let sigma = w / groups as f32 * 0.5;
    let base = rect.bottom() - 3.0;
    let reach = (h - 8.0).max(1.0);
    let group = |g: usize, of: &[f32]| {
        let (from, to) = (
            g * n / groups,
            ((g + 1) * n / groups).max(g * n / groups + 1),
        );
        of[from..to.min(of.len())]
            .iter()
            .cloned()
            .fold(0.0f32, f32::max)
    };
    let loudest: Vec<f32> = (0..groups).map(|g| group(g, &a.bars)).collect();
    painter.line_segment(
        [pos2(rect.left(), base), pos2(rect.right(), base)],
        Stroke::new(1.0, mix(s.background, s.low, 0.3)),
    );
    let mut order: Vec<usize> = (0..groups).collect();
    order.sort_by(|x, y| loudest[*x].total_cmp(&loudest[*y]));
    const POINTS: usize = 25;
    for g in order {
        let centre = rect.left() + (g as f32 + 0.5) / groups as f32 * w;
        let curve: Vec<Pos2> = (0..POINTS)
            .map(|p| {
                let o = -3.0 + 6.0 * p as f32 / (POINTS - 1) as f32;
                pos2(
                    centre + o * sigma,
                    base - loudest[g] * reach * (-0.5 * o * o).exp(),
                )
            })
            .collect();
        let mut mesh = Mesh::default();
        for pair in curve.windows(2) {
            let i = mesh.vertices.len() as u32;
            let level = |p: Pos2| ((base - p.y) / reach).clamp(0.0, 1.0);
            mesh.colored_vertex(pos2(pair[0].x, base), s.low);
            mesh.colored_vertex(pos2(pair[1].x, base), s.low);
            mesh.colored_vertex(pair[1], s.at(level(pair[1])));
            mesh.colored_vertex(pair[0], s.at(level(pair[0])));
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        painter.add(Shape::mesh(mesh));
        painter.add(Shape::line(curve, Stroke::new(1.0, s.at(loudest[g]))));
    }
    for g in 0..groups {
        let centre = rect.left() + (g as f32 + 0.5) / groups as f32 * w;
        let top = group(g, &a.peaks);
        painter.rect_filled(
            Rect::from_min_size(
                pos2(centre - sigma * 0.8, base - top * reach - 3.0),
                vec2(sigma * 1.6, 2.0),
            ),
            0.0,
            s.peak,
        );
    }
}

/// A ribbon of thin lines that twists across the window, its height
/// following the bands from bass (left) to treble, turned by the beat.
fn ribbon(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme, phase: f32) {
    const LINES: usize = 9;
    const POINTS: usize = 120;
    const TWISTS: f32 = 2.5;
    const WIDTH: f32 = 0.7;
    const STRETCHES: usize = 8;
    let n = a.bars.len().max(1);
    // the bands, smoothed over five
    let smooth: Vec<f32> = (0..n)
        .map(|i| {
            let at = |d: isize| a.bars[(i as isize + d).clamp(0, n as isize - 1) as usize];
            (at(-2) + 2.0 * at(-1) + 3.0 * at(0) + 2.0 * at(1) + at(2)) / 9.0
        })
        .collect();
    let level = |u: f32| {
        let x = u * (n - 1) as f32;
        let i = (x.floor() as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        smooth[i] + (smooth[j] - smooth[i]) * (x - i as f32)
    };
    let lines: Vec<Vec<Pos2>> = (0..LINES)
        .map(|j| {
            let offset = -1.0 + 2.0 * j as f32 / (LINES - 1) as f32;
            (0..POINTS)
                .map(|p| {
                    let u = p as f32 / (POINTS - 1) as f32;
                    let amplitude = 0.2 + 0.8 * level(u).max(0.0).sqrt();
                    let envelope = (PI * u).sin().max(0.0).powf(0.6);
                    let theta = TAU * TWISTS * u + phase;
                    let wave = 0.7 * theta.sin() + 0.3 * (TAU * 1.3 * u - 0.5 * phase).sin();
                    let shape = (wave + WIDTH * offset * theta.cos()) / (1.0 + WIDTH);
                    pos2(
                        rect.left() + u * rect.width(),
                        rect.center().y - rect.height() * 0.46 * envelope * amplitude * shape,
                    )
                })
                .collect()
        })
        .collect();
    let middle = LINES / 2;
    for st in 0..STRETCHES {
        let from = st * (POINTS - 1) / STRETCHES;
        let to = ((st + 1) * (POINTS - 1) / STRETCHES + 1).min(POINTS);
        let colour = s.at((st as f32 + 0.5) / STRETCHES as f32);
        for (j, line) in lines.iter().enumerate() {
            if j != middle {
                painter.add(Shape::line(
                    line[from..to].to_vec(),
                    Stroke::new(1.0, colour),
                ));
            }
        }
    }
    painter.add(Shape::line(lines[middle].clone(), Stroke::new(1.2, s.peak)));
}

/// A petal (bass), orbs (middle) and spikes (treble) in a wedge, turned six
/// times and each copy mirrored - turning with the beat; an orb in the peak
/// colour marks the peaks.
fn kaleidoscope(painter: &Painter, rect: Rect, a: &Analyzer, s: &Scheme, phase: f32) {
    const FOLD: usize = 6;
    let c = rect.center();
    let radius = (rect.width().min(rect.height()) / 2.0 - 2.0).max(1.0);
    let alpha = PI / FOLD as f32;
    let n = a.bars.len().max(1);
    let groups: Vec<f32> = (0..6)
        .map(|g| {
            let (from, to) = (g * n / 6, ((g + 1) * n / 6).max(g * n / 6 + 1).min(n));
            a.bars[from..to].iter().sum::<f32>() / (to - from).max(1) as f32
        })
        .collect();
    let peak = a.peaks.iter().cloned().fold(0.0f32, f32::max);
    let polar = |r: f32, angle: f32| vec2(r * angle.cos(), r * angle.sin());
    // the petal: two cubic curves, sampled
    let length = radius * (0.35 + 0.6 * (groups[0] + groups[1]) / 2.0);
    let cubic = |p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2| {
        (0..12).map(move |k| {
            let t = k as f32 / 12.0;
            let u = 1.0 - t;
            p0 * u * u * u + p1 * 3.0 * u * u * t + p2 * 3.0 * u * t * t + p3 * t * t * t
        })
    };
    let start = polar(radius * 0.06, alpha * 0.5);
    let tip = polar(length, alpha * 0.45);
    let petal: Vec<Vec2> = cubic(
        start,
        polar(length * 0.5, alpha * 1.7),
        polar(length * 0.95, alpha * 1.1),
        tip,
    )
    .chain(cubic(
        tip,
        polar(length * 0.75, -alpha * 0.1),
        polar(length * 0.35, alpha * 0.05),
        start,
    ))
    .collect();
    let centroid = petal.iter().fold(Vec2::ZERO, |acc, p| acc + *p) / petal.len() as f32;
    let hollow: Vec<Vec2> = petal.iter().map(|p| *p * 0.58).collect();
    let turn = phase * 0.5;
    let mut mesh = Mesh::default();
    for k in 0..FOLD {
        for mirrored in [false, true] {
            let angle = turn + k as f32 * TAU / FOLD as f32;
            let (sin, cos) = angle.sin_cos();
            let place = |v: Vec2| {
                let v = if mirrored { vec2(v.x, -v.y) } else { v };
                c + vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
            };
            let mut fan = |points: &[Vec2], centre: Vec2, colour: Color32| {
                let i0 = mesh.vertices.len() as u32;
                mesh.colored_vertex(place(centre), colour);
                for p in points {
                    mesh.colored_vertex(place(*p), colour);
                }
                for e in 0..points.len() as u32 {
                    let next = (e + 1) % points.len() as u32;
                    mesh.add_triangle(i0, i0 + 1 + e, i0 + 1 + next);
                }
            };
            fan(&petal, centroid, s.low);
            fan(&hollow, centroid * 0.58, mix(s.background, s.low, 0.35));
            for (j, level) in groups[2..4].iter().enumerate() {
                let centre = polar(
                    radius * (0.3 + 0.18 * j as f32 + 0.25 * level),
                    alpha * (0.3 + 0.4 * j as f32),
                );
                let size = radius * (0.03 + 0.09 * level);
                let orb: Vec<Vec2> = (0..14)
                    .map(|q| centre + polar(size, TAU * q as f32 / 14.0))
                    .collect();
                fan(&orb, centre, s.mid);
            }
            for (j, level) in groups[4..6].iter().enumerate() {
                let r = radius * (0.55 + 0.1 * j as f32 + 0.3 * level);
                let reach = radius * (0.06 + 0.16 * level);
                let angle = alpha * (0.85 - 0.5 * j as f32);
                let spike = [
                    polar(r - reach, angle),
                    polar(r, angle + alpha * 0.12),
                    polar(r + reach, angle),
                    polar(r, angle - alpha * 0.12),
                ];
                fan(&spike, polar(r, angle), s.high);
            }
            let mark = polar(radius * (0.16 + 0.12 * peak), alpha * 0.5);
            let dot: Vec<Vec2> = (0..10)
                .map(|q| mark + polar(radius * 0.035, TAU * q as f32 / 10.0))
                .collect();
            fan(&dot, mark, s.peak);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn starfield(
    painter: &Painter,
    rect: Rect,
    a: &Analyzer,
    s: &Scheme,
    state: &mut VizState,
    dt: f32,
) {
    while state.stars.len() < 260 {
        let (x, y, z) = (
            state.random() * 2.0 - 1.0,
            state.random() * 2.0 - 1.0,
            state.random(),
        );
        state.stars.push([x, y, z.max(0.05)]);
    }
    let speed = (0.08 + a.energy * 1.2 + a.onset * 0.8) * dt.min(0.1);
    let c = rect.center();
    let scale = rect.width().max(rect.height()) * 0.5;
    for k in 0..state.stars.len() {
        let [x, y, z] = state.stars[k];
        let nz = z - speed;
        if nz <= 0.02 {
            let (rx, ry) = (state.random() * 2.0 - 1.0, state.random() * 2.0 - 1.0);
            state.stars[k] = [rx, ry, 1.0];
            continue;
        }
        let from = c + vec2(x / z, y / z) * scale * 0.25;
        let to = c + vec2(x / nz, y / nz) * scale * 0.25;
        state.stars[k][2] = nz;
        if rect.contains(to) {
            let near = 1.0 - nz;
            painter.line_segment(
                [from, to],
                Stroke::new(
                    0.5 + near * 2.5,
                    s.at(near).gamma_multiply(0.4 + near * 0.6),
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_cycle_and_unknown_keys_fall_back() {
        assert_eq!(next("spectrum"), "mirror");
        assert_eq!(next("starfield"), "spectrum");
        // every face of axross's player is drawn here
        for key in [
            "spectrum",
            "mirror",
            "radial",
            "scope",
            "vu",
            "line",
            "waterfall",
            "polar",
            "lissajous",
            "starfield",
            "led",
            "particles",
            "ribbon",
            "ridges",
            "terrain",
            "ring",
            "kaleidoscope",
            "bells",
        ] {
            assert_eq!(known(key), key);
        }
        assert_eq!(known("hologram"), "spectrum");
        assert_eq!(known("vu"), "vu");
    }

    #[test]
    fn schemes_match_axross() {
        let classic = scheme("classic", &Default::default());
        assert_eq!(classic.low, Color32::from_rgb(0x2e, 0xcc, 0x71));
        assert_eq!(classic.at(0.0), classic.low);
        assert_eq!(classic.at(1.0), classic.high);
        let mut custom = std::collections::BTreeMap::new();
        custom.insert("low".to_string(), "#000000".to_string());
        custom.insert("high".to_string(), "#ffffff".to_string());
        let c = scheme("custom", &custom);
        assert_eq!(c.mid, Color32::from_rgb(128, 128, 128));
    }
}
