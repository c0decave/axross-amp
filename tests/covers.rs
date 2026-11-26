//! The stand-in covers (src/covers.rs): 100 presets, each unlike every
//! other (no near-duplicates by a measure of their difference), none with
//! enough saturated red to be taken for a red flash, none flat; the same
//! track always the same cover. `COVERS_SHEET=1`: all 100 at 256 px in a
//! sheet, `target/screenshots/covers/sheet.png` (and each alone).

use axross_amp::covers::{self, FAMILIES, PRESETS};

/// A preset at `side` px, made `cell` x `cell` by averaging: its layout
/// and colours, not its fine detail.
fn thumb(preset: usize, seed: u64, side: u32, cell: u32) -> Vec<[f32; 3]> {
    let img = covers::render(preset, seed, side);
    let k = side / cell;
    let mut out = vec![[0.0f32; 3]; (cell * cell) as usize];
    for (x, y, p) in img.enumerate_pixels() {
        let i = ((y / k) * cell + x / k) as usize;
        for c in 0..3 {
            out[i][c] += p[c] as f32 / 255.0 / (k * k) as f32;
        }
    }
    out
}

fn distance(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(p, q)| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs())
        .sum::<f32>()
        / (a.len() as f32 * 3.0)
}

/// Pairwise unlike: by their 16 x 16 averages (layout and colour) - the
/// nearest two still clearly apart.
#[test]
fn the_hundred_presets_are_each_unlike_the_others() {
    let seed = 0x1234_5678;
    let thumbs: Vec<Vec<[f32; 3]>> = (0..PRESETS).map(|p| thumb(p, seed, 128, 16)).collect();
    let mut nearest = (f32::MAX, 0, 0);
    let mut close = Vec::new();
    for i in 0..PRESETS {
        for j in i + 1..PRESETS {
            let d = distance(&thumbs[i], &thumbs[j]);
            if d < nearest.0 {
                nearest = (d, i, j);
            }
            if d < MIN_DISTANCE {
                close.push(format!("{i} and {j}: {d:.3}"));
            }
        }
    }
    println!("nearest: {} and {}: {:.3}", nearest.1, nearest.2, nearest.0);
    assert!(close.is_empty(), "near-duplicates:\n{}", close.join("\n"));
}

/// The least difference two presets may have (mean absolute, per channel,
/// over their 16 x 16 averages; 0.06 is about 15 levels of 255 everywhere).
const MIN_DISTANCE: f32 = 0.06;

/// None is flat, none too dark or too light to show on a card, none with
/// as much saturated red as the window's red guard watches for (8 %).
#[test]
fn every_preset_has_contrast_and_no_red_flash() {
    for p in 0..PRESETS {
        let img = covers::render(p, 7, 128);
        let art = axross_amp::engine::Art {
            width: 128,
            height: 128,
            rgba: img.as_raw().clone(),
            png: Vec::new(),
            large: None,
        };
        let red = axross_amp::app::red_area(&art);
        assert!(
            red < 0.08,
            "preset {p} ({}): red {red}",
            FAMILIES[covers::family_of(p).0]
        );
        let lum: Vec<f32> = img
            .pixels()
            .map(|q| (0.2126 * q[0] as f32 + 0.7152 * q[1] as f32 + 0.0722 * q[2] as f32) / 255.0)
            .collect();
        let mean = lum.iter().sum::<f32>() / lum.len() as f32;
        let spread =
            (lum.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / lum.len() as f32).sqrt();
        assert!(spread > 0.04, "preset {p}: flat ({spread})");
        assert!((0.06..0.94).contains(&mean), "preset {p}: mean {mean}");
    }
}

/// Two albums of one preset differ a little; the same seed draws the same
/// picture at every size (the small one is the large one made small).
#[test]
fn the_seed_moves_a_presets_details_and_the_sizes_agree() {
    let p = 3;
    let a = thumb(p, 1, 128, 16);
    let b = thumb(p, 2, 128, 16);
    assert!(distance(&a, &b) > 0.002, "the seed moves nothing");
    let small = thumb(p, 1, 64, 16);
    assert!(distance(&a, &small) < 0.03, "64 px and 128 px unlike");
}

/// The sheet of all 100 (COVERS_SHEET=1).
#[test]
fn the_sheet() {
    if std::env::var_os("COVERS_SHEET").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/covers");
    std::fs::create_dir_all(&dir).unwrap();
    let side = 256u32;
    let gap = 6u32;
    let mut sheet = image::RgbaImage::from_pixel(
        10 * (side + gap) + gap,
        10 * (side + gap) + gap,
        image::Rgba([24, 24, 28, 255]),
    );
    let seed: u64 = std::env::var("COVERS_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x1234_5678);
    for p in 0..PRESETS {
        let img = covers::render(p, seed, side);
        img.save(dir.join(format!("{p:03}.png"))).unwrap();
        let (x, y) = (
            (p as u32 % 10) * (side + gap) + gap,
            (p as u32 / 10) * (side + gap) + gap,
        );
        image::imageops::overlay(&mut sheet, &img, x as i64, y as i64);
    }
    sheet.save(dir.join("sheet.png")).unwrap();
}

// ------------------------------------------------------------ in the window

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::state::Track;
use egui_kittest::Harness;

fn window(dir: &std::path::Path) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: true,
    };
    Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .build_eframe(move |cc| App::with_setup(cc, Vec::new(), setup.clone()))
}

fn track(path: &str, album: &str) -> Track {
    Track {
        path: path.to_owned(),
        title: "A title".to_owned(),
        artist: "Someone".to_owned(),
        album: album.to_owned(),
        ..Default::default()
    }
}

/// `tracks` posed, the `current` one opened as the engine would say it -
/// without a cover; frames until its cover is there (or `secs`).
fn open(
    h: &mut Harness<'static, App>,
    tracks: &[Track],
    current: usize,
) -> Option<std::sync::Arc<axross_amp::engine::Art>> {
    h.state_mut().pose(tracks.to_vec(), current, 200.0, 1.0);
    let info = axross_amp::engine::TrackInfo {
        album: tracks[current].album.clone(),
        title: tracks[current].title.clone(),
        ..Default::default()
    };
    let ctx = h.ctx.clone();
    h.state_mut().show_info(&ctx, info);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let want = covers::seed(&tracks[current].path, &tracks[current].album);
    let expected = covers::render(covers::preset_of(want), want, 32);
    while std::time::Instant::now() < end {
        h.run_steps(1);
        // (the one wanted: the last of a track before may still show)
        if let (Some(art), true) = h.state().cover_art()
            && let Some((w, hh, rgba)) = &art.large
        {
            let big = image::RgbaImage::from_raw(*w, *hh, rgba.clone()).unwrap();
            let small =
                image::imageops::resize(&big, 32, 32, image::imageops::FilterType::Triangle);
            let d = small
                .as_raw()
                .iter()
                .zip(expected.as_raw())
                .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs())
                .sum::<u32>() as f32
                / small.as_raw().len() as f32;
            if d < 12.0 {
                return Some(art);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

/// A track without a cover is shown one made for it: the same in another
/// window, one for an album's tracks, another for another album.
#[test]
fn a_track_without_a_cover_gets_its_own_the_same_every_time() {
    let home = player::Scratch::new("covers-window");
    let tracks = vec![
        track("/m/Album/01.flac", "Album"),
        track("/m/Album/02.flac", "Album"),
        track("/m/Other/01.flac", "Other"),
        track("/m/single.mp3", ""),
    ];
    let mut h = window(home.path());
    let a = open(&mut h, &tracks, 0).expect("a cover made for it");
    // (its PNG for the desktop, its large one for the picture)
    assert!(!a.png.is_empty() && a.large.is_some());
    assert_eq!((a.width, a.height), (covers::SMALL, covers::SMALL));
    let b = open(&mut h, &tracks, 1).expect("the album's");
    assert_eq!(a.png, b.png, "an album, one cover");
    let c = open(&mut h, &tracks, 2).expect("another album's");
    assert_ne!(a.png, c.png);
    let d = open(&mut h, &tracks, 3).expect("a track without an album");
    assert_ne!(c.png, d.png);
    // another window, another day: the same picture
    let home2 = player::Scratch::new("covers-window-2");
    let mut h2 = window(home2.path());
    let again = open(&mut h2, &tracks, 0).expect("made again");
    assert_eq!(again.png, a.png);
}

/// The setting at None: the empty box, as before; at Generated again, the
/// track playing gets its cover at once.
#[test]
fn none_leaves_the_box_empty_as_before() {
    let home = player::Scratch::new("covers-none");
    let tracks = vec![track("/m/Album/01.flac", "Album")];
    let mut h = window(home.path());
    let ctx = h.ctx.clone();
    h.state_mut().set_generated_covers(&ctx, false);
    h.state_mut().pose(tracks.clone(), 0, 200.0, 1.0);
    let info = axross_amp::engine::TrackInfo {
        album: "Album".into(),
        ..Default::default()
    };
    h.state_mut().show_info(&ctx, info);
    for _ in 0..40 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.state().cover_art().0.is_none() && !h.state().has_cover());
    h.state_mut().set_generated_covers(&ctx, true);
    let mut shown = false;
    for _ in 0..600 {
        h.run_steps(1);
        if h.state().has_cover() {
            shown = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(shown, "made once the setting is on");
    assert!(h.state().cover_art().1);
    h.state_mut().set_generated_covers(&ctx, false);
    h.run_steps(30);
    assert!(h.state().cover_art().0.is_none(), "let go once it is off");
}

/// A track's own cover is shown, never one made.
#[test]
fn a_track_with_a_cover_keeps_it() {
    let home = player::Scratch::new("covers-own");
    let mut h = window(home.path());
    h.state_mut()
        .pose(vec![track("/m/Album/01.flac", "Album")], 0, 200.0, 1.0);
    let own = std::sync::Arc::new(axross_amp::engine::Art {
        width: 2,
        height: 2,
        rgba: vec![200; 16],
        png: vec![1, 2, 3],
        large: None,
    });
    let ctx = h.ctx.clone();
    h.state_mut().show_info(
        &ctx,
        axross_amp::engine::TrackInfo {
            art: Some(own.clone()),
            ..Default::default()
        },
    );
    for _ in 0..40 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (art, made) = h.state().cover_art();
    assert!(!made && art.is_some_and(|a| a.png == own.png));
}
