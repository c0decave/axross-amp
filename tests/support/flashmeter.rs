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
//!   when, within 34 ms (at once: two frames at 60 a second, one at 30 or
//!   fewer), the pixels transiting in it in the same direction (all
//!   brightening, or all darkening: a flash is an area changing together)
//!   cover a quarter of it - each rise of that above the quarter one, the
//!   other way from the one before (a flash is a pair of opposing changes).
//! - Red: likewise in (R - G - B) x 320 of saturated red pixels (R over
//!   R + G + B at least 0.8), a change of at least 20, with R, G and B
//!   linear, as in relative luminance (WCAG 2.0's definition).
//! - Patterns (BT.1702, the Harding test): regular stripes - at least five
//!   light-dark pairs in a row or a column, each step at least 0.1 of the
//!   luminance with the darker below 0.8, full periods alike (a light and
//!   a dark band together: uneven gratings, 3 on and 9 off, are gratings),
//!   counted over the picture; a hazard where they cover a quarter of it
//!   and move or flicker (a third of the striped pixels changed by 0.1
//!   within a tenth of a second: a slow drift too, not only from one frame
//!   to the next), or where they cover 40 % of it even standing still.
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
    /// Frames a second, and frames "at once".
    fps: usize,
    at_once: usize,
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
    /// The frames with a pattern hazard (the first few).
    pub hazard_at: Vec<usize>,
    /// The last frames' luminance (a tenth of a second of them).
    past: std::collections::VecDeque<Vec<f32>>,
    /// The last way each field's luminance and red transited.
    lum_last: Vec<i8>,
    red_last: Vec<i8>,
    /// The strict reading, counted alongside: "at once" within 100 ms, and
    /// every rise above the quarter a transition (not only one the other
    /// way from the last) - an independent review's meter.
    strict_at_once: usize,
    strict_lum_events: Vec<Vec<usize>>,
    strict_red_events: Vec<Vec<usize>>,
    strict_lum_over: Vec<[bool; 2]>,
    strict_red_over: Vec<[bool; 2]>,
}

impl Meter {
    pub fn new(w: usize, h: usize) -> Meter {
        Meter::with_fps(w, h, 60)
    }

    /// A meter for frames shown `fps` a second.
    pub fn with_fps(w: usize, h: usize, fps: usize) -> Meter {
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
            fps,
            at_once: ((0.034 * fps as f32).round() as usize).max(1),
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
            hazard_at: Vec::new(),
            past: std::collections::VecDeque::new(),
            lum_last: vec![0; n],
            red_last: vec![0; n],
            strict_at_once: ((0.1 * fps as f32).round() as usize).max(1),
            strict_lum_events: vec![Vec::new(); n],
            strict_red_events: vec![Vec::new(); n],
            strict_lum_over: vec![[false; 2]; n],
            strict_red_over: vec![[false; 2]; n],
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
                let (r, g, b) = (linear(p[0]), linear(p[1]), linear(p[2]));
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
        let at_once = self.at_once;
        let recent = |at: usize| at != usize::MAX && f - at < at_once;
        let strict_at_once = self.strict_at_once;
        let strict_recent = |at: usize| at != usize::MAX && f - at < strict_at_once;
        let strict_integral = |at: &[usize], dirs: &[i8], dir: i8| {
            let mut s = vec![0u32; (w + 1) * (h + 1)];
            for y in 0..h {
                let mut row = 0u32;
                for x in 0..w {
                    let i = y * w + x;
                    row += u32::from(strict_recent(at[i]) && dirs[i] == dir);
                    s[(y + 1) * (w + 1) + x + 1] = s[y * (w + 1) + x + 1] + row;
                }
            }
            s
        };
        let (slu, sld) = (
            strict_integral(&self.lum_at, &self.lum_dir, 1),
            strict_integral(&self.lum_at, &self.lum_dir, -1),
        );
        let (sru, srd) = (
            strict_integral(&self.red_at, &self.red_dir, 1),
            strict_integral(&self.red_at, &self.red_dir, -1),
        );
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
            // (a transition the other way from the field's last one)
            let over = [sum(&lu, *field) >= quarter, sum(&ld, *field) >= quarter];
            for j in 0..2 {
                let way = if j == 0 { 1 } else { -1 };
                if over[j] && !self.lum_over[k][j] && self.lum_last[k] != way {
                    self.lum_events[k].push(f);
                    self.lum_last[k] = way;
                }
            }
            self.lum_over[k] = over;
            let over = [sum(&ru, *field) >= quarter, sum(&rd, *field) >= quarter];
            for j in 0..2 {
                let way = if j == 0 { 1 } else { -1 };
                if over[j] && !self.red_over[k][j] && self.red_last[k] != way {
                    self.red_events[k].push(f);
                    self.red_last[k] = way;
                }
            }
            self.red_over[k] = over;
            // (the strict reading: every rise, either way)
            for (lu, ld, overs, events) in [
                (
                    &slu,
                    &sld,
                    &mut self.strict_lum_over[k],
                    &mut self.strict_lum_events[k],
                ),
                (
                    &sru,
                    &srd,
                    &mut self.strict_red_over[k],
                    &mut self.strict_red_events[k],
                ),
            ] {
                let over = [sum(lu, *field) >= quarter, sum(ld, *field) >= quarter];
                for j in 0..2 {
                    if over[j] && !overs[j] {
                        events.push(f);
                    }
                }
                *overs = over;
            }
        }
        // stripes: a hazard over 40 %, or over 25 % where they move
        let (share, marked) = stripes_marked(&lum_now, w, h);
        self.stripes = self.stripes.max(share);
        // (moving: against each of the last tenth of a second's frames - a
        // slow drift changes little from one frame to the next)
        let moving = share > 0.0
            && self.past.iter().any(|before| {
                let (mut n, mut changed) = (0usize, 0usize);
                for (i, m) in marked.iter().enumerate() {
                    if *m {
                        n += 1;
                        if (lum_now[i] - before[i]).abs() >= 0.1 {
                            changed += 1;
                        }
                    }
                }
                changed as f32 >= n as f32 / 3.0
            });
        if share >= 0.4 || (share >= 0.25 && moving) {
            self.pattern_frames += 1;
            if self.hazard_at.len() < 8 {
                self.hazard_at.push(f);
            }
        }
        self.past.push_back(lum_now);
        while self.past.len() > (self.fps / 10).max(1) {
            self.past.pop_front();
        }
        self.frame += 1;
    }

    /// The worst field's transitions (frames) and place (a probe).
    pub fn worst_field(&self) -> (Vec<usize>, (usize, usize, usize, usize)) {
        let best = (0..self.fields.len())
            .max_by_key(|k| self.lum_events[*k].len())
            .unwrap_or(0);
        (self.lum_events[best].clone(), self.fields[best])
    }

    /// The worst field by the strict reading: its transitions (frames).
    pub fn worst_field_strict(&self) -> (Vec<usize>, (usize, usize, usize, usize)) {
        // (the field with the most in any second)
        let most = |e: &[usize]| {
            (0..e.len())
                .map(|k| e[k..].iter().take_while(|&&f| f < e[k] + self.fps).count())
                .max()
                .unwrap_or(0)
        };
        let best = (0..self.fields.len())
            .max_by_key(|k| {
                (
                    most(&self.strict_lum_events[*k]),
                    self.strict_lum_events[*k].len(),
                )
            })
            .unwrap_or(0);
        (self.strict_lum_events[best].clone(), self.fields[best])
    }

    /// As [`Meter::worst_field_strict`], for saturated red.
    pub fn worst_red_field_strict(&self) -> (Vec<usize>, (usize, usize, usize, usize)) {
        let most = |e: &[usize]| {
            (0..e.len())
                .map(|k| e[k..].iter().take_while(|&&f| f < e[k] + self.fps).count())
                .max()
                .unwrap_or(0)
        };
        let best = (0..self.fields.len())
            .max_by_key(|k| {
                (
                    most(&self.strict_red_events[*k]),
                    self.strict_red_events[*k].len(),
                )
            })
            .unwrap_or(0);
        (self.strict_red_events[best].clone(), self.fields[best])
    }

    /// The most transitions in any second over any field (luminance, red).
    pub fn most(&self) -> (usize, usize) {
        let worst = |events: &[Vec<usize>]| {
            events
                .iter()
                .map(|e| {
                    (0..e.len())
                        .map(|k| e[k..].iter().take_while(|&&f| f < e[k] + self.fps).count())
                        .max()
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0)
        };
        (worst(&self.lum_events), worst(&self.red_events))
    }

    /// The same by the strict reading (100 ms at once, every rise).
    pub fn most_strict(&self) -> (usize, usize) {
        let worst = |events: &[Vec<usize>]| {
            events
                .iter()
                .map(|e| {
                    (0..e.len())
                        .map(|k| e[k..].iter().take_while(|&&f| f < e[k] + self.fps).count())
                        .max()
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0)
        };
        (
            worst(&self.strict_lum_events),
            worst(&self.strict_red_events),
        )
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
        // runs of at least 10 turns (5 pairs) with alike periods - a light
        // band and a dark one together, however unlike the two
        let mut start = 0;
        while start + 10 <= turns.len() {
            let mut end = start + 2;
            let first = turns[start + 2] - turns[start];
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

/// Music of a kind, steady, `secs` long (stereo, interleaved, 48 kHz):
/// "pop" (120 BPM: a kick on each beat, a snare on 2 and 4, eighth hats, a
/// pad), "house" (126 BPM, hats on every eighth), "dnb" (174 BPM: kick,
/// snare, sixteenth hats, a reese bass, stabs), "alt3" (bass and treble in
/// turn three times a second at steady loudness), "chirp" (30 Hz - 16 kHz
/// up and down three times a second), "click9" / "click13" (full-scale
/// clicks so many a second). (After an independent review's signals: what
/// changes every 1.2 s does not show what a picture with seconds of history
/// does to a steady rhythm.)
pub fn music(kind: &str, secs: f32) -> Vec<f32> {
    const RATE: f32 = 48_000.0;
    let n = (secs * RATE) as usize;
    let mut seed = 5u32;
    let mut noise = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5
    };
    let tau = std::f32::consts::TAU;
    let mut out = Vec::with_capacity(n * 2);
    let (mut phase, mut last) = (0.0f32, 0.0f32);
    for i in 0..n {
        let t = i as f32 / RATE;
        let v = match kind {
            "pop" | "house" => {
                let bpm = if kind == "pop" { 120.0 } else { 126.0 };
                let beat = t * bpm / 60.0;
                let b = beat.fract() * 60.0 / bpm;
                let mut v =
                    (b * (50.0 + 120.0 * (-b * 35.0).exp()) * tau).sin() * (-b * 14.0).exp() * 0.9;
                if (beat as u32) % 2 == 1 {
                    v += noise() * (-b * 16.0).exp();
                }
                let e8 = (beat * 2.0).fract() * 30.0 / bpm;
                if kind == "house" || (beat * 2.0) as u32 % 2 == 1 {
                    v += noise() * 0.3 * (-e8 * 50.0).exp();
                }
                v += [220.0f32, 277.2, 329.6, 440.0]
                    .iter()
                    .map(|f| (t * f * tau).sin())
                    .sum::<f32>()
                    * 0.07;
                (v + (t * 55.0 * tau).sin() * 0.2) * 0.9
            }
            "dnb" => {
                let beat = t * 174.0 / 60.0;
                let bar = (beat / 4.0).fract() * 4.0;
                let step = (bar * 4.0) as u32;
                let in16 = (bar * 4.0).fract() * 60.0 / 174.0 / 4.0;
                let mut v = 0.0;
                if step == 0 || step == 10 {
                    v += (in16 * (50.0 + 150.0 * (-in16 * 40.0).exp()) * tau).sin()
                        * (-in16 * 25.0).exp();
                }
                if step == 4 || step == 12 {
                    v += noise() * 1.4 * (-in16 * 18.0).exp()
                        + (in16 * 190.0 * tau).sin() * 0.4 * (-in16 * 20.0).exp();
                }
                let hiss = noise();
                v += (hiss - last) * 0.25 * (-in16 * 60.0).exp();
                last = hiss;
                v += ((t * 43.65 * tau).sin() + (t * 44.2 * tau).sin()) * 0.18;
                if step % 4 == 2 {
                    v += [261.6f32, 311.1, 392.0]
                        .iter()
                        .map(|f| (t * f * tau).sin())
                        .sum::<f32>()
                        * 0.15
                        * (-in16 * 10.0).exp();
                }
                v.clamp(-1.0, 1.0) * 0.95
            }
            "alt3" => {
                if (t * 3.0).fract() < 0.5 {
                    (t * 55.0 * tau).sin() * 0.8
                } else {
                    noise() * 0.9 + (t * 7000.0 * tau).sin() * 0.3
                }
            }
            "chirp" => {
                let k = (t * 3.0).fract();
                let k = if k < 0.5 { k * 2.0 } else { 2.0 - k * 2.0 };
                phase += 30.0 * 533.0f32.powf(k) * tau / RATE;
                phase.sin() * 0.95
            }
            // noise gated 8 or 4 times a second (on 40 % of each)
            "gate8" | "gate4" => {
                let hz = if kind == "gate8" { 8.0 } else { 4.0 };
                if (t * hz).fract() < 0.4 {
                    noise() * 1.9
                } else {
                    0.0
                }
            }
            // silence and a full-scale square, five times a second
            "silfull" => {
                if (t * 5.0).fract() < 0.5 {
                    0.0
                } else if (t * 80.0).fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            // bass and treble in turn, eight times a second
            "alt8" => {
                if (t * 8.0).fract() < 0.5 {
                    (t * 55.0 * tau).sin() * 0.8
                } else {
                    noise() * 0.9 + (t * 7000.0 * tau).sin() * 0.3
                }
            }
            "click9" | "click13" => {
                let hz = if kind == "click9" { 9.0 } else { 13.0 };
                let p = (t * hz).fract();
                if p < 0.0008 {
                    1.0
                } else if p < 0.0016 {
                    -1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let v = v.clamp(-1.0, 1.0);
        out.push(v);
        out.push(v);
    }
    out
}

#[cfg(test)]
mod tests {}
