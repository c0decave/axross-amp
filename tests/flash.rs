//! Photosensitivity: no face flashes more than three times in any second,
//! nor flashes red - WCAG 2.3.1 / 2.3.2, measured as ITU-R BT.1702 and
//! the Harding test do. Every face (and its options) is rendered at 60
//! frames a second through the worst music for it - a kick at 180 BPM,
//! bursts of white noise four times a second, silence and full scale in
//! turn - and each frame's relative luminance (sRGB made linear, then
//! Y) is followed in every quarter of the picture and in the whole:
//!
//! - a *transition* is a change of at least 0.1 of the luminance between
//!   a highest and a lowest (or a lowest and a highest) where the darker
//!   is below 0.8; a *general flash* is two opposing transitions; over a
//!   quarter of the picture (25 %, stricter than a quarter of a 10° field
//!   of view on any real screen);
//! - a *red flash* likewise in (R - G - B) x 320 of saturated red pixels
//!   (R / (R + G + B) at least 0.8), a change of at least 20;
//! - high-contrast stripes (a known trigger when they move) are counted
//!   and reported.
//!
//! The test fails on more than 3 general or 3 red flashes in any second.
//! A report of every face is printed (`--nocapture`).

#[path = "support/faces.rs"]
mod faces;

use axross_amp::viz::{FACES, Group};
use egui::vec2;

const RATE: u32 = faces::RATE;
const FPS: f32 = 60.0;

/// The worst music for a picture: 3 s of a kick at 180 BPM over a pad, 3 s
/// of white noise in bursts four times a second, 3 s of silence and full
/// scale a second each.
fn worst_music() -> Vec<f32> {
    let mut seed = 11u32;
    let mut noise = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5
    };
    let n = 9 * RATE as usize;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let tau = std::f32::consts::TAU;
        let v = if t < 3.0 {
            let beat = (t * 3.0).fract();
            let kick = (-beat * 18.0).exp() * (t * 55.0 * tau).sin() * 0.9;
            let pad = (t * 220.0 * tau).sin() * 0.1 + (t * 330.0 * tau).sin() * 0.08;
            kick + pad + noise() * 0.1 * (-beat * 40.0).exp()
        } else if t < 6.0 {
            if (t * 4.0).fract() < 0.5 {
                noise() * 1.6
            } else {
                0.0
            }
        } else if ((t - 6.0) as u32).is_multiple_of(2) {
            0.0
        } else {
            ((t * 110.0 * tau).sin() * 0.4 + (t * 440.0 * tau).sin() * 0.3 + noise() * 0.5)
                .clamp(-1.0, 1.0)
        };
        out.push(v.clamp(-1.0, 1.0));
        out.push((v * 0.95).clamp(-1.0, 1.0));
    }
    out
}

/// sRGB (0..255) to linear light.
fn linear(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// A frame's measures: the relative luminance and the saturated red of
/// each quarter-of-the-picture region (3 x 3 overlapping, the whole last),
/// and its share of high-contrast stripe edges.
fn measure(img: &image::RgbaImage) -> (Vec<f32>, Vec<f32>, f32) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    // 4 x 4 cells, each region 2 x 2 of them
    let mut cell_y = [[0.0f64; 4]; 4];
    let mut cell_r = [[0.0f64; 4]; 4];
    let mut counts = [[0usize; 4]; 4];
    let mut edges = 0usize;
    for y in 0..h {
        let mut last = None::<f32>;
        let mut sign = 0i8;
        for x in 0..w {
            let p = img.get_pixel(x as u32, y as u32);
            let (r, g, b) = (linear(p[0]), linear(p[1]), linear(p[2]));
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let (sr, sg, sb) = (
                p[0] as f32 / 255.0,
                p[1] as f32 / 255.0,
                p[2] as f32 / 255.0,
            );
            let sum = sr + sg + sb;
            let red = if sum > 0.0 && sr / sum >= 0.8 {
                ((sr - sg - sb) * 320.0).max(0.0)
            } else {
                0.0
            };
            let (cx, cy) = ((x * 4 / w).min(3), (y * 4 / h).min(3));
            cell_y[cy][cx] += lum as f64;
            cell_r[cy][cx] += red as f64;
            counts[cy][cx] += 1;
            // stripes: a light-dark-light run of strong steps
            if let Some(l) = last {
                let d = lum - l;
                if d.abs() >= 0.25 {
                    let s = if d > 0.0 { 1 } else { -1 };
                    if s != sign && sign != 0 {
                        edges += 1;
                    }
                    sign = s;
                }
            }
            last = Some(lum);
        }
    }
    let mut ys = Vec::with_capacity(10);
    let mut rs = Vec::with_capacity(10);
    for gy in 0..3 {
        for gx in 0..3 {
            let (mut sy, mut sr, mut n) = (0.0, 0.0, 0usize);
            for cy in gy..gy + 2 {
                for cx in gx..gx + 2 {
                    sy += cell_y[cy][cx];
                    sr += cell_r[cy][cx];
                    n += counts[cy][cx];
                }
            }
            ys.push((sy / n.max(1) as f64) as f32);
            rs.push((sr / n.max(1) as f64) as f32);
        }
    }
    let total: usize = counts.iter().flatten().sum();
    ys.push((cell_y.iter().flatten().sum::<f64>() / total.max(1) as f64) as f32);
    rs.push((cell_r.iter().flatten().sum::<f64>() / total.max(1) as f64) as f32);
    (ys, rs, edges as f32 / (w * h) as f32)
}

/// The frames where a series makes a transition: a change of at least
/// `step` from its last highest to a lowest or back, the darker of the
/// two below `darker` (None: no such condition).
fn transitions(series: &[f32], step: f32, darker: Option<f32>) -> Vec<usize> {
    let mut out = Vec::new();
    let Some(&first) = series.first() else {
        return out;
    };
    let (mut extreme, mut dir) = (first, 0i8);
    let dark = |v: f32| darker.is_none_or(|d| v < d);
    for (i, &v) in series.iter().enumerate() {
        match dir {
            1 if v > extreme => extreme = v,
            -1 if v < extreme => extreme = v,
            1 | 0 if extreme - v >= step => {
                if dark(v) {
                    out.push(i);
                }
                dir = -1;
                extreme = v;
            }
            -1 | 0 if v - extreme >= step => {
                if dark(extreme) {
                    out.push(i);
                }
                dir = 1;
                extreme = v;
            }
            0 => {}
            _ => {}
        }
    }
    out
}

/// The most flashes (pairs of opposing transitions) in any one second.
fn most_a_second(at: &[usize]) -> usize {
    let span = FPS as usize;
    let mut most = 0;
    for (k, &start) in at.iter().enumerate() {
        let n = at[k..].iter().take_while(|&&f| f < start + span).count();
        most = most.max(n / 2);
    }
    most
}

struct Report {
    flashes: usize,
    red: usize,
    stripes: f32,
}

/// A face (key, and how its options are set) through the worst music.
fn run(
    key: &'static str,
    motion: axross_amp::analyzer::guard::Motion,
    setup: impl Fn(&mut axross_amp::viz::VizState),
) -> Report {
    let mut h = faces::harness(key, vec2(160.0, 90.0), true, 9.5);
    h.state_mut().fps = FPS;
    h.state_mut().sound = worst_music();
    h.state_mut().analyzer.motion = motion;
    setup(&mut h.state_mut().state);
    let frames = (9.0 * FPS) as usize;
    let mut ys: Vec<Vec<f32>> = (0..10).map(|_| Vec::with_capacity(frames)).collect();
    let mut rs: Vec<Vec<f32>> = (0..10).map(|_| Vec::with_capacity(frames)).collect();
    let mut stripes = 0.0f32;
    for _ in 0..frames {
        faces::frame(&mut h);
        let img = h.render().unwrap();
        let (y, r, s) = measure(&img);
        for k in 0..10 {
            ys[k].push(y[k]);
            rs[k].push(r[k]);
        }
        stripes = stripes.max(s);
    }
    let flashes = ys
        .iter()
        .map(|s| most_a_second(&transitions(s, 0.1, Some(0.8))))
        .max()
        .unwrap_or(0);
    if std::env::var_os("AMP_FLASH_DEBUG").is_some() {
        for (k, s) in ys.iter().enumerate() {
            let t = transitions(s, 0.1, Some(0.8));
            if most_a_second(&t) > 3 {
                let at: Vec<String> = t
                    .iter()
                    .map(|f| format!("{:.2}", *f as f32 / FPS))
                    .collect();
                println!("{key} region {k}: transitions at {at:?}");
            }
        }
    }
    let red = rs
        .iter()
        .map(|s| most_a_second(&transitions(s, 20.0, None)))
        .max()
        .unwrap_or(0);
    Report {
        flashes,
        red,
        stripes,
    }
}

#[test]
fn the_analyzer_counts_flashes_as_the_guidelines_do() {
    // a full-picture strobe at 5 Hz: 5 flashes a second; at 2 Hz: 2; a
    // slow swell (1 s up, 1 s down): none faster than one
    let strobe = |hz: f32| -> Vec<f32> {
        (0..120)
            .map(|i| {
                if (((i as f32 / FPS) * hz * 2.0) as u32).is_multiple_of(2) {
                    0.05
                } else {
                    0.6
                }
            })
            .collect()
    };
    assert_eq!(most_a_second(&transitions(&strobe(5.0), 0.1, Some(0.8))), 5);
    assert_eq!(most_a_second(&transitions(&strobe(2.0), 0.1, Some(0.8))), 2);
    let swell: Vec<f32> = (0..240)
        .map(|i| 0.3 + 0.3 * (i as f32 / FPS * std::f32::consts::PI).sin())
        .collect();
    assert!(most_a_second(&transitions(&swell, 0.1, Some(0.8))) <= 1);
    // bright flicker above 0.8 (both states light) is no flash
    let light: Vec<f32> = (0..120)
        .map(|i| if i % 4 < 2 { 0.85 } else { 1.0 })
        .collect();
    assert_eq!(most_a_second(&transitions(&light, 0.1, Some(0.8))), 0);
}

/// The faces of `group` (and the options named) through the worst music,
/// in either motion: none flashes more than three times a second.
fn sweep(group: Group, options: &[(&'static str, &str)]) {
    use axross_amp::analyzer::guard::Motion;
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for motion in [Motion::Calm, Motion::Intense] {
        let mut runs: Vec<(String, Report)> = Vec::new();
        // (AMP_FLASH_ONLY="a,b": those faces alone, to look again)
        let only = std::env::var("AMP_FLASH_ONLY").ok();
        let wanted = |k: &str| only.as_deref().is_none_or(|o| o.split(',').any(|w| w == k));
        for face in FACES.iter().filter(|f| f.group == group && wanted(f.key)) {
            runs.push((face.key.to_owned(), run(face.key, motion, |_| {})));
        }
        for (key, option) in options.iter().filter(|(k, _)| wanted(k)) {
            runs.push((
                format!("{key} ({option})"),
                run(key, motion, |s| match *option {
                    "smooth" => s.waterfall_hd = true,
                    _ => s.classic_plus = true,
                }),
            ));
        }
        for (name, r) in &runs {
            let line = format!(
                "{motion:?} {name:<22} flashes/s {:>2}  red/s {:>2}  stripes {:.3}",
                r.flashes, r.red, r.stripes
            );
            if r.flashes > 3 || r.red > 3 {
                failures.push(line.clone());
            }
            lines.push(line);
        }
    }
    println!("{}", lines.join("\n"));
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn no_classic_face_flashes_more_than_three_times_a_second() {
    sweep(
        Group::Classic,
        &[
            ("waterfall", "smooth"),
            ("starfield", "trails"),
            ("lissajous", "trails"),
        ],
    );
}

#[test]
fn no_analysis_face_flashes_more_than_three_times_a_second() {
    sweep(Group::Analysis, &[]);
}

#[test]
fn no_generative_face_flashes_more_than_three_times_a_second() {
    sweep(Group::Generative, &[]);
}
