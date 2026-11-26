//! What plays, in every design: the texts of the display and now-playing
//! areas - the track's number, its name and artist, the position and the
//! length - fit their boxes in windows from 420 x 420 to 3000 x 1600, for
//! the first track of one and the 999th of 999, names of two letters, of a
//! hundred and fifty, in CJK, right to left, emoji and one word of 300
//! letters, tracks of 0:59, 59:59 and 10:00:00. No such text is cut by its
//! box, but a name too long for it, moved within it (src/marquee.rs: the
//! box itself in the window) - or, in reduced motion, ended in an ellipsis;
//! either way its whole is in a tooltip. And no two texts anywhere in the
//! window run into each other.
//! (0.16.0: in the Hi-fi rack's display the length and the name were cut
//! from track 2 on; in Modern a long title ran into the texts beside it.)

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup, clock};
use axross_amp::design::DESIGNS;
use axross_amp::state::Track;
use egui::epaint::Shape;
use egui::{Event, Pos2, Rect};
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
    let h = Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        // (no CJK or Arabic font is bundled: their letters are drawn as
        // boxes - they take room all the same)
        .allow_missing_glyphs()
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, Vec::new(), setup.clone()));
    axross_amp::marquee::record(&h.ctx, true);
    h
}

/// A text the frame painted: what it says, where its glyphs lie, what
/// clips it, whether it was cut to an ellipsis.
struct Painted {
    text: String,
    bounds: Rect,
    clip: Rect,
    elided: bool,
}

fn painted(h: &Harness<'static, App>) -> Vec<Painted> {
    fn walk(shape: &Shape, clip: Rect, out: &mut Vec<Painted>) {
        match shape {
            Shape::Text(t) => {
                // (a slanted text - the collage's scraps: what it covers,
                // turned about its position)
                let r = t.galley.rect;
                let (sin, cos) = t.angle.sin_cos();
                let turn =
                    |p: Pos2| t.pos + egui::vec2(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
                let corners = [
                    r.left_top(),
                    r.right_top(),
                    r.left_bottom(),
                    r.right_bottom(),
                ];
                let bounds = Rect::from_points(&corners.map(turn));
                out.push(Painted {
                    text: t.galley.job.text.clone(),
                    bounds,
                    clip,
                    elided: t.galley.elided,
                });
            }
            Shape::Vec(v) => {
                for s in v {
                    walk(s, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.output().shapes {
        walk(&c.shape, c.clip_rect, &mut out);
    }
    out
}

/// The 1 of 1, the 2 of 2 ... the 999 of 999.
const TRACKS: [usize; 5] = [1, 2, 10, 100, 999];
const LENGTHS: [f64; 3] = [59.0, 3599.0, 36000.0];
const SIZES: [(f32, f32); 9] = [
    // (the window's default size: review 17, M4)
    (620.0, 860.0),
    (420.0, 420.0),
    (600.0, 420.0),
    (420.0, 800.0),
    (800.0, 600.0),
    (1000.0, 700.0),
    (1400.0, 460.0),
    (1920.0, 1080.0),
    (3000.0, 1600.0),
];

/// Names and artists: short, long, in CJK, right to left, emoji, and one
/// word of 300 letters.
fn names() -> Vec<(String, String)> {
    vec![
        ("Qz".to_owned(), "Ok".to_owned()),
        (
            "A Very Long Title Of A Live Recording That Goes On And On, Part Two \
             (Remastered Deluxe Edition, With The Encore Left In)"
                .to_owned(),
            "The Somewhat Lengthily Named Orchestra Of Nowhere And Its Friends".to_owned(),
        ),
        (
            "東京の夜に降る雨の音を聴きながら眠れない夜を過ごす人々のための長い歌".to_owned(),
            "坂本龍一と東京フィルハーモニー交響楽団".to_owned(),
        ),
        (
            "אגדה על הים הגדול והספינה שלא חזרה הביתה לעולם - גרסה מורחבת".to_owned(),
            "فرقة الموسيقى العربية الكلاسيكية للحفلات الكبيرة".to_owned(),
        ),
        (
            "🎵🎶 Party 🎉🎉🎉 All Night 🌙✨ Long Long Long 🚀🚀🚀🚀🚀 Edition".to_owned(),
            "DJ 🎧 Emoji".to_owned(),
        ),
        ("W".repeat(300), "X".repeat(120)),
    ]
}

#[test]
fn what_plays_fits_its_box_in_every_design_and_size() {
    let mut failures = Vec::new();
    fit_at(&SIZES, true, &mut failures);
    assert!(
        failures.is_empty(),
        "{} texts cut or run together:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Every width from 400 to 900, 10 apart, at the default window's height:
/// the widest cases (review 17, M4: Brutalist at 600 - 630 points cut the
/// length, a width no size tried came near).
#[test]
fn what_plays_fits_its_box_at_every_width() {
    let sizes: Vec<(f32, f32)> = (400..=900).step_by(10).map(|w| (w as f32, 860.0)).collect();
    let mut failures = Vec::new();
    fit_at(&sizes, false, &mut failures);
    assert!(
        failures.is_empty(),
        "{} texts cut or run together:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The cases at `sizes`: `full`, every name through every number and
/// length; else the short name, the long one and the word of 300 letters,
/// at the widest (the 999th of 999, 10:00:00) and with one up next.
fn fit_at(sizes: &[(f32, f32)], full: bool, failures: &mut Vec<String>) {
    let only = std::env::var("FIT_ONLY").ok();
    for &(w, hgt) in sizes {
        let size = egui::vec2(w, hgt);
        // (a folder of its own: the two tests run side by side in one process,
        // and one's folder taken away took the other's settings with it)
        let home = Scratch::new(&format!("display-fit-{full}-{w}x{hgt}"));
        let mut h = window(home.path(), size);
        h.state_mut().set_mode("off");
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        for d in &DESIGNS {
            if only.as_deref().is_some_and(|o| o != d.key) {
                continue;
            }
            h.state_mut().set_design(d.key);
            h.run_steps(3);
            let names = names();
            for (k, (title, artist)) in names.iter().enumerate() {
                // (the first two names through every number and length;
                // the others at the widest: the 999th, 10:00:00)
                // (the n-th of n - and the first of a list, with one up next)
                if !full && ![0, 1, 5].contains(&k) {
                    continue;
                }
                let combos: Vec<(usize, usize, f64)> = if !full {
                    vec![(999, 999, 36000.0), (1, 2, 59.0)]
                } else if k < 2 {
                    TRACKS
                        .iter()
                        .flat_map(|&n| LENGTHS.iter().map(move |&l| (n, n, l)))
                        .chain([(1, 2, 59.0), (1, 999, 36000.0)])
                        .collect()
                } else {
                    vec![(999, 999, 36000.0), (1, 999, 36000.0)]
                };
                let mut tooltip_checked = false;
                for (n, of, secs) in combos {
                    // (the one playing, and the one after it, named so)
                    let tracks: Vec<Track> = (0..of)
                        .map(|i| {
                            let named = i + 1 == n || i == n;
                            Track {
                                path: format!("/m/{i:03}.mp3"),
                                title: if named {
                                    title.clone()
                                } else {
                                    format!("t{i}")
                                },
                                artist: if named { artist.clone() } else { String::new() },
                                album: if named {
                                    format!("{artist} - Album")
                                } else {
                                    String::new()
                                },
                                ..Default::default()
                            }
                        })
                        .collect();
                    let at = secs - 1.0;
                    h.state_mut().pose(tracks, n - 1, secs, at);
                    h.run_steps(3);
                    let case = format!(
                        "{} {}x{} track {n} of {of}, name {k} ({} letters), {}",
                        d.key,
                        w,
                        hgt,
                        title.chars().count(),
                        clock(secs)
                    );
                    // (FIT_SHOT=part of a case's name: its picture)
                    if let Ok(want) = std::env::var("FIT_SHOT")
                        && case.contains(&want)
                    {
                        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("target/screenshots/fit");
                        std::fs::create_dir_all(&dir).unwrap();
                        let name: String = case
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                            .collect();
                        h.render()
                            .unwrap()
                            .save(dir.join(format!("{name}.png")))
                            .unwrap();
                    }
                    check(
                        &mut h,
                        &case,
                        screen,
                        title,
                        artist,
                        at,
                        secs,
                        !tooltip_checked,
                        failures,
                    );
                    tooltip_checked = true;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check(
    h: &mut Harness<'static, App>,
    case: &str,
    screen: Rect,
    title: &str,
    artist: &str,
    at: f64,
    secs: f64,
    tooltip: bool,
    failures: &mut Vec<String>,
) {
    let (pos, total) = (clock(at), clock(secs));
    // (the texts of what plays: its name - the head of it, an ellipsis
    // may follow -, its artist, the position or the length)
    let head: String = title.chars().take(12).collect();
    let ours = |p: &Painted| {
        p.text.contains(&head)
            || p.text.contains(artist)
            || p.text.contains(&pos)
            || p.text.contains(&total)
    };
    let all = painted(h);
    // (the playlist's rows are cut at its edges as it scrolls: not ours to
    // hold to their boxes - they are held not to run into others below)
    let list = h.state().list_rect();
    let in_list = |p: &Painted| {
        list.is_positive()
            && (list.expand(2.0).contains(p.bounds.left_center())
                || (p.clip.is_positive() && list.expand(2.0).contains_rect(p.clip))
                || (!p.clip.is_positive() && list.expand(2.0).contains(p.clip.center())))
    };
    // (and a text in a box of no size - a row scrolled out of sight - is
    // not shown at all)
    let shown: Vec<&Painted> = all
        .iter()
        .filter(|p| ours(p) && !in_list(p) && p.clip.is_positive())
        .collect();
    if shown.iter().all(|p| !p.text.contains(&total)) {
        failures.push(format!("{case}: the length is shown nowhere"));
    }
    // (a name moved within its box: drawn in it, cut by it - the box is
    // held to the window, and the name to no other text)
    let moving = axross_amp::marquee::seen(&h.ctx);
    let moved = |p: &Painted| {
        moving.iter().any(|m| {
            !m.elided
                && m.rect.expand(3.0).contains_rect(p.clip)
                && p.bounds.y_range().intersects(m.rect.y_range())
        })
    };
    for m in &moving {
        if !screen.expand(1.0).contains_rect(m.rect) {
            failures.push(format!(
                "{case}: \"{}\" moves in a box out of the window: {:?}",
                short(&m.text),
                m.rect
            ));
        }
    }
    let mut elided = Vec::new();
    for m in moving.iter().filter(|m| !m.elided) {
        if !elided.iter().any(|(_, t): &(Pos2, String)| *t == m.text) {
            elided.push((m.rect.center(), m.text.clone()));
        }
    }
    for p in &shown {
        let inside = p.clip.intersect(screen).expand(1.0);
        let cut = !inside.contains_rect(p.bounds) && !moved(p);
        if p.elided {
            // (a name may end in an ellipsis - the time, never)
            if p.text.contains(&pos) || p.text.contains(&total) {
                failures.push(format!("{case}: \"{}\" cut to an ellipsis", p.text));
            } else if !elided.iter().any(|(_, t): &(Pos2, String)| *t == p.text) {
                elided.push((p.bounds.center(), p.text.clone()));
            }
        }
        if cut {
            failures.push(format!(
                "{case}: \"{}\" cut: its glyphs {:?}, its box {:?}",
                short(&p.text),
                p.bounds,
                p.clip
            ));
        }
    }
    // (no two texts anywhere in the window over each other - what is seen
    // of them, within their boxes: a text drawn twice, a shadow, or the
    // readout's unlit segments behind its digits, is one)
    let seen: Vec<(&Painted, Rect)> = all
        .iter()
        .map(|p| (p, p.bounds.intersect(p.clip).intersect(screen)))
        .filter(|(p, r)| r.is_positive() && !p.text.trim().is_empty())
        .collect();
    let ghost = |t: &str| -> String {
        t.chars()
            .map(|c| if c.is_ascii_digit() { '8' } else { c })
            .collect()
    };
    for (i, (a, ra)) in seen.iter().enumerate() {
        for (b, rb) in &seen[i + 1..] {
            if a.text == b.text || ghost(&a.text) == ghost(&b.text) {
                continue;
            }
            let o = ra.intersect(*rb);
            if o.width() > 1.5 && o.height() > 1.5 {
                failures.push(format!(
                    "{case}: \"{}\" runs into \"{}\" at {:?}",
                    short(&a.text),
                    short(&b.text),
                    o
                ));
            }
        }
    }
    // a text cut to an ellipsis: its whole in a tooltip
    let _ = title;
    for (at, text) in elided.into_iter().take(if tooltip { 3 } else { 0 }) {
        h.event(Event::PointerMoved(at));
        h.run_steps(30);
        let whole = painted(h)
            .iter()
            .any(|p| p.text.contains(&text) && !p.elided && p.clip.contains_rect(p.bounds));
        if !whole {
            failures.push(format!(
                "{case}: \"{}\" cut to an ellipsis, its whole in no tooltip",
                short(&text)
            ));
        }
        h.event(Event::PointerMoved(Pos2::new(-50.0, -50.0)));
        h.event(Event::PointerGone);
        h.run_steps(2);
    }
}

fn short(text: &str) -> String {
    let s: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        format!("{s}…")
    } else {
        s
    }
}
