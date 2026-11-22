//! The window in a harness of its own: its state in a fresh folder (tests
//! run side by side, nothing of the user's is read or written), tracks made
//! as WAV files, and steps that let the engine's thread answer.
#![allow(dead_code)]

use axross_amp::app::{App, Setup};
use egui_kittest::Harness;
use std::io::Write;
use std::path::Path;

#[path = "scratch.rs"]
mod scratch;
pub use scratch::Scratch;

/// A folder for one test (emptied first, removed when the test is done).
pub fn folder(name: &str) -> Scratch {
    Scratch::new(name)
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
        // (by the sound, as a device's buffer would: the next 10 ms taken
        // once the decoder has them - on a busy machine a decoder not given
        // its turn ran the output dry, and the test heard less than it
        // played: a gap, a radio 3 s late - review 13)
        let shared = &harness.state().engine().shared;
        let wait = std::time::Instant::now() + std::time::Duration::from_millis(200);
        while shared.playing.load(std::sync::atomic::Ordering::Relaxed)
            && !shared.ready(480)
            && std::time::Instant::now() < wait
        {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
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

/// A chord over the whole range with a little noise and a beat, in
/// stereo (every face has something to show), at 48 kHz.
pub fn write_chord(path: &Path, secs: u32) {
    let rate = 48_000u32;
    let frames = rate * secs;
    let mut f = std::fs::File::create(path).unwrap();
    let data = frames * 4;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 4).to_le_bytes(),
        &4u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        f.write_all(v).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&data.to_le_bytes()).unwrap();
    let mut seed = 7u32;
    let mut out = Vec::with_capacity(data as usize);
    for i in 0..frames {
        let t = i as f32 / rate as f32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
        let beat = (-(t * 2.0).fract() * 8.0).exp();
        let base: f32 = [55.0, 110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0, 7040.0]
            .iter()
            .enumerate()
            .map(|(k, hz)| (t * hz * std::f32::consts::TAU).sin() * 0.22 / (k as f32 + 1.0).sqrt())
            .sum();
        let l = base * (0.6 + 0.4 * beat) + noise * 0.04;
        let r = base * 0.8 + (t * 330.0 * std::f32::consts::TAU).cos() * 0.15 + noise * 0.04;
        out.extend_from_slice(&((l.clamp(-1.0, 1.0) * 30_000.0) as i16).to_le_bytes());
        out.extend_from_slice(&((r.clamp(-1.0, 1.0) * 30_000.0) as i16).to_le_bytes());
    }
    f.write_all(&out).unwrap();
}
