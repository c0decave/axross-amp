//! axross-amp as a library: the parts of the player (the binary is
//! `src/main.rs`; tests drive the window through [`app::App`]).

/// A line on stderr (the log, in the background): lost, never a panic,
/// when it cannot be written - a full disk under the log, a closed
/// terminal or pipe. (`eprintln!` panics then; in alsa-lib's handler, a C
/// callback, that aborted the player at alsa-lib's first line - review 2
/// of 0.17.1, N1.)
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stderr().lock(), $($arg)*);
    }};
}

pub mod analyzer;
pub mod app;
pub mod bidi;
#[cfg(target_os = "linux")]
pub mod bluetooth;
#[cfg(target_os = "linux")]
pub mod bluetooth_ui;
pub mod cover;
pub mod covers;
pub mod design;
pub mod desktop_theme;
pub mod detach;
pub mod display;
pub mod engine;
pub mod eq;
pub mod fonts;
pub mod fspath;
pub mod graphics;
pub mod icon;
pub mod limiter;
pub mod marquee;
pub mod meters;
pub mod mixer;
pub mod mixer_ui;
pub mod mp4edit;
#[cfg(target_os = "linux")]
pub mod mpris;
pub mod net;
pub mod offload;
pub mod playlist;
pub mod ring;
pub mod saver;
pub mod seektable;
pub mod state;
#[cfg(test)]
pub(crate) mod testdir;
pub mod theme;
pub mod update;
pub mod update_ui;
pub mod viz;
