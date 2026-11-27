//! A title too long for its box is moved within it so all of it is read
//! (src/marquee.rs) - the user's own: "'Back To The 80's' _ Best of
//! Synthwave And Retro Electro Music Mix for 2 Hours _ Vol. 5", in every
//! design, at the default window and the smallest: over a round its first
//! letter and its last are each seen whole in its box, and the box runs
//! into no other text. It moves while paused (0.16.0's marquee stood cut),
//! starts again with a new track, holds under the pointer, moves the way
//! a right-to-left title is read, stands where it fits, and in reduced
//! motion is cut to an ellipsis instead. A screen reader is given it
//! whole. A bright title moving on the dark fluorescent display flashes
//! nothing by either meter.
//!
//! `MARQUEE_SHOTS=1`: pictures of the rack, Modern, the 90s desktop and
//! the terminal at several moments, and a sheet of them, in
//! `target/screenshots/marquee/`.

#[path = "support/flashmeter.rs"]
mod flashmeter;
#[path = "support/m11.rs"]
mod m11;
#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::design::DESIGNS;
use axross_amp::marquee;
use axross_amp::state::Track;
use egui::epaint::Shape;
use egui::{Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use player::Scratch;

const TITLE: &str =
    "'Back To The 80's' _ Best of Synthwave And Retro Electro Music Mix for 2 Hours _ Vol. 5";
const DEFAULT: egui::Vec2 = egui::vec2(620.0, 860.0);
const SMALLEST: egui::Vec2 = egui::vec2(420.0, 420.0);

fn window(dir: &std::path::Path, size: egui::Vec2, dt: f32, gpu: bool) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    // (no Hebrew font is bundled: its letters drawn as boxes)
    let b = Harness::builder()
        .with_size(size)
        .with_step_dt(dt)
        .allow_missing_glyphs();
    let mut h = if gpu {
        b.wgpu()
            .build_eframe(move |cc| App::with_setup(cc, Vec::new(), setup.clone()))
    } else {
        b.build_eframe(move |cc| App::with_setup(cc, Vec::new(), setup.clone()))
    };
    marquee::record(&h.ctx, true);
    // (the desktop's word on motion: none - the test's own)
    h.state_mut().set_desktop_reduces_motion(false);
    h.state_mut().set_mode("off");
    h
}

/// `title` by `artist` playing, the 1st of 2, its picture off.
fn pose(h: &mut Harness<'static, App>, title: &str, artist: &str) {
    let tracks = vec![
        Track {
            path: format!("/m/{}.mp3", title.len()),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: String::new(),
            ..Default::default()
        },
        Track {
            path: "/m/next.mp3".to_owned(),
            title: "Next".to_owned(),
            ..Default::default()
        },
    ];
    h.state_mut().pose(tracks, 0, 3600.0, 12.0);
}

/// When the window asked to be drawn next.
fn delay_of(h: &Harness<'static, App>) -> std::time::Duration {
    h.output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map_or(std::time::Duration::MAX, |v| v.repaint_delay)
}

/// A text painted: what it says, where it was put, its galley, its clip.
struct Painted {
    text: String,
    pos: Pos2,
    galley: std::sync::Arc<egui::Galley>,
    clip: Rect,
    angle: f32,
}

fn painted(h: &Harness<'static, App>) -> Vec<Painted> {
    fn walk(shape: &Shape, clip: Rect, out: &mut Vec<Painted>) {
        match shape {
            Shape::Text(t) => out.push(Painted {
                text: t.galley.job.text.clone(),
                pos: t.pos,
                galley: t.galley.clone(),
                clip,
                angle: t.angle,
            }),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.output().shapes {
        walk(&c.shape, c.clip_rect, &mut out);
    }
    out
}

/// Whether the title's first letter and its last are seen whole this
/// frame, outside the playlist (its glyphs within what clips them, in the
/// window); the collage's scraps: its first word's, its last word's.
fn ends_seen(h: &Harness<'static, App>, screen: Rect) -> (bool, bool) {
    let list = h.state().list_rect();
    let want = TITLE.to_uppercase();
    let (first_word, last_word) = (
        want.split_whitespace().next().unwrap().to_owned(),
        want.split_whitespace().last().unwrap().to_owned(),
    );
    let (mut first, mut last) = (false, false);
    let all = painted(h);
    for p in &all {
        // (a text moved is drawn in strips, each its own clip, the edges
        // fading: what clips it is all of them)
        let clip = all
            .iter()
            .filter(|q| q.pos == p.pos && std::sync::Arc::ptr_eq(&q.galley, &p.galley))
            .fold(Rect::NOTHING, |r, q| r.union(q.clip));
        let view = clip.intersect(screen).expand(0.5);
        if !view.is_positive() || (list.is_positive() && list.expand(2.0).contains(p.pos)) {
            continue;
        }
        if p.angle != 0.0 || p.text.trim() == "5" || p.text.trim() == "'BACK" {
            // (a scrap: whole, or not at all)
            let whole = view.contains_rect(p.galley.rect.translate(p.pos.to_vec2()).shrink(4.0));
            let t = p.text.trim().to_uppercase();
            first |= whole && t == first_word;
            last |= whole && t == last_word;
            continue;
        }
        for row in &p.galley.rows {
            let seen: String = row.glyphs.iter().map(|g| g.chr).collect();
            let Some(at) = seen.to_uppercase().find(&want) else {
                continue;
            };
            let k = seen[..at].chars().count();
            let n = want.chars().count();
            let shows = |i: usize| {
                let g = &row.glyphs[i];
                let x0 = p.pos.x + row.pos.x + g.pos.x;
                let x1 = x0 + g.advance_width;
                let y = p.pos.y + row.pos.y + row.size.y * 0.5;
                view.x_range().contains(x0 + 0.5)
                    && view.x_range().contains(x1 - 0.5)
                    && view.y_range().contains(y)
            };
            first |= shows(k);
            last |= shows(k + n - 1);
        }
    }
    (first, last)
}

/// The boxes the title moves in run into no other text (what is seen of
/// it), and lie in the window.
fn overlaps(h: &Harness<'static, App>, screen: Rect) -> Vec<String> {
    let moving = marquee::seen(&h.ctx);
    let list = h.state().list_rect();
    let mut out = Vec::new();
    let words: Vec<String> = TITLE
        .to_uppercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let ours = |t: &str| {
        let u = t.to_uppercase();
        if words.iter().any(|w| *w == u.trim()) {
            return true;
        }
        u.contains("SYNTHWAVE")
            || u.contains("BACK")
            || u.trim().is_empty()
            || u.trim() == "•"
            || u.trim() == "***"
            || u.chars().all(|c| c == '8' || !c.is_ascii_digit()) && u.contains("8")
    };
    // (a box moved in, waiting at an end or moving)
    for m in moving.iter().filter(|m| !m.elided) {
        if !screen.expand(1.0).contains_rect(m.rect) {
            out.push(format!("its box {:?} out of the window", m.rect));
        }
        for p in painted(h) {
            if ours(&p.text) || (list.is_positive() && list.contains(p.pos)) {
                continue;
            }
            let seen = p
                .galley
                .rect
                .translate(p.pos.to_vec2())
                .intersect(p.clip)
                .intersect(screen);
            let o = seen.intersect(m.rect);
            if o.width() > 1.5 && o.height() > 1.5 {
                out.push(format!(
                    "its box {:?} runs into \"{}\" at {o:?}",
                    m.rect, p.text
                ));
            }
        }
    }
    out
}

/// The title in `design` at `size`: over a round it moves, its first and
/// last letters each seen; nothing run into. Failures said.
fn read_whole(design: &str, size: egui::Vec2) -> Vec<String> {
    let home = Scratch::new("marquee-whole");
    let mut h = window(home.path(), size, 0.1, false);
    h.state_mut().set_design(design);
    h.run_steps(3);
    pose(&mut h, TITLE, "Synthwave Nights");
    h.run_steps(2);
    let screen = Rect::from_min_size(Pos2::ZERO, size);
    let case = format!("{design} {}x{}", size.x, size.y);
    let mut failures = Vec::new();
    let (start_first, start_last) = ends_seen(&h, screen);
    if !start_first {
        failures.push(format!("{case}: its start not seen at first"));
    }
    if start_last {
        failures.push(format!("{case}: its end seen at first - it fits?"));
    }
    let (mut last_seen, mut first_hidden, mut back) = (false, false, false);
    let mut moved = false;
    // (60 s: the slowest round - the display's small type a cell at a time)
    for step in 0..600 {
        h.run_steps(1);
        let (f, l) = ends_seen(&h, screen);
        last_seen |= l;
        first_hidden |= !f;
        back |= last_seen && f && !l;
        moved |= marquee::seen(&h.ctx).iter().any(|m| m.moving);
        if step % 25 == 0 {
            for o in overlaps(&h, screen) {
                failures.push(format!("{case} at {:.1} s: {o}", step as f32 * 0.1));
            }
        }
        if back && step > 40 {
            break;
        }
    }
    if !moved {
        failures.push(format!("{case}: it never moved"));
    }
    if !last_seen {
        failures.push(format!("{case}: its end never seen"));
    }
    if !first_hidden {
        failures.push(format!("{case}: its start never left"));
    }
    if !back {
        failures.push(format!("{case}: it never came back to its start"));
    }
    failures.dedup();
    failures
}

#[test]
fn the_users_title_is_read_whole_in_every_design_at_the_default_size() {
    let failures: Vec<String> = DESIGNS
        .iter()
        .flat_map(|d| read_whole(d.key, DEFAULT))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_users_title_is_read_whole_in_every_design_at_the_smallest_size() {
    let failures: Vec<String> = DESIGNS
        .iter()
        .flat_map(|d| read_whole(d.key, SMALLEST))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where the title's box is, and how far it has moved (Modern's card).
fn offset(h: &Harness<'static, App>) -> Option<(Rect, f32, bool)> {
    marquee::seen(&h.ctx)
        .into_iter()
        .find(|m| m.text.contains("Synthwave") || m.text.contains("שיר"))
        .map(|m| (m.rect, m.offset, m.moving))
}

#[test]
fn it_moves_on_while_paused() {
    let home = Scratch::new("marquee-paused");
    let file = home.join(format!("{TITLE}.wav"));
    player::write_wav(&file, 20.0);
    let setup = Setup {
        config_dir: home.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    let args = vec![file.to_string_lossy().into_owned()];
    let mut h = Harness::builder()
        .with_size(DEFAULT)
        .with_step_dt(0.05)
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()));
    marquee::record(&h.ctx, true);
    h.state_mut().set_desktop_reduces_motion(false);
    h.state_mut().set_mode("off");
    assert!(player::until(&mut h, 8.0, |a| a.is_loaded()));
    if h.state().playing() {
        h.key_press(egui::Key::Space);
        assert!(player::until(&mut h, 4.0, |a| !a.playing()));
    }
    assert!(h.state().is_loaded() && !h.state().playing(), "paused");
    // 8 s of the window, paused: it waits, then moves
    let mut offsets = Vec::new();
    for _ in 0..160 {
        h.run_steps(1);
        if let Some((_, o, _)) = offset(&h) {
            offsets.push(o);
        }
    }
    assert!(!h.state().playing());
    let mut distinct = offsets.clone();
    distinct.dedup();
    assert!(
        distinct.len() > 20 && offsets.iter().any(|o| *o > 100.0),
        "it stood while paused: {distinct:?}"
    );
}

#[test]
fn it_starts_again_with_a_new_track() {
    let home = Scratch::new("marquee-new-track");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.run_steps(3);
    pose(&mut h, TITLE, "Synthwave Nights");
    h.run_steps(140);
    let (_, o, _) = offset(&h).expect("moving");
    assert!(o > 50.0, "{o}");
    let other = format!("{TITLE} (Synthwave Extended Remix, part two)");
    pose(&mut h, &other, "Synthwave Nights");
    // (from its start: still there two seconds)
    for _ in 0..35 {
        h.run_steps(1);
        let (_, o, moving) = offset(&h).expect("shown");
        assert!(o == 0.0 && !moving, "{o}");
    }
    h.run_steps(20);
    let (_, o, _) = offset(&h).expect("shown");
    assert!(o > 0.0);
}

#[test]
fn the_pointer_over_it_holds_it_its_whole_in_a_tooltip() {
    let home = Scratch::new("marquee-hover");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.run_steps(3);
    pose(&mut h, TITLE, "Synthwave Nights");
    h.run_steps(70);
    let (rect, before, _) = offset(&h).expect("moving");
    h.event(egui::Event::PointerMoved(rect.center()));
    h.run_steps(1);
    let (_, held, _) = offset(&h).unwrap();
    h.run_steps(40);
    let (_, still, _) = offset(&h).unwrap();
    assert_eq!(held, still, "held under the pointer (from {before})");
    assert!(
        painted(&h).iter().any(|p| p.text.contains(TITLE)
            && p.clip
                .contains_rect(p.galley.rect.translate(p.pos.to_vec2()))),
        "its whole in a tooltip"
    );
    h.event(egui::Event::PointerMoved(Pos2::new(5.0, DEFAULT.y - 5.0)));
    h.run_steps(20);
    let (_, after, _) = offset(&h).unwrap();
    assert_ne!(after, still, "on again once the pointer has gone");
}

#[test]
fn a_title_that_fits_stands_still_and_wakes_nothing() {
    let home = Scratch::new("marquee-fits");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    for d in &DESIGNS {
        h.state_mut().set_design(d.key);
        h.run_steps(3);
        pose(&mut h, "Qz", "Ok");
        h.run_steps(3);
        let at = |h: &Harness<'static, App>| -> Vec<Pos2> {
            painted(h)
                .iter()
                .filter(|p| p.text.contains("Qz"))
                .map(|p| p.pos)
                .collect()
        };
        let first = at(&h);
        assert!(!first.is_empty(), "{}: shown", d.key);
        for _ in 0..60 {
            h.run_steps(1);
            let seen = marquee::seen(&h.ctx);
            assert!(seen.is_empty(), "{}: nothing moves: {seen:?}", d.key);
        }
        assert_eq!(at(&h), first, "{}: where it was", d.key);
        // (no frame asked for by it: the picture off, nothing playing)
        let delay = delay_of(&h);
        assert!(
            delay > std::time::Duration::from_millis(250),
            "{}: {delay:?}",
            d.key
        );
    }
}

#[test]
fn moving_it_asks_for_frames_at_a_calm_rate_and_waits_without() {
    let home = Scratch::new("marquee-rate");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.run_steps(3);
    pose(&mut h, TITLE, "Synthwave Nights");
    h.run_steps(2);
    // waiting at its start: woken when the wait ends, not before
    let delay = delay_of(&h).as_secs_f32();
    assert!(delay > 1.0, "{delay}");
    h.run_steps(50);
    assert!(offset(&h).unwrap().2, "moving");
    let delay = delay_of(&h).as_secs_f32();
    assert!((1.0 / 31.0..=1.0 / 19.0).contains(&delay), "{delay}");
    // the rack's display: a cell at a time - woken for the next cell
    h.state_mut().set_design("rack");
    h.run_steps(3);
    h.run_steps(50);
    let delay = delay_of(&h).as_secs_f32();
    assert!(delay >= 1.0 / 20.0, "{delay}");
    // a slow renderer (the CPU draws): in jumps, a frame every few seconds
    h.state_mut().set_slow_renderer(true);
    h.state_mut().set_design("modern");
    h.run_steps(3);
    pose(&mut h, &format!("{TITLE} (again)"), "Synthwave Nights");
    let mut most = 0.0f32;
    let mut least = f32::MAX;
    for _ in 0..120 {
        h.run_steps(1);
        if offset(&h).is_some_and(|o| o.2) {
            let d = delay_of(&h).as_secs_f32();
            least = least.min(d);
            most = most.max(d);
        }
    }
    assert!(least >= 1.0 / 60.0 && most >= 1.0, "{least} - {most}");
    // (and it gets to its end: a round of 30 s more)
    let mut end = false;
    for _ in 0..600 {
        h.run_steps(1);
        end |= offset(&h).is_some_and(|o| o.1 > 0.0 && !o.2 && o.1 > 100.0);
    }
    assert!(end, "it got to its end");
}

#[test]
fn a_minimised_window_is_not_woken_for_it() {
    let home = Scratch::new("marquee-minimised");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.run_steps(3);
    pose(&mut h, TITLE, "Synthwave Nights");
    h.run_steps(60);
    assert!(delay_of(&h).as_secs_f32() < 0.1);
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .minimized = Some(true);
    h.run_steps(2);
    assert!(delay_of(&h).as_secs_f32() > 0.25, "{:?}", delay_of(&h));
}

#[test]
fn a_right_to_left_title_starts_at_the_right_and_moves_to_it() {
    let home = Scratch::new("marquee-rtl");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.run_steps(3);
    let title = "שיר ארוך מאוד על הים הגדול והספינה שלא חזרה הביתה לעולם, גרסה מורחבת";
    pose(&mut h, title, "");
    h.run_steps(2);
    let (rect, _, _) = offset(&h).expect("too long for its box");
    // where the title's galley lies (Modern's card, its largest type)
    let x = |h: &Harness<'static, App>| -> (f32, f32) {
        painted(h)
            .iter()
            .filter(|p| p.text.contains("שיר") && rect.expand(3.0).contains_rect(p.clip))
            .map(|p| (p.pos.x, p.pos.x + p.galley.size().x))
            .next()
            .unwrap()
    };
    let (_, right) = x(&h);
    assert!(
        (right - rect.right()).abs() < 1.5,
        "its start at the right: {right} {rect:?}"
    );
    let mut last = x(&h).0;
    let mut moved = false;
    for _ in 0..200 {
        h.run_steps(1);
        let (left, _) = x(&h);
        assert!(
            left >= last - 0.01,
            "moving to the right only: {left} after {last}"
        );
        moved |= left > last;
        last = left;
        if (left - rect.left()).abs() < 1.0 {
            break;
        }
    }
    assert!(
        moved && (last - rect.left()).abs() < 1.0,
        "its end at the left: {last} {rect:?}"
    );
}

#[test]
fn in_reduced_motion_it_is_cut_to_an_ellipsis_and_stands() {
    let home = Scratch::new("marquee-reduced");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.state_mut().set_desktop_reduces_motion(true);
    for d in &DESIGNS {
        h.state_mut().set_design(d.key);
        h.run_steps(3);
        pose(&mut h, TITLE, "Synthwave Nights");
        h.run_steps(3);
        let first: Vec<Pos2> = painted(&h).iter().map(|p| p.pos).collect();
        h.run_steps(100);
        let seen = marquee::seen(&h.ctx);
        assert!(seen.iter().all(|m| !m.moving), "{}: nothing moves", d.key);
        if d.key != "collage" {
            assert!(
                seen.iter().any(|m| m.elided),
                "{}: cut to an ellipsis",
                d.key
            );
        }
        let now: Vec<Pos2> = painted(&h).iter().map(|p| p.pos).collect();
        assert_eq!(first, now, "{}: all where it was", d.key);
    }
}

/// Options' switch (WCAG 2.2.2; a desktop that says nothing of reduced
/// motion left no way to stop it - review of 0.17.1, M5): off, nothing
/// moves - cut to an ellipsis, as in reduced motion.
#[test]
fn the_windows_own_switch_stops_every_title() {
    let home = Scratch::new("marquee-switch");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.state_mut().set_scroll_titles(false);
    for d in &DESIGNS {
        h.state_mut().set_design(d.key);
        h.run_steps(3);
        pose(&mut h, TITLE, "Synthwave Nights");
        h.run_steps(3);
        let first: Vec<Pos2> = painted(&h).iter().map(|p| p.pos).collect();
        h.run_steps(100);
        let seen = marquee::seen(&h.ctx);
        assert!(seen.iter().all(|m| !m.moving), "{}: nothing moves", d.key);
        let now: Vec<Pos2> = painted(&h).iter().map(|p| p.pos).collect();
        assert_eq!(first, now, "{}: all where it was", d.key);
    }
}

/// A click on a moving title stops it - also once the pointer has gone -,
/// another sets it moving again.
#[test]
fn a_click_stops_a_title_and_another_sets_it_moving() {
    let home = Scratch::new("marquee-click");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    h.run_steps(3);
    pose(&mut h, TITLE, "Synthwave Nights");
    h.run_steps(70);
    let (rect, _, _) = offset(&h).expect("moving");
    let click = |h: &mut Harness<'static, App>| {
        let at = rect.center();
        h.event(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run_steps(2);
        h.event(egui::Event::PointerMoved(Pos2::new(5.0, DEFAULT.y - 5.0)));
        h.run_steps(2);
    };
    click(&mut h);
    let (_, stopped, _) = offset(&h).unwrap();
    h.run_steps(60);
    assert_eq!(offset(&h).unwrap().1, stopped, "stopped, the pointer gone");
    click(&mut h);
    h.run_steps(60);
    assert_ne!(offset(&h).unwrap().1, stopped, "moving again");
}

#[test]
fn a_screen_reader_is_given_the_whole_title() {
    let home = Scratch::new("marquee-reader");
    let mut h = window(home.path(), DEFAULT, 0.05, false);
    for d in ["modern", "glass", "terminal", "brutalist"] {
        h.state_mut().set_design(d);
        h.run_steps(3);
        pose(&mut h, TITLE, "Synthwave Nights");
        h.run_steps(60);
        assert!(
            h.query_all_by_label_contains(TITLE).next().is_some(),
            "{d}: no node named with the whole title"
        );
    }
    for d in ["rack", "desktop90", "cassette", "collage"] {
        h.state_mut().set_design(d);
        h.run_steps(3);
        h.run_steps(60);
        assert!(
            h.query_all_by_label_contains(TITLE).next().is_some(),
            "{d}: no node named with the whole title"
        );
    }
}

/// Both meters, as tests/window_flash.rs reads them.
fn flashes(design: &str, theme: &str, size: egui::Vec2, slow: bool) -> String {
    let home = Scratch::new("marquee-flash");
    let mut h = window(home.path(), size, 1.0 / 60.0, true);
    // (llvmpipe here: a slow renderer, every design moved in jumps; else
    // as on a GPU, smoothly or by cells, the edges fading)
    h.state_mut().set_slow_renderer(slow);
    h.state_mut().set_design(design);
    h.state_mut().set_theme(theme);
    h.run_steps(40);
    pose(&mut h, TITLE, "SYNTHWAVE NIGHTS 1984 - 2084 (EXTENDED)");
    h.run_steps(2);
    let (w, ht) = (size.x as usize, size.y as usize);
    let mut m = m11::M11::new(w, ht, 60.0);
    let mut p = flashmeter::Meter::with_fps(w, ht, 60);
    let mut moved = 0;
    // (12 s: the wait, the move, the wait at its end, on round)
    for _ in 0..720 {
        h.run_steps(1);
        let img = h.render().unwrap();
        m.push_rgba(&img);
        p.push(&img);
        moved += marquee::seen(&h.ctx)
            .iter()
            .filter(|m| m.moving)
            .count()
            .min(1);
    }
    assert!(moved > 200, "{design} {size:?}: it moved ({moved} frames)");
    let r = m.reading();
    let (l, red) = p.most();
    let (sl, sred) = p.most_strict();
    let all = r.c[0] + r.c[1] + l + red + sl + sred + p.area + p.area_red;
    if all == 0 && !r.strict_fails() {
        String::new()
    } else {
        format!(
            "{design} {theme} {size:?} slow {slow}: m11 {} red {} (P {:?}); project {l} red {red}, strict {sl} red {sred}, area {} red {}",
            r.c[0], r.c[1], r.p, p.area, p.area_red
        )
    }
}

#[test]
fn a_bright_title_moving_on_the_dark_display_flashes_nothing() {
    let mut said = Vec::new();
    for (design, theme, slow) in [
        ("rack", "midnight", false),
        (
            "rack",
            DESIGNS.iter().find(|d| d.key == "rack").unwrap().suggests,
            false,
        ),
        ("desktop90", "midnight", false),
        ("modern", "midnight", false),
        ("modern", "midnight", true),
    ] {
        for size in [SMALLEST, egui::vec2(640.0, 480.0)] {
            let s = flashes(design, theme, size, slow);
            if !s.is_empty() {
                said.push(s);
            }
        }
    }
    assert!(said.is_empty(), "{}", said.join("\n"));
}

/// Pictures at several moments of a round, and a sheet of them
/// (MARQUEE_SHOTS=1).
#[test]
fn pictures_of_it_moving() {
    if std::env::var_os("MARQUEE_SHOTS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/marquee");
    std::fs::create_dir_all(&dir).unwrap();
    let moments = [1.0f32, 4.0, 8.0, 12.0, 16.0, 22.0];
    let designs = [
        "rack",
        "modern",
        "desktop90",
        "terminal",
        "collage",
        "cassette",
    ];
    let mut rows = Vec::new();
    for (d, size) in designs
        .iter()
        .flat_map(|d| [(*d, DEFAULT), (*d, egui::vec2(620.0, 520.0))])
    {
        let home = Scratch::new("marquee-shots");
        let mut h = window(home.path(), size, 0.05, true);
        // (as on a GPU: smoothly)
        h.state_mut().set_slow_renderer(false);
        h.state_mut().set_design(d);
        h.run_steps(40);
        pose(&mut h, TITLE, "Synthwave Nights");
        let mut t = 0.0;
        let mut row = Vec::new();
        for &at in &moments {
            while t < at {
                h.run_steps(1);
                t += 0.05;
            }
            let img = h.render().unwrap();
            img.save(dir.join(format!("{d}-{}-{at:04.1}s.png", size.y)))
                .unwrap();
            row.push(img);
        }
        rows.push(row);
    }
    // the sheet: the top part of each picture (what plays), a row a design
    let (cw, ch) = (620u32, 300u32);
    let scale = 2;
    let mut sheet = image::RgbaImage::from_pixel(
        cw / scale * moments.len() as u32,
        ch / scale * rows.len() as u32,
        image::Rgba([0, 0, 0, 255]),
    );
    for (r, row) in rows.iter().enumerate() {
        for (c, img) in row.iter().enumerate() {
            let top = image::imageops::crop_imm(img, 0, 0, cw, ch).to_image();
            let small = image::imageops::resize(
                &top,
                cw / scale,
                ch / scale,
                image::imageops::FilterType::Triangle,
            );
            image::imageops::overlay(
                &mut sheet,
                &small,
                (c as u32 * cw / scale) as i64,
                (r as u32 * ch / scale) as i64,
            );
        }
    }
    sheet.save(dir.join("sheet.png")).unwrap();
}

/// What a frame of the window costs the CPU, laid out and tessellated
/// (no renderer): the cost of each frame a moving title asks for
/// (MARQUEE_COST=1 prints it).
#[test]
fn what_a_frame_costs() {
    if std::env::var_os("MARQUEE_COST").is_none() {
        return;
    }
    for d in ["modern", "rack", "desktop90", "terminal", "collage"] {
        let home = Scratch::new("marquee-cost");
        let mut h = window(home.path(), DEFAULT, 1.0 / 25.0, false);
        h.state_mut().set_design(d);
        h.run_steps(40);
        pose(&mut h, TITLE, "Synthwave Nights");
        h.run_steps(60);
        let start = std::time::Instant::now();
        let n = 500;
        for _ in 0..n {
            h.run_steps(1);
            let out = h.output().shapes.clone();
            let _ = h.ctx.tessellate(out, 1.0);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("{d}: {ms:.2} ms a frame (laid out and tessellated)");
    }
}
