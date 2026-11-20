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

// (a GUI program on Windows: no console window opened beside it; started in
// a terminal it attaches to that one for --help and what it says)
#![cfg_attr(windows, windows_subsystem = "windows")]

use axross_amp::app;

const USAGE: &str = "axross-amp [--foreground] [--new-instance] [FILE|FOLDER|PLAYLIST|URL ...]
       axross-amp --help | --version

Play local audio (MP3, FLAC, Ogg Vorbis, Opus, AAC / M4A, ALAC, WAV, AIFF, CAF),
folders and M3U / PLS playlists, and internet radio by its http(s) URL.
The Mixer tab drives the sound server (pactl), the Bluetooth tab BlueZ.
A player already running (Linux) takes the files of a second start;
--new-instance starts another one anyway.

Started from a terminal it goes to the background and the prompt comes
back; it writes to ~/.cache/axross-amp/axross-amp.log (macOS:
~/Library/Caches/axross-amp/axross-amp.log). A start that fails within two
seconds is said there and then (exit status 1); one still starting after
two seconds goes on in the background and says so (exit status 0).
--foreground (-F) keeps it in the terminal. --version says which version
it is, --help (-h) this.

Keys: Space play / pause, Left / Right seek 5 s (Shift: 30 s), Ctrl+Left /
Right previous / next, Up / Down or + / - volume, Home the track's start,
M mute, N next, P previous, V / Shift+V the next / previous visualization,
J / L back / on 15 s, D the pictures on another display, F11 full screen,
Esc out of full screen or the large picture (or closes a dialog, the
display view), Ctrl+F the playlist's filter;
in the playlist Ctrl+Up / Down choose a track, Alt+Up / Down move it, Enter
play it, Delete remove it, Ctrl+Z undo.
The mouse wheel seeks over the position and sets the volume over its slider.";

fn main() -> eframe::Result {
    // (Windows: the terminal it was started in, if any, for its output)
    #[cfg(windows)]
    attach_to_the_terminal();
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
    // (from a terminal: to the background - after the files are handed to a
    // player already running, where there is one: no process for nothing)
    let background = axross_amp::detach::wanted(&args);
    let passed: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let files: Vec<String> = args
        .into_iter()
        .filter(|a| !a.starts_with("--") && a != "-F")
        .collect();
    // one player: a second start (a file opened from the file manager) hands
    // its files to the one running and ends
    // (the name on the session bus is taken here, before the window is made:
    // two starts at once never both get one)
    #[cfg(target_os = "linux")]
    {
        use axross_amp::mpris::{Claim, Mpris};
        match Mpris::claim(None, (!new_instance).then_some(&files[..])) {
            Claim::Handed => return Ok(()),
            // (going to the background: the name let go - the child takes it)
            Claim::Mine(mpris) if background => drop(mpris),
            Claim::Mine(mpris) => Mpris::prepare(*mpris),
            Claim::NoBus(_) => {}
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = new_instance;
    if background {
        match axross_amp::detach::spawn(&passed) {
            axross_amp::detach::Start::Exit(0) => return Ok(()),
            axross_amp::detach::Start::Exit(code) => std::process::exit(code),
            // (it could not go: here then)
            axross_amp::detach::Start::Run => {}
        }
    }
    // (the tests' stand-in for the window: up at once, then N seconds of
    // nothing - so going to the background can be tested without a display)
    if let Some(secs) = std::env::var("AXROSS_AMP_FAKE_WINDOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        if let Some(path) = std::env::var_os("AXROSS_AMP_FAKE_PIDFILE") {
            let _ = std::fs::write(path, std::process::id().to_string());
        }
        axross_amp::detach::signal_ready();
        std::thread::sleep(std::time::Duration::from_secs(secs));
        return Ok(());
    }
    // (a library the window's toolkit loads at its start, missing: said so,
    // with the package that has it - winit panicked on libxkbcommon-x11)
    #[cfg(target_os = "linux")]
    if let Some(missing) = window_library_missing() {
        eprintln!("axross-amp: the window cannot be opened: {missing}");
        std::process::exit(1);
    }
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
            .with_icon(axross_amp::icon::window_icon())
            .with_inner_size(size)
            .with_min_inner_size([420.0, 420.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "axross-amp",
        options,
        Box::new(move |cc| {
            let app = app::App::new(cc, files);
            #[cfg(unix)]
            app::close_on_signals(&cc.egui_ctx);
            // (in the background: the window is up - the terminal may go)
            axross_amp::detach::signal_ready();
            Ok(Box::new(app))
        }),
    )
}

/// The first library the window needs (winit loads them as it starts, by
/// name) that cannot be loaded, said with the packages that carry it:
/// under X11 - Xvfb, a desktop without Wayland - libxkbcommon-x11 and the
/// X libraries, under Wayland libwayland-client; libxkbcommon both. Without
/// libxkbcommon-x11 winit panicked ("Library libxkbcommon-x11.so could not
/// be loaded.", exit status 101) - GitHub's Ubuntu runners have none.
#[cfg(target_os = "linux")]
fn window_library_missing() -> Option<String> {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let wayland =
        set("WAYLAND_DISPLAY") && std::env::var("WINIT_UNIX_BACKEND").map_or(true, |b| b != "x11");
    // (the library; its package in Debian and Ubuntu, Fedora, Arch)
    let mut needed = vec![(
        "libxkbcommon.so.0",
        ["libxkbcommon0", "libxkbcommon", "libxkbcommon"],
    )];
    if wayland {
        needed.push((
            "libwayland-client.so.0",
            ["libwayland-client0", "libwayland-client", "wayland"],
        ));
    } else {
        needed.extend([
            (
                "libxkbcommon-x11.so.0",
                ["libxkbcommon-x11-0", "libxkbcommon-x11", "libxkbcommon-x11"],
            ),
            ("libX11.so.6", ["libx11-6", "libX11", "libx11"]),
            ("libX11-xcb.so.1", ["libx11-xcb1", "libX11-xcb", "libx11"]),
            (
                "libXcursor.so.1",
                ["libxcursor1", "libXcursor", "libxcursor"],
            ),
            ("libXi.so.6", ["libxi6", "libXi", "libxi"]),
        ]);
    }
    needed.into_iter().find_map(|(library, packages)| {
        let name = std::ffi::CString::new(library).ok()?;
        // SAFETY: a library name as the toolkit opens it, closed again at
        // once; nothing of it is used
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if handle.is_null() {
            let [debian, fedora, arch] = packages;
            Some(format!(
                "{library} is not installed (Debian and Ubuntu: {debian}, Fedora: {fedora}, Arch: {arch})"
            ))
        } else {
            // SAFETY: the handle dlopen just gave
            unsafe { libc::dlclose(handle) };
            None
        }
    })
}

/// A GUI program on Windows has no console; started from one (cmd,
/// PowerShell, Windows Terminal) it writes to that one - --help, --version,
/// a start that fails. Started from Explorer there is none, and nothing is
/// opened.
#[cfg(windows)]
fn attach_to_the_terminal() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: no arguments but a constant; fails harmlessly without a parent
    // console. Rust's stdout and stderr ask for the standard handles at
    // each write, so they reach the console attached here.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
