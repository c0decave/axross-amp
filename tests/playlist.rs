//! The playlist in the window: filtered, sorted, filled in the background
//! (titles and lengths read, folders walked on a thread of their own), and
//! honest about what it leaves out.

#[path = "support/player.rs"]
mod player;

use egui::Key;
use egui_kittest::kittest::Queryable;
use player::*;

/// The playlist's rows shown, top to bottom (of the names that may be there).
fn rows(h: &egui_kittest::Harness<'_, axross_amp::app::App>) -> Vec<String> {
    let mut found: Vec<(f32, String)> = ["apple", "banana", "cherry", "a", "b", "c", "d"]
        .iter()
        .filter_map(|name| {
            h.query_by_role_and_label(egui::accesskit::Role::ListItem, name)
                .map(|n| (n.rect().top(), name.to_string()))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, n)| n).collect()
}

#[test]
fn lengths_are_read_in_the_background_and_missing_files_marked() {
    let dir = folder("pl-scan");
    let files = tracks(&dir, &["one.wav", "two.wav", "three.wav"], 2.0);
    // a playlist saved before: a file that is gone since
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let gone = dir.join("gone.mp3").to_string_lossy().into_owned();
    std::fs::write(
        dir.join("config/media_player.json"),
        format!(
            r#"{{"tracks": [{{"path": "{gone}", "title": "", "duration": 0}}], "current": -1}}"#
        ),
    )
    .unwrap();
    let mut h = harness(&dir, files);
    assert!(
        until(&mut h, 5.0, |a| a.snapshot().tracks.len() == 4
            && a.snapshot().tracks[1..]
                .iter()
                .all(|t| t.duration_ms == 2000)),
        "{:?}",
        h.state().snapshot().tracks
    );
    assert!(until(&mut h, 3.0, |a| a.broken(&gone) == Some("no such file")));
}

#[test]
fn the_filter_shows_what_matches_and_takes_the_keys() {
    let dir = folder("pl-filter");
    let files = tracks(&dir, &["apple.wav", "banana.wav", "cherry.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["apple", "banana", "cherry"]);
    // Ctrl+F: the filter has the keys; "n" and "m" filter, not skip or mute
    key_with(&mut h, Key::F, egui::Modifiers::COMMAND);
    settle(&mut h, 2);
    let before = h.state().snapshot();
    h.get_by_label("Filter the playlist").type_text("AN mq");
    settle(&mut h, 2);
    assert_eq!(rows(&h), Vec::<String>::new(), "no track has both words");
    for _ in 0..3 {
        h.key_press(Key::Backspace);
    }
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["banana"]);
    let after = h.state().snapshot();
    assert_eq!((after.current, after.muted), (before.current, before.muted));
    // Escape clears it
    h.key_press(Key::Escape);
    settle(&mut h, 2);
    assert_eq!(rows(&h).len(), 3);
}

#[test]
fn the_sort_menu_orders_the_list_and_keeps_the_track_playing() {
    let dir = folder("pl-sort");
    let files = tracks(&dir, &["b.wav", "c.wav", "a.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.get_by_label("Sort ▾").click();
    settle(&mut h, 2);
    h.get_by_label("By file name").click();
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["a", "b", "c"]);
    let s = h.state().snapshot();
    assert!(s.tracks[s.current as usize].path.ends_with("b.wav"));
    h.get_by_label("Sort ▾").click();
    settle(&mut h, 2);
    h.get_by_label("Reverse the order").click();
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["c", "b", "a"]);
}

#[test]
fn a_folder_past_the_limit_is_said_to_be_cut() {
    let dir = folder("pl-limit");
    let music = dir.join("music");
    std::fs::create_dir_all(&music).unwrap();
    let limit = axross_amp::state::MAX_TRACKS;
    write_wav(&music.join("00000.wav"), 1.0);
    for i in 1..limit + 3 {
        std::fs::write(music.join(format!("{i:05}.mp3")), b"").unwrap();
    }
    let mut h = harness(&dir, vec![music.to_string_lossy().into_owned()]);
    assert!(
        until(&mut h, 20.0, |a| a.snapshot().tracks.len() == limit),
        "{}",
        h.state().status()
    );
    settle(&mut h, 2);
    assert!(
        h.state()
            .status()
            .contains(&format!("at most {limit} tracks")),
        "{}",
        h.state().status()
    );
}

#[test]
fn delete_twice_removes_two_rows() {
    let dir = folder("pl-delete");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav", "d.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.get_by_label("b").click();
    settle(&mut h, 1);
    h.key_press(Key::Delete);
    settle(&mut h, 1);
    h.key_press(Key::Delete);
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["a", "d"]);
}

#[test]
fn the_chosen_row_stays_chosen_when_one_above_it_goes() {
    // (the selection was an index: removing a row above it by its menu
    // moved it onto the next track, and Delete removed that one)
    let dir = folder("pl-select");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav", "d.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.get_by_label("c").click();
    settle(&mut h, 1);
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "a")
        .click_secondary();
    settle(&mut h, 2);
    h.get_by_label("✖ Remove").click();
    settle(&mut h, 2);
    h.key_press(Key::Delete);
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["b", "d"]);
}
