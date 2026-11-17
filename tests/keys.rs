//! The keyboard: the player's keys work on the window, but not while a text
//! is typed (a URL, a file's name, the playlist's filter) or a dialog is
//! open, and Space / Enter belong to the widget that has the focus.

#[path = "support/player.rs"]
mod player;

use egui::Key;
use egui_kittest::kittest::Queryable;
use player::*;

#[test]
fn typing_a_url_is_no_shortcut() {
    let dir = folder("keys-typing");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav"], 2.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.get_by_label("b").click(); // (selects the row)
    settle(&mut h, 2);
    let before = h.state().snapshot();
    let playing = h.state().playing();
    let mode = h.state().mode();

    h.get_by_label("📻 URL").click();
    settle(&mut h, 2);
    let edit = h.get_by_role(egui::accesskit::Role::TextInput);
    edit.type_text("not a url mnpv");
    settle(&mut h, 2);
    for key in [
        Key::Delete,
        Key::Space,
        Key::M,
        Key::N,
        Key::P,
        Key::V,
        Key::ArrowUp,
        Key::ArrowLeft,
        Key::Enter,
    ] {
        h.key_press(key);
        settle(&mut h, 1);
    }
    let after = h.state().snapshot();
    assert_eq!(
        after.tracks.len(),
        3,
        "Delete in the URL field removed a track"
    );
    assert_eq!(after.current, before.current, "a key changed the track");
    assert_eq!(after.volume, before.volume);
    assert_eq!(after.muted, before.muted);
    assert_eq!(h.state().playing(), playing, "Space toggled the player");
    assert_eq!(h.state().mode(), mode);
}

#[test]
fn space_on_a_focused_button_presses_only_the_button() {
    let dir = folder("keys-focus");
    let files = tracks(&dir, &["a.wav", "b.wav"], 3.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded() && a.playing()));
    // the Shuffle button gets the keyboard's focus, Space presses it
    h.get_by_label("Shuffle").focus();
    settle(&mut h, 1);
    h.key_press(Key::Space);
    settle(&mut h, 3);
    assert!(
        h.state().snapshot().shuffle,
        "Space pressed the focused button"
    );
    assert!(h.state().playing(), "Space also paused the player");
}
