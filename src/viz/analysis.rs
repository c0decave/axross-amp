//! Tools to look at the sound with: a spectrum analyzer with note names, a
//! loudness meter after EBU R128, the phase and the stereo width, a
//! goniometer, the track's waveform (a click seeks), a chromagram with the
//! circle of fifths and the key, a piano roll and a mel spectrogram.

use super::{Canvas, Frame, Pen, Scheme, VizState, gpu, mix};
use crate::analyzer::{Analyzer, fine};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, pos2, vec2};
use std::f32::consts::{PI, TAU};

/// Loudness history: 60 s, a value each 100 ms.
const LOUD_STEPS: usize = 600;
/// Phase history: 10 s, a value each 30th.
const PHASE_STEPS: usize = 300;
/// The goniometer's phosphor: its picture's side.
const PHOSPHOR: usize = 256;
/// What a stream played: 20 s, a column each 30th.
const WAVE_STEPS: usize = 600;
/// The piano roll's keys: C2 .. C7.
const ROLL_LOW: usize = 36;
const ROLL_HIGH: usize = 96;

/// The circle of fifths, from C at the top.
const FIFTHS: [usize; 12] = [0, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10, 5];

pub struct State {
    loudness: Vec<f32>,
    loudness_at: usize,
    loudness_clock: f32,
    phase: Vec<[f32; 2]>,
    phase_at: usize,
    phase_clock: f32,
    phosphor: Vec<f32>,
    gain: f32,
    waves: Vec<[f32; 2]>,
    waves_at: usize,
    waves_clock: f32,
    wave_span: [f32; 2],
    chroma: [f32; 12],
    /// The key: root (0 = C), minor, how sure (correlation).
    pub key: (usize, bool, f32),
    last_time: f64,
    picture_step: u64,
    /// The size of the waterfall's picture (the mel's is another).
    hd_picture: (usize, usize),
}

impl Default for State {
    fn default() -> Self {
        State {
            loudness: vec![f32::NEG_INFINITY; LOUD_STEPS],
            loudness_at: 0,
            loudness_clock: 0.0,
            phase: vec![[0.0; 2]; PHASE_STEPS],
            phase_at: 0,
            phase_clock: 0.0,
            phosphor: Vec::new(),
            gain: 1.0,
            waves: vec![[0.0; 2]; WAVE_STEPS],
            waves_at: 0,
            waves_clock: 0.0,
            wave_span: [0.0; 2],
            chroma: [0.0; 12],
            key: (0, false, 0.0),
            last_time: 0.0,
            picture_step: 0,
            hd_picture: (0, 0),
        }
    }
}

impl State {
    /// Seconds of sound since the last frame (none while nothing plays).
    fn advance(&mut self, a: &Analyzer) -> f32 {
        let dt = (a.time - self.last_time).clamp(0.0, 0.25) as f32;
        self.last_time = a.time;
        dt
    }
}

pub fn draw(f: &Frame<'_>, key: &str, state: &mut VizState) {
    let dt = state.analysis.advance(f.a);
    let st = &mut state.analysis;
    let cv = &mut state.canvas;
    // the histories move on whichever face is shown (a switch shows them filled)
    record(f.a, st, dt);
    match key {
        "loudness" => loudness(f, cv, st),
        "phase" => phase(f, cv, st),
        "goniometer" => goniometer(f, state),
        "overview" => overview(f, cv, st),
        "chroma" => chroma(f, cv, st, dt),
        "pianoroll" => pianoroll(f, cv),
        "mel" => mel(f, state),
        _ => analyzer(f, cv),
    }
}

fn record(a: &Analyzer, st: &mut State, dt: f32) {
    st.loudness_clock += dt;
    while st.loudness_clock >= 0.1 {
        st.loudness_clock -= 0.1;
        st.loudness_at = (st.loudness_at + 1) % LOUD_STEPS;
        st.loudness[st.loudness_at] = a.loudness.short_term;
    }
    st.phase_clock += dt;
    while st.phase_clock >= 1.0 / 30.0 {
        st.phase_clock -= 1.0 / 30.0;
        st.phase_at = (st.phase_at + 1) % PHASE_STEPS;
        st.phase[st.phase_at] = [a.correlation, a.width];
    }
    // what was heard: its lowest and highest sample a 30th
    for v in &a.mono[a.mono.len().saturating_sub((dt * a.rate as f32) as usize)..] {
        st.wave_span[0] = st.wave_span[0].min(*v);
        st.wave_span[1] = st.wave_span[1].max(*v);
    }
    st.waves_clock += dt;
    while st.waves_clock >= 1.0 / 30.0 {
        st.waves_clock -= 1.0 / 30.0;
        st.waves_at = (st.waves_at + 1) % WAVE_STEPS;
        st.waves[st.waves_at] = st.wave_span;
        st.wave_span = [0.0; 2];
    }
}

/// A label, never smaller than 11 px (readable).
fn label(p: &Painter, at: Pos2, align: Align2, text: impl ToString, size: f32, colour: Color32) {
    p.text(
        at,
        align,
        text,
        FontId::proportional(size.max(11.0)),
        colour,
    );
}

fn mono_label(
    p: &Painter,
    at: Pos2,
    align: Align2,
    text: impl ToString,
    size: f32,
    colour: Color32,
) {
    p.text(at, align, text, FontId::monospace(size.max(11.0)), colour);
}

/// "-14.2", "-inf" (the font has no real minus nor infinity).
fn db(v: f32) -> String {
    if !v.is_finite() {
        return "-inf".to_owned();
    }
    format!("{v:.1}")
}

fn hz_text(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.1} Hz")
    }
}

fn note_text(hz: f32) -> String {
    let (midi, cents) = fine::note(hz);
    // (rounded first: never "-0 ct")
    let cents = cents.round();
    let sign = if cents > 0.0 {
        "+"
    } else if cents < 0.0 {
        "-"
    } else {
        "±"
    };
    format!("{} {sign}{:.0} ct", fine::note_name(midi), cents.abs())
}

// -- the spectrum analyzer -----------------------------------------------------------

fn analyzer(f: &Frame<'_>, cv: &mut Canvas) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let fine = &a.fine;
    let r = f.rect;
    let plot = Rect::from_min_max(
        pos2(r.left() + 34.0, r.top() + 16.0),
        pos2(r.right() - 6.0, r.bottom() - 16.0),
    );
    if plot.width() < 40.0 || plot.height() < 30.0 {
        return;
    }
    let top_hz = 20_000f32.min(a.rate as f32 / 2.0);
    let span = (top_hz / 20.0).ln();
    let x_of = |hz: f32| plot.left() + (hz / 20.0).ln() / span * plot.width();
    let hz_of = |x: f32| 20.0 * ((x - plot.left()) / plot.width() * span).exp();
    let y_of = |db: f32| plot.top() + (-db / -fine::FLOOR_DB).clamp(0.0, 1.0) * plot.height();
    let grid_hz = [
        20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
    ];
    cv.draw(p, |pen| {
        pen.rect(plot, mix(s.background, s.track(), 0.6));
        for hz in grid_hz {
            if hz <= top_hz {
                let x = x_of(hz);
                pen.rect(
                    Rect::from_x_y_ranges(x - 0.5..=x + 0.5, plot.y_range()),
                    s.grid(),
                );
            }
        }
        let mut level = 0.0;
        while level >= fine::FLOOR_DB {
            let y = y_of(level);
            pen.rect(
                Rect::from_x_y_ranges(plot.x_range(), y - 0.5..=y + 0.5),
                s.grid(),
            );
            level -= 12.0;
        }
        // octaves: a C each
        for octave in 1..=9 {
            let hz = fine::midi_hz((12 * (octave + 1)) as f32);
            if hz > 20.0 && hz < top_hz {
                let x = x_of(hz);
                pen.rect(
                    Rect::from_x_y_ranges(x - 0.5..=x + 0.5, r.top() + 2.0..=plot.top()),
                    s.text(),
                );
            }
        }
        // the spectrum: the loudest bin under each step (two pixels, at
        // most 640 steps across: a 4K picture is no dearer than full HD)
        let step = (plot.width() / 640.0).max(2.0);
        pen.points.clear();
        let mut x = plot.left();
        while x <= plot.right() {
            let db = fine.db_span(hz_of(x), hz_of(x + step), false);
            pen.points.push(pos2(x, y_of(db)));
            x += step;
        }
        under_gradient(pen, s, plot.bottom(), plot.bottom(), plot.top());
        pen.line(1.4, s.trace());
        pen.points.clear();
        let mut x = plot.left();
        while x <= plot.right() {
            let db = fine.db_span(hz_of(x), hz_of(x + step), true);
            pen.points.push(pos2(x, y_of(db)));
            x += step;
        }
        pen.line(1.0, s.peak.gamma_multiply(0.65));
        if fine.peak_db > -80.0 && fine.peak_hz > 20.0 {
            let x = x_of(fine.peak_hz);
            let y = y_of(fine.peak_db);
            pen.segment(
                pos2(x, y - 4.0),
                pos2(x, plot.top()),
                1.0,
                s.peak.gamma_multiply(0.5),
            );
            pen.circle(pos2(x, y), 3.0, s.peak);
        }
        if let Some(h) = f.hover.filter(|h| plot.contains(*h)) {
            pen.rect(
                Rect::from_x_y_ranges(h.x - 0.5..=h.x + 0.5, plot.y_range()),
                s.text(),
            );
        }
    });
    let small = 10.0;
    for hz in grid_hz {
        if hz <= top_hz {
            let text = if hz >= 1000.0 {
                format!("{}k", hz / 1000.0)
            } else {
                format!("{hz}")
            };
            // (at the right edge, ending there: not cut off)
            let x = x_of(hz);
            let align = if x > plot.right() - 14.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::CENTER_TOP
            };
            label(
                p,
                pos2(x.min(plot.right()), plot.bottom() + 2.0),
                align,
                text,
                small,
                s.text(),
            );
        }
    }
    let mut level = 0.0;
    while level >= fine::FLOOR_DB {
        label(
            p,
            pos2(plot.left() - 4.0, y_of(level)),
            Align2::RIGHT_CENTER,
            db(level).trim_end_matches(".0"),
            small,
            s.text(),
        );
        level -= 24.0;
    }
    for octave in 1..=9 {
        let midi = 12 * (octave + 1);
        let hz = fine::midi_hz(midi as f32);
        if hz > 20.0 && hz < top_hz {
            label(
                p,
                pos2(x_of(hz) + 2.0, r.top() + 1.0),
                Align2::LEFT_TOP,
                fine::note_name(midi),
                small,
                s.text(),
            );
        }
    }
    if fine.peak_db > -80.0 && fine.peak_hz > 20.0 {
        let text = format!(
            "{}   {}   {} dB",
            note_text(fine.peak_hz),
            hz_text(fine.peak_hz),
            db(fine.peak_db)
        );
        badge(
            p,
            pos2(plot.right() - 4.0, plot.top() + 4.0),
            Align2::RIGHT_TOP,
            &text,
            s,
        );
    }
    if let Some(h) = f.hover.filter(|h| plot.contains(*h)) {
        let hz = hz_of(h.x);
        let text = format!(
            "{}   {}   {} dB",
            hz_text(hz),
            note_text(hz),
            db(fine.db_at(hz, false))
        );
        badge(
            p,
            pos2(plot.left() + 4.0, plot.top() + 4.0),
            Align2::LEFT_TOP,
            &text,
            s,
        );
    }
}

/// Text on a dark plate (readable over anything).
fn badge(p: &Painter, at: Pos2, align: Align2, text: &str, s: &Scheme) {
    let galley = p.layout_no_wrap(text.to_owned(), FontId::monospace(11.0), s.peak);
    let rect = align.anchor_size(at, galley.size()).expand2(vec2(6.0, 3.0));
    p.rect_filled(rect, 4.0, s.background.gamma_multiply(0.85));
    p.galley(rect.min + vec2(6.0, 3.0), galley, s.peak);
}

/// The area under `pen.points`, quiet at `quiet_y`, loud at `loud_y`.
fn under_gradient(pen: &mut Pen<'_>, s: &Scheme, base: f32, quiet_y: f32, loud_y: f32) {
    // coloured by height alone (each column by its own height showed as
    // streaks), cut every 24th of the height
    let t = |y: f32| ((y - quiet_y) / (loud_y - quiet_y)).clamp(0.0, 1.0);
    // (fine enough that the blend between two is the colour itself, and
    // one at the gradient's middle stop)
    let mut levels = [0.0f32; 26];
    for (k, l) in levels.iter_mut().enumerate().take(25) {
        *l = quiet_y + (loud_y - quiet_y) * k as f32 / 24.0;
    }
    levels[25] = quiet_y + (loud_y - quiet_y) * 0.6;
    pen.fill_by_height(base, &levels, |y| {
        let k = t(y);
        mix(s.background, s.at(k), 0.25 + 0.55 * k)
    });
}

// -- loudness ------------------------------------------------------------------------

/// The loudness scale of the meters: -60..0 LUFS.
const LOUD_LOW: f32 = -60.0;

fn loudness(f: &Frame<'_>, cv: &mut Canvas, st: &State) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let m = &a.loudness;
    let r = f.rect;
    let meters_w = (r.width() * 0.36).clamp(120.0, 260.0);
    let meters = Rect::from_min_max(
        pos2(r.left() + 30.0, r.top() + 22.0),
        pos2(r.left() + meters_w, r.bottom() - 8.0),
    );
    let y_of = |lufs: f32| {
        let t = if lufs.is_finite() {
            ((lufs - LOUD_LOW) / -LOUD_LOW).clamp(0.0, 1.0)
        } else {
            0.0
        };
        meters.bottom() - t * meters.height()
    };
    let values = [
        ("M", m.momentary, m.momentary_max),
        ("S", m.short_term, m.short_term_max),
        ("I", m.integrated, f32::NEG_INFINITY),
    ];
    let column = meters.width() / 3.0;
    let right = Rect::from_min_max(
        pos2(meters.right() + 16.0, r.top() + 8.0),
        pos2(r.right() - 8.0, r.bottom() - 8.0),
    );
    let graph = Rect::from_min_max(
        pos2(right.left(), right.top() + right.height() * 0.5 + 6.0),
        right.max,
    );
    cv.draw(p, |pen| {
        let mut l = 0.0;
        while l >= LOUD_LOW {
            let y = y_of(l);
            pen.rect(
                Rect::from_x_y_ranges(meters.left() - 4.0..=meters.right(), y - 0.5..=y + 0.5),
                s.grid(),
            );
            l -= 6.0;
        }
        for (k, (_, value, max)) in values.iter().enumerate() {
            let bar = Rect::from_x_y_ranges(
                meters.left() + k as f32 * column + 6.0
                    ..=meters.left() + (k as f32 + 1.0) * column - 6.0,
                meters.y_range(),
            );
            pen.rect(bar, s.track());
            let top = y_of(*value);
            // the colour of the loudness: quiet, the target (-23 LUFS), loud
            let lit = Rect::from_x_y_ranges(bar.x_range(), top..=bar.bottom());
            pen.gradient(lit, s, bar.bottom(), bar.top());
            if max.is_finite() {
                let y = y_of(*max);
                pen.rect(
                    Rect::from_x_y_ranges(bar.x_range(), y - 1.0..=y + 1.0),
                    s.peak,
                );
            }
        }
        // EBU R128's target
        let y = y_of(f.loudness_target);
        pen.rect(
            Rect::from_x_y_ranges(
                meters.left() - 6.0..=meters.right() + 4.0,
                y - 1.0..=y + 1.0,
            ),
            s.peak.gamma_multiply(0.8),
        );
        // the short-term loudness of the last minute
        if graph.height() > 20.0 {
            pen.rect(graph, s.track());
            let gy = |l: f32| {
                graph.bottom() - (((l - LOUD_LOW) / -LOUD_LOW).clamp(0.0, 1.0)) * graph.height()
            };
            let t = gy(f.loudness_target);
            pen.rect(
                Rect::from_x_y_ranges(graph.x_range(), t - 0.5..=t + 0.5),
                s.peak.gamma_multiply(0.5),
            );
            pen.points.clear();
            for k in 0..LOUD_STEPS {
                let v = st.loudness[(st.loudness_at + 1 + k) % LOUD_STEPS];
                let x = graph.left() + k as f32 / (LOUD_STEPS - 1) as f32 * graph.width();
                if v.is_finite() {
                    pen.points.push(pos2(x, gy(v)));
                } else if pen.points.len() > 1 {
                    under_gradient(pen, s, graph.bottom(), graph.bottom(), graph.top());
                    pen.line(1.2, s.trace());
                    pen.points.clear();
                } else {
                    pen.points.clear();
                }
            }
            if pen.points.len() > 1 {
                under_gradient(pen, s, graph.bottom(), graph.bottom(), graph.top());
                pen.line(1.2, s.trace());
            }
        }
    });
    // the target, named
    let target_name = crate::state::LOUDNESS_TARGETS
        .iter()
        .find(|(t, _)| *t == f.loudness_target)
        .map(|(_, n)| *n)
        .unwrap_or("target");
    if graph.height() > 20.0 {
        let gy = graph.bottom()
            - (((f.loudness_target - LOUD_LOW) / -LOUD_LOW).clamp(0.0, 1.0)) * graph.height();
        label(
            p,
            pos2(graph.left() + 4.0, gy - 2.0),
            Align2::LEFT_BOTTOM,
            format!("target {:.0} LUFS - {target_name}", f.loudness_target),
            10.0,
            s.peak,
        );
        label(
            p,
            pos2(graph.right() - 4.0, graph.top() + 2.0),
            Align2::RIGHT_TOP,
            "short-term, the last minute",
            10.0,
            s.text(),
        );
    }
    for (k, (name, _, _)) in values.iter().enumerate() {
        label(
            p,
            pos2(meters.left() + (k as f32 + 0.5) * column, r.top() + 4.0),
            Align2::CENTER_TOP,
            *name,
            12.0,
            s.text(),
        );
    }
    let mut l = 0.0;
    while l >= LOUD_LOW {
        label(
            p,
            pos2(meters.left() - 6.0, y_of(l)),
            Align2::RIGHT_CENTER,
            db(l).trim_end_matches(".0"),
            10.0,
            s.text(),
        );
        l -= 12.0;
    }
    // the numbers
    let big = (right.height() * 0.16).clamp(16.0, 46.0);
    let mut y = right.top();
    mono_label(
        p,
        pos2(right.left(), y),
        Align2::LEFT_TOP,
        format!("{} LUFS", db(m.integrated)),
        big,
        s.peak,
    );
    y += big * 1.15;
    label(
        p,
        pos2(right.left(), y),
        Align2::LEFT_TOP,
        "integrated (EBU R128)",
        11.0,
        s.text(),
    );
    y += 18.0;
    let row = (graph.top() - y - 4.0) / 3.0;
    let size = row.clamp(9.0, 14.0) * 0.9;
    let tp_colour = if m.true_peak > -1.0 { s.high } else { s.text() };
    let lines = [
        (
            format!("M {}  S {}", db(m.momentary), db(m.short_term)),
            s.text(),
        ),
        (
            format!("LRA {} LU · measured {}", db(m.range), clock(m.seconds)),
            s.text(),
        ),
        (format!("True peak {} dBTP", db(m.true_peak)), tp_colour),
    ];
    if row >= 9.0 {
        for (text, colour) in lines {
            mono_label(
                p,
                pos2(right.left(), y),
                Align2::LEFT_TOP,
                text,
                size,
                colour,
            );
            y += row;
        }
    }
}

// -- phase and width -----------------------------------------------------------------

fn phase(f: &Frame<'_>, cv: &mut Canvas, st: &State) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let r = f.rect.shrink2(vec2(12.0, 6.0));
    let row = ((r.height() * 0.5) / 3.0).max(12.0);
    let meter = |k: usize| {
        Rect::from_min_size(
            pos2(r.left() + 70.0, r.top() + k as f32 * row + 4.0),
            vec2(r.width() - 130.0, row * 0.55),
        )
    };
    let graph = Rect::from_min_max(
        pos2(r.left() + 70.0, r.top() + 3.0 * row + 8.0),
        pos2(r.right() - 60.0, r.bottom()),
    );
    // (value, from, to, centred)
    let meters = [
        ("Correlation", a.correlation, -1.0, 1.0),
        ("Width", a.width, 0.0, 1.0),
        ("Balance", a.balance, -1.0, 1.0),
    ];
    cv.draw(p, |pen| {
        for (k, (_, value, from, to)) in meters.iter().enumerate() {
            let m = meter(k);
            pen.rect(m, s.track());
            let x_of = |v: f32| m.left() + ((v - from) / (to - from)).clamp(0.0, 1.0) * m.width();
            let zero = x_of(if *from < 0.0 { 0.0 } else { *from });
            let x = x_of(*value);
            // correlation: below zero (out of phase) in the loud colour
            let colour = if k == 0 && *value < 0.0 {
                s.high
            } else {
                s.at(0.35 + 0.4 * value.abs())
            };
            pen.rect(
                Rect::from_x_y_ranges(zero.min(x)..=zero.max(x), m.y_range()),
                colour,
            );
            pen.rect(
                Rect::from_x_y_ranges(zero - 0.5..=zero + 0.5, m.top() - 3.0..=m.bottom() + 3.0),
                s.grid(),
            );
            pen.rect(
                Rect::from_x_y_ranges(x - 1.5..=x + 1.5, m.top() - 3.0..=m.bottom() + 3.0),
                s.peak,
            );
        }
        if graph.height() > 16.0 {
            pen.rect(graph, s.track());
            let mid = graph.center().y;
            pen.rect(
                Rect::from_x_y_ranges(graph.x_range(), mid - 0.5..=mid + 0.5),
                s.grid(),
            );
            // the width as a band, the correlation as a line over it
            for k in 1..PHASE_STEPS {
                let v0 = st.phase[(st.phase_at + k) % PHASE_STEPS];
                let v1 = st.phase[(st.phase_at + k + 1) % PHASE_STEPS];
                let x0 = graph.left() + (k - 1) as f32 / (PHASE_STEPS - 1) as f32 * graph.width();
                let x1 = graph.left() + k as f32 / (PHASE_STEPS - 1) as f32 * graph.width();
                let h0 = v0[1] * graph.height() * 0.5;
                let h1 = v1[1] * graph.height() * 0.5;
                let c = mix(s.background, s.low, 0.45);
                pen.quad(
                    [
                        pos2(x0, mid - h0),
                        pos2(x1, mid - h1),
                        pos2(x1, mid + h1),
                        pos2(x0, mid + h0),
                    ],
                    [c; 4],
                );
            }
            pen.points.clear();
            for k in 0..PHASE_STEPS {
                let v = st.phase[(st.phase_at + 1 + k) % PHASE_STEPS][0];
                pen.points.push(pos2(
                    graph.left() + k as f32 / (PHASE_STEPS - 1) as f32 * graph.width(),
                    mid - v * graph.height() * 0.5,
                ));
            }
            pen.line(1.4, s.trace());
        }
    });
    for (k, (name, value, _, _)) in meters.iter().enumerate() {
        let m = meter(k);
        label(
            p,
            pos2(r.left(), m.center().y),
            Align2::LEFT_CENTER,
            *name,
            11.0,
            s.text(),
        );
        let text = if k == 1 {
            format!("{:.0} %", value * 100.0)
        } else {
            format!("{value:+.2}")
        };
        mono_label(
            p,
            pos2(r.right(), m.center().y),
            Align2::RIGHT_CENTER,
            text,
            12.0,
            s.peak,
        );
    }
    if graph.height() > 16.0 {
        label(
            p,
            pos2(r.left(), graph.top()),
            Align2::LEFT_TOP,
            "+1",
            10.0,
            s.text(),
        );
        label(
            p,
            pos2(r.left(), graph.bottom()),
            Align2::LEFT_BOTTOM,
            "-1",
            10.0,
            s.text(),
        );
        label(
            p,
            pos2(r.right(), graph.top()),
            Align2::RIGHT_TOP,
            "10 s",
            10.0,
            s.text(),
        );
    }
}

// -- the goniometer ------------------------------------------------------------------

fn goniometer(f: &Frame<'_>, state: &mut VizState) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let st = &mut state.analysis;
    let c = f.rect.center();
    let reach = (f.rect.width().min(f.rect.height()) / 2.0 - 16.0).max(10.0);
    // a gentle automatic gain: quiet music fills the screen too
    let loudest = a
        .stereo
        .iter()
        .fold(0.0f32, |m, [l, r]| m.max(l.abs()).max(r.abs()));
    let want = (0.7 / loudest.max(0.02)).clamp(1.0, 8.0);
    let follow = 1.0 - (-f.dt * 3.0).exp();
    st.gain += (want - st.gain) * if want < st.gain { 1.0 } else { follow };
    // the phosphor: what the beam drew fades (a tenth of a second or so),
    // each sample lights where it falls (spread over four pixels)
    if st.phosphor.len() != PHOSPHOR * PHOSPHOR {
        st.phosphor = vec![0.0; PHOSPHOR * PHOSPHOR];
    }
    let fade = (-f.dt * 9.0).exp();
    for v in st.phosphor.iter_mut() {
        *v *= fade;
    }
    let root = std::f32::consts::FRAC_1_SQRT_2;
    let size = PHOSPHOR as f32;
    for [l, r] in &a.stereo {
        let x = ((r - l) * root * st.gain).clamp(-1.0, 1.0);
        let y = ((l + r) * root * st.gain).clamp(-1.0, 1.0);
        let (fx, fy) = (
            (x * 0.5 + 0.5) * (size - 1.0),
            (0.5 - y * 0.5) * (size - 1.0),
        );
        let (i, j) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        for (di, dj, w) in [
            (0, 0, (1.0 - tx) * (1.0 - ty)),
            (1, 0, tx * (1.0 - ty)),
            (0, 1, (1.0 - tx) * ty),
            (1, 1, tx * ty),
        ] {
            let (ii, jj) = ((i + di).min(PHOSPHOR - 1), (j + dj).min(PHOSPHOR - 1));
            let v = &mut st.phosphor[jj * PHOSPHOR + ii];
            *v = (*v + 0.18 * w).min(1.0);
        }
    }
    if let Ok(mut picture) = state.gpu.picture(f.second).lock() {
        if picture.data.len() != PHOSPHOR * PHOSPHOR {
            picture.data.clear();
            picture.data.resize(PHOSPHOR * PHOSPHOR, 0);
            picture.width = PHOSPHOR as u32;
            picture.height = PHOSPHOR as u32;
        }
        for (d, v) in picture.data.iter_mut().zip(&st.phosphor) {
            *d = (v * 255.0) as u8;
        }
        picture.version = picture.version.wrapping_add(1);
    }
    let square = Rect::from_center_size(c, vec2(reach * 2.0, reach * 2.0));
    let u = gpu::Uniforms::new(gpu::Shader::Phosphor, a, s, 0.0).with_opacity(f.opacity);
    let (gpu, second, cv, live) = (&mut state.gpu, f.second, &mut state.canvas, f.dt > 0.0);
    cv.draw(p, |pen| {
        gpu::paint(p, pen, gpu, square, square, u, true, second, live);
    });
    cv.draw(p, |pen| {
        pen.ring(c, reach, 1.0, s.grid());
        let d = reach * root;
        pen.segment(c - vec2(reach, 0.0), c + vec2(reach, 0.0), 1.0, s.grid());
        pen.segment(c - vec2(0.0, reach), c + vec2(0.0, reach), 1.0, s.grid());
        pen.segment(c + vec2(-d, -d), c + vec2(d, d), 1.0, s.grid());
        pen.segment(c + vec2(d, -d), c + vec2(-d, d), 1.0, s.grid());
        // the correlation beside it
        let bar = Rect::from_min_size(
            pos2(f.rect.right() - 12.0, c.y - reach),
            vec2(5.0, reach * 2.0),
        );
        pen.rect(bar, s.track());
        let y = c.y - a.correlation * reach;
        let colour = if a.correlation < 0.0 { s.high } else { s.mid };
        pen.rect(
            Rect::from_x_y_ranges(bar.x_range(), y.min(c.y)..=y.max(c.y)),
            colour,
        );
    });
    let st = &state.analysis;
    let d = reach * root;
    for (text, at, align) in [
        ("L", c + vec2(-d - 4.0, -d - 4.0), Align2::RIGHT_BOTTOM),
        ("R", c + vec2(d + 4.0, -d - 4.0), Align2::LEFT_BOTTOM),
        ("M", c + vec2(0.0, -reach - 2.0), Align2::CENTER_BOTTOM),
        ("+S", c + vec2(-reach - 4.0, 0.0), Align2::RIGHT_CENTER),
        ("-S", c + vec2(reach + 4.0, 0.0), Align2::LEFT_CENTER),
    ] {
        label(p, at, align, text, 11.0, s.text());
    }
    if st.gain > 1.05 {
        mono_label(
            p,
            f.rect.left_bottom() + vec2(4.0, -4.0),
            Align2::LEFT_BOTTOM,
            format!("x{:.1}", st.gain),
            11.0,
            s.text(),
        );
    }
}

// -- the track's waveform ------------------------------------------------------------

fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Where a click at `x` in the overview's picture seeks to (0..1).
pub fn overview_share(rect: Rect, x: f32) -> f32 {
    ((x - rect.left()) / rect.width()).clamp(0.0, 1.0)
}

/// The waveform's place in the overview face drawn into `rect` (the
/// whole picture, as [`super::draw`] is given it).
fn overview_wave(rect: Rect) -> Rect {
    let r = rect.shrink(4.0);
    Rect::from_min_max(
        pos2(r.left(), r.top() + 18.0),
        pos2(r.right(), r.bottom() - 16.0),
    )
}

/// Where in the track (0..1) a click at `at` on the overview face in
/// `rect` goes, if it is on the waveform.
pub fn overview_seek(rect: Rect, at: Pos2) -> Option<f32> {
    let wave = overview_wave(rect);
    wave.expand2(vec2(0.0, 8.0))
        .contains(at)
        .then(|| overview_share(wave, at.x))
}

fn overview(f: &Frame<'_>, cv: &mut Canvas, st: &State) {
    let (s, p) = (f.s, f.painter);
    let r = f.rect;
    let wave = overview_wave(f.rect.expand(4.0));
    let mid = wave.center().y;
    let half = wave.height() / 2.0;
    let Some(view) = f.overview.filter(|o| !o.columns.is_empty()) else {
        // a stream (or not read yet): what was heard lately, scrolling
        cv.draw(p, |pen| {
            pen.rect(
                Rect::from_x_y_ranges(wave.x_range(), mid - 0.5..=mid + 0.5),
                s.grid(),
            );
            for k in 0..WAVE_STEPS {
                let [lo, hi] = st.waves[(st.waves_at + 1 + k) % WAVE_STEPS];
                let x = wave.left() + k as f32 / WAVE_STEPS as f32 * wave.width();
                let w = wave.width() / WAVE_STEPS as f32 + 0.5;
                if hi > lo {
                    pen.rect(
                        Rect::from_x_y_ranges(
                            x..=x + w,
                            mid - hi.clamp(-1.0, 1.0) * half..=mid - lo.clamp(-1.0, 1.0) * half,
                        ),
                        s.at(0.3 + 0.7 * k as f32 / WAVE_STEPS as f32),
                    );
                }
            }
        });
        let text = if f.overview.is_some() {
            "Reading the track…"
        } else {
            "What was heard, the last 20 s"
        };
        label(
            p,
            r.left_top() + vec2(2.0, 0.0),
            Align2::LEFT_TOP,
            text,
            11.0,
            s.text(),
        );
        return;
    };
    let played = if f.duration > 0.0 {
        (f.position / f.duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let columns = &view.columns;
    let n = columns.len();
    let pixels = (wave.width() / 2.0).max(1.0) as usize;
    cv.draw(p, |pen| {
        pen.rect(
            Rect::from_x_y_ranges(wave.x_range(), mid - 0.5..=mid + 0.5),
            s.grid(),
        );
        for px in 0..pixels {
            let (from, to) = (
                px * n / pixels,
                ((px + 1) * n / pixels).max(px * n / pixels + 1).min(n),
            );
            let (mut lo, mut hi, mut rms) = (0.0f32, 0.0f32, 0.0f32);
            for c in &columns[from..to] {
                lo = lo.min(c[0]);
                hi = hi.max(c[1]);
                rms = rms.max(c[2]);
            }
            let share = px as f32 / pixels as f32;
            let x = wave.left() + share * wave.width();
            let w = wave.width() / pixels as f32 - 0.6;
            let done = share <= view.done;
            let outer = if !done {
                s.track()
            } else if share < played {
                s.at(0.35 + 0.65 * rms.min(1.0).sqrt())
            } else {
                mix(s.background, s.low, 0.45)
            };
            pen.rect(
                Rect::from_x_y_ranges(x..=x + w.max(0.6), mid - hi * half..=mid - lo * half),
                outer,
            );
            let inner = if share < played {
                s.peak.gamma_multiply(0.55)
            } else {
                mix(s.background, s.mid, 0.4)
            };
            if done {
                pen.rect(
                    Rect::from_x_y_ranges(x..=x + w.max(0.6), mid - rms * half..=mid + rms * half),
                    inner,
                );
            }
        }
        let x = wave.left() + played * wave.width();
        pen.rect(
            Rect::from_x_y_ranges(x - 1.0..=x + 1.0, wave.top() - 4.0..=wave.bottom() + 4.0),
            s.peak,
        );
        if let Some(h) = f.hover.filter(|h| wave.contains(*h)) {
            pen.rect(
                Rect::from_x_y_ranges(h.x - 0.5..=h.x + 0.5, wave.y_range()),
                s.text(),
            );
        }
    });
    label(
        p,
        r.left_top() + vec2(2.0, 0.0),
        Align2::LEFT_TOP,
        clock(f.position),
        12.0,
        s.peak,
    );
    label(
        p,
        r.right_top() + vec2(-2.0, 0.0),
        Align2::RIGHT_TOP,
        clock(f.duration),
        12.0,
        s.text(),
    );
    if view.done < 1.0 {
        label(
            p,
            r.center_top(),
            Align2::CENTER_TOP,
            format!("Reading the track… {:.0} %", view.done * 100.0),
            11.0,
            s.text(),
        );
    }
    if let Some(h) = f.hover.filter(|h| wave.contains(*h)) {
        let at = overview_share(wave, h.x) * f.duration;
        badge(
            p,
            pos2(h.x, wave.bottom() + 1.0),
            Align2::CENTER_TOP,
            &format!("{} – click to go there", clock(at)),
            s,
        );
    }
    // the minutes along the bottom
    if f.duration > 0.0 {
        let step = [10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0]
            .into_iter()
            .find(|step| f.duration / step <= wave.width() / 70.0)
            .unwrap_or(3600.0);
        let mut t = step;
        while t < f.duration {
            let x = wave.left() + t / f.duration * wave.width();
            label(
                p,
                pos2(x, r.bottom()),
                Align2::CENTER_BOTTOM,
                clock(t),
                10.0,
                s.text(),
            );
            t += step;
        }
    }
}

// -- the chromagram ------------------------------------------------------------------

fn chroma(f: &Frame<'_>, cv: &mut Canvas, st: &mut State, dt: f32) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let fine = &a.fine;
    // the key: the whole track's notes against the key profiles (the
    // meters', which heard all of it); without them, what this face saw
    let keep = (-dt / 30.0).exp();
    for (avg, c) in st.chroma.iter_mut().zip(fine.chroma) {
        *avg = *avg * keep + c * (1.0 - keep);
    }
    st.key = a.key().unwrap_or_else(|| {
        let chroma = st.chroma.map(|v| v as f64);
        crate::meters::key_of(&chroma)
    });
    let r = f.rect;
    let wheel = (r.height() - 8.0).min(r.width() * 0.42);
    let grid = Rect::from_min_max(
        pos2(r.left() + 24.0, r.top() + 4.0),
        pos2(r.right() - wheel - 16.0, r.bottom() - 4.0),
    );
    let row = grid.height() / 12.0;
    let steps = ((grid.width() / 3.0) as usize).clamp(16, fine::STEPS);
    let cell = grid.width() / steps as f32;
    let shift = fine.step_fraction() * cell;
    let painter = p.with_clip_rect(grid.intersect(p.clip_rect()));
    cv.draw(&painter, |pen| {
        for age in 0..=steps {
            let x = grid.right() - (age + 1) as f32 * cell - shift;
            let col = fine.chroma_row(age);
            for (k, v) in col.iter().enumerate() {
                if *v < 0.05 {
                    continue;
                }
                let y = grid.bottom() - (k + 1) as f32 * row;
                pen.rect(
                    Rect::from_min_size(pos2(x, y + 0.5), vec2(cell + 0.4, row - 1.0)),
                    mix(s.background, s.at(*v), *v),
                );
            }
        }
    });
    for (k, name) in fine::NOTE_NAMES.iter().enumerate() {
        let y = grid.bottom() - (k as f32 + 0.5) * row;
        let colour = if fine.chroma[k] > 0.7 {
            s.peak
        } else {
            s.text()
        };
        label(
            p,
            pos2(r.left() + 2.0, y),
            Align2::LEFT_CENTER,
            *name,
            (row * 0.75).clamp(8.0, 12.0),
            colour,
        );
    }
    // the circle of fifths
    let c = pos2(r.right() - wheel / 2.0 - 6.0, r.center().y);
    let outer = wheel / 2.0 - 2.0;
    let inner = outer * 0.55;
    cv.draw(p, |pen| {
        for (slot, class) in FIFTHS.iter().enumerate() {
            let v = fine.chroma[*class];
            let a0 = -PI / 2.0 + (slot as f32 - 0.5) / 12.0 * TAU + 0.02;
            let a1 = -PI / 2.0 + (slot as f32 + 0.5) / 12.0 * TAU - 0.02;
            let colour = mix(s.track(), s.at(v), v);
            let steps = 8;
            for k in 0..steps {
                let (t0, t1) = (
                    a0 + (a1 - a0) * k as f32 / steps as f32,
                    a0 + (a1 - a0) * (k + 1) as f32 / steps as f32,
                );
                let u0 = vec2(t0.cos(), t0.sin());
                let u1 = vec2(t1.cos(), t1.sin());
                pen.quad(
                    [
                        c + u0 * inner,
                        c + u1 * inner,
                        c + u1 * outer,
                        c + u0 * outer,
                    ],
                    [colour; 4],
                );
            }
        }
        // the key's place on the circle
        let (root, _, sure) = st.key;
        if sure >= KEY_SURE {
            let slot = FIFTHS.iter().position(|c| *c == root).unwrap_or(0);
            let angle = -PI / 2.0 + slot as f32 / 12.0 * TAU;
            pen.circle(
                c + vec2(angle.cos(), angle.sin()) * (inner - 7.0),
                4.0,
                s.peak,
            );
        }
    });
    for (slot, class) in FIFTHS.iter().enumerate() {
        let angle = -PI / 2.0 + slot as f32 / 12.0 * TAU;
        let at = c + vec2(angle.cos(), angle.sin()) * (inner + outer) / 2.0;
        let v = fine.chroma[*class];
        let colour = if v > 0.6 { s.background } else { s.text() };
        label(
            p,
            at,
            Align2::CENTER_CENTER,
            fine::NOTE_NAMES[*class],
            (outer * 0.13).clamp(8.0, 14.0),
            colour,
        );
    }
    // the key and how sure it is (a "?" until it is)
    let (root, minor, sure) = st.key;
    let name = if sure >= KEY_SURE {
        format!(
            "{} {}",
            fine::NOTE_NAMES[root],
            if minor { "minor" } else { "major" }
        )
    } else {
        "key ?".to_owned()
    };
    label(
        p,
        c - vec2(0.0, 6.0),
        Align2::CENTER_CENTER,
        name,
        (inner * 0.3).clamp(10.0, 20.0),
        s.peak,
    );
    label(
        p,
        c + vec2(0.0, (inner * 0.3).clamp(10.0, 20.0) * 0.6 + 2.0),
        Align2::CENTER_CENTER,
        format!("{:.0} % sure", sure * 100.0),
        10.0,
        s.text(),
    );
}

/// How sure the key must be to be named.
const KEY_SURE: f32 = 0.2;

// -- the piano roll ------------------------------------------------------------------

fn black(midi: usize) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

fn pianoroll(f: &Frame<'_>, cv: &mut Canvas) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let fine = &a.fine;
    let r = f.rect;
    let keys = ROLL_HIGH - ROLL_LOW + 1;
    let row = r.height() / keys as f32;
    let board = Rect::from_min_max(r.min, pos2(r.left() + 40.0, r.bottom()));
    let roll = Rect::from_min_max(pos2(board.right() + 2.0, r.top()), r.max);
    let steps = ((roll.width() / 3.0) as usize).clamp(16, fine::STEPS);
    let cell = roll.width() / steps as f32;
    let shift = fine.step_fraction() * cell;
    let y_of = |midi: usize| r.bottom() - (midi - ROLL_LOW + 1) as f32 * row;
    // a key sounds: loud, and louder than the keys beside it
    let sounds = |levels: &[f32; fine::KEYS], midi: usize| {
        let k = midi - 21;
        let v = levels[k];
        let left = if k > 0 { levels[k - 1] } else { 0.0 };
        let right = if k + 1 < fine::KEYS {
            levels[k + 1]
        } else {
            0.0
        };
        if v > 0.55 && v >= left && v >= right {
            v
        } else {
            0.0
        }
    };
    cv.draw(p, |pen| {
        for midi in ROLL_LOW..=ROLL_HIGH {
            let y = y_of(midi);
            let lane = if black(midi) {
                mix(s.background, s.track(), 0.5)
            } else {
                s.track()
            };
            pen.rect(Rect::from_x_y_ranges(roll.x_range(), y..=y + row), lane);
            if midi % 12 == 0 {
                pen.rect(
                    Rect::from_x_y_ranges(roll.x_range(), y + row - 0.5..=y + row + 0.5),
                    s.grid(),
                );
            }
            let v = sounds(&fine.keys, midi);
            let key = if v > 0.0 {
                s.at(v)
            } else if black(midi) {
                mix(s.background, Color32::BLACK, 0.4)
            } else {
                mix(s.background, s.peak, 0.75)
            };
            let width = if black(midi) {
                board.width() * 0.62
            } else {
                board.width()
            };
            pen.rect(
                Rect::from_min_size(
                    pos2(board.left(), y + 0.5),
                    vec2(width, (row - 1.0).max(0.5)),
                ),
                key,
            );
        }
    });
    let painter = p.with_clip_rect(roll.intersect(p.clip_rect()));
    cv.draw(&painter, |pen| {
        for age in 0..=steps {
            let x = roll.right() - (age + 1) as f32 * cell - shift;
            let levels = fine.key_row(age);
            for midi in ROLL_LOW..=ROLL_HIGH {
                let v = sounds(levels, midi);
                if v > 0.0 {
                    let y = y_of(midi);
                    pen.rect(
                        Rect::from_min_size(pos2(x, y), vec2(cell + 0.4, row)),
                        mix(s.background, s.at(v), (v - 0.4) / 0.6),
                    );
                }
            }
        }
    });
    for octave in (ROLL_LOW..=ROLL_HIGH).step_by(12) {
        label(
            p,
            pos2(roll.left() + 3.0, y_of(octave) + row / 2.0),
            Align2::LEFT_CENTER,
            fine::note_name(octave as i32),
            (row * 1.6).clamp(8.0, 11.0),
            s.text(),
        );
    }
}

// -- the mel spectrogram -------------------------------------------------------------

/// The waterfall smooth, at high resolution: the fine spectrum's log bands
/// across, a row a step, the newest at the bottom, in axross's waterfall
/// colours (a picture the shader scales up smoothly).
pub fn waterfall_hd(f: &Frame<'_>, state: &mut VizState) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let fine = &a.fine;
    let st = &mut state.analysis;
    let steps = (a.time * 30.0) as u64;
    if let Ok(mut picture) = state.gpu.picture(f.second).lock() {
        let (w, h) = (fine::HD, fine::STEPS);
        if picture.data.len() != w * h || st.hd_picture != (w, h) {
            picture.data.clear();
            picture.data.resize(w * h, 0);
            picture.width = w as u32;
            picture.height = h as u32;
            st.hd_picture = (w, h);
            st.picture_step = u64::MAX;
        }
        if st.picture_step != steps {
            st.picture_step = steps;
            for y in 0..h {
                let row = fine.hd_row(h - 1 - y);
                for (x, v) in row.iter().enumerate() {
                    picture.data[y * w + x] = (v * 255.0) as u8;
                }
            }
            picture.version = picture.version.wrapping_add(1);
        }
    }
    let u = gpu::Uniforms::new(gpu::Shader::Spectrogram, a, s, fine.step_fraction())
        .with_opacity(f.opacity);
    let clip = f.rect.expand(4.0);
    let (gpu, second, live, rect) = (&mut state.gpu, f.second, f.dt > 0.0, f.rect);
    state.canvas.draw(p, |pen| {
        gpu::paint(p, pen, gpu, rect, clip, u, true, second, live);
    });
}

fn mel(f: &Frame<'_>, state: &mut VizState) {
    let (a, s, p) = (f.a, f.s, f.painter);
    let fine = &a.fine;
    let st = &mut state.analysis;
    // the picture: a column a step, the newest at the right, high bands on top
    let steps = (a.time * 30.0) as u64;
    if let Ok(mut picture) = state.gpu.picture(f.second).lock() {
        let (w, h) = (fine::STEPS, fine::MELS);
        if picture.data.len() != w * h {
            picture.data.clear();
            picture.data.resize(w * h, 0);
            picture.width = w as u32;
            picture.height = h as u32;
            st.picture_step = u64::MAX;
        }
        if st.picture_step != steps {
            st.picture_step = steps;
            for x in 0..w {
                let col = fine.mel_row(w - 1 - x);
                for y in 0..h {
                    picture.data[y * w + x] = (col[h - 1 - y] * 255.0) as u8;
                }
            }
            picture.version = picture.version.wrapping_add(1);
        }
    }
    let r = f.rect;
    let plot = Rect::from_min_max(pos2(r.left() + 40.0, r.top()), r.max);
    let u =
        gpu::Uniforms::new(gpu::Shader::Mel, a, s, fine.step_fraction()).with_opacity(f.opacity);
    let clip = f.rect.expand(4.0);
    let (gpu, second, live) = (&mut state.gpu, f.second, f.dt > 0.0);
    state.canvas.draw(p, |pen| {
        gpu::paint(p, pen, gpu, plot, clip, u, true, second, live);
    });
    // the frequencies of some bands
    let mel_of = |hz: f32| 2595.0 * (1.0 + hz / 700.0).log10();
    let top = 16_000f32.min(a.rate as f32 * 0.48);
    let (lo, hi) = (mel_of(30.0), mel_of(top));
    for (hz, text) in [
        (100.0, "100"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (4000.0, "4k"),
        (8000.0, "8k"),
    ] {
        let t = (mel_of(hz) - lo) / (hi - lo);
        let y = plot.bottom() - t * plot.height();
        label(
            p,
            pos2(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            text,
            10.0,
            s.text(),
        );
    }
    let seconds = fine::STEPS as f32 / 30.0;
    label(
        p,
        pos2(plot.left() + 3.0, plot.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        format!("-{seconds:.1} s"),
        10.0,
        s.peak,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_on_the_note_has_no_minus_zero() {
        assert_eq!(note_text(440.0), "A4 ±0 ct");
        assert_eq!(note_text(440.0 * 2f32.powf(-0.2 / 1200.0)), "A4 ±0 ct");
        assert!(note_text(445.0).starts_with("A4 +"));
        assert!(note_text(435.0).starts_with("A4 -"));
    }
}
