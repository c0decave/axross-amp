//! The window's themes: the picker shows a theme while the pointer is on it
//! and keeps the one clicked (in axross-amp's own settings file); every
//! theme draws the window with its words and buttons where they were (the
//! same accessible names). With `--ignored`, every theme in the window's
//! main states - nothing to play, playing with the playlist, the faces'
//! menu, a track's details, no sound device, the Mixer and Bluetooth tabs
//! (against axross's fakes when they are given) - each kept in
//! `target/screenshots/themes/`, and a contact sheet of them all.

#[path = "support/player.rs"]
mod player;

#[path = "support/bluez.rs"]
mod bluez;

use axross_amp::app::{App, Setup, Tab};
use axross_amp::engine::{NO_OUTPUT, RenderState};
use axross_amp::theme::THEMES;
use egui::{Event, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use player::{Scratch, settle, until, write_chord};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

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

#[test]
fn the_picker_shows_a_theme_at_once_and_keeps_the_one_chosen() {
    let home = Scratch::new("theme-picker");
    let mut h = window(&home, vec![], egui::vec2(620.0, 860.0));
    settle(&mut h, 3);
    assert_eq!(h.state().theme_shown(), "midnight", "Midnight by default");
    // the faces' menu, its Theme submenu
    h.get_by_label("⚙").click();
    settle(&mut h, 2);
    let theme = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Theme")
        .rect();
    hover(&mut h, theme.center());
    for t in &THEMES {
        assert!(
            h.query_by_role_and_label(egui::accesskit::Role::RadioButton, t.name)
                .is_some(),
            "{} not offered",
            t.name
        );
    }
    // the pointer on Paper: shown at once
    let radio = |h: &Harness<'static, App>, name: &str| {
        h.get_by_role_and_label(egui::accesskit::Role::RadioButton, name)
            .rect()
    };
    let paper = radio(&h, "Paper");
    hover(&mut h, paper.center());
    assert_eq!(h.state().theme_shown(), "paper", "no preview");
    // off it again (on the submenu's title): back to the chosen one
    hover(&mut h, theme.center());
    assert_eq!(h.state().theme_shown(), "midnight", "the preview stayed");
    // a click keeps one
    let sakura = radio(&h, "Sakura").center();
    hover(&mut h, sakura);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: sakura,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(&mut h, 2);
    h.event(Event::PointerGone);
    settle(&mut h, 3);
    assert_eq!(h.state().theme_shown(), "sakura");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 3);
    assert_eq!(h.state().theme_shown(), "sakura");
    // in axross-amp's own file (not axross's player state)
    let own = home.join("config/axross-amp.json");
    let mut kept = false;
    for _ in 0..200 {
        settle(&mut h, 1);
        if std::fs::read_to_string(&own).is_ok_and(|s| s.contains("\"theme\": \"sakura\"")) {
            kept = true;
            break;
        }
    }
    assert!(kept, "not written to {own:?}");
    let shared = std::fs::read_to_string(home.join("config/media_player.json")).unwrap_or_default();
    assert!(!shared.contains("sakura"), "the theme in axross's file");
    drop(h);
    // and shown again when the window opens
    let mut h = window(&home, vec![], egui::vec2(620.0, 860.0));
    settle(&mut h, 3);
    assert_eq!(h.state().theme_shown(), "sakura");
}

#[test]
fn every_theme_keeps_the_windows_words_and_buttons() {
    let home = Scratch::new("theme-names");
    let a = home.join("Chord.wav");
    write_chord(&a, 6);
    let mut h = window(
        &home,
        vec![a.to_string_lossy().into_owned()],
        egui::vec2(620.0, 860.0),
    );
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let names = [
        "Files",
        "Folder",
        "URL",
        "Player",
        "Mixer",
        "Bluetooth",
        "Volume",
        "Repeat: off",
        "Chord",
    ];
    for t in &THEMES {
        h.state_mut().set_theme(t.key);
        settle(&mut h, 2);
        assert_eq!(h.state().theme_shown(), t.key);
        for name in names {
            assert!(
                h.query_all_by_label(name).next().is_some(),
                "{}: {name} is gone",
                t.name
            );
        }
        // the theme's colours on the window: its ground at a corner
        let shot = h.render().unwrap();
        let p = shot.get_pixel(shot.width() - 3, 3);
        let g = t.ground;
        let near = |x: u8, y: u8| (x as i32 - y as i32).abs() <= 3;
        assert!(
            near(p[0], g.r()) && near(p[1], g.g()) && near(p[2], g.b()),
            "{}: the ground is {p:?}, not {g:?}",
            t.name
        );
    }
}

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/themes");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn shoot(h: &mut Harness<'static, App>, name: &str) -> image::RgbaImage {
    h.event(Event::PointerGone);
    h.run_steps(2);
    let img = h.render().expect("renders");
    img.save(shots().join(format!("{name}.png"))).unwrap();
    img
}

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

/// The fake sound server and BlueZ, when axross's fakes are given: the
/// Mixer and Bluetooth tabs have something to show.
fn fakes(dir: &Path) -> Option<bluez::Bus> {
    let pactl = std::env::var_os("AMP_FAKE_PACTL").map(PathBuf::from)?;
    let mut bus = bluez::Bus::start()?;
    let code = format!(
        "import sys; sys.path.insert(0, {:?}); import fake_pactl; from pathlib import Path; fake_pactl.install(Path({:?}))",
        pactl.parent().unwrap(),
        dir
    );
    assert!(
        std::process::Command::new(&bus.python)
            .args(["-c", &code])
            .status()
            .unwrap()
            .success()
    );
    // SAFETY: set before any window of this test is made
    unsafe {
        std::env::set_var("AXROSS_AMP_PACTL", dir.join("bin/pactl"));
        std::env::set_var("FAKE_PACTL_STATE", dir.join("pactl-state.json"));
        std::env::set_var("FAKE_PACTL_LOG", dir.join("pactl-calls.jsonl"));
        std::env::set_var("AXROSS_AMP_BLUEZ_BUS", &bus.address);
    }
    use serde_json::json;
    bus.command(
        "add_adapter",
        json!({"name": "hci0", "props": {"Alias": "Laptop", "Powered": true}}),
    );
    bus.command(
        "add_device",
        json!({"address": "AA:BB:CC:DD:EE:01", "battery": 70,
               "props": {"Name": "WH-1000XM4", "Alias": "WH-1000XM4", "Icon": "audio-headphones",
                         "Paired": true, "Connected": true, "RSSI": -48}}),
    );
    bus.command(
        "add_device",
        json!({"address": "22:00:00:00:00:01",
               "props": {"Name": "Keyboard K380", "Class": 0x000540, "Paired": true}}),
    );
    bus.command(
        "add_device",
        json!({"address": "22:00:00:00:00:02",
               "props": {"Name": "Kitchen Speaker", "Icon": "audio-card", "RSSI": -71}}),
    );
    Some(bus)
}

/// The states shot of every theme, in this order (the sheet's columns).
const STATES: [&str; 7] = [
    "empty",
    "playing",
    "menu",
    "details",
    "no-output",
    "mixer",
    "bluetooth",
];

#[test]
#[ignore]
fn every_theme_in_every_state() {
    let home = Scratch::new("themes");
    let bus = fakes(&home);
    if bus.is_none() {
        eprintln!("no fakes (AMP_FAKE_PACTL, AMP_FAKE_BLUEZ): the Mixer and Bluetooth tabs empty");
    }
    let a = home.join("Chord and noise.wav");
    write_chord(&a, 40);
    let b = home.join("Second track.wav");
    write_chord(&b, 12);
    let files = vec![
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    ];
    let size = egui::vec2(620.0, 860.0);
    let mut sheet: Vec<Vec<image::RgbaImage>> = Vec::new();
    for t in &THEMES {
        let mut row = Vec::new();
        let dir = home.join(t.key);
        // nothing to play
        let mut h = window(&dir.join("empty"), vec![], size);
        h.state_mut().set_theme(t.key);
        settle(&mut h, 3);
        row.push(shoot(&mut h, &format!("{}-empty", t.key)));
        drop(h);
        // playing, the playlist below; the faces' menu; a track's details
        let mut h = window(&dir.join("full"), files.clone(), size);
        h.state_mut().set_theme(t.key);
        let mut out = RenderState::default();
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        play(&mut h, &mut out, 1.5);
        row.push(shoot(&mut h, &format!("{}-playing", t.key)));
        let picture = h.get_by_role(egui::accesskit::Role::Image).rect();
        right_click(&mut h, picture.center());
        let theme = h
            .get_by_role_and_label(egui::accesskit::Role::Button, "Theme")
            .rect();
        hover(&mut h, theme.center());
        h.run_steps(2);
        let img = h.render().unwrap();
        img.save(shots().join(format!("{}-menu.png", t.key)))
            .unwrap();
        row.push(img);
        h.key_press(egui::Key::Escape);
        h.key_press(egui::Key::Escape);
        settle(&mut h, 2);
        let track = h.get_by_label("Second track").rect();
        right_click(&mut h, track.center());
        h.get_by_label("ℹ Details…").click();
        settle(&mut h, 3);
        row.push(shoot(&mut h, &format!("{}-details", t.key)));
        drop(h);
        // no sound device
        NO_OUTPUT.store(true, Ordering::Relaxed);
        let mut h = window(&dir.join("silent"), files.clone(), size);
        h.state_mut().set_theme(t.key);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        settle(&mut h, 4);
        row.push(shoot(&mut h, &format!("{}-no-output", t.key)));
        NO_OUTPUT.store(false, Ordering::Relaxed);
        drop(h);
        // the Mixer and Bluetooth tabs
        let mut h = window(&dir.join("tabs"), vec![], size);
        h.state_mut().set_theme(t.key);
        h.state_mut().set_tab(Tab::Mixer);
        for _ in 0..200 {
            settle(&mut h, 1);
            if bus.is_none()
                || h.query_all_by_label("Built-in Audio Analog Stereo")
                    .next()
                    .is_some()
            {
                break;
            }
        }
        settle(&mut h, 2);
        row.push(shoot(&mut h, &format!("{}-mixer", t.key)));
        h.state_mut().set_tab(Tab::Bluetooth);
        for _ in 0..200 {
            settle(&mut h, 1);
            if bus.is_none() || h.query_all_by_label("Kitchen Speaker").next().is_some() {
                break;
            }
        }
        settle(&mut h, 2);
        row.push(shoot(&mut h, &format!("{}-bluetooth", t.key)));
        drop(h);
        sheet.push(row);
    }
    // the contact sheet: a theme a row, a state a column, each a third
    // as large
    let (w, hgt) = (620 / 3, 860 / 3);
    let label = 0;
    let gap = 6;
    let mut page = image::RgbaImage::from_pixel(
        (STATES.len() as u32) * (w + gap) + gap,
        (THEMES.len() as u32) * (hgt + gap + label) + gap,
        image::Rgba([40, 40, 40, 255]),
    );
    for (r, row) in sheet.iter().enumerate() {
        for (c, img) in row.iter().enumerate() {
            let small = image::imageops::resize(img, w, hgt, image::imageops::FilterType::Triangle);
            image::imageops::overlay(
                &mut page,
                &small,
                (gap + c as u32 * (w + gap)) as i64,
                (gap + r as u32 * (hgt + gap + label)) as i64,
            );
        }
    }
    page.save(shots().join("sheet.png")).unwrap();
    // the README's gallery: every theme playing, half size, four a row
    let (w, hgt) = (310, 430);
    let cols = 4u32;
    let rows = THEMES.len().div_ceil(cols as usize) as u32;
    let mut gallery = image::RgbaImage::from_pixel(
        cols * (w + gap) + gap,
        rows * (hgt + gap) + gap,
        image::Rgba([13, 17, 23, 255]),
    );
    for (i, row) in sheet.iter().enumerate() {
        let small = image::imageops::resize(&row[1], w, hgt, image::imageops::FilterType::Lanczos3);
        image::imageops::overlay(
            &mut gallery,
            &small,
            (gap + (i as u32 % cols) * (w + gap)) as i64,
            (gap + (i as u32 / cols) * (hgt + gap)) as i64,
        );
    }
    gallery.save(shots().join("gallery.png")).unwrap();
    let _ = STATES;
}
