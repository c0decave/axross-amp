//! A font that panics while a text is laid out does not end the program
//! (review 20, M1): every file is gone through before egui is given it
//! (tests/fonts.rs), and for what no check foresees the frame's panic is
//! caught, the system's fonts are let go - those found for other scripts
//! first, then the faces of the start - and the next frame is drawn with
//! what is left.
//!
//! Here the going-through is switched off (`fonts::UNDRAWN`), and a font
//! whose `glyf` is zeroed - "index out of bounds" in the font's reader at
//! its first letter - is named in `AXROSS_AMP_FONTS` and then given as the
//! bold face. One test: both switches are the whole program's.

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::app::{App, Setup};
use egui_kittest::Harness;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// The tests' font with its `glyf` table zeroed.
fn damaged() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/media/fonts/amp-fixture.ttf");
    let mut data = std::fs::read(path).unwrap();
    let n = u16::from_be_bytes([data[4], data[5]]) as usize;
    for i in 0..n {
        let r = data[12 + 16 * i..28 + 16 * i].to_vec();
        if &r[0..4] == b"glyf" {
            let at = u32::from_be_bytes([r[8], r[9], r[10], r[11]]) as usize;
            let len = u32::from_be_bytes([r[12], r[13], r[14], r[15]]) as usize;
            data[at..at + len].fill(0);
        }
    }
    data
}

/// Whether this machine has a bold face at all.
fn bold_here() -> bool {
    axross_amp::fonts::BOLD_FONTS
        .iter()
        .any(|p| Path::new(p).exists())
}

/// A second of a tone at `path`.
fn tone(path: &Path) {
    let mut wav = Vec::new();
    let n = 48_000u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + n * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&96_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 8000.0) as i16;
        wav.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, wav).unwrap();
}

/// The player with `track`, run until it is loaded and `frames` more.
fn player(dir: &Path, track: &Path, frames: usize) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    let args = vec![track.to_string_lossy().into_owned()];
    let mut h = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .allow_missing_glyphs()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !h.state().is_loaded() {
        h.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(std::time::Instant::now() < end, "never loaded");
    }
    for _ in 0..frames {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h
}

#[test]
fn a_font_that_panics_in_a_frame_is_let_go_and_the_player_goes_on() {
    axross_amp::fonts::UNDRAWN.store(true, Ordering::Relaxed);
    let dir = scratch::Scratch::new("fonts-panic");
    let font = dir.join("damaged.ttf");
    std::fs::write(&font, damaged()).unwrap();
    // -- named in AXROSS_AMP_FONTS, a title in its letters
    // SAFETY: the one test of this file
    unsafe { std::env::set_var("AXROSS_AMP_FONTS", &font) };
    let track = dir.join("日本語の音楽 אבגד.wav");
    tone(&track);
    let mut h = player(&dir.join("a"), &track, 60);
    assert_eq!(h.state().fonts_let_go(), 1, "let go once");
    // (the faces of the start are not blamed for it)
    assert!(axross_amp::fonts::Faces::of_the_system().bold.is_some() || !bold_here());
    // (and on: the title in boxes, the window whole)
    h.run_steps(10);
    assert!(h.state().is_loaded());
    drop(h);
    // -- as the bold face of the start: every title is written with it
    unsafe { std::env::remove_var("AXROSS_AMP_FONTS") };
    *axross_amp::fonts::GIVEN_FACES.lock().unwrap() = Some(axross_amp::fonts::Faces {
        bold: Some(Arc::new(damaged())),
        serif: None,
        mono_bold: None,
    });
    let plain = dir.join("Afterglow.wav");
    tone(&plain);
    let mut h = player(&dir.join("b"), &plain, 30);
    assert_eq!(h.state().fonts_let_go(), 1, "let go once");
    assert!(
        axross_amp::fonts::Faces::of_the_system().bold.is_none(),
        "the bold face is still in use"
    );
    h.run_steps(10);
    // (the window is drawn whole again: its title is there)
    use egui_kittest::kittest::Queryable as _;
    assert!(h.query_all_by_label_contains("Afterglow").next().is_some());
}
