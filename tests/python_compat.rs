//! The state file is shared with the Python player inside axross: a file
//! axross's own code writes (tests/support/python_state.py, through its
//! media_player.py) reads here as it meant, and what this program writes is
//! read back by axross's code as its player restores it.
//!
//! axross is found at AMP_AXROSS (else /work/axross) with the Python of its
//! venv at AMP_PYTHON (else /root/venvs/axross/bin/python); AMP_PY_STATE /
//! AMP_RS_STATE give files made elsewhere instead. Without any of them the
//! test says loudly that it did not run (AMP_REQUIRE_PYTHON=1: it fails).

use axross_amp::state::{self, Track};
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "support/scratch.rs"]
mod scratch;

/// axross's Python, its source, and the environment its Qt wants.
fn axross_python() -> Option<(PathBuf, PathBuf)> {
    let python = std::env::var_os("AMP_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root/venvs/axross/bin/python"));
    let axross = std::env::var_os("AMP_AXROSS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/work/axross"));
    (python.exists() && axross.join("resources/extensions/media_player.py").exists())
        .then_some((python, axross))
}

fn run_python(python: &Path, axross: &Path, mode: &str, file: &Path) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/python_state.py");
    let mut command = Command::new(python);
    command
        .arg("-I")
        .arg(&script)
        .arg(axross)
        .arg(mode)
        .arg(file)
        .env("QT_QPA_PLATFORM", "offscreen");
    // (the Qt libraries axross's venv was built against, where they are)
    let syslib = Path::new("/root/venvs/syslib/usr/lib/x86_64-linux-gnu");
    if syslib.exists() {
        command.env(
            "LD_LIBRARY_PATH",
            format!("{}:{}/pulseaudio", syslib.display(), syslib.display()),
        );
    }
    command.output().expect("axross's Python runs")
}

#[test]
fn a_python_state_reads_and_ours_is_written_for_python() {
    let dir = scratch::Scratch::new("python-compat");
    let made_here = axross_python();
    let (py, rs) = match (
        std::env::var_os("AMP_PY_STATE"),
        std::env::var_os("AMP_RS_STATE"),
        &made_here,
    ) {
        (Some(py), Some(rs), _) => (PathBuf::from(py), PathBuf::from(rs)),
        (_, _, Some((python, axross))) => {
            let py = dir.join("python.json");
            let out = run_python(python, axross, "write", &py);
            assert!(
                out.status.success(),
                "axross could not write its state: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            (py, dir.join("rust.json"))
        }
        _ => {
            let message = "SKIPPED: no axross (AMP_AXROSS) with its Python (AMP_PYTHON): \
                           the state file was not checked against axross's own code";
            assert!(
                std::env::var_os("AMP_REQUIRE_PYTHON").is_none(),
                "{message}"
            );
            eprintln!("\n*** {message} ***\n");
            return;
        }
    };
    let mut s = state::read_state(std::path::Path::new(&py));
    assert_eq!(s.volume, 37);
    assert!(s.muted && s.shuffle);
    assert_eq!(s.repeat, "one");
    assert_eq!(s.visualizer, "radial");
    assert_eq!(s.tracks.len(), 4);
    assert_eq!(s.tracks[3].path, "https://radio.example/live.mp3");
    // (axross keeps durations in seconds)
    assert_eq!(s.tracks[0].duration_ms, 61_000);
    assert_eq!(s.tracks[2].duration_ms, 2_000);
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
    // axross reads it back as its player would
    if let Some((python, axross)) = &made_here {
        let out = run_python(python, axross, "check", &rs);
        assert!(
            out.status.success(),
            "axross's player reads it wrongly:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
