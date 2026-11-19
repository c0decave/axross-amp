//! Started from a terminal the player goes to the background: the prompt
//! back at once, the player on in a session of its own; `--foreground`
//! keeps it there; `--help` and `--version` never go; a second start that
//! hands its files to the player running starts nothing. The window is
//! stood in for by AXROSS_AMP_FAKE_WINDOW (up at once, then N seconds of
//! nothing): no display needed. AXROSS_AMP_ASSUME_TTY takes the tests' pipes
//! for a terminal.
#![cfg(unix)]

#[path = "support/scratch.rs"]
mod scratch;

use scratch::Scratch;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn amp(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_axross-amp"));
    c.env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("AXROSS_AMP_DETACHED")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .unwrap_or_default()
            .contains(") Z ")
}

/// The session a process is in (/proc's stat: after the name, state,
/// parent, group, session).
fn session(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(3)?.parse().ok()
}

fn read_pid(path: &Path) -> Option<u32> {
    for _ in 0..50 {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse()
        {
            return Some(pid);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn from_a_terminal_it_goes_to_the_background_and_stays() {
    let home = Scratch::new("detach-bg");
    let pidfile = home.join("pid");
    let started = Instant::now();
    let out = amp(&home)
        .args(["--new-instance"])
        .env("AXROSS_AMP_ASSUME_TTY", "1")
        .env("AXROSS_AMP_FAKE_WINDOW", "4")
        .env("AXROSS_AMP_FAKE_PIDFILE", &pidfile)
        .output()
        .unwrap();
    let took = started.elapsed();
    assert!(out.status.success(), "{out:?}");
    assert!(
        took < Duration::from_millis(1500),
        "the prompt came back after {took:?}"
    );
    let pid = read_pid(&pidfile).expect("no player started");
    // (the parent is gone; the player goes on, in a session of its own -
    // no terminal to hang up on it)
    assert!(alive(pid), "the player ended with its parent");
    assert_eq!(session(pid), Some(pid), "not a session of its own");
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

#[test]
fn foreground_stays_in_the_terminal() {
    let home = Scratch::new("detach-fg");
    for flag in ["--foreground", "-F"] {
        let started = Instant::now();
        let out = amp(&home)
            .args([flag, "--new-instance"])
            .env("AXROSS_AMP_ASSUME_TTY", "1")
            .env("AXROSS_AMP_FAKE_WINDOW", "2")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert!(
            started.elapsed() >= Duration::from_secs(2),
            "{flag}: it went to the background"
        );
    }
}

#[test]
fn not_from_a_terminal_it_just_runs() {
    // (a desktop entry, a file manager: stdio no terminal)
    let home = Scratch::new("detach-notty");
    let started = Instant::now();
    let out = amp(&home)
        .args(["--new-instance"])
        .env("AXROSS_AMP_FAKE_WINDOW", "2")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "it went to the background"
    );
}

#[test]
fn help_and_version_never_go_to_the_background() {
    let home = Scratch::new("detach-help");
    let pidfile = home.join("pid");
    for (flag, says) in [("--help", "--foreground"), ("--version", "axross-amp ")] {
        let out = amp(&home)
            .arg(flag)
            .env("AXROSS_AMP_ASSUME_TTY", "1")
            .env("AXROSS_AMP_FAKE_WINDOW", "5")
            .env("AXROSS_AMP_FAKE_PIDFILE", &pidfile)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(says),
            "{flag}: {out:?}"
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(!pidfile.exists(), "a player was started");
}

#[test]
fn a_start_that_fails_says_why() {
    // (no display: the child ends at once - its parent prints the log's
    // last lines and ends with 1)
    let home = Scratch::new("detach-fail");
    let out = amp(&home)
        .args(["--new-instance"])
        .env("AXROSS_AMP_ASSUME_TTY", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("could not start"), "{err}");
    assert!(err.contains("axross-amp.log"), "{err}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_start_handing_its_files_over_starts_nothing() {
    use std::io::{BufRead, BufReader};
    let home = Scratch::new("detach-hand");
    let Ok(mut daemon) = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address=unix:dir={}", home.display()))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    let mut bus = String::new();
    BufReader::new(daemon.stdout.take().unwrap())
        .read_line(&mut bus)
        .unwrap();
    let bus = bus.trim().to_owned();
    // the player running (in the foreground: this test holds it)
    let mut first = amp(&home)
        .args(["--foreground"])
        .env("DBUS_SESSION_BUS_ADDRESS", &bus)
        .env("AXROSS_AMP_FAKE_WINDOW", "8")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    // a second start from a terminal, with a file: handed over, nothing started
    let pidfile = home.join("pid2");
    let track = home.join("a.wav");
    std::fs::write(&track, b"").unwrap();
    let started = Instant::now();
    let out = amp(&home)
        .arg(&track)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus)
        .env("AXROSS_AMP_ASSUME_TTY", "1")
        .env("AXROSS_AMP_FAKE_WINDOW", "5")
        .env("AXROSS_AMP_FAKE_PIDFILE", &pidfile)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!pidfile.exists(), "a second player was started");
    let _ = first.kill();
    let _ = first.wait();
    let _ = daemon.kill();
    let _ = daemon.wait();
}
