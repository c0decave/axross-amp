//! The window under held keys and quick taps that change what it shows -
//! the face (V, Shift+V, the Cover face's layout), the playing row (N, P),
//! the playlist's filter - read by the project's meter and review 11's
//! (m11): within six transitions in any second, at most one of saturated
//! red (independent review 17's cases: H2, M1 - M3).

#[path = "support/flashmeter.rs"]
mod flashmeter;
#[path = "support/m11.rs"]
mod m11;
#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use player::{Scratch, until};

fn window(dir: &std::path::Path, args: Vec<String>, size: egui::Vec2) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

/// Both meters over the frames pushed.
struct Meters {
    m: m11::M11,
    p: flashmeter::Meter,
}

impl Meters {
    fn new(size: egui::Vec2) -> Meters {
        Meters {
            m: m11::M11::new(size.x as usize, size.y as usize, 60.0),
            p: flashmeter::Meter::with_fps(size.x as usize, size.y as usize, 60),
        }
    }
    fn push(&mut self, img: &image::RgbaImage) {
        self.m.push_rgba(img);
        self.p.push(img);
    }
    /// The most of every light reading, the most red, and what they said.
    fn most(&mut self) -> (usize, usize, String) {
        let r = self.m.reading();
        let (l, red) = self.p.most();
        let (sl, sred) = self.p.most_strict();
        (
            r.c[0].max(l).max(sl).max(self.p.area),
            r.c[1].max(red).max(sred).max(self.p.area_red),
            format!(
                "m11 {} red {}; project {l} red {red}, strict {sl} red {sred}, area {} red {}",
                r.c[0], r.c[1], self.p.area, self.p.area_red
            ),
        )
    }
}

fn frame(
    h: &mut Harness<'static, App>,
    out: &mut axross_amp::engine::RenderState,
) -> image::RgbaImage {
    let mut buf = vec![0.0f32; 800 * 2];
    h.state().engine().shared.render(&mut buf, 2, out);
    h.run_steps(1);
    h.render().unwrap()
}

fn key(h: &mut Harness<'static, App>, key: Key, repeat: bool, modifiers: Modifiers) {
    h.event(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers,
    });
}

/// `n` tracks of a chord, `secs` long, playing; the face Spectrum, the
/// theme `theme`.
fn player_with(home: &Scratch, n: usize, size: egui::Vec2, theme: &str) -> Harness<'static, App> {
    let mut args = Vec::new();
    for k in 0..n {
        let a = home.join(format!("T{k}.wav"));
        player::write_chord(&a, 30);
        args.push(a.to_string_lossy().into_owned());
    }
    let mut h = window(home.path(), args, size);
    assert!(until(&mut h, 8.0, |a| a.is_loaded()));
    h.state_mut().set_mode("spectrum");
    h.state_mut().set_theme(theme);
    h
}

/// The window's default size.
const DEFAULT: (f32, f32) = (620.0, 860.0);

/// `which` held (a repeat every other frame, as an X server's 25 - 30 a
/// second) for three seconds in `place`: the most of every reading.
fn held(
    place: &str,
    size: (f32, f32),
    theme: &str,
    start: &str,
    which: Key,
    modifiers: Modifiers,
) -> (usize, usize, String, Vec<&'static str>) {
    let size = egui::vec2(size.0, size.1);
    // (a folder of its own: the tests run side by side, and one's state
    // file written into another's was taken for another program's - its
    // face adopted)
    let home = Scratch::new(&format!(
        "held-{place}-{which:?}-{theme}-{start}-{}x{}",
        size.x, size.y
    ));
    let mut h = player_with(&home, 12, size, theme);
    match place {
        "detached" => h.state_mut().set_detached(true),
        "display" => {
            use axross_amp::display::Monitor;
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
                    width: 1920,
                    height: 1080,
                    at: Some((1920, 0)),
                    ..Default::default()
                },
            ]);
            let _ = h.state_mut().show_on("HDMI-1", false);
        }
        _ => {}
    }
    let mut out = axross_amp::engine::RenderState::default();
    for _ in 0..60 {
        frame(&mut h, &mut out);
    }
    h.state_mut().set_mode(start);
    for _ in 0..60 {
        frame(&mut h, &mut out);
    }
    let mut meters = Meters::new(size);
    let mut faces = std::collections::BTreeSet::new();
    faces.insert(h.state().mode());
    for f in 0..180 {
        if f % 2 == 0 {
            key(&mut h, which, f > 0, modifiers);
        }
        meters.push(&frame(&mut h, &mut out));
        faces.insert(h.state().mode());
    }
    let (light, red, said) = meters.most();
    (light, red, said, faces.into_iter().collect())
}

/// V held - the next face at each of its repeats - in the default window,
/// Midnight and Paper, from Spectrum and from Liquid, in the picture's own
/// window and the view on another display: the face changes at its first
/// press, not at every repeat (review 17, H2: 8 - 13 transitions a second -
/// the faces snapped from one to the next 15 times a second).
#[test]
fn holding_v_steps_the_face_once() {
    let mut failures = Vec::new();
    for (place, size, theme, start, modifiers) in [
        ("plain", DEFAULT, "midnight", "plasma", Modifiers::NONE),
        ("plain", DEFAULT, "midnight", "spectrum", Modifiers::NONE),
        ("plain", DEFAULT, "paper", "spectrum", Modifiers::SHIFT),
        (
            "plain",
            (1280.0, 800.0),
            "paper",
            "spectrum",
            Modifiers::NONE,
        ),
        (
            "detached",
            (640.0, 480.0),
            "paper",
            "spectrum",
            Modifiers::NONE,
        ),
        (
            "display",
            (640.0, 480.0),
            "paper",
            "spectrum",
            Modifiers::NONE,
        ),
    ] {
        let (light, red, said, faces) = held(place, size, theme, start, Key::V, modifiers);
        let line = format!("{place} {size:?} {theme} from {start}: {said}; faces {faces:?}");
        println!("{line}");
        // (the face it started from and one more: V's first press)
        if light > 6 || red > 1 || faces.len() != 2 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// N or P held: the next track at the first press, then twice a second at
/// most - the playing row's highlight ran through the list at the key's
/// repeat (review 17, M3: m11 12 at 640 x 480 in Paper).
#[test]
fn holding_n_or_p_skips_twice_a_second_at_most() {
    let mut failures = Vec::new();
    for (size, which) in [
        ((640.0, 480.0), Key::N),
        (DEFAULT, Key::N),
        ((640.0, 480.0), Key::P),
    ] {
        let (light, red, said, _) =
            held("plain", size, "paper", "spectrum", which, Modifiers::NONE);
        let line = format!("{which:?} held {size:?}: {said}");
        println!("{line}");
        if light > 6 || red > 1 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// V and Shift+V tapped in turn round the Cover face (whose Modern layout
/// leaves the now-playing card out), 6.7 and 10 a second: the face and the
/// layout change at the window's turn (review 17, M1: 7 - 20 transitions a
/// second - the card and the picture traded places at each tap).
#[test]
fn the_cover_face_tapped_in_and_out_changes_the_layout_at_its_turn() {
    let mut failures = Vec::new();
    for (size, theme, every) in [
        ((1280.0, 800.0), "paper", 9),
        ((1280.0, 800.0), "paper", 6),
        (DEFAULT, "midnight", 6),
    ] {
        let size = egui::vec2(size.0, size.1);
        let home = Scratch::new("cover-face");
        let mut h = player_with(&home, 3, size, theme);
        h.state_mut().set_mode("mel");
        let mut out = axross_amp::engine::RenderState::default();
        for _ in 0..60 {
            frame(&mut h, &mut out);
        }
        let mut meters = Meters::new(size);
        let mut faces = Vec::new();
        for f in 0..240 {
            if f % every == 0 {
                let back = (f / every) % 2 == 1;
                h.key_press_modifiers(
                    if back {
                        Modifiers::SHIFT
                    } else {
                        Modifiers::NONE
                    },
                    Key::V,
                );
            }
            meters.push(&frame(&mut h, &mut out));
            faces.push(h.state().mode());
        }
        faces.dedup();
        let (light, red, said) = meters.most();
        let line = format!(
            "{size:?} {theme} every {every}: {said}; faces {}",
            faces.len()
        );
        println!("{line}");
        assert!(
            faces.contains(&"cover"),
            "the Cover face never shown: {faces:?}"
        );
        if light > 6 || red > 1 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Typing in the playlist's filter - a letter that matches nothing, then
/// Backspace, 6.7 and 10 a second - in the default window: the picture
/// keeps its size, nothing under it moves (review 17, M2: 7 - 10
/// transitions a second - the picture took the room the list's matching
/// rows left).
#[test]
fn typing_in_the_filter_keeps_the_layout_still() {
    let mut failures = Vec::new();
    for (theme, every) in [("midnight", 6), ("paper", 9)] {
        let size = egui::vec2(DEFAULT.0, DEFAULT.1);
        let home = Scratch::new("filter-typing");
        let mut h = player_with(&home, 12, size, theme);
        let mut out = axross_amp::engine::RenderState::default();
        for _ in 0..40 {
            frame(&mut h, &mut out);
        }
        h.key_press_modifiers(Modifiers::COMMAND, Key::F);
        for _ in 0..20 {
            frame(&mut h, &mut out);
        }
        let mut meters = Meters::new(size);
        for f in 0..240 {
            if f % every == 0 {
                if (f / every) % 2 == 0 {
                    h.event(Event::Text("Z".into()));
                } else {
                    key(&mut h, Key::Backspace, false, Modifiers::NONE);
                }
            }
            meters.push(&frame(&mut h, &mut out));
        }
        let (light, red, said) = meters.most();
        let line = format!("{theme} every {every}: {said}");
        println!("{line}");
        if light > 6 || red > 1 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn click_at(h: &mut Harness<'static, App>, at: egui::Pos2, button: egui::PointerButton) {
    h.event(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
}

/// A popup - the picture's menu (a right click on it), ⚙'s - opened and
/// shut with Escape 5 times a second at 640 x 480 in Paper and Midnight:
/// within the rule (review 17, L3 - review 16's L1: 10 transitions a
/// second by both meters, each popup's frame and shadow coming and going).
#[test]
fn popups_opened_and_shut_at_click_rate_keep_within_the_rule() {
    use egui_kittest::kittest::Queryable;
    let mut failures = Vec::new();
    for (opener, theme) in [("menu", "paper"), ("gear", "paper"), ("menu", "midnight")] {
        let size = egui::vec2(640.0, 480.0);
        let home = Scratch::new("popups");
        let mut h = player_with(&home, 12, size, theme);
        let mut out = axross_amp::engine::RenderState::default();
        for _ in 0..40 {
            frame(&mut h, &mut out);
        }
        let mut meters = Meters::new(size);
        let mut opened = 0;
        let every = 6;
        for f in 0..240 {
            if f % every == 0 {
                if (f / every) % 2 == 0 {
                    let at = match opener {
                        "menu" => {
                            let name = format!(
                                "Visualization: {}",
                                axross_amp::viz::name(h.state().mode())
                            );
                            h.query_by_role_and_label(egui::accesskit::Role::Image, &name)
                                .map(|n| (n.rect().center(), egui::PointerButton::Secondary))
                        }
                        _ => h
                            .query_by_label("⚙")
                            .map(|n| (n.rect().center(), egui::PointerButton::Primary)),
                    };
                    if let Some((at, button)) = at {
                        click_at(&mut h, at, button);
                        opened += 1;
                    }
                } else {
                    key(&mut h, Key::Escape, false, Modifiers::NONE);
                }
            }
            let img = frame(&mut h, &mut out);
            meters.push(&img);
        }
        let (light, red, said) = meters.most();
        let line = format!("{opener} {theme}: {said}; opened {opened}");
        println!("{line}");
        assert!(opened >= 15, "{line}");
        if light > 6 || red > 1 {
            failures.push(line);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
