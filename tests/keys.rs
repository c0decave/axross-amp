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

    h.get_by_label("URL").click();
    settle(&mut h, 2);
    let edit = h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Stream URL");
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
    h.get_by_label_contains("Shuffle").focus();
    settle(&mut h, 1);
    h.key_press(Key::Space);
    settle(&mut h, 3);
    assert!(
        h.state().snapshot().shuffle,
        "Space pressed the focused button"
    );
    assert!(h.state().playing(), "Space also paused the player");
}

#[test]
fn the_player_keys_belong_to_the_player_tab() {
    // (the Bluetooth tab's filter, the Mixer's sliders: letters typed there
    // must not skip, mute or pause)
    let dir = folder("keys-tab");
    let files = tracks(&dir, &["a.wav", "b.wav"], 3.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded() && a.playing()));
    h.state_mut().set_tab(axross_amp::app::Tab::Mixer);
    settle(&mut h, 1);
    let before = h.state().snapshot();
    for key in [Key::N, Key::M, Key::Space, Key::ArrowDown, Key::V] {
        h.key_press(key);
        settle(&mut h, 1);
    }
    let after = h.state().snapshot();
    assert_eq!(
        (after.current, after.muted, after.volume, &after.visualizer),
        (
            before.current,
            before.muted,
            before.volume,
            &before.visualizer
        )
    );
    assert!(h.state().playing());
}

fn position(h: &egui_kittest::Harness<'_, axross_amp::app::App>) -> f64 {
    h.state().engine().shared.position()
}

#[test]
fn axross_keys_seek_skip_and_set_the_volume() {
    let dir = folder("keys-axross");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav"], 60.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    // Shift+→ 30 s, → 5 s, Shift+← 30 s back, Home: the start
    key_with(&mut h, Key::ArrowRight, egui::Modifiers::SHIFT);
    assert!(until(&mut h, 2.0, |a| (a.engine().shared.position()
        - 30.0)
        .abs()
        < 0.1));
    h.key_press(Key::ArrowRight);
    assert!(until(&mut h, 2.0, |a| (a.engine().shared.position()
        - 35.0)
        .abs()
        < 0.1));
    key_with(&mut h, Key::ArrowLeft, egui::Modifiers::SHIFT);
    assert!(until(&mut h, 2.0, |a| (a.engine().shared.position() - 5.0)
        .abs()
        < 0.1));
    h.key_press(Key::Home);
    assert!(until(&mut h, 2.0, |a| a.engine().shared.position() < 0.05));
    // + / − the volume
    let volume = h.state().snapshot().volume;
    h.key_press(Key::Minus);
    settle(&mut h, 1);
    assert_eq!(h.state().snapshot().volume, volume - 5);
    h.key_press(Key::Plus);
    settle(&mut h, 1);
    assert_eq!(h.state().snapshot().volume, volume);
    // Ctrl+→ the next track, Ctrl+← the one before
    key_with(&mut h, Key::ArrowRight, egui::Modifiers::COMMAND);
    assert!(until(&mut h, 2.0, |a| a.snapshot().current == 1));
    key_with(&mut h, Key::ArrowLeft, egui::Modifiers::COMMAND);
    assert!(until(&mut h, 2.0, |a| a.snapshot().current == 0));
    let _ = position(&h);
}

#[test]
fn the_mouse_wheel_seeks_and_sets_the_volume() {
    let dir = folder("keys-wheel");
    let files = tracks(&dir, &["a.wav"], 60.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    let wheel =
        |h: &mut egui_kittest::Harness<'_, axross_amp::app::App>, label: &str, notches: f32| {
            h.get_by_label(label).hover();
            h.run_steps(1);
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, notches),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Default::default(),
            });
            h.run_steps(1);
        };
    wheel(&mut h, "Position", 2.0);
    assert!(
        until(&mut h, 2.0, |a| (a.engine().shared.position() - 10.0).abs()
            < 0.1),
        "{}",
        position(&h)
    );
    let volume = h.state().snapshot().volume;
    wheel(&mut h, "Volume", -1.0);
    settle(&mut h, 1);
    assert_eq!(h.state().snapshot().volume, volume - 5);
}

#[test]
fn the_playlist_is_ordered_by_keys() {
    let dir = folder("keys-order");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let names = |h: &egui_kittest::Harness<'_, axross_amp::app::App>| {
        h.state()
            .snapshot()
            .tracks
            .iter()
            .map(|t| t.path.rsplit('/').next().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    // Ctrl+↓ chooses b, Alt+↓ moves it below c, Alt+↑ back up
    key_with(&mut h, Key::ArrowDown, egui::Modifiers::COMMAND);
    settle(&mut h, 1);
    key_with(&mut h, Key::ArrowDown, egui::Modifiers::ALT);
    settle(&mut h, 1);
    assert_eq!(names(&h), ["a.wav", "c.wav", "b.wav"]);
    key_with(&mut h, Key::ArrowUp, egui::Modifiers::ALT);
    key_with(&mut h, Key::ArrowUp, egui::Modifiers::ALT);
    settle(&mut h, 1);
    assert_eq!(names(&h), ["b.wav", "a.wav", "c.wav"]);
    // the playing track stays the playing one
    assert_eq!(h.state().snapshot().current, 1);
}

/// A focused volume control owns the arrow keys: one step of volume, no
/// seek, no second step from the window's own ↑/↓ (the slider of the
/// modern design, the rack's knob).
#[test]
fn a_focused_volume_owns_the_arrows() {
    for design in ["modern", "rack"] {
        let dir = folder(&format!("keys-volume-{design}"));
        let files = tracks(&dir, &["a.wav"], 20.0);
        let mut h = harness(&dir, files);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(design);
        settle(&mut h, 3);
        h.get_by_label("Volume").focus();
        settle(&mut h, 2);
        let before = h.state().snapshot().volume;
        let pos = h.state_mut().shown_position();
        h.key_press(Key::ArrowRight);
        settle(&mut h, 2);
        let right = h.state().snapshot().volume;
        let moved = h.state_mut().shown_position() - pos;
        h.key_press(Key::ArrowUp);
        settle(&mut h, 2);
        let up = h.state().snapshot().volume;
        assert!(right > before, "{design}: → did not raise the volume");
        assert!(moved < 2.0, "{design}: → also seeked {moved:.1} s");
        assert!(
            up - right <= 5,
            "{design}: ↑ stepped {} at once (twice?)",
            up - right
        );
    }
}

/// A button clicked with the mouse does not keep the arrows: they go on
/// seeking and setting the volume (only a focused slider owns them) - in
/// the designs with keys of their own too.
#[test]
fn after_a_click_the_arrows_are_the_players() {
    for design in ["modern", "rack", "terminal", "cassette"] {
        let dir = folder(&format!("keys-after-click-{design}"));
        let files = tracks(&dir, &["a.wav"], 30.0);
        let mut h = harness(&dir, files);
        assert!(until(&mut h, 5.0, |a| a.is_loaded()));
        h.state_mut().set_design(design);
        settle(&mut h, 3);
        // (the pointer's click, as a mouse does it)
        let at = h.get_by_label_contains("Shuffle").rect().center();
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        settle(&mut h, 2);
        h.event(egui::Event::PointerGone);
        settle(&mut h, 1);
        let v = h.state().snapshot().volume;
        h.key_press(Key::ArrowDown);
        settle(&mut h, 2);
        assert_ne!(
            h.state().snapshot().volume,
            v,
            "{design}: ↓ after a click set no volume"
        );
        let p = h.state_mut().shown_position();
        h.key_press(Key::ArrowRight);
        settle(&mut h, 2);
        assert!(
            h.state_mut().shown_position() > p + 2.0,
            "{design}: → after a click did not seek"
        );
    }
}
