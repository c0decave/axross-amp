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
        desktop_bus: None,
        headless_audio: true,
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
    harness.key_press_modifiers(modifiers, key);
    harness.run_steps(1);
}

/// The sound card: what the window plays, taken at a device's pace for
/// `secs` (10 ms at a time, a frame of the window between); returns the
/// left channel heard.
pub fn listen(
    harness: &mut Harness<'_, App>,
    output: &mut axross_amp::engine::RenderState,
    secs: f32,
) -> Vec<f32> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
    let mut buf = vec![0.0f32; 480 * 2];
    let mut heard = Vec::new();
    while std::time::Instant::now() < end {
        harness.state().engine().shared.render(&mut buf, 2, output);
        heard.extend(buf.chunks(2).map(|f| f[0]));
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    heard
}

/// Runs of silence (8 frames or more) between the first and the last sound.
pub fn gaps(heard: &[f32]) -> Vec<(usize, usize)> {
    let first = heard.iter().position(|s| *s != 0.0).unwrap_or(0);
    let last = heard.iter().rposition(|s| *s != 0.0).unwrap_or(0);
    let mut out = Vec::new();
    let mut run = 0;
    for (i, s) in heard[first..=last].iter().enumerate() {
        if *s == 0.0 {
            run += 1;
        } else {
            if run >= 8 {
                out.push((first + i - run, run));
            }
            run = 0;
        }
    }
    out
}
