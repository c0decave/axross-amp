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
    Stats {
        coverage: drawn as f32 / total,
        colours: colours.len(),
        motion: moved as f32 / total,
    }
}

/// Pixels of a face's picture in the frame's 4 px margin (and its corners)
/// that are not as the empty frame has them.
fn spilled(image: &image::RgbaImage, empty: &image::RgbaImage) -> usize {
    let (w, h) = (image.width(), image.height());
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let margin = x < 3 || y < 3 || x >= w - 3 || y >= h - 3;
            if margin && far(image.get_pixel(x, y), empty.get_pixel(x, y)) {
                n += 1;
            }
        }
    }
    n
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
    // the frame alone (no face): what lies round and under its corners
    let empty = {
        let mut h = faces::harness("off", vec2(620.0, 300.0), true, 1.0);
        faces::frame(&mut h);
        h.render().expect("renders")
    };
    for face in FACES.iter().filter(|f| f.group != Group::Off) {
        let mut h = faces::harness(face.key, vec2(620.0, 300.0), true, 3.0);
        if face.key == "overview" {
            let o = overview_of(&h.state().sound);
            h.state_mut().overview = Some(o);
        }
        // 2.5 s, as axross's faces get in docs/axross_faces.py (the
        // feedback face rendered each frame: it draws on its last picture)
        for _ in 0..75 {
            faces::frame(&mut h);
            if face.key == "milkdrop" {
                h.render().unwrap();
            }
        }
        let first = h.render().expect("renders");
        // a third of a second on
        for _ in 0..10 {
            faces::frame(&mut h);
            if face.key == "milkdrop" {
                h.render().unwrap();
            }
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
        // ... and nothing outside the frame's inner edge: not over its
        // rounded corners, not in the margin round it
        let outside = spilled(&second, &empty);
        if outside > 0 {
            failures.push(format!("{}: {outside} pixels outside its frame", face.key));
        }
    }
    println!("{}", report.join("\n"));
    assert!(failures.is_empty(), "{failures:#?}");
}

/// One colour channel of an image (0 red, 1 green, 2 blue), averaged over
/// 2x2 blocks.
fn channel(image: &image::RgbaImage, c: usize) -> (Vec<f32>, usize, usize) {
    let (w, h) = (image.width() as usize / 2, image.height() as usize / 2);
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                sum += image.get_pixel((2 * x + dx) as u32, (2 * y + dy) as u32)[c] as f32;
            }
            out[y * w + x] = sum / 4.0;
        }
    }
    (out, w, h)
}

/// CIE L*a*b* of an sRGB pixel.
fn lab(p: &image::Rgba<u8>) -> [f32; 3] {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(p[0]), lin(p[1]), lin(p[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The mean colour difference (CIE76 ΔE) of two images, each blurred over
/// 4x4 blocks first (a half-resolution picture's edges are not its colour).
fn delta_e(a: &image::RgbaImage, b: &image::RgbaImage) -> f32 {
    let (w, h) = (a.width() / 4, a.height() / 4);
    let mean = |img: &image::RgbaImage, x: u32, y: u32| {
        let mut sum = [0u32; 3];
        for j in 0..4 {
            for i in 0..4 {
                let p = img.get_pixel(4 * x + i, 4 * y + j);
                for c in 0..3 {
                    sum[c] += p[c] as u32;
                }
            }
        }
        image::Rgba([
            (sum[0] / 16) as u8,
            (sum[1] / 16) as u8,
            (sum[2] / 16) as u8,
            255,
        ])
    };
    let mut total = 0.0;
    for y in 0..h {
        for x in 0..w {
            let (p, q) = (lab(&mean(a, x, y)), lab(&mean(b, x, y)));
            total += ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
        }
    }
    total / (w * h).max(1) as f32
}

/// The structural similarity (SSIM) of two images: each colour channel's,
/// the least of the three.
fn ssim(a: &image::RgbaImage, b: &image::RgbaImage) -> f32 {
    (0..3)
        .map(|c| ssim_of(&channel(a, c), &channel(b, c)))
        .fold(1.0, f32::min)
}

/// The structural similarity (SSIM, 8 x 8 windows) of two planes.
fn ssim_of((la, w, h): &(Vec<f32>, usize, usize), (lb, _, _): &(Vec<f32>, usize, usize)) -> f32 {
    let (w, h) = (*w, *h);
    let (c1, c2) = ((0.01f32 * 255.0).powi(2), (0.03f32 * 255.0).powi(2));
    let mut total = 0.0;
    let mut windows = 0;
    for y in (0..h.saturating_sub(8)).step_by(4) {
        for x in (0..w.saturating_sub(8)).step_by(4) {
            let (mut ma, mut mb) = (0.0f32, 0.0f32);
            for j in 0..8 {
                for i in 0..8 {
                    ma += la[(y + j) * w + x + i];
                    mb += lb[(y + j) * w + x + i];
                }
            }
            ma /= 64.0;
            mb /= 64.0;
            let (mut va, mut vb, mut cov) = (0.0f32, 0.0f32, 0.0f32);
            for j in 0..8 {
                for i in 0..8 {
                    let (da, db) = (la[(y + j) * w + x + i] - ma, lb[(y + j) * w + x + i] - mb);
                    va += da * da;
                    vb += db * db;
                    cov += da * db;
                }
            }
            va /= 63.0;
            vb /= 63.0;
            cov /= 63.0;
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            windows += 1;
        }
    }
    total / windows.max(1) as f32
}

#[test]
fn the_shader_faces_drawn_by_the_cpu_look_like_the_gpus() {
    // where shaders do not run on a GPU the CPU draws the faces per pixel
    // (at half the resolution here): the same picture at the same moment -
    // by structure in each colour channel (SSIM) and by colour (ΔE) - and
    // moving
    let background = {
        let s = viz::scheme("classic", &Default::default()).background;
        image::Rgba([s.r(), s.g(), s.b(), 255])
    };
    let mut report = Vec::new();
    let mut failed = Vec::new();
    for face in FACES.iter().filter(|f| f.shader) {
        let mut seen = Vec::new();
        let mut pictures = Vec::new();
        let mut times = Vec::new();
        for gpu in [true, false] {
            let mut h = faces::harness(face.key, vec2(320.0, 180.0), gpu, 3.0);
            // (every frame rendered: the GPU's feedback moves on when it is)
            for _ in 0..45 {
                faces::frame(&mut h);
                h.render().unwrap();
            }
            let first = h.render().unwrap();
            for _ in 0..3 {
                faces::frame(&mut h);
                h.render().unwrap();
            }
            let second = h.render().unwrap();
            times.push(h.state().analyzer.time);
            if !gpu {
                second
                    .save(shots().join(format!("{}-cpu.png", face.key)))
                    .unwrap();
            }
            seen.push(stats(&second, &first, background));
            pictures.push(second);
        }
        assert_eq!(times[0], times[1], "{}: not the same moment", face.key);
        let similar = ssim(&pictures[0], &pictures[1]);
        let colour = delta_e(&pictures[0], &pictures[1]);
        let c = &seen[1];
        report.push(format!(
            "{:<10} SSIM {similar:.3} (least of R, G, B); ΔE {colour:.1}; motion {:.1} % / {:.1} %",
            face.key,
            seen[0].motion * 100.0,
            c.motion * 100.0
        ));
        if similar < 0.8 || colour > 8.0 || c.motion < 0.0005 || c.coverage < 0.05 {
            failed.push(report.last().unwrap().clone());
        }
    }
    println!("{}", report.join("\n"));
    assert!(failed.is_empty(), "{failed:#?}");
}

/// What the GPU's work costs a face at 620 x 300, 1920 x 1080 and 4K: its
/// own render passes timed by the GPU's timestamp queries where the device
/// has them (else the clock round each submission), nothing read back.
/// Where there is no GPU (Mesa's lavapipe) this is a CPU's time - a
/// measure, not a limit: `cargo test --release --test faces -- --ignored`.
#[test]
#[ignore]
fn what_the_shader_faces_cost_the_gpu() {
    let mut h = faces::harness("plasma", vec2(320.0, 180.0), true, 3.0);
    for _ in 0..30 {
        faces::frame(&mut h);
    }
    let app = h.state();
    let rs = app.render_state.clone().expect("wgpu");
    let info = rs.adapter.get_info();
    println!("{} ({:?})", info.name, info.device_type);
    let scheme = viz::scheme("classic", &Default::default());
    for (w, hgt) in [(620u32, 300u32), (1920, 1080), (3840, 2160)] {
        let mut line = format!("{w}x{hgt}:");
        let mut timed = false;
        for face in FACES.iter().filter(|f| f.shader) {
            let shader = viz::gpu::Shader::of(face.key).unwrap_or(viz::gpu::Shader::Mel);
            let u = viz::gpu::Uniforms::new(shader, &app.analyzer, &scheme, 0.0);
            let (ms, stamps) = viz::gpu::measure(&rs, u, (w, hgt), 10);
            timed |= stamps;
            line.push_str(&format!(" {} {ms:.2} ms,", face.key));
        }
        println!(
            "{line} ({})",
            if timed {
                "GPU timestamps"
            } else {
                "wall clock, no timestamps"
            }
        );
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

#[test]
fn a_new_face_fades_in_over_the_one_before() {
    let mean = |image: &image::RgbaImage| {
        let mut m = [0.0f32; 3];
        for p in image.pixels() {
            for c in 0..3 {
                m[c] += p[c] as f32;
            }
        }
        m.map(|v| v / (image.width() * image.height()) as f32)
    };
    let mut h = faces::harness("spectrum", vec2(320.0, 180.0), true, 4.0);
    for _ in 0..30 {
        faces::frame(&mut h);
    }
    let before = mean(&h.render().unwrap());
    h.state_mut().key = "plasma";
    faces::frame(&mut h);
    assert_eq!(
        viz::fading(&h.state().state),
        Some("spectrum"),
        "no fade began"
    );
    for _ in 0..8 {
        faces::frame(&mut h);
    }
    let during = mean(&h.render().unwrap());
    for _ in 0..12 {
        faces::frame(&mut h);
    }
    assert_eq!(
        viz::fading(&h.state().state),
        None,
        "still fading after {} s",
        viz::CROSSFADE
    );
    let after = mean(&h.render().unwrap());
    // halfway the picture is between the two (every channel)
    for c in 0..3 {
        let (lo, hi) = (before[c].min(after[c]), before[c].max(after[c]));
        assert!(
            during[c] > lo - 2.0
                && during[c] < hi + 2.0
                && (hi - lo < 4.0 || (during[c] - lo) > 1.0 && (hi - during[c]) > 1.0),
            "channel {c}: {before:?} -> {during:?} -> {after:?}"
        );
    }
}

#[test]
fn the_waterfall_at_high_resolution_is_smooth() {
    // the same sound in axross's bars and in the fine spectrum's bands:
    // not in blocks of one colour (a bar's width, a row's height) but
    // changing from pixel to pixel; by the GPU and by the CPU alike
    let mut pictures = Vec::new();
    for (hd, gpu) in [(false, true), (true, true), (true, false)] {
        let mut h = faces::harness("waterfall", vec2(620.0, 300.0), gpu, 10.0);
        h.state_mut().state.waterfall_hd = hd;
        // (the history full: 256 steps at 30 a second)
        for _ in 0..270 {
            faces::frame(&mut h);
        }
        let picture = h.render().unwrap();
        if hd && gpu {
            picture.save(shots().join("waterfall-hd.png")).unwrap();
        }
        pictures.push(picture);
    }
    let measure = |img: &image::RgbaImage| {
        let mut colours = std::collections::HashSet::new();
        let (mut same, mut pairs) = (0usize, 0usize);
        for y in (8..img.height() - 8).step_by(2) {
            for x in 8..img.width() - 9 {
                let (p, q) = (img.get_pixel(x, y), img.get_pixel(x + 1, y));
                colours.insert([p[0] / 4, p[1] / 4, p[2] / 4]);
                pairs += 1;
                if p == q {
                    same += 1;
                }
            }
        }
        (colours.len(), same as f32 / pairs as f32)
    };
    let (bars, hd) = (measure(&pictures[0]), measure(&pictures[1]));
    println!("bars: {bars:?}, high resolution: {hd:?} (colours, neighbours alike)");
    assert!(bars.1 > 0.8 && hd.1 < 0.3, "blocks: {bars:?} vs {hd:?}");
    let similar = ssim(&pictures[1], &pictures[2]);
    assert!(
        similar > 0.8,
        "the CPU's picture is not the GPU's: SSIM {similar}"
    );
}

#[test]
fn the_feedback_face_changes_its_preset_each_phrase() {
    // a beat twice a second: a new preset after 16 of them (8 s), another
    // after 16 more; drawn, each, by the GPU
    let mut h = faces::harness("milkdrop", vec2(320.0, 180.0), true, 20.0);
    let mut seen = vec![viz::feedback_preset(&h.state().state)];
    for frame in 0..540 {
        faces::frame(&mut h);
        h.render().unwrap();
        let now = viz::feedback_preset(&h.state().state);
        if *seen.last().unwrap() != now {
            seen.push(now);
        }
        if frame % 90 == 89 {
            h.render()
                .unwrap()
                .save(shots().join(format!("milkdrop-{}.png", frame / 90)))
                .unwrap();
        }
    }
    println!("presets: {seen:?}");
    assert!(seen.len() >= 3, "presets shown: {seen:?}");
}

#[test]
fn slow_gradients_are_dithered_not_banded() {
    // the aurora's sky and glow change slowly: in 8 bits they stood in bands
    // (runs of one value, then a step); dithered, a pixel's neighbours
    // differ - by the GPU and by the CPU
    for gpu in [true, false] {
        let mut h = faces::harness("aurora", vec2(320.0, 180.0), gpu, 2.0);
        for _ in 0..30 {
            faces::frame(&mut h);
            h.render().unwrap();
        }
        let img = h.render().unwrap();
        // (where the picture is smooth - neighbours within 2 of each other
        // in each channel - how often they are the very same: in bands
        // nearly always, dithered much less)
        let (mut smooth, mut same) = (0usize, 0usize);
        for y in (10..img.height() - 10).step_by(2) {
            for x in 11..img.width() - 10 {
                let (p, q) = (img.get_pixel(x, y), img.get_pixel(x - 1, y));
                if (0..3).all(|c| (p[c] as i32 - q[c] as i32).abs() <= 2) {
                    smooth += 1;
                    if p == q {
                        same += 1;
                    }
                }
            }
        }
        let mean = same as f32 / smooth.max(1) as f32;

        assert!(
            mean < 0.38,
            "{mean} of smooth neighbours the same (gpu {gpu}): banded"
        );
    }
}

/// How much of the picture's height (rows) and width (columns) holds
/// something drawn in colour (not the background, not a grey grid line).
fn extent(img: &image::RgbaImage, background: image::Rgba<u8>) -> (f32, f32) {
    let (w, h) = (img.width(), img.height());
    let mut rows = vec![false; h as usize];
    let mut cols = vec![false; w as usize];
    for y in 6..h - 6 {
        for x in 6..w - 6 {
            let p = img.get_pixel(x, y);
            let (max, min) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
            if far(p, &background) && (max - min) > 40 {
                rows[y as usize] = true;
                cols[x as usize] = true;
            }
        }
    }
    let share = |v: &[bool]| v.iter().filter(|b| **b).count() as f32 / (v.len() - 12) as f32;
    (share(&rows), share(&cols))
}

#[test]
fn quiet_music_still_fills_the_picture() {
    // music five times quieter than the test signal, the scopes' nearly
    // mono: the fire still rises, the metaballs spread across, the scopes
    // (their gain found) still fill their frame
    let background = {
        let s = viz::scheme("classic", &Default::default()).background;
        image::Rgba([s.r(), s.g(), s.b(), 255])
    };
    let mut report = Vec::new();
    for (key, rows, cols) in [
        ("fire", 0.65, 0.0),
        ("metaballs", 0.0, 0.5),
        ("scope", 0.4, 0.0),
        ("lissajous", 0.4, 0.0),
    ] {
        let mut h = faces::harness(key, vec2(620.0, 300.0), true, 3.0);
        for f in h.state_mut().sound.chunks_mut(2) {
            f[0] *= 0.2;
            f[1] = f[0] * 0.97;
        }
        for _ in 0..75 {
            faces::frame(&mut h);
        }
        let img = h.render().unwrap();
        img.save(shots().join(format!("{key}-quiet.png"))).unwrap();
        let (r, c) = extent(&img, background);
        report.push(format!("{key}: rows {r:.2}, columns {c:.2}"));
        assert!(r >= rows && c >= cols, "{report:?}");
    }
    println!("{}", report.join("\n"));
}

#[test]
fn the_starfield_is_axrosss_unless_asked() {
    // axross's starfield: dots, 300 of them; trails and glow only as an
    // option - far more of the picture lit
    let mut lit = Vec::new();
    for plus in [false, true] {
        let mut h = faces::harness("starfield", vec2(620.0, 300.0), true, 3.0);
        h.state_mut().state.classic_plus = plus;
        for _ in 0..60 {
            faces::frame(&mut h);
        }
        let img = h.render().unwrap();
        let background = img.get_pixel(310, 3);
        let n = img.pixels().filter(|p| far(p, background)).count();
        lit.push(n as f32 / (620.0 * 300.0));
    }
    assert!(lit[0] < 0.03, "axross's dots: {lit:?}");
    assert!(lit[1] > lit[0] * 2.0, "the trails: {lit:?}");
}
