//! The faces' menu (⚙ in the top row, a right click on the picture): a face
//! chosen there is the face shown, in every design, by the pointer and by
//! the keys; the menu of 69 faces is whole and reachable in the least
//! window; what a shut menu leaves a moment takes the clicks (review 18:
//! H1, M3, L3). Its submenus open beside their buttons at every size and a
//! click on a submenu's button changes nothing; the arrows go across the
//! grid's columns (review 19: H1, M2, L1).
#[path = "support/player.rs"]
mod player;

use axross_amp::app::App;
use axross_amp::design::DESIGNS;
use axross_amp::viz::FACES;
use egui::accesskit::{Role, Toggled};
use egui::{Event, PointerButton, Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use player::{Scratch, harness_sized, settle, until, write_chord};

/// The least window (main.rs) and the default one.
const LEAST: egui::Vec2 = egui::vec2(420.0, 420.0);
const DEFAULT: egui::Vec2 = egui::vec2(620.0, 860.0);

fn window(name: &str, design: &str, size: egui::Vec2) -> (Scratch, Harness<'static, App>) {
    let home = Scratch::new(name);
    let a = home.join("First.wav");
    write_chord(&a, 40);
    let mut h = harness_sized(&home, vec![a.to_string_lossy().into_owned()], size);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.state_mut().set_design(design);
    settle(&mut h, 3);
    (home, h)
}

fn press(h: &mut Harness<'static, App>, at: Pos2, button: PointerButton) {
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Default::default(),
        });
    }
}

fn click_at(h: &mut Harness<'static, App>, at: Pos2) {
    h.event(Event::PointerMoved(at));
    h.run_steps(2);
    press(h, at, PointerButton::Primary);
}

/// The menu's rows: a face's name, where it is, whether it has the focus,
/// whether it is the one ticked.
fn rows(h: &Harness<'static, App>) -> Vec<(&'static str, Rect, bool, bool)> {
    FACES
        .iter()
        .filter_map(|f| {
            h.query_all_by_role(Role::RadioButton)
                .find(|n| n.accesskit_node().label().as_deref() == Some(f.name))
                .map(|n| {
                    (
                        f.name,
                        n.rect(),
                        n.is_focused(),
                        n.accesskit_node().toggled() == Some(Toggled::True),
                    )
                })
        })
        .collect()
}

fn row(h: &Harness<'static, App>, name: &str) -> Rect {
    rows(h)
        .into_iter()
        .find(|r| r.0 == name)
        .unwrap_or_else(|| panic!("no row {name} in the menu"))
        .1
}

fn focus(h: &mut Harness<'static, App>, name: &str) {
    h.query_all_by_role(Role::RadioButton)
        .find(|n| n.accesskit_node().label().as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no row {name} in the menu"))
        .focus();
    // (the list scrolls to it)
    h.run_steps(12);
}

fn open_gear(h: &mut Harness<'static, App>, what: &str) {
    h.get_by_label("⚙").click();
    h.run_steps(8);
    assert!(!rows(h).is_empty(), "{what}: ⚙'s menu did not open");
}

/// Frames until the menu is gone.
fn shut(h: &mut Harness<'static, App>) {
    for _ in 0..40 {
        h.run_steps(1);
        if rows(h).is_empty() {
            return;
        }
    }
    panic!("the menu stayed open");
}

/// The part of the menu its rows are seen in: under its title, above the
/// boxes below the list, within the window.
fn sight(h: &Harness<'static, App>, size: egui::Vec2) -> Rect {
    let top = h
        .query_all_by_label("Visualization")
        .map(|n| n.rect().bottom())
        .fold(0.0, f32::max)
        .min(size.y);
    let bottom = h
        .get_by_label("The picture in a window of its own")
        .rect()
        .top();
    Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(size.x, bottom.min(size.y)))
        .intersect(Rect::from_min_max(Pos2::new(0.0, 0.0), size.to_pos2()))
        .intersect(Rect::everything_below(top.min(bottom) - 400.0))
}

#[test]
fn a_face_chosen_in_the_gears_menu_is_the_face_shown_in_every_design() {
    let mut bad = Vec::new();
    for d in &DESIGNS {
        for size in [DEFAULT, egui::vec2(900.0, 860.0), LEAST] {
            let what = format!("{} {}x{}", d.key, size.x, size.y);
            let (_home, mut h) = window(&format!("gear-{}-{}", d.key, size.x), d.key, size);
            assert_eq!(h.state().mode(), "spectrum");
            // by the pointer
            open_gear(&mut h, &what);
            focus(&mut h, "Radar");
            let at = row(&h, "Radar").center();
            click_at(&mut h, at);
            shut(&mut h);
            h.run_steps(20);
            if h.state().mode() != "radar" {
                bad.push(format!(
                    "{what}: a click on Radar in ⚙'s menu shows {}",
                    h.state().mode()
                ));
            }
            // by the keys: Enter, Space
            for (name, key, k) in [
                ("Spectrum rain", "rain", egui::Key::Enter),
                ("Mirrored spectrum", "mirror", egui::Key::Space),
                ("Off", "off", egui::Key::Enter),
            ] {
                open_gear(&mut h, &what);
                focus(&mut h, name);
                h.key_press(k);
                shut(&mut h);
                h.run_steps(20);
                if h.state().mode() != key {
                    bad.push(format!(
                        "{what}: {k:?} on {name} in ⚙'s menu shows {}",
                        h.state().mode()
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn the_menu_of_every_face_is_whole_in_the_least_window() {
    let mut bad = Vec::new();
    for d in &DESIGNS {
        for size in [LEAST, DEFAULT] {
            let what = format!("{} {}x{}", d.key, size.x, size.y);
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            let (_home, mut h) = window(&format!("menu-{}-{}", d.key, size.x), d.key, size);
            open_gear(&mut h, &what);
            let all = rows(&h);
            if all.len() != FACES.len() {
                bad.push(format!("{what}: {} of {} faces", all.len(), FACES.len()));
            }
            // every row a target of the project's least size, none over another
            for (name, r, _, _) in &all {
                if r.height() < 24.0 - 0.01 || r.width() < 24.0 {
                    bad.push(format!("{what}: {name} is {} x {}", r.width(), r.height()));
                }
            }
            for (i, a) in all.iter().enumerate() {
                for b in &all[i + 1..] {
                    if a.1.shrink(0.5).intersects(b.1.shrink(0.5)) {
                        bad.push(format!("{what}: {} over {}", a.0, b.0));
                    }
                }
            }
            // what is under the list: all of it in the window
            for name in [
                "The picture in a window of its own",
                "Full screen (F11)",
                "Show the playlist",
                "Show on display",
                "Design",
                "Theme",
                "Colours",
                "Options",
            ] {
                match h.query_by_label(name).map(|n| n.rect()) {
                    Some(r) if screen.contains_rect(r.shrink(0.5)) => {}
                    Some(r) => bad.push(format!("{what}: {name} off the window at {r:?}")),
                    None => bad.push(format!("{what}: no {name}")),
                }
            }
            // Tab from the first row through every one: each seen whole
            // where it has the focus
            focus(&mut h, all[0].0);
            let mut seen = Vec::new();
            for _ in 0..all.len() {
                let Some((name, r, _, _)) = rows(&h).into_iter().find(|r| r.2) else {
                    bad.push(format!("{what}: the focus left the menu after {seen:?}"));
                    break;
                };
                if !sight(&h, size).contains_rect(r.shrink(0.5)) {
                    bad.push(format!("{what}: the focus on {name} out of sight at {r:?}"));
                }
                seen.push(name);
                h.key_press(egui::Key::Tab);
                h.run_steps(10);
            }
            seen.dedup();
            if seen.len() != all.len() {
                bad.push(format!("{what}: Tab went through {} rows", seen.len()));
            }
            // a row at the list's far end, reached by the keys, takes the
            // pointer's click where it is seen
            let last = *seen.last().unwrap();
            focus(&mut h, last);
            let at = row(&h, last).center();
            click_at(&mut h, at);
            shut(&mut h);
            h.run_steps(20);
            let key = FACES.iter().find(|f| f.name == last).unwrap().key;
            if h.state().mode() != key {
                bad.push(format!(
                    "{what}: a click on {last} at {at:?} shows {}",
                    h.state().mode()
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The picture's place, and its menu opened by the right button there.
fn open_at_the_picture(h: &mut Harness<'static, App>) -> Rect {
    let picture = h.get_by_label_contains("Visualization: ").rect();
    let p = picture.center();
    h.event(Event::PointerMoved(p));
    h.run_steps(2);
    press(h, p, PointerButton::Secondary);
    h.run_steps(8);
    picture
}

#[test]
fn a_shut_menus_picture_takes_the_clicks_and_shows_the_choice() {
    // a second click 0.1 - 0.5 s after the choice, while the menu is still
    // drawn: at the same place, on the row below, three rows below, and on
    // a row that lies over the transport (the menu from the picture, and
    // from ⚙). (At the same place alone it was taken: review 19, M2 - a
    // click on another row of the picture stepped the face or paused the
    // music.)
    for gear in [false, true] {
        for gap in [2usize, 4, 7, 9] {
            for rows_on in [0usize, 1, 3, 99] {
                let what = format!("gear {gear}, {gap} frames, {rows_on} rows on");
                let (_home, mut h) =
                    window(&format!("ghost{gap}{gear}{rows_on}"), "modern", DEFAULT);
                let picture = if gear {
                    open_gear(&mut h, &what);
                    h.get_by_label_contains("Visualization: ").rect()
                } else {
                    open_at_the_picture(&mut h)
                };
                // a face whose row lies over the picture
                let all = rows(&h);
                let top = sight(&h, DEFAULT);
                let k = all
                    .iter()
                    .position(|(n, r, _, t)| picture.contains(r.center()) && !t && *n != "Off")
                    .expect("a row over the picture");
                let (name, r, ..) = all[k];
                let key = FACES.iter().find(|f| f.name == name).unwrap().key;
                let at = r.center();
                // where the second click goes: a row of the still picture
                let second = if rows_on == 99 {
                    // (the lowest row in sight in its column: over what is
                    // under the menu's lower part)
                    all.iter()
                        .filter(|(_, q, ..)| {
                            (q.left() - r.left()).abs() < 1.0 && top.contains_rect(q.shrink(0.5))
                        })
                        .map(|(_, q, ..)| q.center())
                        .max_by(|a, b| a.y.total_cmp(&b.y))
                        .unwrap()
                } else {
                    all[k + rows_on].1.center()
                };
                let playing = h.state().playing();
                click_at(&mut h, at);
                // the menu as it was drawn last - what stays a moment where
                // it was - shows the choice
                h.run_steps(1);
                assert_eq!(h.state().mode(), key, "{what}");
                let ticked: Vec<_> = rows(&h).into_iter().filter(|r| r.3).map(|r| r.0).collect();
                assert_eq!(
                    ticked,
                    [name],
                    "{what}: the menu drawn last shows the old choice"
                );
                h.run_steps(gap - 1);
                h.event(Event::PointerMoved(second));
                press(&mut h, second, PointerButton::Primary);
                for _ in 0..40 {
                    h.run_steps(1);
                    assert_eq!(
                        h.state().mode(),
                        key,
                        "{what}: a second click went through the menu's picture to the face"
                    );
                    assert_eq!(
                        h.state().playing(),
                        playing,
                        "{what}: a second click went through the menu's picture to Play"
                    );
                }
                // and once it is gone, the picture answers again
                h.run_steps(20);
                assert!(rows(&h).is_empty(), "{what}");
                h.event(Event::PointerMoved(picture.center()));
                press(&mut h, picture.center(), PointerButton::Primary);
                h.run_steps(20);
                assert_ne!(
                    h.state().mode(),
                    key,
                    "{what}: the picture takes no clicks any more"
                );
            }
        }
    }
}

#[test]
fn the_still_picture_stands_in_no_ones_way() {
    // the right button on the picture under a menu just shut by a choice:
    // the menu again, at once
    let (_home, mut h) = window("ghost-again", "modern", DEFAULT);
    let picture = open_at_the_picture(&mut h);
    let (name, r, ..) = rows(&h)
        .into_iter()
        .find(|(n, r, _, t)| picture.contains(r.center()) && !t && *n != "Off")
        .unwrap();
    click_at(&mut h, r.center());
    h.run_steps(3);
    assert!(rows(&h).is_empty(), "the menu did not shut at the choice");
    let p = r.center();
    h.event(Event::PointerMoved(p));
    press(&mut h, p, PointerButton::Secondary);
    h.run_steps(8);
    let ticked: Vec<_> = rows(&h).into_iter().filter(|r| r.3).map(|r| r.0).collect();
    assert_eq!(ticked, [name], "the picture's menu did not open again");
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    // a menu shut by Escape takes no clicks for what lies under it: Pause,
    // under ⚙'s menu
    let (_home, mut h) = window("ghost-escape", "modern", DEFAULT);
    open_gear(&mut h, "escape");
    let pause = h.get_by_label("Pause").rect();
    assert!(
        rows(&h).iter().any(|r| r.1.intersects(pause)),
        "Pause is not under ⚙'s menu here"
    );
    assert!(h.state().playing());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    click_at(&mut h, pause.center());
    h.run_steps(6);
    assert!(
        !h.state().playing(),
        "a click on Pause just after Escape shut the menu over it was not taken"
    );
    // a dialog a menu's entry opens is clicked at once: Intense… asks, and
    // its answer is taken where the menu's picture still is
    let (_home, mut h) = window("ghost-dialog", "modern", DEFAULT);
    open_gear(&mut h, "dialog");
    let options = h
        .get_by_role_and_label(Role::Button, "Options")
        .rect()
        .center();
    h.event(Event::PointerMoved(options));
    h.run_steps(8);
    let intense = h.get_by_label("Intense…").rect().center();
    click_at(&mut h, intense);
    h.run_steps(3);
    let keep = h.get_by_label("Keep calm").rect().center();
    click_at(&mut h, keep);
    h.run_steps(4);
    assert!(
        h.query_by_label("Keep calm").is_none(),
        "the warning's answer was not taken"
    );
    assert_eq!(h.state().motion(), "calm");
}

/// What the settings files say (the track's place and the window's size
/// aside, which change by themselves).
fn settings(home: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for file in ["axross-amp.json", "media_player.json"] {
        let Ok(text) = std::fs::read_to_string(home.join("config").join(file)) else {
            continue;
        };
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&text) else {
            continue;
        };
        for (k, v) in map {
            if !["tracks", "position", "window", "current", "playlist_open"].contains(&k.as_str()) {
                out.insert(format!("{file}: {k}"), v.to_string());
            }
        }
    }
    out
}

#[test]
fn every_submenu_opens_beside_its_button_and_a_click_on_it_changes_nothing() {
    // (Options, 626 points tall with its Graphics block, was laid over its
    // own button in a window lower than 700: the click that opened it
    // chose an entry - Updates off at 420 - 480, installing by themselves
    // at 520 - 540 - and its last entries lay off the window: review 19,
    // H1.) Each submenu: an entry of it to know it by, its last entry.
    let submenus = [
        ("Show on display", "With the sound (HDMI, DisplayPort)", ""),
        ("Design", "Brutalist", "Collage"),
        ("Theme", "Paper", ""),
        ("Colours", "From the cover, when it has one", "Custom"),
        (
            "Options",
            "Calm - calmer, fewer flashes (the default)",
            "Hide the pointer in full screen",
        ),
    ];
    let mut bad = Vec::new();
    // (AMP_MENU_DESIGN=brutalist: that design alone)
    let only = std::env::var("AMP_MENU_DESIGN").ok();
    for d in DESIGNS
        .iter()
        .filter(|d| only.as_deref().is_none_or(|o| o == d.key))
    {
        for size in [
            LEAST,
            egui::vec2(800.0, 600.0),
            egui::vec2(620.0, 520.0),
            DEFAULT,
            egui::vec2(700.0, 1100.0),
        ] {
            let what = format!("{} {}x{}", d.key, size.x, size.y);
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            let (home, mut h) = window(&format!("submenus-{}-{}", d.key, size.y), d.key, size);
            // (its first save done: both files there)
            for _ in 0..300 {
                settle(&mut h, 2);
                let saved = settings(&home);
                if ["axross-amp.json", "media_player.json"]
                    .iter()
                    .all(|file| saved.keys().any(|k| k.starts_with(file)))
                {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
            settle(&mut h, 6);
            let before = settings(&home);
            for (name, entry, last) in submenus {
                open_gear(&mut h, &what);
                let at = h.get_by_role_and_label(Role::Button, name).rect().center();
                h.event(Event::PointerMoved(at));
                h.run_steps(8);
                press(&mut h, at, PointerButton::Primary);
                h.run_steps(8);
                match h.query_all_by_label(entry).last().map(|n| n.rect()) {
                    None => bad.push(format!("{what}: {name} is not open after a click on it")),
                    Some(r) if !screen.contains_rect(r.shrink(0.5)) => {
                        bad.push(format!("{what}: {name}'s {entry} off the window at {r:?}"));
                    }
                    Some(_) => {}
                }
                // its last entry: brought into sight by the keys
                if !last.is_empty() && h.query_all_by_label(last).last().is_some() {
                    h.query_all_by_label(last).last().unwrap().focus();
                    h.run_steps(12);
                    let r = h.query_all_by_label(last).last().unwrap().rect();
                    if !screen.contains_rect(r.shrink(0.5)) {
                        bad.push(format!(
                            "{what}: {name}'s {last} off the window at {r:?} with the focus on it"
                        ));
                    }
                }
                for _ in 0..3 {
                    h.key_press(egui::Key::Escape);
                    h.run_steps(3);
                }
                h.run_steps(12);
                if !rows(&h).is_empty() {
                    bad.push(format!("{what}: the menu is still open after Escape"));
                }
                // a click that comes with the pointer - a tap: pressed in
                // the frame after it arrived, let go in the next (the whole
                // menu shut at it, on "Theme" and "Colours" at the default
                // width: review 20, L3)
                open_gear(&mut h, &what);
                let at = h.get_by_role_and_label(Role::Button, name).rect().center();
                h.event(Event::PointerMoved(at));
                press(&mut h, at, PointerButton::Primary);
                h.run_steps(8);
                if h.query_all_by_label(entry).last().is_none() {
                    bad.push(format!("{what}: {name} is not open after a tap on it"));
                }
                for _ in 0..3 {
                    h.key_press(egui::Key::Escape);
                    h.run_steps(3);
                }
                h.run_steps(12);
            }
            // (what a click chose would be saved by now)
            for _ in 0..12 {
                settle(&mut h, 2);
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            let after = settings(&home);
            for (k, v) in &after {
                if before.get(k) != Some(v) {
                    bad.push(format!(
                        "{what}: the clicks on the submenus' buttons changed {k}: {:?} -> {v}",
                        before.get(k)
                    ));
                }
            }
            if h.state().motion() != "calm" {
                bad.push(format!("{what}: motion is {}", h.state().motion()));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn the_arrows_go_across_the_grids_columns() {
    let (_home, mut h) = window("arrows", "modern", DEFAULT);
    open_gear(&mut h, "arrows");
    focus(&mut h, "Polar spectrum");
    let focused = |h: &Harness<'static, App>| rows(h).into_iter().find(|r| r.2).map(|r| (r.0, r.1));
    let (_, start) = focused(&h).expect("the focus on a row");
    // right: the row beside it in the next column, and the next
    let mut at = start;
    for step in 1..=2 {
        h.key_press(egui::Key::ArrowRight);
        h.run_steps(6);
        let (name, r) =
            focused(&h).unwrap_or_else(|| panic!("ArrowRight {step}: the focus left the grid"));
        assert!(
            r.left() > at.right() - 1.0 && (r.center().y - start.center().y).abs() < 30.0,
            "ArrowRight {step}: the focus on {name} at {r:?}, from {at:?}"
        );
        at = r;
    }
    // at the last column it stays
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(6);
    assert_eq!(
        focused(&h).map(|f| f.1),
        Some(at),
        "ArrowRight at the last column"
    );
    // left, back to where it began - and no further
    for _ in 0..2 {
        h.key_press(egui::Key::ArrowLeft);
        h.run_steps(6);
    }
    assert_eq!(
        focused(&h).map(|f| f.0),
        Some("Polar spectrum"),
        "two ArrowLefts back"
    );
    h.key_press(egui::Key::ArrowLeft);
    h.run_steps(6);
    assert_eq!(
        focused(&h).map(|f| f.0),
        Some("Polar spectrum"),
        "ArrowLeft at the first column took the focus out of the grid"
    );
    // down and up as ever
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(6);
    assert_eq!(focused(&h).map(|f| f.0), Some("Waterfall"));
}

/// Pictures of ⚙'s menu in every design, at the least window and the
/// default one (target/screenshots/menu-*.png): to look at.
#[test]
#[ignore]
fn pictures_of_the_menu() {
    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into()),
    )
    .join("screenshots");
    std::fs::create_dir_all(&dir).unwrap();
    for d in &DESIGNS {
        for size in [LEAST, DEFAULT] {
            let home = Scratch::new(&format!("menushot-{}-{}", d.key, size.x));
            let a = home.join("First.wav");
            write_chord(&a, 40);
            let setup = axross_amp::app::Setup {
                config_dir: home.join("config"),
                desktop: false,
                desktop_bus: None,
                headless_audio: true,
            };
            let args = vec![a.to_string_lossy().into_owned()];
            let mut h = Harness::builder()
                .with_size(size)
                .with_step_dt(0.05)
                .wgpu()
                .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()));
            assert!(until(&mut h, 5.0, |a| a.is_loaded()));
            h.state_mut().set_design(d.key);
            settle(&mut h, 3);
            open_gear(&mut h, d.key);
            focus(&mut h, "Pendulum wave");
            h.render()
                .unwrap()
                .save(dir.join(format!("menu-{}-{}.png", d.key, size.x)))
                .unwrap();
        }
    }
}

/// Review 20, L1: several taps on the still picture of a menu just shut
/// by a choice. Every one that falls while the picture is seen - half a
/// second from the choice - is taken; the first after it is a click on
/// the picture under it. (The review's third tap "0.30 s after the
/// choice" was let go 0.65 s after it: the harness draws a frame for each
/// event of a tap.)
#[test]
fn every_tap_on_the_still_picture_is_taken() {
    let mut bad = Vec::new();
    for gear in [false, true] {
        for wait in [0usize, 1, 2] {
            let what = format!("gear {gear}, taps from frame {wait}");
            let (_home, mut h) = window(&format!("taps{gear}{wait}"), "modern", DEFAULT);
            let picture = if gear {
                open_gear(&mut h, &what);
                h.get_by_label_contains("Visualization: ").rect()
            } else {
                open_at_the_picture(&mut h)
            };
            let (name, r, ..) = rows(&h)
                .into_iter()
                .find(|(n, r, _, t)| {
                    picture.contains(r.center())
                        && picture.contains(r.center() + egui::vec2(0.0, 52.0))
                        && !t
                        && *n != "Off"
                })
                .expect("a row over the picture");
            let key = FACES.iter().find(|f| f.name == name).unwrap().key;
            // (two rows down: over the picture)
            let p = r.center() + egui::vec2(0.0, 52.0);
            click_at(&mut h, r.center());
            h.run_steps(1);
            let chosen = h.ctx.input(|i| i.time);
            assert_eq!(h.state().mode(), key, "{what}: the choice");
            h.event(Event::PointerMoved(p));
            h.run_steps(wait);
            // taps as fast as the harness lets them be (a frame for the
            // press, one for the letting go), as long as the picture is
            // seen: three or four of them
            let mut taps = 0;
            while h.ctx.input(|i| i.time) - chosen < 0.4 {
                press(&mut h, p, PointerButton::Primary);
                h.run_steps(1);
                taps += 1;
            }
            h.run_steps(12);
            if taps < 3 || h.state().mode() != key {
                bad.push(format!(
                    "{what}: after {taps} taps the face is {}, chosen was {key}",
                    h.state().mode()
                ));
            }
            // and once the picture is gone, a click there is the picture's
            press(&mut h, p, PointerButton::Primary);
            h.run_steps(16);
            if h.state().mode() == key {
                bad.push(format!(
                    "{what}: a click on the picture after it all did nothing"
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Review 20, L2: the right button on the still picture opens the
/// picture's menu again as long as that picture takes the pointer - 0.40 s
/// after the choice it took the click and nothing came.
#[test]
fn the_right_button_opens_the_menu_again_until_the_picture_is_gone() {
    let mut bad = Vec::new();
    for gear in [false, true] {
        for frames in [1usize, 3, 5, 7, 8, 9, 10, 12] {
            let (_home, mut h) = window(&format!("again{gear}{frames}"), "modern", DEFAULT);
            let picture = if gear {
                open_gear(&mut h, "again");
                h.get_by_label_contains("Visualization: ").rect()
            } else {
                open_at_the_picture(&mut h)
            };
            let (name, r, ..) = rows(&h)
                .into_iter()
                .find(|(n, r, _, t)| picture.contains(r.center()) && !t && *n != "Off")
                .unwrap();
            click_at(&mut h, r.center());
            h.run_steps(frames);
            let p = r.center();
            h.event(Event::PointerMoved(p));
            press(&mut h, p, PointerButton::Secondary);
            h.run_steps(10);
            let ticked: Vec<_> = rows(&h).into_iter().filter(|r| r.3).map(|r| r.0).collect();
            if ticked != [name] {
                bad.push(format!(
                    "gear {gear}, {frames} frames after the choice: the menu shows {ticked:?}"
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Review 20, L11: where a submenu holds more than fits - Options in a
/// window 420 points tall shows 7 of its 20 entries - its scroll bar is
/// to be seen: a handle that stands off its track, not only a node for a
/// screen reader.
#[test]
fn a_submenu_that_scrolls_shows_its_bar() {
    let size = egui::vec2(620.0, 420.0);
    let (_home, mut h) = window("submenu-bar", "modern", size);
    open_gear(&mut h, "bar");
    let at = h
        .get_by_role_and_label(Role::Button, "Options")
        .rect()
        .center();
    h.event(Event::PointerMoved(at));
    h.run_steps(8);
    press(&mut h, at, PointerButton::Primary);
    h.run_steps(8);
    let entry = h
        .get_by_label("Calm - calmer, fewer flashes (the default)")
        .rect();
    // (the pointer on the submenu's button, not in the list: its bar at
    // rest, as it is before one goes to it)
    h.event(Event::PointerMoved(at));
    h.run_steps(12);
    // the submenu's bar: the upright one along its entries' right end
    let bar = h
        .query_all_by_role(Role::ScrollBar)
        .map(|n| n.rect())
        .filter(|r| {
            r.height() > r.width() * 3.0
                && (r.right() - entry.right()).abs() < 3.0
                && r.y_range().contains(entry.center().y)
        })
        .max_by(|a, b| a.height().total_cmp(&b.height()))
        .expect("the submenu's scroll bar");
    let shot = h.render().unwrap();
    let luma = |x: f32, y: f32| {
        let p = shot.get_pixel(
            (x.round() as u32).min(shot.width() - 1),
            (y.round() as u32).min(shot.height() - 1),
        );
        0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
    };
    // over the bar: its handle against what lies beside it
    let mut pixels = Vec::new();
    for dy in 0..bar.height() as usize {
        for dx in 0..bar.width().ceil() as usize {
            pixels.push(luma(
                bar.left() + dx as f32 + 0.5,
                bar.top() + dy as f32 + 0.5,
            ));
        }
    }
    let (least, most) = pixels
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!(
        most - least > 40.0,
        "no handle to be seen in the bar at {bar:?}: its pixels from {least:.0} to {most:.0} of 255"
    );
}
