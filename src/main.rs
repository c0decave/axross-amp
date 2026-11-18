//! axross-amp: the Media Player of axross as a program of its own, in Rust -
//! `axross-amp [FILE|FOLDER|PLAYLIST|URL ...]`.
//!
//! Local files, folders, M3U / PLS playlists and internet radio (http /
//! https); a ten-band equalizer with axross's presets; visualizations that
//! a click steps through; shuffle and repeat; the system mixer and (Linux)
//! Bluetooth in tabs of their own. The playlist, the position,
//! the volume, the visualization and the equalizer are axross's own
//! (`~/.config/axross/media_player.json`, `audio.json`): the player inside
//! axross and this program share them.

use axross_amp::app;

const USAGE: &str = "axross-amp [--new-instance] [FILE|FOLDER|PLAYLIST|URL ...]

Play local audio (MP3, FLAC, Ogg Vorbis, Opus, AAC / M4A, ALAC, WAV, AIFF, CAF),
folders and M3U / PLS playlists, and internet radio by its http(s) URL.
The Mixer tab drives the sound server (pactl), the Bluetooth tab BlueZ.
A player already running (Linux) takes the files of a second start;
--new-instance starts another one anyway.

Keys: Space play / pause, Left / Right seek 5 s (Shift: 30 s), Ctrl+Left /
Right previous / next, Up / Down or + / - volume, Home the track's start,
M mute, N next, P previous, V / Shift+V the next / previous visualization;
in the playlist Ctrl+Up / Down choose a track, Alt+Up / Down move it, Enter
play it, Delete remove it, Ctrl+Z undo.
The mouse wheel seeks over the position and sets the volume over its slider.";

fn main() -> eframe::Result {
    // (args_os: a file name in bytes that are no UTF-8 - "Open with" from a
    // file manager - is kept, not a panic)
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| axross_amp::fspath::os_to_string(&a))
        .collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version") {
        println!("axross-amp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let new_instance = args.iter().any(|a| a == "--new-instance");
    let files: Vec<String> = args.into_iter().filter(|a| !a.starts_with("--")).collect();
    // one player: a second start (a file opened from the file manager) hands
    // its files to the one running and ends
    // (the name on the session bus is taken here, before the window is made:
    // two starts at once never both get one)
    #[cfg(target_os = "linux")]
    {
        use axross_amp::mpris::{Claim, Mpris};
        match Mpris::claim(None, (!new_instance).then_some(&files[..])) {
            Claim::Handed => return Ok(()),
            Claim::Mine(mpris) => Mpris::prepare(*mpris),
            Claim::NoBus(_) => {}
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = new_instance;
    // the window as large as it was left
    let size = axross_amp::state::read_amp(
        &axross_amp::state::config_dir().join(axross_amp::state::AMP_FILE),
    )
    .window
    .unwrap_or([620.0, 860.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("axross-amp")
            .with_app_id("axross-amp")
            .with_inner_size(size)
            .with_min_inner_size([420.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "axross-amp",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, files)))),
    )
}
