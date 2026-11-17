//! What a frame costs on the CPU (layout, painting, tessellation - not the
//! GPU) for every visualization at its worst: 1920x1080, 128 bars, loud
//! noise. Each must stay well under a frame at 60 fps (16.7 ms).

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::app::App;
use axross_amp::viz::MODES;
use egui_kittest::Harness;
use std::sync::atomic::Ordering;
use std::time::Instant;

#[test]
fn every_face_paints_a_full_hd_frame_in_a_few_milliseconds() {
    let home = scratch::Scratch::new("cost");
    std::fs::create_dir_all(home.join(".config/axross")).unwrap();
    std::fs::write(
        home.join(".config/axross/media_player.json"),
        r#"{"visual_options": {"bars": 128, "fps": 60}}"#,
    )
    .unwrap();
    // SAFETY: the only test of this binary
    unsafe { std::env::set_var("HOME", &home) };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1920.0, 1080.0))
        .build_eframe(|cc| App::new(cc, vec![]));
    harness.run_steps(3);
    let mut seed = 1u32;
    let mut report = Vec::new();
    for (key, _) in MODES {
        harness.state_mut().set_mode(key);
        let mut worst = 0.0f64;
        let mut total = 0.0;
        for frame in 0..40 {
            {
                let shared = &harness.state().engine().shared;
                let mut tap = Vec::with_capacity(8192 * 2);
                for _ in 0..8192 * 2 {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    tap.push((seed >> 8) as f32 / (1u32 << 24) as f32 * 1.6 - 0.8);
                }
                shared.tap.write(&tap);
                shared.playing.store(true, Ordering::Relaxed);
            }
            let started = Instant::now();
            harness.step();
            let took = started.elapsed().as_secs_f64() * 1000.0;
            if frame >= 5 {
                worst = worst.max(took);
                total += took;
            }
        }
        report.push(format!(
            "{key:<10} mean {:>5.2} ms, worst {:>5.2} ms",
            total / 35.0,
            worst
        ));
        assert!(total / 35.0 < 12.0, "{key}: {:.2} ms a frame", total / 35.0);
    }
    println!("{}", report.join("\n"));
}
