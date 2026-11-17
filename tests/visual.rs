//! The picture's own menu (a right click, as axross's context menu): the
//! face, the colours, full screen, the playlist shown or not; F11.

#[path = "support/player.rs"]
mod player;

use egui::{Event, Key, PointerButton, Pos2};
use egui_kittest::kittest::Queryable;
use player::*;

#[test]
fn a_right_click_on_the_picture_offers_its_options() {
    let dir = folder("visual-menu");
    let files = tracks(&dir, &["a.wav"], 2.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    let centre = Pos2::new(310.0, 250.0);
    h.hover_at(centre);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: centre,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(&mut h, 2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Visualization")
        .click();
    settle(&mut h, 2);
    // the faces in their groups
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Classic")
        .click();
    settle(&mut h, 2);
    h.get_by_label("3D ring").click();
    settle(&mut h, 2);
    assert_eq!(h.state().mode(), "ring");
    h.hover_at(centre);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: centre,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(&mut h, 2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Visualization")
        .click();
    settle(&mut h, 2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Generative")
        .click();
    settle(&mut h, 2);
    h.get_by_label("Plasma").click();
    settle(&mut h, 2);
    assert_eq!(h.state().mode(), "plasma");
    // the colours of the window itself
    h.get_by_label("⚙").click();
    settle(&mut h, 2);
    h.get_by_label("Follow the theme").click();
    settle(&mut h, 2);
    assert_eq!(h.state().snapshot().visual_scheme, "theme");
    // the playlist folded away from the menu
    if h.query_by_label("Show the playlist").is_none() {
        h.get_by_label("⚙").click();
        settle(&mut h, 2);
    }
    h.get_by_label("Show the playlist").click();
    settle(&mut h, 2);
    assert!(!h.state().snapshot().playlist_open);
    h.key_press(Key::Escape);
    settle(&mut h, 2);
    // F11: the picture alone; Escape back
    h.key_press(Key::F11);
    settle(&mut h, 2);
    assert!(h.state().big_visual());
    h.key_press(Key::Escape);
    settle(&mut h, 2);
    assert!(!h.state().big_visual());
}

#[test]
fn off_draws_no_analysis() {
    let dir = folder("visual-off");
    let files = tracks(&dir, &["a.wav"], 3.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.state_mut().set_mode("off");
    settle(&mut h, 2);
    let before = h.state().analyzer_frames();
    settle(&mut h, 10);
    assert_eq!(
        h.state().analyzer_frames(),
        before,
        "the analysis ran with the picture off"
    );
    // a click on the picture never steps into "off"
    assert_eq!(axross_amp::viz::next("typography"), "spectrum");
}
