//! The Cassette deck's cassette as a compact cassette is (IEC 60094): a
//! shell of 100.4 x 63.8 mm with rounded corners, the trapezoid at its
//! bottom edge with the head openings and the pinch-roller holes, two hubs
//! of six teeth 42 mm apart, the window between them showing the tape
//! packs - the left one shrinking as the track plays, the right one
//! growing -, the label above, the screws in the corners, the
//! write-protect tabs on the top edge. Checked as geometry at 420 x 420,
//! 800 x 600 and 1920 x 1080, at the start, the middle and the end of a
//! track; pictures in `target/screenshots/cassette/` (`CASSETTE_SHOTS=x`:
//! named x-...).

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::state::Track;
use egui_kittest::Harness;
use player::Scratch;

fn window(dir: &std::path::Path, size: egui::Vec2) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, Vec::new(), setup.clone()))
}

fn shots() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/cassette");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SIZES: [(f32, f32); 3] = [(420.0, 420.0), (800.0, 600.0), (1920.0, 1080.0)];

/// The cassette at every size, at the start, the middle and the end of a
/// track of five minutes: pictures.
#[test]
fn the_cassette_in_pictures() {
    let name = std::env::var("CASSETTE_SHOTS").unwrap_or_else(|_| "now".into());
    for (w, hgt) in SIZES {
        let home = Scratch::new("cassette-shots");
        let mut h = window(home.path(), egui::vec2(w, hgt));
        h.state_mut().set_design("cassette");
        h.run_steps(3);
        for (when, at) in [("start", 0.0), ("middle", 150.0), ("end", 299.0)] {
            let tracks = vec![Track {
                path: "/m/a.mp3".into(),
                title: "Side A".into(),
                artist: "The Tapes".into(),
                ..Default::default()
            }];
            h.state_mut().pose(tracks, 0, 300.0, at);
            h.run_steps(4);
            h.render()
                .unwrap()
                .save(shots().join(format!("{name}-{w}x{hgt}-{when}.png")))
                .unwrap();
        }
    }
}

/// The cassette's geometry at every size, through the side: a compact
/// cassette's proportions (0.16.0's was a box of 100 x 63 with a label
/// and two reels at its window's ends, over its edges).
#[test]
fn the_cassette_is_a_compact_cassette_at_every_size() {
    use egui_kittest::kittest::Queryable;
    let mut failures = Vec::new();
    for (w, hgt) in SIZES {
        let home = Scratch::new("cassette-geometry");
        let mut h = window(home.path(), egui::vec2(w, hgt));
        h.state_mut().set_design("cassette");
        h.run_steps(3);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, hgt));
        let mut areas = Vec::new();
        let mut packs = Vec::new();
        for at in [0.0, 150.0, 300.0] {
            let tracks = vec![Track {
                path: "/m/a.mp3".into(),
                title: "Side A".into(),
                artist: "The Tapes".into(),
                ..Default::default()
            }];
            h.state_mut().pose(tracks, 0, 300.0, at);
            h.run_steps(4);
            let size = format!("{w}x{hgt} at {at}");
            let Some(c) = h.state().cassette_drawn() else {
                failures.push(format!("{size}: no cassette"));
                continue;
            };
            let mut fail = |what: &str| failures.push(format!("{size}: {what}"));
            let ratio = c.shell.width() / c.shell.height();
            if (ratio - 100.4 / 63.8).abs() > 0.01 {
                fail(&format!("the shell {ratio:.3} to 1, not 1.574"));
            }
            if !screen.contains_rect(c.shell) {
                fail("the shell out of the window");
            }
            let [l, r] = c.hubs;
            if (l.y - r.y).abs() > 0.5 || ((l.x + r.x) / 2.0 - c.shell.center().x).abs() > 0.5 {
                fail("the hubs not symmetric");
            }
            let apart = (r.x - l.x) / c.shell.width();
            if (apart - 42.0 / 100.4).abs() > 0.005 {
                fail(&format!(
                    "the hubs {apart:.3} of the width apart, not 0.418"
                ));
            }
            for hub in [l, r] {
                let disc = egui::Rect::from_center_size(hub, egui::Vec2::splat(c.hub * 2.0));
                if !c.window.contains_rect(disc) {
                    fail("a hub not inside the window");
                }
            }
            for part in [c.paper, c.label, c.window] {
                if !c.shell.contains_rect(part) {
                    fail("the label or the window out of the shell");
                }
            }
            if c.label.bottom() > c.window.top() {
                fail("the label's words over the window");
            }
            for s in c.screws {
                let head = egui::Rect::from_center_size(s, egui::Vec2::splat(c.screw * 2.0));
                if !c.shell.contains_rect(head)
                    || head.intersects(c.window)
                    || head.intersects(c.label)
                {
                    fail("a screw out of the shell, or over the window or the words");
                }
            }
            for tab in c.tabs {
                if (tab.top() - c.shell.top()).abs() > 0.5 || !c.shell.contains_rect(tab) {
                    fail("a write-protect tab not on the top edge");
                }
            }
            if c.ramp.iter().any(|p| !c.shell.expand(0.5).contains(*p))
                || (c.ramp[2].y - c.shell.bottom()).abs() > 0.5
                || c.ramp[0].x <= c.ramp[3].x
            {
                fail("the trapezoid not on the bottom edge, narrower at its top");
            }
            let ramp = egui::Rect::from_points(&c.ramp);
            for o in c.openings {
                if !ramp.expand(0.5).contains_rect(o) {
                    fail("an opening out of the trapezoid");
                }
            }
            // (beside the counter, not under it)
            if let Some(counter) = h.query_by_label_contains("Counter ")
                && counter.rect().intersects(c.shell)
            {
                fail("the counter over the cassette");
            }
            areas.push(c.packs[0].powi(2) + c.packs[1].powi(2));
            packs.push(c.packs);
        }
        if let [start, middle, end] = packs[..] {
            if !(start[0] > start[1] && end[0] < end[1]) {
                failures.push(format!(
                    "{w}x{hgt}: the packs {start:?} at the start, {end:?} at the end"
                ));
            }
            if (middle[0] - middle[1]).abs() > 0.5 {
                failures.push(format!("{w}x{hgt}: the packs {middle:?} in the middle"));
            }
            if (start[0] - end[1]).abs() > 0.5 {
                failures.push(format!(
                    "{w}x{hgt}: a full pack {} at the start, {} at the end",
                    start[0], end[1]
                ));
            }
        }
        // (the tape's length the same: the packs' areas add up alike)
        if let (Some(lo), Some(hi)) = (
            areas.iter().cloned().reduce(f32::min),
            areas.iter().cloned().reduce(f32::max),
        ) && hi > lo * 1.01
        {
            failures.push(format!("{w}x{hgt}: the tape's length changes: {areas:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
