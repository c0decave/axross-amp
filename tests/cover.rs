//! The cover at its size: a fine cover (stripes a pixel wide, 1024 px)
//! shown on the card and as the Cover face is smooth there, not aliased
//! (egui's renderer makes no mipmaps: a cover made smaller by the GPU
//! merged its stripes and stepped its edges).

#[path = "support/player.rs"]
mod player;

use axross_amp::engine::{Art, TrackInfo};
use player::{Scratch, harness, settle, until, write_chord};
use std::sync::Arc;

fn shots() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn striped() -> Arc<Art> {
    let n = 1024u32;
    let rgba: Vec<u8> = (0..n * n)
        .flat_map(|i| {
            let v = if (i % n).is_multiple_of(2) { 250 } else { 10 };
            [v, v, v, 255]
        })
        .collect();
    // (the small one striped too: shown as it is, it would alias as well)
    let small: Vec<u8> = (0..256u32 * 256)
        .flat_map(|i| {
            let v = if (i % 256).is_multiple_of(3) { 250 } else { 10 };
            [v, v, v, 255]
        })
        .collect();
    Arc::new(Art {
        width: 256,
        height: 256,
        rgba: small,
        png: Vec::new(),
        large: Some((n, n, rgba)),
    })
}

/// How much neighbouring pixels differ across a box (high frequencies).
fn roughness(img: &image::RgbaImage, centre: egui::Pos2, half: u32) -> f32 {
    let (cx, cy) = (centre.x as u32, centre.y as u32);
    let mut sum = 0.0;
    let mut n = 0;
    for y in cy - half..cy + half {
        for x in cx - half..cx + half {
            let (a, b) = (img.get_pixel(x, y)[0], img.get_pixel(x - 1, y)[0]);
            sum += (a as f32 - b as f32).abs();
            n += 1;
        }
    }
    sum / n as f32
}

#[test]
fn a_fine_cover_is_smooth_on_the_card_and_as_the_picture() {
    let home = Scratch::new("cover");
    let track = home.join("a.wav");
    write_chord(&track, 6);
    let mut h = harness(&home, vec![track.to_string_lossy().into_owned()]);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let ctx = h.ctx.clone();
    h.state_mut().show_info(
        &ctx,
        TrackInfo {
            title: "Stripes".into(),
            art: Some(striped()),
            rate: 48_000,
            channels: 2,
            ..Default::default()
        },
    );
    // (the sizes are made on a thread)
    for _ in 0..20 {
        settle(&mut h, 1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let img = h.render().unwrap();
    // the card's cover: its left part, below the tabs
    let card = img.clone();
    let r = roughness(&card, egui::pos2(84.0, 157.0), 30);
    card.save(shots().join("cover-card.png")).ok();
    assert!(r < 12.0, "the card's cover aliased: {r}");
    // the cover as the picture
    h.state_mut().set_mode("cover");
    for _ in 0..20 {
        settle(&mut h, 1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let img = h.render().unwrap();
    img.save(shots().join("cover-face.png")).ok();
    use egui_kittest::kittest::Queryable;
    let picture = h.get_by_label_contains("Visualization: ").rect();
    let r = roughness(&img, picture.center(), 40);
    assert!(r < 12.0, "the cover face aliased: {r}");
}
