//! The window used as a person uses it (egui_kittest: clicks, keys, files
//! dropped on it), headless; what it did is read from the app and from the
//! files it shares with axross.

use axross_amp::app::App;
use egui::{Event, Key, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::io::Write;
use std::path::Path;

fn write_wav(path: &Path, secs: u32) {
    let rate = 22_050u32;
    let frames = rate * secs;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + frames * 2).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 2).to_le_bytes(),
        &2u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        f.write_all(v).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&(frames * 2).to_le_bytes()).unwrap();
    for i in 0..frames {
        let v = ((i as f32 * 0.06).sin() * 8000.0) as i16;
        f.write_all(&v.to_le_bytes()).unwrap();
    }
}

/// A file dropped on the window from the desktop.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn settle(harness: &mut Harness<'_, App>, steps: usize) {
    for _ in 0..steps {
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

#[test]
fn buttons_keys_equalizer_playlist_url_and_dropped_files() {
    let home = std::env::temp_dir().join(format!("amp-click-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join("music/sub")).unwrap();
    // SAFETY: the only test of this binary; set before the app reads it
    unsafe { std::env::set_var("HOME", &home) };
    for name in ["a.wav", "b.wav", "sub/c.wav"] {
        write_wav(&home.join("music").join(name), 2);
    }
    std::fs::write(home.join("music/notes.txt"), "not audio").unwrap();
    let folder = home.join("music").to_string_lossy().into_owned();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .build_eframe(move |cc| App::new(cc, vec![folder.clone()]));
    settle(&mut harness, 10);

    // the folder came in, walked and sorted; the first track is loaded and wanted
    let s = harness.state().snapshot();
    let names: Vec<String> = s
        .tracks
        .iter()
        .map(|t| {
            Path::new(&t.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["a.wav", "b.wav", "c.wav"]);
    assert_eq!(s.current, 0);
    assert!(harness.state().is_loaded(), "{}", harness.state().status());

    // transport: Next, Previous, Shuffle, Repeat (off -> all -> one -> off)
    harness.get_by_label("Next").click();
    settle(&mut harness, 3);
    assert_eq!(harness.state().snapshot().current, 1);
    harness.get_by_label("Previous").click();
    settle(&mut harness, 3);
    assert_eq!(harness.state().snapshot().current, 0);
    harness.get_by_label("Shuffle").click();
    settle(&mut harness, 2);
    assert!(harness.state().snapshot().shuffle);
    for want in ["all", "one", "off"] {
        harness.get_by_label("Repeat").click();
        settle(&mut harness, 2);
        assert_eq!(harness.state().snapshot().repeat, want);
    }

    // keys: volume up / down, mute, next visualization
    let volume = harness.state().snapshot().volume;
    harness.key_press(Key::ArrowDown);
    settle(&mut harness, 2);
    assert_eq!(harness.state().snapshot().volume, volume - 5);
    harness.key_press(Key::M);
    settle(&mut harness, 2);
    assert!(harness.state().snapshot().muted);
    harness.get_by_label("Mute").click();
    settle(&mut harness, 2);
    assert!(!harness.state().snapshot().muted);
    let face = harness.state().mode();
    harness.key_press(Key::V);
    settle(&mut harness, 2);
    assert_ne!(harness.state().mode(), face);

    // a click on the picture: the next face (once it cannot be a double-click);
    // a double-click: large and back, the face unchanged
    let centre = Pos2::new(310.0, 250.0);
    let click = |h: &mut Harness<'_, App>| {
        h.event(Event::PointerButton {
            pos: centre,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.event(Event::PointerButton {
            pos: centre,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(1);
    };
    harness.hover_at(centre);
    harness.run_steps(1);
    let face = harness.state().mode();
    click(&mut harness);
    assert_eq!(
        harness.state().mode(),
        face,
        "not at once: it may become a double-click"
    );
    harness.run_steps(8); // (0.4 s)
    assert_ne!(harness.state().mode(), face, "a click shows the next face");
    let face = harness.state().mode();
    click(&mut harness);
    click(&mut harness);
    harness.run_steps(8);
    assert!(
        harness.state().big_visual(),
        "a double-click shows it large"
    );
    assert_eq!(
        harness.state().mode(),
        face,
        "a double-click leaves the face"
    );
    click(&mut harness);
    click(&mut harness);
    harness.run_steps(8);
    assert!(!harness.state().big_visual());

    // the equalizer preset: set, sent, and written to axross's audio.json
    harness.get_by_label("Equalizer preset").click();
    settle(&mut harness, 2);
    harness.get_by_label("Rock").click();
    settle(&mut harness, 4);
    assert_eq!(harness.state().equalizer_settings().preset, "Rock");
    let audio = std::fs::read_to_string(home.join(".config/axross/audio.json"))
        .expect("audio.json written");
    assert!(
        audio.contains("\"Rock\"") && audio.contains("eq_gains"),
        "{audio}"
    );

    // the playlist: Delete removes the selected track
    harness.get_by_label("Shuffle").click(); // (off again: Next is the next one)
    settle(&mut harness, 2);
    assert!(!harness.state().snapshot().shuffle);
    harness.get_by_label("Next").click(); // (selects and plays b)
    settle(&mut harness, 3);
    harness.key_press(Key::Delete);
    settle(&mut harness, 3);
    let s = harness.state().snapshot();
    assert_eq!(s.tracks.len(), 2, "{:?}", s.tracks);
    assert!(s.tracks.iter().all(|t| !t.path.ends_with("b.wav")));

    // a stream by its URL: the dialog, then a track that is a stream
    harness.get_by_label("📻 URL").click();
    settle(&mut harness, 2);
    harness.get_by_label("Add and play").click_accesskit(); // (disabled until a URL is typed)
    settle(&mut harness, 1);
    let edit = harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Stream URL");
    edit.type_text("https://radio.example/stream.mp3");
    settle(&mut harness, 2);
    harness.get_by_label("Add and play").click();
    settle(&mut harness, 4);
    let s = harness.state().snapshot();
    assert_eq!(
        s.tracks.last().unwrap().path,
        "https://radio.example/stream.mp3"
    );
    // (radio.example does not answer: the stream the user chose stays, and says why)
    for _ in 0..200 {
        if harness.state().status().starts_with("Cannot play") {
            break;
        }
        settle(&mut harness, 1);
    }
    assert!(
        harness.state().status().starts_with("Cannot play"),
        "{}",
        harness.state().status()
    );
    let s = harness.state().snapshot();
    assert_eq!(s.current as usize, s.tracks.len() - 1);

    // files dropped on the window
    let dropped = home.join("dropped.wav");
    write_wav(&dropped, 1);
    harness.event(Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    });
    harness
        .input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(dropped.clone())));
    settle(&mut harness, 3);
    assert!(
        harness
            .state()
            .snapshot()
            .tracks
            .iter()
            .any(|t| t.path.ends_with("dropped.wav"))
    );

    // closing writes the state into axross's file
    let before = harness.state().snapshot();
    drop(harness); // (the app saves when it goes)
    let saved = axross_amp::state::read_state(&home.join(".config/axross/media_player.json"));
    assert_eq!(saved.tracks.len(), before.tracks.len(), "{saved:?}");
    assert_eq!(saved.repeat, "off");
    assert!(!saved.shuffle);
}
