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
pub mod flashguard;
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
    /// It changes slowly (the cover): drawn at a low frame rate.
    pub calm: bool,
}

const fn face(key: &'static str, name: &'static str, group: Group) -> Face {
    Face {
        key,
        name,
        group,
        fine: false,
        shader: false,
        calm: false,
    }
}

const fn calm(mut f: Face) -> Face {
    f.calm = true;
    f
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
    calm(face("cover", "Cover", G)),
    shader(face("plasma", "Liquid", G)),
    shader(face("fire", "Fire", G)),
    shader(face("metaballs", "Metaballs", G)),
    shader(face("aurora", "Aurora", G)),
    face("flowfield", "Flow field", G),
    face("synthwave", "Synthwave", G),
    shader(face("tunnel", "Tunnel", G)),
    face("galaxy", "Galaxy", G),
    shader(face("julia", "Julia set", G)),
    shader(face("reaction", "Reaction-diffusion", G)),
    shader(face("milkdrop", "Feedback", G)),
    shader(face("flyover", "Flyover", G)),
    face("skyline", "Skyline", G),
    face("isometric", "Isometric bars", G),
    face("helix", "DNA helix", G),
    face("spirograph", "Spirograph", G),
    face("morph", "Polygon morph", G),
    shader(face("crystals", "Crystals", G)),
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

impl Scheme {
    /// The scheme's colours `k` of the way to its ground (the ground and
    /// the peak as they are): for what fills large areas in the starkest
    /// schemes.
    pub fn towards_ground(&self, k: f32) -> Scheme {
        Scheme {
            background: self.background,
            low: mix(self.low, self.background, k),
            mid: mix(self.mid, self.background, k),
            high: mix(self.high, self.background, k),
            peak: self.peak,
        }
    }

    /// How far the starkest schemes' large fills go towards the ground
    /// (0 in all but white or red on black, black on white).
    pub fn stark(&self) -> f32 {
        ((scheme_calm(self) - 1.0) / 0.6).clamp(0.0, 1.0)
    }

    /// The scheme with no saturated red (see [`unred`]): for the faces
    /// whose lines, many of them moving at once, flashed red in a red one.
    pub fn unred(&self) -> Scheme {
        Scheme {
            background: unred(self.background),
            low: unred(self.low),
            mid: unred(self.mid),
            high: unred(self.high),
            peak: unred(self.peak),
        }
    }
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

const HUES: usize = 24;

/// A picture's colourful hues (RGBA pixels), strongest first (weighed by
/// how colourful and how bright): each its hue bucket, its mean colour and
/// its weight. None for a grey picture (a few colourful pixels are none).
pub fn hues(rgba: &[u8]) -> Option<Vec<(usize, (Color32, f64))>> {
    let mut weight = [0.0f64; HUES];
    let mut sum = [[0.0f64; 3]; HUES];
    let mut all = 0.0f64;
    for px in rgba.as_chunks::<4>().0.iter().step_by(3) {
        let (r, g, b) = (
            px[0] as f64 / 255.0,
            px[1] as f64 / 255.0,
            px[2] as f64 / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        all += 1.0;
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        if sat < 0.25 || max < 0.2 {
            continue;
        }
        let d = max - min;
        let hue = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
        let k = ((hue * HUES as f64) as usize).min(HUES - 1);
        let w = sat * max;
        weight[k] += w;
        for (c, v) in sum[k].iter_mut().zip([r, g, b]) {
            *c += v * w;
        }
    }
    let mut out: Vec<(usize, (Color32, f64))> = (0..HUES)
        .filter(|k| weight[*k] > 0.0)
        .map(|k| {
            let to = |v: f64| (v / weight[k] * 255.0).round().clamp(0.0, 255.0) as u8;
            (
                k,
                (
                    Color32::from_rgb(to(sum[k][0]), to(sum[k][1]), to(sum[k][2])),
                    weight[k],
                ),
            )
        })
        .collect();
    out.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
    (out.first()?.1.1 >= all * 0.02).then_some(out)
}

/// A scheme from a cover's colours (RGBA pixels): its strongest hue the
/// middle, the next distinct ones quiet and loud, a ground of its darkest
/// tint, readable peaks. None for a cover without colour.
pub fn cover_scheme(rgba: &[u8]) -> Option<Scheme> {
    let hues = hues(rgba)?;
    let [_, _, hue] = oklch(hues[0].1.0);
    // a second hue of the cover's, far enough from the first and of some
    // weight, makes the quiet end (two related hues, not one ramp of mauve)
    let main = hues[0].1.1;
    let second = hues.iter().skip(1).find_map(|(_, (c, w))| {
        let [_, chroma, h] = oklch(*c);
        let apart = (h - hue).rem_euclid(360.0).min((hue - h).rem_euclid(360.0));
        (apart >= 45.0 && *w > main * 0.1 && chroma > 0.04).then_some(h)
    });
    Some(match second {
        Some(other) => ramp_between(hue, other),
        None => ramp(hue),
    })
}

/// A scheme with every colour's hue turned by `degrees` (lightness and
/// chroma kept: OKLCH) - each generative face its own neighbour of the
/// cover's colours, not all the same ramp.
pub fn turned(s: &Scheme, degrees: f32) -> Scheme {
    let turn = |c: Color32| {
        let [l, ch, h] = oklch(c);
        from_oklch(l, ch, h + degrees)
    };
    Scheme {
        background: turn(s.background),
        low: turn(s.low),
        mid: turn(s.mid),
        high: turn(s.high),
        peak: turn(s.peak),
    }
}

/// A generative face's turn of the cover's colours (degrees), the same each
/// time for a face.
pub fn face_turn(key: &str) -> f32 {
    const TURNS: [f32; 7] = [-50.0, -30.0, -15.0, 0.0, 15.0, 30.0, 50.0];
    let h = key
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    TURNS[(h % TURNS.len() as u32) as usize]
}

/// As [`ramp`], the quiet end in a second hue: low in `other`, middle in
/// `hue`, loud in `hue` turned a little away from `other`.
pub fn ramp_between(hue: f32, other: f32) -> Scheme {
    // (the turn away from the other hue, the shorter way round)
    let d = (other - hue + 540.0).rem_euclid(360.0) - 180.0;
    let away = if d > 0.0 { -30.0 } else { 30.0 };
    Scheme {
        background: from_oklch(0.17, 0.025, hue),
        low: from_oklch(0.50, 0.14, other),
        mid: from_oklch(0.66, 0.17, hue),
        high: from_oklch(0.80, 0.14, hue + away),
        peak: from_oklch(0.95, 0.04, hue + away),
    }
}

/// A scheme designed round one hue (the cover's): lightness rising evenly
/// from a dark ground to a near-white peak, the hue turning a little along
/// it (quiet a little cooler, loud a little warmer), never less colourful
/// than a floor - made in OKLCH and brought into sRGB by giving up chroma,
/// not by clipping (no muddy browns, no olive greys).
pub fn ramp(hue: f32) -> Scheme {
    // (lightness, chroma, hue turn in degrees)
    let at = |l: f32, c: f32, turn: f32| from_oklch(l, c, hue + turn);
    Scheme {
        background: at(0.17, 0.025, 0.0),
        low: at(0.50, 0.14, -35.0),
        mid: at(0.66, 0.17, 0.0),
        high: at(0.80, 0.14, 30.0),
        peak: at(0.95, 0.04, 30.0),
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// OKLCH of a colour: lightness 0..1, chroma, hue in degrees.
pub fn oklch(c: Color32) -> [f32; 3] {
    let [r, g, b] = [c.r(), c.g(), c.b()].map(|v| srgb_to_linear(v as f32 / 255.0));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let ok_l = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    [
        ok_l,
        a.hypot(bb),
        bb.atan2(a).to_degrees().rem_euclid(360.0),
    ]
}

/// The sRGB colour of an OKLCH one; where it lies outside sRGB, its chroma
/// lowered until it fits (lightness and hue kept).
pub fn from_oklch(l: f32, c: f32, hue: f32) -> Color32 {
    let to_linear = |c: f32| {
        let (a, b) = (c * hue.to_radians().cos(), c * hue.to_radians().sin());
        let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        [
            4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
            -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
            -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
        ]
    };
    let mut chroma = c;
    let mut rgb = to_linear(chroma);
    for _ in 0..24 {
        if rgb.iter().all(|v| (-0.000_1..=1.000_1).contains(v)) {
            break;
        }
        chroma *= 0.92;
        rgb = to_linear(chroma);
    }
    let [r, g, b] = rgb.map(|v| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
}

thread_local! {
    /// While a face draws: how calm its scheme as chosen asks it to be
    /// (before its reds were taken out - a red scheme stays calmed as red).
    static CALM_AS_CHOSEN: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// The calm of scheme `s` as chosen, for faces drawn until this is dropped
/// (then the one before again, however the drawing ends).
struct CalmAsChosen(Option<f32>);

impl CalmAsChosen {
    fn of(s: &Scheme) -> CalmAsChosen {
        CalmAsChosen(CALM_AS_CHOSEN.with(|c| c.replace(Some(scheme_calm_of(s)))))
    }
}

impl Drop for CalmAsChosen {
    fn drop(&mut self) {
        CALM_AS_CHOSEN.with(|c| c.set(self.0));
    }
}

/// How much calmer the pictures move in a scheme of strong contrast or of
/// saturated red (1 to 1.6: what drives them turns that much less often).
/// White on black, or a red that is red, turns a picture's every change
/// into a flash by the strict reading - bars of four turns a second, eight
/// rises and falls; in such a scheme they turn at most two and a half times.
/// While a face draws, at least as calm as the scheme chosen asks.
pub fn scheme_calm(s: &Scheme) -> f32 {
    let chosen = CALM_AS_CHOSEN.with(|c| c.get()).unwrap_or(1.0);
    scheme_calm_of(s).max(chosen)
}

fn scheme_calm_of(s: &Scheme) -> f32 {
    let ground = luminance(s.background);
    // (the colours of the faces' bodies - the peak's thin marks aside)
    let colours = [s.low, s.mid, s.high];
    // (the mean of their contrast with the ground, either way - black on
    // white as white on black - and the share of them whose change to the
    // ground is a red transition: red on black, or black on a red ground;
    // a ramp ending in red - Classic's - is mostly not)
    let contrast = colours
        .iter()
        .map(|c| (luminance(*c) - ground).abs())
        .sum::<f32>()
        / colours.len() as f32;
    let red_of = |c: Color32| {
        let lin = |v: u8| LINEAR[v as usize];
        flashguard::red(lin(c.r()), lin(c.g()), lin(c.b()))
    };
    let red = colours
        .iter()
        .filter(|c| (red_of(**c) - red_of(s.background)).abs() >= 20.0)
        .count() as f32
        / colours.len() as f32;
    let by_contrast = 1.0 + 0.6 * ((contrast - 0.45) / 0.45).clamp(0.0, 1.0);
    let by_red = 1.0 + 0.6 * ((red - 0.34) / 0.66).clamp(0.0, 1.0);
    by_contrast.max(by_red)
}

/// The most of R over R + G + B by either encoding: in linear light (WCAG
/// 2.0's measure) or as the values sent to the screen (gamma-encoded, as a
/// review's reading took it).
pub fn red_share(c: Color32) -> f32 {
    let lin = |v: u8| LINEAR[v as usize];
    let share = |r: f32, g: f32, b: f32| {
        if r + g + b > 0.0 {
            r / (r + g + b)
        } else {
            0.0
        }
    };
    let linear = share(lin(c.r()), lin(c.g()), lin(c.b()));
    let gamma = share(c.r() as f32, c.g() as f32, c.b() as f32);
    linear.max(gamma)
}

/// A ground no flash can be red against: a saturated red one darkened -
/// by WCAG's measure, (R - G - B) x 320 under 16, linear - and turned from
/// red until it is no saturated red by either encoding (R over R + G + B
/// under 0.75). On a pure red ground every dark mark that moves is a red
/// flash (the oscilloscope's trace, the terrain's lines, the morph's: 8
/// to 12 a second); darkened only, to #330000, it was still saturated red
/// as the screen's values have it, and the scope's trace over it swung a
/// field's red 11 times a second by that reading.
pub fn quiet_ground(c: Color32) -> Color32 {
    let red_of = |c: Color32| {
        flashguard::red(
            LINEAR[c.r() as usize],
            LINEAR[c.g() as usize],
            LINEAR[c.b() as usize],
        )
    };
    if red_share(c) < 0.75 {
        return c;
    }
    let mut out = c;
    let mut k = 0.0;
    while red_of(out) >= 16.0 && k < 1.0 {
        k += 0.05;
        out = mix(c, Color32::BLACK, k);
    }
    // (towards a grey as light: the red's hue let go of, not its lightness)
    let v = ((out.r() as u32 + out.g() as u32 + out.b() as u32) / 3) as u8;
    let grey = Color32::from_rgb(v, v, v);
    let dark = out;
    let mut t = 0.0;
    while red_share(out) >= 0.75 && t < 1.0 {
        t += 0.05;
        out = mix(dark, grey, t);
    }
    out
}

pub fn scheme(key: &str, custom: &std::collections::BTreeMap<String, String>) -> Scheme {
    if key == "theme" {
        // (the window's theme as it is now - its picture's ground, its
        // accent, its strongest text: one of the twelve, light or dark;
        // it was Midnight's three colours whatever the theme)
        let t = crate::theme::current();
        let (base, light, text) = (t.sunken, t.accent, t.strong);
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
        // (no saturated red by either encoding: a pure red is taken toward
        // a warm white until it is none - every face, whatever it draws)
        let low = unred(get("low", "#1e90ff"));
        let high = unred(get("high", "#ff1493"));
        let background = quiet_ground(get("background", "#000000"));
        return Scheme {
            background,
            low,
            mid: mix(low, high, 0.5),
            high,
            peak: unred(readable_on(get("peak", "#ffffff"), background, 3.0)),
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
/// An additive colour (premultiplied, alpha 0): it lights what is below.
pub(crate) fn glow(c: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (c.r() as f32 * k) as u8,
        (c.g() as f32 * k) as u8,
        (c.b() as f32 * k) as u8,
        0,
    )
}

/// Relative luminance (WCAG) of a colour.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// What lies between bars of `c` on `ground`: the colour towards the
/// ground just dim enough to be seen and no dimmer - its luminance less
/// than 0.08 below the bar's (bars with dark gaps between them are a
/// regular grating, ITU-R BT.1702's pattern hazard, where their light
/// and dark differ by 0.1 or more).
pub fn between(c: Color32, ground: Color32) -> Color32 {
    // (either way: a dark colour on a light ground is lightened towards it
    // as a light one on a dark ground is darkened - else black bars on a
    // white ground had the white ground between them, a grating)
    let (lc, lg) = (luminance(c), luminance(ground));
    let want = if lc >= lg {
        (lc - 0.075).max(lg)
    } else {
        (lc + 0.075).min(lg)
    };
    towards(c, ground, want)
}

/// A colour towards `ground` until its luminance is no further than 0.08
/// from the ground's (either way): lines so faint form no grating (their
/// light and dark differ by less than 0.1).
pub fn faint(c: Color32, ground: Color32) -> Color32 {
    let (lc, lg) = (luminance(c), luminance(ground));
    if (lc - lg).abs() <= 0.08 {
        return c;
    }
    towards(c, ground, if lc > lg { lg + 0.08 } else { lg - 0.08 })
}

/// `c` mixed towards `ground` until its luminance is `want` (between the
/// two's).
fn towards(c: Color32, ground: Color32, want: f32) -> Color32 {
    let up = luminance(c) >= luminance(ground);
    // (t: how much of `c`; at 1 it is `c`, at 0 the ground)
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..12 {
        let t = (lo + hi) / 2.0;
        let l = luminance(mix(ground, c, t));
        let past = if up { l > want } else { l < want };
        if past {
            hi = t;
        } else {
            lo = t;
        }
    }
    mix(ground, c, lo)
}

/// A colour no longer saturated red (WCAG: R / (R + G + B) of 0.8 or more,
/// linear): towards white until it is not - for what comes and goes on
/// the beat, a red flash being stricter than any other.
pub fn unred(c: Color32) -> Color32 {
    // (by either encoding: linear light, or the screen's values)
    let red = |c: Color32| red_share(c) >= 0.75;
    let mut out = c;
    let mut t = 0.0;
    while red(out) && t < 1.0 {
        t += 0.1;
        out = mix(
            c,
            Color32::from_rgb(255, 236, 214).gamma_multiply(c.a() as f32 / 255.0),
            t,
        );
    }
    out
}

/// A colour no longer saturated red, its relative luminance kept: towards
/// a warm grey as light as it is (not towards white - a scheme's red made
/// lighter moved more light with the music: the line face in Fire, Intense,
/// at 9 by an independent 100 ms reading where its red had been 7).
pub fn unred_level(c: Color32) -> Color32 {
    let red = |c: Color32| red_share(c) >= 0.75;
    if !red(c) {
        return c;
    }
    // (the grey of the same relative luminance: Y linear, made sRGB)
    let y = luminance(c).clamp(0.0, 1.0);
    let v = if y <= 0.003_130_8 {
        y * 12.92
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    };
    let g = (v * 255.0).round().clamp(0.0, 255.0) as u8;
    // (a little warm: the hue of the fire, not ash)
    let grey = Color32::from_rgb(g.saturating_add(10), g, g.saturating_sub(12))
        .gamma_multiply(c.a() as f32 / 255.0);
    let mut out = c;
    let mut t = 0.0;
    while red(out) && t < 1.0 {
        t += 0.05;
        out = mix(c, grey, t);
    }
    out
}

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
        // (faint either way: on a light ground, lines a fifth of the way to
        // black were a grating of octaves over the piano roll)
        faint(mix(self.background, self.peak, 0.22), self.background)
    }

    /// The ground a meter's track lies on.
    pub fn track(&self) -> Color32 {
        faint(mix(self.background, self.peak, 0.08), self.background)
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
        self.gradient_as(rect, s, quiet_y, loud_y, |c| c);
    }

    /// As [`Pen::gradient`], each colour made so first (`between`: what
    /// lies between bars).
    pub fn gradient_as(
        &mut self,
        rect: Rect,
        s: &Scheme,
        quiet_y: f32,
        loud_y: f32,
        f: impl Fn(Color32) -> Color32,
    ) {
        let span = loud_y - quiet_y;
        if span.abs() < 1e-3 {
            return self.rect(rect, f(s.low));
        }
        let t = |y: f32| ((y - quiet_y) / span).clamp(0.0, 1.0);
        let stop = quiet_y + 0.6 * span;
        let (top, bottom) = (rect.top(), rect.bottom());
        if (top - stop) * (bottom - stop) < 0.0 {
            self.vertical(
                Rect::from_x_y_ranges(rect.x_range(), top..=stop),
                f(s.at(t(top))),
                f(s.mid),
            );
            self.vertical(
                Rect::from_x_y_ranges(rect.x_range(), stop..=bottom),
                f(s.mid),
                f(s.at(t(bottom))),
            );
        } else {
            self.vertical(rect, f(s.at(t(top))), f(s.at(t(bottom))));
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

    /// The area between the line in `self.points` and `base` (y, below it),
    /// coloured by height alone: `colour(y)` at every vertex, the area cut
    /// at the heights `levels` (so the colour between two of them is a
    /// straight blend, the same across the picture: no column stands out).
    pub fn fill_by_height(&mut self, base: f32, levels: &[f32], colour: impl Fn(f32) -> Color32) {
        let top = self
            .points
            .iter()
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min);
        // the slabs between the heights, from the top of the line to the base
        let mut cuts = [0.0f32; 34];
        let mut n = 0;
        cuts[n] = top;
        n += 1;
        for l in levels {
            if *l > top && *l < base && n < cuts.len() - 1 {
                cuts[n] = *l;
                n += 1;
            }
        }
        cuts[n] = base;
        n += 1;
        cuts[..n].sort_unstable_by(f32::total_cmp);
        for w in 1..self.points.len() {
            let (p, q) = (self.points[w - 1], self.points[w]);
            let strip = [p, q, pos2(q.x, base), pos2(p.x, base)];
            let (high, low) = (p.y.min(q.y), p.y.max(q.y));
            for k in 1..n {
                let (ya, yb) = (cuts[k - 1], cuts[k]);
                if yb - ya < 1e-4 || yb <= high {
                    continue;
                }
                if ya >= low {
                    // (wholly under the line: a plain band)
                    let i0 = self.mesh.vertices.len() as u32;
                    let (top, bottom) = (colour(ya), colour(yb));
                    self.mesh.colored_vertex(pos2(p.x, ya), top);
                    self.mesh.colored_vertex(pos2(q.x, ya), top);
                    self.mesh.colored_vertex(pos2(q.x, yb), bottom);
                    self.mesh.colored_vertex(pos2(p.x, yb), bottom);
                    self.mesh.add_triangle(i0, i0 + 1, i0 + 2);
                    self.mesh.add_triangle(i0, i0 + 2, i0 + 3);
                    continue;
                }
                let mut poly = [Pos2::ZERO; 8];
                let count = clip_band(&strip, ya, yb, &mut poly);
                if count < 3 {
                    continue;
                }
                let i0 = self.mesh.vertices.len() as u32;
                for v in &poly[..count] {
                    self.mesh.colored_vertex(*v, colour(v.y));
                }
                for t in 1..count as u32 - 1 {
                    self.mesh.add_triangle(i0, i0 + t, i0 + t + 1);
                }
            }
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

/// The part of the convex `shape` between the heights `ya` and `yb` (ya
/// above): its corners into `out`, how many.
fn clip_band(shape: &[Pos2; 4], ya: f32, yb: f32, out: &mut [Pos2; 8]) -> usize {
    let mut a = [Pos2::ZERO; 8];
    let mut n = 0;
    // (Sutherland-Hodgman: below ya, then above yb)
    let clip = |input: &[Pos2], keep: &dyn Fn(Pos2) -> bool, edge: f32, out: &mut [Pos2; 8]| {
        let mut m = 0;
        for i in 0..input.len() {
            let (cur, prev) = (input[i], input[(i + input.len() - 1) % input.len()]);
            let (cin, pin) = (keep(cur), keep(prev));
            if cin != pin && m < 8 {
                let t = (edge - prev.y) / (cur.y - prev.y);
                out[m] = pos2(prev.x + (cur.x - prev.x) * t, edge);
                m += 1;
            }
            if cin && m < 8 {
                out[m] = cur;
                m += 1;
            }
        }
        m
    };
    n += clip(shape, &|p: Pos2| p.y >= ya, ya, &mut a);
    if n < 3 {
        return 0;
    }
    clip(&a[..n], &|p: Pos2| p.y <= yb, yb, out)
}

/// A picture made small (16 x 16, in linear light): what a texture shows,
/// as the flash guard estimates it, and a blurred backdrop.
pub struct Thumb {
    pub n: usize,
    pub rgb: Vec<[f32; 3]>,
}

/// sRGB bytes in linear light.
static LINEAR: std::sync::LazyLock<[f32; 256]> = std::sync::LazyLock::new(|| {
    std::array::from_fn(|i| {
        let v = i as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
});

impl Thumb {
    pub const N: usize = 16;

    /// From an RGBA picture (unmultiplied), each cell its block's mean.
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Thumb {
        let n = Thumb::N;
        let mut sum = vec![[0.0f32; 4]; n * n];
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) * 4;
                let Some(p) = rgba.get(i..i + 4) else {
                    continue;
                };
                let a = p[3] as f32 / 255.0;
                let cell = &mut sum[(y * n / height.max(1)) * n + x * n / width.max(1)];
                for k in 0..3 {
                    cell[k] += LINEAR[p[k] as usize] * a;
                }
                cell[3] += 1.0;
            }
        }
        // (the mean in linear light: a black and white check is a light
        // grey, not a mid one)
        Thumb {
            n,
            rgb: sum
                .iter()
                .map(|c| {
                    let k = c[3].max(1.0);
                    [c[0] / k, c[1] / k, c[2] / k]
                })
                .collect(),
        }
    }

    /// Made smaller still: `m` x `m` cells, each the mean of its block.
    pub fn smaller(&self, m: usize) -> Thumb {
        let n = self.n;
        let mut rgb = vec![[0.0f32; 3]; m * m];
        for y in 0..n {
            for x in 0..n {
                let cell = &mut rgb[(y * m / n) * m + x * m / n];
                for k in 0..3 {
                    cell[k] += self.rgb[y * n + x][k];
                }
            }
        }
        let block = ((n * n) / (m * m)).max(1) as f32;
        for c in &mut rgb {
            for v in c.iter_mut() {
                *v /= block;
            }
        }
        Thumb { n: m, rgb }
    }

    /// As [`Thumb::at`], in sRGB (0..1).
    pub fn at_srgb(&self, u: f32, v: f32) -> [f32; 3] {
        let srgb = |v: f32| {
            let v = v.clamp(0.0, 1.0);
            if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        let c = self.at(u, v);
        [srgb(c[0]), srgb(c[1]), srgb(c[2])]
    }

    /// At (u, v) in 0..1, between the cells (linear).
    pub fn at(&self, u: f32, v: f32) -> [f32; 3] {
        let n = self.n;
        let fx = (u * n as f32 - 0.5).clamp(0.0, (n - 1) as f32);
        let fy = (v * n as f32 - 0.5).clamp(0.0, (n - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let c = |x: usize, y: usize| self.rgb[y * n + x];
        let mut out = [0.0; 3];
        for (k, o) in out.iter_mut().enumerate() {
            let top = c(x0, y0)[k] + (c(x1, y0)[k] - c(x0, y0)[k]) * tx;
            let bottom = c(x0, y1)[k] + (c(x1, y1)[k] - c(x0, y1)[k]) * tx;
            *o = top + (bottom - top) * ty;
        }
        out
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
    /// The face shown, and the one it is fading from (and how far, 0..1).
    shown: &'static str,
    fading: Option<(&'static str, f32)>,
    /// The waterfall smooth, at high resolution (the fine spectrum's), not
    /// in axross's bars.
    pub waterfall_hd: bool,
    /// The starfield and the stereo scope with trails and a glow, not as
    /// axross draws them.
    pub classic_plus: bool,
    /// The flash guard over the picture (off only to measure what it costs).
    pub flash: flashguard::FlashGuard,
    #[doc(hidden)]
    pub flash_off: bool,
    /// The cover, small (the flash guard's estimate of it; the cover face's
    /// backdrop, blurred).
    pub cover_thumb: Option<std::sync::Arc<Thumb>>,
}

/// Seconds a face takes to fade into the next.
pub const CROSSFADE: f32 = 0.6;

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
    /// How opaque the face is (a crossfade), and whether it is the one
    /// being faded from (its shader has a slot of its own).
    pub opacity: f32,
    pub second: bool,
    /// The loudness meter's target (LUFS).
    pub loudness_target: f32,
    /// The cover of what plays (its texture and size), for the cover face.
    pub cover: Option<(egui::TextureId, egui::Vec2)>,
}

/// One frame of face `key`: a change of face fades over CROSSFADE seconds
/// from the one before.
pub fn draw(f: &Frame<'_>, key: &str, state: &mut VizState) {
    // (no saturated red in any face, whatever its scheme - Classic's and
    // Fire's own reds, a cover's, a custom one: every colour of the scheme
    // turned toward a warm grey as light as it is (unred_level) until it is
    // none by either encoding, the ground darkened and turned from red; no
    // red flash can come of them)
    // (calmed as the scheme chosen asks - a red one as red - though its
    // reds are taken out here: faces ask scheme_calm of what they draw with)
    let _calm = CalmAsChosen::of(f.s);
    let level = |c: Color32| unred_level(c);
    let safe = Scheme {
        background: quiet_ground(f.s.background),
        low: level(f.s.low),
        mid: level(f.s.mid),
        high: level(f.s.high),
        peak: level(f.s.peak),
    };
    let f = &Frame { s: &safe, ..*f };
    let ppp = f.painter.pixels_per_point();
    state.canvas.begin(ppp);
    // (the shapes drawn from here on - the ground too - and the shader
    // faces painted: what the flash guard looks at)
    let layer = f.painter.layer_id();
    let first = f
        .painter
        .ctx()
        .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
    f.painter.rect_filled(f.rect, 8.0, f.s.background);
    if f.rect.width() < 8.0 || f.rect.height() < 8.0 {
        return;
    }
    let face = find(key).unwrap_or(&FACES[0]);
    state.gpu.drawn.clear();
    if state.shown != face.key {
        // (to and from "off" too: it went by the crossfade and the guard -
        // switching a face and off every few frames read 15 transitions a
        // second, review 12; back to the face fading out, on from where
        // the fade is, not that face whole again at once)
        if !state.shown.is_empty() {
            state.fading = Some(match state.fading {
                Some((old, t)) if old == face.key => (state.shown, 1.0 - t),
                _ => (state.shown, 0.0),
            });
        }
        state.shown = face.key;
    }
    // (paused: no time passes, the fade completes at once)
    if let Some((_, t)) = state.fading.as_mut() {
        *t += if f.dt > 0.0 { f.dt / CROSSFADE } else { 1.0 };
    }
    if state.fading.is_some_and(|(_, t)| t >= 1.0) {
        state.fading = None;
    }
    if let Some((old, t)) = state.fading {
        // (the old one whole, the new one over it ever more: a blend of
        // the two - the old one fading as well let the ground through, a
        // dip to dark halfway)
        if face.key == "off" {
            // (into "off": the face before fades out over the ground)
            one(f, old, state, 1.0 - t, true);
            one(f, face.key, state, 1.0, false);
        } else {
            one(f, old, state, 1.0, true);
            one(f, face.key, state, t, false);
        }
        // (at the frame rate's cap, as the faces themselves)
        f.painter
            .ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(state.gpu.interval));
    } else {
        one(f, face.key, state, 1.0, false);
    }
    // the flash guard: a veil over whatever would flash too fast
    if !state.flash_off {
        let drawn = std::mem::take(&mut state.gpu.drawn);
        let texture = f.cover.map(|c| c.0).zip(state.cover_thumb.clone());
        state.flash.guard(
            f.painter,
            first,
            f.rect.shrink(4.0),
            &drawn,
            texture.as_ref().map(|(id, t)| (*id, &**t)),
            f.dt,
        );
        state.gpu.drawn = drawn;
    }
}

pub use generative::cover_side;

/// The feedback face's preset now (tests).
pub fn feedback_preset(state: &VizState) -> usize {
    generative::preset(&state.generative)
}

/// The feedback face's looks: how many (tests).
pub const FEEDBACK_LOOKS: usize = generative::LOOKS;

/// Shows the feedback face's look `k` (tests).
pub fn set_feedback_preset(state: &mut VizState, k: usize) {
    generative::set_preset(&mut state.generative, k);
}

/// The faces fading: which is (tests).
pub fn fading(state: &VizState) -> Option<&'static str> {
    state.fading.map(|(k, _)| k)
}

fn one(f: &Frame<'_>, key: &'static str, state: &mut VizState, opacity: f32, second: bool) {
    let Some(face) = find(key) else { return };
    // (a little room within the rounded corners; nothing drawn outside it,
    // so nothing over the corners either)
    let inner = f.rect.shrink(4.0);
    let mut clipped = f
        .painter
        .with_clip_rect(inner.intersect(f.painter.clip_rect()));
    clipped.multiply_opacity(opacity);
    // (the pictures' own motion at the pace the motion setting asks for:
    // calm, slower; axross's faces as axross moves them)
    let dt = if face.group == Group::Generative {
        f.dt * f.a.motion.pace()
    } else {
        f.dt
    };
    let frame = Frame {
        rect: inner,
        painter: &clipped,
        opacity,
        second,
        dt,
        ..*f
    };
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

/// What the analysis must work out for face `key` and the one it fades from.
pub fn needs_now(key: &str, state: &VizState) -> Needs {
    let hd = |k: &str| state.waterfall_hd && k == "waterfall";
    let mut n = needs(key);
    n.fine |= hd(key);
    if let Some((old, _)) = state.fading {
        n.fine |= needs(old).fine || hd(old);
    }
    n
}

#[cfg(test)]
mod tests {

    /// Saturated red by either encoding: R over R + G + B at least 0.8, in
    /// linear light or as the values sent to the screen.
    fn saturated_red_either_way(c: Color32) -> bool {
        let lin = |v: u8| LINEAR[v as usize];
        let (r, g, b) = (lin(c.r()), lin(c.g()), lin(c.b()));
        let linear = r + g + b > 0.0 && r / (r + g + b) >= 0.8;
        let (r, g, b) = (c.r() as f32, c.g() as f32, c.b() as f32);
        let gamma = r + g + b > 0.0 && r / (r + g + b) >= 0.8;
        linear || gamma
    }

    #[test]
    fn a_red_turned_from_red_keeps_its_light() {
        for c in [
            "#e74c3c", "#8b1a00", "#ff6a00", "#ff0000", "#c0392b", "#ff3030",
        ] {
            let before = hex(c);
            let after = unred_level(before);
            assert!(!saturated_red_either_way(after), "{c} -> {after:?}");
            let (a, b) = (luminance(before), luminance(after));
            assert!(
                (a - b).abs() <= 0.03 + 0.1 * a,
                "{c}: luminance {a:.3} -> {b:.3}"
            );
        }
        // (no red: as it is)
        assert_eq!(unred_level(hex("#2ecc71")), hex("#2ecc71"));
    }

    #[test]
    fn a_red_ground_or_colour_is_red_by_neither_reading() {
        for ground in [
            "#ff0000", "#cc0000", "#800000", "#ff2020", "#330000", "#ff0040",
        ] {
            let q = quiet_ground(hex(ground));
            assert!(!saturated_red_either_way(q), "{ground} -> {q:?}");
        }
        // (a ground that is no red is left as it is)
        assert_eq!(quiet_ground(hex("#ffffff")), hex("#ffffff"));
        assert_eq!(quiet_ground(hex("#000000")), hex("#000000"));
        let mut custom = std::collections::BTreeMap::new();
        for (k, v) in [
            ("background", "#000000"),
            ("low", "#ff0000"),
            ("high", "#ff0000"),
            ("peak", "#ff3030"),
        ] {
            custom.insert(k.to_owned(), v.to_owned());
        }
        let s = scheme("custom", &custom);
        for c in [s.low, s.mid, s.high, s.peak, s.background] {
            assert!(!saturated_red_either_way(c), "{c:?} in the pure red scheme");
        }
    }
    use super::*;

    #[test]
    fn faces_cycle_and_unknown_keys_fall_back() {
        assert_eq!(next("spectrum"), "mirror");
        assert_eq!(next("starfield"), "analyzer");
        assert_eq!(next("typography"), "spectrum", "off is never stepped into");
        // (the faces that went: their saved keys show the spectrum)
        assert_eq!(known("ripples"), "spectrum");
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
            31
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

    #[test]
    fn a_scheme_from_a_cover() {
        // a cover mostly deep blue with orange and a little green: a ramp
        // round its blue, rising evenly in lightness, colourful throughout
        let mut rgba = Vec::new();
        for i in 0..4096 {
            let c: [u8; 4] = match i % 10 {
                0..=5 => [20, 40, 160, 255],
                6..=8 => [230, 120, 20, 255],
                _ => [40, 170, 60, 255],
            };
            rgba.extend_from_slice(&c);
        }
        let s = cover_scheme(&rgba).expect("colours");
        assert!(
            s.mid.b() > s.mid.r() && s.mid.b() > s.mid.g(),
            "blue in the middle: {s:?}"
        );
        // turned for a face: the same lightness, another hue
        let t = turned(&s, 40.0);
        for (a, b) in [(s.low, t.low), (s.mid, t.mid), (s.high, t.high)] {
            let ([la, _, ha], [lb, _, hb]) = (oklch(a), oklch(b));
            assert!((la - lb).abs() < 0.03, "lightness kept");
            assert!(
                ((hb - ha).rem_euclid(360.0) - 40.0).abs() < 8.0,
                "hue turned: {ha} {hb}"
            );
        }
        // two hues: the quiet end in the cover's orange, not the blue's
        assert!(
            s.low.r() > s.low.b(),
            "the second hue at the quiet end: {s:?}"
        );
        let lch = [s.background, s.low, s.mid, s.high, s.peak].map(oklch);
        for pair in lch.windows(2) {
            assert!(pair[1][0] > pair[0][0] + 0.1, "lightness rising: {lch:?}");
        }
        for v in &lch[1..4] {
            assert!(v[1] > 0.08, "colourful (not muddy): {lch:?}");
        }
        assert!(contrast(s.peak, s.background) >= 4.5);
        assert!(contrast(s.low, s.background) >= 2.5);
        assert!(contrast(s.mid, s.background) >= 3.0);
        // OKLCH there and back
        for c in [
            Color32::from_rgb(200, 40, 90),
            Color32::from_rgb(10, 120, 60),
        ] {
            let [l, ch, h] = oklch(c);
            let back = from_oklch(l, ch, h);
            assert!(
                (0..3).all(|k| (back[k] as i32 - c[k] as i32).abs() <= 1),
                "{c:?} -> {back:?}"
            );
        }
        // a grey cover has none
        let grey: Vec<u8> = (0..4096).flat_map(|_| [120, 120, 120, 255]).collect();
        assert!(cover_scheme(&grey).is_none());
    }
}

#[cfg(test)]
mod scheme_calm_tests {
    use super::*;

    /// "Follow the theme" is the theme's: its picture's ground, its accent,
    /// in every one of the twelve (review 11: it was Midnight's whatever
    /// the theme).
    #[test]
    fn follow_the_theme_follows_every_theme() {
        let mut grounds = std::collections::BTreeSet::new();
        for t in crate::theme::THEMES.iter() {
            crate::theme::set(t.key);
            let s = scheme("theme", &Default::default());
            assert_eq!(s.background, t.sunken, "{}", t.key);
            assert_eq!(s.mid, t.accent, "{}", t.key);
            grounds.insert(s.background.to_array());
        }
        crate::theme::set("midnight");
        assert!(
            grounds.len() >= 10,
            "{} grounds of twelve themes",
            grounds.len()
        );
    }

    #[test]
    fn strong_and_red_schemes_move_calmer() {
        let custom = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        for key in ["classic", "neon", "mono", "fire", "ice", "theme"] {
            println!(
                "{key}: {:.2}",
                scheme_calm(&scheme(key, &Default::default()))
            );
        }
        let bw = custom(&[
            ("background", "#000000"),
            ("low", "#ffffff"),
            ("high", "#ffffff"),
            ("peak", "#ffffff"),
        ]);
        let red = custom(&[
            ("background", "#000000"),
            ("low", "#ff0000"),
            ("high", "#ff0000"),
            ("peak", "#ff3030"),
        ]);
        assert!(scheme_calm(&scheme("custom", &bw)) > 1.55);
        // (a pure red scheme is no saturated red any more: its colours
        // are turned toward a warm white - see unred)
        let red = scheme("custom", &red);
        assert!(red_share(red.low) < 0.75 && red_share(red.peak) < 0.75);
        assert!(scheme_calm(&scheme("classic", &Default::default())) < 1.3);
    }
    /// Fire's reds are taken out before a face draws, but the face still
    /// moves as calmly as Fire asks (its lighter colours alone ask less:
    /// the ridges read 11 and 12 by a 100 ms reading that way, not 0).
    #[test]
    fn a_scheme_turned_from_red_moves_as_calmly_as_chosen() {
        let fire = scheme("fire", &Default::default());
        let safe = Scheme {
            background: quiet_ground(fire.background),
            low: unred_level(fire.low),
            mid: unred_level(fire.mid),
            high: unred_level(fire.high),
            peak: unred_level(fire.peak),
        };
        let chosen = scheme_calm(&fire);
        assert!(scheme_calm(&safe) < chosen - 0.05, "the reds were its calm");
        {
            let _calm = CalmAsChosen::of(&fire);
            assert_eq!(scheme_calm(&safe), chosen);
            assert_eq!(safe.stark(), fire.stark());
            {
                let _inner = CalmAsChosen::of(&safe);
                assert!(scheme_calm(&safe) < chosen);
            }
            assert_eq!(scheme_calm(&safe), chosen);
        }
        assert!(scheme_calm(&safe) < chosen - 0.05);
    }
}
