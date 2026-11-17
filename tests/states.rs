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
    assert!(h.query_by_label_contains("Files").is_some());
    shoot(&mut h, "empty");
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
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Visualization")
        .click();
    settle(&mut h, 2);
    for name in [
        "Spectrum",
        "Loudness (EBU R128)",
        "Kinetic typography",
        "Off",
    ] {
        assert!(h.query_by_label(name).is_some(), "{name} not in the menu");
    }
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
