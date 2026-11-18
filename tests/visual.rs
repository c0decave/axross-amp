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
    // (the picture's middle, wherever the card above leaves it)
    let centre = picture_centre(&h);
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
    // (the menu finds its place in the window over a few frames)
    settle(&mut h, 6);
    // the faces in their groups, side by side, in the menu itself
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
    settle(&mut h, 6);
    h.get_by_label("Kinetic typography").click();
    settle(&mut h, 2);
    assert_eq!(h.state().mode(), "typography");
    // the colours of the window itself
    h.get_by_label("⚙").click();
    settle(&mut h, 2);
    let colours = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Colours")
        .rect();
    h.hover_at(colours.center());
    settle(&mut h, 2);
    if h.query_by_label("Follow the theme").is_none() {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Colours")
            .click();
        settle(&mut h, 3);
    }
    h.get_by_label("Follow the theme").click();
    settle(&mut h, 2);
    assert_eq!(h.state().snapshot().visual_scheme, "theme");
    // the playlist folded away from the menu (above the colours' menu:
    // not under it)
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

#[test]
fn the_overview_shows_the_track_and_a_click_goes_there() {
    let dir = folder("visual-overview");
    let files = tracks(&dir, &["long.wav"], 20.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.state_mut().set_mode("overview");
    // the whole track is read on a thread of its own
    assert!(until(&mut h, 20.0, |a| a.overview_read() >= 1.0));
    let picture = h.get_by_role(egui::accesskit::Role::Image).rect();
    // a click three quarters along the waveform
    let at = Pos2::new(
        picture.left() + 4.0 + (picture.width() - 8.0) * 0.75,
        picture.center().y,
    );
    h.hover_at(at);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(&mut h, 3);
    let pos = h.state().engine().shared.position();
    assert!((pos - 15.0).abs() < 1.0, "at {pos} s");
    // and the face stays (no step to the next)
    settle(&mut h, 30);
    assert_eq!(h.state().mode(), "overview");
}

/// The middle of the picture (its accessible name says what it shows).
fn picture_centre(h: &egui_kittest::Harness<'_, axross_amp::app::App>) -> Pos2 {
    use egui_kittest::kittest::Queryable;
    h.get_by_label_contains("Visualization: ").rect().center()
}

#[test]
fn the_menus_lowest_rows_take_their_clicks() {
    // the faces' menu taller than the window was moved about from frame to
    // frame, and its lowest rows missed their clicks
    let dir = folder("visual-lowest");
    let files = tracks(&dir, &["a.wav"], 3.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.get_by_label("⚙").click();
    settle(&mut h, 3);
    let r = h.get_by_label("Show the playlist").rect();
    assert!(r.bottom() < 860.0, "{r:?}");
    h.get_by_label("Show the playlist").click();
    settle(&mut h, 2);
    assert!(!h.state().snapshot().playlist_open, "the click was missed");
}

#[test]
fn intense_motion_only_after_the_photosensitivity_warning() {
    // calm by default; "Intense…" shows a warning first, and
    // only its own button turns it on
    let dir = folder("visual-motion");
    let files = tracks(&dir, &["a.wav"], 3.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    assert_eq!(h.state().motion(), "calm");
    h.get_by_label("⚙").click();
    settle(&mut h, 3);
    let options = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Options")
        .rect();
    h.hover_at(options.center());
    settle(&mut h, 2);
    if h.query_by_label("Intense…").is_none() {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Options")
            .click();
        settle(&mut h, 3);
    }
    h.get_by_label("Intense…").click();
    settle(&mut h, 3);
    assert!(h.query_by_label("Photosensitivity warning").is_some());
    assert_eq!(
        h.state().motion(),
        "calm",
        "not before the warning is answered"
    );
    h.get_by_label("Keep calm").click();
    settle(&mut h, 2);
    assert_eq!(h.state().motion(), "calm");
    assert!(h.query_by_label("Photosensitivity warning").is_none());
}

#[test]
fn escape_closes_only_the_innermost_thing() {
    // the picture large, a track's details over it: Escape closes the
    // details and leaves the picture large; again, the picture too
    let dir = folder("visual-escape");
    let files = tracks(&dir, &["a.wav"], 3.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.key_press(Key::F11);
    settle(&mut h, 2);
    assert!(h.state().big_visual());
    h.get_by_label("Show the playlist").click();
    settle(&mut h, 2);
    let row = h
        .query_all_by_label("a")
        .last()
        .map(|n| n.rect())
        .expect("the row");
    h.hover_at(row.center());
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: row.center(),
            button: PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
    }
    settle(&mut h, 2);
    h.get_by_label("ℹ Details…").click();
    settle(&mut h, 3);
    assert!(h.query_by_label("Track details").is_some());
    h.key_press(Key::Escape);
    settle(&mut h, 2);
    assert!(
        h.query_by_label("Track details").is_none(),
        "the details stay"
    );
    assert!(
        h.state().big_visual(),
        "one Escape left the large picture too"
    );
    h.key_press(Key::Escape);
    settle(&mut h, 2);
    assert!(!h.state().big_visual());
}
