//! The window's designs: how it is built, not what colours it has (that is
//! the theme's, and any design goes with any theme). A design is data - its
//! corners, its strokes, its type, its density, how its panels are drawn,
//! what it adds (an LCD readout, tape reels, VU needles, a volume knob) -
//! and a few painters in this file; the window is the same window in all
//! of them, every control where it can be reached, by pointer and by key.
//!
//! What moves in a design (the reels, the needles) moves calmly: slowly,
//! following the guarded sound, still where the desktop asks for reduced
//! motion; `tests/designs.rs` puts each through the flash meter.

use crate::theme::Theme;
use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use std::cell::Cell;

/// How a design's panels (the now-playing card, the playlist) are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// A rounded card on the ground.
    Card,
    /// Brushed metal: fine horizontal grain, screws at the corners.
    Brushed,
    /// Raised: a light edge above and left, a dark one below and right.
    Bevel,
    /// Glass: translucent over a blurred cover, a highlight along the top.
    Glass,
    /// Soft: extruded from the ground by a light and a dark shadow.
    Soft,
    /// Lines of box-drawing characters round it.
    Frame,
    /// A thick black frame, no corners, no shadow.
    Slab,
    /// Paper cut out and taped on, a little askew.
    Paper,
    /// Nothing: the content on the ground.
    Bare,
}

/// What a design adds to the now-playing card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extras {
    /// The time and the track's number as a segment display.
    pub lcd: bool,
    /// A cassette's window: two reels turning slowly while it plays.
    pub reels: bool,
    /// Two analogue meters, their needles following the sound.
    pub needles: bool,
    /// The position as a bar of characters: [#####-----].
    pub ascii: bool,
    /// The volume as a rotary knob (in the row of buttons).
    pub knob: bool,
    /// The transport as a deck's piano keys: square, tall, the playing
    /// key pressed in.
    pub keys: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Design {
    pub key: &'static str,
    pub name: &'static str,
    pub character: &'static str,
    /// The corners (None: the theme's).
    pub radius: Option<u8>,
    /// The controls' outline (0: none).
    pub stroke: f32,
    pub shadows: bool,
    /// Room round a button's words, and between things.
    pub padding: [f32; 2],
    pub spacing: [f32; 2],
    /// Monospace for everything (the terminal).
    pub mono: bool,
    /// The type's size (body) and the titles'.
    pub text: f32,
    pub title: f32,
    pub panel: Panel,
    pub extras: Extras,
    /// The column at most so wide (points; the minimal design's one
    /// column); 0: the window's.
    pub column: f32,
    /// The row of buttons faint until the pointer or the keys come to it.
    pub fade_controls: bool,
    /// A theme that goes with it (never forced).
    pub suggests: &'static str,
}

const NONE: Extras = Extras {
    lcd: false,
    reels: false,
    needles: false,
    ascii: false,
    knob: false,
    keys: false,
};

/// The designs, the first the default.
pub const DESIGNS: [Design; 10] = [
    Design {
        key: "modern",
        name: "Modern",
        character: "Today's: cards, round corners, the cover's colour.",
        radius: None,
        stroke: 0.0,
        shadows: true,
        padding: [10.0, 4.0],
        spacing: [8.0, 6.0],
        mono: false,
        text: 14.0,
        title: 22.0,
        panel: Panel::Card,
        extras: NONE,
        column: 0.0,
        fade_controls: false,
        suggests: "midnight",
    },
    Design {
        key: "zen",
        name: "Zen",
        character: "One quiet column, much room, the controls coming when wanted.",
        radius: Some(14),
        stroke: 0.0,
        shadows: false,
        padding: [12.0, 6.0],
        spacing: [12.0, 10.0],
        mono: false,
        text: 15.0,
        title: 26.0,
        panel: Panel::Bare,
        extras: NONE,
        column: 720.0,
        fade_controls: true,
        suggests: "paper",
    },
    Design {
        key: "rack",
        name: "Hi-fi rack",
        character: "A component of 1979: brushed aluminium, an LCD, VU needles, a knob.",
        radius: Some(3),
        stroke: 1.0,
        shadows: true,
        padding: [10.0, 4.0],
        spacing: [8.0, 6.0],
        mono: false,
        text: 13.0,
        title: 20.0,
        panel: Panel::Brushed,
        extras: Extras {
            lcd: true,
            needles: true,
            knob: true,
            ..NONE
        },
        column: 0.0,
        fade_controls: false,
        suggests: "studio",
    },
    Design {
        key: "desktop90",
        name: "90s desktop",
        character: "A compact bevelled strip, small type, the playlist in a panel of its own.",
        radius: Some(0),
        stroke: 1.0,
        shadows: false,
        padding: [6.0, 2.0],
        spacing: [4.0, 3.0],
        mono: true,
        text: 12.0,
        title: 15.0,
        panel: Panel::Bevel,
        extras: Extras { lcd: true, ..NONE },
        column: 0.0,
        fade_controls: false,
        suggests: "frost",
    },
    Design {
        key: "cassette",
        name: "Cassette deck",
        character: "A tape window with turning reels, piano-key buttons, a counter.",
        radius: Some(2),
        stroke: 1.0,
        shadows: true,
        padding: [12.0, 8.0],
        spacing: [6.0, 6.0],
        mono: false,
        text: 13.0,
        title: 19.0,
        panel: Panel::Bevel,
        extras: Extras {
            reels: true,
            lcd: true,
            keys: true,
            ..NONE
        },
        column: 0.0,
        fade_controls: false,
        suggests: "espresso",
    },
    Design {
        key: "terminal",
        name: "Terminal",
        character: "Monospace, box-drawing frames, bars of characters; the keys first.",
        radius: Some(0),
        stroke: 1.0,
        shadows: false,
        padding: [8.0, 2.0],
        spacing: [8.0, 4.0],
        mono: true,
        text: 13.0,
        title: 16.0,
        panel: Panel::Frame,
        extras: Extras {
            ascii: true,
            ..NONE
        },
        column: 0.0,
        fade_controls: false,
        suggests: "contrast",
    },
    Design {
        key: "brutalist",
        name: "Brutalist",
        character: "Raw: thick black frames, big type, a hard grid, no curve, no shadow.",
        radius: Some(0),
        stroke: 3.0,
        shadows: false,
        padding: [14.0, 6.0],
        spacing: [10.0, 8.0],
        mono: false,
        text: 16.0,
        title: 34.0,
        panel: Panel::Slab,
        extras: NONE,
        column: 0.0,
        fade_controls: false,
        suggests: "daylight",
    },
    Design {
        key: "glass",
        name: "Glass",
        character: "Panes of frosted glass over the cover, soft highlights.",
        radius: Some(16),
        stroke: 1.0,
        shadows: true,
        padding: [12.0, 5.0],
        spacing: [8.0, 6.0],
        mono: false,
        text: 14.0,
        title: 22.0,
        panel: Panel::Glass,
        extras: NONE,
        column: 0.0,
        fade_controls: false,
        suggests: "ocean",
    },
    Design {
        key: "soft",
        name: "Soft",
        character: "Neumorphic: everything pressed out of one soft material.",
        radius: Some(18),
        stroke: 0.0,
        shadows: true,
        padding: [14.0, 7.0],
        spacing: [10.0, 8.0],
        mono: false,
        text: 14.0,
        title: 22.0,
        panel: Panel::Soft,
        extras: NONE,
        column: 0.0,
        fade_controls: false,
        suggests: "sakura",
    },
    Design {
        key: "collage",
        name: "Collage",
        character: "A zine: paper cut out, taped askew, type of all sorts - every control still in reach.",
        radius: Some(2),
        stroke: 2.0,
        shadows: true,
        padding: [10.0, 4.0],
        spacing: [8.0, 6.0],
        mono: false,
        text: 14.0,
        title: 28.0,
        panel: Panel::Paper,
        extras: NONE,
        column: 0.0,
        fade_controls: false,
        suggests: "synthwave",
    },
];

thread_local! {
    /// The design shown now (the window's thread's own, as the theme).
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

/// The design shown now.
pub fn current() -> &'static Design {
    &DESIGNS[CURRENT.with(Cell::get) % DESIGNS.len()]
}

/// The design of that key (the first - Modern - for one not known).
pub fn find(key: &str) -> &'static Design {
    DESIGNS.iter().find(|d| d.key == key).unwrap_or(&DESIGNS[0])
}

/// Shows the design of that key from the next frame on.
pub fn set(key: &str) {
    let i = DESIGNS.iter().position(|d| d.key == key).unwrap_or(0);
    CURRENT.with(|c| c.set(i));
}

/// egui's look as the design changes it (over the theme's).
pub fn restyle(v: &mut egui::Visuals, d: &Design, t: &Theme) {
    if let Some(r) = d.radius {
        let radius = CornerRadius::same(r);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        v.window_corner_radius = CornerRadius::same(r.saturating_add(2));
        v.menu_corner_radius = CornerRadius::same(r.saturating_add(1));
    }
    if d.stroke > 0.0 {
        // (an outline the theme's text colour, faint, or the border's; the
        // brutalist's in the text's own colour)
        let edge = if d.stroke >= 3.0 {
            t.strong
        } else {
            crate::theme::mix(t.border, t.text, 0.35)
        };
        v.widgets.inactive.bg_stroke = Stroke::new(d.stroke, edge);
        v.widgets.hovered.bg_stroke = Stroke::new(d.stroke, t.strong);
        v.widgets.open.bg_stroke = Stroke::new(d.stroke, edge);
        v.window_stroke = Stroke::new(d.stroke, edge);
    }
    if !d.shadows {
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow::NONE;
    }
}

/// The type and the room as the design has them.
pub fn retype(style: &mut egui::Style, d: &Design) {
    use egui::{FontFamily, FontId, TextStyle};
    let family = if d.mono {
        FontFamily::Monospace
    } else {
        FontFamily::Proportional
    };
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(d.text, family.clone()));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::new(d.text, family.clone()));
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new((d.text * 0.8).max(11.0), family.clone()),
    );
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::new(d.title, family));
    style.spacing.button_padding = vec2(d.padding[0], d.padding[1]);
    style.spacing.item_spacing = vec2(d.spacing[0], d.spacing[1]);
    // (no control lower than 24: a hit target in every design, WCAG 2.2)
    style.spacing.interact_size.y = style.spacing.interact_size.y.max(24.0);
}

/// The now-playing card's fill in a design (the theme's card tint as it
/// shows through the design's panel): what its words must read on.
pub fn card_fill(d: &Design, t: &Theme, tint: Color32) -> Color32 {
    use crate::theme::mix;
    match d.panel {
        Panel::Card | Panel::Paper => tint,
        Panel::Brushed => mix(tint, t.raised, 0.5),
        Panel::Bevel => t.raised,
        // (glass: the card a little lighter or darker than the ground it
        // lies on, as frosted glass is)
        // (smoked on a dark ground, frosted on a light one: darker or
        // lighter than the tint, so its words read at least as well)
        Panel::Glass => mix(tint, if t.dark { t.ground } else { t.surface }, 0.35),
        Panel::Soft | Panel::Bare | Panel::Frame => t.ground,
        Panel::Slab => t.surface,
    }
}

/// The glass design's ground: the cover made a haze - 8 x 8 colours, the
/// texture's smoothing blurs them across the window - each taken toward the
/// ground until every word and the accent still reads on it as on the
/// ground itself.
pub fn haze(
    rgba: &[u8],
    width: usize,
    height: usize,
    t: &Theme,
    accent: Color32,
) -> egui::ColorImage {
    const N: usize = 8;
    let need = crate::theme::text_contrast(t);
    let reads = |c: Color32| {
        [t.text, t.muted, t.strong, accent].iter().all(|ink| {
            crate::viz::contrast(*ink, c) >= need.min(crate::viz::contrast(*ink, t.ground))
        })
    };
    let mut pixels = vec![t.ground; N * N];
    if width > 0 && height > 0 && rgba.len() >= width * height * 4 {
        for (i, px) in pixels.iter_mut().enumerate() {
            let (cx, cy) = (i % N, i / N);
            let (x0, x1) = (
                cx * width / N,
                ((cx + 1) * width / N).max(cx * width / N + 1),
            );
            let (y0, y1) = (
                cy * height / N,
                ((cy + 1) * height / N).max(cy * height / N + 1),
            );
            let mut sum = [0u32; 3];
            let mut n = 0u32;
            for y in y0..y1.min(height) {
                for x in x0..x1.min(width) {
                    let k = (y * width + x) * 4;
                    for c in 0..3 {
                        sum[c] += rgba[k + c] as u32;
                    }
                    n += 1;
                }
            }
            let n = n.max(1);
            let raw = Color32::from_rgb((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8);
            // (a tint, never the picture: at least 55 % ground)
            let mut k = 0.55;
            let mut c = crate::theme::mix(raw, t.ground, k);
            while !reads(c) && k < 1.0 {
                k = (k + 0.05).min(1.0);
                c = crate::theme::mix(raw, t.ground, k);
            }
            *px = c;
        }
    }
    egui::ColorImage::new([N, N], pixels)
}

/// What the playlist lies on: the ground where the design leaves it bare
/// (the modern card, zen), else the design's panel in the theme's colours.
pub fn list_fill(d: &Design, t: &Theme) -> Color32 {
    match d.panel {
        Panel::Card | Panel::Bare => t.ground,
        Panel::Glass => crate::theme::mix(t.surface, t.ground, 0.35),
        _ => card_fill(d, t, t.surface),
    }
}

/// The room between the playlist's panel and its rows (none where there
/// is no panel).
pub fn list_margin(d: &Design) -> f32 {
    match d.panel {
        Panel::Card | Panel::Bare => 0.0,
        Panel::Frame => 12.0,
        _ => 8.0,
    }
}

/// A panel drawn as the design draws it: under its content, `rect` the
/// panel's (the content is laid over it).
pub fn panel(p: &Painter, rect: Rect, d: &Design, t: &Theme, fill: Color32, seed: u32) {
    let r = CornerRadius::same(d.radius.unwrap_or(t.radius.saturating_add(6)));
    match d.panel {
        Panel::Card => {
            p.rect_filled(rect, r, fill);
        }
        Panel::Bare => {}
        Panel::Brushed => {
            p.rect_filled(rect, r, fill);
            // the grain: fine lines, a shade either way of the metal -
            // still (no motion), and far under 0.1 of luminance apart
            let light = crate::theme::mix(fill, Color32::WHITE, 0.035);
            let dark = crate::theme::mix(fill, Color32::BLACK, 0.035);
            let mut y = rect.top() + 2.0;
            let mut k = seed;
            while y < rect.bottom() - 1.0 {
                k = k.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let c = if k >> 16 & 1 == 0 { light } else { dark };
                p.line_segment(
                    [pos2(rect.left() + 2.0, y), pos2(rect.right() - 2.0, y)],
                    Stroke::new(1.0, c),
                );
                y += 2.0 + ((k >> 20) & 1) as f32;
            }
            // screws at the corners
            for (x, y) in [
                (rect.left() + 8.0, rect.top() + 8.0),
                (rect.right() - 8.0, rect.top() + 8.0),
                (rect.left() + 8.0, rect.bottom() - 8.0),
                (rect.right() - 8.0, rect.bottom() - 8.0),
            ] {
                p.circle_filled(pos2(x, y), 3.0, crate::theme::mix(fill, t.border, 0.8));
                p.line_segment(
                    [pos2(x - 2.0, y - 2.0), pos2(x + 2.0, y + 2.0)],
                    Stroke::new(1.0, crate::theme::mix(fill, Color32::BLACK, 0.3)),
                );
            }
            p.rect_stroke(
                rect,
                r,
                Stroke::new(1.0, t.border),
                egui::StrokeKind::Inside,
            );
        }
        Panel::Bevel => {
            p.rect_filled(rect, r, fill);
            let light = crate::theme::mix(fill, Color32::WHITE, 0.35);
            let dark = crate::theme::mix(fill, Color32::BLACK, 0.45);
            let (l, tp, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
            for (i, w) in [(0.5f32, 1.0f32), (1.5, 1.0)] {
                let (l, tp, rt, b) = (l + i, tp + i, rt - i, b - i);
                p.line_segment([pos2(l, b), pos2(l, tp)], Stroke::new(w, light));
                p.line_segment([pos2(l, tp), pos2(rt, tp)], Stroke::new(w, light));
                p.line_segment([pos2(rt, tp), pos2(rt, b)], Stroke::new(w, dark));
                p.line_segment([pos2(rt, b), pos2(l, b)], Stroke::new(w, dark));
            }
        }
        Panel::Glass => {
            // frosted: the pane, a sheen down from its top edge, a bright
            // rim above and a faint one round it
            p.rect_filled(rect, r, fill);
            let sheen = Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() * 0.45));
            let mut mesh = egui::Mesh::default();
            let top = Color32::from_white_alpha(if t.dark { 18 } else { 60 });
            mesh.colored_vertex(sheen.left_top(), top);
            mesh.colored_vertex(sheen.right_top(), top);
            mesh.colored_vertex(sheen.right_bottom(), Color32::TRANSPARENT);
            mesh.colored_vertex(sheen.left_bottom(), Color32::TRANSPARENT);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            p.add(Shape::mesh(mesh));
            p.rect_stroke(
                rect,
                r,
                Stroke::new(
                    1.0,
                    Color32::from_white_alpha(if t.dark { 40 } else { 140 }),
                ),
                egui::StrokeKind::Inside,
            );
            p.line_segment(
                [
                    pos2(rect.left() + 14.0, rect.top() + 1.0),
                    pos2(rect.right() - 14.0, rect.top() + 1.0),
                ],
                Stroke::new(
                    1.0,
                    Color32::from_white_alpha(if t.dark { 70 } else { 200 }),
                ),
            );
        }
        Panel::Soft => {
            // pressed out of the ground: a dark shadow below right, a
            // light one above left, the same colour as the ground
            let dark = crate::theme::mix(t.ground, Color32::BLACK, if t.dark { 0.5 } else { 0.18 });
            let light =
                crate::theme::mix(t.ground, Color32::WHITE, if t.dark { 0.08 } else { 0.7 });
            for (offset, c) in [(vec2(6.0, 6.0), dark), (vec2(-6.0, -6.0), light)] {
                p.add(
                    egui::epaint::Shadow {
                        offset: [offset.x as i8, offset.y as i8],
                        blur: 16,
                        spread: 0,
                        color: c,
                    }
                    .as_shape(rect, r),
                );
            }
            p.rect_filled(rect, r, fill);
        }
        Panel::Frame => {
            // box-drawing frame: the corners and edges as characters
            let font = egui::FontId::monospace(14.0);
            let c = t.muted;
            p.rect_filled(rect, 0.0, fill);
            p.rect_stroke(
                rect.shrink(6.0),
                0.0,
                Stroke::new(1.0, crate::theme::mix(fill, c, 0.6)),
                egui::StrokeKind::Inside,
            );
            for (at, text, align) in [
                (
                    rect.left_top() + vec2(2.0, 0.0),
                    "┌",
                    egui::Align2::LEFT_TOP,
                ),
                (
                    rect.right_top() + vec2(-2.0, 0.0),
                    "┐",
                    egui::Align2::RIGHT_TOP,
                ),
                (
                    rect.left_bottom() + vec2(2.0, 0.0),
                    "└",
                    egui::Align2::LEFT_BOTTOM,
                ),
                (
                    rect.right_bottom() + vec2(-2.0, 0.0),
                    "┘",
                    egui::Align2::RIGHT_BOTTOM,
                ),
            ] {
                p.text(at, align, text, font.clone(), c);
            }
        }
        Panel::Slab => {
            p.rect_filled(rect, 0.0, fill);
            p.rect_stroke(
                rect,
                0.0,
                Stroke::new(3.0, t.strong),
                egui::StrokeKind::Inside,
            );
            // the slab's offset block below right: a hard shadow
            let block = rect.translate(vec2(6.0, 6.0));
            p.rect_filled(
                Rect::from_min_max(pos2(rect.right(), block.top()), block.right_bottom()),
                0.0,
                t.strong,
            );
            p.rect_filled(
                Rect::from_min_max(pos2(block.left(), rect.bottom()), block.right_bottom()),
                0.0,
                t.strong,
            );
        }
        Panel::Paper => {
            // a sheet behind, turned a little (still: no motion), the card
            // on it, two strips of tape
            let angle = (((seed % 7) as f32) - 3.0) * 0.006 + 0.012;
            let behind = crate::theme::mix(t.surface, t.accent, 0.25);
            quad(p, rect.expand(6.0).translate(vec2(5.0, 4.0)), angle, behind);
            p.rect_filled(rect, r, fill);
            tape(p, rect);
        }
    }
}

/// Two strips of tape over a sheet's top corners - within a few points of
/// its edge (never over the controls round it).
fn tape(p: &Painter, rect: Rect) {
    let tape = Color32::from_rgba_unmultiplied(240, 226, 170, 150);
    quad(
        p,
        Rect::from_center_size(rect.left_top() + vec2(20.0, 8.0), vec2(44.0, 12.0)),
        -0.35,
        tape,
    );
    quad(
        p,
        Rect::from_center_size(rect.right_top() + vec2(-20.0, 8.0), vec2(44.0, 12.0)),
        0.32,
        tape,
    );
}

/// The picture's frame as the design draws it, over the picture's edge
/// (still: a frame never moves): the 90s desktop's sunken well, the rack's
/// bezel, the terminal's box, the brutalist's slab, the glass rim, the
/// soft design's hollow, the collage's photo taped down.
pub fn bezel(p: &Painter, rect: Rect, d: &Design, t: &Theme) {
    let mix = crate::theme::mix;
    let r = CornerRadius::same(d.radius.unwrap_or(t.radius.saturating_add(6)).min(8));
    match d.panel {
        Panel::Card | Panel::Bare => {}
        Panel::Bevel => {
            // sunken: dark above and left, light below and right
            let fill = t.raised;
            let light = mix(fill, Color32::WHITE, 0.35);
            let dark = mix(fill, Color32::BLACK, 0.45);
            let (l, tp, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
            for i in [0.5f32, 1.5] {
                let (l, tp, rt, b) = (l + i, tp + i, rt - i, b - i);
                p.line_segment([pos2(l, b), pos2(l, tp)], Stroke::new(1.0, dark));
                p.line_segment([pos2(l, tp), pos2(rt, tp)], Stroke::new(1.0, dark));
                p.line_segment([pos2(rt, tp), pos2(rt, b)], Stroke::new(1.0, light));
                p.line_segment([pos2(rt, b), pos2(l, b)], Stroke::new(1.0, light));
            }
        }
        Panel::Brushed => {
            // a metal bezel, a dark line inside it
            p.rect_stroke(
                rect,
                r,
                Stroke::new(4.0, mix(t.raised, t.border, 0.5)),
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                rect.shrink(4.0),
                CornerRadius::ZERO,
                Stroke::new(1.0, mix(t.raised, Color32::BLACK, 0.5)),
                egui::StrokeKind::Inside,
            );
        }
        Panel::Frame => {
            p.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, t.accent),
                egui::StrokeKind::Inside,
            );
        }
        Panel::Slab => {
            p.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(3.0, t.strong),
                egui::StrokeKind::Inside,
            );
        }
        Panel::Glass => {
            p.rect_stroke(
                rect,
                r,
                Stroke::new(
                    1.0,
                    Color32::from_white_alpha(if t.dark { 50 } else { 160 }),
                ),
                egui::StrokeKind::Inside,
            );
        }
        Panel::Soft => {
            // a hollow pressed into the ground: its shade along the top
            // and left inside edge
            let dark = mix(t.ground, Color32::BLACK, if t.dark { 0.5 } else { 0.18 });
            for k in 0..4 {
                let a = 90 - 20 * k as u8;
                let c = Color32::from_rgba_unmultiplied(dark.r(), dark.g(), dark.b(), a);
                let i = k as f32 + 0.5;
                p.line_segment(
                    [
                        pos2(rect.left() + i, rect.bottom() - 6.0),
                        pos2(rect.left() + i, rect.top() + i),
                    ],
                    Stroke::new(1.0, c),
                );
                p.line_segment(
                    [
                        pos2(rect.left() + i, rect.top() + i),
                        pos2(rect.right() - 6.0, rect.top() + i),
                    ],
                    Stroke::new(1.0, c),
                );
            }
        }
        Panel::Paper => {
            // a photo: a white border, taped down
            p.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(5.0, mix(t.surface, Color32::WHITE, 0.85)),
                egui::StrokeKind::Inside,
            );
            tape(p, rect);
        }
    }
}

/// A rectangle turned about its centre, filled.
fn quad(p: &Painter, rect: Rect, angle: f32, fill: Color32) {
    let c = rect.center();
    let (s, co) = angle.sin_cos();
    let turn = |v: Vec2| c + vec2(v.x * co - v.y * s, v.x * s + v.y * co);
    let h = rect.size() / 2.0;
    let mut mesh = egui::Mesh::default();
    for v in [
        vec2(-h.x, -h.y),
        vec2(h.x, -h.y),
        vec2(h.x, h.y),
        vec2(-h.x, h.y),
    ] {
        mesh.colored_vertex(turn(v), fill);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(Shape::mesh(mesh));
}

/// Digits as a segment display: the readout, its unlit segments a shade
/// over the glass (no flashing: digits change once a second, small).
pub fn lcd(p: &Painter, at: Pos2, text: &str, size: f32, t: &Theme) -> Rect {
    let font = egui::FontId::monospace(size);
    let glass = crate::theme::mix(t.sunken, t.accent, 0.08);
    let galley = p.layout_no_wrap(text.to_owned(), font.clone(), t.accent);
    let rect = Rect::from_min_size(at, galley.size() + vec2(16.0, 8.0));
    p.rect_filled(rect, 3.0, glass);
    // the unlit segments: 8s behind
    let ghost: String = text
        .chars()
        .map(|c| if c.is_ascii_digit() { '8' } else { c })
        .collect();
    p.text(
        rect.min + vec2(8.0, 4.0),
        egui::Align2::LEFT_TOP,
        ghost,
        font.clone(),
        crate::theme::mix(glass, t.accent, 0.12),
    );
    p.galley(rect.min + vec2(8.0, 4.0), galley, t.accent);
    rect
}

/// A cassette's window: two reels, their spokes turned by `turn` (in
/// turns), the tape between them.
pub fn reels(p: &Painter, rect: Rect, turn: f32, t: &Theme) {
    let window = crate::theme::mix(t.sunken, t.ground, 0.3);
    p.rect_filled(rect, 6.0, window);
    p.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    let r = (rect.height() * 0.36).min(rect.width() * 0.18);
    let tape = crate::theme::mix(t.ground, Color32::from_rgb(70, 45, 30), 0.6);
    let left = pos2(rect.left() + rect.width() * 0.28, rect.center().y);
    let right = pos2(rect.right() - rect.width() * 0.28, rect.center().y);
    p.line_segment(
        [left + vec2(0.0, r), right + vec2(0.0, r)],
        Stroke::new(2.0, tape),
    );
    for c in [left, right] {
        p.circle_filled(c, r, tape);
        p.circle_filled(c, r * 0.45, t.surface);
        p.circle_stroke(c, r * 0.45, Stroke::new(1.0, t.border));
        for k in 0..6 {
            let a = (turn + k as f32 / 6.0) * std::f32::consts::TAU;
            let d = vec2(a.cos(), a.sin());
            p.line_segment(
                [c + d * r * 0.18, c + d * r * 0.42],
                Stroke::new(2.0, t.muted),
            );
        }
    }
}

/// Two analogue meters, left and right, their needles at `levels`
/// (0..1, already eased).
pub fn needles(p: &Painter, rect: Rect, levels: [f32; 2], t: &Theme) {
    let w = rect.width() / 2.0 - 4.0;
    for (i, level) in levels.into_iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + vec2(i as f32 * (w + 8.0), 0.0),
            vec2(w, rect.height()),
        );
        let face = crate::theme::mix(
            t.surface,
            Color32::from_rgb(235, 220, 170),
            if t.dark { 0.25 } else { 0.6 },
        );
        p.rect_filled(r, 4.0, face);
        p.rect_stroke(r, 4.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        let pivot = pos2(r.center().x, r.bottom() + r.height() * 0.15);
        let len = r.height() * 0.95;
        let ink = crate::theme::ink_on(face);
        // the scale: an arc of ticks, the last third in the warning colour
        for k in 0..=10 {
            let a = (-50.0 + k as f32 * 10.0f32).to_radians();
            let d = vec2(a.sin(), -a.cos());
            let c = if k >= 8 { t.bad } else { ink };
            p.line_segment(
                [pivot + d * len * 0.82, pivot + d * len * 0.92],
                Stroke::new(1.0, c),
            );
        }
        let a = (-50.0 + 100.0 * level.clamp(0.0, 1.0)).to_radians();
        let d = vec2(a.sin(), -a.cos());
        p.line_segment([pivot, pivot + d * len * 0.9], Stroke::new(1.5, ink));
        p.text(
            r.left_bottom() + vec2(6.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            if i == 0 { "L" } else { "R" },
            egui::FontId::proportional(10.0),
            ink,
        );
    }
}

/// A rotary knob for a value 0..=100 (the hi-fi rack's volume): dragged
/// up or right to turn it up, the arrow keys when it has the focus, the
/// wheel over it; a slider to a screen reader (`name`, its value).
pub fn knob(ui: &mut egui::Ui, value: &mut u32, name: &str) -> egui::Response {
    let size = vec2(34.0, 34.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let before = *value;
    if response.dragged() {
        let d = response.drag_delta();
        let step = (d.x - d.y) * 0.6;
        *value = (*value as f32 + step).round().clamp(0.0, 100.0) as u32;
    }
    if response.has_focus() {
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft),
            )
        });
        if up {
            *value = (*value + 5).min(100);
        }
        if down {
            *value = value.saturating_sub(5);
        }
    }
    if *value != before {
        response.mark_changed();
    }
    let v = *value as f64;
    let name = name.to_owned();
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::slider(true, v, &name);
        info.label = Some(name.clone());
        info
    });
    let visuals = *ui.style().interact(&response);
    let t = crate::theme::current();
    let c = rect.center();
    let r = rect.width() / 2.0 - 2.0;
    // the knob: a ring of ticks, the cap, its pointer
    for k in 0..=10 {
        let a = (-135.0 + 27.0 * k as f32).to_radians();
        let d = vec2(a.sin(), -a.cos());
        ui.painter().line_segment(
            [c + d * (r + 0.5), c + d * (r + 2.5)],
            Stroke::new(1.0, t.muted),
        );
    }
    ui.painter().circle_filled(c, r - 2.0, t.raised);
    ui.painter().circle_stroke(c, r - 2.0, visuals.bg_stroke);
    let a = (-135.0 + 270.0 * (*value as f32 / 100.0)).to_radians();
    let d = vec2(a.sin(), -a.cos());
    ui.painter().line_segment(
        [c + d * (r * 0.25), c + d * (r - 4.0)],
        Stroke::new(2.5, t.accent),
    );
    response
}

/// The position as a bar of characters: "[#######-------] 1:23 / 4:05".
pub fn ascii_bar(share: f32, width: usize) -> String {
    let n = ((share.clamp(0.0, 1.0) * width as f32).round() as usize).min(width);
    format!("[{}{}]", "#".repeat(n), "-".repeat(width - n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;
    use crate::viz::contrast;

    #[test]
    fn the_designs_are_ten_and_each_its_own() {
        assert_eq!(DESIGNS.len(), 10);
        let mut keys: Vec<_> = DESIGNS.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 10);
        for d in &DESIGNS {
            assert!(
                THEMES.iter().any(|t| t.key == d.suggests),
                "{}: {}",
                d.key,
                d.suggests
            );
            assert!(d.text >= 12.0, "{}: type too small", d.key);
        }
    }

    #[test]
    fn every_design_reads_in_every_theme() {
        // the card's words on the card as each design shows it, over many
        // cover hues; the readout's digits on its glass
        let mut failures = Vec::new();
        for d in &DESIGNS {
            for t in &THEMES {
                let need = crate::theme::text_contrast(t);
                for hue in 0..24 {
                    let raw = crate::viz::from_oklch(0.6, 0.15, hue as f32 * 15.0);
                    let accent = if t.high_contrast {
                        t.accent
                    } else {
                        crate::app::accent_for(raw, t)
                    };
                    let tint = crate::app::card_tint_for(t, accent);
                    let fill = card_fill(d, t, tint);
                    for (what, c) in [("title", t.strong), ("text", t.text), ("quiet", t.muted)] {
                        if contrast(c, fill) < need {
                            failures.push(format!(
                                "{} in {}: {what} on the card {:.2}",
                                d.name,
                                t.name,
                                contrast(c, fill)
                            ));
                        }
                    }
                }
                let list = list_fill(d, t);
                for (what, c) in [("text", t.text), ("quiet", t.muted)] {
                    if contrast(c, list) < need {
                        failures.push(format!(
                            "{} in {}: {what} on the playlist {:.2}",
                            d.name,
                            t.name,
                            contrast(c, list)
                        ));
                    }
                }
                let glass = crate::theme::mix(t.sunken, t.accent, 0.08);
                if contrast(t.accent, glass) < need.min(4.5) {
                    failures.push(format!("{} in {}: LCD digits", d.name, t.name));
                }
            }
        }
        failures.sort();
        failures.dedup_by(|a, b| a.split(':').next() == b.split(':').next());
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn the_haze_keeps_every_word_readable() {
        // loud covers: pure colours, black, white, a checker of them
        let mut covers: Vec<Vec<u8>> = Vec::new();
        for c in [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 0],
            [0, 0, 0],
            [255, 255, 255],
        ] {
            covers.push((0..16 * 16).flat_map(|_| [c[0], c[1], c[2], 255]).collect());
        }
        covers.push(
            (0..16 * 16)
                .flat_map(|i| {
                    if (i % 16 + i / 16) % 2 == 0 {
                        [255, 0, 255, 255]
                    } else {
                        [0, 255, 255, 255]
                    }
                })
                .collect(),
        );
        for t in &THEMES {
            let need = crate::theme::text_contrast(t);
            for cover in &covers {
                let image = haze(cover, 16, 16, t, t.accent);
                assert_eq!(image.size, [8, 8]);
                for px in &image.pixels {
                    for ink in [t.text, t.muted, t.strong, t.accent] {
                        let want = need.min(contrast(ink, t.ground));
                        assert!(
                            contrast(ink, *px) >= want - 0.01,
                            "{}: {:.2} < {want:.2}",
                            t.name,
                            contrast(ink, *px)
                        );
                    }
                }
            }
        }
        // no cover: the ground itself
        let image = haze(&[], 0, 0, &THEMES[0], THEMES[0].accent);
        assert!(image.pixels.iter().all(|p| *p == THEMES[0].ground));
    }

    #[test]
    fn the_bar_of_characters() {
        assert_eq!(ascii_bar(0.5, 10), "[#####-----]");
        assert_eq!(ascii_bar(2.0, 4), "[####]");
        assert_eq!(ascii_bar(-1.0, 4), "[----]");
    }
}
