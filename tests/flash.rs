//! Photosensitivity: every face, with its options, in either motion and in
//! several colour schemes, through the worst music for it, measured by
//! `support/flashmeter.rs` (WCAG 2.3.1 / 2.3.2 and ITU-R BT.1702: any field
//! of a third of the picture each way - the picture taken as the whole
//! screen - transiting over a quarter of it; luminance and saturated red;
//! regular stripes). More than six transitions (three flashes) in any
//! second fails, and so does a pattern hazard.
//!
//! The suite runs the faces in Calm with the classic colours at 384 x 216;
//! `--ignored` runs every scheme and both motions at 320 x 180 (the report).

#[path = "support/faces.rs"]
mod faces;
#[path = "support/flashmeter.rs"]
mod flashmeter;

use axross_amp::analyzer::guard::Motion;
use axross_amp::viz::{FACES, Group};
use egui::vec2;
use flashmeter::{FPS, Meter, worst_music};

struct Report {
    transitions: usize,
    red: usize,
    pattern_frames: usize,
    stripes: f32,
}

/// A face (and how its options are set) through the worst music.
fn run(
    key: &'static str,
    motion: Motion,
    scheme: &str,
    size: (f32, f32),
    part: f32,
    setup: impl Fn(&mut axross_amp::viz::VizState),
) -> Report {
    // (AMP_FLASH_PLAIN: the plain test music instead, as long)
    let music = if std::env::var_os("AMP_FLASH_PLAIN").is_some() {
        faces::signal(part * 8.0)
    } else {
        worst_music(part)
    };
    let secs = music.len() as f32 / 2.0 / 48_000.0;
    let mut h = faces::harness(key, vec2(size.0, size.1), true, 1.0);
    h.state_mut().fps = FPS;
    h.state_mut().sound = music;
    h.state_mut().analyzer.motion = motion;
    h.state_mut().scheme = scheme.to_owned();
    // (AMP_FLASH_NO_GUARD: the faces as they draw, without the guard - the
    // "before" of the report)
    h.state_mut().state.flash_off = std::env::var_os("AMP_FLASH_NO_GUARD").is_some();
    setup(&mut h.state_mut().state);
    let frames = ((secs - 0.3) * FPS) as usize;
    let mut meter = Meter::new(size.0 as usize, size.1 as usize);
    // (AMP_FLASH_DUMP=dir: frames 100 - 160 kept as pictures there)
    let dump = std::env::var("AMP_FLASH_DUMP").ok();
    for f in 0..frames {
        faces::frame(&mut h);
        let img = h.render().unwrap();
        if let Some(dir) = dump.as_deref().filter(|_| (100..160).contains(&f)) {
            img.save(format!("{dir}/{key}-{f:03}.png")).unwrap();
        }
        meter.push(&img);
    }
    let (transitions, red) = meter.most();
    if std::env::var_os("AMP_FLASH_WHERE").is_some() {
        let (at, field) = meter.worst_field();
        println!("{key}: field {field:?} transits at frames {at:?}");
    }
    Report {
        transitions,
        red,
        pattern_frames: meter.pattern_frames,
        stripes: meter.stripes,
    }
}

/// Faces with their options: (key, option).
fn cases(group: Option<Group>) -> Vec<(&'static str, &'static str)> {
    let only = std::env::var("AMP_FLASH_ONLY").ok();
    let wanted = |k: &str| only.as_deref().is_none_or(|o| o.split(',').any(|w| w == k));
    let mut out: Vec<(&'static str, &'static str)> = FACES
        .iter()
        .filter(|f| f.group != Group::Off && group.is_none_or(|g| f.group == g) && wanted(f.key))
        .map(|f| (f.key, ""))
        .collect();
    if group.is_none_or(|g| g == Group::Classic) {
        for (k, o) in [
            ("waterfall", "bars"),
            ("starfield", "trails"),
            ("lissajous", "trails"),
        ] {
            if wanted(k) {
                out.push((k, o));
            }
        }
    }
    out
}

fn sweep(
    group: Option<Group>,
    motions: &[Motion],
    schemes: &[&str],
    size: (f32, f32),
    part: f32,
) -> Vec<String> {
    let mut failures = Vec::new();
    for &motion in motions {
        for scheme in schemes {
            for (key, option) in cases(group) {
                let r = run(key, motion, scheme, size, part, |s| match option {
                    "bars" => s.waterfall_hd = false,
                    "trails" => s.classic_plus = true,
                    _ => s.waterfall_hd = true,
                });
                let name = if option.is_empty() {
                    key.to_owned()
                } else {
                    format!("{key} ({option})")
                };
                let line = format!(
                    "{motion:?} {scheme:<8} {name:<20} transitions/s {:>2}  red {:>2}  stripes {:.2} ({} frames)",
                    r.transitions, r.red, r.stripes, r.pattern_frames
                );
                // (as it is measured: a long sweep shows how far it is)
                println!("{line}");
                if r.transitions > 6 || r.red > 6 || r.pattern_frames > 0 {
                    failures.push(line);
                }
            }
        }
    }
    failures
}

#[test]
fn the_meter_counts_flashes_areas_and_patterns_as_the_guidelines_do() {
    let (w, h) = (96usize, 54usize);
    let frame = |f: &dyn Fn(usize, usize) -> u8| {
        image::RgbaImage::from_fn(w as u32, h as u32, |x, y| {
            let v = f(x as usize, y as usize);
            image::Rgba([v, v, v, 255])
        })
    };
    let strobe = |hz: f32, area: &dyn Fn(usize, usize) -> bool| {
        let mut m = Meter::new(w, h);
        for i in 0..120 {
            let on = (((i as f32 / FPS) * hz * 2.0) as u32).is_multiple_of(2);
            m.push(&frame(&|x, y| if on && area(x, y) { 200 } else { 20 }));
        }
        m.most().0
    };
    let all = |_: usize, _: usize| true;
    // the whole picture at 5 Hz: 10 transitions a second; at 2 Hz: 4
    assert_eq!(strobe(5.0, &all), 10);
    assert_eq!(strobe(2.0, &all), 4);
    // a field's quarter is enough (a ninth of the picture: over a quarter
    // of a third-by-third field) ...
    assert!(strobe(5.0, &|x, y| x < w / 3 && y < h / 3) >= 9);
    // ... a sliver is not (2 % of a field)
    assert_eq!(strobe(5.0, &|x, y| x < 2 && y < h / 3), 0);
    // stripes: 12 pairs across the picture are a pattern; a smooth ramp not
    let lum =
        |f: &dyn Fn(usize, usize) -> f32| (0..w * h).map(|i| f(i % w, i / w)).collect::<Vec<f32>>();
    assert!(
        flashmeter::stripes(
            &lum(&|x, _| if (x / 4) % 2 == 0 { 0.05 } else { 0.6 }),
            w,
            h
        ) > 0.8
    );
    assert!(flashmeter::stripes(&lum(&|x, _| x as f32 / w as f32 * 0.7), w, h) < 0.01);
}

#[test]
fn no_classic_face_flashes() {
    let f = sweep(
        Some(Group::Classic),
        &[Motion::Calm],
        &["classic"],
        (384.0, 216.0),
        1.2,
    );
    assert!(f.is_empty(), "{f:#?}");
}

#[test]
fn no_analysis_face_flashes() {
    let f = sweep(
        Some(Group::Analysis),
        &[Motion::Calm],
        &["classic"],
        (384.0, 216.0),
        1.2,
    );
    assert!(f.is_empty(), "{f:#?}");
}

#[test]
fn no_generative_face_flashes() {
    let f = sweep(
        Some(Group::Generative),
        &[Motion::Calm],
        &["classic"],
        (384.0, 216.0),
        1.2,
    );
    assert!(f.is_empty(), "{f:#?}");
}

/// What the whole sweep (below) found, kept in the suite: each at the
/// scheme, motion and size where it failed.
#[test]
fn what_the_sweep_found_stays_fixed() {
    let mut failures = Vec::new();
    for (key, motion, scheme, hd, guard) in [
        // the fine waterfall's rows (37 transitions a second, stripes)
        ("waterfall", Motion::Calm, "neon", true, true),
        // the 3D ring's bars and the particle columns (standing stripes)
        ("ring", Motion::Calm, "neon", false, true),
        ("particles", Motion::Calm, "classic", false, true),
        // the oscilloscope's held wave (moving stripes)
        ("scope", Motion::Intense, "neon", false, true),
        // the plasma, unseen by the guard (11 transitions a second)
        ("plasma", Motion::Intense, "mono", false, true),
        // the ribbon by itself, nine lines moving together in a small
        // picture (9)
        ("ribbon", Motion::Calm, "classic", false, false),
        // the waterfall's bars by themselves, a kick's rows in stripes
        // (28 transitions a second; stripes over 66 %)
        ("waterfall", Motion::Intense, "neon", false, false),
        ("waterfall", Motion::Calm, "mono", false, false),
        // the kaleidoscope by itself, turned at once by each onset (12)
        ("kaleidoscope", Motion::Calm, "classic", false, false),
        // the spirograph by itself, without the guard (18 - 25: its
        // wheel's size followed, every lap of the curve moved)
        ("spirograph", Motion::Calm, "classic", false, false),
    ] {
        let r = run(key, motion, scheme, (320.0, 180.0), 1.2, |s| {
            s.waterfall_hd = hd;
            s.flash_off = !guard;
        });
        let line = format!(
            "{motion:?} {scheme} {key}: transitions/s {}  red {}  stripes {:.2} ({} frames)",
            r.transitions, r.red, r.stripes, r.pattern_frames
        );
        println!("{line}");
        if r.transitions > 6 || r.red > 6 || r.pattern_frames > 0 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every face, both motions, several schemes (the bright and the dark),
/// at 320 x 180: `--ignored --nocapture` (the report).
#[test]
#[ignore]
fn no_face_flashes_in_any_scheme_or_motion() {
    // (AMP_FLASH_SCHEMES=neon,fire and AMP_FLASH_MOTIONS=intense: a part
    // of it, to run the parts side by side)
    let schemes = std::env::var("AMP_FLASH_SCHEMES").unwrap_or("classic,neon,mono,fire".into());
    let schemes: Vec<&str> = schemes.split(',').collect();
    let motions: Vec<Motion> = std::env::var("AMP_FLASH_MOTIONS")
        .unwrap_or("calm,intense".into())
        .split(',')
        .map(|m| match m {
            "intense" => Motion::Intense,
            "reduced" => Motion::Reduced,
            _ => Motion::Calm,
        })
        .collect();
    let f = sweep(None, &motions, &schemes, (320.0, 180.0), 1.2);
    assert!(f.is_empty(), "{f:#?}");
}

/// The guard's estimate of a face against the picture rendered (the mean
/// relative luminance of each cell): (the mean error, the picture's mean,
/// the estimate's mean).
fn estimate_error(key: &'static str, gpu: bool) -> (f32, f32, f32) {
    let mut h = faces::harness(key, vec2(384.0, 216.0), gpu, 2.0);
    h.state_mut().fps = FPS;
    for _ in 0..40 {
        faces::frame(&mut h);
    }
    let img = h.render().unwrap();
    let (est, cols, rows) = h.state().state.flash.estimated();
    let (w, h_) = (img.width() as f32 - 8.0, img.height() as f32 - 8.0);
    let (mut err, mut shown, mut estimated, mut n) = (0.0, 0.0, 0.0, 0.0);
    for cy in 0..rows {
        for cx in 0..cols {
            let (x0, y0) = (
                4.0 + cx as f32 * w / cols as f32,
                4.0 + cy as f32 * h_ / rows as f32,
            );
            let (mut s, mut k) = (0.0, 0);
            for y in y0 as u32..(y0 + h_ / rows as f32).ceil() as u32 {
                for x in x0 as u32..(x0 + w / cols as f32).ceil() as u32 {
                    let p = img.get_pixel(x, y);
                    s += 0.2126 * flashmeter::linear(p[0])
                        + 0.7152 * flashmeter::linear(p[1])
                        + 0.0722 * flashmeter::linear(p[2]);
                    k += 1;
                }
            }
            let s = s / k.max(1) as f32;
            let e = est[cy * cols + cx];
            err += (s - e).abs();
            shown += s;
            estimated += e;
            n += 1.0;
        }
    }
    (err / n, shown / n, estimated / n)
}

/// The guard sees the shader faces where they are painted: over the ground
/// drawn before them (it once laid the ground over them, and saw nothing
/// of the plasma, the fire or the smoke - they could have flashed unveiled).
#[test]
fn the_guard_sees_the_shader_faces() {
    for (key, gpu) in [
        ("plasma", true),
        ("smoke", true),
        ("julia", true),
        ("plasma", false),
    ] {
        let (err, shown, estimated) = estimate_error(key, gpu);
        println!("{key} (gpu {gpu}): error {err:.3}, picture {shown:.3}, estimate {estimated:.3}");
        assert!(
            err < 0.05 && estimated > shown * 0.6,
            "{key} (gpu {gpu}): the estimate is off by {err:.3} (picture {shown:.3}, estimate {estimated:.3})"
        );
    }
}

/// How near the guard's estimate is to the picture (`--ignored`, a probe).
#[test]
#[ignore]
fn the_guards_estimate_follows_the_picture() {
    let key: &'static str = Box::leak(
        std::env::var("AMP_FLASH_PROBE")
            .unwrap_or("ridges".into())
            .into_boxed_str(),
    );
    let mut h = faces::harness(key, vec2(384.0, 216.0), true, 7.0);
    h.state_mut().fps = FPS;
    // (AMP_FLASH_PLAIN: the plain test music - a chord, a beat twice a
    // second - how often the veil comes for it)
    if std::env::var_os("AMP_FLASH_PLAIN").is_none() {
        h.state_mut().sound = worst_music(1.2);
    }
    if std::env::var("AMP_FLASH_MOTIONS").is_ok_and(|m| m == "intense") {
        h.state_mut().analyzer.motion = Motion::Intense;
    }
    if let Ok(scheme) = std::env::var("AMP_FLASH_SCHEMES") {
        h.state_mut().scheme = scheme;
    }
    let mut strength = Vec::new();
    let mut veiled = 0;
    for i in 0..360 {
        faces::frame(&mut h);
        if h.state().state.flash.strength > 0.1 {
            veiled += 1;
        }
        if i % 10 == 0 {
            let (raw, shown) = h.state().state.flash.pressure();
            strength.push(format!(
                "{:.2}/{raw}/{shown}",
                h.state().state.flash.strength
            ));
        }
    }
    println!("veil: {}", strength.join(" "));
    println!("veiled in {veiled} of 360 frames");
    let img = h.render().unwrap();
    let (est, cols, rows) = h.state().state.flash.estimated();
    let (w, h_) = (img.width() as f32 - 8.0, img.height() as f32 - 8.0);
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let mut lines = Vec::new();
    for cy in (0..rows).step_by(3) {
        let mut line = String::new();
        for cx in (0..cols).step_by(3) {
            let (x0, y0) = (
                4.0 + cx as f32 * w / cols as f32,
                4.0 + cy as f32 * h_ / rows as f32,
            );
            let (mut s, mut n) = (0.0, 0);
            for y in y0 as u32..(y0 + h_ / rows as f32) as u32 {
                for x in x0 as u32..(x0 + w / cols as f32) as u32 {
                    let p = img.get_pixel(x, y);
                    s += 0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2]);
                    n += 1;
                }
            }
            line.push_str(&format!(
                "{:.2}/{:.2} ",
                s / n.max(1) as f32,
                est[cy * cols + cx]
            ));
        }
        lines.push(line);
    }
    println!("{key} (shown/estimated)\n{}", lines.join("\n"));
}
