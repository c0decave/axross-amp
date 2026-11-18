//! The window's interface designs (orthogonal to the themes): the picker at
//! ⚙ → Design shows a design while the pointer is on it and keeps the one
//! clicked (in axross-amp's own settings); in every design, at a small, a
//! middling and a large window, every control is there, large enough to
//! hit, and does what it says when clicked - and the playlist is in sight.
//! The designs that move (the reels, the needles) are put through the flash
//! meter with the picture off. With `--ignored`: every design's picture in
//! `target/screenshots/designs/` and a gallery of them, `docs/designs.png`.

#[path = "support/player.rs"]
mod player;

#[path = "support/flashmeter.rs"]
mod flashmeter;

use axross_amp::app::{App, Setup};
use axross_amp::design::DESIGNS;
use axross_amp::engine::RenderState;
use egui::accesskit::Role;
use egui::{Event, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use player::{Scratch, settle, until, write_chord};
use std::path::{Path, PathBuf};

fn window(dir: &Path, args: Vec<String>, size: egui::Vec2) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

fn hover(h: &mut Harness<'static, App>, at: Pos2) {
    h.event(Event::PointerMoved(at));
    settle(h, 2);
}

fn click_at(h: &mut Harness<'static, App>, at: Pos2) {
    hover(h, at);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(h, 2);
}

fn two_tracks(home: &Path) -> Vec<String> {
    let a = home.join("First.wav");
    write_chord(&a, 40);
    let b = home.join("Second.wav");
    write_chord(&b, 20);
    vec![
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    ]
}

/// Stereo samples (48 kHz, -1..1) as a 16-bit WAV file.
fn write_samples(path: &Path, samples: &[f32]) {
    use std::io::Write;
    let rate = 48_000u32;
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for v in samples {
        out.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32_000.0) as i16).to_le_bytes());
    }
    std::fs::File::create(path)
        .unwrap()
        .write_all(&out)
        .unwrap();
}

/// `secs` of playing, taken at a device's pace.
fn play(h: &mut Harness<'static, App>, out: &mut RenderState, secs: f32) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
    let mut buf = vec![0.0f32; 480 * 2];
    while std::time::Instant::now() < end {
        h.state().engine().shared.render(&mut buf, 2, out);
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_picker_shows_a_design_at_once_and_keeps_the_one_chosen() {
    let home = Scratch::new("design-picker");
    let mut h = window(&home, vec![], egui::vec2(620.0, 860.0));
    settle(&mut h, 3);
    assert_eq!(h.state().design_shown(), "modern", "Modern by default");
    h.get_by_label("⚙").click();
    settle(&mut h, 2);
    let design = h.get_by_role_and_label(Role::Button, "Design").rect();
    hover(&mut h, design.center());
    for d in &DESIGNS {
        assert!(
            h.query_by_role_and_label(Role::RadioButton, d.name)
                .is_some(),
            "{} not offered",
            d.name
        );
    }
    let radio = |h: &Harness<'static, App>, name: &str| {
        h.get_by_role_and_label(Role::RadioButton, name).rect()
    };
    // the pointer on the rack: shown at once; off it: back
    let rack = radio(&h, "Hi-fi rack");
    hover(&mut h, rack.center());
    assert_eq!(h.state().design_shown(), "rack", "no preview");
    hover(&mut h, design.center());
    assert_eq!(h.state().design_shown(), "modern", "the preview stayed");
    // (the rows stay under the pointer while it previews: a design's
    // own type only once chosen)
    let terminal = radio(&h, "Terminal");
    hover(&mut h, terminal.center());
    assert_eq!(h.state().design_shown(), "terminal");
    assert_eq!(radio(&h, "Terminal"), terminal, "the rows moved");
    // a click keeps one
    let cassette = radio(&h, "Cassette deck").center();
    click_at(&mut h, cassette);
    h.event(Event::PointerGone);
    settle(&mut h, 3);
    assert_eq!(h.state().design_shown(), "cassette");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 3);
    assert_eq!(h.state().design_shown(), "cassette");
    let own = home.join("config/axross-amp.json");
    let mut kept = false;
    for _ in 0..200 {
        settle(&mut h, 1);
        if std::fs::read_to_string(&own).is_ok_and(|s| s.contains("\"design\": \"cassette\"")) {
            kept = true;
            break;
        }
    }
    assert!(kept, "not written to {own:?}");
    let shared = std::fs::read_to_string(home.join("config/media_player.json")).unwrap_or_default();
    assert!(!shared.contains("cassette"), "the design in axross's file");
    drop(h);
    let mut h = window(&home, vec![], egui::vec2(620.0, 860.0));
    settle(&mut h, 3);
    assert_eq!(h.state().design_shown(), "cassette", "not kept");
}

#[test]
fn the_picker_answers_the_keys() {
    // the menu by its keys alone: focus on a design's row, Enter
    let home = Scratch::new("design-keys");
    let mut h = window(&home, vec![], egui::vec2(620.0, 860.0));
    settle(&mut h, 3);
    h.get_by_label("⚙").click();
    settle(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Design").click();
    settle(&mut h, 2);
    h.get_by_role_and_label(Role::RadioButton, "Brutalist")
        .focus();
    settle(&mut h, 2);
    h.key_press(egui::Key::Enter);
    settle(&mut h, 3);
    assert_eq!(h.state().design_shown(), "brutalist");
}

/// The controls by their names, each with what clicking it must change.
const CONTROLS: [&str; 20] = [
    "Files",
    "Folder",
    "URL",
    "⚙",
    "Position",
    "Previous",
    "Back 15 s",
    "Stop",
    "Forward 15 s",
    "Next",
    "Shuffle: off",
    "Repeat: off",
    "Volume",
    "Mute",
    "More",
    "Equalizer",
    "Save…",
    "Clear",
    "Filter the playlist",
    "Sort ▾",
];

/// The least a control may be to hit (points): WCAG 2.2's 24 x 24 for the
/// buttons; a slider is a line along its length, its height the bar.
fn least(name: &str) -> egui::Vec2 {
    match name {
        "Position" | "Volume" => egui::vec2(24.0, 18.0),
        _ => egui::vec2(24.0, 24.0),
    }
}

fn check_controls(h: &mut Harness<'static, App>, what: &str, size: egui::Vec2) {
    let screen = egui::Rect::from_min_size(Pos2::ZERO, size);
    let mut failures = Vec::new();
    // (it plays at once: Play is Pause by now; the first row at least
    // of the playlist in sight, the rest a scroll away)
    let play = if h.query_by_label("Pause").is_some() {
        "Pause"
    } else {
        "Play"
    };
    for name in CONTROLS.iter().chain(&[play, "First"]) {
        // (a track's title is on the card and in the playlist: its
        // playlist's row, the lowest)
        let Some(r) = h
            .query_all_by_label(name)
            .map(|n| n.rect())
            .max_by(|a, b| a.top().total_cmp(&b.top()))
        else {
            failures.push(format!("{what}: {name} is gone"));
            continue;
        };
        if !screen.contains_rect(r.shrink(0.5)) {
            failures.push(format!("{what}: {name} lies off the window at {r:?}"));
        }
        let want = least(name);
        if *name == "First" {
            continue;
        }
        if r.width() < want.x - 0.01 || r.height() < want.y - 0.01 {
            failures.push(format!(
                "{what}: {name} is {:.0} x {:.0}, less than {:.0} x {:.0}",
                r.width(),
                r.height(),
                want.x,
                want.y
            ));
        }
    }
    // none lies over another (a click meant for one would reach the other)
    let rects: Vec<(&str, egui::Rect)> = CONTROLS
        .iter()
        .filter_map(|name| h.query_by_label(name).map(|n| (*name, n.rect())))
        .chain(
            // (the faces' list where there is room for it: in ⚙ as well)
            ["Play", "Pause", "Visualization"]
                .iter()
                .filter_map(|name| h.query_by_label(name).map(|n| (*name, n.rect()))),
        )
        .collect();
    for (i, (a, ra)) in rects.iter().enumerate() {
        for (b, rb) in &rects[i + 1..] {
            if ra.shrink(1.0).intersects(rb.shrink(1.0)) {
                failures.push(format!("{what}: {a} and {b} lie over each other"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// What the window's clicks did: each control clicked where it is drawn
/// (a pointer, not an accessibility action: something lying over it would
/// take the click).
fn click_through(h: &mut Harness<'static, App>, what: &str, out: &mut RenderState) {
    let centre = |h: &Harness<'static, App>, name: &str| {
        h.query_all_by_label(name)
            .map(|n| n.rect())
            .max_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| {
                panic!(
                    "{what}: {name} is gone (playing {}, large {}, tab {:?}, status {:?})",
                    h.state().playing(),
                    h.state().big_visual(),
                    h.state().tab(),
                    h.state().status()
                )
            })
            .center()
    };
    // pause, play again
    if h.query_by_label("Pause").is_none() {
        let at = centre(h, "Play");
        click_at(h, at);
    }
    play(h, out, 0.3);
    assert!(h.state().playing(), "{what}: Play did not play");
    let at = centre(h, "Pause");
    click_at(h, at);
    assert!(!h.state().playing(), "{what}: Pause did not pause");
    let at = centre(h, "Play");
    click_at(h, at);
    play(h, out, 0.3);
    assert!(h.state().playing(), "{what}: Play did not play");
    // on 15 s
    let before = h.state().engine().shared.position();
    let at = centre(h, "Forward 15 s");
    click_at(h, at);
    play(h, out, 0.2);
    let after = h.state().engine().shared.position();
    assert!(
        after > before + 10.0,
        "{what}: Forward 15 s: {before} → {after}"
    );
    let at = centre(h, "Back 15 s");
    click_at(h, at);
    play(h, out, 0.2);
    assert!(
        h.state().engine().shared.position() < after - 10.0,
        "{what}: Back 15 s did nothing"
    );
    // next, previous
    let at = centre(h, "Next");
    click_at(h, at);
    assert!(
        until(h, 3.0, |a| a.snapshot().current == 1),
        "{what}: Next did not go on"
    );
    let at = centre(h, "Previous");
    click_at(h, at);
    assert!(
        until(h, 3.0, |a| a.snapshot().current == 0),
        "{what}: Previous did not go back"
    );
    // shuffle, repeat: their names change with them
    let at = centre(h, "Shuffle: off");
    click_at(h, at);
    assert!(h.state().snapshot().shuffle, "{what}: Shuffle");
    let at = centre(h, "Shuffle: on");
    click_at(h, at);
    let at = centre(h, "Repeat: off");
    click_at(h, at);
    assert_ne!(h.state().snapshot().repeat, "off", "{what}: Repeat");
    // the volume: the arrow keys on it, then the mute
    let v = h.state().snapshot().volume;
    let at = centre(h, "Volume");
    click_at(h, at);
    h.get_by_label("Volume").focus();
    settle(h, 1);
    h.key_press(egui::Key::ArrowDown);
    settle(h, 2);
    assert_ne!(
        h.state().snapshot().volume,
        v,
        "{what}: the volume did not move"
    );
    h.event(Event::PointerGone);
    settle(h, 1);
    let at = centre(h, "Mute");
    click_at(h, at);
    assert!(h.state().snapshot().muted, "{what}: Mute");
    let at = centre(h, "Mute");
    click_at(h, at);
    // stop; the playlist's row (the first, in sight: the list follows
    // what plays) - a double-click plays it again
    let at = centre(h, "Stop");
    click_at(h, at);
    assert!(!h.state().playing(), "{what}: Stop");
    let at = centre(h, "First");
    hover(h, at);
    // (well after the click on Stop: within 0.3 s it would be a third
    // click, not a double one)
    h.run_steps(8);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        h.run_steps(1);
    }
    play(h, out, 0.3);
    assert!(
        h.state().playing() && h.state().snapshot().current == 0,
        "{what}: the playlist's row did not play"
    );
    // the menus open where they are clicked
    for menu in ["⚙", "More"] {
        let at = centre(h, menu);
        click_at(h, at);
        assert!(
            h.query_by_role_and_label(Role::Button, "Theme").is_some()
                || h.query_by_label("Track details…").is_some()
                || h.query_by_label("Sleep timer").is_some(),
            "{what}: {menu} opened nothing"
        );
        h.key_press(egui::Key::Escape);
        settle(h, 2);
    }
}

const SIZES: [(f32, f32); 3] = [(420.0, 420.0), (620.0, 860.0), (1400.0, 900.0)];

#[test]
fn every_design_keeps_every_control_at_every_size() {
    for d in &DESIGNS {
        for (w, ht) in SIZES {
            // (each its own settings: the last one's shuffle and repeat
            // not carried over)
            let home = Scratch::new(&format!("design-{}-{w}", d.key));
            let tracks = two_tracks(&home);
            let size = egui::vec2(w, ht);
            let mut h = window(&home, tracks.clone(), size);
            assert!(until(&mut h, 5.0, |a| a.is_loaded()));
            h.state_mut().set_design(d.key);
            settle(&mut h, 3);
            assert_eq!(h.state().design_shown(), d.key);
            let what = format!("{} at {w} x {ht}", d.name);
            if std::env::var_os("AMP_DESIGN_SHOTS").is_some() {
                h.render()
                    .unwrap()
                    .save(shots().join(format!("{}-{w}x{ht}.png", d.key)))
                    .unwrap();
            }
            check_controls(&mut h, &what, size);
            let mut out = RenderState::default();
            click_through(&mut h, &what, &mut out);
        }
    }
}

/// The designs that move on their own (the cassette's reels, the rack's
/// needles and its readout), the picture off: under the meter's three
/// readings, through loud music.
#[test]
fn the_designs_that_move_do_not_flash() {
    let home = Scratch::new("design-flash");
    // the loudest music the meter knows (its beats, its silences)
    let loud = home.join("Loud.wav");
    write_samples(&loud, &flashmeter::worst_music(1.0));
    let loud = loud.to_string_lossy().into_owned();
    let mut failures = Vec::new();
    for d in DESIGNS
        .iter()
        .filter(|d| d.extras.reels || d.extras.needles || d.extras.lcd || d.fade_controls)
    {
        let size = egui::vec2(620.0, 860.0);
        let mut h = window(&home, vec![loud.clone()], size);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(d.key);
        h.state_mut().set_mode("off");
        settle(&mut h, 3);
        // (it plays at once; else Play)
        if let Some(at) = h.query_by_label("Play").map(|n| n.rect().center()) {
            click_at(&mut h, at);
        }
        h.event(Event::PointerGone);
        let mut out = RenderState::default();
        // the window as the screen, and the card alone as the screen (its
        // reels, needles and readout as large as a screen: the strictest)
        let title = h
            .query_all_by_label("Loud")
            .map(|n| n.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("the card's title");
        let top = (title.top() - 14.0).max(0.0) as u32;
        let card = (size.x as u32, 170u32);
        // (the window's steps, 20 a second, the sound at that pace)
        let mut whole = flashmeter::Meter::with_fps(size.x as usize, size.y as usize, 20);
        let mut alone = flashmeter::Meter::with_fps(card.0 as usize, card.1 as usize, 20);
        let mut buf = vec![0.0f32; 2400 * 2];
        let mut last: Option<image::RgbaImage> = None;
        let mut moved = 0;
        for _ in 0..120 {
            h.state().engine().shared.render(&mut buf, 2, &mut out);
            h.run_steps(1);
            let img = h.render().expect("renders");
            whole.push(&img);
            let crop = image::imageops::crop_imm(&img, 0, top, card.0, card.1).to_image();
            if last.as_ref().is_some_and(|l| l != &crop) {
                moved += 1;
            }
            alone.push(&crop);
            last = Some(crop);
        }
        for (what, meter) in [("the window", &whole), ("the card", &alone)] {
            let (lum, red) = meter.most();
            let (strict, strict_red) = meter.most_strict();
            let worst = [lum, red, strict, strict_red, meter.area, meter.area_red];
            if worst.iter().any(|&n| n > 6) || meter.pattern_frames > 0 {
                failures.push(format!(
                    "{} ({what}): transitions {lum} (red {red}), strict {strict} (red \
                     {strict_red}), area {} (red {}), pattern frames {}",
                    d.name, meter.area, meter.area_red, meter.pattern_frames
                ));
            }
            eprintln!(
                "{} ({what}): {worst:?} stripes {:.2}, {moved} of 120 frames changed",
                d.name, meter.stripes
            );
        }
        // (the meter saw them move: the reels turn, the needles swing)
        if d.extras.reels || d.extras.needles {
            assert!(moved > 20, "{}: still in {moved} frames of 120", d.name);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A cover: a sunset's gradient, a sun low in it.
fn sunset() -> std::sync::Arc<axross_amp::engine::Art> {
    let n = 256u32;
    let rgba: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let (x, y) = ((i % n) as f32 / n as f32, (i / n) as f32 / n as f32);
            let sun = ((x - 0.62).powi(2) + (y - 0.62).powi(2)).sqrt() < 0.16;
            if sun {
                [255, 214, 120, 255]
            } else if y > 0.7 {
                [(40.0 + 30.0 * x) as u8, 30, (70.0 + 40.0 * y) as u8, 255]
            } else {
                [
                    (90.0 + 160.0 * y) as u8,
                    (40.0 + 80.0 * y) as u8,
                    (140.0 - 60.0 * y) as u8,
                    255,
                ]
            }
        })
        .collect();
    std::sync::Arc::new(axross_amp::engine::Art {
        width: n,
        height: n,
        rgba,
        png: Vec::new(),
        large: None,
    })
}

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/designs");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every design, playing, at 620 x 860, in its suggested theme and in the
/// default one; a gallery of them: `docs/designs.png`.
#[test]
#[ignore]
fn every_design_in_a_gallery() {
    let home = Scratch::new("design-gallery");
    let mut tracks = two_tracks(&home);
    let c = home.join("Third.wav");
    write_chord(&c, 30);
    tracks.push(c.to_string_lossy().into_owned());
    let size = egui::vec2(620.0, 860.0);
    let mut shots_made = Vec::new();
    for d in &DESIGNS {
        let mut h = window(&home, tracks.clone(), size);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(d.key);
        h.state_mut().set_theme(d.suggests);
        settle(&mut h, 3);
        // (it plays at once; else Play)
        if let Some(at) = h.query_by_label("Play").map(|n| n.rect().center()) {
            click_at(&mut h, at);
        }
        h.event(Event::PointerGone);
        // a cover and tags, as a real track has (the glass design's haze
        // is the cover's; the collage sets the artist in its own type)
        let ctx = h.ctx.clone();
        h.state_mut().show_info(
            &ctx,
            axross_amp::engine::TrackInfo {
                title: "Night Engines".into(),
                artist: "Ada Lovelace Ensemble".into(),
                album: "Analytical".into(),
                art: Some(sunset()),
                rate: 48_000,
                channels: 2,
                bits: Some(16),
                duration: Some(40.0),
                seekable: true,
                ..Default::default()
            },
        );
        let mut out = RenderState::default();
        play(&mut h, &mut out, 1.5);
        h.event(Event::PointerGone);
        h.run_steps(2);
        let img = h.render().expect("renders");
        img.save(shots().join(format!("{}.png", d.key))).unwrap();
        shots_made.push(img);
    }
    // five across, two down, each at half its size
    let (w, ht) = (310u32, 430u32);
    let gap = 12u32;
    let mut page = image::RgbaImage::from_pixel(
        5 * w + 6 * gap,
        2 * ht + 3 * gap,
        image::Rgba([24, 24, 28, 255]),
    );
    for (i, img) in shots_made.iter().enumerate() {
        let small = image::imageops::resize(img, w, ht, image::imageops::FilterType::Lanczos3);
        let (x, y) = (i as u32 % 5, i as u32 / 5);
        image::imageops::overlay(
            &mut page,
            &small,
            (gap + x * (w + gap)) as i64,
            (gap + y * (ht + gap)) as i64,
        );
    }
    let docs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    page.save(docs.join("designs.png")).unwrap();
}
