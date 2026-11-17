//! The state file is shared with the Python player inside axross: a file
//! the Python player wrote (AMP_PY_STATE) reads here as it meant, and what
//! this program writes (to AMP_RS_STATE) is read back by the Python player
//! (checked by the script that made the first file). Without the variables
//! the test passes (CI without axross).

use axross_amp::state::{self, Track};

#[test]
fn a_python_state_reads_and_ours_is_written_for_python() {
    let (Some(py), Some(rs)) = (
        std::env::var_os("AMP_PY_STATE"),
        std::env::var_os("AMP_RS_STATE"),
    ) else {
        eprintln!("AMP_PY_STATE / AMP_RS_STATE not set: skipped");
        return;
    };
    let mut s = state::read_state(std::path::Path::new(&py));
    assert_eq!(s.volume, 37);
    assert!(s.muted && s.shuffle);
    assert_eq!(s.repeat, "one");
    assert_eq!(s.visualizer, "radial");
    assert_eq!(s.tracks.len(), 4);
    assert_eq!(s.tracks[3].path, "https://radio.example/live.mp3");
    assert_eq!(s.visual_options.bars, 32);
    assert_eq!(
        s.visual_custom.get("low").map(String::as_str),
        Some("#1e90ff")
    );
    // what the Rust player changes
    std::fs::copy(&py, &rs).unwrap(); // (keys only Python knows must survive)
    s.volume = 64;
    s.muted = false;
    s.repeat = "all".into();
    s.current = 1;
    s.position_ms = 1500;
    s.visualizer = "waterfall".into();
    s.equalizer_open = false;
    s.tracks[0].title = "Renamed by Rust".into();
    s.tracks.push(Track {
        path: "/music/tone.m4a".into(),
        title: String::new(),
        duration_ms: 3000,
        ..Default::default()
    });
    state::write_state(std::path::Path::new(&rs), &s).unwrap();
}
