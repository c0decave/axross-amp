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

fn row(h: &egui_kittest::Harness<'_, axross_amp::app::App>, name: &str) -> egui::Rect {
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, name)
        .rect()
}

#[test]
fn several_rows_are_chosen_removed_and_dragged_together() {
    let dir = folder("pl-multi");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav", "d.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    // Shift-click: a to b; drag them onto d
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "a")
        .click();
    settle(&mut h, 1);
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "b")
        .click_modifiers(egui::Modifiers::SHIFT);
    settle(&mut h, 1);
    let (from, to) = (row(&h, "a").center(), row(&h, "d").center());
    h.hover_at(from);
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    for k in 1..=10 {
        let p = from + (to - from) * (k as f32 / 10.0);
        h.event(egui::Event::PointerMoved(p));
        h.run_steps(1);
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    settle(&mut h, 3);
    assert_eq!(rows(&h), ["c", "d", "a", "b"]);
    // Ctrl-click c and b (the moved block still chosen: taken out first)
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "c")
        .click();
    settle(&mut h, 1);
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "b")
        .click_modifiers(egui::Modifiers::COMMAND);
    settle(&mut h, 1);
    h.key_press(Key::Delete);
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["d", "a"]);
}

#[test]
fn removals_and_clear_are_undone_with_ctrl_z() {
    let dir = folder("pl-undo");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "b")
        .click();
    settle(&mut h, 1);
    h.key_press(Key::Delete);
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["a", "c"]);
    h.get_by_label("Clear").click();
    settle(&mut h, 2);
    assert!(rows(&h).is_empty());
    key_with(&mut h, Key::Z, egui::Modifiers::COMMAND);
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["a", "c"]);
    h.get_by_label("↶ Undo").click();
    settle(&mut h, 2);
    assert_eq!(rows(&h), ["a", "b", "c"]);
    assert_eq!(
        h.state().snapshot().current,
        0,
        "the track playing stays current"
    );
}

#[test]
fn a_track_already_there_is_not_added_twice() {
    let dir = folder("pl-dupes");
    let files = tracks(&dir, &["a.wav", "b.wav"], 1.0);
    let mut h = harness(&dir, files.clone());
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    // dropped again, with a new one
    let more = tracks(&dir, &["c.wav"], 1.0);
    for f in files.iter().chain(&more) {
        h.input_mut()
            .dropped_files
            .push(std::sync::Arc::new(Dropped(f.into())));
    }
    assert!(until(&mut h, 5.0, |a| a.status().contains("already")));
    assert_eq!(h.state().snapshot().tracks.len(), 3);
    assert!(
        h.state()
            .status()
            .contains("1 added; 2 already in the playlist"),
        "{}",
        h.state().status()
    );
}

/// A file dropped on the window.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn play_next_from_a_rows_menu() {
    let dir = folder("pl-next");
    let files = tracks(&dir, &["a.wav", "b.wav", "c.wav"], 0.4);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    h.get_by_role_and_label(egui::accesskit::Role::ListItem, "c")
        .click_secondary();
    settle(&mut h, 2);
    h.get_by_label("⏭ Play next").click();
    settle(&mut h, 2);
    let mut output = axross_amp::engine::RenderState::default();
    let mut order = vec![h.state().snapshot().current];
    for _ in 0..20 {
        listen(&mut h, &mut output, 0.1);
        let c = h.state().snapshot().current;
        if order.last() != Some(&c) {
            order.push(c);
        }
    }
    // (then the list goes on from a: b, and c in its turn)
    assert_eq!(order, vec![0, 2, 1, 2], "c was to play next");
}

/// A file named in bytes that are no UTF-8 (Latin-1's "café", as old disks
/// have them): found in its folder, opened and played - not Stopped for a
/// name changed on the way - and, given on the command line, no panic.
#[cfg(unix)]
#[test]
fn a_name_that_is_no_utf8_plays() {
    use std::os::unix::ffi::OsStrExt;
    let dir = folder("latin1");
    let raw = std::ffi::OsStr::from_bytes(b"caf\xe9.wav");
    write_wav(&dir.join(raw), 1.0);
    let found = axross_amp::playlist::collect(&[dir.to_string_lossy().into_owned()]);
    assert_eq!(found.tracks.len(), 1);
    let path = &found.tracks[0].path;
    let info = axross_amp::engine::probe(path);
    assert!(info.is_ok(), "{path:?}: {info:?}");
    // the program itself, given such a name (with --version: it ends there)
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_axross-amp"))
        .arg("--version")
        .arg(dir.join(raw))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
