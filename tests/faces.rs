//! Every face, from a known signal, rendered by wgpu (Mesa's lavapipe
//! without a GPU): what each draws is measured (how much of the picture,
//! how many colours, how much moves from one frame to the next), the
//! pictures are kept in `target/screenshots/faces/` (docs/faces_sheet.py
//! makes the README's sheets from them), and the shader faces drawn on the
//! CPU (no wgpu) must look like the GPU's.

#[path = "support/faces.rs"]
mod faces;

use axross_amp::viz::{self, FACES, Group, Overview};
use egui::vec2;
use std::path::PathBuf;

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/faces");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn far(a: &image::Rgba<u8>, b: &image::Rgba<u8>) -> bool {
    (0..3)
        .map(|c| (a[c] as i32 - b[c] as i32).abs())
        .sum::<i32>()
        > 24
}

struct Stats {
    coverage: f32,
    colours: usize,
    motion: f32,
    mean: [f32; 3],
}

fn stats(
    image: &image::RgbaImage,
    before: &image::RgbaImage,
    background: image::Rgba<u8>,
) -> Stats {
    // (inside the rounded corners)
    let (x0, y0, x1, y1) = (6, 6, image.width() - 6, image.height() - 6);
    let (mut drawn, mut moved) = (0usize, 0usize);
    let mut colours = std::collections::HashSet::new();
    let mut mean = [0.0f64; 3];
    for y in y0..y1 {
        for x in x0..x1 {
            let p = image.get_pixel(x, y);
            for c in 0..3 {
                mean[c] += p[c] as f64;
            }
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
    Stats {
        coverage: drawn as f32 / total,
        colours: colours.len(),
        motion: moved as f32 / total,
        mean: mean.map(|m| (m / total as f64) as f32),
    }
}

/// The track's waveform as the player reads it (min, max, rms a column).
fn overview_of(sound: &[f32]) -> Overview {
    let frames = sound.len() / 2;
    let columns = 1200;
    let per = frames / columns;
    Overview {
        columns: (0..columns)
            .map(|c| {
                let part = &sound[c * per * 2..(c + 1) * per * 2];
                let (mut lo, mut hi, mut sq) = (0.0f32, 0.0f32, 0.0f32);
                for f in part.chunks(2) {
                    let v = (f[0] + f[1]) * 0.5;
                    lo = lo.min(v);
                    hi = hi.max(v);
                    sq += v * v;
                }
                [lo, hi, (sq / per as f32).sqrt()]
            })
            .collect(),
        done: 1.0,
    }
}

#[test]
fn every_face_draws_moves_and_is_kept_as_a_picture() {
    let background = {
        let s = viz::scheme("classic", &Default::default()).background;
        image::Rgba([s.r(), s.g(), s.b(), 255])
    };
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for face in FACES.iter().filter(|f| f.group != Group::Off) {
        let mut h = faces::harness(face.key, vec2(620.0, 300.0), true, 3.0);
        if face.key == "overview" {
            let o = overview_of(&h.state().sound);
            h.state_mut().overview = Some(o);
        }
        // 2.5 s, as axross's faces get in docs/axross_faces.py
        for _ in 0..75 {
            faces::frame(&mut h);
        }
        let first = h.render().expect("renders");
        // a third of a second on
        for _ in 0..10 {
            faces::frame(&mut h);
        }
        let second = h.render().expect("renders");
        second
            .save(shots().join(format!("{}.png", face.key)))
            .unwrap();
        let st = stats(&second, &first, background);
        report.push(format!(
            "{:<13} {:<10} coverage {:>5.1} %, {:>3} colours, motion {:>5.1} %",
            face.key,
            face.group.name(),
            st.coverage * 100.0,
            st.colours,
            st.motion * 100.0
        ));
        // something of its own, in more than two colours, moving with the sound
        if !(0.001..=1.0).contains(&st.coverage) || st.colours < 3 || st.motion < 0.0005 {
            failures.push(report.last().unwrap().clone());
        }
    }
    println!("{}", report.join("\n"));
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_shader_faces_look_alike_on_the_cpu() {
    // without wgpu's shaders the CPU draws them, coarser: the same picture
    // in the large (its mean colour), and still moving
    let background = {
        let s = viz::scheme("classic", &Default::default()).background;
        image::Rgba([s.r(), s.g(), s.b(), 255])
    };
    let mut report = Vec::new();
    for face in FACES.iter().filter(|f| f.shader) {
        let mut seen = Vec::new();
        for gpu in [true, false] {
            let mut h = faces::harness(face.key, vec2(320.0, 180.0), gpu, 3.0);
            for _ in 0..45 {
                faces::frame(&mut h);
            }
            let first = h.render().unwrap();
            for _ in 0..3 {
                faces::frame(&mut h);
            }
            let second = h.render().unwrap();
            if !gpu {
                second
                    .save(shots().join(format!("{}-cpu.png", face.key)))
                    .unwrap();
            }
            seen.push(stats(&second, &first, background));
        }
        let (g, c) = (&seen[0], &seen[1]);
        let distance: f32 = (0..3).map(|k| (g.mean[k] - c.mean[k]).abs()).sum();
        report.push(format!(
            "{:<10} mean colour GPU {:?} CPU {:?} (apart {distance:.1}); motion {:.1} % / {:.1} %",
            face.key,
            g.mean.map(|v| v as u32),
            c.mean.map(|v| v as u32),
            g.motion * 100.0,
            c.motion * 100.0
        ));
        assert!(distance < 40.0, "{}", report.last().unwrap());
        assert!(
            c.motion > 0.0005 && c.coverage > 0.05,
            "{}",
            report.last().unwrap()
        );
    }
    println!("{}", report.join("\n"));
}

/// What the GPU's work costs, rendered by wgpu (where there is no GPU: Mesa's
/// lavapipe on the CPU, far slower than any graphics card) - for a measure,
/// not a limit: `cargo test --test faces -- --ignored`.
#[test]
#[ignore]
fn what_the_shader_faces_cost_the_gpu() {
    for (w, h) in [(620.0, 300.0), (1920.0, 1080.0), (3840.0, 2160.0)] {
        let mut line = format!("{w}x{h}:");
        for face in FACES.iter().filter(|f| f.shader) {
            let mut harness = faces::harness(face.key, vec2(w, h), true, 3.0);
            for _ in 0..10 {
                faces::frame(&mut harness);
            }
            harness.render().unwrap();
            let started = std::time::Instant::now();
            for _ in 0..3 {
                faces::frame(&mut harness);
                harness.render().unwrap();
            }
            let ms = started.elapsed().as_secs_f64() * 1000.0 / 3.0;
            line.push_str(&format!(" {} {ms:.0} ms,", face.key));
        }
        println!("{line}");
    }
}

#[test]
fn the_analyzer_curve_is_filled_by_height_without_streaks() {
    // each column coloured by its own height showed as vertical streaks:
    // along a row of the filled area the colour must hardly change
    let mut h = faces::harness("analyzer", vec2(620.0, 300.0), true, 3.0);
    for _ in 0..60 {
        faces::frame(&mut h);
    }
    let image = h.render().unwrap();
    // the plot's own ground (above the curve, under the labels)
    let ground = {
        // (the commonest colour of a row high in the plot)
        let mut counts = std::collections::HashMap::new();
        for x in 60..590u32 {
            *counts.entry(image.get_pixel(x, 50).0).or_insert(0) += 1;
        }
        image::Rgba(counts.into_iter().max_by_key(|(_, n)| *n).unwrap().0)
    };
    let near = |a: &image::Rgba<u8>, b: &image::Rgba<u8>| {
        (0..3)
            .map(|c| (a[c] as i32 - b[c] as i32).abs())
            .sum::<i32>()
            < 12
    };
    // where the curve is in each column (the first pixel from the top that
    // is not the ground), and the plot's bottom
    let tops: Vec<u32> = (0..image.width())
        .map(|x| {
            // (a grid line is one pixel with the ground under it: the fill goes on)
            // (from under the readout in the top corner)
            (48..276u32)
                .find(|y| (0..4).all(|d| !near(image.get_pixel(x, y + d), &ground)))
                .unwrap_or(280)
        })
        .collect();
    // every row of the plot well under the curve: the colour is the
    // row's (by height alone) but where a line of the curve crosses
    let (mut off, mut all) = (0usize, 0usize);
    for y in 48..276u32 {
        let row: Vec<image::Rgba<u8>> = (60..590u32)
            .filter(|x| y >= tops[*x as usize] + 12)
            .map(|x| *image.get_pixel(x, y))
            .collect();
        if row.len() < 20 {
            continue;
        }
        let median = |c: usize| {
            let mut v: Vec<u8> = row.iter().map(|p| p[c]).collect();
            v.sort_unstable();
            v[v.len() / 2] as i32
        };
        let m = [median(0), median(1), median(2)];
        all += row.len();
        off += row
            .iter()
            .filter(|p| (0..3).map(|c| (p[c] as i32 - m[c]).abs()).sum::<i32>() > 8)
            .count();
    }
    let share = off as f32 / all.max(1) as f32;
    assert!(all > 10_000, "too little under the curve: {all}");
    assert!(
        share < 0.15,
        "{:.1} % of the fill off its row's colour",
        share * 100.0
    );
}
