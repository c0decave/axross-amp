//! The flash guard at the output: whatever a face draws, what is shown may
//! not flash faster than WCAG 2.3.1 allows (three flashes - six
//! transitions - in any second), nor flash in saturated red (2.3.2).
//!
//! After the faces have drawn a frame, the guard estimates the picture on
//! a grid of cells (128 across): the frame's shapes, tessellated, laid onto
//! the cells in the order they are painted (a glyph as the share of its box
//! ink covers; a shader face by its own function, evaluated on the CPU on a
//! coarser grid - the same the CPU's fallback draws - where its paint
//! callback or picture is). Each cell's relative luminance and saturated
//! red are followed in time; a *field* - a third of the picture each way,
//! a 10° view when the face fills the screen - transits when a quarter of
//! its area transits at once (a cell counted whole where its large content
//! changed, else as much as small shapes cover of it). Where a field has
//! transited five times in the last second (raw, as the face draws), the
//! guard lays a veil over it: each cell blended towards a slow average of
//! itself (persistence), up to 85 % - 95 % where the veiled picture itself
//! still transits - easing off a second after the danger is gone. The veil
//! is a mesh over the picture, shaded smoothly and feathered (the CPU's
//! fallback and the GPU alike).

use super::gpu::{self, Uniforms};
use egui::{Color32, Pos2, Rect, epaint::Mesh};

/// Cells across (the rows follow the picture's shape).
const COLS: usize = 128;
/// Shader faces are evaluated on this coarser grid (cells take the
/// nearest).
const SHADE_COLS: usize = 48;
/// How long a transition stays "at once" with others: 100 ms - six frames
/// at 60 a second, three at 30 (counted in frames, not compared in seconds:
/// a float compare against a frame's length was a coin toss). The stricter
/// of the two readings the tests apply (the other: 34 ms).
const AT_ONCE: f32 = 0.1;
/// The screen the fields are a third of, each way, when the monitor's size
/// is not known (points).
const SCREEN: egui::Vec2 = egui::vec2(1920.0, 1080.0);
/// A shape's coverage taken a little larger than its geometry (a line a
/// pixel wide is drawn feathered, over more).
const SPREAD: f32 = 1.0;
/// A transition in a cell's relative luminance (WCAG's 0.1).
const STEP: f32 = 0.1;
/// Transitions in a field's last second (every rise, 100 ms "at once")
/// from which the veil comes: seven - over the rule. The faces keep within
/// it by themselves; the veil is for what they do not foresee, and comes
/// no sooner, for its own coming and going are transitions too (at five or
/// six, counted strictly, it came for faces within the rule, and its
/// coming and going broke the rule where they had not).
const RAW_BUDGET: usize = 7;
/// And from which, seen through the veil, it closes further.
const SHOWN_BUDGET: usize = 7;

#[derive(Clone, Copy, Default)]
struct Track {
    extreme: f32,
    dir: i8,
    started: bool,
}

impl Track {
    /// A new value: +1 / -1 where it makes a transition (that counts).
    fn step(&mut self, v: f32, step: f32, darker: Option<f32>) -> i8 {
        if !self.started {
            self.started = true;
            self.extreme = v;
            return 0;
        }
        let dark = |x: f32| darker.is_none_or(|d| x < d);
        match self.dir {
            1 if v > self.extreme => self.extreme = v,
            -1 if v < self.extreme => self.extreme = v,
            1 | 0 if self.extreme - v >= step => {
                self.dir = -1;
                self.extreme = v;
                return if dark(v) { -1 } else { 0 };
            }
            -1 | 0 if v - self.extreme >= step => {
                let counts = dark(self.extreme);
                self.dir = 1;
                self.extreme = v;
                return if counts { 1 } else { 0 };
            }
            _ => {}
        }
        0
    }
}

/// Transitions of the cells followed on one picture (raw or shown).
#[derive(Default)]
struct Watch {
    lum: Vec<Track>,
    red: Vec<Track>,
    /// When each cell last transited (the frame), and which way.
    lum_at: Vec<u64>,
    red_at: Vec<u64>,
    lum_dir: Vec<i8>,
    red_dir: Vec<i8>,
    /// The same of the cells' large content alone (see `step`), and how
    /// much of a cell (sixteenths) its last transition is taken for.
    lum_big: Vec<Track>,
    red_big: Vec<Track>,
    lum_w: Vec<u8>,
    red_w: Vec<u8>,
    /// Each field: whether it was over the quarter (luminance up, down; red
    /// up, down); its transitions of luminance and of red (s) and the way
    /// the last of each went (a transition is a change the other way from
    /// the one before: two the same way are one, as WCAG counts flashes -
    /// pairs of opposing changes).
    over: Vec<[bool; 4]>,
    events: Vec<[Vec<f32>; 2]>,
    last: Vec<[i8; 2]>,
    /// Counts of this frame's changes, summed (four kinds a cell).
    integral: Vec<u32>,
}

impl Watch {
    fn reset(&mut self, cells: usize, fields: usize) {
        self.lum = vec![Track::default(); cells];
        self.red = vec![Track::default(); cells];
        self.lum_at = vec![u64::MAX; cells];
        self.red_at = vec![u64::MAX; cells];
        self.lum_dir = vec![0; cells];
        self.red_dir = vec![0; cells];
        self.lum_big = vec![Track::default(); cells];
        self.red_big = vec![Track::default(); cells];
        self.lum_w = vec![0; cells];
        self.red_w = vec![0; cells];
        self.over = vec![[false; 4]; fields];
        self.events = (0..fields)
            .map(|_| [Vec::with_capacity(16), Vec::with_capacity(16)])
            .collect();
        self.last = vec![[0; 2]; fields];
    }

    /// The same cells and fields on a new grid (each new one as the nearest
    /// old one): the history kept over a change of size.
    fn remap(&mut self, map: &Remap) {
        self.lum = map.cells(&self.lum);
        self.red = map.cells(&self.red);
        self.lum_at = map.cells(&self.lum_at);
        self.red_at = map.cells(&self.red_at);
        self.lum_dir = map.cells(&self.lum_dir);
        self.red_dir = map.cells(&self.red_dir);
        self.lum_big = map.cells(&self.lum_big);
        self.red_big = map.cells(&self.red_big);
        self.lum_w = map.cells(&self.lum_w);
        self.red_w = map.cells(&self.red_w);
        self.over = map.fields(&self.over);
        self.events = map.fields(&self.events);
        self.last = map.fields(&self.last);
    }

    /// The picture now (linear RGB a cell), the same of its large content
    /// alone (shader faces and shapes covering whole cells), and how much
    /// of each cell small shapes cover (0..1); the fields' transitions in
    /// the last second, after this frame.
    ///
    /// (WCAG's area is of the pixels that change: a cell whose average
    /// changes because a star a pixel or two wide moved in it has not
    /// changed all over. A transition of the large content counts the whole
    /// cell; one only of the whole picture counts as much of the cell as
    /// small shapes cover - else a starry sky, twinkling, read as a strobe.)
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        cells: &[[f32; 3]],
        big: &[[f32; 3]],
        small: &[f32],
        fields: &[Field],
        cols: usize,
        frame: u64,
        at_once: u64,
        now: f32,
        second: f32,
    ) {
        let luminance = |c: &[f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let red_of = |c: &[f32; 3]| red(c[0], c[1], c[2]);
        for (i, c) in cells.iter().enumerate() {
            let part = (small[i].min(1.0) * 16.0).ceil() as u8;
            let all = self.lum[i].step(luminance(c), STEP, Some(0.8));
            let large = self.lum_big[i].step(luminance(&big[i]), STEP, Some(0.8));
            if large != 0 {
                self.lum_at[i] = frame;
                self.lum_dir[i] = large;
                self.lum_w[i] = 16;
            } else if all != 0 && part > 0 {
                self.lum_at[i] = frame;
                self.lum_dir[i] = all;
                self.lum_w[i] = part;
            }
            // (WCAG's step of 20, a little less: the estimate is coarse)
            let all = self.red[i].step(red_of(c), 16.0, None);
            let large = self.red_big[i].step(red_of(&big[i]), 16.0, None);
            if large != 0 {
                self.red_at[i] = frame;
                self.red_dir[i] = large;
                self.red_w[i] = 16;
            } else if all != 0 && part > 0 {
                self.red_at[i] = frame;
                self.red_dir[i] = all;
                self.red_w[i] = part;
            }
        }
        // (a flash: an area changing together, the same way - counted in
        // each field by integral images of the four kinds of change)
        let rows = cells.len() / cols.max(1);
        let stride = cols + 1;
        self.integral.clear();
        self.integral.resize(stride * (rows + 1) * 4, 0);
        for y in 0..rows {
            let mut row = [0u32; 4];
            for x in 0..cols {
                let i = y * cols + x;
                if frame.wrapping_sub(self.lum_at[i]) < at_once {
                    row[usize::from(self.lum_dir[i] > 0)] += u32::from(self.lum_w[i]);
                }
                if frame.wrapping_sub(self.red_at[i]) < at_once {
                    row[2 + usize::from(self.red_dir[i] > 0)] += u32::from(self.red_w[i]);
                }
                for (k, r) in row.iter().enumerate() {
                    let at = ((y + 1) * stride + x + 1) * 4 + k;
                    self.integral[at] = self.integral[(y * stride + x + 1) * 4 + k] + r;
                }
            }
        }
        let integral = &self.integral;
        let sum = |(x, y, w, h): (usize, usize, usize, usize), k: usize| {
            let at = |x: usize, y: usize| integral[(y * stride + x) * 4 + k] as i64;
            (at(x + w, y + h) - at(x, y + h) - at(x + w, y) + at(x, y)) as usize
        };
        for (k, field) in fields.iter().enumerate() {
            let lum = [sum(field.at, 0), sum(field.at, 1)];
            let red = [sum(field.at, 2), sum(field.at, 3)];
            // (each way on its own: brightening, then darkening, is two;
            // the quarter of a field's area in sixteenths of a cell)
            // (on at a quarter, off only under a fifth: the estimate's
            // coverage, coarse, wavered about the quarter within one rise -
            // each waver another "transition")
            let quarter = field.quarter as usize;
            let was = self.over[k];
            let over = |j: usize, area: usize| {
                if was[j] {
                    area * 5 >= quarter * 4
                } else {
                    area >= quarter
                }
            };
            let is = [
                over(0, lum[1]),
                over(1, lum[0]),
                over(2, red[1]),
                over(3, red[0]),
            ];
            for (j, now_over) in is.iter().enumerate() {
                let (kind, way) = (j / 2, if j % 2 == 0 { 1 } else { -1 });
                // (every rise counts - the stricter reading; not only one
                // the other way from the last)
                if *now_over && !self.over[k][j] {
                    self.events[k][kind].push(now);
                    self.last[k][kind] = way;
                }
            }
            self.over[k] = is;
            // (a second as frames count it: up to half a frame short of one)
            for e in &mut self.events[k] {
                e.retain(|t| now - t < second);
            }
        }
    }

    fn recent(&self, field: usize) -> usize {
        self.events[field][0].len().max(self.events[field][1].len())
    }
}

/// A field: its rectangle of cells (x, y, w, h), and a quarter of its
/// area as it would be on the screen (in sixteenths of a cell): a field is
/// a third of the screen each way - WCAG's 10° view, 341 x 256 at 1024 x
/// 768 - however small the face; where the face is smaller, its flashes
/// must still cover a quarter of the whole field.
struct Field {
    at: (usize, usize, usize, usize),
    quarter: u32,
}

/// Where each cell and field of a new grid was on the old one.
struct Remap {
    cells: Vec<usize>,
    fields: Vec<usize>,
}

impl Remap {
    fn cells<T: Clone>(&self, old: &[T]) -> Vec<T> {
        self.cells.iter().map(|&i| old[i].clone()).collect()
    }
    fn fields<T: Clone>(&self, old: &[T]) -> Vec<T> {
        self.fields.iter().map(|&i| old[i].clone()).collect()
    }
}

/// Saturated red as WCAG 2.0 reckons it - with R, G and B linear, as in
/// relative luminance: R / (R + G + B) at least 0.8, the change of
/// (R - G - B) x 320 (was: gamma-encoded values, which took an orange red
/// such as #ff4000 for no red at all).
pub fn red(r: f32, g: f32, b: f32) -> f32 {
    let sum = r + g + b;
    if sum > 0.0 && r / sum >= 0.8 {
        ((r - g - b) * 320.0).max(0.0)
    } else {
        0.0
    }
}

/// The guard's state (one for the picture shown).
pub struct FlashGuard {
    cols: usize,
    rows: usize,
    rect: Rect,
    /// The screen the fields are a third of (points): the monitor's, or
    /// `screen_is` where a test takes its picture for the whole screen.
    screen: egui::Vec2,
    pub screen_is: Option<egui::Vec2>,
    /// Frames estimated.
    frame: u64,
    fields: Vec<Field>,
    /// The raw estimate and the shown one (linear RGB a cell).
    raw: Vec<[f32; 3]>,
    shown: Vec<[f32; 3]>,
    /// The same of the large content alone, and how much of each cell
    /// small shapes cover (this frame and the one before, the most).
    raw_big: Vec<[f32; 3]>,
    shown_big: Vec<[f32; 3]>,
    small: Vec<f32>,
    small_before: Vec<f32>,
    small_most: Vec<f32>,
    /// The slow average the veil blends towards, the veil's strength a
    /// cell, and a field's wanted strength and how long it holds.
    slow: Vec<[f32; 3]>,
    veil: Vec<f32>,
    hold: Vec<(f32, f32)>,
    raw_watch: Watch,
    shown_watch: Watch,
    time: f32,
    /// The feedback face's own estimate (its last picture).
    feedback: Vec<gpu::V3>,
    /// A shader face's values on the coarse grid.
    shade: Vec<[f32; 4]>,
    /// The most the veil covers now (0..1; tests and the report).
    pub strength: f32,
    /// The pattern stage: the cells in regular stripes now, how strongly
    /// each is softened (0..1), the local mean it is softened towards, the
    /// last tenth of a second's luminance (is it moving?), and the most it
    /// softens now (tests).
    pattern: Pattern,
    pub pattern_strength: f32,
    target: Vec<f32>,
    /// The estimate as laid on (sRGB values), kept for the next frame.
    acc: Vec<[f32; 3]>,
    acc_big: Vec<[f32; 3]>,
    /// The pieces covering part of a cell, this frame (kept: its memory).
    parts: Vec<Part>,
}

impl Default for FlashGuard {
    fn default() -> Self {
        FlashGuard {
            cols: 0,
            rows: 0,
            rect: Rect::NOTHING,
            screen: SCREEN,
            screen_is: None,
            frame: 0,
            fields: Vec::new(),
            raw: Vec::new(),
            shown: Vec::new(),
            raw_big: Vec::new(),
            shown_big: Vec::new(),
            small: Vec::new(),
            small_before: Vec::new(),
            small_most: Vec::new(),
            slow: Vec::new(),
            veil: Vec::new(),
            hold: Vec::new(),
            raw_watch: Watch::default(),
            shown_watch: Watch::default(),
            time: 0.0,
            feedback: Vec::new(),
            shade: Vec::new(),
            strength: 0.0,
            pattern: Pattern::default(),
            pattern_strength: 0.0,
            target: Vec::new(),
            acc: Vec::new(),
            acc_big: Vec::new(),
            parts: Vec::new(),
        }
    }
}

/// sRGB (0..1) to linear light, by a table (1,024 steps).
fn to_linear(v: f32) -> f32 {
    static TABLE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=1024)
            .map(|i| {
                let v = i as f32 / 1024.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            })
            .collect()
    });
    table[((v.clamp(0.0, 1.0) * 1024.0) as usize).min(1024)]
}

fn to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

impl FlashGuard {
    fn layout(&mut self, rect: Rect, screen: egui::Vec2) {
        let rows =
            ((COLS as f32 * rect.height() / rect.width().max(1.0)).round() as usize).clamp(6, 128);
        let same_grid = self.cols == COLS && self.rows == rows;
        let same = same_grid && self.rect.size() == rect.size() && self.screen == screen;
        self.rect = rect;
        if same {
            return;
        }
        let (old_cols, old_rows) = (self.cols, self.rows);
        let old_centres: Vec<(f32, f32)> = self
            .fields
            .iter()
            .map(|f| {
                (
                    (f.at.0 as f32 + f.at.2 as f32 / 2.0) / old_cols.max(1) as f32,
                    (f.at.1 as f32 + f.at.3 as f32 / 2.0) / old_rows.max(1) as f32,
                )
            })
            .collect();
        self.cols = COLS;
        self.rows = rows;
        self.screen = screen;
        let n = COLS * rows;
        // the fields: a third of the screen each way (in cells, at most the
        // face), slid a quarter of themselves at a time, the last to the
        // edge
        let cw = rect.width() / COLS as f32;
        let ch = rect.height() / rows as f32;
        let (fw_pt, fh_pt) = (screen.x / 3.0, screen.y / 3.0);
        let fw = ((fw_pt / cw).round() as usize).clamp(1, COLS);
        let fh = ((fh_pt / ch).round() as usize).clamp(1, rows);
        let quarter = (fw_pt * fh_pt / 4.0 / (cw * ch) * 16.0).round().max(1.0) as u32;
        let starts = |len: usize, total: usize| {
            let step = (len / 4).max(1);
            let mut at: Vec<usize> = (0..=total - len).step_by(step).collect();
            if at.last() != Some(&(total - len)) {
                at.push(total - len);
            }
            at
        };
        self.fields.clear();
        for &y in &starts(fh, rows) {
            for &x in &starts(fw, COLS) {
                self.fields.push(Field {
                    at: (x, y, fw, fh),
                    quarter,
                });
            }
        }
        let f = self.fields.len();
        if old_cols == 0 || old_centres.is_empty() {
            self.raw = vec![[0.0; 3]; n];
            self.shown = vec![[0.0; 3]; n];
            self.raw_big = vec![[0.0; 3]; n];
            self.shown_big = vec![[0.0; 3]; n];
            self.small = vec![0.0; n];
            self.small_before = vec![0.0; n];
            self.small_most = vec![0.0; n];
            self.slow = vec![[0.0; 3]; n];
            self.veil = vec![0.0; n];
            self.hold = vec![(0.0, 0.0); f];
            self.raw_watch.reset(n, f);
            self.shown_watch.reset(n, f);
            self.feedback.clear();
            return;
        }
        // (a new size - a resize, the large view: the history kept, each
        // new cell and field as the nearest old one)
        let map = Remap {
            cells: (0..n)
                .map(|i| {
                    let (x, y) = (i % COLS, i / COLS);
                    let ox = ((x as f32 + 0.5) / COLS as f32 * old_cols as f32) as usize;
                    let oy = ((y as f32 + 0.5) / rows as f32 * old_rows as f32) as usize;
                    oy.min(old_rows - 1) * old_cols + ox.min(old_cols - 1)
                })
                .collect(),
            fields: self
                .fields
                .iter()
                .map(|fd| {
                    let c = (
                        (fd.at.0 as f32 + fd.at.2 as f32 / 2.0) / COLS as f32,
                        (fd.at.1 as f32 + fd.at.3 as f32 / 2.0) / rows as f32,
                    );
                    (0..old_centres.len())
                        .min_by(|a, b| {
                            let d = |k: usize| {
                                (old_centres[k].0 - c.0).powi(2) + (old_centres[k].1 - c.1).powi(2)
                            };
                            d(*a).total_cmp(&d(*b))
                        })
                        .unwrap_or(0)
                })
                .collect(),
        };
        self.raw = map.cells(&self.raw);
        self.shown = map.cells(&self.shown);
        self.raw_big = map.cells(&self.raw_big);
        self.shown_big = map.cells(&self.shown_big);
        self.small = map.cells(&self.small);
        self.small_before = map.cells(&self.small_before);
        self.small_most = map.cells(&self.small_most);
        self.slow = map.cells(&self.slow);
        self.veil = map.cells(&self.veil);
        self.hold = map.fields(&self.hold);
        self.raw_watch.remap(&map);
        self.shown_watch.remap(&map);
        self.feedback.clear();
    }

    /// After the faces have drawn into `rect` (their shapes from
    /// `first` of the painter's layer on, the shader faces in `drawn`):
    /// the veil over what would flash.
    pub fn guard(
        &mut self,
        painter: &egui::Painter,
        first: usize,
        rect: Rect,
        drawn: &[gpu::Drawn],
        texture: Option<(egui::TextureId, &super::Thumb)>,
        dt: f32,
    ) {
        if rect.width() < 8.0 || rect.height() < 8.0 {
            return;
        }
        let screen = self.screen_is.unwrap_or_else(|| {
            painter
                .ctx()
                .input(|i| i.viewport().monitor_size)
                .filter(|s| s.x > 100.0 && s.y > 100.0)
                .unwrap_or(SCREEN)
        });
        self.layout(rect, screen);
        if dt > 0.0 {
            self.time += dt;
            self.frame += 1;
            self.estimate(painter, first, drawn, texture);
            self.follow(dt);
        }
        self.draw(painter);
    }

    /// The picture as the faces drew it, on the cells.
    fn estimate(
        &mut self,
        painter: &egui::Painter,
        first: usize,
        drawn: &[gpu::Drawn],
        texture: Option<(egui::TextureId, &super::Thumb)>,
    ) {
        let (cols, rows, rect) = (self.cols, self.rows, self.rect);
        // (in sRGB values while laying on, as egui blends; made linear after)
        let mut acc = std::mem::take(&mut self.acc);
        acc.clear();
        acc.resize(cols * rows, [0.0f32; 3]);
        let mut big = std::mem::take(&mut self.acc_big);
        big.clear();
        big.resize(cols * rows, [0.0f32; 3]);
        let mut parts = std::mem::take(&mut self.parts);
        parts.clear();
        parts.resize(cols * rows, Part::default());
        std::mem::swap(&mut self.small, &mut self.small_before);
        self.small.clear();
        self.small.resize(cols * rows, 0.0);
        let cw = rect.width() / cols as f32;
        let ch = rect.height() / rows as f32;
        // the shapes, tessellated, in the order they are painted - a shader
        // face where its paint callback is (or, on the CPU's fallback,
        // where its picture is shown): the ground drawn first lies under
        // it, not over it
        let ctx = painter.ctx();
        let layer = painter.layer_id();
        let shapes: Vec<egui::epaint::ClippedShape> = ctx.graphics(|g| {
            g.get(layer)
                .map(|list| list.all_entries().skip(first).cloned().collect())
                .unwrap_or_default()
        });
        let primitives = ctx.tessellate(shapes, 1.0);
        let mut next = 0;
        for p in primitives {
            match &p.primitive {
                egui::epaint::Primitive::Callback(_) if next < drawn.len() => {
                    let (r, u, picture) = &drawn[next];
                    self.shade_onto(&mut acc, &mut big, *r, u, picture.as_ref());
                    next += 1;
                }
                egui::epaint::Primitive::Mesh(mesh) => {
                    let shown = drawn.get(next).filter(|(r, _, _)| {
                        let b = mesh.calc_bounds();
                        mesh.texture_id != egui::TextureId::default()
                            && (b.min - r.min).length() < 2.0
                            && (b.max - r.max).length() < 2.0
                    });
                    if let Some((r, u, picture)) = shown {
                        self.shade_onto(&mut acc, &mut big, *r, u, picture.as_ref());
                        next += 1;
                    } else {
                        let mut onto = Onto {
                            all: &mut acc,
                            big: &mut big,
                            small: &mut self.small,
                            parts: &mut parts,
                        };
                        let thumb = texture
                            .filter(|(id, _)| *id == mesh.texture_id)
                            .map(|(_, t)| t);
                        lay_mesh(
                            &mut onto,
                            mesh,
                            thumb,
                            rect,
                            cols,
                            rows,
                            cw,
                            ch,
                            p.clip_rect,
                        );
                    }
                }
                _ => {}
            }
        }
        // (any not found in the shapes: over them, rather than not at all)
        for (r, u, picture) in &drawn[next..] {
            self.shade_onto(&mut acc, &mut big, *r, u, picture.as_ref());
        }
        for (i, c) in acc.iter().enumerate() {
            self.raw[i] = [to_linear(c[0]), to_linear(c[1]), to_linear(c[2])];
            let b = big[i];
            self.raw_big[i] = [to_linear(b[0]), to_linear(b[1]), to_linear(b[2])];
        }
        self.acc = acc;
        self.acc_big = big;
        self.parts = parts;
    }

    /// A shader face's colours on the cells it covers (evaluated on a
    /// coarser grid).
    fn shade_onto(
        &mut self,
        acc: &mut [[f32; 3]],
        big: &mut [[f32; 3]],
        r: Rect,
        u: &Uniforms,
        picture: Option<&std::sync::Arc<std::sync::Mutex<gpu::Picture>>>,
    ) {
        let (cols, rows, rect) = (self.cols, self.rows, self.rect);
        let face = u.v[4][0].round() as u32;
        let opacity = u.v[4][3].clamp(0.0, 1.0);
        let aspect = r.width() / r.height().max(1.0);
        // (a picture's face - the spectrograms, the reaction, the phosphor -
        // costs a texture's look a cell: as fine as the cells)
        let sc = if picture.is_some() {
            COLS
        } else if face == gpu::Shader::Flyover as u32 {
            // (a raymarch a cell: dear - coarser, its land changes slowly)
            SHADE_COLS / 2
        } else {
            SHADE_COLS
        };
        let sr = ((sc as f32 / aspect).round() as usize).clamp(4, 96);
        let guard = picture.and_then(|p| p.lock().ok());
        let pic = guard.as_deref();
        self.shade.clear();
        let feedback = face == gpu::Shader::Feedback as u32;
        if feedback && self.feedback.len() != sc * sr {
            self.feedback = vec![[0.0; 3]; sc * sr];
        }
        let mut next = if feedback {
            vec![[0.0f32; 3]; sc * sr]
        } else {
            Vec::new()
        };
        for y in 0..sr {
            for x in 0..sc {
                let uv = [(x as f32 + 0.5) / sc as f32, (y as f32 + 0.5) / sr as f32];
                let (c, a) = if feedback {
                    let c = gpu::feedback_pixel(u, uv, aspect, &self.feedback, sc, sr);
                    next[y * sc + x] = c;
                    (c, 1.0)
                } else if face == gpu::Shader::Phosphor as u32 {
                    let c = gpu::shade(face, u, uv, aspect, pic, 2.0 / sr as f32);
                    let a = (pic.map_or(0.0, |p| p.at(uv[0], uv[1])) * 6.0).clamp(0.0, 1.0);
                    ([c[0] * a, c[1] * a, c[2] * a], a)
                } else {
                    (gpu::shade(face, u, uv, aspect, pic, 2.0 / sr as f32), 1.0)
                };
                self.shade.push([c[0], c[1], c[2], a]);
            }
        }
        if feedback {
            self.feedback = next;
        }
        // onto the cells
        let cw = rect.width() / cols as f32;
        let ch = rect.height() / rows as f32;
        for cy in 0..rows {
            for cx in 0..cols {
                let p = Pos2::new(
                    rect.left() + (cx as f32 + 0.5) * cw,
                    rect.top() + (cy as f32 + 0.5) * ch,
                );
                if !r.contains(p) {
                    continue;
                }
                let (fx, fy) = ((p.x - r.left()) / r.width(), (p.y - r.top()) / r.height());
                // (between the coarse grid's points, bilinear: the veil
                // made from it shows no blocks)
                let gx = (fx * sc as f32 - 0.5).clamp(0.0, (sc - 1) as f32);
                let gy = (fy * sr as f32 - 0.5).clamp(0.0, (sr - 1) as f32);
                let (x0, y0) = (gx as usize, gy as usize);
                let (x1, y1) = ((x0 + 1).min(sc - 1), (y0 + 1).min(sr - 1));
                let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
                let at = |x: usize, y: usize| self.shade[y * sc + x];
                let (a00, a10, a01, a11) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
                let mut v = [0.0f32; 4];
                for (k, v) in v.iter_mut().enumerate() {
                    let top = a00[k] + (a10[k] - a00[k]) * tx;
                    let bottom = a01[k] + (a11[k] - a01[k]) * tx;
                    *v = top + (bottom - top) * ty;
                }
                let [c0, c1, c2, a] = v;
                let a = a * opacity;
                for cell in [&mut acc[cy * cols + cx], &mut big[cy * cols + cx]] {
                    for (k, v) in [c0, c1, c2].into_iter().enumerate() {
                        cell[k] = v.clamp(0.0, 1.0) * opacity + cell[k] * (1.0 - a);
                    }
                }
            }
        }
    }

    /// The transitions raw and shown, and the veil they call for.
    fn follow(&mut self, dt: f32) {
        let now = self.time;
        // shown: as the veil (as it is) leaves the picture
        for i in 0..self.raw.len() {
            self.small_most[i] = self.small[i].max(self.small_before[i]);
        }
        for i in 0..self.raw.len() {
            let a = self.veil[i];
            for k in 0..3 {
                self.shown[i][k] = self.raw[i][k] * (1.0 - a) + self.slow[i][k] * a;
                self.shown_big[i][k] = self.raw_big[i][k] * (1.0 - a) + self.slow[i][k] * a;
            }
        }
        // (34 ms in frames: two at 60 a second, one at 30 and fewer; a
        // second, up to half a frame short of one)
        let at_once = ((AT_ONCE / dt).round() as u64).max(1);
        let second = 1.0 - dt * 0.5;
        self.raw_watch.step(
            &self.raw,
            &self.raw_big,
            &self.small_most,
            &self.fields,
            self.cols,
            self.frame,
            at_once,
            now,
            second,
        );
        self.shown_watch.step(
            &self.shown,
            &self.shown_big,
            &self.small_most,
            &self.fields,
            self.cols,
            self.frame,
            at_once,
            now,
            second,
        );
        // each field's wanted veil: from the fifth raw transition in a
        // second (closer where the veiled picture itself still transits);
        // once on, it stays while the face goes on near the rule - four in
        // a second - and a second and a half after (no pumping on a loop
        // that comes and goes)
        for k in 0..self.fields.len() {
            let raw = self.raw_watch.recent(k);
            let shown = self.shown_watch.recent(k);
            let (level, until) = &mut self.hold[k];
            // (0.95 at once: a veil of 85 % left 0.15 of a black and white
            // swing - still a transition)
            // (held a second and a half after the last time it was called
            // for - not while the face merely stays near the limit: counted
            // strictly, ordinary music stays there, and was veiled)
            let want = if shown >= SHOWN_BUDGET || raw >= RAW_BUDGET {
                0.95
            } else {
                0.0
            };
            if want > 0.0 {
                *until = now + 1.5;
            }
            if want >= *level || now > *until {
                *level = want;
            }
        }
        // the whole face, as strongly as its strongest field calls for:
        // veiled field by field, as each came to the limit, the veil's own
        // edges moved over a black and white picture - flashes of their own
        let level = self.hold.iter().map(|h| h.0).fold(0.0f32, f32::max);
        let mut target = std::mem::take(&mut self.target);
        target.clear();
        target.resize(self.veil.len(), level);
        // (on within a few frames - before the next transition - off over
        // a second)
        let on = 1.0 - (-dt / 0.08).exp();
        let off = 1.0 - (-dt / 0.9).exp();
        // (the slow picture follows over half a second where there is no
        // veil, and stands nearly still under it: following, in black and
        // white, it flashed itself through the veil)
        let follow = 1.0 - (-dt / 0.5).exp();
        let mut strongest = 0.0f32;
        for i in 0..self.veil.len() {
            let t = target[i];
            let k = if t > self.veil[i] { on } else { off };
            self.veil[i] += (t - self.veil[i]) * k;
            strongest = strongest.max(self.veil[i]);
            // the slow picture the veil blends towards
            let follow = follow * (1.0 - self.veil[i]).powi(2);
            for c in 0..3 {
                self.slow[i][c] += (self.raw[i][c] - self.slow[i][c]) * follow;
            }
        }
        self.strength = strongest;
        self.target = target;
        self.patterns(dt);
    }

    /// The pattern stage (ITU-R BT.1702): regular stripes in the estimate
    /// (five light-dark pairs or more in a row or column of cells, full
    /// periods alike, each step 0.1 of luminance) over a quarter of the
    /// picture while they move (a third of them changed by 0.1 within a
    /// tenth of a second) or 35 % standing: softened in place, towards the
    /// mean of the cells round them (21 cells each way: wider than the
    /// stripes), by 95 - 98 % (about 0.03 of their contrast left), until
    /// they no longer are. (For what no face foresaw: a cover, a scheme.
    /// Stripes finer than two cells - about 1 % of the picture's width -
    /// are not seen.)
    fn patterns(&mut self, dt: f32) {
        let (cols, rows) = (self.cols, self.rows);
        let n = cols * rows;
        let p = &mut self.pattern;
        if p.soft.len() != n {
            *p = Pattern {
                soft: vec![0.0; n],
                mean: vec![[0.0; 3]; n],
                marked: vec![false; n],
                past: Default::default(),
            };
        }
        let y: Vec<f32> = self
            .raw
            .iter()
            .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
            .collect();
        p.marked.iter_mut().for_each(|m| *m = false);
        let share = stripes(&y, cols, rows, &mut p.marked);
        let moving = share > 0.0
            && p.past.iter().any(|before: &Vec<f32>| {
                let (mut k, mut changed) = (0usize, 0usize);
                for (i, m) in p.marked.iter().enumerate() {
                    if *m {
                        k += 1;
                        changed += usize::from((y[i] - before[i]).abs() >= 0.1);
                    }
                }
                changed * 3 >= k
            });
        p.past.push_back(y);
        let keep = ((0.1 / dt.max(1e-3)).round() as usize).max(1);
        while p.past.len() > keep {
            p.past.pop_front();
        }
        let hazard = share >= 0.35 || (share >= 0.25 && moving);
        // (the marks spread over a few cells: the stripes' runs end at
        // their turns, and what lies between two runs is the pattern too)
        if hazard {
            let reach = 6usize;
            let mut wide = p.marked.clone();
            for _ in 0..2 {
                let src = wide.clone();
                for cy in 0..rows {
                    for cx in 0..cols {
                        if src[cy * cols + cx] {
                            for xx in cx.saturating_sub(reach)..(cx + reach + 1).min(cols) {
                                wide[cy * cols + xx] = true;
                            }
                            for yy in cy.saturating_sub(reach)..(cy + reach + 1).min(rows) {
                                wide[yy * cols + cx] = true;
                            }
                        }
                    }
                }
            }
            p.marked = wide;
        }
        // the mean of each cell's neighbourhood (21 x 21 cells: wider than
        // the stripes' period, so that it is even), and how far its light
        // and dark lie apart (5 x 5)
        const R: usize = 10;
        let stride = cols + 1;
        let mut sums = vec![[0.0f32; 3]; stride * (rows + 1)];
        for cy in 0..rows {
            let mut row = [0.0f32; 3];
            for cx in 0..cols {
                let c = self.raw[cy * cols + cx];
                for j in 0..3 {
                    row[j] += c[j];
                    sums[(cy + 1) * stride + cx + 1][j] = sums[cy * stride + cx + 1][j] + row[j];
                }
            }
        }
        let y_now = p.past.back().cloned().unwrap_or_default();
        let mut contrast = vec![0.0f32; n];
        for cy in 0..rows {
            for cx in 0..cols {
                let (x0, y0) = (cx.saturating_sub(R), cy.saturating_sub(R));
                let (x1, y1) = ((cx + R + 1).min(cols), (cy + R + 1).min(rows));
                let k = ((x1 - x0) * (y1 - y0)) as f32;
                let mut m = [0.0f32; 3];
                for (j, v) in m.iter_mut().enumerate() {
                    *v = (sums[y1 * stride + x1][j]
                        - sums[y0 * stride + x1][j]
                        - sums[y1 * stride + x0][j]
                        + sums[y0 * stride + x0][j])
                        / k;
                }
                p.mean[cy * cols + cx] = m;
                let (mut lo, mut hi) = (f32::MAX, 0.0f32);
                for yy in cy.saturating_sub(2)..(cy + 3).min(rows) {
                    for xx in cx.saturating_sub(2)..(cx + 3).min(cols) {
                        let l = y_now.get(yy * cols + xx).copied().unwrap_or(0.0);
                        lo = lo.min(l);
                        hi = hi.max(l);
                    }
                }
                contrast[cy * cols + cx] = (hi - lo).max(0.0);
            }
        }
        let on = 1.0 - (-dt / 0.05).exp();
        let off = 1.0 - (-dt / 0.6).exp();
        let mut most = 0.0f32;
        for i in 0..n {
            // (at least 95 %: the veil blends as egui does, in sRGB, where
            // a tenth of a black and white grating left is still more than
            // 0.1 of luminance; and the cells average finer stripes)
            let want = if hazard && p.marked[i] {
                (1.0 - 0.03 / contrast[i].max(0.03)).clamp(0.95, 0.98)
            } else {
                0.0
            };
            let k = if want > p.soft[i] { on } else { off };
            p.soft[i] += (want - p.soft[i]) * k;
            most = most.max(p.soft[i]);
        }
        self.pattern_strength = most;
    }

    /// The most transitions in a field's last second, raw and shown
    /// (tests).
    pub fn pressure(&self) -> (usize, usize) {
        let most = |w: &Watch| {
            (0..self.fields.len())
                .map(|k| w.recent(k))
                .max()
                .unwrap_or(0)
        };
        (most(&self.raw_watch), most(&self.shown_watch))
    }

    /// How much of the picture the pattern stage softens (the mean of its
    /// strength over the cells; tests).
    pub fn pattern_cover(&self) -> f32 {
        let soft = &self.pattern.soft;
        soft.iter().sum::<f32>() / soft.len().max(1) as f32
    }

    /// The estimate's luminance a cell, and the grid's size (tests).
    pub fn estimated(&self) -> (Vec<f32>, usize, usize) {
        let y = self
            .raw
            .iter()
            .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
            .collect();
        (y, self.cols, self.rows)
    }

    /// The veil as a mesh over the picture: a vertex at each cell's
    /// corner, coloured as the cells around it, so that it shades smoothly
    /// (no blocks, no edges).
    fn draw(&self, painter: &egui::Painter) {
        if self.pattern_strength >= 0.01 {
            painter.add(egui::Shape::mesh(
                self.cells_mesh(&self.pattern.soft, &self.pattern.mean),
            ));
        }
        if self.strength < 0.01 {
            return;
        }
        painter.add(egui::Shape::mesh(self.veil_mesh()));
    }

    fn veil_mesh(&self) -> Mesh {
        // (the slow picture blurred over a few cells: drawn as it is, the
        // cells of a grid of lights - the LED equalizer's - stood in it as
        // stripes)
        let (cols, rows) = (self.cols, self.rows);
        let blurred = box_mean(&self.slow, cols, rows, 3);
        self.cells_mesh(&self.veil, &blurred)
    }

    /// A mesh over the picture of `colour` (linear) each cell, as much as
    /// `alpha` says, shaded smoothly between the cells.
    fn cells_mesh(&self, alpha: &[f32], colour: &[[f32; 3]]) -> Mesh {
        let (cols, rows, rect) = (self.cols, self.rows, self.rect);
        let cw = rect.width() / cols as f32;
        let ch = rect.height() / rows as f32;
        let cell = |cx: usize, cy: usize| {
            let i = cy * cols + cx;
            let a = alpha[i];
            let s = colour[i];
            [to_srgb(s[0]) * a, to_srgb(s[1]) * a, to_srgb(s[2]) * a, a]
        };
        let mut mesh = Mesh::default();
        mesh.vertices.reserve((cols + 1) * (rows + 1));
        for vy in 0..=rows {
            for vx in 0..=cols {
                let mut sum = [0.0f32; 4];
                let mut n = 0.0;
                for cy in vy.saturating_sub(1)..(vy + 1).min(rows) {
                    for cx in vx.saturating_sub(1)..(vx + 1).min(cols) {
                        let c = cell(cx, cy);
                        for k in 0..4 {
                            sum[k] += c[k];
                        }
                        n += 1.0;
                    }
                }
                let byte = |v: f32| (v / n * 255.0).round().clamp(0.0, 255.0) as u8;
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: Pos2::new(rect.left() + vx as f32 * cw, rect.top() + vy as f32 * ch),
                    uv: egui::epaint::WHITE_UV,
                    color: Color32::from_rgba_premultiplied(
                        byte(sum[0]),
                        byte(sum[1]),
                        byte(sum[2]),
                        byte(sum[3]),
                    ),
                });
            }
        }
        let stride = cols as u32 + 1;
        for cy in 0..rows {
            for cx in 0..cols {
                let v = cy as u32 * stride + cx as u32;
                let corners = [v, v + 1, v + stride, v + stride + 1];
                if corners
                    .iter()
                    .all(|&c| mesh.vertices[c as usize].color.a() == 0)
                {
                    continue;
                }
                mesh.indices.extend_from_slice(&[
                    v,
                    v + 1,
                    v + stride + 1,
                    v,
                    v + stride + 1,
                    v + stride,
                ]);
            }
        }
        mesh
    }
}

/// The pattern stage's state.
#[derive(Default)]
struct Pattern {
    soft: Vec<f32>,
    mean: Vec<[f32; 3]>,
    marked: Vec<bool>,
    past: std::collections::VecDeque<Vec<f32>>,
}

/// The share of the cells lying in regular stripes (rows and columns),
/// marking them: at least ten turning points of 0.1 of luminance (the
/// darker under 0.8) whose full periods - a light band and a dark together -
/// are alike (within 1.6 times, give or take a cell). As the tests' meter
/// reckons it on pixels.
fn stripes(y: &[f32], cols: usize, rows: usize, marked: &mut [bool]) -> f32 {
    let mut turns: Vec<usize> = Vec::new();
    let mut line = |at: &dyn Fn(usize) -> usize, len: usize, marked: &mut [bool]| {
        turns.clear();
        let mut t = Track::default();
        for k in 0..len {
            if t.step(y[at(k)], STEP, Some(0.8)) != 0 {
                turns.push(k);
            }
        }
        let mut start = 0;
        while start + 10 <= turns.len() {
            let first = turns[start + 2] - turns[start];
            let mut end = start + 2;
            while end + 1 < turns.len() {
                let period = turns[end + 1] - turns[end - 1];
                if (period as f32) > first as f32 * 1.6 + 1.0
                    || (period as f32) < first as f32 / 1.6 - 1.0
                {
                    break;
                }
                end += 1;
            }
            if end - start + 1 >= 10 {
                for k in turns[start]..=turns[end] {
                    marked[at(k)] = true;
                }
                start = end;
            } else {
                start += 1;
            }
        }
    };
    for r in 0..rows {
        line(&|k| r * cols + k, cols, marked);
    }
    for c in 0..cols {
        line(&|k| k * cols + c, rows, marked);
    }
    marked.iter().filter(|m| **m).count() as f32 / (cols * rows).max(1) as f32
}

/// Each cell the mean of the cells within `r` of it (a box, by an integral
/// image).
fn box_mean(cells: &[[f32; 3]], cols: usize, rows: usize, r: usize) -> Vec<[f32; 3]> {
    let stride = cols + 1;
    let mut sums = vec![[0.0f32; 3]; stride * (rows + 1)];
    for cy in 0..rows {
        let mut row = [0.0f32; 3];
        for cx in 0..cols {
            let c = cells[cy * cols + cx];
            for j in 0..3 {
                row[j] += c[j];
                sums[(cy + 1) * stride + cx + 1][j] = sums[cy * stride + cx + 1][j] + row[j];
            }
        }
    }
    let mut out = vec![[0.0f32; 3]; cols * rows];
    for cy in 0..rows {
        for cx in 0..cols {
            let (x0, y0) = (cx.saturating_sub(r), cy.saturating_sub(r));
            let (x1, y1) = ((cx + r + 1).min(cols), (cy + r + 1).min(rows));
            let k = ((x1 - x0) * (y1 - y0)) as f32;
            for j in 0..3 {
                out[cy * cols + cx][j] = (sums[y1 * stride + x1][j]
                    - sums[y0 * stride + x1][j]
                    - sums[y1 * stride + x0][j]
                    + sums[y0 * stride + x0][j])
                    / k;
            }
        }
    }
    out
}

/// What a mesh is laid onto: the whole picture, its large content alone
/// (shapes covering whole cells), and how much of each cell small shapes
/// cover.
struct Onto<'a> {
    all: &'a mut [[f32; 3]],
    big: &'a mut [[f32; 3]],
    small: &'a mut [f32],
    parts: &'a mut [Part],
}

/// The pieces laid on a cell that each cover part of it: side by side, as
/// most do - the halves of a rectangle, the bands of a column, the pieces
/// of a line - they add up, until they cover it all. (Laid each over the
/// last, two halves of a cell lay as three quarters of it, and an even
/// band of rows touching read as bright rows and dim ones: a grating.)
#[derive(Clone, Copy, Default)]
struct Part {
    /// What lay under the first of them.
    under: [f32; 3],
    /// Their colours, each by how much it covers (premultiplied).
    sum: [f32; 3],
    /// How much of the cell they cover (up to all of it).
    cover: f32,
    /// The cell as they left it: anything else laid since, over all of
    /// it, starts them afresh.
    last: [f32; 3],
    open: bool,
}

/// A tessellated mesh laid onto the cells (sRGB values, premultiplied).
#[allow(clippy::too_many_arguments)]
fn lay_mesh(
    acc: &mut Onto<'_>,
    mesh: &Mesh,
    thumb: Option<&super::Thumb>,
    rect: Rect,
    cols: usize,
    rows: usize,
    cw: f32,
    ch: f32,
    clip: Rect,
) {
    let font = mesh.texture_id == egui::TextureId::default();
    let clip = clip.intersect(rect);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    // (the clip takes in the whole picture: no cell need be asked)
    let clip_all = clip.contains_rect(rect);
    // (a mesh of tens of thousands of small pieces - the spirograph's
    // thousands of segments - spread over the cells around it one piece in
    // so many, the others each to its own cell: the frame costs about what
    // 20,000 pieces spread do)
    let stride = (mesh.indices.len() / 3 / 20_000).max(1);
    for (index, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
        let sampled = stride > 1 && index % stride != 0;
        let v = [
            &mesh.vertices[tri[0] as usize],
            &mesh.vertices[tri[1] as usize],
            &mesh.vertices[tri[2] as usize],
        ];
        // how much of the triangle is ink: all of it on the white texel;
        // a glyph's box about a third; another texture (a cover) a grey
        let solid = v.iter().all(|v| v.uv == egui::epaint::WHITE_UV);
        let ink = if font {
            if solid { 1.0 } else { 0.35 }
        } else {
            0.45
        };
        let colour = |v: &egui::epaint::Vertex| {
            let c = v.color;
            [
                c.r() as f32 / 255.0 * ink,
                c.g() as f32 / 255.0 * ink,
                c.b() as f32 / 255.0 * ink,
                c.a() as f32 / 255.0 * ink,
            ]
        };
        let (p0, p1, p2) = (v[0].pos, v[1].pos, v[2].pos);
        let area = ((p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y)) / 2.0;
        if area.abs() < 1e-6 {
            continue;
        }
        let min = Pos2::new(p0.x.min(p1.x).min(p2.x), p0.y.min(p1.y).min(p2.y));
        let max = Pos2::new(p0.x.max(p1.x).max(p2.x), p0.y.max(p1.y).max(p2.y));
        let c = [colour(v[0]), colour(v[1]), colour(v[2])];
        let mean = [
            (c[0][0] + c[1][0] + c[2][0]) / 3.0,
            (c[0][1] + c[1][1] + c[2][1]) / 3.0,
            (c[0][2] + c[1][2] + c[2][2]) / 3.0,
            (c[0][3] + c[1][3] + c[2][3]) / 3.0,
        ];
        let cell_of = |p: Pos2| {
            (
                ((p.x - rect.left()) / cw).floor() as i64,
                ((p.y - rect.top()) / ch).floor() as i64,
            )
        };
        let (x0, y0) = cell_of(min);
        let (x1, y1) = cell_of(max);
        let blend = |acc: &mut Onto<'_>, cx: i64, cy: i64, k: f32| {
            if cx < 0 || cy < 0 || cx >= cols as i64 || cy >= rows as i64 {
                return;
            }
            if !clip_all {
                let centre = Pos2::new(
                    rect.left() + (cx as f32 + 0.5) * cw,
                    rect.top() + (cy as f32 + 0.5) * ch,
                );
                if !clip.contains(centre) {
                    return;
                }
            }
            let at = cy as usize * cols + cx as usize;
            let a = (mean[3] * k).min(1.0);
            let lay = |cell: &mut [f32; 3]| {
                for i in 0..3 {
                    cell[i] = mean[i] * k + cell[i] * (1.0 - a);
                }
            };
            // (how much it covers: without the half again - an additive
            // glow too, which has no alpha)
            if k >= 0.99 {
                lay(&mut acc.all[at]);
                lay(&mut acc.big[at]);
                acc.parts[at].open = false;
            } else {
                let part = &mut acc.parts[at];
                if !part.open || part.last != acc.all[at] {
                    *part = Part {
                        under: acc.all[at],
                        open: true,
                        ..Part::default()
                    };
                }
                // (beyond all of the cell: over those before it)
                let beyond = part.cover + a - 1.0;
                if beyond > 0.0 && part.cover > 0.0 {
                    let keep = ((part.cover - beyond) / part.cover).max(0.0);
                    part.sum.iter_mut().for_each(|v| *v *= keep);
                    part.cover *= keep;
                }
                for i in 0..3 {
                    part.sum[i] += mean[i] * k;
                }
                part.cover = (part.cover + a).min(1.0);
                for i in 0..3 {
                    acc.all[at][i] = part.sum[i] + part.under[i] * (1.0 - part.cover);
                }
                part.last = acc.all[at];
                if a > 0.004 || mean[0] + mean[1] + mean[2] > 0.004 {
                    acc.small[at] += k / SPREAD;
                }
            }
        };
        // (a small piece's share spread over the four cells nearest its
        // centre, as they lie: moving, it moves the estimate smoothly from
        // cell to cell - quantised to one cell, it jumped, and a twinkling
        // galaxy read as a strobe)
        let splat = |acc: &mut Onto<'_>, p: Pos2, k: f32| {
            // (a sliver - a piece of a line's tessellation, many to a cell -
            // to its own cell: spread, thousands of them cost four times as
            // much, and together they move smoothly anyway)
            if k < 0.12 {
                let (cx, cy) = cell_of(p);
                blend(acc, cx, cy, k);
                return;
            }
            let fx = (p.x - rect.left()) / cw - 0.5;
            let fy = (p.y - rect.top()) / ch - 0.5;
            let (x0, y0) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - x0, fy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            for (dx, dy, w) in [
                (0, 0, (1.0 - tx) * (1.0 - ty)),
                (1, 0, tx * (1.0 - ty)),
                (0, 1, (1.0 - tx) * ty),
                (1, 1, tx * ty),
            ] {
                if w > 0.04 {
                    blend(acc, x0 + dx, y0 + dy, (k * w).min(1.0));
                }
            }
        };
        // each cell it touches, by how much of it it covers (4 x 4 samples;
        // half as much again: a line drawn a pixel or two wide covers more
        // than its geometry - better too much than too little)
        let uv_at = |p: Pos2| {
            let d = (p1.y - p2.y) * (p0.x - p2.x) + (p2.x - p1.x) * (p0.y - p2.y);
            let l0 = ((p1.y - p2.y) * (p.x - p2.x) + (p2.x - p1.x) * (p.y - p2.y)) / d;
            let l1 = ((p2.y - p0.y) * (p.x - p2.x) + (p0.x - p2.x) * (p.y - p2.y)) / d;
            let l2 = 1.0 - l0 - l1;
            (v[0].uv.to_vec2() * l0 + v[1].uv.to_vec2() * l1 + v[2].uv.to_vec2() * l2)
                .to_pos2()
                .clamp(Pos2::ZERO, Pos2::new(1.0, 1.0))
        };
        let inside = |p: Pos2| {
            let d0 = (p1.x - p0.x) * (p.y - p0.y) - (p1.y - p0.y) * (p.x - p0.x);
            let d1 = (p2.x - p1.x) * (p.y - p1.y) - (p2.y - p1.y) * (p.x - p1.x);
            let d2 = (p0.x - p2.x) * (p.y - p2.y) - (p0.y - p2.y) * (p.x - p2.x);
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        };
        // (small - within four cells each way: its share where it lies, no
        // samples; most of a line's or a dot's pieces)
        // (a known picture is sampled cell by cell, however its triangles
        // lie)
        if thumb.is_none() && x1 - x0 <= 3 && y1 - y0 <= 3 {
            let centroid = Pos2::new((p0.x + p1.x + p2.x) / 3.0, (p0.y + p1.y + p2.y) / 3.0);
            let k = SPREAD * area.abs() / (cw * ch);
            // (in a mesh of very many, the others to their own cell, each
            // as much as it covers: laid one in so many, each so many times
            // over, the particles' bands behind their dots lay as bright
            // rows with dark ones between - a grating the guard then
            // blurred over the whole face)
            let (bw, bh) = ((max.x - min.x).max(1e-3), (max.y - min.y).max(1e-3));
            if (x1 - x0 >= 2 || y1 - y0 >= 2) && area.abs() >= bw * bh * 0.3 {
                // (over three cells or four, filling its box - a rectangle's
                // half, a dot's wedge - over its box, as it overlaps
                // each cell - at its centre, a band of two such triangles
                // lay as two bright spots with a gap between, or both in
                // one row of cells and none in the next)
                for cy in y0..=y1 {
                    let top = rect.top() + cy as f32 * ch;
                    let oy = (max.y.min(top + ch) - min.y.max(top)).max(0.0);
                    for cx in x0..=x1 {
                        let left = rect.left() + cx as f32 * cw;
                        let ox = (max.x.min(left + cw) - min.x.max(left)).max(0.0);
                        let w = ox * oy / (bw * bh);
                        if w > 0.01 {
                            blend(acc, cx, cy, (k * w).min(1.0));
                        }
                    }
                }
            } else if sampled {
                let (cx, cy) = cell_of(centroid);
                blend(acc, cx, cy, k);
            } else {
                splat(acc, centroid, k);
            }
            continue;
        }
        // (thin - a line's long piece: its area along its length, half a
        // cell at a time, not every cell of its box sampled)
        let bbox = (max.x - min.x).max(1e-3) * (max.y - min.y).max(1e-3);
        if thumb.is_none() && area.abs() < bbox * 0.25 {
            if sampled {
                continue;
            }
            let edges = [(p0, p1, p2), (p1, p2, p0), (p2, p0, p1)];
            let (a, b, c) = edges
                .into_iter()
                .max_by(|x, y| (x.1 - x.0).length().total_cmp(&(y.1 - y.0).length()))
                .unwrap_or((p0, p1, p2));
            let length = (b - a).length();
            let n = ((length / (cw.min(ch) * 0.5)).ceil() as usize).max(1);
            let towards = (c - Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)) / 3.0;
            let share = SPREAD * area.abs() * stride as f32 / n as f32 / (cw * ch);
            for k in 0..n {
                let p = a + (b - a) * ((k as f32 + 0.5) / n as f32) + towards;
                splat(acc, p, share);
            }
            continue;
        }
        let mut any = false;
        for cy in y0.max(0)..=y1.min(rows as i64 - 1) {
            for cx in x0.max(0)..=x1.min(cols as i64 - 1) {
                let corner = Pos2::new(rect.left() + cx as f32 * cw, rect.top() + cy as f32 * ch);
                // (a cell wholly inside - its four corners in - needs no
                // samples: most of a large shape's cells)
                let whole = inside(corner)
                    && inside(corner + egui::vec2(cw, 0.0))
                    && inside(corner + egui::vec2(0.0, ch))
                    && inside(corner + egui::vec2(cw, ch));
                let mut n = if whole { 16 } else { 0 };
                if !whole {
                    for sy in 0..4 {
                        for sx in 0..4 {
                            let p = corner
                                + egui::vec2(
                                    (sx as f32 + 0.5) * cw / 4.0,
                                    (sy as f32 + 0.5) * ch / 4.0,
                                );
                            n += usize::from(inside(p));
                        }
                    }
                }
                if n > 0 {
                    any = true;
                    let k = (n as f32 / 16.0 * SPREAD).min(1.0);
                    match thumb {
                        // (a picture whose pixels are known - the cover:
                        // what it shows there, not a grey)
                        Some(t) => {
                            let centre = corner + egui::vec2(cw / 2.0, ch / 2.0);
                            let uv = uv_at(centre);
                            let px = t.at_srgb(uv.x, uv.y);
                            let tint = mean;
                            let a = (tint[3] / ink * k).min(1.0);
                            let at = cy as usize * cols + cx as usize;
                            if clip.contains(centre) {
                                for cell in [&mut acc.all[at], &mut acc.big[at]] {
                                    for i in 0..3 {
                                        cell[i] = px[i] * tint[i] / ink * k + cell[i] * (1.0 - a);
                                    }
                                }
                            }
                        }
                        None => blend(acc, cx, cy, k),
                    }
                }
            }
        }
        if !any {
            // (thinner than the samples: its share of the cell it lies in)
            let centroid = Pos2::new((p0.x + p1.x + p2.x) / 3.0, (p0.y + p1.y + p2.y) / 3.0);
            splat(acc, centroid, SPREAD * area.abs() / (cw * ch));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A column of bands touching (a pitch of 4.65 points, cells of 2.5),
    /// in a mesh of `extra` more pieces besides, laid on the cells: the
    /// rows' least and most luminance where the bands lie.
    fn bands_laid(extra: usize) -> (f32, f32) {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(320.0, 180.0));
        let (cols, rows) = (128usize, 72usize);
        let (cw, ch) = (rect.width() / cols as f32, rect.height() / rows as f32);
        let mut mesh = Mesh::default();
        let mut y = 20.0;
        while y < 160.0 {
            mesh.add_colored_rect(
                Rect::from_min_size(Pos2::new(100.0, y), egui::vec2(10.0, 4.65 + 0.3)),
                Color32::from_gray(236),
            );
            y += 4.65;
        }
        // (far off, out of the way: only so many)
        for k in 0..extra {
            let x = 250.0 + (k % 50) as f32;
            mesh.add_colored_rect(
                Rect::from_min_size(Pos2::new(x, 170.0), egui::vec2(0.1, 0.1)),
                Color32::from_gray(10),
            );
        }
        let mut all = vec![[0.0f32; 3]; cols * rows];
        let mut big = all.clone();
        let mut small = vec![0.0f32; cols * rows];
        let mut parts = vec![Part::default(); cols * rows];
        let mut onto = Onto {
            all: &mut all,
            big: &mut big,
            small: &mut small,
            parts: &mut parts,
        };
        lay_mesh(&mut onto, &mesh, None, rect, cols, rows, cw, ch, rect);
        let (mut lo, mut hi) = (f32::MAX, 0.0f32);
        for cy in (30.0 / ch) as usize..(150.0 / ch) as usize {
            let cx = (105.0 / cw) as usize;
            let v = to_linear(all[cy * cols + cx][0]);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        (lo, hi)
    }

    #[test]
    fn bands_touching_lie_even_on_the_cells() {
        // (laid each over the last, the cells where two bands met came out
        // a quarter darker: bright rows and dim ones, a grating the pattern
        // stage then blurred the particles' whole face over)
        for extra in [0, 30_000] {
            let (lo, hi) = bands_laid(extra);
            assert!(
                hi - lo < 0.05,
                "rows from {lo} to {hi} ({extra} more pieces)"
            );
            assert!(lo > 0.75, "{lo}");
        }
    }

    #[test]
    fn a_strobe_in_the_estimate_brings_the_veil() {
        let mut g = FlashGuard::default();
        g.layout(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)),
            egui::vec2(384.0, 216.0),
        );
        let n = g.raw.len();
        for i in 0..120 {
            let on = (((i as f32 / 60.0) * 10.0) as u32).is_multiple_of(2);
            let v = if on { 0.6 } else { 0.02 };
            g.raw = vec![[v, v, v]; n];
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        let (raw, _) = g.pressure();
        assert!(raw >= 6, "raw transitions a second: {raw}");
        assert!(g.strength > 0.8, "the veil: {}", g.strength);
    }

    /// The picture's middle field pulsing: `per_second` flashes (2 a
    /// flash's transitions), for two seconds.
    fn pulse(per_second: f32) -> FlashGuard {
        let mut g = FlashGuard::default();
        g.layout(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)),
            egui::vec2(384.0, 216.0),
        );
        let (cols, rows) = (g.cols, g.rows);
        for i in 0..120 {
            let t = i as f32 / 60.0;
            let on = (t * per_second).fract() < 0.5;
            g.raw = (0..cols * rows)
                .map(|k| {
                    let (x, y) = (k % cols, k / cols);
                    let inside = (cols / 3..2 * cols / 3).contains(&x)
                        && (rows / 3..2 * rows / 3).contains(&y);
                    let v = if inside && on { 0.6 } else { 0.02 };
                    [v, v, v]
                })
                .collect();
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        g
    }

    /// Small shapes covering `cover` of every cell, all brightening and
    /// darkening at once 3.5 times a second, over a ground that stays.
    fn twinkle(cover: f32) -> FlashGuard {
        let mut g = FlashGuard::default();
        g.layout(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)),
            egui::vec2(384.0, 216.0),
        );
        let n = g.raw.len();
        for i in 0..120 {
            let on = (i as f32 / 60.0 * 3.5).fract() < 0.5;
            let v = if on { 0.02 + 0.6 * cover } else { 0.02 };
            g.raw = vec![[v, v, v]; n];
            g.raw_big = vec![[0.02; 3]; n];
            g.small = vec![cover; n];
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        g
    }

    /// A strobe over a `w` x `h` corner of a 384 x 216 face on a screen
    /// of `screen`: the veil's strength after two seconds.
    fn corner(w: usize, h: usize, screen: egui::Vec2) -> f32 {
        let mut g = FlashGuard::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0));
        g.layout(rect, screen);
        let (cols, rows) = (g.cols, g.rows);
        let (cw, ch) = (384.0 / cols as f32, 216.0 / rows as f32);
        for i in 0..120 {
            let on = (i as f32 / 60.0 * 5.0).fract() < 0.5;
            g.raw = (0..cols * rows)
                .map(|k| {
                    let (x, y) = ((k % cols) as f32 * cw, (k / cols) as f32 * ch);
                    let v = if on && x < w as f32 && y < h as f32 {
                        0.6
                    } else {
                        0.02
                    };
                    [v, v, v]
                })
                .collect();
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        g.strength
    }

    #[test]
    fn transitions_count_alike_at_30_frames_and_every_rise() {
        // a 5 Hz strobe at 30 frames a second: ten transitions in a second
        // (34 ms "at once" against a frame of 33.3 ms was a coin toss)
        let mut g = FlashGuard::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0));
        g.layout(rect, egui::vec2(384.0, 216.0));
        let n = g.raw.len();
        for i in 0..60 {
            let on = (i as f32 / 30.0 * 5.0).fract() < 0.5;
            let v = if on { 0.6 } else { 0.02 };
            g.raw = vec![[v, v, v]; n];
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 30.0;
            g.frame += 1;
            g.follow(1.0 / 30.0);
        }
        assert!(
            (9..=10).contains(&g.raw_watch.events[0][0].len()),
            "{}",
            g.raw_watch.recent(0)
        );
        // brightening in two steps, the left half then the right, then all
        // dark: three transitions by the strict reading (every rise), as
        // the stricter of the tests' two meters counts them
        let mut g = FlashGuard::default();
        g.layout(rect, egui::vec2(384.0, 216.0));
        let cols = g.cols;
        for step in 0..4 {
            for _ in 0..10 {
                g.raw = (0..n)
                    .map(|k| {
                        let left = k % cols < cols / 2;
                        let v = match step {
                            1 if left => 0.6,
                            2 => 0.6,
                            _ => 0.02,
                        };
                        [v, v, v]
                    })
                    .collect();
                g.raw_big = g.raw.clone();
                g.time += 1.0 / 60.0;
                g.frame += 1;
                g.follow(1.0 / 60.0);
            }
        }
        assert_eq!(g.pressure().0, 3, "{:?}", g.pressure());
    }

    #[test]
    fn a_field_is_a_third_of_the_screen() {
        // a 100 x 100 strobe: over a quarter of a field when the face is
        // the screen (a field 128 x 72), not on a 1920 x 1080 screen (a
        // field 640 x 360: its quarter 57,600 px²) - WCAG's area is the
        // screen's, not the window's (the guard once veiled 12 times too
        // eagerly in the default window)
        assert!(corner(100, 100, egui::vec2(384.0, 216.0)) > 0.8);
        assert!(corner(100, 100, egui::vec2(1920.0, 1080.0)) < 0.01);
        // the whole face strobing is enough either way
        assert!(corner(384, 216, egui::vec2(1920.0, 1080.0)) > 0.8);
    }

    #[test]
    fn a_new_size_keeps_what_the_guard_has_seen() {
        // (it once forgot it all on a resize or the large view: a strobe
        // went on unveiled for another second)
        let mut g = pulse(3.5);
        let (raw, _) = g.pressure();
        assert!(raw >= 6 && g.strength > 0.8);
        g.layout(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 300.0)),
            egui::vec2(640.0, 300.0),
        );
        assert!(g.pressure().0 >= 6, "forgotten: {:?}", g.pressure());
        assert!(g.veil.iter().any(|v| *v > 0.8));
    }

    /// Stripes `period` cells apart over the share `cover` of the picture
    /// (its top), moving a cell every `every` frames (0: standing), for a
    /// second: the pattern stage's strength.
    fn grating(period: usize, cover: f32, every: usize) -> f32 {
        let mut g = FlashGuard::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0));
        g.layout(rect, egui::vec2(384.0, 216.0));
        let (cols, rows) = (g.cols, g.rows);
        for i in 0..60 {
            let shift = (i as usize).checked_div(every).unwrap_or(0);
            g.raw = (0..cols * rows)
                .map(|k| {
                    let (x, y) = (k % cols, k / cols);
                    let striped = (y as f32) < rows as f32 * cover;
                    let v = if striped && ((x + shift) / (period / 2)).is_multiple_of(2) {
                        0.6
                    } else {
                        0.02
                    };
                    [v, v, v]
                })
                .collect();
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        g.pattern_strength
    }

    #[test]
    fn regular_stripes_are_softened_in_place() {
        // standing stripes over 60 % of the picture: softened
        assert!(grating(6, 0.6, 0) > 0.7);
        // over 30 %: standing, left; moving, softened
        assert!(grating(6, 0.3, 0) < 0.01);
        assert!(grating(6, 0.3, 3) > 0.7);
        // no stripes, no softening
        assert!(grating(6, 0.0, 0) < 0.01);
        // the mesh lowers their contrast: the softened cells are drawn over
        // with their neighbourhood's mean
        let mut g = FlashGuard::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0));
        g.layout(rect, egui::vec2(384.0, 216.0));
        let (cols, rows) = (g.cols, g.rows);
        for _ in 0..60 {
            g.raw = (0..cols * rows)
                .map(|k| {
                    if (k % cols / 3) % 2 == 0 {
                        [0.6; 3]
                    } else {
                        [0.02; 3]
                    }
                })
                .collect();
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        let mesh = g.cells_mesh(&g.pattern.soft, &g.pattern.mean);
        assert!(!mesh.indices.is_empty());
        assert!(mesh.vertices.iter().any(|v| v.color.a() > 180));
    }

    #[test]
    fn small_shapes_count_by_the_area_they_cover() {
        // stars over a tenth of the sky, twinkling together: a tenth of the
        // area changes - no flash; over half of it, a flash
        let g = twinkle(0.1);
        assert_eq!(g.pressure().0, 0);
        assert!(g.strength < 0.01, "veiled: {}", g.strength);
        assert!(twinkle(0.5).strength > 0.8);
    }

    #[test]
    fn a_beat_within_the_rule_is_left_alone() {
        // 120 BPM: two flashes, four transitions a second - allowed
        let g = pulse(2.0);
        assert_eq!(g.pressure().0, 4);
        assert!(g.strength < 0.01, "veiled: {}", g.strength);
        // 3.5 flashes a second (seven transitions) - veiled
        assert!(pulse(3.5).strength > 0.8);
    }

    #[test]
    fn the_veil_has_no_edge_and_no_blocks() {
        let g = pulse(3.5);
        let mesh = g.veil_mesh();
        let stride = g.cols + 1;
        let mut steepest = 0i32;
        for (k, v) in mesh.vertices.iter().enumerate() {
            let a = i32::from(v.color.a());
            if k % stride + 1 < stride {
                steepest = steepest.max((a - i32::from(mesh.vertices[k + 1].color.a())).abs());
            }
            if k + stride < mesh.vertices.len() {
                steepest = steepest.max((a - i32::from(mesh.vertices[k + stride].color.a())).abs());
            }
        }
        let most = mesh.vertices.iter().map(|v| v.color.a()).max().unwrap();
        assert!(most > 200, "the veil: {most}");
        // (from nothing to the full veil over several cells, not one)
        assert!(steepest <= 64, "a step of {steepest} between neighbours");
    }
}
