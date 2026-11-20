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
#[path = "support/m11.rs"]
mod m11;
#[path = "support/music11.rs"]
mod music11;

use axross_amp::analyzer::guard::Motion;
use axross_amp::viz::{FACES, Group};
use egui::vec2;
use flashmeter::{FPS, Meter, worst_music};

impl Report {
    /// Over the limit by either reading, or a pattern hazard.
    fn fails(&self) -> bool {
        self.transitions > 6
            || self.red > 6
            || self.strict > 6
            || self.strict_red > 6
            || self.area > 6
            || self.area_red > 6
            || self.red_gamma > 6
            || self.pattern_frames > 0
    }
}

struct Report {
    transitions: usize,
    red: usize,
    /// By the strict reading (100 ms at once, every rise).
    strict: usize,
    strict_red: usize,
    /// By the area reading, whatever the phase (see the meter).
    area: usize,
    area_red: usize,
    /// Red by the gamma-encoded reading: the most of its normal, strict
    /// and either-way counts.
    red_gamma: usize,
    /// By the swept reading (review 9's 100 ms one: see the meter).
    swept: usize,
    /// By review 11 / 12's own meter (tests/support/m11.rs): its concurrent
    /// area reading, luminance or red, the most.
    review: usize,
    pattern_frames: usize,
    stripes: f32,
    /// The share of frames the guard's veil was on (over 0.3).
    veiled: f32,
    /// The share of frames the guard's pattern stage softened (over a
    /// twentieth of the picture).
    softened: f32,
}

thread_local! {
    /// The Options the faces are measured with (this test's thread; None:
    /// the defaults).
    static OPTIONS: std::cell::RefCell<Option<axross_amp::state::VisualOptions>> =
        const { std::cell::RefCell::new(None) };
}

/// The Options at their extremes: no smoothing, 128 bars, peaks falling
/// at 300 % a second.
fn extreme_options() -> axross_amp::state::VisualOptions {
    axross_amp::state::VisualOptions {
        bars: 128,
        peak_fall: 300,
        smoothing: 0,
        ..Default::default()
    }
}

/// How a face is measured.
struct Play<'a> {
    motion: Motion,
    scheme: &'a str,
    size: (f32, f32),
    fps: f32,
    music: Vec<f32>,
}

/// A face (and how its options are set) through the worst music, at 60
/// frames a second.
#[allow(clippy::too_many_arguments)]
fn run(
    key: &'static str,
    motion: Motion,
    scheme: &str,
    size: (f32, f32),
    part: f32,
    fps: f32,
    setup: impl Fn(&mut axross_amp::viz::VizState),
) -> Report {
    // (AMP_FLASH_PLAIN: the plain test music instead, as long)
    let music = if std::env::var_os("AMP_FLASH_PLAIN").is_some() {
        faces::signal(part * 8.0)
    } else {
        worst_music(part)
    };
    measure(
        key,
        Play {
            motion,
            scheme,
            size,
            fps,
            music,
        },
        setup,
    )
}

/// A face played so, measured.
fn measure(
    key: &'static str,
    play: Play<'_>,
    setup: impl Fn(&mut axross_amp::viz::VizState),
) -> Report {
    let Play {
        motion,
        scheme,
        size,
        fps,
        music,
    } = play;
    let secs = music.len() as f32 / 2.0 / 48_000.0;
    let mut h = faces::harness(key, vec2(size.0, size.1), true, 1.0);
    h.state_mut().fps = fps;
    h.state_mut().sound = music;
    h.state_mut().analyzer.motion = motion;
    // (the Options as a test sets them, on its own thread)
    if let Some(o) = OPTIONS.with(|o| o.borrow().clone()) {
        h.state_mut().options = o;
    }
    // (the extreme schemes: white on black, pure red on black)
    let (key_scheme, custom): (&str, &[(&str, &str)]) = match scheme {
        "bw" => (
            "custom",
            &[
                ("background", "#000000"),
                ("low", "#ffffff"),
                ("high", "#ffffff"),
                ("peak", "#ffffff"),
            ],
        ),
        "redcustom" => (
            "custom",
            &[
                ("background", "#000000"),
                ("low", "#ff0000"),
                ("high", "#ff0000"),
                ("peak", "#ff3030"),
            ],
        ),
        // (the other way: black on white, black on a pure red ground; the
        // colour editor's own defaults)
        "wb" => (
            "custom",
            &[
                ("background", "#ffffff"),
                ("low", "#000000"),
                ("high", "#000000"),
                ("peak", "#000000"),
            ],
        ),
        "redbg" => (
            "custom",
            &[
                ("background", "#ff0000"),
                ("low", "#000000"),
                ("high", "#000000"),
                ("peak", "#000000"),
            ],
        ),
        "defcustom" => ("custom", &[]),
        other => (other, &[]),
    };
    h.state_mut().scheme = key_scheme.to_owned();
    for (k, v) in custom {
        h.state_mut().custom.insert((*k).into(), (*v).into());
    }
    // (AMP_FLASH_NO_GUARD: the faces as they draw, without the guard - the
    // "before" of the report)
    h.state_mut().state.flash_off = std::env::var_os("AMP_FLASH_NO_GUARD").is_some();
    // (AMP_FLASH_MONITOR: the guard's fields a third of a 1920 x 1080
    // screen, as in a window, not of the picture)
    if std::env::var_os("AMP_FLASH_MONITOR").is_some() {
        h.state_mut().state.flash.screen_is = Some(vec2(1920.0, 1080.0));
    }
    setup(&mut h.state_mut().state);
    let frames = ((secs - 0.3) * fps) as usize;
    let mut meter = Meter::with_fps(size.0 as usize, size.1 as usize, fps.round() as usize);
    let mut gamma = Meter::gamma_red(size.0 as usize, size.1 as usize, fps.round() as usize);
    let mut review = m11::M11::new(size.0 as usize, size.1 as usize, fps);
    // (AMP_FLASH_DUMP=dir: frames 100 - 160 kept as pictures there)
    let dump = std::env::var("AMP_FLASH_DUMP").ok();
    let mut veiled = 0usize;
    let mut softened = 0usize;
    for f in 0..frames {
        faces::frame(&mut h);
        let img = h.render().unwrap();
        let dump_from: usize = std::env::var("AMP_FLASH_DUMP_AT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        if let Some(dir) = dump
            .as_deref()
            .filter(|_| (dump_from..dump_from + 60).contains(&f))
        {
            img.save(format!("{dir}/{key}-{f:03}.png")).unwrap();
            // (and the guard's estimate of it, a cell a pixel)
            let (est, cols, rows) = h.state().state.flash.estimated();
            if cols * rows > 0 {
                let grey = image::GrayImage::from_fn(cols as u32, rows as u32, |x, y| {
                    let v = est[y as usize * cols + x as usize].clamp(0.0, 1.0);
                    image::Luma([(v.powf(1.0 / 2.2) * 255.0) as u8])
                });
                grey.save(format!("{dir}/{key}-{f:03}-guard.png")).unwrap();
            }
        }
        meter.push(&img);
        gamma.push(&img);
        review.push_rgba(&img);
        if h.state().state.flash.strength > 0.3 {
            veiled += 1;
        }
        if h.state().state.flash.pattern_cover() > 0.05 {
            softened += 1;
        }
        if std::env::var_os("AMP_FLASH_TRACE").is_some() {
            let (raw, shown) = h.state().state.flash.pressure();
            println!(
                "frame {f}: guard {raw}/{shown} veil {:.2} patterns {:.2} over {:.2}",
                h.state().state.flash.strength,
                h.state().state.flash.pattern_strength,
                h.state().state.flash.pattern_cover()
            );
        }
    }
    let (transitions, red) = meter.most();
    let (strict, strict_red) = meter.most_strict();
    if std::env::var_os("AMP_FLASH_WHERE").is_some() {
        let (at, field) = meter.worst_field();
        println!("{key}: field {field:?} transits at frames {at:?}");
        println!("{key}: patterns at frames {:?}", meter.hazard_at);
        let (at, field) = meter.worst_field_strict();
        println!("{key}: strictly, field {field:?} transits at frames {at:?}");
        println!("{key}: by area over six at frames {:?}", meter.area_at);
        let (at, field) = meter.worst_field_swept();
        println!("{key}: swept, field {field:?} at frames {at:?}");
        let (at, field) = meter.worst_red_field_strict();
        println!("{key}: red strictly, field {field:?} transits at frames {at:?}");
    }
    Report {
        transitions,
        red,
        strict,
        strict_red,
        area: meter.area,
        area_red: meter.area_red,
        red_gamma: gamma
            .most()
            .1
            .max(gamma.most_strict().1)
            .max(gamma.area_red),
        swept: meter.most_swept(),
        review: {
            let r = review.reading();
            r.c[0].max(r.c[1])
        },
        pattern_frames: meter.pattern_frames,
        stripes: meter.stripes,
        veiled: veiled as f32 / frames.max(1) as f32,
        softened: softened as f32 / frames.max(1) as f32,
    }
}

/// A probe (`--ignored`): AMP_PROBE_FACES=a,b AMP_PROBE_MUSIC=pop,alt3,worst
/// AMP_PROBE_SECS=10 AMP_PROBE_FPS=60 AMP_PROBE_SIZE=480x270
/// AMP_PROBE_SCHEME=classic AMP_PROBE_MOTION=calm AMP_PROBE_HD=0|1
/// AMP_PROBE_PLUS=0|1 (with AMP_FLASH_NO_GUARD, AMP_FLASH_MONITOR).
#[test]
#[ignore]
fn probe() {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or(d.into());
    let secs: f32 = env("AMP_PROBE_SECS", "10").parse().unwrap();
    let fps: f32 = env("AMP_PROBE_FPS", "60").parse().unwrap();
    let size = env("AMP_PROBE_SIZE", "480x270");
    let (w, h) = size.split_once('x').unwrap();
    let size = (w.parse().unwrap(), h.parse().unwrap());
    let scheme = env("AMP_PROBE_SCHEME", "classic");
    let motion = if env("AMP_PROBE_MOTION", "calm") == "intense" {
        Motion::Intense
    } else {
        Motion::Calm
    };
    let hd = env("AMP_PROBE_HD", "1") == "1";
    let plus = env("AMP_PROBE_PLUS", "0") == "1";
    for key in env("AMP_PROBE_FACES", "spectrum").split(',') {
        let key: &'static str = FACES.iter().find(|f| f.key == key).unwrap().key;
        for music in env("AMP_PROBE_MUSIC", "pop").split(',') {
            let sound = if music == "worst" {
                worst_music(secs / 8.0)
            } else {
                flashmeter::music(music, secs)
            };
            let r = measure(
                key,
                Play {
                    motion,
                    scheme: &scheme,
                    size,
                    fps,
                    music: sound,
                },
                |s| {
                    s.waterfall_hd = hd;
                    s.classic_plus = plus;
                },
            );
            println!(
                "{key:<13} {music:<7} {motion:?} {scheme} {}x{}@{fps}: transitions/s {:>2} ({:>2})  red {:>2} ({:>2})  stripes {:.2} ({} frames)  area {} (red {})  gamma red {}  veiled {:.0} %",
                size.0,
                size.1,
                r.transitions,
                r.strict,
                r.red,
                r.strict_red,
                r.stripes,
                r.pattern_frames,
                r.area,
                r.area_red,
                r.red_gamma,
                r.veiled * 100.0
            );
        }
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
    fps: f32,
) -> Vec<String> {
    let mut failures = Vec::new();
    for &motion in motions {
        for scheme in schemes {
            for (key, option) in cases(group) {
                let r = run(key, motion, scheme, size, part, fps, |s| match option {
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
                    "{motion:?} {scheme:<8} {name:<20} transitions/s {:>2} ({:>2})  red {:>2} ({:>2})  stripes {:.2} ({} frames)  area {} (red {})  gamma red {}",
                    r.transitions,
                    r.strict,
                    r.red,
                    r.strict_red,
                    r.stripes,
                    r.pattern_frames,
                    r.area,
                    r.area_red,
                    r.red_gamma
                );
                // (as it is measured: a long sweep shows how far it is)
                println!("{line}");
                if r.fails() {
                    failures.push(line);
                }
            }
        }
    }
    failures
}

/// A checker of 5 x 5 tiles flickering grey 200 <-> 20 at `hz`, its tiles
/// in phase or each the other way from its neighbours: the most
/// transitions a second by the normal, the strict and the area reading.
fn checker(hz: f32, antiphase: bool) -> (usize, usize, usize) {
    let (w, h) = (300usize, 180usize);
    let mut meter = Meter::new(w, h);
    for f in 0..120usize {
        let on = (((f as f32 / 60.0) * hz * 2.0) as usize).is_multiple_of(2);
        let img = image::RgbaImage::from_fn(w as u32, h as u32, |x, y| {
            let tile = (x as usize * 5 / w) + (y as usize * 5 / h);
            let lit = if antiphase && tile % 2 == 1 { !on } else { on };
            let v = if lit { 200 } else { 20 };
            image::Rgba([v, v, v, 255])
        });
        meter.push(&img);
    }
    let (normal, _) = meter.most();
    let (strict, _) = meter.most_strict();
    (normal, strict, meter.area)
}

/// Flicker in antiphase is flicker: WCAG counts the area of the flashes
/// at once, whatever their phase. The readings that count a quarter of a
/// field changing the same way saw 2 of a checker in antiphase at 15 Hz;
/// the area reading sees it as it sees the checker in phase.
#[test]
fn flicker_in_antiphase_is_counted() {
    for hz in [5.0, 8.0, 15.0] {
        for antiphase in [false, true] {
            let (normal, strict, area) = checker(hz, antiphase);
            println!(
                "{hz} Hz, antiphase {antiphase}: normal {normal}, strict {strict}, area {area}"
            );
            assert!(area >= 9, "{hz} Hz, antiphase {antiphase}: area {area}");
        }
    }
    // thin lines moving over white - motion, not flicker: by area, no more
    // than a pixel's worth of the lines' edges each frame
    let (w, h) = (300usize, 180usize);
    let mut meter = Meter::new(w, h);
    for f in 0..120usize {
        let img = image::RgbaImage::from_fn(w as u32, h as u32, |x, _| {
            let v = if (x as usize + 2 * f) % 14 < 1 {
                0
            } else {
                255
            };
            image::Rgba([v, v, v, 255])
        });
        meter.push(&img);
    }
    assert!(
        meter.area <= 2,
        "moving lines read as flicker: {}",
        meter.area
    );
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
    // uneven gratings are gratings: 3 on and 9 off, 2 / 10, 10 / 3, 12 / 2
    // (half-periods compared with the first gap once scored 0.00)
    for (on, off) in [(3, 9), (2, 10), (10, 3), (12, 2)] {
        let wide = (0..w * 4 * h)
            .map(|i| {
                if (i % (w * 4)) % (on + off) < on {
                    0.6
                } else {
                    0.05
                }
            })
            .collect::<Vec<f32>>();
        let across = flashmeter::stripes(&wide, w * 4, h);
        assert!(across > 0.8, "{on} on, {off} off: {across:.2}");
    }
    // a grating drifting slowly (a pixel every third frame) moves
    let mut m = Meter::new(w, h);
    for i in 0..60 {
        let shift = i / 3;
        m.push(&frame(&|x, _| {
            if ((x + shift) / 4) % 2 == 0 { 200 } else { 20 }
        }));
    }
    // (stripes over the whole picture: a hazard standing or moving; over a
    // third of it, only moving)
    let mut third = Meter::new(w, h);
    for i in 0..60 {
        let shift = i / 3;
        third.push(&frame(&|x, y| {
            if y < h / 3 && ((x + shift) / 4) % 2 == 0 {
                200
            } else {
                20
            }
        }));
    }
    assert!(
        m.pattern_frames > 50 && third.pattern_frames > 40,
        "{}",
        third.pattern_frames
    );
    // an orange red (#ff4000) flashing against black five times a second is
    // a red flash (linear R, G and B: ten transitions a second)
    let mut red = Meter::new(w, h);
    for i in 0..120 {
        let on = (((i as f32 / FPS) * 10.0) as u32).is_multiple_of(2);
        red.push(&image::RgbaImage::from_fn(w as u32, h as u32, |_, _| {
            if on {
                image::Rgba([255, 0x40, 0, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            }
        }));
    }
    assert_eq!(red.most().1, 10);
    // at 30 frames a second, the same strobes count the same
    let mut slow = Meter::with_fps(w, h, 30);
    for i in 0..60 {
        let on = (((i as f32 / 30.0) * 10.0) as u32).is_multiple_of(2);
        slow.push(&frame(&|_, _| if on { 200 } else { 20 }));
    }
    assert_eq!(slow.most().0, 10);
    // the strict reading: a rise spread over frames (a fortieth of the
    // picture's width each frame) reaches a quarter of a field within
    // 100 ms, not within 34 ms; two rises one after the other (the left,
    // then the right) then all dark is three transitions strictly, two as
    // pairs
    let mut spread = Meter::new(w, h);
    for i in 0..120 {
        let k = i % 60;
        spread.push(&frame(&|x, _| {
            let lit = k < 45 && x * 40 < k * w;
            if lit { 200 } else { 20 }
        }));
    }
    assert!(
        spread.most_strict().0 > spread.most().0,
        "{:?} {:?}",
        spread.most(),
        spread.most_strict()
    );
    let mut steps = Meter::new(w, h);
    for i in 0..40 {
        steps.push(&frame(&|x, _| match i / 10 {
            1 if x < w / 2 => 200,
            2 => 200,
            _ => 20,
        }));
    }
    assert_eq!((steps.most().0, steps.most_strict().0), (2, 3));
}

#[test]
fn no_classic_face_flashes() {
    let f = sweep(
        Some(Group::Classic),
        &[Motion::Calm],
        &["classic"],
        (384.0, 216.0),
        1.2,
        FPS,
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
        FPS,
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
        FPS,
    );
    assert!(f.is_empty(), "{f:#?}");
}

/// Every face as the player draws it - 30 frames a second - and as a
/// software renderer may - 12 (the meter counts a second as so many
/// frames, "at once" as 34 ms).
#[test]
fn no_face_flashes_at_30_or_12_frames_a_second() {
    let mut f = Vec::new();
    for fps in [30.0, 12.0] {
        f.extend(sweep(
            None,
            &[Motion::Calm],
            &["classic"],
            (384.0, 216.0),
            1.2,
            fps,
        ));
    }
    assert!(f.is_empty(), "{f:#?}");
}

/// The faces with seconds of history (spectrograms, the waterfall, the
/// chromagram, the piano roll, the overview) through steady music, ten
/// seconds of each: a rhythm held for longer than they remember (the worst
/// music's parts, 1.2 s, never showed the mel spectrogram's grating).
#[test]
fn faces_with_history_stay_calm_through_steady_music() {
    let mut failures = Vec::new();
    for (key, hd) in [
        ("mel", true),
        ("waterfall", true),
        ("waterfall", false),
        ("chroma", true),
        ("pianoroll", true),
        ("overview", true),
    ] {
        for music in ["alt3", "chirp", "click9"] {
            let r = measure(
                key,
                Play {
                    motion: Motion::Calm,
                    scheme: "classic",
                    size: (384.0, 216.0),
                    fps: FPS,
                    music: flashmeter::music(music, 10.0),
                },
                |s| s.waterfall_hd = hd,
            );
            let line = format!(
                "{key} (hd {hd}) {music}: transitions/s {} ({})  red {} ({})  stripes {:.2} ({} frames)  area {} (red {})  gamma red {}",
                r.transitions,
                r.strict,
                r.red,
                r.strict_red,
                r.stripes,
                r.pattern_frames,
                r.area,
                r.area_red,
                r.red_gamma
            );
            println!("{line}");
            if r.fails() {
                failures.push(line);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The guard leaves alone what does not flash: the default faces through
/// pop, house and drum and bass (once it veiled the spectrum 40 - 85 % of
/// the time there, pumping on and off, the meter counting 2 - 3).
#[test]
fn the_veil_stays_off_where_nothing_flashes() {
    let mut failures = Vec::new();
    for key in ["spectrum", "line", "mirror", "led"] {
        for music in ["pop", "house", "dnb"] {
            let r = measure(
                key,
                Play {
                    motion: Motion::Calm,
                    scheme: "classic",
                    size: (384.0, 216.0),
                    fps: FPS,
                    music: flashmeter::music(music, 6.0),
                },
                |_| {},
            );
            let line = format!(
                "{key} {music}: transitions/s {} ({})  veiled {:.0} %",
                r.transitions,
                r.strict,
                r.veiled * 100.0
            );
            println!("{line}");
            // KNOWN EXCEPTION (line / Classic / Calm / drum and bass): the
            // guard over-estimates the line's moving fill - its own count
            // comes to its budget of 7 where the meter reads 4 strictly - and
            // veils it about half the time (48 %; so before review 9's line
            // change too, when the strict reading was 5). Safe - dimmed, no
            // flash let through - but darker than needed. Held where it is:
            // the veil at most 55 %, the strict reading at most 4.
            // TODO: the guard's estimate of a moving gradient fill.
            if (key, music) == ("line", "dnb") {
                if r.strict > 4 || r.veiled > 0.55 {
                    failures.push(format!("{line} (known exception drifted)"));
                }
                continue;
            }
            // (off wherever the strict reading counts four or fewer)
            if r.strict <= 4 && r.veiled > 0.05 {
                failures.push(line);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// How much a face moves: the mean change of a pixel's luminance from one
/// frame to the next, through the worst music, as if `played` seconds had
/// played already.
fn motion(key: &'static str, played: f64) -> f32 {
    let mut h = faces::harness(key, vec2(320.0, 180.0), true, 1.0);
    h.state_mut().fps = FPS;
    h.state_mut().sound = worst_music(0.6);
    h.state_mut().state.flash_off = true;
    {
        let a = &mut h.state_mut().analyzer;
        a.time = played;
        a.energy_flow = played * 0.4;
        a.bass_flow = played * 0.3;
    }
    let (mut before, mut sum, mut n) = (None::<Vec<f32>>, 0.0f32, 0usize);
    for _ in 0..240 {
        faces::frame(&mut h);
        let img = h.render().unwrap();
        let y: Vec<f32> = img
            .pixels()
            .map(|p| {
                0.2126 * flashmeter::linear(p[0])
                    + 0.7152 * flashmeter::linear(p[1])
                    + 0.0722 * flashmeter::linear(p[2])
            })
            .collect();
        if let Some(b) = &before {
            sum += y.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f32>() / y.len() as f32;
            n += 1;
        }
        before = Some(y);
    }
    sum / n.max(1) as f32
}

/// The faces that move faster as the music gets louder move no faster
/// after 25 minutes of it: their clock times the energy moved on faster
/// the longer they had played (a flyover 25 minutes in flew a thousand
/// times as fast at a loud passage - a picture of noise; the helix spun
/// as fast) - now they move on by the energy's own clock.
#[test]
fn faces_moved_by_the_energy_move_alike_after_long_play() {
    let mut failures = Vec::new();
    for key in ["plasma", "fire", "tunnel", "flyover", "helix"] {
        let (fresh, long) = (motion(key, 0.0), motion(key, 1500.0));
        let line = format!("{key}: {fresh:.4} a frame at the start, {long:.4} after 25 minutes");
        println!("{line}");
        if long > fresh * 2.0 + 0.002 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The Options at their extremes - no smoothing, 128 bars, peaks falling
/// at 300 % a second - in Classic: the faces stay within both readings (the
/// pictures take at least 75 % smoothing and at most 100 %/s). Before: the
/// LED equalizer 11 by the strict reading in Calm, the aurora and the
/// isometric bars 8; with no smoothing alone, the isometric bars 9 and the
/// mirror 7 in Intense.
#[test]
fn the_options_at_their_extremes_keep_the_faces_calm() {
    OPTIONS.with(|o| *o.borrow_mut() = Some(extreme_options()));
    let mut failures = Vec::new();
    for (key, motion) in [
        ("led", Motion::Calm),
        ("aurora", Motion::Calm),
        ("isometric", Motion::Calm),
        ("bells", Motion::Calm),
        ("spectrum", Motion::Intense),
        ("mirror", Motion::Intense),
        ("isometric", Motion::Intense),
    ] {
        let r = run(key, motion, "classic", (320.0, 180.0), 1.2, FPS, |_| {});
        let line = format!(
            "{motion:?} {key}: transitions/s {} ({})  red {} ({})  area {} ({})  stripes {:.2} ({} frames)  gamma red {}",
            r.transitions,
            r.strict,
            r.red,
            r.strict_red,
            r.area,
            r.area_red,
            r.stripes,
            r.pattern_frames,
            r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    // (the sweep at the extremes found these over six by the strict
    // reading: the terrain's fine mesh 8 to 11, the beat rings' spokes 7,
    // the starfield's trails 7)
    for (key, trails, motion, scheme) in [
        ("terrain", false, Motion::Intense, "bw"),
        ("terrain", false, Motion::Calm, "classic"),
        ("beatrings", false, Motion::Intense, "classic"),
        ("beatrings", false, Motion::Intense, "bw"),
        ("starfield", true, Motion::Intense, "wb"),
        ("starfield", true, Motion::Intense, "neon"),
    ] {
        let r = run(key, motion, scheme, (320.0, 180.0), 1.2, FPS, |s| {
            s.classic_plus = trails
        });
        let line = format!(
            "{motion:?} {scheme} {key}: transitions/s {} ({})  red {} ({})  area {} ({})  stripes {:.2} ({} frames)  gamma red {}",
            r.transitions,
            r.strict,
            r.red,
            r.strict_red,
            r.area,
            r.area_red,
            r.stripes,
            r.pattern_frames,
            r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The pattern stage softens only what is striped: where the meter finds
/// no stripes, the faces are left as they are. (The particles' columns of
/// dots, laid on the guard's cells unevenly, read as a grating there: the
/// whole face was blurred, its columns' coming and going a flash.)
#[test]
fn the_pattern_stage_leaves_what_is_not_striped() {
    let mut failures = Vec::new();
    for (key, scheme) in [
        ("particles", "bw"),
        ("particles", "classic"),
        ("led", "bw"),
        ("spectrum", "classic"),
    ] {
        let r = run(key, Motion::Calm, scheme, (320.0, 180.0), 0.6, FPS, |_| {});
        let line = format!(
            "{key} {scheme}: stripes {:.2}, softened {:.0} % of the frames",
            r.stripes,
            r.softened * 100.0
        );
        println!("{line}");
        if r.stripes < 0.25 && r.softened > 0.05 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
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
        // 0.13.0, by the strict reading: the tunnel's near rings sweeping
        // a third of the screen (7), the flow field's white hairs in rows
        // (stripes), the typography's word a beat (8), the Julia set
        // zoomed and turned by the beat (7), the terrain's dots (7), the
        // particles blurred by a grating the guard saw that was not there
        // (7)
        ("tunnel", Motion::Calm, "bw", false, true),
        ("tunnel", Motion::Calm, "fire", false, true),
        ("flowfield", Motion::Calm, "bw", false, true),
        ("typography", Motion::Calm, "fire", false, true),
        ("julia", Motion::Calm, "bw", false, true),
        ("terrain", Motion::Intense, "classic", false, true),
        ("particles", Motion::Calm, "bw", false, true),
        // review 7: custom schemes the other way round - black on white,
        // black on a pure red ground - and the editor's own defaults (the
        // LED 8, the scope red 8, the piano roll's octaves a grating in 439
        // frames, the moiré in 371, the helix in 72 and 22)
        ("led", Motion::Calm, "wb", false, true),
        ("particles", Motion::Calm, "wb", false, true),
        ("helix", Motion::Calm, "wb", false, true),
        ("terrain", Motion::Calm, "wb", false, true),
        ("pianoroll", Motion::Calm, "wb", false, true),
        ("ridges", Motion::Calm, "wb", false, true),
        ("scope", Motion::Calm, "redbg", false, true),
        ("terrain", Motion::Calm, "redbg", false, true),
        ("morph", Motion::Calm, "redbg", false, true),
        ("moire", Motion::Calm, "redbg", false, true),
        ("helix", Motion::Calm, "defcustom", false, true),
        // the moiré at its most: Intense, white on black
        ("moire", Motion::Intense, "bw", false, true),
        // the kaleidoscope by itself, turned at once by each onset (12)
        ("kaleidoscope", Motion::Calm, "classic", false, false),
        // the spirograph by itself, without the guard (18 - 25: its
        // wheel's size followed, every lap of the curve moved)
        ("spirograph", Motion::Calm, "classic", false, false),
    ] {
        let r = run(key, motion, scheme, (320.0, 180.0), 1.2, FPS, |s| {
            s.waterfall_hd = hd;
            s.flash_off = !guard;
        });
        let line = format!(
            "{motion:?} {scheme} {key}: transitions/s {} ({})  red {} ({})  stripes {:.2} ({} frames)  area {} (red {})  gamma red {}",
            r.transitions,
            r.strict,
            r.red,
            r.strict_red,
            r.stripes,
            r.pattern_frames,
            r.area,
            r.area_red,
            r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The feedback face's every look, at its loudest (Intense) in the starkest
/// schemes: within both readings. (A look is shown until the next phrase,
/// 16 beats: the most of the music in it.)
#[test]
fn every_feedback_look_stays_calm() {
    let mut failures = Vec::new();
    for k in 0..axross_amp::viz::FEEDBACK_LOOKS {
        for scheme in ["bw", "redcustom"] {
            let r = run(
                "milkdrop",
                Motion::Intense,
                scheme,
                (320.0, 180.0),
                0.6,
                FPS,
                |s| axross_amp::viz::set_feedback_preset(s, k),
            );
            let line = format!(
                "look {k} Intense {scheme}: transitions/s {} ({})  red {} ({})  stripes {:.2} ({} frames)  area {} (red {})  gamma red {}",
                r.transitions,
                r.strict,
                r.red,
                r.strict_red,
                r.stripes,
                r.pattern_frames,
                r.area,
                r.area_red,
                r.red_gamma
            );
            println!("{line}");
            if r.fails() {
                failures.push(line);
            }
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
    let schemes = std::env::var("AMP_FLASH_SCHEMES")
        .unwrap_or("classic,neon,mono,fire,bw,redcustom,wb,redbg,defcustom".into());
    // (AMP_FLASH_OPTIONS=extreme: with the Options at their extremes)
    if std::env::var("AMP_FLASH_OPTIONS").as_deref() == Ok("extreme") {
        OPTIONS.with(|o| *o.borrow_mut() = Some(extreme_options()));
    }
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
    // (AMP_FLASH_SIZE=1920x1080, AMP_FLASH_FPS=30: elsewhere)
    let size = std::env::var("AMP_FLASH_SIZE").unwrap_or("320x180".into());
    let (w, h) = size.split_once('x').unwrap();
    let fps = std::env::var("AMP_FLASH_FPS").map_or(FPS, |v| v.parse().unwrap());
    let f = sweep(
        None,
        &motions,
        &schemes,
        (w.parse().unwrap(), h.parse().unwrap()),
        1.2,
        fps,
    );
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
        // (and the cover: its pixels as they are, not a flat grey)
        ("cover", true),
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
    if let Ok(kind) = std::env::var("AMP_FLASH_MUSIC") {
        h.state_mut().sound = flashmeter::music(&kind, 7.0);
    } else if std::env::var_os("AMP_FLASH_PLAIN").is_none() {
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

/// Red by the gamma-encoded reading too: a pure red ground darkened to
/// #330000 was under 20 in linear light but 64 gamma-encoded - the scope's
/// dense trace over it swung a field's red 11 times a second, and the red
/// scheme's trace 10 (a review's numbers).
#[test]
fn red_by_the_gamma_encoded_reading_too() {
    let mut failures = Vec::new();
    for (key, motion, scheme) in [
        ("scope", Motion::Calm, "redbg"),
        ("scope", Motion::Intense, "redcustom"),
        ("terrain", Motion::Calm, "redbg"),
        ("morph", Motion::Calm, "redbg"),
    ] {
        let r = run(key, motion, scheme, (320.0, 180.0), 1.2, FPS, |_| {});
        let line = format!(
            "{motion:?} {scheme} {key}: red {} ({}) area red {} gamma red {}",
            r.red, r.strict_red, r.area_red, r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// No red flash at all, in any scheme: Classic's and Fire's own reds
/// (#e74c3c, #8b1a00, #ff6a00) were saturated red - an independent review
/// counted red transitions in 59 of 519 cases, the isometric view in Fire
/// at 12 frames a second 8. Every face's colours are no saturated red by
/// either encoding now, whatever the scheme: not one red transition.
#[test]
fn no_red_flash_in_any_scheme() {
    let mut failures = Vec::new();
    for (key, motion, scheme, fps) in [
        ("line", Motion::Intense, "fire", FPS),
        ("bells", Motion::Intense, "fire", FPS),
        ("spectrum", Motion::Intense, "fire", 12.0),
        ("line", Motion::Intense, "fire", 12.0),
        ("polar", Motion::Intense, "fire", 12.0),
        ("isometric", Motion::Intense, "fire", 12.0),
        ("spectrum", Motion::Intense, "classic", FPS),
        ("isometric", Motion::Calm, "classic", 12.0),
    ] {
        let r = run(key, motion, scheme, (320.0, 180.0), 1.2, fps, |_| {});
        let line = format!(
            "{motion:?} {scheme} {key} at {fps}: red {} ({}) area red {} gamma red {}",
            r.red, r.strict_red, r.area_red, r.red_gamma
        );
        println!("{line}");
        if r.red + r.strict_red + r.area_red + r.red_gamma > 0 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// At a slow renderer's 12 frames a second, in the schemes the sweep at 12
/// found: the line in the pure red custom scheme (pale since it is no red:
/// 7 by the either-way reading), the flow field's standing stripes (40 - 42 %
/// in a frame).
/// The line in Fire, Intense, read 7 to 10 by review 9's 100 ms reading
/// (its curve sweeping over a field quickly); with Classic's colours 9 and
/// 10 (as since review 9's lighter colours took the reds out).
#[test]
fn the_line_sweeps_no_field_too_often() {
    let mut failures = Vec::new();
    for (motion, scheme) in [
        (Motion::Intense, "fire"),
        (Motion::Calm, "fire"),
        (Motion::Intense, "classic"),
        (Motion::Calm, "classic"),
    ] {
        let r = run("line", motion, scheme, (320.0, 180.0), 1.2, 60.0, |_| {});
        let line = format!("{motion:?} {scheme} line: swept {}", r.swept);
        println!("{line}");
        if r.swept > 6 || r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The wave ribbon in black on white at 960 x 540, Intense: its strands
/// twisting with each onset were a grating oscillating across the picture
/// (review 10: 40 transitions a second in a third of the screen, 50 ms at
/// once; the swept reading here 10 and more). In the starkest schemes its
/// twist and its height follow slower.
#[test]
fn the_ribbon_in_black_on_white_is_no_grating() {
    let mut failures = Vec::new();
    for scheme in ["wb", "bw"] {
        let r = run(
            "ribbon",
            Motion::Intense,
            scheme,
            (960.0, 540.0),
            0.6,
            60.0,
            |_| {},
        );
        let line = format!(
            "ribbon {scheme} 960 x 540: swept {} strict {} area {} stripes {:.2}",
            r.swept, r.strict, r.area, r.stripes
        );
        println!("{line}");
        if r.swept > 6 || r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The stereo scope through stereo that differs left and right (noise
/// unlike in each channel, a chord in antiphase, 30 ms bursts): it drew a
/// new tangle every frame - review 11: 33 transitions a second in Classic,
/// Calm, the project's meter 21 (strict 31). At 60 frames a second, every
/// reading within the limit.
#[test]
fn the_stereo_scope_is_calm_through_uncorrelated_stereo() {
    let mut failures = Vec::new();
    for (motion, scheme) in [
        (Motion::Calm, "classic"),
        (Motion::Intense, "classic"),
        (Motion::Intense, "fire"),
        (Motion::Intense, "bw"),
        (Motion::Intense, "wb"),
    ] {
        let r = run(
            "lissajous",
            motion,
            scheme,
            (320.0, 180.0),
            0.6,
            60.0,
            |_| {},
        );
        let line = format!(
            "lissajous {motion:?} {scheme}: transitions/s {} (strict {}) area {} swept {} red {}",
            r.transitions,
            r.strict,
            r.area,
            r.swept,
            r.red + r.strict_red + r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The stereo scope "with trails and a glow" (classic_plus) through review
/// 12's stereo noise gated at 7 Hz, in Calm and Intense and the starkest
/// schemes: within the limits. Its glow is light added, a tangle's lines
/// crossing over and over added it up past the faint light the tangle is
/// drawn in - 10 transitions a second in white on black, Calm too, by
/// review 12's meter.
#[test]
fn the_stereo_scope_with_trails_is_calm_through_gated_stereo_noise() {
    let mut failures = Vec::new();
    for motion in [Motion::Calm, Motion::Intense] {
        for scheme in ["bw", "wb", "classic", "fire"] {
            let r = measure(
                "lissajous",
                Play {
                    motion,
                    scheme,
                    size: (320.0, 180.0),
                    fps: 60.0,
                    music: music11::part("noisegate", 4.0, 5),
                },
                |state| state.classic_plus = true,
            );
            let line = format!(
                "lissajous with trails {motion:?} {scheme}: transitions/s {} (strict {}) area {} red {}, review's meter {}",
                r.transitions,
                r.strict,
                r.area,
                r.red + r.strict_red + r.red_gamma,
                r.review
            );
            println!("{line}");
            // (the project's meter read it within the limit; review 12's
            // own did not: both are asked)
            if r.fails() || r.review > 6 {
                failures.push(line);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Switching between a face and "off" quickly (every 4 frames, as a hand
/// on V and the menu may): crossfaded as any change of face, and the guard
/// watches it - "off" went by both, review 12 read 15 - 16 transitions a
/// second switching plasma and off.
#[test]
fn switching_to_and_from_off_is_crossfaded_and_guarded() {
    let mut h = faces::harness("plasma", vec2(320.0, 180.0), true, 1.0);
    h.state_mut().sound = faces::signal(4.5);
    let mut meter = Meter::with_fps(320, 180, 60);
    let mut review = m11::M11::new(320, 180, 60.0);
    for f in 0..240 {
        if f % 4 == 0 {
            let app = h.state_mut();
            app.key = if app.key == "off" { "plasma" } else { "off" };
        }
        faces::frame(&mut h);
        let img = h.render().unwrap();
        meter.push(&img);
        review.push_rgba(&img);
    }
    let (transitions, _) = meter.most();
    let (strict, _) = meter.most_strict();
    let r = review.reading();
    let line = format!(
        "plasma <-> off every 4 frames: transitions/s {transitions} (strict {strict}) area {}, review's meter {}",
        meter.area,
        r.c[0].max(r.c[1])
    );
    println!("{line}");
    assert!(
        transitions <= 6 && strict <= 6 && meter.area <= 6 && r.c[0].max(r.c[1]) <= 6,
        "{line}"
    );
}

/// Review 11's worst music (its music11.rs: chords gated at 6 - 10 Hz,
/// stereo noise, sweeps, 175 BPM beats, kicks eight a second, a chord in
/// antiphase, 30 ms bursts) through the cases its meter found over the
/// project's own limits: the particles in Classic; the LED equalizer, the
/// particles, the line, the mirrored spectrum and the bell curves in pure
/// red; the piano roll and the oscilloscope in black on white and white on
/// black; the tunnel in black on white; and in Calm the spectrum in pure
/// red, the Julia set and the reaction-diffusion in black on white, the
/// flow field in pure red - every reading within the limit. (In Intense
/// the spectrum in pure red and the Julia set in black on white read 8 by
/// the strict reading: docs/FLASH_SAFETY.md says so.)
#[test]
fn review_11s_cases_keep_within_the_limits() {
    // (AMP_FLASH_ONLY=tunnel,...: those faces only, as the other runs)
    let only = std::env::var("AMP_FLASH_ONLY").ok();
    let wanted = |k: &str| only.as_deref().is_none_or(|o| o.split(',').any(|w| w == k));
    let mut failures = Vec::new();
    for (key, motion, scheme) in [
        ("particles", Motion::Calm, "classic"),
        ("led", Motion::Intense, "redcustom"),
        ("particles", Motion::Intense, "redcustom"),
        ("line", Motion::Intense, "redcustom"),
        ("pianoroll", Motion::Intense, "bw"),
        ("pianoroll", Motion::Intense, "wb"),
        ("scope", Motion::Intense, "bw"),
        ("scope", Motion::Intense, "wb"),
        ("tunnel", Motion::Intense, "wb"),
        ("spectrum", Motion::Calm, "redcustom"),
        ("mirror", Motion::Intense, "redcustom"),
        ("bells", Motion::Intense, "redcustom"),
        ("julia", Motion::Calm, "wb"),
        ("flowfield", Motion::Calm, "redcustom"),
        ("reaction", Motion::Calm, "wb"),
    ] {
        if !wanted(key) {
            continue;
        }
        let music: Vec<f32> = music11::worst(1.2);
        let r = measure(
            key,
            Play {
                motion,
                scheme,
                size: (320.0, 180.0),
                fps: 60.0,
                music,
            },
            |_| {},
        );
        let line = format!(
            "{key} {motion:?} {scheme}: transitions/s {} (strict {}) area {} stripes {:.2} ({} frames) red {}",
            r.transitions,
            r.strict,
            r.area,
            r.stripes,
            r.pattern_frames,
            r.red + r.strict_red + r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn twelve_frames_a_second_in_every_scheme() {
    let mut failures = Vec::new();
    for (key, motion, scheme) in [
        ("line", Motion::Calm, "redcustom"),
        ("flowfield", Motion::Intense, "classic"),
        ("flowfield", Motion::Intense, "fire"),
        ("flowfield", Motion::Intense, "redcustom"),
    ] {
        let r = run(key, motion, scheme, (320.0, 180.0), 1.2, 12.0, |_| {});
        let line = format!(
            "{motion:?} {scheme} {key} at 12: transitions/s {} ({}) area {} stripes {:.2} ({} frames) red {}",
            r.transitions,
            r.strict,
            r.area,
            r.stripes,
            r.pattern_frames,
            r.red + r.strict_red + r.red_gamma
        );
        println!("{line}");
        if r.fails() {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A whole picture flickering in saturated red at `hz` (over `ground`,
/// black or a lit grey), drawn through the flash guard for `secs` at 60
/// frames a second: the project's and the review's meters' red readings
/// (the most in any second), and the share of frames veiled.
struct RedStrobe {
    hz: f32,
    ground: egui::Color32,
    frame: usize,
    guard: axross_amp::viz::flashguard::FlashGuard,
}

impl eframe::App for RedStrobe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                let layer = painter.layer_id();
                let first = painter
                    .ctx()
                    .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
                let t = self.frame as f32 / 60.0;
                let on = ((t * self.hz * 2.0) as u32).is_multiple_of(2);
                painter.rect_filled(
                    rect,
                    0.0,
                    if on {
                        egui::Color32::from_rgb(255, 0, 0)
                    } else {
                        self.ground
                    },
                );
                self.guard
                    .guard(painter, first, rect, &[], None, 1.0 / 60.0);
            });
    }
}

fn red_strobe(hz: f32, ground: egui::Color32, secs: f32) -> (usize, usize, usize, f32) {
    let (w, h) = (480usize, 270usize);
    let mut guard = axross_amp::viz::flashguard::FlashGuard::default();
    guard.screen_is = Some(vec2(w as f32, h as f32));
    let mut app = Some(RedStrobe {
        hz,
        ground,
        frame: 0,
        guard,
    });
    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(w as f32, h as f32))
        .wgpu()
        .build_eframe(move |_| app.take().unwrap());
    let mut meter = Meter::with_fps(w, h, 60);
    let mut gamma = Meter::gamma_red(w, h, 60);
    let mut review = m11::M11::with(w, h, 60.0, true);
    let mut veiled = 0;
    let frames = (secs * 60.0) as usize;
    for f in 0..frames {
        harness.state_mut().frame = f;
        harness.step();
        let img = harness.render().unwrap();
        meter.push(&img);
        gamma.push(&img);
        review.push_rgba(&img);
        if harness.state().guard.strength > 0.3 {
            veiled += 1;
        }
    }
    let project = meter
        .most()
        .1
        .max(meter.most_strict().1)
        .max(meter.area_red)
        .max(gamma.most().1)
        .max(gamma.most_strict().1)
        .max(gamma.area_red);
    let r = review.reading();
    (
        project,
        r.c[1],
        meter.most().0,
        veiled as f32 / frames as f32,
    )
}

/// No saturated red flash passes the guard, however slow (review 12: a
/// whole picture flickering red at 2 Hz - four red transitions a second,
/// two red flashes - was not veiled at all: the guard waited for the
/// general budget, three flashes a second, as for any other). The first
/// red change is veiled as it comes (a flash is a pair: the rise let
/// through, its fall follows whatever the veil does); a white flicker at
/// 2 Hz, within the rule, is left alone.
#[test]
fn no_saturated_red_flash_passes_the_guard() {
    let mut failures = Vec::new();
    for (hz, ground, name) in [
        (2.0, egui::Color32::BLACK, "red on black at 2 Hz"),
        (4.0, egui::Color32::BLACK, "red on black at 4 Hz"),
        (2.0, egui::Color32::from_gray(90), "red on grey at 2 Hz"),
    ] {
        let (project, review, lum, veiled) = red_strobe(hz, ground, 5.0);
        let line = format!(
            "{name}: red transitions a second {project} (the review's meter {review}), luminance {lum}, veiled {:.0} %",
            veiled * 100.0
        );
        println!("{line}");
        // (one red change is no flash; a pair is)
        if project > 1 || review > 1 || lum > 6 {
            failures.push(line);
        }
    }
    // (a white flicker within the rule: no veil)
    let (lum, veiled) = white_strobe(2.0, 5.0);
    println!(
        "white on black at 2 Hz: luminance {lum}, veiled {:.0} %",
        veiled * 100.0
    );
    if veiled > 0.01 {
        failures.push(format!(
            "a white flicker at 2 Hz veiled {:.0} %",
            veiled * 100.0
        ));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// As [`red_strobe`], white on black: the luminance reading and the share
/// of frames veiled.
fn white_strobe(hz: f32, secs: f32) -> (usize, f32) {
    struct White(RedStrobe);
    impl eframe::App for White {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let s = &mut self.0;
                    let rect = ui.max_rect();
                    let painter = ui.painter();
                    let layer = painter.layer_id();
                    let first = painter
                        .ctx()
                        .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
                    let t = s.frame as f32 / 60.0;
                    let on = ((t * s.hz * 2.0) as u32).is_multiple_of(2);
                    painter.rect_filled(
                        rect,
                        0.0,
                        if on {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::BLACK
                        },
                    );
                    s.guard.guard(painter, first, rect, &[], None, 1.0 / 60.0);
                });
        }
    }
    let (w, h) = (480usize, 270usize);
    let mut guard = axross_amp::viz::flashguard::FlashGuard::default();
    guard.screen_is = Some(vec2(w as f32, h as f32));
    let mut app = Some(White(RedStrobe {
        hz,
        ground: egui::Color32::BLACK,
        frame: 0,
        guard,
    }));
    let mut harness = egui_kittest::Harness::builder()
        .with_size(vec2(w as f32, h as f32))
        .wgpu()
        .build_eframe(move |_| app.take().unwrap());
    let mut meter = Meter::with_fps(w, h, 60);
    let mut veiled = 0;
    let frames = (secs * 60.0) as usize;
    for f in 0..frames {
        harness.state_mut().0.frame = f;
        harness.step();
        meter.push(&harness.render().unwrap());
        if harness.state().0.guard.strength > 0.3 {
            veiled += 1;
        }
    }
    (meter.most().0, veiled as f32 / frames as f32)
}
