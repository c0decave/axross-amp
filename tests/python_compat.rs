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
    assert_eq!(s.tracks.len(), 5, "{:?}", s.tracks);
    assert_eq!(s.tracks[3].path, "https://radio.example/live.mp3");
    // a name that is no UTF-8, written by axross as the escape "\udce9" (a
    // lone surrogate serde_json refuses: the whole file was dropped, review
    // 12): its bytes as on the disk
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            axross_amp::fspath::to_path(&s.tracks[4].path)
                .as_os_str()
                .as_bytes(),
            b"/music/caf\xe9 del mar.mp3"
        );
    }
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

/// A track added here while axross runs, then axross saves with its own
/// code (its list as loaded, its merge): the track stays in the file, and
/// this program does not take a list without it over (review 14, M3: the
/// file went back to axross's list and the window dropped the track).
#[test]
fn a_track_added_here_survives_a_save_by_axross() {
    use axross_amp::state::{PlayerState, read_state, write_state_merged};
    let Some((python, axross)) = axross_python() else {
        let message = "SKIPPED: no axross with its Python: axross's save was not tried";
        assert!(
            std::env::var_os("AMP_REQUIRE_PYTHON").is_none(),
            "{message}"
        );
        eprintln!("\n*** {message} ***\n");
        return;
    };
    let dir = scratch::Scratch::new("python-stale");
    let path = dir.join("media_player.json");
    let t = |p: &str| Track {
        path: p.into(),
        ..Default::default()
    };
    let mut state = PlayerState {
        tracks: vec![t("/m/1.mp3"), t("/m/2.mp3")],
        current: 0,
        volume: 50,
        ..Default::default()
    };
    let mut base = None;
    write_state_merged(&path, &state, &mut base).unwrap();
    // (axross reads the file at its start: its base)
    let axross_base = dir.join("axross-base.json");
    std::fs::copy(&path, &axross_base).unwrap();
    state.tracks.push(t("/m/added-here.mp3"));
    write_state_merged(&path, &state, &mut base).unwrap();
    // axross saves its own state (its volume changed): its list as loaded
    let own = serde_json::json!({
        "volume": 60, "muted": false, "shuffle": false, "repeat": "off",
        "tracks": [{"path": "/m/1.mp3", "title": "", "duration": 0},
                   {"path": "/m/2.mp3", "title": "", "duration": 0}],
        "current": 0, "position_ms": 0, "visualizer": "spectrum",
        "equalizer_open": false, "visual_scheme": "classic", "visual_custom": {},
        "visual_options": {}, "playlist_open": true
    });
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/axross_save.py");
    let mut child = Command::new(&python)
        .arg("-I")
        .arg(&script)
        .arg(&axross)
        .arg(&path)
        .arg(&axross_base)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(own.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "axross's save: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let paths =
        |p: &Path| -> Vec<String> { read_state(p).tracks.into_iter().map(|t| t.path).collect() };
    assert_eq!(
        paths(&path).len(),
        3,
        "axross's save dropped it: {:?}",
        paths(&path)
    );
    assert_eq!(read_state(&path).volume, 60, "axross's own value");
    // this program's next save: the list as it is, nothing for the window
    // to take over without the track
    state.position_ms = 5000;
    let kept = write_state_merged(&path, &state, &mut base).unwrap();
    assert_eq!(paths(&path).len(), 3);
    if let Some(tracks) = kept.get("tracks") {
        assert_eq!(axross_amp::state::tracks_of(tracks).len(), 3);
    }
}

/// Both programs edit the list while both run, with axross's own merge
/// (review 15, M3): a track removed in axross keeps the track added here
/// (S2), tracks added in both are both kept (S3), and a track added here
/// that the user removes in axross stays removed (S1).
#[test]
fn both_programs_editing_the_list_keep_each_others_edits() {
    use axross_amp::state::{PlayerState, read_state, write_state_merged};
    let Some((python, axross)) = axross_python() else {
        let message = "SKIPPED: no axross with its Python: the shared edits were not tried";
        assert!(
            std::env::var_os("AMP_REQUIRE_PYTHON").is_none(),
            "{message}"
        );
        eprintln!("\n*** {message} ***\n");
        return;
    };
    let t = |p: &str| Track {
        path: p.into(),
        ..Default::default()
    };
    let entry = |p: &str| serde_json::json!({"path": p, "title": "", "duration": 0});
    // axross saves `shown` (its list), its base being `base` (a file's content)
    let axross_saves = |path: &Path, base: &Path, shown: &[&str]| {
        let own = serde_json::json!({
            "volume": 60, "muted": false, "shuffle": false, "repeat": "off",
            "tracks": shown.iter().map(|p| entry(p)).collect::<Vec<_>>(),
            "current": 0, "position_ms": 0, "visualizer": "spectrum",
            "equalizer_open": false, "visual_scheme": "classic", "visual_custom": {},
            "visual_options": {}, "playlist_open": true
        });
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/axross_save.py");
        let mut child = Command::new(&python)
            .arg("-I")
            .arg(&script)
            .arg(&axross)
            .arg(path)
            .arg(base)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(own.to_string().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let paths =
        |p: &Path| -> Vec<String> { read_state(p).tracks.into_iter().map(|t| t.path).collect() };
    let dir = scratch::Scratch::new("python-both-edit");
    // (here: the list edited here; axross: axross's list; the list both
    // programs keep. S6: a reorder in one program and an addition in the
    // other - review 16, M7: the reorder was undone)
    let added = vec!["/m/1", "/m/2", "/m/3", "/m/X"];
    for (case, here, axross_list, want) in [
        (
            "S2",
            added.clone(),
            vec!["/m/1", "/m/3"],
            vec!["/m/1", "/m/3", "/m/X"],
        ),
        (
            "S3",
            added.clone(),
            vec!["/m/1", "/m/2", "/m/3", "/m/Y"],
            vec!["/m/1", "/m/2", "/m/3", "/m/X", "/m/Y"],
        ),
        (
            "S6a",
            added.clone(),
            vec!["/m/3", "/m/1", "/m/2"],
            vec!["/m/3", "/m/1", "/m/2", "/m/X"],
        ),
        (
            "S6b",
            vec!["/m/3", "/m/1", "/m/2"],
            vec!["/m/1", "/m/2", "/m/3", "/m/Y"],
            vec!["/m/3", "/m/1", "/m/2", "/m/Y"],
        ),
    ] {
        let path = dir.join(format!("{case}.json"));
        let mut state = PlayerState {
            tracks: vec![t("/m/1"), t("/m/2"), t("/m/3")],
            current: 0,
            ..Default::default()
        };
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        let axross_base = dir.join(format!("{case}-axross-base.json"));
        std::fs::copy(&path, &axross_base).unwrap();
        // both running: the list edited here, and in axross
        state.tracks = here.iter().map(|p| t(p)).collect();
        write_state_merged(&path, &state, &mut base).unwrap();
        axross_saves(&path, &axross_base, &axross_list);
        assert_eq!(paths(&path), want, "{case}: axross's save");
        // here the next save, the window taking it over
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path), want, "{case}: this program's save");
        if let Some(tracks) = kept.get("tracks") {
            state.tracks = axross_amp::state::tracks_of(tracks);
        }
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path), want, "{case}: after the window took it over");
    }
    // S1: X added here; axross started anew (its base: the file with X);
    // the user removes X in axross: it stays removed
    let path = dir.join("S1.json");
    let mut state = PlayerState {
        tracks: vec![t("/m/1"), t("/m/2")],
        current: 0,
        ..Default::default()
    };
    let mut base = None;
    write_state_merged(&path, &state, &mut base).unwrap();
    state.tracks.push(t("/m/X"));
    write_state_merged(&path, &state, &mut base).unwrap();
    let axross_base = dir.join("S1-axross-base.json");
    std::fs::copy(&path, &axross_base).unwrap();
    axross_saves(&path, &axross_base, &["/m/1", "/m/2"]);
    assert_eq!(paths(&path), ["/m/1", "/m/2"]);
    let kept = write_state_merged(&path, &state, &mut base).unwrap();
    assert_eq!(paths(&path), ["/m/1", "/m/2"], "the removal undone here");
    assert_eq!(
        kept.get("tracks")
            .map(|v| axross_amp::state::tracks_of(v).len()),
        Some(2),
        "the window keeps the removed track"
    );
}
