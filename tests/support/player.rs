//! The window in a harness of its own: its state in a fresh folder (tests
//! run side by side, nothing of the user's is read or written), tracks made
//! as WAV files, and steps that let the engine's thread answer.
#![allow(dead_code)]

use axross_amp::app::{App, Setup};
use egui_kittest::Harness;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A folder for one test (emptied first).
pub fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("amp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mono WAV of `secs` seconds (a quiet tone) at `rate`.
pub fn write_wav_rate(path: &Path, rate: u32, secs: f32) {
    let frames = (rate as f32 * secs) as u32;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + frames * 2).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 2).to_le_bytes(),
        &2u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        f.write_all(v).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&(frames * 2).to_le_bytes()).unwrap();
    let mut data = Vec::with_capacity(frames as usize * 2);
    for i in 0..frames {
        let v = ((i as f32 * 0.06).sin() * 8000.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    f.write_all(&data).unwrap();
}

pub fn write_wav(path: &Path, secs: f32) {
    write_wav_rate(path, 22_050, secs)
}

/// `names` as WAV files of `secs` in `dir`; their paths.
pub fn tracks(dir: &Path, names: &[&str], secs: f32) -> Vec<String> {
    names
        .iter()
        .map(|n| {
            let p = dir.join(n);
            write_wav(&p, secs);
            p.to_string_lossy().into_owned()
        })
        .collect()
}

/// The window with its state in `dir/config`, started with `args`.
pub fn harness(dir: &Path, args: Vec<String>) -> Harness<'static, App> {
    harness_sized(dir, args, egui::vec2(620.0, 860.0))
}

pub fn harness_sized(dir: &Path, args: Vec<String>, size: egui::Vec2) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
    };
    Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

/// `steps` frames, a little time between them (the engine answers).
pub fn settle(harness: &mut Harness<'_, App>, steps: usize) {
    for _ in 0..steps {
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

/// Frames until `done` holds (at most `secs`); whether it did.
pub fn until(harness: &mut Harness<'_, App>, secs: f32, done: impl Fn(&App) -> bool) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
    while std::time::Instant::now() < end {
        if done(harness.state()) {
            return true;
        }
        settle(harness, 1);
    }
    done(harness.state())
}

/// A key pressed with modifiers.
pub fn key_with(harness: &mut Harness<'_, App>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        harness.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }
    harness.run_steps(1);
}
