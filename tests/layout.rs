//! The playlist never vanishes: with 3, 30 and 300 tracks, from the
//! smallest window to 2560 x 1440, its list keeps at least three rows'
//! height inside the window and the playing track's row is in sight; with
//! the picture large (double-click, F11) the playlist is a click away and
//! so is the way back. Every case is rendered to
//! `target/screenshots/layout/` to be looked at.

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use player::{Scratch, settle, until};
use std::path::{Path, PathBuf};

/// The least height the playlist's list keeps (about three rows).
const LEAST: f32 = 150.0;
/// The status line at the window's foot.
const STATUS: f32 = 26.0;

fn shots() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/layout");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn window(dir: &Path, args: Vec<String>, size: egui::Vec2) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

/// `n` short tracks, "t000.wav" ...
fn tracks(dir: &Path, n: usize) -> Vec<String> {
    (0..n)
        .map(|k| {
            let p = dir.join(format!("t{k:03}.wav"));
            player::write_wav_rate(&p, 8000, 0.5);
            p.to_string_lossy().into_owned()
        })
        .collect()
}

#[test]
fn the_playlist_stays_in_sight_at_every_size() {
    let home = Scratch::new("layout");
    let mut failures = Vec::new();
    for n in [3usize, 30, 300] {
        let dir = home.join(format!("n{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let files = tracks(&dir, n);
        for (w, h) in [
            (420.0, 640.0),
            (620.0, 860.0),
            (1200.0, 980.0),
            (1920.0, 1080.0),
            (2560.0, 1440.0),
        ] {
            let size = egui::vec2(w, h);
            let mut win = window(&dir.join(format!("{w}x{h}")), files.clone(), size);
            assert!(until(&mut win, 8.0, |a| a.is_loaded()));
            settle(&mut win, 6);
            let name = format!("n{n}-{w}x{h}");
            // normal: the list's top (its filter) leaves it room to the foot
            let ok = match win.query_by_label("Filter the playlist") {
                None => Err("no playlist".to_owned()),
                Some(filter) => {
                    let room = h - STATUS - filter.rect().bottom();
                    if room < LEAST {
                        Err(format!("the list has {room:.0} px"))
                    } else {
                        Ok(())
                    }
                }
            };
            if let Err(why) = ok {
                failures.push(format!("{name}: {why}"));
            }
            // the playing track's row in sight (below the list's filter: not
            // the title above)
            let below = win
                .query_by_label("Filter the playlist")
                .map_or(f32::MAX, |f| f.rect().bottom());
            let row = win
                .query_all_by_label("t000")
                .map(|n| n.rect())
                .find(|r| r.top() >= below - 1.0);
            match row {
                Some(r) if r.bottom() <= h - STATUS => {}
                Some(r) => failures.push(format!("{name}: the playing row at {r:?}")),
                None => failures.push(format!("{name}: the playing row is not shown")),
            }
            // a wide window: a column of a readable width in the middle (not
            // a card stretched across it)
            if w > 1240.0 {
                let picture = win.get_by_label_contains("Visualization: ").rect();
                if picture.width() > 1190.0 || (picture.center().x - w / 2.0).abs() > 4.0 {
                    failures.push(format!("{name}: the picture {picture:?} not in a column"));
                }
                // (and the toolbar over the column, not across the screen)
                let left = win
                    .query_all_by_label_contains("Files")
                    .map(|n| n.rect().left())
                    .fold(f32::MAX, f32::min);
                if left == f32::MAX || left < picture.left() - 8.0 {
                    failures.push(format!(
                        "{name}: the toolbar's Files at {left:.0}, the column at {:.0}",
                        picture.left()
                    ));
                }
            }
            win.render()
                .unwrap()
                .save(shots().join(format!("{name}.png")))
                .unwrap();
            // the picture large: the playlist is a click away, and so is
            // the way back
            win.key_press(egui::Key::F11);
            settle(&mut win, 4);
            assert!(win.state().big_visual(), "{name}: F11");
            win.render()
                .unwrap()
                .save(shots().join(format!("{name}-large.png")))
                .unwrap();
            let window_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for label in ["Show the playlist", "Normal size"] {
                match win.query_by_label_contains(label) {
                    Some(b) if window_rect.contains_rect(b.rect()) => {}
                    _ => failures.push(format!("{name} large: no \"{label}\" in sight")),
                }
            }
            if let Some(b) = win.query_by_label_contains("Show the playlist") {
                b.click();
                settle(&mut win, 4);
                let shown = win
                    .query_all_by_label("t000")
                    .filter(|n| window_rect.contains_rect(n.rect()))
                    .count();
                if shown < 2 {
                    failures.push(format!("{name} large: the playlist did not come"));
                }
                win.render()
                    .unwrap()
                    .save(shots().join(format!("{name}-large-list.png")))
                    .unwrap();
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
