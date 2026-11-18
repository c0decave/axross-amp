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
/// How long a transition stays "at once" with others (two frames at 60 fps).
const AT_ONCE: f32 = 0.034;
/// A transition in a cell's relative luminance (WCAG's 0.1).
const STEP: f32 = 0.1;
/// Transitions in a field's last second from which the veil comes (five:
/// a beat's pulse at 120 BPM - four - is within the rule and left
/// alone; the sixth and seventh, which would break it, are veiled).
const RAW_BUDGET: usize = 5;
/// And from which, seen through the veil, it closes further.
const SHOWN_BUDGET: usize = 5;

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
    /// When each cell last transited (s), and which way.
    lum_at: Vec<f32>,
    red_at: Vec<f32>,
    lum_dir: Vec<i8>,
    red_dir: Vec<i8>,
    /// The same of the cells' large content alone (see `step`), and how
    /// much of a cell (sixteenths) its last transition is taken for.
    lum_big: Vec<Track>,
    red_big: Vec<Track>,
    lum_w: Vec<u8>,
    red_w: Vec<u8>,
    /// Each field: whether it was over the quarter (luminance up, down; red
    /// up, down), its transitions (s).
    over: Vec<[bool; 4]>,
    events: Vec<Vec<f32>>,
    /// Counts of this frame's changes, summed (four kinds a cell).
    integral: Vec<u32>,
}

impl Watch {
    fn reset(&mut self, cells: usize, fields: usize) {
        self.lum = vec![Track::default(); cells];
        self.red = vec![Track::default(); cells];
        self.lum_at = vec![f32::NEG_INFINITY; cells];
        self.red_at = vec![f32::NEG_INFINITY; cells];
        self.lum_dir = vec![0; cells];
        self.red_dir = vec![0; cells];
        self.lum_big = vec![Track::default(); cells];
        self.red_big = vec![Track::default(); cells];
        self.lum_w = vec![0; cells];
        self.red_w = vec![0; cells];
        self.over = vec![[false; 4]; fields];
        self.events = (0..fields).map(|_| Vec::with_capacity(16)).collect();
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
    fn step(
        &mut self,
        cells: &[[f32; 3]],
        big: &[[f32; 3]],
        small: &[f32],
        fields: &[Field],
        cols: usize,
        now: f32,
    ) {
        let luminance = |c: &[f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let red_of = |c: &[f32; 3]| {
            // (saturated red in the sRGB values, as WCAG reckons it)
            // (no red unless red outweighs the rest: most cells, no powers)
            if c[0] > 0.0005 && c[0] > (c[1] + c[2]) * 2.0 {
                let (r, g, b) = (to_srgb(c[0]), to_srgb(c[1]), to_srgb(c[2]));
                let sum = r + g + b;
                if sum > 0.0 && r / sum >= 0.8 {
                    ((r - g - b) * 320.0).max(0.0)
                } else {
                    0.0
                }
            } else {
                0.0
            }
        };
        for (i, c) in cells.iter().enumerate() {
            let part = (small[i].min(1.0) * 16.0).ceil() as u8;
            let all = self.lum[i].step(luminance(c), STEP, Some(0.8));
            let large = self.lum_big[i].step(luminance(&big[i]), STEP, Some(0.8));
            if large != 0 {
                self.lum_at[i] = now;
                self.lum_dir[i] = large;
                self.lum_w[i] = 16;
            } else if all != 0 && part > 0 {
                self.lum_at[i] = now;
                self.lum_dir[i] = all;
                self.lum_w[i] = part;
            }
            let all = self.red[i].step(red_of(c), 14.0, None);
            let large = self.red_big[i].step(red_of(&big[i]), 14.0, None);
            if large != 0 {
                self.red_at[i] = now;
                self.red_dir[i] = large;
                self.red_w[i] = 16;
            } else if all != 0 && part > 0 {
                self.red_at[i] = now;
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
                if now - self.lum_at[i] < AT_ONCE {
                    row[usize::from(self.lum_dir[i] > 0)] += u32::from(self.lum_w[i]);
                }
                if now - self.red_at[i] < AT_ONCE {
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
            // (each way on its own: brightening, then darkening, is two)
            // (in sixteenths of a cell)
            let quarter = field.cells.len() * 4;
            let is = [
                lum[1] >= quarter,
                lum[0] >= quarter,
                red[1] >= quarter,
                red[0] >= quarter,
            ];
            for (j, now_over) in is.iter().enumerate() {
                if *now_over && !self.over[k][j] {
                    self.events[k].push(now);
                }
            }
            self.over[k] = is;
            // (a second: less than one, as frames count it - 59 apart at
            // 60 a second is the most in a second)
            self.events[k].retain(|t| now - t < 0.99);
        }
    }

    fn recent(&self, field: usize) -> usize {
        self.events[field].len()
    }
}

/// A field: its cells, and its rectangle of them (x, y, w, h).
struct Field {
    cells: Vec<usize>,
    at: (usize, usize, usize, usize),
}

/// The guard's state (one for the picture shown).
pub struct FlashGuard {
    cols: usize,
    rows: usize,
    rect: Rect,
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
    target: Vec<f32>,
    /// The estimate as laid on (sRGB values), kept for the next frame.
    acc: Vec<[f32; 3]>,
    acc_big: Vec<[f32; 3]>,
    /// Room for smoothing.
    blur: Vec<[f32; 3]>,
    blur_one: Vec<f32>,
}

impl Default for FlashGuard {
    fn default() -> Self {
        FlashGuard {
            cols: 0,
            rows: 0,
            rect: Rect::NOTHING,
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
            target: Vec::new(),
            acc: Vec::new(),
            acc_big: Vec::new(),
            blur: Vec::new(),
            blur_one: Vec::new(),
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
    fn layout(&mut self, rect: Rect) {
        let rows =
            ((COLS as f32 * rect.height() / rect.width().max(1.0)).round() as usize).clamp(6, 128);
        if self.cols == COLS && self.rows == rows && self.rect.size() == rect.size() {
            self.rect = rect;
            return;
        }
        self.cols = COLS;
        self.rows = rows;
        self.rect = rect;
        let n = COLS * rows;
        // the fields: a third each way, slid in twelfths
        let (fw, fh) = ((COLS / 3).max(1), (rows / 3).max(1));
        let (sx, sy) = ((COLS / 12).max(1), (rows / 12).max(1));
        self.fields.clear();
        let mut y = 0;
        while y + fh <= rows {
            let mut x = 0;
            while x + fw <= COLS {
                let cells = (y..y + fh)
                    .flat_map(|cy| (x..x + fw).map(move |cx| cy * COLS + cx))
                    .collect();
                self.fields.push(Field {
                    cells,
                    at: (x, y, fw, fh),
                });
                x += sx;
            }
            y += sy;
        }
        let f = self.fields.len();
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
        dt: f32,
    ) {
        if rect.width() < 8.0 || rect.height() < 8.0 {
            return;
        }
        self.layout(rect);
        if dt > 0.0 {
            self.time += dt;
            self.estimate(painter, first, drawn);
            self.follow(dt);
        }
        self.draw(painter);
    }

    /// The picture as the faces drew it, on the cells.
    fn estimate(&mut self, painter: &egui::Painter, first: usize, drawn: &[gpu::Drawn]) {
        let (cols, rows, rect) = (self.cols, self.rows, self.rect);
        // (in sRGB values while laying on, as egui blends; made linear after)
        let mut acc = std::mem::take(&mut self.acc);
        acc.clear();
        acc.resize(cols * rows, [0.0f32; 3]);
        let mut big = std::mem::take(&mut self.acc_big);
        big.clear();
        big.resize(cols * rows, [0.0f32; 3]);
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
                        };
                        lay_mesh(&mut onto, mesh, rect, cols, rows, cw, ch, p.clip_rect);
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
        // (each cell with its neighbours: the estimate of thin and small
        // shapes jitters from cell to cell as they move - quantised where
        // they lie - which is not the picture flashing; a flash's area,
        // a quarter of a field, is hundreds of cells and stays)
        let (cols, rows) = (self.cols, self.rows);
        smooth(&mut self.raw, &mut self.blur, cols, rows);
        smooth(&mut self.raw_big, &mut self.blur, cols, rows);
        smooth_one(&mut self.small_most, &mut self.blur_one, cols, rows);
        for i in 0..self.raw.len() {
            let a = self.veil[i];
            for k in 0..3 {
                self.shown[i][k] = self.raw[i][k] * (1.0 - a) + self.slow[i][k] * a;
                self.shown_big[i][k] = self.raw_big[i][k] * (1.0 - a) + self.slow[i][k] * a;
            }
        }
        self.raw_watch.step(
            &self.raw,
            &self.raw_big,
            &self.small_most,
            &self.fields,
            self.cols,
            now,
        );
        self.shown_watch.step(
            &self.shown,
            &self.shown_big,
            &self.small_most,
            &self.fields,
            self.cols,
            now,
        );
        // each field's wanted veil: from the fifth raw transition in a
        // second, held a second after the last; closer where the veiled
        // picture itself still transits
        for k in 0..self.fields.len() {
            let raw = self.raw_watch.recent(k);
            let shown = self.shown_watch.recent(k);
            let want = if shown >= SHOWN_BUDGET {
                0.95
            } else if raw >= RAW_BUDGET {
                0.85
            } else {
                0.0
            };
            let (level, until) = &mut self.hold[k];
            if want >= *level || now > *until {
                if want > 0.0 {
                    *until = now + 1.0;
                }
                *level = want.max(if now > *until { 0.0 } else { *level });
            }
        }
        // each cell: the strongest of its fields, eased (on fast, off slow)
        let mut target = std::mem::take(&mut self.target);
        target.clear();
        target.resize(self.veil.len(), 0.0);
        for (k, field) in self.fields.iter().enumerate() {
            let level = self.hold[k].0;
            if level > 0.0 {
                for &i in &field.cells {
                    target[i] = target[i].max(level);
                }
            }
        }
        // (feathered: the veil falls off over a few cells outside its
        // fields, so that it has no edge to see)
        feather(&mut target, self.cols, self.rows, (self.cols / 24).max(2));
        let on = 1.0 - (-dt / 0.03).exp();
        let off = 1.0 - (-dt / 0.6).exp();
        let follow = 1.0 - (-dt / 0.5).exp();
        let mut strongest = 0.0f32;
        for i in 0..self.veil.len() {
            let t = target[i];
            let k = if t > self.veil[i] { on } else { off };
            self.veil[i] += (t - self.veil[i]) * k;
            strongest = strongest.max(self.veil[i]);
            // the slow picture the veil blends towards
            for c in 0..3 {
                self.slow[i][c] += (self.raw[i][c] - self.slow[i][c]) * follow;
            }
        }
        self.strength = strongest;
        self.target = target;
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
        if self.strength < 0.01 {
            return;
        }
        painter.add(egui::Shape::mesh(self.veil_mesh()));
    }

    fn veil_mesh(&self) -> Mesh {
        let (cols, rows, rect) = (self.cols, self.rows, self.rect);
        let cw = rect.width() / cols as f32;
        let ch = rect.height() / rows as f32;
        let cell = |cx: usize, cy: usize| {
            let i = cy * cols + cx;
            let a = self.veil[i];
            let s = self.slow[i];
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

/// Each cell the mean of itself and its neighbours (3 x 3; `room` the
/// scratch it needs).
fn smooth(cells: &mut [[f32; 3]], room: &mut Vec<[f32; 3]>, cols: usize, rows: usize) {
    room.clear();
    room.extend_from_slice(cells);
    for y in 0..rows {
        for x in 0..cols {
            let mut sum = [0.0f32; 3];
            let mut n = 0.0;
            for yy in y.saturating_sub(1)..(y + 2).min(rows) {
                for xx in x.saturating_sub(1)..(x + 2).min(cols) {
                    let c = room[yy * cols + xx];
                    for k in 0..3 {
                        sum[k] += c[k];
                    }
                    n += 1.0;
                }
            }
            cells[y * cols + x] = [sum[0] / n, sum[1] / n, sum[2] / n];
        }
    }
}

fn smooth_one(cells: &mut [f32], room: &mut Vec<f32>, cols: usize, rows: usize) {
    room.clear();
    room.extend_from_slice(cells);
    for y in 0..rows {
        for x in 0..cols {
            let mut sum = 0.0;
            let mut n = 0.0;
            for yy in y.saturating_sub(1)..(y + 2).min(rows) {
                for xx in x.saturating_sub(1)..(x + 2).min(cols) {
                    sum += room[yy * cols + xx];
                    n += 1.0;
                }
            }
            cells[y * cols + x] = sum / n;
        }
    }
}

/// Spreads each cell's veil to its neighbours, falling off linearly over
/// `reach` cells (a cone: rows, then columns).
fn feather(target: &mut [f32], cols: usize, rows: usize, reach: usize) {
    let fall = 1.0 / (reach as f32 + 1.0);
    let mut pass = |at: &dyn Fn(usize, usize) -> usize, lines: usize, len: usize| {
        for l in 0..lines {
            let mut run = 0.0f32;
            for k in 0..len {
                let i = at(l, k);
                run = (run - fall).max(target[i]);
                target[i] = run;
            }
            run = 0.0;
            for k in (0..len).rev() {
                let i = at(l, k);
                run = (run - fall).max(target[i]);
                target[i] = run;
            }
        }
    };
    pass(&|l, k| l * cols + k, rows, cols);
    pass(&|l, k| k * cols + l, cols, rows);
}

/// What a mesh is laid onto: the whole picture, its large content alone
/// (shapes covering whole cells), and how much of each cell small shapes
/// cover.
struct Onto<'a> {
    all: &'a mut [[f32; 3]],
    big: &'a mut [[f32; 3]],
    small: &'a mut [f32],
}

/// A tessellated mesh laid onto the cells (sRGB values, premultiplied).
#[allow(clippy::too_many_arguments)]
fn lay_mesh(
    acc: &mut Onto<'_>,
    mesh: &Mesh,
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
    for tri in mesh.indices.as_chunks::<3>().0 {
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
            let centre = Pos2::new(
                rect.left() + (cx as f32 + 0.5) * cw,
                rect.top() + (cy as f32 + 0.5) * ch,
            );
            if !clip.contains(centre) {
                return;
            }
            let at = cy as usize * cols + cx as usize;
            let a = (mean[3] * k).min(1.0);
            let lay = |cell: &mut [f32; 3]| {
                for i in 0..3 {
                    cell[i] = mean[i] * k + cell[i] * (1.0 - a);
                }
            };
            lay(&mut acc.all[at]);
            // (how much it covers: without the half again - an additive
            // glow too, which has no alpha)
            if k >= 0.99 {
                lay(&mut acc.big[at]);
            } else if a > 0.004 || mean[0] + mean[1] + mean[2] > 0.004 {
                acc.small[at] += k / 1.5;
            }
        };
        // each cell it touches, by how much of it it covers (4 x 4 samples;
        // half as much again: a line drawn a pixel or two wide covers more
        // than its geometry - better too much than too little)
        let inside = |p: Pos2| {
            let d0 = (p1.x - p0.x) * (p.y - p0.y) - (p1.y - p0.y) * (p.x - p0.x);
            let d1 = (p2.x - p1.x) * (p.y - p1.y) - (p2.y - p1.y) * (p.x - p1.x);
            let d2 = (p0.x - p2.x) * (p.y - p2.y) - (p0.y - p2.y) * (p.x - p2.x);
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        };
        // (small - within four cells each way: its share where it lies, no
        // samples; most of a line's or a dot's pieces)
        if x1 - x0 <= 3 && y1 - y0 <= 3 {
            let centroid = Pos2::new((p0.x + p1.x + p2.x) / 3.0, (p0.y + p1.y + p2.y) / 3.0);
            let (cx, cy) = cell_of(centroid);
            blend(acc, cx, cy, (1.5 * area.abs() / (cw * ch)).min(1.0));
            continue;
        }
        // (thin - a line's long piece: its area along its length, half a
        // cell at a time, not every cell of its box sampled)
        let bbox = (max.x - min.x).max(1e-3) * (max.y - min.y).max(1e-3);
        if area.abs() < bbox * 0.25 {
            let edges = [(p0, p1, p2), (p1, p2, p0), (p2, p0, p1)];
            let (a, b, c) = edges
                .into_iter()
                .max_by(|x, y| (x.1 - x.0).length().total_cmp(&(y.1 - y.0).length()))
                .unwrap_or((p0, p1, p2));
            let length = (b - a).length();
            let n = ((length / (cw.min(ch) * 0.5)).ceil() as usize).max(1);
            let towards = (c - Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)) / 3.0;
            let share = (1.5 * area.abs() / n as f32 / (cw * ch)).min(1.0);
            for k in 0..n {
                let p = a + (b - a) * ((k as f32 + 0.5) / n as f32) + towards;
                let (cx, cy) = cell_of(p);
                blend(acc, cx, cy, share);
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
                    blend(acc, cx, cy, (n as f32 / 16.0 * 1.5).min(1.0));
                }
            }
        }
        if !any {
            // (thinner than the samples: its share of the cell it lies in)
            let centroid = Pos2::new((p0.x + p1.x + p2.x) / 3.0, (p0.y + p1.y + p2.y) / 3.0);
            let (cx, cy) = cell_of(centroid);
            blend(acc, cx, cy, (2.0 * area.abs() / (cw * ch)).min(1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strobe_in_the_estimate_brings_the_veil() {
        let mut g = FlashGuard::default();
        g.layout(Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)));
        let n = g.raw.len();
        for i in 0..120 {
            let on = (((i as f32 / 60.0) * 10.0) as u32).is_multiple_of(2);
            let v = if on { 0.6 } else { 0.02 };
            g.raw = vec![[v, v, v]; n];
            g.raw_big = g.raw.clone();
            g.time += 1.0 / 60.0;
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
        g.layout(Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)));
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
            g.follow(1.0 / 60.0);
        }
        g
    }

    /// Small shapes covering `cover` of every cell, all brightening and
    /// darkening at once 3.5 times a second, over a ground that stays.
    fn twinkle(cover: f32) -> FlashGuard {
        let mut g = FlashGuard::default();
        g.layout(Rect::from_min_size(Pos2::ZERO, egui::vec2(384.0, 216.0)));
        let n = g.raw.len();
        for i in 0..120 {
            let on = (i as f32 / 60.0 * 3.5).fract() < 0.5;
            let v = if on { 0.02 + 0.6 * cover } else { 0.02 };
            g.raw = vec![[v, v, v]; n];
            g.raw_big = vec![[0.02; 3]; n];
            g.small = vec![cover; n];
            g.time += 1.0 / 60.0;
            g.follow(1.0 / 60.0);
        }
        g
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
