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
/// Four transitions within this (seconds): a flicker of eight a second or
/// more - veiled at once, not after the seventh (review 11: four or five
/// flashes went through in the first second of a hazard).
const FAST: f32 = 0.45;
/// The fifth transition in a second coming this soon after the fourth
/// (seconds): a flicker at 2.6 Hz or quicker that has reached five -
/// veiled now, for the veil's own coming over a light picture may be the
/// sixth, and the seventh is not let through. A flicker that starts within
/// the rule and speeds up (3 Hz stepping to 4, 2 rising to 5, 2 stepping
/// to 3.6) went through with seven or eight: RAW_BUDGET's seventh and
/// FAST's eight a second came too late (review 12). A beat of 2.5 Hz is
/// left alone.
const PACE: f32 = 0.19;

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
#[derive(Clone, Default)]
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
    /// The either-way count, whatever the phase: the area of each field
    /// newly transiting - brightening and darkening alike, luminance or
    /// red - summed over the last frames "at once"; each time it reaches a
    /// quarter of the field, a transition, and the sum starts again. A
    /// checker flickering in antiphase - its tiles each the other way from
    /// their neighbours - never has a quarter of a field changing the same
    /// way, and was not seen.
    either_new: Vec<std::collections::VecDeque<u32>>,
    either_events: Vec<Vec<f32>>,
    either_sum: Vec<u32>,
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
        self.either_new = vec![std::collections::VecDeque::new(); fields];
        self.either_events = (0..fields).map(|_| Vec::with_capacity(16)).collect();
        let _ = cells;
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
        self.either_new = map.fields(&self.either_new);
        self.either_events = map.fields(&self.either_events);
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
        // (the either-way count: this frame's new transitions, of either
        // kind and either way, their area summed in each field over the
        // frames "at once")
        {
            let rows = cells.len() / cols.max(1);
            let stride = cols + 1;
            self.either_sum.clear();
            self.either_sum.resize(stride * (rows + 1), 0);
            for y in 0..rows {
                let mut row = 0u32;
                for x in 0..cols {
                    let i = y * cols + x;
                    // (of the large content only: small shapes moving -
                    // a galaxy's thousands of stars - change cells all
                    // over, and the face was veiled half the time)
                    let lum = self.lum_at[i] == frame && self.lum_w[i] == 16;
                    let red = self.red_at[i] == frame && self.red_w[i] == 16;
                    if lum || red {
                        row += 16;
                    }
                    self.either_sum[(y + 1) * stride + x + 1] =
                        self.either_sum[y * stride + x + 1] + row;
                }
            }
            let sums = &self.either_sum;
            for (k, field) in fields.iter().enumerate() {
                let (x, y, w, h) = field.at;
                let at = |x: usize, y: usize| sums[y * stride + x] as i64;
                let new = (at(x + w, y + h) - at(x, y + h) - at(x + w, y) + at(x, y)) as u32;
                let ring = &mut self.either_new[k];
                ring.push_back(new);
                // (a frame's: summed over frames, a moving line -
                // transiting where it comes and where it leaves - came to
                // a quarter, and the galaxy and the ribbon were veiled)
                while ring.len() > 1 {
                    ring.pop_front();
                }
                // (half the field, not a quarter: the estimate lays a
                // gradient's triangles flat, and a glow growing with the
                // bass changed many cells in one frame where the picture
                // changed a few pixels; a checker in antiphase changes the
                // whole field at once)
                if ring.iter().sum::<u32>() >= field.quarter * 2 {
                    self.either_events[k].push(now);
                    ring.clear();
                }
                self.either_events[k].retain(|t| now - t < second);
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
        self.events[field][0]
            .len()
            .max(self.events[field][1].len())
            .max(self.either_events[field].len())
    }

    /// Whether a field flickers fast: four transitions (of one kind) within
    /// the last FAST seconds - eight or more a second, where a face within
    /// the rule, at most six evenly, takes half a second for four; or a
    /// fifth in a second within PACE of the one before.
    fn fast(&self, field: usize, now: f32) -> bool {
        let quick = |ev: &[f32]| {
            let n = ev.len();
            ev.iter().filter(|&&t| now - t < FAST).count() >= 4
                || (n >= 5 && ev[n - 1] - ev[n - 2] <= PACE)
        };
        quick(&self.events[field][0])
            || quick(&self.events[field][1])
            || quick(&self.either_events[field])
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
    /// No veil on, now or the last frame: the shown picture is the raw one,
    /// and its transitions are followed once (the raw's), not twice.
    shown_is_raw: bool,
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
    /// The pieces covering part of a cell, this frame (kept: its memory),
    /// and the cells they lie on.
    parts: Vec<Part>,
    touched: Vec<u32>,
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
            shown_is_raw: true,
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
            touched: Vec::new(),
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
            self.pattern = Pattern::default();
            self.pattern_strength = 0.0;
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
        // (the pattern stage's cells too: kept at the old grid's size, the
        // next frame drawn while time stood still - paused, then F11 or a
        // resize - read past their end and the player aborted: review 15)
        let old = old_cols * old_rows;
        let p = &mut self.pattern;
        if p.soft.len() == old && p.mean.len() == old && p.marked.len() == old {
            p.soft = map.cells(&p.soft);
            p.mean = map.cells(&p.mean);
            p.marked = map.cells(&p.marked);
            for frame in p.past.iter_mut() {
                if frame.len() == old {
                    *frame = map.cells(frame);
                }
            }
            p.past.retain(|f| f.len() == n);
        } else {
            *p = Pattern::default();
            self.pattern_strength = 0.0;
        }
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
                            touched: &mut self.touched,
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
        // (no veil: the shown picture is the raw one - its transitions
        // are the raw's, followed once; when a veil comes, the shown
        // picture's are followed from where the raw's are)
        let veiled = self.strength > 0.0;
        if veiled && self.shown_is_raw {
            self.shown_watch = self.raw_watch.clone();
            self.shown_is_raw = false;
        } else if !veiled {
            self.shown_is_raw = true;
        }
        if !self.shown_is_raw {
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
        }
        // each field's wanted veil: from the fifth raw transition in a
        // second (closer where the veiled picture itself still transits);
        // once on, it stays while the face goes on near the rule - four in
        // a second - and a second and a half after (no pumping on a loop
        // that comes and goes)
        // (a saturated red change over a field: the veil at once, whatever
        // the count - no red flash passes, however slow: a flash is a pair,
        // and once the first change is shown its pair follows whatever the
        // veil does. Review 12: red at 2 Hz, two red flashes a second, was
        // not veiled - it waited for the general budget.)
        let mut red_now = false;
        for k in 0..self.fields.len() {
            let raw = self.raw_watch.recent(k);
            let red = self.raw_watch.events[k][1]
                .last()
                .is_some_and(|t| now - t < dt * 0.5);
            red_now |= red;
            let shown = if self.shown_is_raw {
                raw
            } else {
                self.shown_watch.recent(k)
            };
            let (level, until) = &mut self.hold[k];
            // (0.95 at once: a veil of 85 % left 0.15 of a black and white
            // swing - still a transition)
            // (held a second and a half after the last time it was called
            // for - not while the face merely stays near the limit: counted
            // strictly, ordinary music stays there, and was veiled)
            let fast = self.raw_watch.fast(k, now)
                || (!self.shown_is_raw && self.shown_watch.fast(k, now));

            // (red: 0.99 - the picture the veil holds may be red itself, a
            // red that came before the veil; 0.05 of a red swing over it was
            // still a red transition)
            let want = if red {
                0.99
            } else if shown >= SHOWN_BUDGET || raw >= RAW_BUDGET || fast {
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
        // (on in a frame or two: at 8 Hz the next transition is 60 ms away)
        let on = 1.0 - (-dt / 0.03).exp();
        let off = 1.0 - (-dt / 0.9).exp();
        // (the slow picture follows over half a second where there is no
        // veil, and stands nearly still under it: following, in black and
        // white, it flashed itself through the veil)
        let follow = 1.0 - (-dt / 0.5).exp();
        let mut strongest = 0.0f32;
        for i in 0..self.veil.len() {
            let t = target[i];
            // (for red: in this frame, before the change is shown)
            let k = if t <= self.veil[i] {
                off
            } else if red_now {
                1.0
            } else {
                on
            };
            self.veil[i] += (t - self.veil[i]) * k;
            // (gone below a thousandth: gone - the shown picture is the raw)
            if t == 0.0 && self.veil[i] < 0.001 {
                self.veil[i] = 0.0;
            }
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
                spare: Vec::new(),
                sums: Vec::new(),
                contrast: Vec::new(),
                wide: Vec::new(),
                src: Vec::new(),
            };
        }
        let mut y = p.spare.pop().unwrap_or_default();
        y.clear();
        y.extend(
            self.raw
                .iter()
                .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]),
        );
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
            if let Some(old) = p.past.pop_front() {
                p.spare.push(old);
            }
        }
        let hazard = share >= 0.35 || (share >= 0.25 && moving);
        // (the marks spread over a few cells: the stripes' runs end at
        // their turns, and what lies between two runs is the pattern too)
        if hazard {
            let reach = 6usize;
            let mut wide = std::mem::take(&mut p.wide);
            wide.clear();
            wide.extend_from_slice(&p.marked);
            let mut src = std::mem::take(&mut p.src);
            for _ in 0..2 {
                src.clear();
                src.extend_from_slice(&wide);
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
            p.wide = std::mem::replace(&mut p.marked, wide);
            p.src = src;
        }
        // (nothing striped and nothing softened still: nothing to work
        // out - most frames, at no cost)
        if !hazard && self.pattern_strength < 0.002 {
            if self.pattern_strength > 0.0 {
                p.soft.iter_mut().for_each(|v| *v = 0.0);
                self.pattern_strength = 0.0;
            }
            return;
        }
        // the mean of each cell's neighbourhood (21 x 21 cells: wider than
        // the stripes' period, so that it is even), and how far its light
        // and dark lie apart (5 x 5)
        const R: usize = 10;
        let stride = cols + 1;
        let mut sums = std::mem::take(&mut p.sums);
        sums.clear();
        sums.resize(stride * (rows + 1), [0.0f32; 3]);
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
        let y_now = p.past.back().map(Vec::as_slice).unwrap_or_default();
        let mut contrast = std::mem::take(&mut p.contrast);
        contrast.clear();
        contrast.resize(n, 0.0);
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
        // (on in the frame it is seen: easing it on over 0.05 s left the
        // stripes standing for a frame or several - review 11's meter saw
        // one to six frames of a pattern in the scope, the flow field, the
        // reaction-diffusion in the starkest schemes)
        let on = 1.0;
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
        p.sums = sums;
        p.contrast = contrast;
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
        let shown = if self.shown_is_raw {
            &self.raw_watch
        } else {
            &self.shown_watch
        };
        (most(&self.raw_watch), most(shown))
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
        let n = self.cols * self.rows;
        if self.pattern_strength >= 0.01
            && self.pattern.soft.len() == n
            && self.pattern.mean.len() == n
        {
            // (its edge feathered over three cells each way: softened cell
            // by cell, its edges were dark steps over the picture)
            let alpha: Vec<[f32; 3]> = self.pattern.soft.iter().map(|v| [*v; 3]).collect();
            // (the marks reach six cells past the stripes: blurred over
            // three, the stripes themselves stay softened in full)
            let alpha: Vec<f32> = box_mean(&alpha, self.cols, self.rows, 3)
                .into_iter()
                .map(|m| m[0])
                .collect();
            painter.add(egui::Shape::mesh(
                self.cells_mesh(&alpha, &self.pattern.mean),
            ));
        }
        if self.strength < 0.01 || self.veil.len() != n || self.slow.len() != n {
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
        // (each cell's colour once - not again for each of its four
        // corners: four times the sRGB curves, 3 - 4 ms of a 1080p frame
        // unoptimised, review 17, L5)
        let cells: Vec<[f32; 4]> = alpha
            .iter()
            .zip(colour)
            .map(|(&a, s)| {
                if a <= 0.0 {
                    [0.0; 4]
                } else {
                    [to_srgb(s[0]) * a, to_srgb(s[1]) * a, to_srgb(s[2]) * a, a]
                }
            })
            .collect();
        let cell = |cx: usize, cy: usize| cells[cy * cols + cx];
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
    /// Kept from frame to frame (no allocation a frame): the pictures let
    /// go of, the sums, the contrast, the marks spread.
    spare: Vec<Vec<f32>>,
    sums: Vec<[f32; 3]>,
    contrast: Vec<f32>,
    wide: Vec<bool>,
    src: Vec<bool>,
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
    /// The cells with pieces pending (laid on when the mesh is done).
    touched: &'a mut Vec<u32>,
}

/// The pieces laid on a cell that each cover part of it: side by side, as
/// most do - the halves of a rectangle, the bands of a column, the pieces
/// of a line - they add up, until they cover it all. (Laid each over the
/// last, two halves of a cell lay as three quarters of it, and an even
/// band of rows touching read as bright rows and dim ones: a grating.)
/// (Summed while a mesh is laid and laid on the cell once, when it is done
/// or a piece covering all of the cell comes: a cell's arithmetic a piece
/// is two additions - the particles' twenty thousand pieces cost a few
/// milliseconds less.)
#[derive(Clone, Copy, Default)]
struct Part {
    /// Their colours, each by how much it covers (premultiplied).
    sum: [f32; 3],
    /// How much of the cell they cover (beyond all of it: each the less).
    cover: f32,
    open: bool,
}

/// The pieces pending on cell `at` laid over it.
fn lay_parts(acc: &mut Onto<'_>, at: usize) {
    let part = &mut acc.parts[at];
    if !part.open {
        return;
    }
    part.open = false;
    let scale = if part.cover > 1.0 {
        1.0 / part.cover
    } else {
        1.0
    };
    let left = 1.0 - part.cover.min(1.0);
    for i in 0..3 {
        acc.all[at][i] = part.sum[i] * scale + acc.all[at][i] * left;
    }
}

/// The part of the segment `a`..`b` within `r`: its range of t (a + (b - a)
/// t), or None (Liang-Barsky).
fn within(a: Pos2, b: Pos2, r: Rect) -> Option<(f32, f32)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - r.left()),
        (d.x, r.right() - a.x),
        (-d.y, a.y - r.top()),
        (d.y, r.bottom() - a.y),
    ] {
        if p.abs() < 1e-9 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then_some((t0, t1))
}

/// The grid a mesh is laid on: the picture's rect, its cells, the clip.
#[derive(Clone, Copy)]
struct Grid {
    rect: Rect,
    cols: usize,
    rows: usize,
    cw: f32,
    ch: f32,
    clip: Rect,
    /// The clip takes in the whole picture: no cell need be asked.
    clip_all: bool,
}

/// A piece of colour `mean` (sRGB, premultiplied, its alpha last) laid on
/// cell (`cx`, `cy`), covering `k` of it: all of it laid over the cell
/// (what is pending under it first), part of it pending with the other
/// parts laid side by side.
fn blend(acc: &mut Onto<'_>, g: &Grid, cx: i64, cy: i64, k: f32, mean: [f32; 4]) {
    if cx < 0 || cy < 0 || cx >= g.cols as i64 || cy >= g.rows as i64 {
        return;
    }
    if !g.clip_all {
        let centre = Pos2::new(
            g.rect.left() + (cx as f32 + 0.5) * g.cw,
            g.rect.top() + (cy as f32 + 0.5) * g.ch,
        );
        if !g.clip.contains(centre) {
            return;
        }
    }
    let at = cy as usize * g.cols + cx as usize;
    let a = (mean[3] * k).min(1.0);
    let lay = |cell: &mut [f32; 3]| {
        for i in 0..3 {
            cell[i] = mean[i] * k + cell[i] * (1.0 - a);
        }
    };
    // (how much it covers: without the half again - an additive
    // glow too, which has no alpha)
    if k >= 0.99 {
        // (what is pending under it first)
        lay_parts(acc, at);
        lay(&mut acc.all[at]);
        lay(&mut acc.big[at]);
    } else {
        let part = &mut acc.parts[at];
        if !part.open {
            *part = Part {
                open: true,
                ..Part::default()
            };
            acc.touched.push(at as u32);
        }
        for i in 0..3 {
            part.sum[i] += mean[i] * k;
        }
        part.cover += a;
        if a > 0.004 || mean[0] + mean[1] + mean[2] > 0.004 {
            acc.small[at] += k / SPREAD;
        }
    }
}

/// Two triangles (a, b, c) and (a, c, d) of a mesh that make a rectangle
/// on the axes, of solid colour (egui's coloured rectangle, a pen's
/// gradient).
struct RectPair {
    rect: Rect,
    /// Its diagonal, a to c, and which side of it the first triangle lies
    /// (+1 or -1, by the cross product).
    from: Pos2,
    to: Pos2,
    side: f32,
    /// Each triangle's mean colour (sRGB, premultiplied, alpha last) - as
    /// a triangle alone is laid.
    first: [f32; 4],
    second: [f32; 4],
}

fn rect_of(mesh: &Mesh, t: &[u32; 3], n: &[u32; 3]) -> Option<RectPair> {
    if n[0] != t[0] || n[1] != t[2] {
        return None;
    }
    let v = |i: u32| mesh.vertices.get(i as usize);
    let (a, b, c, d) = (v(t[0])?, v(t[1])?, v(t[2])?, v(n[2])?);
    if [a, b, c, d].iter().any(|v| v.uv != egui::epaint::WHITE_UV) {
        return None;
    }
    let (p, q) = (a.pos, c.pos);
    if p.x == q.x || p.y == q.y {
        return None;
    }
    let others = [Pos2::new(p.x, q.y), Pos2::new(q.x, p.y)];
    if !((b.pos == others[0] && d.pos == others[1]) || (b.pos == others[1] && d.pos == others[0])) {
        return None;
    }
    let f = |c: Color32| {
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            c.a() as f32 / 255.0,
        ]
    };
    let mean = |x: Color32, y: Color32, z: Color32| {
        let (x, y, z) = (f(x), f(y), f(z));
        std::array::from_fn(|i| (x[i] + y[i] + z[i]) / 3.0)
    };
    let e = q - p;
    let cross = e.x * (b.pos.y - p.y) - e.y * (b.pos.x - p.x);
    Some(RectPair {
        rect: Rect::from_two_pos(p, q),
        from: p,
        to: q,
        side: cross.signum(),
        first: mean(a.color, b.color, c.color),
        second: mean(a.color, c.color, d.color),
    })
}

/// A rectangle on the axes laid onto the cells it covers, each by how much
/// of it it covers, in its triangles' colours as they share the cell - as
/// its two triangles would be laid, without sampling each cell row's edges
/// (a spectrum's 128 tall bars cost 2 - 3 ms a frame so: review 17, L5).
fn lay_rect(acc: &mut Onto<'_>, g: &Grid, pair: &RectPair) {
    let r = pair.rect;
    let e = pair.to - pair.from;
    let length = e.length().max(1e-6);
    // (the cell's half extent across the diagonal)
    let half = ((e.y.abs() * g.cw + e.x.abs() * g.ch) / length / 2.0).max(1e-6);
    let cell_x = |x: f32| ((x - g.rect.left()) / g.cw).floor() as i64;
    let cell_y = |y: f32| ((y - g.rect.top()) / g.ch).floor() as i64;
    let (x0, x1) = (
        cell_x(r.min.x).max(0),
        cell_x(r.max.x).min(g.cols as i64 - 1),
    );
    let (y0, y1) = (
        cell_y(r.min.y).max(0),
        cell_y(r.max.y).min(g.rows as i64 - 1),
    );
    for cy in y0..=y1 {
        let top = g.rect.top() + cy as f32 * g.ch;
        let y_lo = r.min.y.max(top);
        let y_hi = r.max.y.min(top + g.ch);
        let oy = (y_hi - y_lo).max(0.0);
        if oy <= 0.0 {
            continue;
        }
        for cx in x0..=x1 {
            let left = g.rect.left() + cx as f32 * g.cw;
            let x_lo = r.min.x.max(left);
            let x_hi = r.max.x.min(left + g.cw);
            let ox = (x_hi - x_lo).max(0.0);
            let share = ox * oy / (g.cw * g.ch);
            if share <= 0.03 {
                continue;
            }
            // (the part of the cell on the first triangle's side: a cell the
            // diagonal crosses takes two pieces, side by side, as the
            // triangles laid alone gave it - laid whole, its moving fill
            // counted as a large shape's, and the line's fill through drum
            // and bass was veiled 75 % of the time, not 48)
            let centre = Pos2::new((x_lo + x_hi) / 2.0, (y_lo + y_hi) / 2.0);
            let d = (e.x * (centre.y - pair.from.y) - e.y * (centre.x - pair.from.x)) / length
                * pair.side;
            let w = (0.5 + d / (2.0 * half)).clamp(0.0, 1.0);
            let k = (share * SPREAD).min(1.0);
            if w >= 1.0 {
                blend(acc, g, cx, cy, k, pair.first);
            } else if w <= 0.0 {
                blend(acc, g, cx, cy, k, pair.second);
            } else {
                blend(acc, g, cx, cy, (k * w).min(0.98), pair.first);
                blend(acc, g, cx, cy, (k * (1.0 - w)).min(0.98), pair.second);
            }
        }
    }
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
    acc.touched.clear();
    lay_pieces(acc, mesh, thumb, rect, cols, rows, cw, ch, clip);
    // (the pieces side by side, pending, laid on their cells)
    let touched = std::mem::take(acc.touched);
    for at in &touched {
        lay_parts(acc, *at as usize);
    }
    *acc.touched = touched;
}

#[allow(clippy::too_many_arguments)]
fn lay_pieces(
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
    let grid = Grid {
        rect,
        cols,
        rows,
        cw,
        ch,
        clip,
        clip_all,
    };
    // (a mesh of tens of thousands of small pieces - the spirograph's
    // thousands of segments - laid one piece in so many, each counted so
    // many times: dense lines are evenly covered either way, and the frame
    // costs what one of 20,000 pieces does; pieces over two cells are
    // each laid over their box, as they lie)
    let stride = (mesh.indices.len() / 3 / 20_000).max(1);
    let tris = mesh.indices.as_chunks::<3>().0;
    let mut pair_laid = false;
    for (index, tri) in tris.iter().enumerate() {
        // (the second triangle of a rectangle laid whole)
        if std::mem::take(&mut pair_laid) {
            continue;
        }
        let sampled = stride > 1 && index % stride != 0;
        let v = [
            &mesh.vertices[tri[0] as usize],
            &mesh.vertices[tri[1] as usize],
            &mesh.vertices[tri[2] as usize],
        ];
        let (p0, p1, p2) = (v[0].pos, v[1].pos, v[2].pos);
        let area = ((p1.x - p0.x) * (p2.y - p0.y) - (p2.x - p0.x) * (p1.y - p0.y)) / 2.0;
        if area.abs() < 1e-6 {
            continue;
        }
        let min = Pos2::new(p0.x.min(p1.x).min(p2.x), p0.y.min(p1.y).min(p2.y));
        let max = Pos2::new(p0.x.max(p1.x).max(p2.x), p0.y.max(p1.y).max(p2.y));
        let cell_of = |p: Pos2| {
            (
                ((p.x - rect.left()) / cw).floor() as i64,
                ((p.y - rect.top()) / ch).floor() as i64,
            )
        };
        let (x0, y0) = cell_of(min);
        let (x1, y1) = cell_of(max);
        let (bw, bh) = ((max.x - min.x).max(1e-3), (max.y - min.y).max(1e-3));
        let small = x1 - x0 <= 3 && y1 - y0 <= 3;
        // (in a mesh of very many, one in so many: those left out cost
        // no more than this - the particles' 90,000 pieces at 1920 x 1080)
        if sampled && thumb.is_none() {
            if small {
                if !((x1 - x0 >= 2 || y1 - y0 >= 2) && area.abs() >= bw * bh * 0.3) {
                    continue;
                }
            } else {
                let longest = (p1 - p0)
                    .length()
                    .max((p2 - p1).length())
                    .max((p0 - p2).length())
                    .max(1e-3);
                if area.abs() < bw * bh * 0.25 && 2.0 * area.abs() / longest < 2.0 * cw.max(ch) {
                    continue;
                }
            }
        }
        // (a large rectangle: laid whole, with its second triangle)
        if font
            && thumb.is_none()
            && !small
            && let Some(pair) = tris.get(index + 1).and_then(|n| rect_of(mesh, tri, n))
        {
            lay_rect(acc, &grid, &pair);
            pair_laid = true;
            continue;
        }
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
        let c = [colour(v[0]), colour(v[1]), colour(v[2])];
        let mean = [
            (c[0][0] + c[1][0] + c[2][0]) / 3.0,
            (c[0][1] + c[1][1] + c[2][1]) / 3.0,
            (c[0][2] + c[1][2] + c[2][2]) / 3.0,
            (c[0][3] + c[1][3] + c[2][3]) / 3.0,
        ];
        let blend =
            |acc: &mut Onto<'_>, cx: i64, cy: i64, k: f32| blend(acc, &grid, cx, cy, k, mean);
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
        // (small - within four cells each way: its share where it lies, no
        // samples; most of a line's or a dot's pieces)
        // (a known picture is sampled cell by cell, however its triangles
        // lie)
        if thumb.is_none() && small {
            let centroid = Pos2::new((p0.x + p1.x + p2.x) / 3.0, (p0.y + p1.y + p2.y) / 3.0);
            let k = SPREAD * area.abs() / (cw * ch);
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
            } else if !sampled {
                // (in a mesh of very many, one in so many, so many times
                // over: spread round its centre, they are even enough)
                splat(acc, centroid, k * stride as f32);
            }
            continue;
        }
        // (thin - a line's long piece: its area along its length, a cell
        // at a time, not every cell of its box sampled)
        let bbox = (max.x - min.x).max(1e-3) * (max.y - min.y).max(1e-3);
        // (thin for the cells: a band two cells wide and more, a long
        // slanting quad's half filling little of its box, is laid as the
        // large shape it is - laid as small pieces, its cells were never
        // counted whole, and bands crossing the picture every frame were
        // seen as a line moving: review 16, H1)
        let longest = (p1 - p0)
            .length()
            .max((p2 - p1).length())
            .max((p0 - p2).length())
            .max(1e-3);
        let across = 2.0 * area.abs() / longest;
        if thumb.is_none() && area.abs() < bbox * 0.25 && across < 2.0 * cw.max(ch) {
            if sampled {
                continue;
            }
            let edges = [(p0, p1, p2), (p1, p2, p0), (p2, p0, p1)];
            let (a, b, c) = edges
                .into_iter()
                .max_by(|x, y| (x.1 - x.0).length().total_cmp(&(y.1 - y.0).length()))
                .unwrap_or((p0, p1, p2));
            // (only the part of it within the picture: a line's sharp turn
            // is tessellated with a miter reaching thousands of points out -
            // a tangle of them, the stereo scope in uncorrelated noise, was
            // laid along 14 million cells, 0.17 s a frame at 1080p)
            let Some((t0, t1)) = within(a, b, rect.expand2(egui::vec2(cw, ch))) else {
                continue;
            };
            let length = (b - a).length() * (t1 - t0);
            // (a cell at a time: half a cell at a time, a bar's thin edges
            // a whole picture tall cost hundreds of blends each)
            let n = ((length / cw.min(ch)).ceil() as usize).max(1);
            // (as wide at each place along it as the triangle is there -
            // nothing at the long edge's ends, most where its third corner
            // stands over it: laid evenly along its length, the slivers of
            // a fan - a rounded ground's, from one corner to the arc of
            // the corner across - were too much at the fan's corner and
            // half of the ground at the far one; on a light ground the
            // estimate held a dark wedge from that corner in that no face
            // draws, and the veil, made of the estimate, showed it; review
            // 18, M1)
            let ab = b - a;
            let tc = ((c - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            let wide = |t: f32| {
                if t <= tc && tc > 1e-4 {
                    t / tc
                } else {
                    (1.0 - t) / (1.0 - tc).max(1e-4)
                }
            };
            let height = c - (a + ab * tc);
            let share =
                2.0 * SPREAD * area.abs() * (t1 - t0) * stride as f32 / n as f32 / (cw * ch);
            for k in 0..n {
                let t = t0 + (t1 - t0) * ((k as f32 + 0.5) / n as f32);
                let w = wide(t).clamp(0.0, 1.0);
                if w <= 0.0 {
                    continue;
                }
                let p = a + ab * t + height * (w / 2.0);
                splat(acc, p, share * w);
            }
            continue;
        }
        // where a horizontal line crosses the triangle (x from, to)
        let span = |y: f32| -> Option<(f32, f32)> {
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for (a, b) in [(p0, p1), (p1, p2), (p2, p0)] {
                let (ya, yb) = (a.y.min(b.y), a.y.max(b.y));
                if y < ya || y > yb {
                    continue;
                }
                let x = if (b.y - a.y).abs() < 1e-6 {
                    lo = lo.min(a.x.min(b.x));
                    hi = hi.max(a.x.max(b.x));
                    continue;
                } else {
                    a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y)
                };
                lo = lo.min(x);
                hi = hi.max(x);
            }
            (lo <= hi).then_some((lo, hi))
        };
        let mut any = false;
        for cy in y0.max(0)..=y1.min(rows as i64 - 1) {
            let top = rect.top() + cy as f32 * ch;
            // (a convex shape: the cells between where it crosses the
            // cell row's top and bottom edges are wholly inside - laid with
            // no samples)
            let (t, b) = (span(top), span(top + ch));
            let inner = match (t, b) {
                (Some(t), Some(b)) => Some((t.0.max(b.0), t.1.min(b.1))),
                _ => None,
            };
            // (and where it crosses four lines through the row: a cell at
            // its edge covered as much as those lines' lengths in it say -
            // not sixteen points tested a cell)
            let subs = [0.125f32, 0.375, 0.625, 0.875].map(|f| span(top + f * ch));
            for cx in x0.max(0)..=x1.min(cols as i64 - 1) {
                let corner = Pos2::new(rect.left() + cx as f32 * cw, top);
                let whole = inner.is_some_and(|(l, r)| corner.x >= l && corner.x + cw <= r);
                let share = if whole {
                    1.0
                } else {
                    let (l, r) = (corner.x, corner.x + cw);
                    subs.iter()
                        .flatten()
                        .map(|(a, b)| (b.min(r) - a.max(l)).max(0.0))
                        .sum::<f32>()
                        / (4.0 * cw)
                };
                if share > 0.03 {
                    any = true;
                    let k = (share * SPREAD).min(1.0);
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
        let mut touched = Vec::new();
        let mut onto = Onto {
            all: &mut all,
            big: &mut big,
            small: &mut small,
            parts: &mut parts,
            touched: &mut touched,
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

    /// A light ground with rounded corners - a fan of triangles from one
    /// corner, as egui tessellates it, long slivers of it reaching the
    /// corner across - laid on the cells of a 240 x 135 picture: the
    /// darkest cell within it.
    fn rounded_ground_laid() -> f32 {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(240.0, 135.0));
        let (cols, rows) = (128usize, 70usize);
        let (cw, ch) = (rect.width() / cols as f32, rect.height() / rows as f32);
        let radius = 10.0;
        let mut points = Vec::new();
        for (k, corner) in [
            rect.right_bottom() + egui::vec2(-radius, -radius),
            rect.left_bottom() + egui::vec2(radius, -radius),
            rect.left_top() + egui::vec2(radius, radius),
            rect.right_top() + egui::vec2(-radius, radius),
        ]
        .into_iter()
        .enumerate()
        {
            for j in 0..=8 {
                let angle = (k as f32 + j as f32 / 8.0) * std::f32::consts::FRAC_PI_2;
                points.push(corner + egui::Vec2::angled(angle) * radius);
            }
        }
        let mut mesh = Mesh::default();
        for p in &points {
            mesh.colored_vertex(*p, Color32::from_gray(236));
        }
        for i in 1..points.len() as u32 - 1 {
            mesh.add_triangle(0, i, i + 1);
        }
        let mut all = vec![[0.0f32; 3]; cols * rows];
        let mut big = all.clone();
        let mut small = vec![0.0f32; cols * rows];
        let mut parts = vec![Part::default(); cols * rows];
        let mut touched = Vec::new();
        let mut onto = Onto {
            all: &mut all,
            big: &mut big,
            small: &mut small,
            parts: &mut parts,
            touched: &mut touched,
        };
        lay_mesh(&mut onto, &mesh, None, rect, cols, rows, cw, ch, rect);
        let inset = (radius / cw).ceil() as usize + 1;
        let mut lo = f32::MAX;
        for cy in inset..rows - inset {
            for cx in inset..cols - inset {
                lo = lo.min(all[cy * cols + cx][0]);
            }
        }
        lo
    }

    #[test]
    fn a_rounded_ground_lies_even_on_the_cells() {
        // (its slivers laid evenly along their length, the cells along the
        // diagonal from the far corner held a quarter of the ground: a dark
        // wedge in the estimate on every light ground - review 18, M1; as
        // wide as they are at each place, every cell holds four fifths of
        // it and more)
        let lo = rounded_ground_laid();
        assert!(
            lo > 0.7 * 236.0 / 255.0,
            "the ground's darkest cell: {lo} of {}",
            236.0 / 255.0
        );
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
    fn flicker_in_antiphase_brings_the_veil() {
        // a checker of 5 x 5 tiles, each the other way from its
        // neighbours, at 8 Hz: never a quarter of a field changing the
        // same way at once - by area, a strobe
        let mut g = FlashGuard::default();
        g.layout(
            Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)),
            egui::vec2(384.0, 216.0),
        );
        let (cols, rows) = (g.cols, g.rows);
        for i in 0..120 {
            let on = (((i as f32 / 60.0) * 16.0) as u32).is_multiple_of(2);
            g.raw = (0..cols * rows)
                .map(|k| {
                    let tile = (k % cols) * 5 / cols + (k / cols) * 5 / rows;
                    let lit = if tile % 2 == 1 { !on } else { on };
                    let v = if lit { 0.58 } else { 0.007 };
                    [v, v, v]
                })
                .collect();
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
        }
        assert!(g.strength > 0.8, "the veil: {}", g.strength);
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

    /// A whole-face strobe of 8 Hz (and one of 5 Hz): the veil comes
    /// within the first three flashes - six transitions at most through it
    /// in the first second (review 11: four or five flashes went through
    /// before the veil, at the seventh transition).
    #[test]
    fn the_veil_comes_within_the_first_three_flashes() {
        for hz in [8.0f32, 5.0, 10.0] {
            let mut g = FlashGuard::default();
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0));
            g.layout(rect, egui::vec2(384.0, 216.0));
            let n = g.cols * g.rows;
            for i in 0..60 {
                let on = (i as f32 / 60.0 * hz).fract() < 0.5;
                let v = if on { 0.9 } else { 0.01 };
                g.raw = vec![[v, v, v]; n];
                g.raw_big = g.raw.clone();
                g.time += 1.0 / 60.0;
                g.frame += 1;
                g.follow(1.0 / 60.0);
            }
            let (raw, shown) = g.pressure();
            assert!(raw >= 8, "{hz} Hz: the strobe itself {raw}");
            assert!(
                shown <= 6,
                "{hz} Hz: {shown} transitions through the veil in the first second"
            );
        }
    }

    /// A whole-face flicker whose phase (in cycles) at `t` seconds is
    /// `phase(t)`, for `secs`: the most transitions through the veil in any
    /// second, and the share of frames it was veiled.
    fn flicker(phase: impl Fn(f32) -> f32, secs: f32) -> (usize, f32) {
        let mut g = FlashGuard::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0));
        g.layout(rect, egui::vec2(384.0, 216.0));
        let n = g.cols * g.rows;
        let (mut most, mut veiled) = (0, 0);
        let frames = (secs * 60.0) as usize;
        for i in 0..frames {
            let on = phase(i as f32 / 60.0).fract() < 0.5;
            let v = if on { 0.9 } else { 0.01 };
            g.raw = vec![[v, v, v]; n];
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
            g.frame += 1;
            g.follow(1.0 / 60.0);
            most = most.max(g.pressure().1);
            if g.strength > 0.3 {
                veiled += 1;
            }
        }
        (most, veiled as f32 / frames as f32)
    }

    /// A flicker that starts within the rule and speeds up past it is
    /// veiled before its seventh transition in a second (review 12: 3 Hz
    /// stepping to 4, 2 Hz rising to 5 over three seconds, 2 Hz stepping
    /// to 3.6 went through with 7 - 8 a second - the veil came after the
    /// seventh, and fast only from eight a second); one well within it,
    /// 2 Hz, is never veiled.
    #[test]
    fn a_flicker_that_speeds_up_is_veiled_in_time() {
        let step = |a: f32, b: f32, at: f32| {
            move |t: f32| if t < at { a * t } else { a * at + b * (t - at) }
        };
        let ramp = |a: f32, b: f32, d: f32| {
            move |t: f32| {
                let tt = t.min(d);
                a * tt + (b - a) * tt * tt / (2.0 * d) + if t > d { b * (t - d) } else { 0.0 }
            }
        };
        for (name, most) in [
            ("3 -> 4 Hz at 2 s", flicker(step(3.0, 4.0, 2.0), 5.0).0),
            ("2 -> 5 Hz over 3 s", flicker(ramp(2.0, 5.0, 3.0), 5.0).0),
            ("2 -> 3.6 Hz at 1.5 s", flicker(step(2.0, 3.6, 1.5), 5.0).0),
        ] {
            assert!(
                most <= 6,
                "{name}: {most} transitions in a second through the veil"
            );
        }
        let (_, veiled) = flicker(|t| 2.0 * t, 5.0);
        assert!(
            veiled < 0.01,
            "a 2 Hz flicker veiled {:.0} % of the time",
            veiled * 100.0
        );
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
