//! The window's themes: its chrome - the ground, the cards, the playlist,
//! the buttons, the sliders, the tabs, the menus, the Mixer and Bluetooth -
//! each a small set of design tokens, not colours picked per widget. The
//! faces' colour schemes are separate (a theme only suggests one): a theme
//! never touches the picture, which paints its own ground, and so never
//! the flash guard's budget.
//!
//! Every theme's text, secondary text, disabled text, accent, states and
//! focus ring are checked against every surface they lie on (WCAG AA: 4.5 : 1
//! for text, 3 : 1 for a control's edge and the focus ring; AAA, 7 : 1, in
//! the high-contrast theme) by the tests below.

use egui::{Color32, CornerRadius, Shadow, Stroke};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A theme: its name, its character and its tokens.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub key: &'static str,
    pub name: &'static str,
    /// A line on its character (the picker says it).
    pub character: &'static str,
    pub dark: bool,
    /// The high-contrast theme: AAA text, a heavy focus ring, no shadows.
    pub high_contrast: bool,
    /// The window's ground (the panels).
    pub ground: Color32,
    /// Below the ground: fields to type in, the track of a meter.
    pub sunken: Color32,
    /// Cards, menus, windows.
    pub surface: Color32,
    /// Controls at rest (buttons, a slider's rail).
    pub raised: Color32,
    /// A control or a row under the pointer.
    pub hover: Color32,
    /// A control pressed, a chosen row.
    pub pressed: Color32,
    /// Body text and icons.
    pub text: Color32,
    /// Titles.
    pub strong: Color32,
    /// Secondary text: what plays' album, the times, the labels.
    pub muted: Color32,
    /// The accent (links, the chosen tab, a slider's fill, the default
    /// device) and on hover and pressed.
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_pressed: Color32,
    /// Lines between things, a card's edge.
    pub border: Color32,
    /// The keyboard focus' ring, and how thick.
    pub focus: Color32,
    pub focus_width: f32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    /// A slider's or a meter's rail (its fill is the accent).
    pub track: Color32,
    /// The ground of selected text and of the chosen row.
    pub selection: Color32,
    /// Below a card or a menu (its colour; none in high contrast).
    pub shadow: Color32,
    /// The corners of controls and cards.
    pub radius: u8,
    /// Room round a button's text (density: the same in all but high
    /// contrast - a theme shown in the picker keeps the chosen one's, so
    /// that its rows stay under the pointer).
    pub padding: [f32; 2],
    /// A face scheme that goes with it (see `viz::SCHEMES`).
    pub faces: &'static str,
    /// What disabled controls are faded to (egui's `disabled_alpha`): as
    /// much as their text keeps 4.5 : 1 (7 : 1).
    pub disabled_alpha: f32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The themes, the first the default.
pub const THEMES: [Theme; 12] = [
    Theme {
        key: "midnight",
        name: "Midnight",
        character: "The night sky over a city: deep blue-black, one clear blue.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x0d1117),
        sunken: rgb(0x090c11),
        surface: rgb(0x161b22),
        raised: rgb(0x21262d),
        hover: rgb(0x2a3039),
        pressed: rgb(0x2f3a4a),
        text: rgb(0xd0d7de),
        strong: rgb(0xf0f3f6),
        muted: rgb(0x9aa4ae),
        accent: rgb(0x58a6ff),
        accent_hover: rgb(0x79b8ff),
        accent_pressed: rgb(0x388bfd),
        border: rgb(0x30363d),
        focus: rgb(0x79b8ff),
        focus_width: 2.0,
        good: rgb(0x56d364),
        warn: rgb(0xe3b341),
        bad: rgb(0xff7b72),
        track: rgb(0x30363d),
        selection: rgb(0x1f3a5f),
        shadow: Color32::from_black_alpha(120),
        radius: 6,
        padding: [10.0, 4.0],
        faces: "classic",
        disabled_alpha: 0.75,
    },
    Theme {
        key: "paper",
        name: "Paper",
        character: "Printed paper: warm white, ink-blue accents, pencil greys.",
        dark: false,
        high_contrast: false,
        ground: rgb(0xf3eee4),
        sunken: rgb(0xe9e2d4),
        surface: rgb(0xfbf8f2),
        raised: rgb(0xe6dece),
        hover: rgb(0xddd3c0),
        pressed: rgb(0xd2c6ae),
        text: rgb(0x2b2620),
        strong: rgb(0x16120d),
        muted: rgb(0x5a5143),
        accent: rgb(0x1d4f91),
        accent_hover: rgb(0x173f75),
        accent_pressed: rgb(0x10305a),
        border: rgb(0xc9bda6),
        focus: rgb(0x1d4f91),
        focus_width: 2.0,
        good: rgb(0x2b6836),
        warn: rgb(0x805100),
        bad: rgb(0xa02f27),
        track: rgb(0xaa9c84),
        selection: rgb(0xd4dff0),
        shadow: Color32::from_black_alpha(40),
        radius: 4,
        padding: [10.0, 4.0],
        faces: "mono",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "frost",
        name: "Frost",
        character: "An arctic morning: slate and snow, a frozen-lake blue.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x2b303b),
        sunken: rgb(0x242832),
        surface: rgb(0x343a47),
        raised: rgb(0x3e4554),
        hover: rgb(0x47505f),
        pressed: rgb(0x4a5466),
        text: rgb(0xe5e9f0),
        strong: rgb(0xf4f6f9),
        muted: rgb(0xc0c8d6),
        accent: rgb(0x8fcfe0),
        accent_hover: rgb(0xa8dbe8),
        accent_pressed: rgb(0x78bfd2),
        border: rgb(0x55607a),
        focus: rgb(0xa8dbe8),
        focus_width: 2.0,
        good: rgb(0xb3d59a),
        warn: rgb(0xf0d39b),
        bad: rgb(0xf3b1b6),
        track: rgb(0x606c82),
        selection: rgb(0x3e5470),
        shadow: Color32::from_black_alpha(80),
        radius: 8,
        padding: [10.0, 4.0],
        faces: "ice",
        disabled_alpha: 0.85,
    },
    Theme {
        key: "dusk",
        name: "Dusk",
        character: "Solarized at sundown: deep teal ground, warm gold and cool cyan.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x002b36),
        sunken: rgb(0x00222b),
        surface: rgb(0x073642),
        raised: rgb(0x0d4251),
        hover: rgb(0x0f4554),
        pressed: rgb(0x124a58),
        text: rgb(0xd5dcd6),
        strong: rgb(0xfdf6e3),
        muted: rgb(0xa9b7b7),
        accent: rgb(0xe0b030),
        accent_hover: rgb(0xecc24f),
        accent_pressed: rgb(0xc99a1c),
        border: rgb(0x2a5a66),
        focus: rgb(0x5fd1c6),
        focus_width: 2.0,
        good: rgb(0xb4c94a),
        warn: rgb(0xf1936a),
        bad: rgb(0xff8c84),
        track: rgb(0x376874),
        selection: rgb(0x124a58),
        shadow: Color32::from_black_alpha(100),
        radius: 5,
        padding: [10.0, 4.0],
        faces: "fire",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "synthwave",
        name: "Night drive",
        character: "A night drive in 1986: deep violet, magenta and a little cyan.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x140b24),
        sunken: rgb(0x0e0719),
        surface: rgb(0x1e1236),
        raised: rgb(0x2a1a4a),
        hover: rgb(0x35225c),
        pressed: rgb(0x412a6e),
        text: rgb(0xe8dcff),
        strong: rgb(0xffffff),
        muted: rgb(0xb3a2d6),
        accent: rgb(0xff5cd9),
        accent_hover: rgb(0xff85e3),
        accent_pressed: rgb(0xe63fc0),
        border: rgb(0x3d2a66),
        focus: rgb(0x3fe6ff),
        focus_width: 2.0,
        good: rgb(0x5cf2a6),
        warn: rgb(0xffd166),
        bad: rgb(0xff7a9a),
        track: rgb(0x4a3577),
        selection: rgb(0x4a2560),
        shadow: Color32::from_rgba_premultiplied(40, 0, 50, 140),
        radius: 10,
        padding: [10.0, 4.0],
        faces: "neon",
        disabled_alpha: 0.75,
    },
    Theme {
        key: "forest",
        name: "Forest",
        character: "Under the canopy: deep greens, moss, an amber lamp.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x0f1a14),
        sunken: rgb(0x0a130e),
        surface: rgb(0x16241c),
        raised: rgb(0x1f3227),
        hover: rgb(0x283f31),
        pressed: rgb(0x2b4635),
        text: rgb(0xd8e6d6),
        strong: rgb(0xf2f8ef),
        muted: rgb(0xa0b49f),
        accent: rgb(0xe6a73d),
        accent_hover: rgb(0xf0bb5c),
        accent_pressed: rgb(0xcc8f2a),
        border: rgb(0x2e4636),
        focus: rgb(0xbfdc6e),
        focus_width: 2.0,
        good: rgb(0x9ad68f),
        warn: rgb(0xe6a73d),
        bad: rgb(0xf29580),
        track: rgb(0x3f5a47),
        selection: rgb(0x2a4733),
        shadow: Color32::from_black_alpha(110),
        radius: 7,
        padding: [10.0, 4.0],
        faces: "classic",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "espresso",
        name: "Espresso",
        character: "A café at closing time: roasted browns, a crema glow.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x1b1410),
        sunken: rgb(0x140f0b),
        surface: rgb(0x251c16),
        raised: rgb(0x33271f),
        hover: rgb(0x403129),
        pressed: rgb(0x4d3b30),
        text: rgb(0xecdfd2),
        strong: rgb(0xfff7ee),
        muted: rgb(0xbba795),
        accent: rgb(0xe8b886),
        accent_hover: rgb(0xf2c99f),
        accent_pressed: rgb(0xd49f6b),
        border: rgb(0x46362b),
        focus: rgb(0xf2c99f),
        focus_width: 2.0,
        good: rgb(0xa8d38f),
        warn: rgb(0xebb352),
        bad: rgb(0xf29384),
        track: rgb(0x5a4637),
        selection: rgb(0x4e3a2b),
        shadow: Color32::from_black_alpha(120),
        radius: 8,
        padding: [10.0, 4.0],
        faces: "fire",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "ocean",
        name: "Ocean",
        character: "Deep water: navy and teal, sea glass catching the light.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x071a24),
        sunken: rgb(0x05131b),
        surface: rgb(0x0c2531),
        raised: rgb(0x12323f),
        hover: rgb(0x183f4e),
        pressed: rgb(0x1a4453),
        text: rgb(0xd3e9ee),
        strong: rgb(0xf0fbfc),
        muted: rgb(0x95b9c2),
        accent: rgb(0x64d8c4),
        accent_hover: rgb(0x8ae4d4),
        accent_pressed: rgb(0x48c2ad),
        border: rgb(0x214a59),
        focus: rgb(0xa3eee2),
        focus_width: 2.0,
        good: rgb(0x80e2a2),
        warn: rgb(0xf2c46d),
        bad: rgb(0xff958c),
        track: rgb(0x2c5c6c),
        selection: rgb(0x154653),
        shadow: Color32::from_black_alpha(120),
        radius: 9,
        padding: [10.0, 4.0],
        faces: "ice",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "sakura",
        name: "Sakura",
        character: "Cherry blossom in spring light: soft pink, plum ink.",
        dark: false,
        high_contrast: false,
        ground: rgb(0xfbf0f3),
        sunken: rgb(0xf3e1e7),
        surface: rgb(0xfffafb),
        raised: rgb(0xf0dbe2),
        hover: rgb(0xe8ccd6),
        pressed: rgb(0xdfbdca),
        text: rgb(0x3b2932),
        strong: rgb(0x23141b),
        muted: rgb(0x644b57),
        accent: rgb(0x8a3563),
        accent_hover: rgb(0x732b52),
        accent_pressed: rgb(0x5d2141),
        border: rgb(0xdcbcc8),
        focus: rgb(0x8a3563),
        focus_width: 2.0,
        good: rgb(0x2b6644),
        warn: rgb(0x824e00),
        bad: rgb(0xa3283a),
        track: rgb(0xc19aa9),
        selection: rgb(0xf0d0dd),
        shadow: Color32::from_rgba_premultiplied(40, 10, 25, 50),
        radius: 12,
        padding: [10.0, 4.0],
        faces: "neon",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "studio",
        name: "Studio",
        character: "A mixing desk: greyscale, one orange signal light.",
        dark: true,
        high_contrast: false,
        ground: rgb(0x121212),
        sunken: rgb(0x0b0b0b),
        surface: rgb(0x1b1b1b),
        raised: rgb(0x262626),
        hover: rgb(0x303030),
        pressed: rgb(0x3a3a3a),
        text: rgb(0xd8d8d8),
        strong: rgb(0xffffff),
        muted: rgb(0xa3a3a3),
        accent: rgb(0xff9f1a),
        accent_hover: rgb(0xffb44d),
        accent_pressed: rgb(0xe68a00),
        border: rgb(0x383838),
        focus: rgb(0xff9f1a),
        focus_width: 2.0,
        good: rgb(0xb8d8b8),
        warn: rgb(0xffcf70),
        bad: rgb(0xff8f85),
        track: rgb(0x474747),
        selection: rgb(0x3d3326),
        shadow: Color32::from_black_alpha(130),
        radius: 3,
        padding: [10.0, 4.0],
        faces: "mono",
        disabled_alpha: 0.75,
    },
    Theme {
        key: "daylight",
        name: "Daylight",
        character: "A clear studio by a window: cool white, graphite, a calm teal.",
        dark: false,
        high_contrast: false,
        ground: rgb(0xf5f7fa),
        sunken: rgb(0xe8ecf1),
        surface: rgb(0xffffff),
        raised: rgb(0xe4e9ef),
        hover: rgb(0xd8dfe7),
        pressed: rgb(0xcbd4de),
        text: rgb(0x1f2933),
        strong: rgb(0x0b1117),
        muted: rgb(0x4f5c69),
        accent: rgb(0x0f6c6c),
        accent_hover: rgb(0x0b5757),
        accent_pressed: rgb(0x084343),
        border: rgb(0xc5ced8),
        focus: rgb(0x0f6e6e),
        focus_width: 2.0,
        good: rgb(0x23683a),
        warn: rgb(0x8a5300),
        bad: rgb(0xb02a37),
        track: rgb(0xadb6c0),
        selection: rgb(0xcfe6e6),
        shadow: Color32::from_black_alpha(35),
        radius: 6,
        padding: [10.0, 4.0],
        faces: "classic",
        disabled_alpha: 0.8,
    },
    Theme {
        key: "contrast",
        name: "High contrast",
        character: "For low vision: black and white, yellow for what is chosen, a heavy focus ring.",
        dark: true,
        high_contrast: true,
        ground: rgb(0x000000),
        sunken: rgb(0x000000),
        surface: rgb(0x000000),
        raised: rgb(0x141414),
        hover: rgb(0x262626),
        pressed: rgb(0x333333),
        text: rgb(0xffffff),
        strong: rgb(0xffffff),
        muted: rgb(0xe6e6e6),
        accent: rgb(0xffe100),
        accent_hover: rgb(0xfff066),
        accent_pressed: rgb(0xffd000),
        border: rgb(0xffffff),
        focus: rgb(0x3df5ff),
        focus_width: 3.0,
        good: rgb(0x8cff8c),
        warn: rgb(0xffe100),
        bad: rgb(0xff9e9e),
        track: rgb(0x7e7e7e),
        selection: rgb(0x00336b),
        shadow: Color32::TRANSPARENT,
        radius: 4,
        padding: [12.0, 5.0],
        faces: "mono",
        disabled_alpha: 0.85,
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// The theme shown now.
pub fn current() -> &'static Theme {
    &THEMES[CURRENT.load(Ordering::Relaxed) % THEMES.len()]
}

/// The theme of that key (the first - Midnight - for one not known).
pub fn find(key: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.key == key).unwrap_or(&THEMES[0])
}

/// Shows the theme of that key from the next frame on (a theme not known:
/// the default).
pub fn set(key: &str) {
    let i = THEMES.iter().position(|t| t.key == key).unwrap_or(0);
    CURRENT.store(i, Ordering::Relaxed);
}

/// Ink on a fill: the theme's titles' colour, its ground or its cards',
/// whichever reads best on it.
pub fn ink_on(fill: Color32) -> Color32 {
    ink_on_in(current(), fill)
}

/// As [`ink_on`], in that theme.
pub fn ink_on_in(t: &Theme, fill: Color32) -> Color32 {
    [t.strong, t.ground, t.surface]
        .into_iter()
        .max_by(|a, b| crate::viz::contrast(*a, fill).total_cmp(&crate::viz::contrast(*b, fill)))
        .unwrap_or(t.strong)
}

/// The least contrast text must keep in this theme.
pub fn text_contrast(t: &Theme) -> f32 {
    if t.high_contrast { 7.0 } else { 4.5 }
}

/// A colour (the cover's) made an accent this theme can carry: lightened
/// on a dark ground, darkened on a light one, until text of it reads on the
/// ground, the cards and the controls (4.5 : 1; 7 : 1 in high contrast),
/// and its pressed and hovered shades with it.
pub fn accent_from(colour: Color32, t: &Theme) -> Color32 {
    let need = text_contrast(t);
    let toward = if t.dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let reads = |c: Color32| {
        [t.ground, t.surface, t.raised]
            .iter()
            .all(|g| crate::viz::contrast(c, *g) >= need)
    };
    let mut c = colour;
    let mut k = 0.0;
    while !reads(c) && k < 1.0 {
        k += 0.05;
        c = crate::viz::mix(colour, toward, k);
    }
    c
}

/// A blend of two colours (`k` of `b`).
pub fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    crate::viz::mix(a, b, k)
}

/// A fill faded as egui fades disabled controls (over `ground`).
pub fn faded(c: Color32, ground: Color32, alpha: f32) -> Color32 {
    mix(ground, c, alpha)
}

/// The fill of the row that plays (the accent laid thinly over the ground).
pub fn playing_row(t: &Theme, accent: Color32) -> Color32 {
    mix(t.ground, accent, if t.high_contrast { 0.18 } else { 0.22 })
}

/// The no-sound banner's ground.
pub fn warn_fill(t: &Theme) -> Color32 {
    mix(t.surface, t.warn, if t.dark { 0.14 } else { 0.12 })
}

/// What is chosen (the tab shown, the playlist's chip, selected text) and
/// a slider's filled part (egui fills it so): the accent over the cards,
/// strong enough to stand out from the slider's rail and the ground (3 : 1),
/// with ink on it that reads (4.5 : 1; 7 : 1 in high contrast) - and that
/// ink.
pub fn selection(t: &Theme, accent: Color32) -> (Color32, Color32) {
    use crate::viz::contrast;
    let need = text_contrast(t);
    let ok = |fill: Color32| {
        contrast(ink_on_in(t, fill), fill) >= need
            && contrast(fill, t.track) >= 3.0
            && contrast(fill, t.ground) >= 3.0
    };
    for k in [0.85, 1.0, 0.7, 0.6, 0.5] {
        let fill = mix(t.surface, accent, k);
        if ok(fill) {
            return (fill, ink_on_in(t, fill));
        }
    }
    // (none of them: the accent itself)
    (accent, ink_on_in(t, accent))
}

/// egui's look for a theme and an accent (the theme's own, or the cover's
/// made to fit it).
pub fn visuals(t: &Theme, accent: Color32) -> egui::Visuals {
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = t.ground;
    v.window_fill = t.surface;
    v.faint_bg_color = t.raised;
    v.extreme_bg_color = t.sunken;
    v.text_edit_bg_color = Some(t.sunken);
    v.code_bg_color = t.raised;
    v.window_stroke = Stroke::new(if t.high_contrast { 2.0 } else { 1.0 }, t.border);
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.bad;
    v.hyperlink_color = accent;
    v.weak_text_color = Some(t.muted);
    v.disabled_alpha = t.disabled_alpha;
    let shadow = |blur: u8, y: i8| Shadow {
        offset: [0, y],
        blur,
        spread: 0,
        color: t.shadow,
    };
    v.window_shadow = shadow(18, 6);
    v.popup_shadow = shadow(10, 4);
    let radius = CornerRadius::same(t.radius);
    v.window_corner_radius = CornerRadius::same(t.radius.saturating_add(4));
    v.menu_corner_radius = CornerRadius::same(t.radius.saturating_add(2));
    // (the chosen: the accent's ground, its text the theme's ink on it)
    let (fill, ink) = selection(t, accent);
    v.selection.bg_fill = fill;
    v.selection.stroke = Stroke::new(1.0, ink);
    let edge = Stroke::new(if t.high_contrast { 1.5 } else { 1.0 }, t.border);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.surface;
    w.noninteractive.weak_bg_fill = t.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    w.inactive.bg_fill = t.track;
    w.inactive.weak_bg_fill = t.raised;
    w.inactive.bg_stroke = if t.high_contrast { edge } else { Stroke::NONE };
    w.inactive.fg_stroke = Stroke::new(1.0, t.text);
    w.hovered.bg_fill = t.hover;
    w.hovered.weak_bg_fill = t.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, mix(t.border, t.text, 0.4));
    w.hovered.fg_stroke = Stroke::new(1.5, t.strong);
    // (pressed, and the keyboard's focus: egui shows both so - the ring)
    w.active.bg_fill = t.pressed;
    w.active.weak_bg_fill = t.pressed;
    w.active.bg_stroke = Stroke::new(t.focus_width, t.focus);
    w.active.fg_stroke = Stroke::new(2.0, t.strong);
    w.open.bg_fill = t.raised;
    w.open.weak_bg_fill = t.raised;
    w.open.bg_stroke = Stroke::new(1.0, t.border);
    w.open.fg_stroke = Stroke::new(1.0, t.strong);
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = radius;
    }
    v.text_cursor.stroke = Stroke::new(2.0, t.strong);
    v.slider_trailing_fill = true;
    // (no lines down the side of what folds open)
    v.indent_has_left_vline = false;
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viz::contrast;

    /// Every pair of text and what it lies on, by every theme.
    fn pairs(t: &Theme) -> Vec<(&'static str, Color32, &'static str, Color32, f32)> {
        let text = text_contrast(t);
        let edge = if t.high_contrast { 4.5 } else { 3.0 };
        let v = visuals(t, t.accent);
        let grounds = [
            ("ground", t.ground),
            ("sunken", t.sunken),
            ("surface", t.surface),
            ("raised", t.raised),
            ("hover", t.hover),
            ("pressed", t.pressed),
            ("selection", t.selection),
            ("playing row", playing_row(t, t.accent)),
        ];
        let mut out = Vec::new();
        for (g, gc) in grounds {
            for (name, c) in [("text", t.text), ("strong", t.strong), ("muted", t.muted)] {
                out.push((name, c, g, gc, text));
            }
        }
        for (g, gc) in [
            ("ground", t.ground),
            ("surface", t.surface),
            ("raised", t.raised),
            ("hover", t.hover),
        ] {
            // the accent as text and links; good, warning and error
            for (name, c) in [
                ("accent", t.accent),
                ("good", t.good),
                ("warn", t.warn),
                ("bad", t.bad),
            ] {
                out.push((name, c, g, gc, text));
            }
            // disabled text (egui fades the control over its ground)
            out.push((
                "disabled text",
                faded(t.text, gc, t.disabled_alpha),
                g,
                gc,
                text,
            ));
            // the focus ring, a slider's fill against its rail's ground
            out.push(("focus ring", t.focus, g, gc, edge));
        }
        out.push(("warn", t.warn, "no-sound banner", warn_fill(t), text));
        out.push(("text", t.text, "no-sound banner", warn_fill(t), text));
        let (fill, _) = selection(t, t.accent);
        out.push(("slider fill", fill, "slider rail", t.track, 3.0));
        out.push(("slider fill", fill, "ground", t.ground, 3.0));
        out.push((
            "slider rail",
            t.track,
            "ground",
            t.ground,
            if t.high_contrast { 3.0 } else { 1.5 },
        ));
        for (g, gc) in [
            ("accent", t.accent),
            ("accent hover", t.accent_hover),
            ("accent pressed", t.accent_pressed),
        ] {
            out.push(("ink on accent", ink_on_in(t, gc), g, gc, text));
        }
        out.push((
            "selected text",
            v.selection.stroke.color,
            "selection fill",
            v.selection.bg_fill,
            text,
        ));
        out
    }

    #[test]
    fn every_theme_reads() {
        let mut failures = Vec::new();
        for t in &THEMES {
            for (what, c, on, g, need) in pairs(t) {
                let k = contrast(c, g);
                if k < need {
                    failures.push(format!("{}: {what} on {on}: {k:.2} < {need}", t.name));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn the_themes_are_many_and_alike_in_nothing_but_their_tokens() {
        assert!(THEMES.len() >= 10);
        assert!(THEMES.iter().filter(|t| !t.dark).count() >= 2);
        assert!(THEMES.iter().any(|t| t.high_contrast));
        let mut keys: Vec<_> = THEMES.iter().map(|t| t.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), THEMES.len());
        // each suggests a face scheme there is
        for t in &THEMES {
            assert!(
                crate::viz::SCHEMES.iter().any(|(k, _, _)| *k == t.faces),
                "{}: {}",
                t.key,
                t.faces
            );
        }
    }

    #[test]
    fn a_cover_accent_is_made_to_read_in_every_theme() {
        // a dark blue cover, a pale yellow one, a mid grey, pure red
        for raw in [
            rgb(0x10205a),
            rgb(0xf6f0a0),
            rgb(0x808080),
            rgb(0xff0000),
            rgb(0x2a7f3f),
        ] {
            for t in &THEMES {
                let a = accent_from(raw, t);
                for g in [t.ground, t.surface, t.raised] {
                    assert!(
                        contrast(a, g) >= text_contrast(t),
                        "{}: {raw:?} -> {a:?}: {:.2}",
                        t.name,
                        contrast(a, g)
                    );
                }
                assert!(
                    contrast(ink_on_in(t, a), a) >= 4.5,
                    "{}: ink on {a:?}",
                    t.name
                );
            }
        }
    }
}
