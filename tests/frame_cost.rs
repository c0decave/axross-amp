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

/// What a fixed piece of the same work - egui tessellating 3000 dots and
/// a line of 4000 points, in batches as a face's meshes are - takes on this
/// machine (CPU time, the median of nine). (In batches: as one, its meshes
/// were large enough for the allocator to map and unmap them each time, and
/// the reading was of the page faults - 0.8 ms at the start, 0.33 once the
/// faces had run.)
fn calibration_ms() -> f64 {
    use egui::epaint::{ClippedShape, Shape};
    let ctx = egui::Context::default();
    ctx.run_ui(Default::default(), |_| {})
        .textures_delta
        .clear();
    let shape = |shape: Shape| ClippedShape {
        clip_rect: egui::Rect::EVERYTHING,
        transform_after_tessellation: Default::default(),
        shape,
    };
    let mut batches: Vec<Vec<ClippedShape>> = (0..30)
        .map(|b| {
            (0..100)
                .map(|k| {
                    let k = b * 100 + k;
                    shape(Shape::circle_filled(
                        egui::pos2((k % 60) as f32 * 30.0, (k / 60) as f32 * 20.0),
                        6.0,
                        egui::Color32::WHITE,
                    ))
                })
                .collect()
        })
        .collect();
    for b in 0..10 {
        let line: Vec<egui::Pos2> = (0..400)
            .map(|k| {
                let t = (b * 400 + k) as f32 * 0.01;
                egui::pos2(
                    900.0 + 400.0 * (3.0 * t).sin(),
                    500.0 + 300.0 * (5.0 * t).cos(),
                )
            })
            .collect();
        batches.push(vec![shape(Shape::line(
            line,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
        ))]);
    }
    let run = || {
        let mut took = 0.0;
        for batch in &batches {
            let batch = batch.clone();
            let started = cpu_ms();
            let out = ctx.tessellate(batch, 1.0);
            took += cpu_ms() - started;
            std::hint::black_box(out);
        }
        took
    };
    for _ in 0..3 {
        run();
    }
    let mut times: Vec<f64> = (0..9).map(|_| run()).collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

/// The calibration's time on the machine the limits were set on (an
/// unoptimised build; 0.32 - 0.34 ms there, at the start and after the
/// faces alike): a machine slower than it - a shared runner of four
/// virtual CPUs read particles at 1920 x 1080 in 18.7 - 18.8 ms where that
/// one reads 13 - 13.6 - has its limits raised as much, up to twice; a
/// faster one keeps them as they are.
const CALIBRATION_REFERENCE_MS: f64 = 0.34;

/// The CPU time of a frame of `face` at `w` x `h` - the median, the 95th
/// percentile and the worst of frames 5 - 44 (the first ones fill the
/// analysis and the meshes).
fn frame_ms(face: &'static str, w: f32, h: f32) -> (f64, f64, f64) {
    let mut harness = faces::harness_cpu(face, egui::vec2(w, h), 3.0);
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
    let mut times = Vec::new();
    for frame in 0..45 {
        let started = cpu_ms();
        faces::frame(&mut harness);
        let took = cpu_ms() - started;
        if frame >= 5 {
            times.push(took);
        }
    }
    times.sort_by(f64::total_cmp);
    let p95 = times[(times.len() * 95).div_ceil(100) - 1];
    (times[times.len() / 2], p95, times[times.len() - 1])
}

/// Every face's frame within its limit at every size - measured as CPU
/// time, the median of 40 frames and their 95th percentile (the median
/// alone hid a face whose cost spikes every few frames: review 17, L6),
/// the limits scaled to this machine by a
/// calibration of the same kind of work, and a face over its limit
/// measured once more (the machine calibrated again) before it fails:
/// 0.16.2's release run failed on a shared runner reading 1.4 times this
/// machine's times, the faces unchanged (particles at 1920 x 1080 18.69
/// ms, the spirograph at 620 x 300 12.28).
#[test]
fn every_face_draws_a_frame_in_a_few_milliseconds_at_any_size() {
    let sizes = [
        (620.0, 300.0, 12.0),
        (1920.0, 1080.0, 16.0),
        (3840.0, 2160.0, 30.0),
    ];
    let scale_now = || {
        let calibration = calibration_ms();
        (
            calibration,
            (calibration / CALIBRATION_REFERENCE_MS).clamp(1.0, 2.0),
        )
    };
    let (calibration, mut scale) = scale_now();
    println!(
        "calibration: {calibration:.2} ms ({CALIBRATION_REFERENCE_MS} ms on the reference): \
         the limits times {scale:.2}"
    );
    let mut report = Vec::new();
    let mut slow = Vec::new();
    // (AMP_COST_ONLY: these faces only, by key, comma-separated)
    let only = std::env::var("AMP_COST_ONLY").ok();
    for face in FACES.iter().filter(|f| {
        f.group != Group::Off
            && only
                .as_deref()
                .is_none_or(|o| o.split(',').any(|k| k == f.key))
    }) {
        let mut line = format!("{:<13}", face.key);
        for (w, h, limit) in sizes {
            let (mut median, mut p95, mut worst) = frame_ms(face.key, w, h);
            if median.max(p95) > limit * scale {
                // (once more: a busy neighbour, a slowed clock)
                // (never stricter than the first calibration said)
                let (c, s) = scale_now();
                scale = scale.max(s);
                (median, p95, worst) = frame_ms(face.key, w, h);
                println!(
                    "{} at {w}x{h} measured again (calibration {c:.2} ms): {median:.2} ms, \
                     95th percentile {p95:.2}",
                    face.key
                );
            }
            line.push_str(&format!(
                " {w:>4}x{h:<4} {median:>6.2} ms ({p95:>6.2}, {worst:>6.2})"
            ));
            if median.max(p95) > limit * scale {
                slow.push(format!(
                    "{} at {w}x{h}: {median:.2} ms, 95th percentile {p95:.2} (limit {:.2})",
                    face.key,
                    limit * scale
                ));
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
