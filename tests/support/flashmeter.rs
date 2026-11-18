//! A flash meter after WCAG 2.3.1 / 2.3.2 and ITU-R BT.1702: frames in,
//! the most transitions in any second over any area that counts.
//!
//! - Each pixel's relative luminance (sRGB made linear, then Y) is
//!   followed in time: a *transition* is a change of at least 0.1 from its
//!   last highest to a lowest (or back) with the darker below 0.8.
//! - The area: WCAG's quarter of a 10° field of view - 21,824 px² of a
//!   1024 x 768 screen (a field 341 x 256: a third of the screen each way,
//!   the quarter of it 2.8 % of the screen). The picture is taken as the
//!   whole screen (the face full screen: the strictest case), so a field is
//!   a third of it each way, slid over it in twelfths; a field *transits*
//!   when, within three frames (50 ms: at once), the pixels transiting in it
//!   in the same direction (all brightening, or all darkening: a flash is
//!   an area changing together) cover a quarter of it - each rise of that
//!   above the quarter one.
//! - Red: likewise in (R - G - B) x 320 of saturated red pixels (R / (R + G
//!   + B) at least 0.8), a change of at least 20.
//! - Patterns (BT.1702, the Harding test): regular stripes - at least five
//!   light-dark pairs in a row or a column, each step at least 0.1 of the
//!   luminance with the darker below 0.8, periods alike - counted over the
//!   picture; a hazard where they cover a quarter of it and move or flicker
//!   (a third of the striped pixels changing by 0.1 from the frame before),
//!   or where they cover 40 % of it even standing still.
//!
//! WCAG allows three flashes (six transitions) in any second.
#![allow(dead_code)]

pub const FPS: f32 = 60.0;

/// sRGB (0..255) to linear light.
pub fn linear(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// One series' transition tracker.
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
                let counts = dark(v);
                self.dir = -1;
                self.extreme = v;
                return if counts { -1 } else { 0 };
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

pub struct Meter {
    w: usize,
    h: usize,
    frame: usize,
    lum: Vec<Track>,
    red: Vec<Track>,
    /// The frame of each pixel's last transition (luminance, red) and its
    /// direction.
    lum_at: Vec<usize>,
    red_at: Vec<usize>,
    lum_dir: Vec<i8>,
    red_dir: Vec<i8>,
    /// Each field's transitions (frames) and whether it was over the
    /// quarter last frame.
    fields: Vec<(usize, usize, usize, usize)>,
    lum_events: Vec<Vec<usize>>,
    red_events: Vec<Vec<usize>>,
    lum_over: Vec<[bool; 2]>,
    red_over: Vec<[bool; 2]>,
    /// Frames with a pattern hazard, and the largest share of stripes
    /// (standing or moving).
    pub pattern_frames: usize,
    pub stripes: f32,
    last_lum: Vec<f32>,
}

/// Frames a transition stays "at once" with others (two at 60 fps).
const AT_ONCE: usize = 2;

impl Meter {
    pub fn new(w: usize, h: usize) -> Meter {
        let (fw, fh) = (w / 3, h / 3);
        let mut fields = Vec::new();
        let (sx, sy) = ((w / 12).max(1), (h / 12).max(1));
        let mut y = 0;
        while y + fh <= h {
            let mut x = 0;
            while x + fw <= w {
                fields.push((x, y, fw, fh));
                x += sx;
            }
            y += sy;
        }
        let n = fields.len();
        Meter {
            w,
            h,
            frame: 0,
            lum: vec![Track::default(); w * h],
            red: vec![Track::default(); w * h],
            lum_at: vec![usize::MAX; w * h],
            red_at: vec![usize::MAX; w * h],
            lum_dir: vec![0; w * h],
            red_dir: vec![0; w * h],
            fields,
            lum_events: vec![Vec::new(); n],
            red_events: vec![Vec::new(); n],
            lum_over: vec![[false; 2]; n],
            red_over: vec![[false; 2]; n],
            pattern_frames: 0,
            stripes: 0.0,
            last_lum: Vec::new(),
        }
    }

    pub fn push(&mut self, img: &image::RgbaImage) {
        let (w, h) = (self.w, self.h);
        assert_eq!((img.width() as usize, img.height() as usize), (w, h));
        let f = self.frame;
        let mut lum_now = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let p = img.get_pixel(x as u32, y as u32);
                let i = y * w + x;
                let l = 0.2126 * linear(p[0]) + 0.7152 * linear(p[1]) + 0.0722 * linear(p[2]);
                lum_now[i] = l;
                let d = self.lum[i].step(l, 0.1, Some(0.8));
                if d != 0 {
                    self.lum_at[i] = f;
                    self.lum_dir[i] = d;
                }
                let (r, g, b) = (
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                );
                let sum = r + g + b;
                let red = if sum > 0.0 && r / sum >= 0.8 {
                    ((r - g - b) * 320.0).max(0.0)
                } else {
                    0.0
                };
                let d = self.red[i].step(red, 20.0, None);
                if d != 0 {
                    self.red_at[i] = f;
                    self.red_dir[i] = d;
                }
            }
        }
        // integral images of "transited at once" (luminance, red)
        let recent = |at: usize| at != usize::MAX && f - at < AT_ONCE;
        let integral = |at: &[usize], dirs: &[i8], dir: i8| {
            let mut s = vec![0u32; (w + 1) * (h + 1)];
            for y in 0..h {
                let mut row = 0u32;
                for x in 0..w {
                    let i = y * w + x;
                    row += u32::from(recent(at[i]) && dirs[i] == dir);
                    s[(y + 1) * (w + 1) + x + 1] = s[y * (w + 1) + x + 1] + row;
                }
            }
            s
        };
        let (lu, ld) = (
            integral(&self.lum_at, &self.lum_dir, 1),
            integral(&self.lum_at, &self.lum_dir, -1),
        );
        let (ru, rd) = (
            integral(&self.red_at, &self.red_dir, 1),
            integral(&self.red_at, &self.red_dir, -1),
        );
        let sum = |s: &[u32], (x, y, fw, fh): (usize, usize, usize, usize)| {
            let at = |x: usize, y: usize| s[y * (w + 1) + x] as i64;
            (at(x + fw, y + fh) - at(x, y + fh) - at(x + fw, y) + at(x, y)) as usize
        };
        for (k, field) in self.fields.iter().enumerate() {
            let quarter = field.2 * field.3 / 4;
            // (each way on its own: brightening, then darkening, is two)
            let over = [sum(&lu, *field) >= quarter, sum(&ld, *field) >= quarter];
            for j in 0..2 {
                if over[j] && !self.lum_over[k][j] {
                    self.lum_events[k].push(f);
                }
            }
            self.lum_over[k] = over;
            let over = [sum(&ru, *field) >= quarter, sum(&rd, *field) >= quarter];
            for j in 0..2 {
                if over[j] && !self.red_over[k][j] {
                    self.red_events[k].push(f);
                }
            }
            self.red_over[k] = over;
        }
        // stripes: a hazard over 40 %, or over 25 % where they move
        let (share, marked) = stripes_marked(&lum_now, w, h);
        self.stripes = self.stripes.max(share);
        let moving = if self.last_lum.len() == lum_now.len() && share > 0.0 {
            let (mut n, mut changed) = (0usize, 0usize);
            for (i, m) in marked.iter().enumerate() {
                if *m {
                    n += 1;
                    if (lum_now[i] - self.last_lum[i]).abs() >= 0.1 {
                        changed += 1;
                    }
                }
            }
            changed as f32 >= n as f32 / 3.0
        } else {
            false
        };
        if share >= 0.4 || (share >= 0.25 && moving) {
            self.pattern_frames += 1;
        }
        self.last_lum = lum_now;
        self.frame += 1;
    }

    /// The worst field's transitions (frames) and place (a probe).
    pub fn worst_field(&self) -> (Vec<usize>, (usize, usize, usize, usize)) {
        let best = (0..self.fields.len())
            .max_by_key(|k| self.lum_events[*k].len())
            .unwrap_or(0);
        (self.lum_events[best].clone(), self.fields[best])
    }

    /// The most transitions in any second over any field (luminance, red).
    pub fn most(&self) -> (usize, usize) {
        let worst = |events: &[Vec<usize>]| {
            events
                .iter()
                .map(|e| {
                    (0..e.len())
                        .map(|k| {
                            e[k..]
                                .iter()
                                .take_while(|&&f| f < e[k] + FPS as usize)
                                .count()
                        })
                        .max()
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0)
        };
        (worst(&self.lum_events), worst(&self.red_events))
    }
}

/// The share of the picture lying in regular stripes (rows and columns).
pub fn stripes(lum: &[f32], w: usize, h: usize) -> f32 {
    stripes_marked(lum, w, h).0
}

/// That share, and which pixels lie in them.
pub fn stripes_marked(lum: &[f32], w: usize, h: usize) -> (f32, Vec<bool>) {
    let mut marked = vec![false; w * h];
    let run_line = |get: &dyn Fn(usize) -> f32, n: usize, mark: &mut dyn FnMut(usize)| {
        // the turning points with a step of at least 0.1 (darker below 0.8)
        let mut turns: Vec<usize> = Vec::new();
        let mut t = Track::default();
        for i in 0..n {
            if t.step(get(i), 0.1, Some(0.8)) != 0 {
                turns.push(i);
            }
        }
        // runs of at least 10 turns (5 pairs) with alike spacing
        let mut start = 0;
        while start + 10 <= turns.len() {
            let mut end = start + 1;
            let first = turns[start + 1] - turns[start];
            while end + 1 < turns.len() {
                let gap = turns[end + 1] - turns[end];
                if (gap as f32) > first as f32 * 1.6 + 1.0
                    || (gap as f32) < first as f32 / 1.6 - 1.0
                {
                    break;
                }
                end += 1;
            }
            if end - start + 1 >= 10 {
                for i in turns[start]..=turns[end] {
                    mark(i);
                }
                start = end;
            } else {
                start += 1;
            }
        }
    };
    for y in 0..h {
        run_line(&|x| lum[y * w + x], w, &mut |x| marked[y * w + x] = true);
    }
    for x in 0..w {
        run_line(&|y| lum[y * w + x], h, &mut |y| marked[y * w + x] = true);
    }
    let share = marked.iter().filter(|m| **m).count() as f32 / (w * h) as f32;
    (share, marked)
}

/// The worst music for a picture (stereo, interleaved, 48 kHz), each part
/// `part` seconds: a kick at 120 BPM and at 240, white noise gated at
/// 5.5 Hz, bass and treble in turn at steady loudness, a sweep 40 Hz - 12
/// kHz five times a second, the left and the right in turn, noise in bursts
/// four times a second, silence and full scale a second each.
pub fn worst_music(part: f32) -> Vec<f32> {
    const RATE: f32 = 48_000.0;
    let mut seed = 11u32;
    let mut noise = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5
    };
    let tau = std::f32::consts::TAU;
    let parts = 8.0;
    let n = (part * parts * RATE) as usize;
    let mut out = Vec::with_capacity(n * 2);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / RATE;
        let k = (t / part) as usize;
        let tp = t - k as f32 * part;
        let (l, r) = match k {
            0 | 1 => {
                let bpm = if k == 0 { 120.0 } else { 240.0 };
                let beat = (tp * bpm / 60.0).fract();
                let v = (-beat * 18.0).exp() * (tp * 55.0 * tau).sin() * 0.95
                    + (tp * 220.0 * tau).sin() * 0.08;
                (v, v)
            }
            2 => {
                let v = if (tp * 5.5).fract() < 0.5 {
                    noise() * 1.9
                } else {
                    0.0
                };
                (v, v)
            }
            3 => {
                // as loud either way: bass, then hiss and 6 kHz
                let v = if (tp * 6.0).fract() < 0.5 {
                    (tp * 60.0 * tau).sin() * 0.7
                } else {
                    noise() * 1.0 + (tp * 6000.0 * tau).sin() * 0.35
                };
                (v, v)
            }
            4 => {
                let f = 40.0 * 300f32.powf((tp * 5.0).fract());
                phase += f * tau / RATE;
                let v = phase.sin() * 0.95;
                (v, v)
            }
            5 => {
                let v = (tp * 220.0 * tau).sin() * 0.9 + noise() * 0.3;
                if (tp * 7.0).fract() < 0.5 {
                    (v, 0.0)
                } else {
                    (0.0, v)
                }
            }
            6 => {
                let v = if (tp * 4.0).fract() < 0.5 {
                    noise() * 1.6
                } else {
                    0.0
                };
                (v, v)
            }
            _ => {
                let v = if (tp as u32).is_multiple_of(2) {
                    0.0
                } else {
                    (tp * 110.0 * tau).sin() * 0.4 + (tp * 440.0 * tau).sin() * 0.3 + noise() * 0.5
                };
                (v, v)
            }
        };
        out.push(l.clamp(-1.0, 1.0));
        out.push(r.clamp(-1.0, 1.0));
    }
    out
}

#[cfg(test)]
mod tests {}
