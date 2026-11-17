//! axross-amp as a library: the parts of the player (the binary is
//! `src/main.rs`; tests drive the window through [`app::App`]).

pub mod analyzer;
pub mod app;
#[cfg(target_os = "linux")]
pub mod bluetooth;
#[cfg(target_os = "linux")]
pub mod bluetooth_ui;
pub mod engine;
pub mod eq;
pub mod mixer;
pub mod mixer_ui;
pub mod mp4edit;
#[cfg(target_os = "linux")]
pub mod mpris;
pub mod net;
pub mod playlist;
pub mod ring;
pub mod state;
pub mod viz;
