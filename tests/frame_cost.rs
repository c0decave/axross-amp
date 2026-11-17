//! What a frame of each face costs on the CPU - the analysis, the meshes,
//! egui's tessellation; not the GPU's work - in a small window (620 x 300),
//! full HD and 4K, with the most bars (128) and loud noise, the worst case
//! (the shader faces drawn by the CPU, as where there is no wgpu). And the
//! analysis alone: its FFTs, the loudness, the beat.
//!
//! The limits hold for an unoptimised build (`cargo test`); the release
//! build is several times faster.

#[path = "support/faces.rs"]
mod faces;

use axross_amp::analyzer::{Analyzer, Needs};
use axross_amp::state::VisualOptions;
use axross_amp::viz::{FACES, Group};
use std::time::Instant;

fn noise(seconds: f32) -> Vec<f32> {
    let mut seed = 1u32;
    (0..(seconds * faces::RATE as f32) as usize * 2)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 1.6 - 0.8
        })
        .collect()
}

#[test]
fn every_face_draws_a_frame_in_a_few_milliseconds_at_any_size() {
    let sizes = [
        (620.0, 300.0, 12.0),
        (1920.0, 1080.0, 16.0),
        (3840.0, 2160.0, 30.0),
    ];
    let mut report = Vec::new();
    let mut slow = Vec::new();
    for face in FACES.iter().filter(|f| f.group != Group::Off) {
        let mut line = format!("{:<13}", face.key);
        for (w, h, limit) in sizes {
            let mut harness = faces::harness_cpu(face.key, egui::vec2(w, h), 3.0);
            {
                let app = harness.state_mut();
                app.sound = noise(3.0);
                app.options = VisualOptions {
                    bars: 128,
                    ..Default::default()
                };
            }
            let mut total = 0.0;
            let mut worst = 0.0f64;
            for frame in 0..30 {
                let started = Instant::now();
                faces::frame(&mut harness);
                let took = started.elapsed().as_secs_f64() * 1000.0;
                if frame >= 5 {
                    total += took;
                    worst = worst.max(took);
                }
            }
            let mean = total / 25.0;
            line.push_str(&format!(" {w:>4}x{h:<4} {mean:>6.2} ms ({worst:>6.2})"));
            if mean > limit {
                slow.push(format!("{} at {w}x{h}: {mean:.2} ms", face.key));
            }
        }
        report.push(line);
    }
    println!("{}", report.join("\n"));
    assert!(slow.is_empty(), "{slow:#?}");
}

#[test]
fn the_analysis_of_a_frame_is_cheap() {
    let sound = noise(4.0);
    let options = VisualOptions {
        bars: 128,
        ..Default::default()
    };
    for (name, needs) in [
        ("bars, levels, loudness, beat", Needs::default()),
        ("with the fine spectrum", Needs { fine: true }),
    ] {
        let mut a = Analyzer::new();
        let mut total = 0.0;
        let frames = 120;
        for k in 0..frames {
            let end = 8192 + k * 800;
            let started = Instant::now();
            a.update(
                &sound[(end - 8192) * 2..end * 2],
                end as u64,
                faces::RATE,
                &options,
                1.0 / 60.0,
                true,
                needs,
            );
            total += started.elapsed().as_secs_f64() * 1000.0;
        }
        let mean = total / frames as f64;
        println!("analysis ({name}): {mean:.3} ms a frame (800 new frames each)");
        assert!(mean < 8.0, "{name}: {mean:.2} ms");
    }
}
