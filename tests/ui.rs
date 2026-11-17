//! The window, driven and rendered without a display (egui_kittest, wgpu):
//! it starts with a file from the command line, plays it for real (the
//! test takes the sound at a sound card's pace, nothing is pretended),
//! shows every visualization from that sound, checks what each face
//! draws, and saves pictures of itself into `target/screenshots/`.

#[path = "support/player.rs"]
mod player;
use player::Scratch;

use axross_amp::app::App;
use axross_amp::engine::RenderState;
use axross_amp::viz::{FACES, Group};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

/// `secs` of playing, the sound taken at a device's pace by the test.
fn play_for(h: &mut Harness<'_, App>, output: &mut RenderState, secs: f32) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
    let mut buf = vec![0.0f32; 480 * 2];
    while std::time::Instant::now() < end {
        h.state().engine().shared.render(&mut buf, 2, output);
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What a face's picture is made of: how much of it is drawn, how many
/// colours, how much changed from the frame before.
struct Picture {
    coverage: f32,
    colours: usize,
    motion: f32,
}

fn far(a: &image::Rgba<u8>, b: &image::Rgba<u8>) -> bool {
    (0..3)
        .map(|c| (a[c] as i32 - b[c] as i32).abs())
        .sum::<i32>()
        > 24
}

fn picture(
    image: &image::RgbaImage,
    rect: egui::Rect,
    before: &image::RgbaImage,
    background: image::Rgba<u8>,
) -> Picture {
    let x0 = rect.left().max(0.0) as u32 + 2;
    let y0 = rect.top().max(0.0) as u32 + 2;
    let x1 = (rect.right() as u32).min(image.width()) - 2;
    let y1 = (rect.bottom() as u32).min(image.height()) - 2;
    let (mut drawn, mut moved) = (0usize, 0usize);
    let mut colours = std::collections::HashSet::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let p = image.get_pixel(x, y);
            if far(p, &background) {
                drawn += 1;
                colours.insert([p[0] / 32, p[1] / 32, p[2] / 32]);
            }
            if far(p, before.get_pixel(x, y)) {
                moved += 1;
            }
        }
    }
    let total = ((x1 - x0) * (y1 - y0)) as f32;
    Picture {
        coverage: drawn as f32 / total,
        colours: colours.len(),
        motion: moved as f32 / total,
    }
}

#[test]
fn the_window_shows_the_track_and_every_visualization() {
    let home = Scratch::new("ui");
    let sound = home.join("Chord and noise.wav");
    player::write_chord(&sound, 40);
    let setup = axross_amp::app::Setup {
        config_dir: home.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    let arg = sound.to_string_lossy().into_owned();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, vec![arg.clone()], setup.clone()));
    let mut output = RenderState::default();
    play_for(&mut harness, &mut output, 1.5);
    assert!(harness.state().playing(), "plays");
    let image = harness.render().expect("renders");
    image.save(shots().join("main.png")).unwrap();

    // the visual's own background: the "off" face draws nothing on it
    harness.state_mut().set_mode("off");
    play_for(&mut harness, &mut output, 0.2);
    let rect = harness.get_by_role(egui::accesskit::Role::Image).rect();
    let blank = harness.render().unwrap();
    let background = *blank.get_pixel(rect.center().x as u32, rect.center().y as u32);

    let mut report = Vec::new();
    for key in FACES
        .iter()
        .filter(|f| f.group != Group::Off)
        .map(|f| f.key)
    {
        harness.state_mut().set_mode(key);
        play_for(&mut harness, &mut output, 0.35);
        assert_eq!(harness.state().mode(), key);
        let first = harness.render().unwrap();
        play_for(&mut harness, &mut output, 0.25);
        let second = harness.render().unwrap();
        let rect = harness.get_by_role(egui::accesskit::Role::Image).rect();
        let p = picture(&second, rect, &first, background);
        report.push(format!(
            "{key:<13} coverage {:>5.1} %, {:>3} colours, motion {:>5.1} %",
            p.coverage * 100.0,
            p.colours,
            p.motion * 100.0
        ));
        second.save(shots().join(format!("viz-{key}.png"))).unwrap();
        // each face draws something of its own, in more than one colour,
        // and moves with the sound
        assert!(
            (0.001..=1.0).contains(&p.coverage),
            "{key}: coverage {}",
            p.coverage
        );
        assert!(p.colours >= 2, "{key}: {} colours", p.colours);
        assert!(p.motion > 0.0005, "{key}: does not move ({})", p.motion);
    }
    println!("{}", report.join("\n"));
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
    let dir = Scratch::new("hero");
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
    let mut state = RenderState::default();
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
