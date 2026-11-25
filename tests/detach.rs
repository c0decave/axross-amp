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
    // (up to 20 s: a shared CI runner, busy with the suite beside it, may take
    // seconds to start the background child - this waits for it, it does not
    // time it)
    for _ in 0..400 {
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
    // (a window started would stay its 5 s: back sooner, it handed over -
    // with room for a busy machine, where 3 s was close)
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5), "{took:?}");
    std::thread::sleep(Duration::from_millis(500));
    assert!(!pidfile.exists(), "a second player was started");
    let _ = first.kill();
    let _ = first.wait();
    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// The program where an update puts it (not a build folder: the updater's
/// watch is for a release's program), the marker of its first start as an
/// update by itself leaves it, and a stand-in for the program it replaced
/// that says when it runs.
fn updated_program(home: &Path) -> (std::path::PathBuf, Vec<u8>) {
    use std::os::unix::fs::PermissionsExt;
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("axross-amp");
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_axross-amp")).unwrap();
    std::fs::write(&program, &bytes).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = bin.join("axross-amp.previous");
    std::fs::write(
        &old,
        format!("#!/bin/sh\ntouch '{}'\n", home.join("old-ran").display()),
    )
    .unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        bin.join("axross-amp.first-start"),
        format!("{}\ninstalled\n", env!("CARGO_PKG_VERSION")),
    )
    .unwrap();
    (program, bytes)
}

fn updated_command(home: &Path, program: &Path) -> Command {
    let mut c = Command::new(program);
    c.env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("AXROSS_AMP_NO_UPDATE_CHECK", "1")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("AXROSS_AMP_DETACHED")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

/// An update put in place by itself, started first from a terminal: the
/// start goes to the background, and the child that opens the window is
/// the one watched - the update stays (review 17, H1: the parent marked the
/// first start, the child took it for a start that never drew its window,
/// put the old program back and refused the new one for good).
#[test]
fn an_updates_first_start_from_a_terminal_keeps_it() {
    let home = Scratch::new("first-start-tty");
    let (program, bytes) = updated_program(&home);
    let pidfile = home.join("pid");
    let out = updated_command(&home, &program)
        .env("AXROSS_AMP_ASSUME_TTY", "1")
        .env("AXROSS_AMP_FAKE_WINDOW", "2")
        .env("AXROSS_AMP_FAKE_PIDFILE", &pidfile)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let child = read_pid(&pidfile).expect("the background child started");
    let end = Instant::now() + Duration::from_secs(10);
    while alive(child) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let bin = home.join("bin");
    assert!(!home.join("old-ran").exists(), "the old program was run");
    assert_eq!(
        std::fs::read(&program).unwrap(),
        bytes,
        "the update was undone"
    );
    assert!(
        !bin.join("axross-amp.refused").exists(),
        "the update was refused"
    );
    assert!(
        !bin.join("axross-amp.first-start").exists(),
        "its first start, up, still watched"
    );
}

/// A file opened while the player that installed the update still runs:
/// the start hands the file over and ends - it is not the update's first
/// start, and its watch is left as it was (review 17, H1: marked
/// "starting" by it, the next real start put the old program back).
#[cfg(target_os = "linux")]
#[test]
fn a_start_handing_its_files_over_leaves_the_first_start_watch() {
    use std::io::{BufRead, BufReader};
    let home = Scratch::new("first-start-hand");
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
    // the player running (the old one, as it was: from the build)
    let mut first = amp(&home)
        .args(["--foreground"])
        .env("DBUS_SESSION_BUS_ADDRESS", &bus)
        .env("AXROSS_AMP_FAKE_WINDOW", "8")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    // the update in place meanwhile; a file opened with it
    let (program, _) = updated_program(&home);
    let track = home.join("a.wav");
    std::fs::write(&track, b"").unwrap();
    let out = updated_command(&home, &program)
        .arg(&track)
        .env("DBUS_SESSION_BUS_ADDRESS", &bus)
        .env("AXROSS_AMP_FAKE_WINDOW", "5")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let marker = std::fs::read_to_string(home.join("bin/axross-amp.first-start")).unwrap();
    assert!(
        marker.contains("installed"),
        "the hand-over took the first start: {marker:?}"
    );
    assert!(!home.join("bin/axross-amp.refused").exists());
    let _ = first.kill();
    let _ = first.wait();
    let _ = daemon.kill();
    let _ = daemon.wait();
}
