//! Skipping through tracks never blinks the cover (review 12, high 2): a
//! manual Next cleared the cover, the Glass backdrop made of it and the
//! faces' thumbnail at once - each skip dropped to the ♫ placeholder and
//! back. Holding N in Glass with the same white cover on every track read
//! 16 transitions a second; white and black covers 8 (a press every 8
//! frames) and 30 (every 2). Now the cover stays until the new one comes,
//! changes at most twice a second and crossfades: the whole window, read
//! by the project's flash meter, keeps within three flashes a second.
//!
//! The tracks are the corpus's tone.mp3 (AMP_CORPUS) with an ID3 picture.

#[path = "support/flashmeter.rs"]
mod flashmeter;
#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::engine::RenderState;
use egui_kittest::Harness;
use player::{Scratch, settle, until};
use std::path::{Path, PathBuf};

/// A square PNG of one colour.
fn png(rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(96, 96, image::Rgb(rgb));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// `source` with an ID3v2.3 tag holding `cover` as its front cover.
fn with_cover(source: &Path, cover: &[u8], title: &str, target: &Path) {
    let mut apic = vec![0u8];
    apic.extend_from_slice(b"image/png\0");
    apic.push(3); // (the front cover)
    apic.push(0); // (no description)
    apic.extend_from_slice(cover);
    let mut tit2 = vec![0u8];
    tit2.extend_from_slice(title.as_bytes());
    let mut tag = Vec::new();
    for (id, body) in [(b"TIT2", tit2), (b"APIC", apic)] {
        tag.extend_from_slice(id);
        tag.extend_from_slice(&(body.len() as u32).to_be_bytes());
        tag.extend_from_slice(&[0, 0]);
        tag.extend_from_slice(&body);
    }
    let size = tag.len() as u32;
    let mut out = b"ID3\x03\x00\x00".to_vec();
    out.extend_from_slice(&[
        (size >> 21) as u8 & 0x7f,
        (size >> 14) as u8 & 0x7f,
        (size >> 7) as u8 & 0x7f,
        size as u8 & 0x7f,
    ]);
    out.extend_from_slice(&tag);
    out.extend_from_slice(&std::fs::read(source).unwrap());
    std::fs::write(target, out).unwrap();
}

struct Skipped {
    transitions: usize,
    strict: usize,
    area: usize,
    skips: usize,
    covers_seen: bool,
}

/// Next pressed every `every` frames (held: key repeat) for four seconds
/// at 60 frames a second, in `design` at 1000 x 700; the window measured.
fn skip(dir: &Path, tracks: &[PathBuf], design: &str, every: usize) -> Skipped {
    let args: Vec<String> = tracks
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()));
    assert!(until(&mut h, 8.0, |a| a.is_loaded()));
    let d = axross_amp::design::DESIGNS
        .iter()
        .find(|d| d.key == design)
        .unwrap();
    h.state_mut().set_design(design);
    h.state_mut().set_theme(d.suggests);
    h.state_mut().set_mode("spectrum");
    settle(&mut h, 30);
    h.event(egui::Event::PointerGone);
    let shared = h.state().engine().shared.clone();
    let mut out = RenderState::default();
    let mut buf = vec![0.0f32; 800 * 2];
    let first = h.render().unwrap();
    let mut meter =
        flashmeter::Meter::with_fps(first.width() as usize, first.height() as usize, 60);
    let mut skips = 0;
    let mut last = h.state().snapshot().current;
    for f in 0..300 {
        if f % every == 0 && f < 240 {
            h.event(egui::Event::Key {
                key: egui::Key::N,
                physical_key: None,
                pressed: true,
                repeat: f > 0,
                modifiers: Default::default(),
            });
        }
        shared.render(&mut buf, 2, &mut out);
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(16));
        let img = h.render().unwrap();
        // (AMP_COVER_DUMP=dir: some of the frames kept as pictures there)
        if let Ok(d) = std::env::var("AMP_COVER_DUMP")
            && (f < 12 || f % 30 == 0)
        {
            img.save(format!("{d}/{design}-{every}-{f:03}.png"))
                .unwrap();
        }
        meter.push(&img);
        let now = h.state().snapshot().current;
        if now != last {
            skips += 1;
            last = now;
        }
    }
    let (transitions, _) = meter.most();
    let (strict, _) = meter.most_strict();
    Skipped {
        transitions,
        strict,
        area: meter.area,
        skips,
        covers_seen: h.state().has_cover(),
    }
}

#[test]
fn skipping_never_blinks_the_cover() {
    let Some(corpus) = std::env::var_os("AMP_CORPUS")
        .map(PathBuf::from)
        .filter(|c| c.join("tone.mp3").exists())
    else {
        eprintln!("AMP_CORPUS not set: the cover test is skipped");
        return;
    };
    let home = Scratch::new("cover-skip");
    let (white, black) = (png([255, 255, 255]), png([0, 0, 0]));
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for (case, covers, design, every) in [
        ("same white cover, Glass", [&white, &white], "glass", 2),
        ("white and black, Glass", [&white, &black], "glass", 8),
        ("white and black, Glass, held", [&white, &black], "glass", 2),
        ("white and black, Modern", [&white, &black], "modern", 8),
    ] {
        let dir = home.join(case.replace([' ', ','], "-"));
        std::fs::create_dir_all(&dir).unwrap();
        let tracks: Vec<PathBuf> = (0..8)
            .map(|k| {
                let p = dir.join(format!("t{k}.mp3"));
                with_cover(
                    &corpus.join("tone.mp3"),
                    covers[k % 2],
                    &format!("Track {k}"),
                    &p,
                );
                p
            })
            .collect();
        let r = skip(&dir, &tracks, design, every);
        let line = format!(
            "{case} (every {every} frames): transitions/s {} (strict {}) area {}, {} skips",
            r.transitions, r.strict, r.area, r.skips
        );
        println!("{line}");
        report.push(line.clone());
        // (it skipped, and the tracks had their covers)
        assert!(r.skips >= 8, "{line}: did not skip");
        assert!(r.covers_seen, "{line}: no cover shown");
        if r.transitions > 6 || r.strict > 6 || r.area > 6 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}\n{report:#?}");
}
