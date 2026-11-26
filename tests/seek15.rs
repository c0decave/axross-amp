//! Winding 15 seconds back and on: the two buttons beside play, the keys J
//! and L, from 0:00 at the least, past the end on to the next track (or,
//! the last, stopped), paused staying paused, the time shown at once, and
//! disabled with nothing to wind. A picture of the row of buttons.

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::engine::RenderState;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use player::{Scratch, settle, until, write_chord};
use std::path::{Path, PathBuf};

fn window(dir: &Path, args: Vec<String>) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

/// `secs` of sound played, taken as a device takes it: 10 ms at a time, a
/// frame of the window between - and by the sound, not by the clock: the
/// next 10 ms once the decoder has them. (By the clock a busy machine took
/// little sound in "half a second": the track was at 0.3 s where the test
/// reckoned with 1 s, and "Back 15 s" there is no seek to speak of - the
/// test failed two times in eight under a load of 25: review 19, L6.)
fn play(h: &mut Harness<'static, App>, out: &mut RenderState, secs: f32) {
    let started = std::time::Instant::now();
    let end = started + std::time::Duration::from_secs_f32(secs * 20.0 + 10.0);
    let mut buf = vec![0.0f32; 480 * 2];
    let mut taken = 0.0;
    while taken < secs && std::time::Instant::now() < end {
        let shared = &h.state().engine().shared;
        let playing = shared.playing.load(std::sync::atomic::Ordering::Relaxed);
        if playing && !shared.ready(480) {
            // (the decoder's turn; a seek under way)
            std::thread::sleep(std::time::Duration::from_millis(1));
            // (a player that plays nothing more - the end of the list -
            // still gets its frames, at the clock's pace)
            if started.elapsed().as_secs_f32() < secs + 0.2 {
                continue;
            }
        }
        h.state().engine().shared.render(&mut buf, 2, out);
        h.run_steps(1);
        taken += 0.01;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn heard(h: &Harness<'static, App>) -> f64 {
    h.state().engine().shared.position()
}

fn current(h: &Harness<'static, App>) -> i64 {
    h.state().snapshot().current
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

fn disabled(h: &Harness<'static, App>, name: &str) -> bool {
    h.get_by_label(name).accesskit_node().is_disabled()
}

#[test]
fn the_buttons_wind_15_seconds_and_show_it_at_once() {
    let home = Scratch::new("seek15-buttons");
    let mut h = window(&home, two_tracks(&home));
    let mut out = RenderState::default();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 1.0);
    assert!(h.state().playing());
    assert!(!disabled(&h, "Forward 15 s") && !disabled(&h, "Back 15 s"));
    let before = heard(&h);
    h.get_by_label("Forward 15 s").click();
    h.run_steps(1);
    // shown at once (the time and the slider), heard soon after
    let shown = h.state_mut().shown_position();
    assert!(
        (shown - (before + 15.0)).abs() < 0.6,
        "shown {shown} after {before}"
    );
    play(&mut h, &mut out, 0.5);
    let now = heard(&h);
    assert!((now - (before + 15.0)).abs() < 1.2, "{before} -> {now}");
    assert!(h.state().playing(), "winding stopped the sound");
    // twice quickly: 30 s on from where it was shown
    let from = h.state_mut().shown_position();
    h.get_by_label("Forward 15 s").click();
    h.run_steps(1);
    h.get_by_label("Forward 15 s").click();
    h.run_steps(1);
    let shown = h.state_mut().shown_position();
    // (40 s long: from about 16 s, 30 on is past its end - the next track)
    assert!(
        shown < from || current(&h) == 1,
        "two steps from {from}: shown {shown}, track {}",
        current(&h)
    );
    // back
    let home2 = Scratch::new("seek15-back");
    let mut h = window(&home2, two_tracks(&home2));
    let mut out = RenderState::default();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 0.5);
    h.get_by_label("Forward 15 s").click();
    play(&mut h, &mut out, 0.5);
    let before = heard(&h);
    h.get_by_label("Back 15 s").click();
    play(&mut h, &mut out, 0.5);
    let now = heard(&h);
    assert!((now - (before - 15.0)).abs() < 1.2, "{before} -> {now}");
    // and from a second in: to 0:00, no further
    play(&mut h, &mut out, 0.5);
    let now = heard(&h);
    assert!((0.7..3.0).contains(&now), "a second in, not {now}");
    h.get_by_label("Back 15 s").click();
    h.run_steps(1);
    assert!(h.state_mut().shown_position() < 0.2);
    play(&mut h, &mut out, 0.3);
    assert!(heard(&h) < 1.0, "not at the start: {}", heard(&h));
    assert_eq!(current(&h), 0, "back past the start left the track");
}

#[test]
fn on_past_the_end_goes_to_the_next_track_or_stops_at_the_last() {
    let home = Scratch::new("seek15-end");
    let mut h = window(&home, two_tracks(&home));
    let mut out = RenderState::default();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 0.3);
    // 15 s from the end of the first (40 s): to 30 s, then on past its end
    for _ in 0..2 {
        h.get_by_label("Forward 15 s").click();
        play(&mut h, &mut out, 0.3);
    }
    assert_eq!(current(&h), 0);
    h.get_by_label("Forward 15 s").click();
    assert!(until(&mut h, 5.0, |a| a.snapshot().current == 1 && a.is_loaded()));
    play(&mut h, &mut out, 0.5);
    assert!(h.state().playing(), "the next track does not play");
    assert!(heard(&h) < 2.0, "the next track not from its start");
    // the last (20 s): on past its end - stopped, as at its end
    h.get_by_label("Forward 15 s").click();
    play(&mut h, &mut out, 0.3);
    h.get_by_label("Forward 15 s").click();
    settle(&mut h, 4);
    assert!(
        !h.state().playing(),
        "still playing past the last track's end"
    );
    assert!(
        h.state().status().contains("End of the playlist"),
        "{}",
        h.state().status()
    );
}

#[test]
fn paused_it_stays_paused_and_the_keys_wind_too() {
    let home = Scratch::new("seek15-paused");
    let mut h = window(&home, two_tracks(&home));
    let mut out = RenderState::default();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 0.5);
    h.key_press(egui::Key::Space);
    play(&mut h, &mut out, 0.3);
    assert!(!h.state().playing());
    let before = heard(&h);
    h.get_by_label("Forward 15 s").click();
    play(&mut h, &mut out, 0.5);
    assert!(!h.state().playing(), "winding started the sound");
    let now = heard(&h);
    assert!(
        (now - (before + 15.0)).abs() < 0.3,
        "paused: {before} -> {now}"
    );
    // L on, J back
    h.key_press(egui::Key::L);
    play(&mut h, &mut out, 0.3);
    assert!((heard(&h) - (now + 15.0)).abs() < 0.3, "L: {}", heard(&h));
    h.key_press(egui::Key::J);
    h.key_press(egui::Key::J);
    play(&mut h, &mut out, 0.3);
    assert!(
        (heard(&h) - (now - 15.0)).abs() < 0.3,
        "J twice: {}",
        heard(&h)
    );
    assert!(!h.state().playing());
    // the tooltips say the keys
    let tip = |h: &mut Harness<'static, App>, name: &str| {
        let at = h.get_by_label(name).rect().center();
        h.hover_at(at);
        settle(h, 30);
    };
    tip(&mut h, "Forward 15 s");
    assert!(
        h.query_by_label_contains("(L)").is_some(),
        "no key in the tooltip"
    );
}

#[test]
fn nothing_loaded_nothing_to_wind() {
    // nothing at all: no buttons, and L winds nothing
    let home = Scratch::new("seek15-empty");
    let mut h = window(&home, vec![]);
    settle(&mut h, 3);
    assert!(h.query_by_label("Forward 15 s").is_none());
    h.key_press(egui::Key::L);
    settle(&mut h, 2);
    assert!(!h.state().playing());
    // a track that cannot be opened: the buttons there, disabled
    let bad = home.join("Broken.wav");
    std::fs::write(&bad, b"RIFF....WAVEnot a wave at all").unwrap();
    let mut h = window(&home.join("b"), vec![bad.to_string_lossy().into_owned()]);
    settle(&mut h, 20);
    assert!(!h.state().is_loaded());
    assert!(
        disabled(&h, "Back 15 s"),
        "Back 15 s enabled with nothing loaded"
    );
    assert!(
        disabled(&h, "Forward 15 s"),
        "Forward 15 s enabled with nothing loaded"
    );
    let before = h.state().engine().shared.position();
    h.key_press(egui::Key::L);
    settle(&mut h, 2);
    assert_eq!(h.state().engine().shared.position(), before);
}

#[test]
fn a_picture_of_the_row_of_buttons() {
    let home = Scratch::new("seek15-picture");
    let mut h = window(&home, two_tracks(&home));
    let mut out = RenderState::default();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    play(&mut h, &mut out, 1.0);
    h.event(egui::Event::PointerGone);
    h.run_steps(2);
    let shot = h.render().unwrap();
    let back = h.get_by_label("Back 15 s").rect();
    let (top, bottom) = (back.top() - 10.0, back.bottom() + 10.0);
    let row = image::imageops::crop_imm(
        &shot,
        0,
        top.max(0.0) as u32,
        shot.width(),
        (bottom - top) as u32,
    )
    .to_image();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    row.save(dir.join("transport.png")).unwrap();
    // (the README's picture of it)
    if std::env::var_os("AMP_README_PICTURES").is_some() {
        row.save(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/transport.png"))
            .unwrap();
    }
    // in a narrow window the row still fits, nothing over anything
    let home = Scratch::new("seek15-narrow");
    let setup = Setup {
        config_dir: home.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    let args = two_tracks(&home);
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, 640.0))
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()));
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 3);
    let order = [
        "Previous",
        "Back 15 s",
        "Pause",
        "Stop",
        "Forward 15 s",
        "Next",
        "Shuffle: off",
        "Repeat: off",
        "More",
        "Volume",
    ];
    let rects: Vec<egui::Rect> = order
        .iter()
        .map(|n| {
            h.query_by_label(n)
                .unwrap_or_else(|| h.get_by_label(if *n == "Pause" { "Play" } else { n }))
                .rect()
        })
        .collect();
    for (k, pair) in rects.windows(2).enumerate() {
        assert!(
            pair[0].right() <= pair[1].left() + 0.5,
            "{} lies over {}",
            order[k],
            order[k + 1]
        );
    }
    assert!(
        rects.last().unwrap().right() <= 420.0,
        "the row runs off the window"
    );
}

/// Paused, L past the end goes on to the next track - and stays paused.
#[test]
fn paused_past_the_end_stays_paused() {
    let home = Scratch::new("seek15-paused-end");
    let mut h = window(&home, two_tracks(&home));
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let mut out = RenderState::default();
    play(&mut h, &mut out, 0.3);
    h.state_mut().wind(30.0);
    play(&mut h, &mut out, 0.3);
    h.key_press(egui::Key::Space);
    play(&mut h, &mut out, 0.3);
    assert!(!h.state().playing());
    h.key_press(egui::Key::L);
    play(&mut h, &mut out, 1.0);
    assert!(until(&mut h, 3.0, |a| a.snapshot().current == 1));
    play(&mut h, &mut out, 0.5);
    assert!(
        !h.state().playing(),
        "L past the end started it while paused"
    );
}
