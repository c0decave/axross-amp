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
