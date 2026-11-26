//! What plays, moved within its box where it does not fit: a title of a
//! hundred letters ("'Back To The 80's' _ Best of Synthwave And Retro
//! Electro Music Mix for 2 Hours _ Vol. 5") is read whole, not cut to an
//! ellipsis.
//!
//! A text that fits stands still, as it always did. One that does not
//! waits two seconds at its start, moves at a calm pace - about 2.3 times
//! its type's size in points a second, some 32 a second at 14 points;
//! a quarter faster in the intense motion - to its end, waits there a
//! second and a half, and comes back to its start: on round, the start
//! following the end after a gap ([`Style::Wrap`]), or back the way it
//! came, three times as fast ([`Style::Bounce`]). Each design moves it in
//! its own way ([`Look`]): smoothly, its cut edges fading out; or by whole
//! character cells, cut hard, as a fluorescent display or a terminal does.
//!
//! It moves by the clock, not by the sound: paused or stopped, it goes on
//! (0.16.0's marquee stood cut while paused). A new track starts it from
//! its start. The pointer over it holds it where it is, and its whole is in
//! a tooltip as before. A screen reader is given its whole, never the part
//! seen. Where the desktop asks for reduced motion nothing moves: the text
//! is cut to an ellipsis, its whole in the tooltip.
//!
//! Right-to-left text (src/bidi.rs) starts at the right and moves the way
//! it is read.
//!
//! A frame draws only while it moves: during its waits the window is woken
//! when the wait ends; moving smoothly, 20 to 30 times a second (as often
//! as it moves a pixel); by cells, when the next cell comes - some 4 times
//! a second. Where the CPU draws the window (a slow renderer: llvmpipe, no
//! GPU), where a frame costs 50 - 100 ms, every design moves it in jumps
//! of at most a third of its box, there and back, at the same pace on
//! average: a frame every few seconds. A
//! minimised or covered window draws nothing (eframe calls no `ui` then),
//! so nothing moves there. The text is laid out once for each change of it
//! (egui's galley cache: a frame looks it up, it does not lay it out; a
//! galley is not kept here from one frame to the next, since egui clears
//! its font atlas now and then and a galley kept would point into the old
//! one).

use egui::epaint::TextShape;
use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontId, Id, Painter, Pos2, Rect, Response, Sense, Ui, pos2, vec2};
use std::sync::Arc;
use std::time::Duration;

/// How the text comes back to its start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// Back the way it came, after showing its end.
    Bounce,
    /// On round: its start comes after its end, after a gap.
    Wrap,
}

/// A design's way of moving a text that does not fit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub style: Style,
    /// By whole character cells (a fluorescent display, a terminal, the
    /// 90s strip); else smoothly.
    pub stepped: bool,
    /// The edges where it is cut fade out (else cut hard).
    pub fade: bool,
    /// Between its end and its start again ([`Style::Wrap`]).
    pub gap: &'static str,
}

/// Modern, Glass: smoothly, on round, the edges fading.
pub const ROUND: Look = Look {
    style: Style::Wrap,
    stepped: false,
    fade: true,
    gap: "   •   ",
};
/// Zen, Soft, the cassette's label, the collage's artist, the display
/// view: smoothly there and back, the edges fading.
pub const THERE_AND_BACK: Look = Look {
    style: Style::Bounce,
    stepped: false,
    fade: true,
    gap: "",
};
/// Brutalist: on round, cut hard.
pub const HARD_ROUND: Look = Look {
    style: Style::Wrap,
    stepped: false,
    fade: false,
    gap: "   •   ",
};
/// The rack's fluorescent display: a cell at a time, on round, after
/// blank cells, as such a display does.
pub const DISPLAY: Look = Look {
    style: Style::Wrap,
    stepped: true,
    fade: false,
    gap: "    ",
};
/// The 90s strip: a cell at a time, on round, "***" between, as the
/// players of the time had it.
pub const STRIP: Look = Look {
    style: Style::Wrap,
    stepped: true,
    fade: false,
    gap: "  ***  ",
};
/// The terminal: a cell at a time, there and back, cut hard.
pub const TERMINAL: Look = Look {
    style: Style::Bounce,
    stepped: true,
    fade: false,
    gap: "",
};

/// How much the window may move (the settings: Calm, the default;
/// Intense; and the desktop's reduced motion over both).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Motion {
    #[default]
    Calm,
    Intense,
    /// Nothing moves: cut to an ellipsis, the whole in the tooltip.
    Reduced,
}

/// The pace of a text that moves.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Timing {
    /// Points a second.
    pub speed: f32,
    /// Seconds at its start, and at its end.
    pub hold_start: f32,
    pub hold_end: f32,
    /// How much faster it comes back ([`Style::Bounce`]).
    pub back: f32,
    /// A character cell's width, moved by whole cells (0: smoothly).
    pub cell: f32,
}

/// The pace for type of `size` points in `motion`; None where nothing
/// moves.
pub fn timing(size: f32, motion: Motion, cell: f32) -> Option<Timing> {
    let calm = Timing {
        speed: (size * 2.3).clamp(24.0, 80.0),
        hold_start: 2.0,
        hold_end: 1.5,
        back: 3.0,
        cell,
    };
    match motion {
        Motion::Reduced => None,
        Motion::Calm => Some(calm),
        Motion::Intense => Some(Timing {
            speed: calm.speed * 1.25,
            ..calm
        }),
    }
}

/// Where a text is, `t` seconds into its moving.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct At {
    /// How far it has moved toward its end (points; 0 at its start).
    pub offset: f32,
    /// Moving now (not waiting at an end; not standing, fitting).
    pub moving: bool,
    /// Seconds until it is drawn elsewhere: a wait's rest, the next cell;
    /// 0 moving smoothly (every frame); infinite where it stands.
    pub next: f32,
}

impl At {
    const STILL: At = At {
        offset: 0.0,
        moving: false,
        next: f32::INFINITY,
    };
}

/// How far it must go to show its end: what does not fit, in whole cells
/// where it moves by them (its last cell whole).
pub fn reach(text: f32, room: f32, m: &Timing) -> f32 {
    let over = text - room;
    if over <= 0.5 {
        return 0.0;
    }
    if m.cell > 0.0 {
        (over / m.cell - 0.01).ceil() * m.cell
    } else {
        over
    }
}

/// The seconds of one round: from its start, to its end, back to its start.
pub fn cycle(text: f32, room: f32, gap: f32, style: Style, m: &Timing) -> f32 {
    let over = reach(text, room, m);
    if over <= 0.0 || m.speed <= 0.0 {
        return f32::INFINITY;
    }
    let go = over / m.speed;
    let back = match style {
        Style::Bounce => over / (m.speed * m.back),
        Style::Wrap => (round_trip(text, gap, m) - over).max(0.0) / m.speed,
    };
    m.hold_start + go + m.hold_end + back
}

/// How far it goes on round before it looks as at its start (its width
/// and the gap's; whole cells).
fn round_trip(text: f32, gap: f32, m: &Timing) -> f32 {
    let d = text + gap;
    if m.cell > 0.0 {
        (d / m.cell).round() * m.cell
    } else {
        d
    }
}

/// Where a text `text` points wide is in a box `room` wide, `t` seconds
/// into its moving (the time it was shown, less what the pointer held it):
/// a pure function of time and widths - the sound plays no part.
pub fn at(t: f32, text: f32, room: f32, gap: f32, style: Style, m: &Timing) -> At {
    let over = reach(text, room, m);
    if over <= 0.0 || m.speed <= 0.0 {
        return At::STILL;
    }
    let total = cycle(text, room, gap, style, m);
    let t = t.max(0.0).rem_euclid(total);
    let go = over / m.speed;
    // (by cells: the cell it has reached, and when the next comes)
    let moved = |d: f32, speed: f32| -> (f32, f32) {
        if m.cell > 0.0 {
            let cells = (d / m.cell + 1e-4).floor();
            let next = ((cells + 1.0) * m.cell - d) / speed;
            (cells * m.cell, next.max(0.0))
        } else {
            (d, 0.0)
        }
    };
    let (start_end, end_end) = (m.hold_start, m.hold_start + go);
    let held_end = end_end + m.hold_end;
    if t < start_end {
        return At {
            offset: 0.0,
            moving: false,
            next: start_end - t,
        };
    }
    if t < end_end {
        let (offset, next) = moved((t - start_end) * m.speed, m.speed);
        return At {
            offset: offset.min(over),
            moving: true,
            next,
        };
    }
    if t < held_end {
        return At {
            offset: over,
            moving: false,
            next: held_end - t,
        };
    }
    match style {
        Style::Bounce => {
            let speed = m.speed * m.back;
            let (back, next) = moved((t - held_end) * speed, speed);
            At {
                offset: (over - back).max(0.0),
                moving: true,
                next,
            }
        }
        Style::Wrap => {
            let round = round_trip(text, gap, m);
            let (d, next) = moved((t - held_end) * m.speed, m.speed);
            let offset = over + d;
            At {
                offset: if offset >= round - 0.01 { 0.0 } else { offset },
                moving: true,
                next,
            }
        }
    }
}

/// What the window says of this frame: how much may move, the track (a
/// new one starts every text over), whether to keep what was drawn
/// (tests).
#[derive(Clone, Copy, Default)]
struct Frame {
    motion: Motion,
    track: u64,
    /// A slow renderer (the CPU draws the window): moved in jumps of at
    /// most a third of the box, a frame every few seconds.
    slow: bool,
    record: bool,
}

/// A text moving: what it is (a hash), of which track, how long it has
/// moved, when it was last drawn.
#[derive(Clone, Copy)]
struct Clock {
    text: u64,
    track: u64,
    t: f32,
    last: f64,
}

/// What was drawn moving this frame (tests).
#[derive(Clone, Debug)]
pub struct Seen {
    /// The whole text, as written.
    pub text: String,
    /// Its box (it is drawn nowhere else).
    pub rect: Rect,
    pub offset: f32,
    pub moving: bool,
    /// Cut to an ellipsis (reduced motion).
    pub elided: bool,
}

fn frame_id() -> Id {
    Id::NULL.with("amp-marquee-frame")
}

fn seen_id() -> Id {
    Id::NULL.with("amp-marquee-seen")
}

/// At a frame's start: how much may move, which track plays, whether the
/// renderer is slow.
pub fn begin_frame(ctx: &egui::Context, motion: Motion, track: u64, slow: bool) {
    ctx.data_mut(|d| {
        let f = d.get_temp_mut_or_default::<Frame>(frame_id());
        f.motion = motion;
        f.track = track;
        f.slow = slow;
        if f.record {
            d.insert_temp(seen_id(), Arc::new(Vec::<Seen>::new()));
        }
    });
}

/// Keep what is drawn moving, for [`seen`] (tests).
#[doc(hidden)]
pub fn record(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Frame>(frame_id()).record = on);
}

/// The texts drawn moving (or cut, in reduced motion) in the last frame
/// (tests; [`record`] first).
#[doc(hidden)]
pub fn seen(ctx: &egui::Context) -> Vec<Seen> {
    ctx.data(|d| d.get_temp::<Arc<Vec<Seen>>>(seen_id()))
        .map(|v| v.as_ref().clone())
        .unwrap_or_default()
}

fn frame(ctx: &egui::Context) -> Frame {
    ctx.data(|d| d.get_temp::<Frame>(frame_id()))
        .unwrap_or_default()
}

/// How much may move this frame.
pub fn motion(ctx: &egui::Context) -> Motion {
    frame(ctx).motion
}

/// Whether the window is drawn by a slow renderer (moved in jumps, few
/// frames).
pub fn slow(ctx: &egui::Context) -> bool {
    frame(ctx).slow
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The seconds `text` at `id` has moved, counted on: from 0 for a new
/// text or a new track; not while `held` (the pointer over it), nor over a
/// gap of more than five seconds (it was not drawn: the window hidden,
/// another design).
pub fn clock(ctx: &egui::Context, id: Id, text: &str, held: bool) -> f32 {
    let now = ctx.input(|i| i.time);
    let f = frame(ctx);
    let key = hash(text);
    ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_insert_with(id, || Clock {
            text: key,
            track: f.track,
            t: 0.0,
            last: now,
        });
        if c.text != key || c.track != f.track {
            *c = Clock {
                text: key,
                track: f.track,
                t: 0.0,
                last: now,
            };
        }
        let dt = (now - c.last) as f32;
        if !held && (0.0..=5.0).contains(&dt) {
            c.t += dt;
        }
        c.last = now;
        c.t
    })
}

/// Forget the clock of a text that fits again.
fn forget(ctx: &egui::Context, id: Id) {
    ctx.data_mut(|d| d.remove::<Clock>(id));
}

/// Wake the window in `secs` (not while minimised; at most 60 times a
/// second; never "now" - tests run until nothing asks at once).
pub fn wake(ctx: &egui::Context, secs: f32) {
    if !secs.is_finite() || ctx.input(|i| i.viewport().minimized == Some(true)) {
        return;
    }
    ctx.request_repaint_after(Duration::from_secs_f32(secs.max(1.0 / 60.0)));
}

/// How often to draw a text moving smoothly at `speed`: as often as it
/// moves a pixel, 20 to 30 times a second.
fn frame_every(speed: f32, ppp: f32) -> f32 {
    (1.0 / (speed * ppp).max(1.0)).clamp(1.0 / 30.0, 1.0 / 20.0)
}

/// A text to draw: what it says, in what type and colour; what lies under
/// it and moves with it (the display's unlit segments).
pub struct Text<'a> {
    pub text: &'a str,
    pub font: FontId,
    pub colour: Color32,
    pub under: Option<(String, Color32)>,
}

/// Where a text was drawn.
pub struct Shown {
    pub rect: Rect,
    /// It does not fit: moved (or cut, in reduced motion).
    pub cut: bool,
}

fn one_row(painter: &Painter, text: &str, format: TextFormat) -> Arc<egui::Galley> {
    painter.layout_job(LayoutJob::single_section(
        crate::bidi::seen(text).into_owned(),
        format,
    ))
}

/// `text` painted at `pos` (`align` to it) in at most `width`: whole where
/// it fits (as before); else moved within a box `width` wide (`look`), its
/// whole in a tooltip. `id` keeps its clock.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    ui: &Ui,
    painter: &Painter,
    id: Id,
    pos: Pos2,
    align: Align2,
    text: &Text,
    width: f32,
    look: Look,
) -> Shown {
    let format = TextFormat::simple(text.font.clone(), text.colour);
    let galley = one_row(painter, text.text, format.clone());
    if galley.size().x <= width + 0.5 {
        let rect = align.anchor_size(pos, galley.size());
        if let Some((under, colour)) = &text.under {
            let g = one_row(
                painter,
                under,
                TextFormat::simple(text.font.clone(), *colour),
            );
            painter.galley(rect.min, g, *colour);
        }
        painter.galley(rect.min, galley, text.colour);
        forget(ui.ctx(), id);
        return Shown { rect, cut: false };
    }
    let f = frame(ui.ctx());
    if f.motion == Motion::Reduced {
        // (cut to an ellipsis, its whole in a tooltip)
        let cut = crate::app::one_line(painter, text.text, text.font.clone(), text.colour, width);
        let rect = align.anchor_size(pos, cut.size());
        if let Some((under, colour)) = &text.under {
            let g = crate::app::one_line(painter, under, text.font.clone(), *colour, width);
            painter.galley(rect.min, g, *colour);
        }
        painter.galley(rect.min, cut, text.colour);
        crate::app::tooltip_over(ui, rect, text.text);
        keep(ui.ctx(), f, text.text, rect, 0.0, false, true);
        return Shown { rect, cut: true };
    }
    let rect = align.anchor_size(pos, vec2(width, galley.size().y));
    let response = ui.interact(rect, id.with("whole"), Sense::hover());
    let under = text.under.as_ref().map(|(u, c)| {
        (
            one_row(painter, u, TextFormat::simple(text.font.clone(), *c)),
            *c,
        )
    });
    moving(
        ui,
        painter,
        id,
        rect,
        &Laid {
            galley,
            format,
            under,
            whole: text.text,
        },
        look,
        response.hovered(),
    );
    let _ = crate::app::whole_on_hover(response, text.text);
    Shown { rect, cut: true }
}

/// A label in one line (as [`crate::app::fit_label`]): whole where it
/// fits, as before; else moved within the room the line has (`look`), its
/// whole in a tooltip and given to a screen reader.
pub fn label(ui: &mut Ui, text: impl Into<egui::WidgetText>, look: Look) -> Response {
    let text: egui::WidgetText = text.into();
    let whole = text.text().to_owned();
    let job =
        text.clone()
            .into_layout_job(ui.style(), egui::FontSelection::Default, ui.text_valign());
    let Some(section) = job.sections.first() else {
        return crate::app::fit_label(ui, text);
    };
    let mut format = section.format.clone();
    if format.color == Color32::PLACEHOLDER {
        format.color = ui.visuals().text_color();
    }
    let width = ui.available_width();
    let galley = one_row(ui.painter(), &whole, format.clone());
    if galley.size().x <= width + 0.5 {
        return crate::app::fit_label(ui, text);
    }
    let f = frame(ui.ctx());
    if f.motion == Motion::Reduced {
        // (cut to an ellipsis, its whole in a tooltip)
        let response = crate::app::fit_label(ui, text);
        keep(ui.ctx(), f, &whole, response.rect, 0.0, false, true);
        return response;
    }
    let id = ui.next_auto_id().with("marquee");
    let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, &whole));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter().clone();
        moving(
            ui,
            &painter,
            id,
            rect,
            &Laid {
                galley,
                format,
                under: None,
                whole: &whole,
            },
            look,
            response.hovered(),
        );
    }
    crate::app::whole_on_hover(response, &whole)
}

/// A text laid out: its galley in one row, its type, what moves under it.
struct Laid<'a> {
    galley: Arc<egui::Galley>,
    format: TextFormat,
    under: Option<(Arc<egui::Galley>, Color32)>,
    whole: &'a str,
}

/// The text moved within `rect`, cut there (its cut edges faded, where
/// `look` has them); the window woken for its next move.
fn moving(ui: &Ui, painter: &Painter, id: Id, rect: Rect, laid: &Laid, look: Look, held: bool) {
    let ctx = ui.ctx();
    let f = frame(ctx);
    let ppp = ctx.pixels_per_point();
    let font = &laid.format.font_id;
    let text_w = laid.galley.size().x;
    // (a slow renderer - every frame drawn by the CPU, 50 - 100 ms of it
    // in a window of 620 x 860 -: in jumps of at most a third of the box,
    // evenly to its end, there and back, at the same pace on average - a
    // frame every few seconds)
    let (look, cell) = if f.slow {
        let over = (text_w - rect.width()).max(1.0);
        let n = (over / (rect.width() / 3.0).max(8.0)).ceil().max(1.0);
        let look = Look {
            style: Style::Bounce,
            stepped: true,
            ..look
        };
        (look, over / n)
    } else if look.stepped {
        (look, ctx.fonts_mut(|fonts| fonts.glyph_width(font, '0')))
    } else {
        (look, 0.0)
    };
    let Some(m) = timing(font.size, f.motion, cell) else {
        return;
    };
    let gap = (look.style == Style::Wrap && !look.gap.is_empty())
        .then(|| one_row(painter, look.gap, laid.format.clone()));
    let gap_w = gap.as_ref().map_or(0.0, |g| g.size().x);
    let t = clock(ctx, id, laid.whole, held);
    let now = at(t, text_w, rect.width(), gap_w, look.style, &m);
    // (read from the right: it starts at the right and moves to it)
    let rtl = crate::bidi::right_to_left(laid.whole);
    let px = |x: f32| (x * ppp).round() / ppp;
    let x0 = if rtl {
        px(rect.right() - text_w + now.offset)
    } else {
        px(rect.left() - now.offset)
    };
    // the pieces: the text, and on round the gap and the text again
    let mut pieces: [Option<(f32, f32, bool)>; 3] = [Some((x0, text_w, true)), None, None];
    if look.style == Style::Wrap {
        let (gx, nx) = if rtl {
            (x0 - gap_w, x0 - gap_w - text_w)
        } else {
            (x0 + text_w, x0 + text_w + gap_w)
        };
        pieces[1] = gap.as_ref().map(|_| (gx, gap_w, false));
        pieces[2] = Some((nx, text_w, true));
    }
    let y = rect.top();
    // how much of the text lies beyond each edge (the fade's strength)
    let (mut left, mut right) = (0.0f32, 0.0f32);
    for (x, w, _) in pieces.iter().flatten().filter(|(_, _, t)| *t) {
        if x + w > rect.left() && *x < rect.right() {
            left = left.max(rect.left() - x);
            right = right.max(x + w - rect.right());
        }
    }
    let fade = if look.fade {
        (rect.height() * 0.9).min(rect.width() * 0.15).min(18.0)
    } else {
        0.0
    };
    let strength = |hidden: f32| {
        if fade > 0.0 {
            // (less than a pixel beyond: rounding, not text)
            ((hidden - 1.0) / fade).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let (sl, sr) = (strength(left), strength(right));
    let draw = |clip: Rect, opacity: f32| {
        let clip = clip.intersect(painter.clip_rect());
        if !clip.is_positive() {
            return;
        }
        let p = painter.with_clip_rect(clip);
        for (x, w, is_text) in pieces.iter().flatten() {
            if x + w < clip.left() || *x > clip.right() {
                continue;
            }
            let at = pos2(*x, y);
            if *is_text {
                if let Some((under, colour)) = &laid.under {
                    p.add(TextShape::new(at, under.clone(), *colour).with_opacity_factor(opacity));
                }
                p.add(
                    TextShape::new(at, laid.galley.clone(), laid.format.color)
                        .with_opacity_factor(opacity),
                );
            } else if let Some(g) = &gap {
                p.add(
                    TextShape::new(at, g.clone(), laid.format.color).with_opacity_factor(opacity),
                );
            }
        }
    };
    // (the box a little taller than the row: descenders, accents)
    let band =
        |a: f32, b: f32| Rect::from_x_y_ranges(a..=b, rect.top() - 2.0..=rect.bottom() + 2.0);
    const STRIPS: usize = 5;
    let mut inner = (rect.left(), rect.right());
    for (s, from_left) in [(sl, true), (sr, false)] {
        if s <= 0.0 {
            continue;
        }
        let step = fade / STRIPS as f32;
        for i in 0..STRIPS {
            let opacity = 1.0 - s * (1.0 - (i as f32 + 0.5) / STRIPS as f32);
            let (a, b) = if from_left {
                (
                    rect.left() + i as f32 * step,
                    rect.left() + (i + 1) as f32 * step,
                )
            } else {
                (
                    rect.right() - (i + 1) as f32 * step,
                    rect.right() - i as f32 * step,
                )
            };
            draw(band(a, b), opacity);
        }
        if from_left {
            inner.0 += fade;
        } else {
            inner.1 -= fade;
        }
    }
    draw(band(inner.0, inner.1), 1.0);
    keep(ctx, f, laid.whole, rect, now.offset, now.moving, false);
    if !held {
        let next = if now.moving && m.cell <= 0.0 {
            frame_every(m.speed, ppp)
        } else {
            now.next
        };
        wake(ctx, next);
    }
}

/// A text shown in parts by a design's own way (the collage's pages):
/// which part, whether it moves (tests).
pub fn note(ctx: &egui::Context, text: &str, rect: Rect, part: f32, moving: bool) {
    keep(ctx, frame(ctx), text, rect, part, moving, false);
}

/// What was drawn, for the tests.
fn keep(
    ctx: &egui::Context,
    f: Frame,
    text: &str,
    rect: Rect,
    offset: f32,
    moving: bool,
    elided: bool,
) {
    if !f.record {
        return;
    }
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Arc<Vec<Seen>>>(seen_id());
        Arc::make_mut(list).push(Seen {
            text: text.to_owned(),
            rect,
            offset,
            moving,
            elided,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Timing = Timing {
        speed: 32.0,
        hold_start: 2.0,
        hold_end: 1.5,
        back: 3.0,
        cell: 0.0,
    };

    #[test]
    fn a_text_that_fits_stands_still() {
        for style in [Style::Bounce, Style::Wrap] {
            for t in [0.0, 1.0, 5.0, 100.0, 1e6] {
                assert_eq!(at(t, 200.0, 200.0, 20.0, style, &M), At::STILL);
                assert_eq!(at(t, 120.0, 300.0, 20.0, style, &M), At::STILL);
            }
        }
    }

    #[test]
    fn it_waits_at_its_start_then_moves_at_its_pace() {
        // 100 points too wide: two seconds still, then 32 points a second
        for style in [Style::Bounce, Style::Wrap] {
            for t in [0.0, 0.5, 1.99] {
                let a = at(t, 300.0, 200.0, 20.0, style, &M);
                assert_eq!(a.offset, 0.0);
                assert!(!a.moving);
                assert!((a.next - (2.0 - t)).abs() < 1e-4, "{a:?}");
            }
            let a = at(2.5, 300.0, 200.0, 20.0, style, &M);
            assert!((a.offset - 16.0).abs() < 1e-3, "{a:?}");
            assert!(a.moving && a.next == 0.0);
            let b = at(3.5, 300.0, 200.0, 20.0, style, &M);
            assert!((b.offset - a.offset - 32.0).abs() < 1e-3);
        }
    }

    #[test]
    fn it_waits_at_its_end_showing_its_tail() {
        // 100 points at 32 a second: at its end 2 + 3.125 s in, for 1.5 s
        for style in [Style::Bounce, Style::Wrap] {
            for t in [5.13, 6.0, 6.6] {
                let a = at(t, 300.0, 200.0, 20.0, style, &M);
                assert_eq!(a.offset, 100.0, "{style:?} {t}");
                assert!(!a.moving);
            }
        }
    }

    #[test]
    fn bounced_it_comes_back_faster_and_starts_again() {
        let back_from = 2.0 + 100.0 / 32.0 + 1.5;
        let a = at(back_from + 0.5, 300.0, 200.0, 20.0, Style::Bounce, &M);
        assert!((a.offset - (100.0 - 48.0)).abs() < 1e-3, "{a:?}");
        let total = cycle(300.0, 200.0, 20.0, Style::Bounce, &M);
        assert!((total - (back_from + 100.0 / 96.0)).abs() < 1e-4);
        // a round later, where it was
        for t in [0.3, 2.7, 5.5, 7.0] {
            let (x, y) = (
                at(t, 300.0, 200.0, 20.0, Style::Bounce, &M),
                at(t + total, 300.0, 200.0, 20.0, Style::Bounce, &M),
            );
            assert!((x.offset - y.offset).abs() < 1e-2, "{t}: {x:?} {y:?}");
        }
        // never past its ends
        for k in 0..2000 {
            let a = at(k as f32 * 0.01, 300.0, 200.0, 20.0, Style::Bounce, &M);
            assert!((0.0..=100.0).contains(&a.offset));
        }
    }

    #[test]
    fn on_round_its_start_follows_its_end_after_the_gap() {
        // 300 wide, a gap of 20: after its end it goes on to 320 (its copy
        // at the start) - then it is at its start again
        let back_from = 2.0 + 100.0 / 32.0 + 1.5;
        let a = at(back_from + 1.0, 300.0, 200.0, 20.0, Style::Wrap, &M);
        assert!((a.offset - 132.0).abs() < 1e-3, "{a:?}");
        let total = cycle(300.0, 200.0, 20.0, Style::Wrap, &M);
        assert!((total - (back_from + 220.0 / 32.0)).abs() < 1e-4);
        let end = at(total - 0.001, 300.0, 200.0, 20.0, Style::Wrap, &M);
        assert!(end.offset > 319.0, "{end:?}");
        let again = at(total + 0.001, 300.0, 200.0, 20.0, Style::Wrap, &M);
        assert_eq!(again.offset, 0.0);
        // moving forward only, smoothly: no jump but the one to its copy
        let mut last = 0.0;
        for k in 0..((total * 100.0) as usize) {
            let a = at(k as f32 * 0.01, 300.0, 200.0, 20.0, Style::Wrap, &M);
            assert!(a.offset >= last - 1e-3 || a.offset == 0.0, "{k}");
            assert!(a.offset - last <= 32.0 * 0.01 + 1e-3 || last == 0.0);
            last = a.offset;
        }
    }

    #[test]
    fn by_cells_it_moves_a_whole_cell_at_a_time() {
        let m = Timing { cell: 8.0, ..M };
        // 101 too wide: 13 cells (its last whole)
        assert_eq!(reach(301.0, 200.0, &m), 104.0);
        for style in [Style::Bounce, Style::Wrap] {
            let mut seen = Vec::new();
            for k in 0..1000 {
                let a = at(k as f32 * 0.013, 301.0, 200.0, 24.0, style, &m);
                assert_eq!(a.offset % 8.0, 0.0, "{a:?}");
                if a.moving {
                    // (woken when the next cell comes: at most a cell's time)
                    assert!(a.next > 0.0 && a.next <= 8.0 / 32.0 + 1e-4, "{a:?}");
                }
                seen.push(a.offset);
            }
            assert!(seen.contains(&104.0), "{style:?}: its end shown");
        }
    }

    #[test]
    fn the_pace_follows_the_type_and_the_motion() {
        let calm = timing(14.0, Motion::Calm, 0.0).unwrap();
        assert!((30.0..=34.0).contains(&calm.speed), "{calm:?}");
        let big = timing(26.0, Motion::Calm, 0.0).unwrap();
        assert!(big.speed > calm.speed);
        let small = timing(9.0, Motion::Calm, 0.0).unwrap();
        assert!(small.speed >= 24.0);
        let intense = timing(14.0, Motion::Intense, 0.0).unwrap();
        assert!(intense.speed > calm.speed && intense.speed <= 42.0);
        assert!(timing(14.0, Motion::Reduced, 0.0).is_none());
    }

    #[test]
    fn its_clock_starts_over_for_a_new_text_or_track_and_stops_while_held() {
        let ctx = egui::Context::default();
        let id = Id::NULL.with("t");
        let mut time = 0.0;
        let mut step = |held: bool, text: &str, track: u64, dt: f64| -> f32 {
            time += dt;
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut out = 0.0;
            let mut output = ctx.run_ui(input, |ui| {
                begin_frame(ui.ctx(), Motion::Calm, track, false);
                out = clock(ui.ctx(), id, text, held);
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(step(false, "a", 1, 0.0), 0.0);
        assert!((step(false, "a", 1, 0.5) - 0.5).abs() < 1e-4);
        assert!((step(false, "a", 1, 0.5) - 1.0).abs() < 1e-4);
        // held: where it was
        assert!((step(true, "a", 1, 3.0) - 1.0).abs() < 1e-4);
        assert!((step(false, "a", 1, 0.25) - 1.25).abs() < 1e-4);
        // not drawn for a while (hidden): where it was
        assert!((step(false, "a", 1, 60.0) - 1.25).abs() < 1e-4);
        // a new text, a new track: from its start
        assert_eq!(step(false, "b", 1, 0.5), 0.0);
        assert!((step(false, "b", 1, 0.5) - 0.5).abs() < 1e-4);
        assert_eq!(step(false, "b", 2, 0.5), 0.0);
    }
}
