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
        // (covers made for tracks without one, as the player has them: their
        // coming and their accent are part of what is tested - review of
        // 0.17.1, L4)
        stand_in_covers: true,
    };
    Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

fn hover(h: &mut Harness<'static, App>, at: Pos2) {
    h.event(Event::PointerMoved(at));
    // (a submenu the pointer opens finds its place - beside, above or below
    // its button - in the frame after it is measured)
    settle(h, 4);
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
fn the_picker_shows_a_design_once_clicked_and_keeps_it() {
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
    // the pointer on the rack or the terminal: nothing changes (shown on
    // the pointer's way, the whole window flashed - review 13)
    let rack = radio(&h, "Hi-fi rack");
    hover(&mut h, rack.center());
    assert_eq!(
        h.state().design_shown(),
        "modern",
        "shown on the pointer's way"
    );
    let terminal = radio(&h, "Terminal");
    hover(&mut h, terminal.center());
    assert_eq!(h.state().design_shown(), "modern");
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
                failures.push(format!(
                    "{what}: {a} and {b} lie over each other ({ra:?}, {rb:?})"
                ));
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
    assert!(
        until(h, 3.0, |a| !a.playing()),
        "{what}: Pause did not pause"
    );
    // (its name follows on the next frame: on a busy machine, wait for it)
    settle(h, 2);
    assert!(
        h.query_by_label("Play").is_some(),
        "{what}: paused, but no Play to press"
    );
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

const SIZES: [(f32, f32); 5] = [
    (420.0, 420.0),
    (800.0, 600.0),
    (1920.0, 1080.0),
    // (wide and low: a laptop's window half the screen tall)
    (900.0, 400.0),
    (1600.0, 420.0),
];

#[test]
fn every_design_keeps_every_control_at_every_size() {
    // (AMP_DESIGN_ONLY=key,key: those alone, while one is worked on)
    let only = std::env::var("AMP_DESIGN_ONLY").ok();
    for d in DESIGNS.iter().filter(|d| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|k| k == d.key))
    }) {
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

/// Every design at the window's default size (620 x 860) and at every
/// width from 400 to 900, 10 apart, at that height: every control within
/// the window, large enough, none over another (review 17, M4: Brutalist at
/// 600 - 630 points wide - the default - had Next half outside the window).
#[test]
fn every_control_lies_within_the_window_at_every_width() {
    let only = std::env::var("AMP_DESIGN_ONLY").ok();
    let mut failures = Vec::new();
    for d in DESIGNS.iter().filter(|d| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|k| k == d.key))
    }) {
        let home = Scratch::new(&format!("design-widths-{}", d.key));
        let tracks = two_tracks(&home);
        let mut h = window(&home, tracks, egui::vec2(620.0, 860.0));
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(d.key);
        settle(&mut h, 3);
        for w in std::iter::once(620).chain((400..=900).step_by(10)) {
            let size = egui::vec2(w as f32, 860.0);
            h.set_size(size);
            settle(&mut h, 3);
            let what = format!("{} at {w} x 860", d.name);
            if std::env::var_os("AMP_DESIGN_SHOTS").is_some() {
                h.render()
                    .unwrap()
                    .save(shots().join(format!("{}-w{w}.png", d.key)))
                    .unwrap();
            }
            let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check_controls(&mut h, &what, size)
            }));
            if let Err(e) = checked {
                let said = e
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| format!("{what}: failed"));
                failures.push(said);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
        // (the designs with something of their own that moves: reels,
        // needles, a counter, a readout, a marquee)
        .filter(|d| d.extras.reels || d.extras.needles || d.extras.lcd || d.fade_controls)
    {
        let size = egui::vec2(800.0, 600.0);
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
        // (what a design adds lies under the bars at the top: the card, the
        // rack's display and amplifier, the cassette, the 90s strip - the
        // window's upper part, as the whole screen)
        let top = 80u32;
        let card = (size.x as u32, 300u32);
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
    let size = egui::vec2(1280.0, 800.0);
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
    // two across, five down, each at half its size
    let (w, ht) = (640u32, 400u32);
    let gap = 12u32;
    let mut page = image::RgbaImage::from_pixel(
        2 * w + 3 * gap,
        5 * ht + 6 * gap,
        image::Rgba([24, 24, 28, 255]),
    );
    for (i, img) in shots_made.iter().enumerate() {
        let small = image::imageops::resize(img, w, ht, image::imageops::FilterType::Lanczos3);
        let (x, y) = (i as u32 % 2, i as u32 / 2);
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

/// Tab goes through the controls in the order they are seen: row by row,
/// left to right in a row, a column to the right after the one at its
/// left - in every design (a group laid out right to left, as Clear before
/// Save… was, would go backwards).
#[test]
fn tab_follows_what_is_seen() {
    use egui_kittest::kittest::NodeT;
    let only = std::env::var("AMP_DESIGN_ONLY").ok();
    let mut failures = Vec::new();
    for d in DESIGNS.iter().filter(|d| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|k| k == d.key))
    }) {
        for (w, ht) in [(800.0, 600.0), (1920.0, 1080.0)] {
            let home = Scratch::new(&format!("design-tab-{}-{w}", d.key));
            let tracks = two_tracks(&home);
            let mut h = window(&home, tracks, egui::vec2(w, ht));
            assert!(until(&mut h, 5.0, |a| a.is_loaded()));
            h.state_mut().set_design(d.key);
            settle(&mut h, 4);
            let mut seen: Vec<(String, egui::Rect)> = Vec::new();
            for _ in 0..90 {
                h.key_press(egui::Key::Tab);
                h.run_steps(1);
                let Some(node) = h.query_by(|n| n.is_focused()) else {
                    continue;
                };
                let label = node.accesskit_node().label().unwrap_or_default();
                let rect = node.rect();
                if seen.first().is_some_and(|(l, r)| *l == label && *r == rect) {
                    break;
                }
                seen.push((label, rect));
            }
            for pair in seen.windows(2) {
                let ((a, ra), (b, rb)) = (&pair[0], &pair[1]);
                let same_row =
                    (rb.center().y - ra.center().y).abs() < ra.height().min(rb.height()) * 0.6;
                let above = rb.bottom() <= ra.top() + 1.0;
                let column_right = rb.left() >= ra.right() - 1.0;
                let backwards = if same_row {
                    rb.center().x < ra.center().x - 1.0
                } else {
                    above && !column_right
                };
                if backwards {
                    failures.push(format!(
                        "{} at {w} x {ht}: {b:?} {rb:?} after {a:?} {ra:?}",
                        d.name
                    ));
                }
            }
            if seen.len() < 15 {
                failures.push(format!(
                    "{} at {w} x {ht}: only {} stops by Tab",
                    d.name,
                    seen.len()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The 90s desktop's title, too long for its strip, moves on while paused
/// (0.16.0's marquee stood cut then; src/marquee.rs) - and nothing else in
/// the strip moves: what changes lies within the title's box.
#[test]
fn a_paused_90s_desktop_moves_its_title_only() {
    let home = Scratch::new("design-90s-idle");
    // (a title long enough to move)
    let long = home.join("A title far too long to fit in the little window of the strip.wav");
    write_chord(&long, 30);
    let mut h = window(
        &home,
        vec![long.to_string_lossy().into_owned()],
        egui::vec2(800.0, 600.0),
    );
    axross_amp::marquee::record(&h.ctx, true);
    // (no cover made for the track: its coming, and the accent it brings,
    // would change the strip beside the title)
    let ctx = h.ctx.clone();
    h.state_mut().set_generated_covers(&ctx, false);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.state_mut().set_design("desktop90");
    h.state_mut().set_mode("off");
    h.state_mut().set_desktop_reduces_motion(false);
    // (as on a GPU: a character at a time, some four a second)
    h.state_mut().set_slow_renderer(false);
    settle(&mut h, 3);
    if h.state().playing() {
        h.key_press(egui::Key::Space);
    }
    // (the face's name, shown for a moment after it changed, gone; and the
    // change to "off" crossfaded to its end - 0.6 s of the clock, which
    // frames run back to back do not pass)
    h.run_steps(50);
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        h.run_steps(1);
    }
    assert!(!h.state().playing());
    // (past its wait at the start: moving)
    let moving = |h: &Harness<'static, App>| {
        axross_amp::marquee::seen(&h.ctx)
            .into_iter()
            .find(|m| m.moving && m.text.contains("A title far too long"))
    };
    for _ in 0..100 {
        if moving(&h).is_some() {
            break;
        }
        h.run_steps(1);
    }
    let words = moving(&h).expect("the title moves, paused").rect;
    let strip = |h: &mut Harness<'static, App>| {
        let img = h.render().expect("renders");
        image::imageops::crop_imm(&img, 0, 80, 800, 120).to_image()
    };
    let before = strip(&mut h);
    h.run_steps(10);
    let after = strip(&mut h);
    let moved: Vec<(u32, u32)> = before
        .enumerate_pixels()
        .filter(|(x, y, p)| after.get_pixel(*x, *y) != *p)
        .map(|(x, y, _)| (x, y))
        .collect();
    assert!(!moved.is_empty(), "the paused title stood");
    let inside = words.expand(2.0);
    let outside: Vec<&(u32, u32)> = moved
        .iter()
        .filter(|(x, y)| !inside.contains(egui::pos2(*x as f32 + 0.5, *y as f32 + 80.5)))
        .collect();
    assert!(
        outside.is_empty(),
        "the paused strip moved outside its title's box {words:?}: {} pixels, e.g. {:?}",
        outside.len(),
        outside.first()
    );
}

/// The words each design writes, where they are seen: (text, its rect, the
/// clip rect it is cut to).
type Words = Vec<(String, egui::Rect, egui::Rect)>;

fn texts(h: &Harness<'static, App>) -> Words {
    fn walk(shape: &egui::Shape, clip: egui::Rect, out: &mut Words) {
        match shape {
            egui::Shape::Text(t) => {
                let text = t.galley.job.text.trim().to_owned();
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                if !text.is_empty()
                    && rect.intersect(clip).is_positive()
                    && t.fallback_color.a() > 0
                {
                    out.push((text, rect, clip));
                }
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.output().shapes {
        walk(&c.shape, c.clip_rect, &mut out);
    }
    out
}

/// Every design at every size (the low and wide ones too): no words written
/// over other words (the cassette's label at 420 x 420 and 1400 x 460 had
/// the title over the artist: review 10), and the playlist's Clear and
/// durations not cut off (the terminal at 420 x 420).
#[test]
fn no_design_writes_words_over_words() {
    let only = std::env::var("AMP_DESIGN_ONLY").ok();
    let mut failures = Vec::new();
    for d in DESIGNS.iter().filter(|d| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|k| k == d.key))
    }) {
        for (w, ht) in SIZES
            .iter()
            .copied()
            .chain([(1400.0, 460.0), (1280.0, 800.0)])
        {
            let home = Scratch::new(&format!("design-words-{}-{w}", d.key));
            let tracks = two_tracks(&home);
            let size = egui::vec2(w, ht);
            let mut h = window(&home, tracks, size);
            assert!(until(&mut h, 5.0, |a| a.is_loaded()));
            h.state_mut().set_design(d.key);
            // (in the theme it suggests: the terminal's is monospace)
            h.state_mut().set_theme(d.suggests);
            // (a track's tags, as a real one has: a title and an artist)
            let ctx = h.ctx.clone();
            h.state_mut().show_info(
                &ctx,
                axross_amp::engine::TrackInfo {
                    title: "Night Engines".into(),
                    artist: "Ada Lovelace Ensemble".into(),
                    album: "Analytical".into(),
                    rate: 48_000,
                    channels: 2,
                    bits: Some(16),
                    duration: Some(40.0),
                    seekable: true,
                    ..Default::default()
                },
            );
            settle(&mut h, 3);
            let what = format!("{} at {w} x {ht}", d.name);
            let words = texts(&h);
            // (an LCD's unlit segments under its digits: "88:88" under
            // "01:23" - one picture, not words over words)
            let ghost = |a: &str, b: &str| {
                let unlit = |s: &str| -> String {
                    s.chars()
                        .map(|c| if c.is_ascii_digit() { '8' } else { c })
                        .collect()
                };
                (a.chars().all(|c| !c.is_ascii_digit() || c == '8')
                    || b.chars().all(|c| !c.is_ascii_digit() || c == '8'))
                    && unlit(a).len() == unlit(b).len()
            };
            for (i, (a, ra, ca)) in words.iter().enumerate() {
                for (b, rb, cb) in &words[i + 1..] {
                    let over = ra.intersect(*ca).intersect(rb.intersect(*cb));
                    // (more than a sliver: a third of the smaller one's height
                    // and a few points along)
                    if over.is_positive()
                        && !ghost(a, b)
                        && over.height() > ra.height().min(rb.height()) / 3.0
                        && over.width() > 4.0
                    {
                        failures.push(format!("{what}: {a:?} over {b:?}"));
                    }
                }
            }
            // the playlist's buttons and durations whole in sight, not cut
            // by the box they are in
            let duration = |t: &str| {
                let (m, sec) = t.split_once(':').unwrap_or(("", ""));
                !m.is_empty()
                    && m.chars().all(|c| c.is_ascii_digit())
                    && sec.len() == 2
                    && sec.chars().all(|c| c.is_ascii_digit())
            };
            for (text, rect, clip) in &words {
                let whole = ["Clear", "Save…", "Sort ▾", "Equalizer"].contains(&text.as_str())
                    || duration(text);
                // (cut along the row by its box - not a row half scrolled
                // into sight - nor out over the design's own margin)
                let inside = rect.left() >= clip.left() - 0.5
                    && rect.right() <= clip.right().min(w - 4.0) + 0.5;
                if whole && rect.center().y < clip.bottom() && !inside {
                    failures.push(format!("{what}: {text:?} cut off ({rect:?} in {clip:?})"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Wide and low windows (half a laptop's screen tall): the picture is not
/// squeezed to a strip under the list - the list goes beside it (Modern,
/// Glass, Soft, the rack and the collage had it about 10 points tall at
/// 1400 x 460: review 10).
#[test]
fn a_wide_and_low_window_keeps_the_picture() {
    let only = std::env::var("AMP_DESIGN_ONLY").ok();
    let mut failures = Vec::new();
    for d in DESIGNS.iter().filter(|d| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|k| k == d.key))
    }) {
        // (the cassette's picture is the window of a cassette; the 90s
        // desktop's, the terminal's and Zen's were small at 900 x 400 too:
        // review 11)
        let least = match d.key {
            "cassette" => 40.0,
            _ => 60.0,
        };
        for (w, ht) in [(900.0, 400.0), (1600.0, 420.0), (1400.0, 460.0)] {
            let home = Scratch::new(&format!("design-low-{}-{w}", d.key));
            let tracks = two_tracks(&home);
            let mut h = window(&home, tracks, egui::vec2(w, ht));
            assert!(until(&mut h, 5.0, |a| a.is_loaded()));
            h.state_mut().set_design(d.key);
            h.state_mut().set_theme(d.suggests);
            settle(&mut h, 4);
            let picture = h
                .query_all_by_label_contains("Visualization: ")
                .map(|n| n.rect())
                .max_by(|a, b| a.height().total_cmp(&b.height()));
            let tall = picture.map_or(0.0, |r| r.height());
            println!(
                "{} at {w} x {ht}: the picture {tall:.0} points tall",
                d.name
            );
            if tall < least {
                failures.push(format!(
                    "{} at {w} x {ht}: the picture {tall:.0} points tall",
                    d.name
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Glass and Soft are arrangements of their own, not Modern's in other
/// surfaces (review 10): Glass in two columns - the keys at the left of
/// the picture -, Soft centred - the play key in the middle, under the
/// picture.
#[test]
fn glass_and_soft_are_not_modern() {
    let mut failures = Vec::new();
    let size = egui::vec2(1280.0, 800.0);
    let shown = |key: &str| {
        let home = Scratch::new(&format!("design-own-{key}"));
        let tracks = two_tracks(&home);
        let mut h = window(&home, tracks, size);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(key);
        settle(&mut h, 4);
        let play = ["Pause", "Play"]
            .iter()
            .find_map(|l| h.query_by_label(l).map(|n| n.rect()))
            .expect("a play key");
        let picture = h
            .query_all_by_label_contains("Visualization: ")
            .map(|n| n.rect())
            .max_by(|a, b| a.height().total_cmp(&b.height()))
            .expect("the picture");
        (play, picture)
    };
    let (play, picture) = shown("glass");
    if play.right() > picture.left() {
        failures.push(format!(
            "Glass: the keys not beside the picture ({play:?}, {picture:?})"
        ));
    }
    let (play, picture) = shown("soft");
    if (play.center().x - picture.center().x).abs() > picture.width() * 0.1
        || play.top() < picture.bottom()
    {
        failures.push(format!(
            "Soft: the play key not centred under the picture ({play:?}, {picture:?})"
        ));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A very large window (3000 x 1600) is used: the picture and the playlist
/// span most of it, not a column of 1180 points in the middle (review 11:
/// eight designs stayed a narrow column). Zen alone is one column by its
/// nature.
/// At 3000 x 1600 every design gives the picture room: at least an eighth
/// of the window's height and a fifth of its width (review 12: Zen kept a
/// column of 720 points with a strip of a picture, the Cassette a picture
/// of some 400 x 150 under its reels).
#[test]
fn a_very_large_window_gives_the_picture_room() {
    let mut failures = Vec::new();
    let size = egui::vec2(3000.0, 1600.0);
    for d in DESIGNS.iter() {
        let home = Scratch::new(&format!("design-room-{}", d.key));
        let tracks = two_tracks(&home);
        let mut h = window(&home, tracks, size);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(d.key);
        settle(&mut h, 4);
        let picture = h.get_by_label_contains("Visualization: ").rect();
        if picture.height() < size.y / 8.0 || picture.width() < size.x / 5.0 {
            failures.push(format!(
                "{}: the picture {:.0} x {:.0}",
                d.name,
                picture.width(),
                picture.height()
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_very_large_window_is_used() {
    let mut failures = Vec::new();
    let size = egui::vec2(3000.0, 1600.0);
    for d in DESIGNS.iter().filter(|d| d.key != "zen") {
        let home = Scratch::new(&format!("design-large-{}", d.key));
        let tracks = two_tracks(&home);
        let mut h = window(&home, tracks, size);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(d.key);
        settle(&mut h, 4);
        let mut span = egui::Rect::NOTHING;
        for r in h
            .query_all_by_label_contains("Visualization: ")
            .map(|n| n.rect())
            .chain(h.query_by_label("Filter the playlist").map(|n| n.rect()))
        {
            span = span.union(r);
        }
        if span.width() < size.x * 0.6 {
            failures.push(format!(
                "{}: {:.0} points of {}",
                d.name,
                span.width(),
                size.x
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
