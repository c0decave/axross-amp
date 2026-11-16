//! axross-amp: the Media Player of axross as a program of its own, in Rust -
//! `axross-amp [FILE|FOLDER|PLAYLIST|URL ...]`.
//!
//! Local files, folders, M3U / PLS playlists and internet radio (http /
//! https); a ten-band equalizer with axross's presets; visualizations that
//! a click steps through; shuffle and repeat. The playlist, the position,
//! the volume, the visualization and the equalizer are axross's own
//! (`~/.config/axross/media_player.json`, `audio.json`): the player inside
//! axross and this program share them.

use axross_amp::app;

const USAGE: &str = "axross-amp [FILE|FOLDER|PLAYLIST|URL ...]

Play local audio (MP3, FLAC, Ogg Vorbis, Opus, AAC / M4A, ALAC, WAV, AIFF, CAF),
folders and M3U / PLS playlists, and internet radio by its http(s) URL.

Keys: Space play / pause, Left / Right seek 5 s, Up / Down volume,
M mute, N next, P previous, V next visualization, Enter play the selected
track, Delete remove it.";

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version") {
        println!("axross-amp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let files: Vec<String> = args.into_iter().filter(|a| !a.starts_with("--")).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("axross-amp")
            .with_app_id("axross-amp")
            .with_inner_size([620.0, 860.0])
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
