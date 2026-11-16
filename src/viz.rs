//! The visualizations, drawn with egui's painter (on the GPU) from the
//! analyzer: the faces of axross's player under the same keys, in its colour
//! schemes. A click on the picture shows the next one.

use crate::analyzer::Analyzer;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, epaint::Mesh, pos2, vec2};
use std::f32::consts::{PI, TAU};

/// The faces, in the order a click steps through them: (key, name).
pub const MODES: [(&str, &str); 12] = [
    ("spectrum", "Spectrum"),
    ("mirror", "Mirrored spectrum"),
    ("radial", "Radial spectrum"),
    ("line", "Line spectrum"),
    ("led", "LED equalizer"),
    ("polar", "Polar spectrum"),
    ("waterfall", "Waterfall"),
    ("ridges", "Ridgelines"),
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

/// What a face keeps between frames (the starfield's stars).
#[derive(Default)]
pub struct VizState {
    stars: Vec<[f32; 3]>,
    seed: u32,
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
    match mode {
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
        assert_eq!(known("kaleidoscope"), "spectrum");
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
