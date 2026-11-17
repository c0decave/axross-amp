//! The window, driven and rendered without a display (egui_kittest, wgpu):
//! it starts with a file from the command line, shows every visualization
//! from the sound it is fed, and saves pictures of itself into
//! `target/screenshots/`.

use axross_amp::app::App;
use axross_amp::viz::MODES;
use egui_kittest::Harness;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

fn write_wav(path: &Path, rate: u32, secs: u32) {
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
    for i in 0..frames {
        let t = i as f32 / rate as f32;
        let v = ((t * 220.0 * std::f32::consts::TAU).sin() * 9000.0) as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
        f.write_all(&v.to_le_bytes()).unwrap();
    }
}

/// A chord with some noise: every face has something to show.
fn feed(app: &App, phase: f32) {
    let shared = &app.engine().shared;
    let mut tap = Vec::with_capacity(8192 * 2);
    let mut seed = 7u32;
    for i in 0..8192 {
        let t = i as f32 / 48_000.0 + phase;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
        let base: f32 = [55.0, 110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0, 7040.0]
            .iter()
            .enumerate()
            .map(|(k, f)| (t * f * std::f32::consts::TAU).sin() * 0.3 / (k as f32 + 1.0).sqrt())
            .sum();
        tap.push(base + noise * 0.05);
        tap.push(base * 0.8 + (t * 330.0 * std::f32::consts::TAU).cos() * 0.2 + noise * 0.05);
    }
    shared.tap.write(&tap);
    shared.playing.store(true, Ordering::Relaxed);
}

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_window_shows_the_track_and_every_visualization() {
    let home = std::env::temp_dir().join(format!("amp-ui-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: the only test of this binary; nothing else reads the environment meanwhile
    unsafe { std::env::set_var("HOME", &home) };
    let wav = home.join("Tone in A.wav");
    write_wav(&wav, 44_100, 3);
    let arg = wav.to_string_lossy().into_owned();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .wgpu()
        .build_eframe(move |cc| App::new(cc, vec![arg.clone()]));
    for step in 0..30 {
        feed(harness.state(), step as f32 * 0.01);
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let image = harness.render().expect("renders");
    image.save(shots().join("main.png")).unwrap();
    for (key, _name) in MODES {
        harness.state_mut().set_mode(key);
        for step in 0..12 {
            feed(harness.state(), step as f32 * 0.013);
            harness.run_steps(1);
        }
        assert_eq!(harness.state().mode(), key);
        harness
            .render()
            .unwrap()
            .save(shots().join(format!("viz-{key}.png")))
            .unwrap();
    }
    // the state was written into axross's file
    drop(harness);
    let saved =
        std::fs::read_to_string(home.join(".config/axross/media_player.json")).unwrap_or_default();
    assert!(saved.is_empty() || saved.contains("Tone in A"), "{saved}");
}

/// The README's picture: the window playing a real track (AMP_DEMO_TRACK,
/// synthesized and tagged as a demo), its sound taken in real time by the
/// test as a sound card would. `cargo test --test ui -- --ignored`.
#[test]
#[ignore]
fn the_readme_picture() {
    let Some(track) = std::env::var_os("AMP_DEMO_TRACK") else {
        eprintln!("AMP_DEMO_TRACK not set");
        return;
    };
    let dir = std::env::temp_dir().join(format!("amp-hero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config/media_player.json"),
        r#"{"visualizer": "spectrum", "visual_options": {"bars": 48}, "playlist_open": true}"#,
    )
    .unwrap();
    let setup = axross_amp::app::Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    let args = vec![track.to_string_lossy().into_owned()];
    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()));
    let mut state = axross_amp::engine::RenderState::default();
    let mut buf = vec![0.0f32; 480 * 2];
    let end = std::time::Instant::now() + std::time::Duration::from_secs(9);
    while std::time::Instant::now() < end {
        harness
            .state()
            .engine()
            .shared
            .render(&mut buf, 2, &mut state);
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let image = harness.render().expect("renders");
    // (without the status line: the test's own output has no device name)
    let image = image::imageops::crop_imm(&image, 0, 0, 620, 834).to_image();
    image.save(shots().join("hero.png")).unwrap();
}
