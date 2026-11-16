//! axross-amp as a library: the parts of the player (the binary is
//! `src/main.rs`; tests drive the window through [`app::App`]).

pub mod analyzer;
pub mod app;
pub mod engine;
pub mod eq;
pub mod mixer;
pub mod mixer_ui;
pub mod mp4edit;
pub mod playlist;
pub mod state;
pub mod viz;
