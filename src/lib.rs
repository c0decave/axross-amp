//! axross-amp as a library: the parts of the player (the binary is
//! `src/main.rs`; tests drive the window through [`app::App`]).

pub mod analyzer;
pub mod app;
#[cfg(target_os = "linux")]
pub mod bluetooth;
#[cfg(target_os = "linux")]
pub mod bluetooth_ui;
pub mod cover;
pub mod design;
pub mod desktop_theme;
pub mod detach;
pub mod display;
pub mod engine;
pub mod eq;
pub mod fspath;
pub mod icon;
pub mod limiter;
pub mod meters;
pub mod mixer;
pub mod mixer_ui;
pub mod mp4edit;
#[cfg(target_os = "linux")]
pub mod mpris;
pub mod net;
pub mod playlist;
pub mod ring;
pub mod saver;
pub mod state;
#[cfg(test)]
pub(crate) mod testdir;
pub mod theme;
pub mod viz;
