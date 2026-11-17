//! The visualizations: axross's eighteen faces under its keys and in its
//! colour schemes ([`classic`]), tools to look at the sound with
//! ([`analysis`]: an analyzer with note names, a loudness meter after EBU
//! R128, phase and stereo width, a goniometer, the track's waveform, a
//! chromagram, a piano roll, a mel spectrogram) and pictures it moves
//! ([`generative`]) - the pixel effects among them drawn by shaders on
//! the GPU ([`gpu`]), on the CPU where there is no wgpu.
//!
//! Every face draws into meshes kept from frame to frame ([`Canvas`]):
//! a frame allocates nothing of its own once they have grown.

pub mod analysis;
pub mod classic;
pub mod generative;
pub mod gpu;

use crate::analyzer::{Analyzer, Needs};
use egui::epaint::tessellator::Path;
use egui::epaint::{Mesh, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Shape, pos2};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Classic,
    Analysis,
    Generative,
    Off,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Classic => "Classic",
            Group::Analysis => "Analysis",
            Group::Generative => "Generative",
            Group::Off => "",
        }
    }
}

/// A face: its key (saved in axross's state), its name, its group, what
/// it needs of the analysis, whether it is drawn by a shader.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub key: &'static str,
    pub name: &'static str,
    pub group: Group,
    pub fine: bool,
    pub shader: bool,
}

const fn face(key: &'static str, name: &'static str, group: Group) -> Face {
    Face {
        key,
        name,
        group,
        fine: false,
        shader: false,
    }
}

const fn fine(mut f: Face) -> Face {
    f.fine = true;
    f
}

const fn shader(mut f: Face) -> Face {
    f.shader = true;
    f
}

use Group::{Analysis as A, Classic as C, Generative as G};

/// Every face, in the order a click steps through them: axross's own
/// first (under its keys), then the analysis tools, then the pictures.
pub const FACES: &[Face] = &[
    face("spectrum", "Spectrum", C),
    face("mirror", "Mirrored spectrum", C),
    face("radial", "Radial spectrum", C),
    face("line", "Line spectrum", C),
    face("led", "LED equalizer", C),
    face("particles", "Particle columns", C),
    face("polar", "Polar spectrum", C),
    face("waterfall", "Waterfall", C),
    face("ridges", "Ridgelines", C),
    face("terrain", "Spectrum terrain", C),
    face("ring", "3D ring", C),
    face("bells", "Bell curves", C),
    face("ribbon", "Wave ribbon", C),
    face("kaleidoscope", "Kaleidoscope", C),
    face("scope", "Oscilloscope", C),
    face("lissajous", "Stereo scope", C),
    face("vu", "VU meter", C),
    face("starfield", "Starfield", C),
    fine(face("analyzer", "Spectrum analyzer", A)),
    face("loudness", "Loudness (EBU R128)", A),
    face("phase", "Phase and width", A),
    face("goniometer", "Goniometer", A),
    face("overview", "Waveform overview", A),
    fine(face("chroma", "Chromagram", A)),
    fine(face("pianoroll", "Piano roll", A)),
    shader(fine(face("mel", "Mel spectrogram", A))),
    shader(face("plasma", "Plasma", G)),
    shader(face("fire", "Fire", G)),
    shader(face("metaballs", "Metaballs", G)),
    shader(face("aurora", "Aurora", G)),
    face("flowfield", "Flow field", G),
    face("synthwave", "Synthwave", G),
    shader(face("tunnel", "Tunnel", G)),
    face("galaxy", "Galaxy", G),
    shader(face("julia", "Julia set", G)),
    shader(face("reaction", "Reaction-diffusion", G)),
    face("arcs", "Arcs", G),
    shader(face("ripples", "Ripples", G)),
    face("skyline", "Skyline", G),
    face("isometric", "Isometric bars", G),
    face("helix", "DNA helix", G),
    face("spirograph", "Spirograph", G),
    face("morph", "Polygon morph", G),
    face("mandala", "Mandala", G),
    shader(face("moire", "Moiré", G)),
    face("beatrings", "Beat rings", G),
    shader(face("smoke", "Smoke", G)),
    face("typography", "Kinetic typography", G),
    // (no picture at all: no frames drawn for it - a laptop's battery)
    face("off", "Off", Group::Off),
];

/// The faces of axross's player (its keys, its order).
pub const AXROSS_FACES: usize = 18;

pub fn find(key: &str) -> Option<&'static Face> {
    FACES.iter().find(|f| f.key == key)
}

/// The key a saved name stands for here (one not known shows the spectrum).
pub fn known(key: &str) -> &'static str {
    find(key).map(|f| f.key).unwrap_or("spectrum")
}

/// The face after `key` (a click on the picture, V): "off" is chosen in
/// the menu only, never stepped into.
pub fn next(key: &str) -> &'static str {
    let i = FACES.iter().position(|f| f.key == key).unwrap_or(0);
    let next = FACES[(i + 1) % FACES.len()].key;
    if next == "off" { FACES[0].key } else { next }
}

/// The face before `key` (Shift+V).
pub fn previous(key: &str) -> &'static str {
    let i = FACES.iter().position(|f| f.key == key).unwrap_or(0);
    let count = FACES.len() - 1; // without "off"
    FACES[(i.min(count - 1) + count - 1) % count].key
}

pub fn name(key: &str) -> &'static str {
    find(key).map(|f| f.name).unwrap_or("Spectrum")
}

pub fn needs(key: &str) -> Needs {
    Needs {
        fine: find(key).is_some_and(|f| f.fine),
    }
}

// -- colours -------------------------------------------------------------------------

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

/// The window's own colours for "Follow the theme": its background, its
/// accent, its text.
const THEME_BASE: &str = "#0d1117";
const THEME_HIGHLIGHT: &str = "#4c9aff";
const THEME_TEXT: &str = "#e6edf3";

/// The custom scheme's colours (axross's CUSTOM_COLOURS): key, name, default.
pub const CUSTOM_COLOURS: [(&str, &str, &str); 4] = [
    ("background", "Background", "#000000"),
    ("low", "Quiet", "#1e90ff"),
    ("high", "Loud", "#ff1493"),
    ("peak", "Peaks", "#ffffff"),
];

/// WCAG 2.x contrast of two colours (1..21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let ch = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `colour` when it has `minimum` contrast on `background`, else the
/// nearest step towards white or black that has (axross's readable_on).
pub fn readable_on(colour: Color32, background: Color32, minimum: f32) -> Color32 {
    if contrast(colour, background) >= minimum {
        return colour;
    }
    for step in 1..=10 {
        for target in [Color32::WHITE, Color32::BLACK] {
            let candidate = mix(colour, target, step as f32 / 10.0);
            if contrast(candidate, background) >= minimum {
                return candidate;
            }
        }
    }
    Color32::WHITE
}

pub fn scheme(key: &str, custom: &std::collections::BTreeMap<String, String>) -> Scheme {
    if key == "theme" {
        let (base, light, text) = (hex(THEME_BASE), hex(THEME_HIGHLIGHT), hex(THEME_TEXT));
        return Scheme {
            background: base,
            low: mix(light, base, 0.45),
            mid: light,
            high: mix(light, text, 0.5),
            peak: readable_on(text, base, 3.0),
        };
    }
    if key == "custom" {
        let get = |k: &str, d: &str| {
            let v = custom.get(k).map(String::as_str).unwrap_or(d);
            let ok =
                v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit());
            hex(if ok { v } else { d })
        };
        let low = get("low", "#1e90ff");
        let high = get("high", "#ff1493");
        let background = get("background", "#000000");
        return Scheme {
            background,
            low,
            mid: mix(low, high, 0.5),
            high,
            peak: readable_on(get("peak", "#ffffff"), background, 3.0),
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

/// A colour a share `t` of the way from `a` to `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

impl Scheme {
    /// The colour of a level 0..1: quiet over middle (at 0.6) to loud,
    /// like the bars' gradient (axross's _level_hex).
    pub fn at(&self, v: f32) -> Color32 {
        let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        if v < 0.6 {
            mix(self.low, self.mid, v / 0.6)
        } else {
            mix(self.mid, self.high, (v - 0.6) / 0.4)
        }
    }

    /// Lines and ticks on the background.
    pub fn grid(&self) -> Color32 {
        mix(self.background, self.peak, 0.22)
    }

    /// The ground a meter's track lies on.
    pub fn track(&self) -> Color32 {
        mix(self.background, self.peak, 0.08)
    }

    /// Labels.
    pub fn text(&self) -> Color32 {
        mix(self.background, self.peak, 0.6)
    }

    /// A trace: the first bar colour that stands out on the background.
    pub fn trace(&self) -> Color32 {
        [self.low, self.mid, self.high]
            .into_iter()
            .find(|c| contrast(*c, self.background) >= 3.0)
            .unwrap_or(self.peak)
    }

    /// The colour at `v` with alpha `a` (premultiplied: additive glow
    /// when `a` is 0).
    pub fn glow(&self, v: f32, a: f32) -> Color32 {
        let c = self.at(v);
        let k = |x: u8| x;
        Color32::from_rgba_premultiplied(k(c.r()), k(c.g()), k(c.b()), (a * 255.0) as u8)
    }
}

// -- drawing -------------------------------------------------------------------------

/// Meshes kept from frame to frame: a face fills one, it goes to egui as a
/// shape, and the next frame takes it back once egui has let go of it -
/// its memory stays.
#[derive(Default)]
pub struct Canvas {
    meshes: Vec<Arc<Mesh>>,
    used: usize,
    pub path: Path,
    pub points: Vec<Pos2>,
    /// A pixel in points (the antialiasing's feather).
    pub feather: f32,
}

impl Canvas {
    fn begin(&mut self, pixels_per_point: f32) {
        self.used = 0;
        self.feather = 1.0 / pixels_per_point.max(0.1);
    }

    /// An empty mesh to draw into; [`Canvas::put`] hands it to the painter.
    pub fn mesh(&mut self) -> &mut Mesh {
        if self.used == self.meshes.len() {
            self.meshes.push(Arc::new(Mesh::default()));
        }
        let slot = &mut self.meshes[self.used];
        if Arc::get_mut(slot).is_none() {
            // (still held: a frame egui has not let go of)
            *slot = Arc::new(Mesh::default());
        }
        let mesh = Arc::get_mut(slot).expect("unique");
        mesh.vertices.clear();
        mesh.indices.clear();
        mesh.texture_id = Default::default();
        mesh
    }

    /// The mesh last asked for goes to the painter (over what is there).
    pub fn put(&mut self, painter: &Painter) {
        let mesh = &self.meshes[self.used];
        if !mesh.indices.is_empty() {
            painter.add(Shape::Mesh(mesh.clone()));
        }
        self.used += 1;
    }

    /// Draw into a mesh and put it.
    pub fn draw(&mut self, painter: &Painter, f: impl FnOnce(&mut Pen<'_>)) {
        let feather = self.feather;
        // (the path and the points live beside the meshes)
        let mut path = std::mem::take(&mut self.path);
        let mut points = std::mem::take(&mut self.points);
        {
            let mesh = self.mesh();
            let mut pen = Pen {
                mesh,
                path: &mut path,
                points: &mut points,
                feather,
            };
            f(&mut pen);
        }
        self.path = path;
        self.points = points;
        self.put(painter);
    }
}

/// What a face draws with: a mesh, a path and points to build lines in.
pub struct Pen<'a> {
    pub mesh: &'a mut Mesh,
    pub path: &'a mut Path,
    pub points: &'a mut Vec<Pos2>,
    pub feather: f32,
}

impl Pen<'_> {
    pub fn rect(&mut self, rect: Rect, colour: Color32) {
        if rect.width() > 0.0 && rect.height() > 0.0 {
            self.mesh.add_colored_rect(rect, colour);
        }
    }

    /// A rect from `top` (colour at its top) to `bottom` (at its bottom).
    pub fn vertical(&mut self, rect: Rect, top: Color32, bottom: Color32) {
        let i = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(rect.left_top(), top);
        self.mesh.colored_vertex(rect.right_top(), top);
        self.mesh.colored_vertex(rect.right_bottom(), bottom);
        self.mesh.colored_vertex(rect.left_bottom(), bottom);
        self.mesh.add_triangle(i, i + 1, i + 2);
        self.mesh.add_triangle(i, i + 2, i + 3);
    }

    /// A rect from `left` (colour at its left) to `right`.
    pub fn horizontal(&mut self, rect: Rect, left: Color32, right: Color32) {
        let i = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(rect.left_top(), left);
        self.mesh.colored_vertex(rect.right_top(), right);
        self.mesh.colored_vertex(rect.right_bottom(), right);
        self.mesh.colored_vertex(rect.left_bottom(), left);
        self.mesh.add_triangle(i, i + 1, i + 2);
        self.mesh.add_triangle(i, i + 2, i + 3);
    }

    /// A rect in the scheme's bar gradient: quiet at `quiet_y`, loud at
    /// `loud_y` (axross's _gradient: stops at 0, 0.6 and 1).
    pub fn gradient(&mut self, rect: Rect, s: &Scheme, quiet_y: f32, loud_y: f32) {
        let span = loud_y - quiet_y;
        if span.abs() < 1e-3 {
            return self.rect(rect, s.low);
        }
        let t = |y: f32| ((y - quiet_y) / span).clamp(0.0, 1.0);
        let stop = quiet_y + 0.6 * span;
        let (top, bottom) = (rect.top(), rect.bottom());
        if (top - stop) * (bottom - stop) < 0.0 {
            self.vertical(
                Rect::from_x_y_ranges(rect.x_range(), top..=stop),
                s.at(t(top)),
                s.mid,
            );
            self.vertical(
                Rect::from_x_y_ranges(rect.x_range(), stop..=bottom),
                s.mid,
                s.at(t(bottom)),
            );
        } else {
            self.vertical(rect, s.at(t(top)), s.at(t(bottom)));
        }
    }

    /// The points in `self.points` as a line (antialiased).
    pub fn line(&mut self, width: f32, colour: Color32) {
        if self.points.len() < 2 {
            return;
        }
        self.path.clear();
        self.path.add_open_points(self.points);
        self.path
            .stroke_open(self.feather, &PathStroke::new(width, colour), self.mesh);
    }

    /// The points in `self.points` as a closed line.
    pub fn outline(&mut self, width: f32, colour: Color32) {
        if self.points.len() < 3 {
            return;
        }
        self.path.clear();
        self.path.add_line_loop(self.points);
        self.path
            .stroke_closed(self.feather, &PathStroke::new(width, colour), self.mesh);
    }

    pub fn segment(&mut self, a: Pos2, b: Pos2, width: f32, colour: Color32) {
        if a == b {
            return;
        }
        self.path.clear();
        self.path.add_line_segment([a, b]);
        self.path
            .stroke_open(self.feather, &PathStroke::new(width, colour), self.mesh);
    }

    /// A filled circle (antialiased).
    pub fn circle(&mut self, centre: Pos2, radius: f32, colour: Color32) {
        if radius <= 0.0 {
            return;
        }
        self.path.clear();
        self.path.add_circle(centre, radius);
        self.path.fill(self.feather, colour, self.mesh);
    }

    pub fn ring(&mut self, centre: Pos2, radius: f32, width: f32, colour: Color32) {
        if radius <= 0.0 {
            return;
        }
        self.path.clear();
        self.path.add_circle(centre, radius);
        self.path
            .stroke_closed(self.feather, &PathStroke::new(width, colour), self.mesh);
    }

    /// A shape seen from `centre` (every edge visible from it: a star, a
    /// petal, a polygon round it) in `self.points`, filled as a fan.
    pub fn fan(&mut self, centre: Pos2, inner: Color32, outer: Color32) {
        let n = self.points.len() as u32;
        if n < 3 {
            return;
        }
        let i0 = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(centre, inner);
        for p in self.points.iter() {
            self.mesh.colored_vertex(*p, outer);
        }
        for e in 0..n {
            self.mesh.add_triangle(i0, i0 + 1 + e, i0 + 1 + (e + 1) % n);
        }
    }

    /// A soft disc: `inner` at the centre fading to `outer` at `radius`
    /// (a glow when they are additive colours).
    pub fn radial(&mut self, centre: Pos2, radius: f32, inner: Color32, outer: Color32) {
        if radius <= 0.0 {
            return;
        }
        const SIDES: usize = 48;
        let i0 = self.mesh.vertices.len() as u32;
        self.mesh.colored_vertex(centre, inner);
        for k in 0..SIDES {
            let a = std::f32::consts::TAU * k as f32 / SIDES as f32;
            self.mesh
                .colored_vertex(centre + egui::vec2(a.cos(), a.sin()) * radius, outer);
        }
        for k in 0..SIDES as u32 {
            self.mesh
                .add_triangle(i0, i0 + 1 + k, i0 + 1 + (k + 1) % SIDES as u32);
        }
    }

    /// The area between the line in `self.points` and `base` (y), each
    /// strip in `fill(y)` at its top and `base_colour` at the base.
    pub fn under(&mut self, base: f32, base_colour: Color32, fill: impl Fn(f32) -> Color32) {
        for w in 1..self.points.len() {
            let (p, q) = (self.points[w - 1], self.points[w]);
            let i = self.mesh.vertices.len() as u32;
            self.mesh.colored_vertex(pos2(p.x, base), base_colour);
            self.mesh.colored_vertex(pos2(q.x, base), base_colour);
            self.mesh.colored_vertex(q, fill(q.y));
            self.mesh.colored_vertex(p, fill(p.y));
            self.mesh.add_triangle(i, i + 1, i + 2);
            self.mesh.add_triangle(i, i + 2, i + 3);
        }
    }

    /// A quad of four corners and their colours.
    pub fn quad(&mut self, p: [Pos2; 4], c: [Color32; 4]) {
        let i = self.mesh.vertices.len() as u32;
        for k in 0..4 {
            self.mesh.colored_vertex(p[k], c[k]);
        }
        self.mesh.add_triangle(i, i + 1, i + 2);
        self.mesh.add_triangle(i, i + 2, i + 3);
    }

    pub fn triangle(&mut self, p: [Pos2; 3], c: Color32) {
        let i = self.mesh.vertices.len() as u32;
        for v in p {
            self.mesh.colored_vertex(v, c);
        }
        self.mesh.add_triangle(i, i + 1, i + 2);
    }
}

/// What a face keeps between frames.
#[derive(Default)]
pub struct VizState {
    pub canvas: Canvas,
    pub classic: classic::State,
    pub analysis: analysis::State,
    pub generative: generative::State,
    pub gpu: gpu::State,
    seed: u32,
}

impl VizState {
    /// A random number 0..1 (an LCG: the same field each time for tests).
    pub fn random(&mut self) -> f32 {
        random(&mut self.seed)
    }

    /// Whether shaders run on the GPU here (else their faces are drawn on
    /// the CPU, coarser).
    pub fn set_gpu(&mut self, on: bool) {
        self.gpu.on = on;
    }
}

pub fn random(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

/// The track's waveform for the overview: (min, max, rms) a column.
#[derive(Clone, Debug, Default)]
pub struct Overview {
    pub columns: Vec<[f32; 3]>,
    /// The share of the track read so far (0..1).
    pub done: f32,
}

/// What a frame of a face is given.
pub struct Frame<'a> {
    pub painter: &'a Painter,
    pub rect: Rect,
    pub a: &'a Analyzer,
    pub s: &'a Scheme,
    pub dt: f32,
    /// What plays (the kinetic typography's words).
    pub title: &'a str,
    /// The track's waveform and where it is (0..1), for the overview.
    pub overview: Option<&'a Overview>,
    pub position: f32,
    pub duration: f32,
    /// The pointer over the picture, if it is.
    pub hover: Option<Pos2>,
}

/// One frame of face `key`.
pub fn draw(f: &Frame<'_>, key: &str, state: &mut VizState) {
    let ppp = f.painter.pixels_per_point();
    state.canvas.begin(ppp);
    f.painter.rect_filled(f.rect, 8.0, f.s.background);
    if f.rect.width() < 8.0 || f.rect.height() < 8.0 {
        return;
    }
    // (a little room within the rounded corners; nothing drawn outside)
    let inner = f.rect.shrink(4.0);
    let clipped = f
        .painter
        .with_clip_rect(f.rect.intersect(f.painter.clip_rect()));
    let frame = Frame {
        rect: inner,
        painter: &clipped,
        ..*f
    };
    let face = find(key).unwrap_or(&FACES[0]);
    match face.group {
        Group::Classic => classic::draw(&frame, face.key, state),
        Group::Analysis => analysis::draw(&frame, face.key, state),
        Group::Generative => generative::draw(&frame, face.key, state),
        Group::Off => {
            f.painter.text(
                f.rect.center(),
                egui::Align2::CENTER_CENTER,
                "Visualization off",
                egui::FontId::proportional(14.0),
                f.s.text(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_cycle_and_unknown_keys_fall_back() {
        assert_eq!(next("spectrum"), "mirror");
        assert_eq!(next("starfield"), "analyzer");
        assert_eq!(next("typography"), "spectrum", "off is never stepped into");
        assert_eq!(previous("spectrum"), "typography");
        assert_eq!(previous("mirror"), "spectrum");
        // every face of axross's player is drawn here, under its key
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
            assert_eq!(find(key).unwrap().group, Group::Classic);
        }
        assert_eq!(
            FACES.iter().filter(|f| f.group == Group::Classic).count(),
            AXROSS_FACES
        );
        assert_eq!(
            FACES
                .iter()
                .filter(|f| matches!(f.group, Group::Analysis | Group::Generative))
                .count(),
            30
        );
        assert_eq!(known("hologram"), "spectrum");
        // keys are unique, groups in order
        let mut keys: Vec<&str> = FACES.iter().map(|f| f.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), FACES.len());
        let groups: Vec<Group> = FACES.iter().map(|f| f.group).collect();
        let mut sorted = groups.clone();
        sorted.dedup();
        assert_eq!(
            sorted,
            [
                Group::Classic,
                Group::Analysis,
                Group::Generative,
                Group::Off
            ]
        );
    }

    #[test]
    fn schemes_match_axross() {
        let classic = scheme("classic", &Default::default());
        assert_eq!(classic.low, Color32::from_rgb(0x2e, 0xcc, 0x71));
        assert_eq!(classic.at(0.0), classic.low);
        assert_eq!(classic.at(0.6), classic.mid);
        assert_eq!(classic.at(1.0), classic.high);
        // axross's derived colours
        assert_eq!(classic.grid(), mix(classic.background, classic.peak, 0.22));
        let mut custom = std::collections::BTreeMap::new();
        custom.insert("low".to_string(), "#000000".to_string());
        custom.insert("high".to_string(), "#ffffff".to_string());
        // a peak too close to the background is made readable
        custom.insert("peak".to_string(), "#050505".to_string());
        custom.insert("background".to_string(), "not a colour".to_string());
        let c = scheme("custom", &custom);
        assert_eq!(c.mid, Color32::from_rgb(128, 128, 128));
        assert_eq!(c.background, Color32::BLACK);
        assert!(contrast(c.peak, c.background) >= 3.0);
        for key in ["classic", "fire", "ice", "neon", "mono", "theme", "custom"] {
            let s = scheme(key, &Default::default());
            assert!(contrast(s.peak, s.background) >= 3.0, "{key}");
            assert!(contrast(s.trace(), s.background) >= 3.0, "{key}");
            assert!(contrast(s.text(), s.background) >= 3.0, "{key}: labels");
        }
        assert!((contrast(Color32::WHITE, Color32::BLACK) - 21.0).abs() < 0.01);
    }
}
