//! The window in each of its states, rendered for a look (kept in
//! `target/screenshots/states/`): nothing to play, playing, paused, the
//! picture large, a track's details, the visualization's menu, a narrow
//! window, no sound device. Each state also says what it must: its buttons
//! and words are there.

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::engine::{NO_OUTPUT, RenderState};
use egui::{Event, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use player::{Scratch, settle, until, write_chord};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/states");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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

fn shoot(h: &mut Harness<'static, App>, name: &str) {
    // (the pointer out of the picture)
    h.event(egui::Event::PointerGone);
    h.run_steps(1);
    h.render()
        .expect("renders")
        .save(shots().join(format!("{name}.png")))
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

fn right_click(h: &mut Harness<'static, App>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(h, 2);
}

#[test]
fn every_state_of_the_window() {
    let home = Scratch::new("states");
    // nothing to play
    let mut h = window(&home.join("empty"), vec![], egui::vec2(620.0, 860.0));
    settle(&mut h, 3);
    let size = egui::vec2(620.0, 860.0);
    empty_state_says_what_to_do(&h, size);
    shoot(&mut h, "empty");
    drop(h);
    // ... in the smallest window too (420 x 420)
    let size = egui::vec2(420.0, 420.0);
    let mut h = window(&home.join("empty-small"), vec![], size);
    settle(&mut h, 3);
    empty_state_says_what_to_do(&h, size);
    shoot(&mut h, "empty-small");
    drop(h);

    let a = home.join("Chord and noise.wav");
    write_chord(&a, 40);
    let b = home.join("Second track.wav");
    write_chord(&b, 12);
    let files = vec![
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    ];
    let mut h = window(&home.join("full"), files.clone(), egui::vec2(620.0, 860.0));
    let mut out = RenderState::default();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 1.5);
    assert!(h.state().playing());
    shoot(&mut h, "playing");
    // wide: the cover a hero, and what comes next beside it
    {
        let mut wide = window(&home.join("wide"), files.clone(), egui::vec2(1400.0, 900.0));
        let mut wide_out = RenderState::default();
        assert!(until(&mut wide, 5.0, |a| a.is_loaded()));
        play(&mut wide, &mut wide_out, 1.0);
        assert!(
            wide.query_by_label("Up next").is_some(),
            "a wide window shows what comes next"
        );
        shoot(&mut wide, "wide");
    }
    // paused: the picture stands as it was (not emptied), the button says Play
    let picture = h.get_by_role(egui::accesskit::Role::Image).rect();
    let before = h.render().unwrap();
    h.key_press(egui::Key::Space);
    play(&mut h, &mut out, 1.5);
    assert!(!h.state().playing());
    shoot(&mut h, "paused");
    let after = h.render().unwrap();
    let drawn = |img: &image::RgbaImage| {
        let bg = *img.get_pixel(picture.left() as u32 + 6, picture.top() as u32 + 6);
        let mut n = 0;
        for y in picture.top() as u32 + 8..picture.bottom() as u32 - 8 {
            for x in picture.left() as u32 + 8..picture.right() as u32 - 8 {
                let p = img.get_pixel(x, y);
                if (0..3)
                    .map(|c| (p[c] as i32 - bg[c] as i32).abs())
                    .sum::<i32>()
                    > 30
                {
                    n += 1;
                }
            }
        }
        n
    };
    let (b, a) = (drawn(&before), drawn(&after));
    assert!(
        a as f32 > b as f32 * 0.6,
        "paused, the picture emptied: {b} -> {a}"
    );
    h.key_press(egui::Key::Space);
    play(&mut h, &mut out, 0.5);
    // the visualization's menu, the generative faces open
    let picture = h.get_by_role(egui::accesskit::Role::Image).rect();
    right_click(&mut h, picture.center());
    settle(&mut h, 2);
    for name in [
        "Spectrum",
        "Loudness (EBU R128)",
        "Kinetic typography",
        "Off",
    ] {
        assert!(h.query_by_label(name).is_some(), "{name} not in the menu");
    }
    // the colours and options below the faces, not under them (and their
    // menus open over nothing but themselves: they come last)
    let colours = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Colours")
        .rect();
    let last = h.get_by_label("Kinetic typography").rect();
    assert!(colours.top() >= last.bottom(), "{colours:?} under {last:?}");
    shoot(&mut h, "menu");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 2);
    // a track's details
    let row = h.get_by_label("Second track").rect();
    right_click(&mut h, row.center());
    h.get_by_label("ℹ Details…").click();
    settle(&mut h, 3);
    shoot(&mut h, "details");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 2);
    assert!(
        h.query_by_label("Track details").is_none(),
        "Escape leaves the details open"
    );
    // the equalizer's page
    h.get_by_label("Equalizer").click();
    play(&mut h, &mut out, 0.3);
    // flat: no band filled (once each was filled from the bottom, as if
    // cut by 12 dB); raised: filled from the 0 dB line up
    let band = h.get_by_label("Equalizer 31").rect();
    let shot = h.render().unwrap();
    let (above, below) = colour_above_and_below(&shot, band);
    assert!(
        above + below < 3,
        "a flat band filled: {above} above, {below} below"
    );
    shoot(&mut h, "equalizer-flat");
    // (a click near the top of its rail)
    let at = Pos2::new(band.center().x, band.top() + band.height() * 0.2);
    h.event(Event::PointerMoved(at));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
        h.run_steps(1);
    }
    assert!(h.query_by_label_contains("+7").is_some() || h.query_by_label_contains("+8").is_some());
    play(&mut h, &mut out, 0.3);
    let shot = h.render().unwrap();
    let (above, below) = colour_above_and_below(&shot, band);
    assert!(
        above > 10 && below < 3,
        "a raised band: {above} above, {below} below"
    );
    shoot(&mut h, "equalizer");
    h.get_by_label_contains("Playlist").click();
    play(&mut h, &mut out, 0.2);
    // the picture large (F11 in a harness: the picture alone), the title kept
    h.state_mut().set_mode("galaxy");
    h.key_press(egui::Key::F11);
    play(&mut h, &mut out, 1.0);
    assert!(h.state().big_visual());
    assert!(
        h.query_by_label_contains("Chord and noise").is_some(),
        "no title over the large picture"
    );
    shoot(&mut h, "large");
    h.key_press(egui::Key::Escape);
    drop(h);

    // the smallest window, playing: the controls are in it, side by side
    let mut h = window(
        &home.join("smallest"),
        files.clone(),
        egui::vec2(420.0, 420.0),
    );
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 1.0);
    shoot(&mut h, "smallest");
    let window_rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(420.0, 420.0));
    for name in ["Pause", "Next", "Repeat: off", "More", "Volume"] {
        let r = h.get_by_label(name).rect();
        assert!(window_rect.contains_rect(r), "{name} off the window: {r:?}");
    }
    let repeat = h.get_by_label("Repeat: off").rect();
    let more = h.get_by_label("More").rect();
    assert!(repeat.right() <= more.left(), "{repeat:?} under {more:?}");
    drop(h);

    // the narrowest window
    let mut h = window(
        &home.join("narrow"),
        files.clone(),
        egui::vec2(420.0, 640.0),
    );
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 1.0);
    shoot(&mut h, "narrow");
    // nothing lies over anything in the row of buttons
    let repeat = h.get_by_label("Repeat: off").rect();
    let more = h.get_by_label("More").rect();
    let volume = h.get_by_label("Volume").rect();
    assert!(repeat.right() <= more.left(), "{repeat:?} under {more:?}");
    assert!(more.right() <= volume.left(), "{more:?} under {volume:?}");
    // the playlist is there to be seen and used, in a window this small
    let row = h.get_by_label("Second track").rect();
    assert!(
        row.bottom() <= 640.0 - 20.0 && row.top() > 0.0,
        "the playlist is off the window: {row:?}"
    );
    drop(h);

    // no sound device
    NO_OUTPUT.store(true, Ordering::Relaxed);
    let mut h = window(&home.join("silent"), files, egui::vec2(620.0, 860.0));
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 4);
    assert!(h.query_by_label("Retry").is_some());
    // said one way wherever it is said
    let said: Vec<String> = h
        .query_all_by_label_contains("No sound")
        .map(|n| {
            let node = n.accesskit_node();
            let label = node.label().or_else(|| node.value()).unwrap_or_default();
            label[label.find("No sound").unwrap()..].to_owned()
        })
        .collect();
    assert!(said.len() >= 2, "{said:?}");
    assert!(
        said.iter().all(|s| *s == said[0]),
        "worded differently: {said:?}"
    );
    shoot(&mut h, "no-output");
    NO_OUTPUT.store(false, Ordering::Relaxed);
}

/// The empty window's drop zone: its words, its three buttons inside the
/// window, none over another.
fn empty_state_says_what_to_do(h: &Harness<'static, App>, size: egui::Vec2) {
    assert!(h.query_by_label("Drop music here").is_some());
    // (inside the drop zone's dashed edge: above the status line, within
    // the zone's margin)
    let window = egui::Rect::from_min_max(
        Pos2::new(12.0, 0.0),
        Pos2::new(size.x - 12.0, size.y - 26.0 - 12.0),
    );
    let rects: Vec<egui::Rect> = ["Open files…", "Open folder…", "Add a URL…"]
        .iter()
        .map(|name| {
            let r = h.get_by_label_contains(name).rect();
            assert!(window.contains_rect(r), "{name} off the window: {r:?}");
            r
        })
        .collect();
    for (i, a) in rects.iter().enumerate() {
        for b in &rects[i + 1..] {
            assert!(!a.shrink(1.0).intersects(*b), "{a:?} over {b:?}");
        }
    }
    if size.y >= 700.0 {
        assert!(h.query_by_label_contains("play / pause").is_some());
    }
}

/// Colourful pixels down a band's rail, above and below its 0 dB line
/// (the middle of the slider), the handle's place left out.
fn colour_above_and_below(shot: &image::RgbaImage, band: egui::Rect) -> (usize, usize) {
    let x = band.center().x as u32;
    let zero = band.center().y;
    let (mut above, mut below) = (0, 0);
    for y in (band.top() + 12.0) as u32..(band.bottom() - 12.0) as u32 {
        if (y as f32 - zero).abs() < 14.0 {
            continue;
        }
        let p = shot.get_pixel(x, y);
        let (max, min) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        if max > 60 && (max - min) as f32 > max as f32 * 0.4 {
            if (y as f32) < zero {
                above += 1;
            } else {
                below += 1;
            }
        }
    }
    (above, below)
}
