//! The window itself keeps within the flash rules, not only the faces
//! (review 13): the Theme and Design pickers showed each theme and design
//! the pointer passed over at once - down and up the twelve themes, 12
//! transitions a second over the whole window -, and skipping between a
//! red cover and a dark one gave one saturated red flash a second (the
//! cover card, the Glass haze, the accent), outside the flash guard.
//!
//! Now a theme or design is shown once clicked, at most twice a second,
//! and a change of cover to or from one of saturated red comes at most
//! every two seconds, faded over one; the haze and the accent are never
//! saturated red. Read by the project's meter and by the review's (m11):
//! at most six transitions in any second, and at most one saturated red
//! transition (no red flash).

#[path = "support/flashmeter.rs"]
mod flashmeter;
#[path = "support/m11.rs"]
mod m11;
#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use egui::accesskit::Role;
use egui::{Event, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use player::{Scratch, settle, until};

const SIZE: egui::Vec2 = egui::vec2(640.0, 480.0);

fn window(dir: &std::path::Path, args: Vec<String>) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        // (covers made for tracks without one, as the player has them: their
        // coming and their accent are part of what is tested - review of
        // 0.17.1, L4)
        stand_in_covers: true,
    };
    Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

/// Both meters over the frames pushed.
struct Meters {
    m: m11::M11,
    p: flashmeter::Meter,
}

/// The most transitions in any second by every reading: (light, red).
struct Most {
    light: usize,
    red: usize,
    said: String,
}

impl Meters {
    fn new() -> Meters {
        Meters {
            m: m11::M11::new(SIZE.x as usize, SIZE.y as usize, 60.0),
            p: flashmeter::Meter::with_fps(SIZE.x as usize, SIZE.y as usize, 60),
        }
    }

    fn push(&mut self, img: &image::RgbaImage) {
        self.m.push_rgba(img);
        self.p.push(img);
    }

    fn most(&mut self) -> Most {
        let r = self.m.reading();
        let (l, red) = self.p.most();
        let (sl, sred) = self.p.most_strict();
        Most {
            light: r.c[0].max(l).max(sl).max(self.p.area),
            red: r.c[1].max(red).max(sred).max(self.p.area_red),
            said: format!(
                "m11 {} red {}; project {l} red {red}, strict {sl} red {sred}, area {} red {}",
                r.c[0], r.c[1], self.p.area, self.p.area_red
            ),
        }
    }
}

fn open_menu(h: &mut Harness<'static, App>, name: &str) {
    h.get_by_label("⚙").click();
    h.run_steps(2);
    let at = h.get_by_role_and_label(Role::Button, name).rect().center();
    h.event(Event::PointerMoved(at));
    h.run_steps(2);
}

/// The pointer down and up a picker's rows, a row every 3 frames, twice:
/// what is shown does not change, and the window reads calm.
fn sweep(h: &mut Harness<'static, App>, rows: &[Pos2], shown: impl Fn(&App) -> &'static str) {
    let before = shown(h.state());
    let mut meters = Meters::new();
    for _ in 0..2 {
        for at in rows.iter().chain(rows.iter().rev()) {
            for _ in 0..3 {
                h.event(Event::PointerMoved(*at));
                h.run_steps(1);
                meters.push(&h.render().unwrap());
                assert_eq!(shown(h.state()), before, "shown on the pointer's way");
            }
        }
    }
    let most = meters.most();
    assert!(most.light <= 6 && most.red <= 1, "{}", most.said);
}

#[test]
fn the_theme_picker_shows_nothing_on_the_pointer_s_way() {
    let home = Scratch::new("wf-theme");
    let mut h = window(home.path(), vec![]);
    settle(&mut h, 3);
    open_menu(&mut h, "Theme");
    let rows: Vec<Pos2> = axross_amp::theme::THEMES
        .iter()
        .filter_map(|t| h.query_by_role_and_label(Role::RadioButton, t.name))
        .map(|r| r.rect().center())
        .collect();
    assert!(rows.len() >= 10, "{} rows", rows.len());
    sweep(&mut h, &rows, |a| a.theme_shown());
}

#[test]
fn the_design_picker_shows_nothing_on_the_pointer_s_way_or_the_keys() {
    let home = Scratch::new("wf-design");
    let mut h = window(home.path(), vec![]);
    settle(&mut h, 3);
    open_menu(&mut h, "Design");
    let rows: Vec<Pos2> = axross_amp::design::DESIGNS
        .iter()
        .filter_map(|d| h.query_by_role_and_label(Role::RadioButton, d.name))
        .map(|r| r.rect().center())
        .collect();
    assert!(rows.len() >= 3, "{} rows", rows.len());
    sweep(&mut h, &rows, |a| a.design_shown());
    // the keys move the focus down the list: nothing shown either
    let before = h.state().design_shown();
    for _ in 0..12 {
        h.key_press(egui::Key::ArrowDown);
        h.run_steps(1);
        assert_eq!(h.state().design_shown(), before, "shown on the focus");
    }
}

/// Clicked one after another ten times a second - dark and light themes
/// in turn -: the window changes at most twice a second, the last one
/// clicked shown at its turn.
#[test]
fn themes_clicked_in_quick_turn_change_the_window_twice_a_second_at_most() {
    let home = Scratch::new("wf-clicks");
    let mut h = window(home.path(), vec![]);
    settle(&mut h, 3);
    open_menu(&mut h, "Theme");
    let names = ["Paper", "Midnight", "Daylight", "High contrast"];
    let mut meters = Meters::new();
    let mut shown = vec![h.state().theme_shown()];
    // (a frame: run, read, what is shown kept)
    let mut frame = |h: &mut Harness<'static, App>, meters: &mut Meters| {
        h.run_steps(1);
        meters.push(&h.render().unwrap());
        shown.push(h.state().theme_shown());
    };
    let started = h.ctx.input(|i| i.time);
    for f in 0..180 {
        if f % 6 == 0 {
            let name = names[(f / 6) % names.len()];
            // (a click closes the menu: opened again, as fast as it goes)
            if h.query_by_role_and_label(Role::RadioButton, name).is_none() {
                if h.query_by_role_and_label(Role::Button, "Theme").is_none() {
                    h.get_by_label("⚙").click();
                    frame(&mut h, &mut meters);
                }
                if let Some(theme) = h.query_by_role_and_label(Role::Button, "Theme") {
                    let at = theme.rect().center();
                    h.event(Event::PointerMoved(at));
                    frame(&mut h, &mut meters);
                }
            }
            if let Some(row) = h.query_by_role_and_label(Role::RadioButton, name) {
                let at = row.rect().center();
                h.event(Event::PointerMoved(at));
                for pressed in [true, false] {
                    h.event(Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
            }
        }
        frame(&mut h, &mut meters);
    }
    let secs = h.ctx.input(|i| i.time) - started;
    shown.dedup();
    let changes = shown.len() - 1;
    assert!(changes >= 2, "the clicks changed nothing: {shown:?}");
    assert!(
        changes as f64 <= secs * 2.0 + 1.0,
        "{changes} changes in {secs:.1} s: {shown:?}"
    );
    let most = meters.most();
    assert!(most.light <= 6 && most.red <= 1, "{}", most.said);
    // (the last one clicked comes at its turn)
    let last = axross_amp::theme::find(h.state().theme_chosen()).key;
    for _ in 0..40 {
        h.run_steps(1);
    }
    assert_eq!(h.state().theme_shown(), last);
}

fn plain_art(rgb: [u8; 3], tag: u8) -> std::sync::Arc<axross_amp::engine::Art> {
    let n = 64u32;
    let rgba: Vec<u8> = (0..n * n)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    std::sync::Arc::new(axross_amp::engine::Art {
        width: n,
        height: n,
        rgba,
        png: vec![tag, rgb[0], rgb[1], rgb[2]],
        large: None,
    })
}

/// A red cover and a dark (or white) one in turn every half second - the
/// most often a new track's cover may come, Next held - in Glass (its haze
/// over the whole window) and Modern: no saturated red flash.
#[test]
fn red_and_dark_covers_in_quick_turn_flash_no_red() {
    let mut failures = Vec::new();
    for (design, theme, a, b) in [
        ("glass", "midnight", [230u8, 0, 0], [0u8, 0, 0]),
        ("glass", "paper", [230u8, 0, 0], [255u8, 255, 255]),
        ("glass", "contrast", [255u8, 0, 0], [0u8, 0, 0]),
        ("modern", "midnight", [230u8, 0, 0], [0u8, 0, 0]),
    ] {
        let home = Scratch::new("wf-covers");
        let a_path = home.join("A.wav");
        player::write_chord(&a_path, 20);
        let mut h = window(home.path(), vec![a_path.to_string_lossy().into_owned()]);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(design);
        h.state_mut().set_theme(theme);
        h.state_mut().set_mode("off");
        settle(&mut h, 40);
        let mut meters = Meters::new();
        let ctx = h.ctx.clone();
        for f in 0..(60 * 5) {
            if f % 30 == 0 {
                let k = (f / 30) as u8;
                let art = if k.is_multiple_of(2) {
                    plain_art(a, k)
                } else {
                    plain_art(b, k)
                };
                let info = axross_amp::engine::TrackInfo {
                    art: Some(art),
                    title: format!("T{k}"),
                    ..Default::default()
                };
                h.state_mut().show_info(&ctx, info);
            }
            h.run_steps(1);
            meters.push(&h.render().unwrap());
        }
        let most = meters.most();
        let line = format!("{design} {theme} {a:?}/{b:?}: {}", most.said);
        println!("{line}");
        if most.red > 1 || most.light > 6 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Tracks without covers in quick turn - a new one every half second, the
/// most often a cover may change - each shown the cover made for it, in
/// Modern's card and as the picture itself (the Cover face, the window's
/// largest area): within the rule by both meters, no red flash.
#[test]
fn covers_made_for_tracks_in_quick_turn_keep_within_the_rule() {
    let mut failures = Vec::new();
    // (every design with the picture off - the cover's slot, its accent,
    // the cassette, the collage, the Glass haze -, and Modern with the
    // Cover face, the window's largest area)
    let cases = axross_amp::design::DESIGNS
        .iter()
        .map(|d| (d.key, "off"))
        .chain([("modern", "cover")]);
    for (design, mode) in cases {
        let home = Scratch::new("wf-made-covers");
        let mut h = window(home.path(), Vec::new());
        let ctx = h.ctx.clone();
        h.state_mut().set_generated_covers(&ctx, true);
        h.state_mut().set_design(design);
        h.state_mut().set_mode(mode);
        settle(&mut h, 20);
        let tracks: Vec<axross_amp::state::Track> = (0..12)
            .map(|i| axross_amp::state::Track {
                path: format!("/m/album{i}/01.flac"),
                title: format!("T{i}"),
                album: format!("Album {i}"),
                ..Default::default()
            })
            .collect();
        let mut meters = Meters::new();
        for f in 0..(60 * 6) {
            if f % 30 == 0 {
                let k = (f / 30) % tracks.len();
                h.state_mut().pose(tracks.clone(), k, 200.0, 1.0);
                let info = axross_amp::engine::TrackInfo {
                    album: tracks[k].album.clone(),
                    title: tracks[k].title.clone(),
                    ..Default::default()
                };
                h.state_mut().show_info(&ctx, info);
            }
            h.run_steps(1);
            // (the maker's thread given its time, as a real frame's would)
            std::thread::sleep(std::time::Duration::from_millis(8));
            meters.push(&h.render().unwrap());
        }
        let most = meters.most();
        let line = format!("made covers, {design}, mode {mode}: {}", most.said);
        println!("{line}");
        assert!(h.state().has_cover(), "{design} {mode}: no cover shown");
        if most.red > 1 || most.light > 6 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A frame with the sound taken as a device would.
fn frame(
    h: &mut Harness<'static, App>,
    out: &mut axross_amp::engine::RenderState,
) -> image::RgbaImage {
    let mut buf = vec![0.0f32; 800 * 2];
    h.state().engine().shared.render(&mut buf, 2, out);
    h.run_steps(1);
    h.render().unwrap()
}

fn playing(home: &Scratch) -> Harness<'static, App> {
    let a = home.join("A.wav");
    player::write_chord(&a, 30);
    let mut h = window(home.path(), vec![a.to_string_lossy().into_owned()]);
    assert!(until(&mut h, 8.0, |a| a.is_loaded()));
    h.state_mut().set_mode("spectrum");
    h
}

/// `press` every other frame for three seconds (a key held: its repeats
/// come about 30 a second), the window read by both meters.
fn held(
    h: &mut Harness<'static, App>,
    mut press: impl FnMut(&mut Harness<'static, App>, usize),
) -> Most {
    let mut out = axross_amp::engine::RenderState::default();
    for _ in 0..30 {
        frame(h, &mut out);
    }
    let mut meters = Meters::new();
    for f in 0..180 {
        if f % 2 == 0 {
            press(h, f);
        }
        meters.push(&frame(h, &mut out));
    }
    meters.most()
}

fn key(h: &mut Harness<'static, App>, key: egui::Key, repeat: bool) {
    h.event(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: Default::default(),
    });
}

/// F11 held: full screen once, not in and out at the key's repeat (review
/// 14, H1: 30 transitions a second over the whole window by both meters);
/// pressed again and again (no repeat), at most twice a second.
#[test]
fn f11_held_or_pressed_fast_changes_the_window_twice_a_second_at_most() {
    let home = Scratch::new("wf-f11");
    let mut h = playing(&home);
    let most = held(&mut h, |h, f| key(h, egui::Key::F11, f > 0));
    assert!(most.light <= 6 && most.red <= 1, "held: {}", most.said);
    assert!(h.state().big_visual(), "the first press is taken");
    let home = Scratch::new("wf-f11-pressed");
    let mut h = playing(&home);
    let most = held(&mut h, |h, _| key(h, egui::Key::F11, false));
    assert!(most.light <= 6 && most.red <= 1, "pressed: {}", most.said);
}

/// A double click on the picture after a double click: the large picture
/// at most twice a second.
#[test]
fn double_clicks_in_quick_turn_change_the_window_twice_a_second_at_most() {
    let home = Scratch::new("wf-dbl");
    let mut h = playing(&home);
    let most = held(&mut h, |h, _| {
        let name = format!("Visualization: {}", axross_amp::viz::name(h.state().mode()));
        if let Some(at) = h
            .query_by_role_and_label(Role::Image, &name)
            .map(|n| n.rect().center())
        {
            for _ in 0..2 {
                for pressed in [true, false] {
                    h.event(Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
            }
        }
    });
    assert!(most.light <= 6 && most.red <= 1, "{}", most.said);
}

/// D held with a second display: the view there opened once, not opened
/// and closed at the key's repeat (review 14, H1: 31 by the strict reading).
#[test]
fn d_held_opens_the_display_view_once() {
    use axross_amp::display::Monitor;
    let home = Scratch::new("wf-d");
    let mut h = playing(&home);
    h.state_mut().set_monitors(vec![
        Monitor {
            connector: "eDP-1".into(),
            width: 1920,
            height: 1080,
            at: Some((0, 0)),
            primary: true,
            ..Default::default()
        },
        Monitor {
            connector: "HDMI-1".into(),
            name: "TV".into(),
            width: 1920,
            height: 1080,
            at: Some((1920, 0)),
            ..Default::default()
        },
    ]);
    let most = held(&mut h, |h, f| key(h, egui::Key::D, f > 0));
    assert!(most.light <= 6 && most.red <= 1, "{}", most.said);
}

/// Designs clicked one after another: within the rule (review 14, L1:
/// strict 7 - 8 - the fonts were swapped at the click, outside the turn).
#[test]
fn designs_clicked_in_quick_turn_keep_within_the_rule() {
    let home = Scratch::new("wf-designs");
    let mut h = playing(&home);
    let names: Vec<&str> = axross_amp::design::DESIGNS.iter().map(|d| d.name).collect();
    let mut out = axross_amp::engine::RenderState::default();
    settle(&mut h, 3);
    let mut meters = Meters::new();
    let mut shown = vec![h.state().design_shown()];
    for f in 0..240 {
        if f % 4 == 0 {
            let name = names[(f / 4) % names.len()];
            if h.query_by_role_and_label(Role::RadioButton, name).is_none() {
                if h.query_by_role_and_label(Role::Button, "Design").is_none() {
                    h.get_by_label("⚙").click();
                    meters.push(&frame(&mut h, &mut out));
                }
                if let Some(b) = h.query_by_role_and_label(Role::Button, "Design") {
                    let at = b.rect().center();
                    h.event(Event::PointerMoved(at));
                    meters.push(&frame(&mut h, &mut out));
                }
            }
            if let Some(row) = h.query_by_role_and_label(Role::RadioButton, name) {
                let at = row.rect().center();
                h.event(Event::PointerMoved(at));
                for pressed in [true, false] {
                    h.event(Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
            }
        }
        meters.push(&frame(&mut h, &mut out));
        shown.push(h.state().design_shown());
    }
    shown.dedup();
    assert!(shown.len() > 3, "the clicks changed nothing: {shown:?}");
    let most = meters.most();
    assert!(
        most.light <= 6 && most.red <= 1,
        "{} changes: {}",
        shown.len() - 1,
        most.said
    );
}

fn click_at(h: &mut Harness<'static, App>, at: Pos2, button: egui::PointerButton) {
    h.event(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Default::default(),
        });
    }
}

fn picture_at(h: &Harness<'static, App>) -> Option<Pos2> {
    let name = format!("Visualization: {}", axross_amp::viz::name(h.state().mode()));
    h.query_by_role_and_label(Role::Image, &name)
        .map(|n| n.rect().center())
}

/// Six tracks playing in the Paper theme (the picture's dark ground on the
/// light page: the starkest case), optionally the large picture.
fn paper(home: &Scratch, large: bool) -> Harness<'static, App> {
    let mut args = Vec::new();
    for k in 0..6 {
        let a = home.join(format!("T{k}.wav"));
        player::write_chord(&a, 30);
        args.push(a.to_string_lossy().into_owned());
    }
    let mut h = window(home.path(), args);
    assert!(until(&mut h, 8.0, |a| a.is_loaded()));
    h.state_mut().set_mode("spectrum");
    h.state_mut().set_theme("paper");
    if large {
        key(&mut h, egui::Key::F11, false);
        for _ in 0..40 {
            h.run_steps(1);
        }
    }
    h
}

/// A switch focused by the keyboard (as Tab focuses it), then Space or
/// Enter held: egui clicks a focused widget at each key repeat - the
/// playlist and the equalizer, the large picture's playlist, flipped 31
/// times a second (review 15, H2). Held keys' repeats reach no widget now.
#[test]
fn space_or_enter_held_on_a_focused_switch_flips_nothing_at_their_repeat() {
    for (case, large, menu, label, which) in [
        (
            "equalizer switch",
            false,
            false,
            "Equalizer",
            egui::Key::Space,
        ),
        (
            "playlist box",
            false,
            true,
            "Show the playlist",
            egui::Key::Space,
        ),
        (
            "large picture's list",
            true,
            false,
            "Hide the playlist",
            egui::Key::Space,
        ),
        (
            "large picture's list, Enter",
            true,
            false,
            "Hide the playlist",
            egui::Key::Enter,
        ),
    ] {
        let home = Scratch::new("wf-focus");
        let mut h = paper(&home, large);
        if menu && let Some(at) = picture_at(&h) {
            click_at(&mut h, at, egui::PointerButton::Secondary);
            h.run_steps(2);
        }
        // (the large picture's button says what a press does)
        let label = if large && h.query_by_label(label).is_none() {
            "Show the playlist"
        } else {
            label
        };
        h.get_by_label(label).focus();
        h.run_steps(1);
        let most = held(&mut h, |h, f| key(h, which, f > 0));
        assert!(most.light <= 6 && most.red <= 1, "{case}: {}", most.said);
    }
}

/// The tabs, the playlist and equalizer switch, the playlist's box and the
/// large picture's playlist clicked again and again (6.7 a second), in
/// Paper: the layout changes at most twice a second (review 15, M1: 7 - 14
/// transitions a second; the Player tab drew a black band for a frame).
#[test]
fn layout_switches_clicked_quickly_change_the_window_twice_a_second_at_most() {
    for case in ["tabs", "equalizer", "playlist box", "large list"] {
        let home = Scratch::new("wf-switch");
        let mut h = paper(&home, case == "large list");
        let mut out = axross_amp::engine::RenderState::default();
        for _ in 0..30 {
            frame(&mut h, &mut out);
        }
        let mut meters = Meters::new();
        for f in 0..180 {
            if f % 9 == 0 {
                let step = f / 9;
                match case {
                    "tabs" => {
                        let label = if step % 2 == 0 { "Mixer" } else { "Player" };
                        if let Some(n) = h.query_by_label(label) {
                            let at = n.rect().center();
                            click_at(&mut h, at, egui::PointerButton::Primary);
                        }
                    }
                    "equalizer" => {
                        if let Some(n) = h.query_by_label("Equalizer") {
                            let at = n.rect().center();
                            click_at(&mut h, at, egui::PointerButton::Primary);
                        }
                    }
                    "playlist box" => {
                        if h.query_by_label("Show the playlist").is_none()
                            && let Some(at) = picture_at(&h)
                        {
                            click_at(&mut h, at, egui::PointerButton::Secondary);
                            h.run_steps(1);
                        }
                        if let Some(n) = h.query_by_label("Show the playlist") {
                            let at = n.rect().center();
                            click_at(&mut h, at, egui::PointerButton::Primary);
                        }
                    }
                    _ => {
                        let label = if h.query_by_label("Hide the playlist").is_some() {
                            "Hide the playlist"
                        } else {
                            "Show the playlist"
                        };
                        if let Some(n) = h.query_by_label(label) {
                            let at = n.rect().center();
                            click_at(&mut h, at, egui::PointerButton::Primary);
                        }
                    }
                }
            }
            meters.push(&frame(&mut h, &mut out));
        }
        let most = meters.most();
        assert!(most.light <= 6 && most.red <= 1, "{case}: {}", most.said);
    }
}

/// The Player tab whole from its first frame: no band of nothing (black)
/// where its toolbar comes - the top bar's panel kept its old height for a
/// frame (review 15, M1).
#[test]
fn the_player_tab_comes_whole_in_its_first_frame() {
    let home = Scratch::new("wf-tab-band");
    let mut h = paper(&home, false);
    let mut out = axross_amp::engine::RenderState::default();
    for (tab, label) in [("Mixer", "Mixer"), ("Player", "Player")]
        .into_iter()
        .cycle()
        .take(4)
    {
        // (each in its turn: half a second and more since the last)
        for _ in 0..40 {
            frame(&mut h, &mut out);
        }
        let at = h.get_by_label(label).rect().center();
        click_at(&mut h, at, egui::PointerButton::Primary);
        let mut seen = 0;
        while seen < 3 {
            let img = frame(&mut h, &mut out);
            let (w, rows) = (img.width(), img.height());
            // a row dark from side to side (the picture keeps a margin):
            // never in Paper
            let dark = (0..rows).find(|&y| {
                (0..w)
                    .filter(|&x| {
                        let p = img.get_pixel(x, y).0;
                        u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 90
                    })
                    .count() as u32
                    > w * 99 / 100
            });
            assert!(dark.is_none(), "{tab}: a black band at row {dark:?}");
            seen += 1;
        }
    }
}

/// The picture's colour schemes clicked in turn in its right-click menu
/// (it stays open), Mono and Follow the theme, 6.7 and 4 a second, the
/// picture large, in Paper: the picture's ground flipped black and paper
/// at the clicks' pace - 7 - 10 transitions a second (review 16, M1). A
/// scheme is shown at the window's turn, the last one chosen; so is the
/// cover's colours' box.
#[test]
fn schemes_clicked_in_turn_change_the_picture_twice_a_second_at_most() {
    for (every, pair) in [
        (9, ["Mono", "Follow the theme"]),
        (15, ["Mono", "Follow the theme"]),
    ] {
        let home = Scratch::new("wf-schemes");
        let mut h = paper(&home, true);
        let mut out = axross_amp::engine::RenderState::default();
        for _ in 0..30 {
            frame(&mut h, &mut out);
        }
        let mut meters = Meters::new();
        let mut clicks = 0;
        let mut chosen = Vec::new();
        for f in 0..180 {
            if f % every == 0 {
                let name = pair[(f / every) % 2];
                if h.query_by_role_and_label(Role::RadioButton, name).is_none() {
                    if h.query_by_role_and_label(Role::Button, "Colours").is_none()
                        && let Some(at) = picture_at(&h)
                    {
                        click_at(&mut h, at, egui::PointerButton::Secondary);
                        meters.push(&frame(&mut h, &mut out));
                    }
                    if let Some(at) = h
                        .query_by_role_and_label(Role::Button, "Colours")
                        .map(|n| n.rect().center())
                    {
                        h.event(Event::PointerMoved(at));
                        meters.push(&frame(&mut h, &mut out));
                    }
                }
                if let Some(at) = h
                    .query_by_role_and_label(Role::RadioButton, name)
                    .map(|n| n.rect().center())
                {
                    click_at(&mut h, at, egui::PointerButton::Primary);
                    clicks += 1;
                }
            }
            meters.push(&frame(&mut h, &mut out));
            chosen.push(h.state().scheme_shown().to_owned());
        }
        chosen.dedup();
        assert!(clicks * every > 120, "every {every}: {clicks} clicks");
        assert!(
            chosen.len() > 2,
            "every {every}: the schemes did not change: {chosen:?}"
        );
        let most = meters.most();
        assert!(
            most.light <= 6 && most.red <= 1,
            "every {every}: {}",
            most.said
        );
    }
}
