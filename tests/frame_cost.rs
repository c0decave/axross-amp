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

/// The CPU time this thread has used, in milliseconds: what a frame costs
/// the thread that draws it, whatever else the machine runs (wall-clock
/// time on a busy machine counted the other processes' turns: particles at
/// 1920 x 1080 read 17 - 18 ms of a 16 ms budget with the load at 5 - 30).
/// (Where there is no thread clock, the wall clock.)
fn cpu_ms() -> f64 {
    #[cfg(unix)]
    {
        let mut t = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime writes the timespec it is given, no more.
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } == 0 {
            return t.tv_sec as f64 * 1000.0 + t.tv_nsec as f64 / 1e6;
        }
    }
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

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
            // (AMP_NO_GUARD: what the face costs without the flash guard)
            harness.state_mut().state.flash_off = std::env::var_os("AMP_NO_GUARD").is_some();
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
                let started = cpu_ms();
                faces::frame(&mut harness);
                let took = cpu_ms() - started;
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
            let started = cpu_ms();
            a.update(
                &sound[(end - 8192) * 2..end * 2],
                end as u64,
                faces::RATE,
                &options,
                1.0 / 60.0,
                true,
                needs,
            );
            total += cpu_ms() - started;
        }
        let mean = total / frames as f64;
        println!("analysis ({name}): {mean:.3} ms a frame (800 new frames each)");
        assert!(mean < 8.0, "{name}: {mean:.2} ms");
    }
}

/// What a window's design adds to a frame at 1920 x 1080: its panels (the
/// card, the playlist's), the picture's frame, the readout, the reels, the
/// needles - drawn and tessellated - under a millisecond each.
#[test]
fn a_design_draws_in_under_a_millisecond_at_full_hd() {
    use axross_amp::design::{self, DESIGNS};
    let ctx = egui::Context::default();
    let t = &axross_amp::theme::THEMES[0];
    let size = egui::vec2(1920.0, 1080.0);
    let mut report = Vec::new();
    let mut slow = Vec::new();
    for d in &DESIGNS {
        let mut total = 0.0;
        for k in 0..40 {
            let started = cpu_ms();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let p = ui.painter();
                let all = ui.max_rect().shrink(10.0);
                let card = egui::Rect::from_min_size(all.min, egui::vec2(all.width(), 220.0));
                let list =
                    egui::Rect::from_min_max(egui::pos2(all.left(), all.bottom() - 360.0), all.max);
                let picture = egui::Rect::from_min_max(
                    egui::pos2(all.left(), card.bottom() + 8.0),
                    egui::pos2(all.right(), list.top() - 90.0),
                );
                design::panel(p, card, d, t, t.surface, 1);
                design::panel(p, list, d, t, design::list_fill(d, t), 2);
                design::bezel(p, picture, d, t);
                let at = card.min + egui::vec2(260.0, 120.0);
                if d.extras.lcd {
                    design::lcd(p, at, "01  1:23", 18.0, t);
                }
                let gauge =
                    egui::Rect::from_min_size(at + egui::vec2(140.0, 0.0), egui::vec2(150.0, 44.0));
                if d.extras.reels {
                    design::reels(p, gauge, k as f32 * 0.01, t);
                }
                if d.extras.needles {
                    design::needles(p, gauge, [0.4, 0.6], t);
                }
            });
            output.textures_delta.clear();
            let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
            let took = cpu_ms() - started;
            if k >= 10 {
                total += took;
            }
        }
        let mean = total / 30.0;
        report.push(format!("{:<10} {mean:>6.3} ms", d.key));
        if mean > 1.0 {
            slow.push(format!("{}: {mean:.3} ms", d.key));
        }
    }
    println!("{}", report.join("\n"));
    assert!(slow.is_empty(), "{slow:#?}");
}

#[path = "support/flashmeter.rs"]
mod flashmeter;

/// The stereo scope through stereo noise unlike left and right (and the
/// rest of the flash music) at 1920 x 1080: no slow frame. Its tangle's
/// sharp turns were tessellated with miters reaching thousands of points
/// out, and the flash guard laid them along millions of cells: 0.4 - 0.5 s
/// a frame (review 11), 0.2 s in a release build here.
#[test]
fn the_stereo_scope_has_no_slow_frame_in_stereo_noise() {
    let mut h = faces::harness_cpu("lissajous", egui::vec2(1920.0, 1080.0), 4.0);
    h.state_mut().sound = flashmeter::worst_music(0.5);
    h.state_mut().fps = 60.0;
    let mut worst = (0.0f64, 0);
    for frame in 0..330 {
        let started = cpu_ms();
        faces::frame(&mut h);
        let took = cpu_ms() - started;
        if frame >= 5 && took > worst.0 {
            worst = (took, frame);
        }
    }
    println!(
        "the stereo scope at 1920 x 1080: its slowest frame {:.1} ms (frame {})",
        worst.0, worst.1
    );
    // (an unoptimised build: several times the release's 5 ms)
    assert!(worst.0 < 100.0, "{:.1} ms at frame {}", worst.0, worst.1);
}
