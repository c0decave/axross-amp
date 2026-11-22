//! The player in a real window (Xvfb, wgpu on whatever it finds - here
//! Mesa's llvmpipe), its sound through ALSA into a FIFO taken at a sound
//! card's pace: while it plays it draws no more frames than its cap, and
//! once the track has ended it costs (nearly) nothing - for a bar face and
//! for a shader face, whose pictures the CPU may draw. Needs Xvfb; without
//! it the test says loudly that it did not run. (And the libraries the
//! window's toolkit loads: libxkbcommon-x11 and the X libraries - CI
//! installs libxkbcommon-x11-0 on Ubuntu, whose runners lack it.)

#[path = "support/player.rs"]
mod player;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// CPU seconds the process has used (all its threads).
fn cpu_seconds(pid: u32) -> f64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    // (after the command's name in parentheses)
    let rest = stat.rsplit(')').next().unwrap_or("");
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ticks = |i: usize| {
        fields
            .get(i)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    // utime, stime: fields 14 and 15 of the whole line, 11 and 12 here
    (ticks(11) + ticks(12)) / 100.0
}

fn frames(stats: &Path) -> u64 {
    std::fs::read_to_string(stats)
        .ok()
        .and_then(|s| {
            s.trim()
                .strip_prefix("frames ")
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}

struct Run {
    face: &'static str,
    fps: f64,
    playing_cpu: f64,
    idle_cpu: f64,
    slow: bool,
    /// What it wrote (its log's last lines: why it drew nothing).
    said: String,
}

/// The last `n` lines of `text` that are not ALSA's own complaints.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with("ALSA lib ") && !l.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn run(
    face: &'static str,
    shaders: Option<&str>,
    size: (u32, u32),
    display: u32,
    home: &Path,
    music: &Path,
) -> Run {
    let config = home.join(".config/axross");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("media_player.json"),
        format!(r#"{{"visualizer": "{face}", "volume": 100, "playlist_open": true}}"#),
    )
    .unwrap();
    std::fs::write(
        config.join("axross-amp.json"),
        format!(r#"{{"window": [{}, {}]}}"#, size.0, size.1),
    )
    .unwrap();
    let fifo = home.join("sound.fifo");
    let _ = std::fs::remove_file(&fifo);
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        home.join(".asoundrc"),
        format!(
            "pcm.!default {{ type file slave.pcm \"nul\" file \"{}\" format \"raw\" }}\npcm.nul {{ type null }}\n",
            fifo.display()
        ),
    )
    .unwrap();
    // the sound card: the FIFO read at 48 kHz, two channels of f32
    let pace = fifo.clone();
    std::thread::spawn(move || {
        let Ok(mut f) = std::fs::File::open(&pace) else {
            return;
        };
        let started = Instant::now();
        let mut taken = 0u64;
        let mut buf = vec![0u8; 4096];
        while let Ok(n) = f.read(&mut buf) {
            if n == 0 {
                std::thread::sleep(Duration::from_millis(10));
                if started.elapsed() > Duration::from_secs(60) {
                    return;
                }
                continue;
            }
            taken += n as u64;
            let ahead = taken as f64 / 384_000.0 - started.elapsed().as_secs_f64();
            if ahead > 0.0 {
                std::thread::sleep(Duration::from_secs_f64(ahead));
            }
        }
    });
    let stats = home.join(format!("stats-{face}"));
    let log = std::fs::File::create(home.join(format!("app-{face}.log"))).unwrap();
    let runtime = home.join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_axross-amp"));
    command
        .arg("--new-instance")
        .arg(music)
        .env("DISPLAY", format!(":{display}"))
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("ALSA_CONFIG_PATH", "/usr/share/alsa/alsa.conf")
        .env("AMP_STATS", &stats)
        .env_remove("AMP_GPU_SHADERS")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdout(Stdio::null())
        .stderr(log);
    if let Some(v) = shaders {
        command.env("AMP_GPU_SHADERS", v);
    }
    let syslib = Path::new("/root/venvs/syslib/usr/lib/x86_64-linux-gnu");
    if syslib.exists() {
        let old = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        command.env("LD_LIBRARY_PATH", format!("{old}:{}", syslib.display()));
    }
    let app = Killed(command.spawn().expect("the player starts"));
    let pid = app.0.id();
    // (timed from its first frame - the track begins as the window comes
    // up: from the start, a debug build's coming up on a busy machine took
    // the "at rest" seconds into the track)
    let up = Instant::now() + Duration::from_secs(60);
    while frames(&stats) == 0 && Instant::now() < up {
        std::thread::sleep(Duration::from_millis(100));
    }
    let first = Instant::now();
    // playing: 5 s to settle (the frame rate finds its budget), then 4 s
    // counted
    std::thread::sleep(Duration::from_secs(5));
    let (f0, c0, t0) = (frames(&stats), cpu_seconds(pid), Instant::now());
    std::thread::sleep(Duration::from_secs(4));
    let secs = t0.elapsed().as_secs_f64();
    let (f1, c1) = (frames(&stats), cpu_seconds(pid));
    // the track (13 s) has ended: 4 s for the picture to fall to rest,
    // 3 s counted
    std::thread::sleep((first + Duration::from_secs(17)).saturating_duration_since(Instant::now()));
    let (c2, t2) = (cpu_seconds(pid), Instant::now());
    std::thread::sleep(Duration::from_secs(3));
    let idle = (cpu_seconds(pid) - c2) / t2.elapsed().as_secs_f64();
    let said = std::fs::read_to_string(home.join(format!("app-{face}.log"))).unwrap_or_default();
    Run {
        face,
        fps: (f1 - f0) as f64 / secs,
        playing_cpu: (c1 - c0) / secs,
        idle_cpu: idle,
        slow: said.contains("slow renderer"),
        said: tail(&said, 6),
    }
}

#[test]
fn in_a_real_window_it_keeps_its_frame_rate_and_rests_when_the_music_stops() {
    let xvfb = ["/usr/bin/Xvfb", "/usr/local/bin/Xvfb"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists());
    let Some(xvfb) = xvfb else {
        let message = "SKIPPED: no Xvfb - the real window was not measured";
        assert!(std::env::var_os("AMP_REQUIRE_X").is_none(), "{message}");
        eprintln!("\n*** {message} ***\n");
        return;
    };
    let home = player::Scratch::new("real-window");
    let display = 40 + std::process::id() % 50;
    let _x = Killed(
        Command::new(xvfb)
            .arg(format!(":{display}"))
            .args(["-screen", "0", "1600x1200x24"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb starts"),
    );
    std::thread::sleep(Duration::from_secs(1));
    let music = home.join("music.wav");
    player::write_chord(&music, 13);
    let mut report = Vec::new();
    // a bar face, a shader face as measured best, and the shader face drawn
    // by the CPU's thread (which once went on drawing at rest: 390 %)
    let (small, large) = ((620, 860), (1200, 980));
    for (face, shaders, size, name) in [
        ("spectrum", None, small, "spectrum"),
        ("plasma", None, small, "plasma"),
        ("plasma", Some("0"), small, "plasma-cpu"),
        ("cover", None, small, "cover"),
        ("spectrum", None, large, "spectrum-large"),
        ("milkdrop", None, large, "feedback-large"),
    ] {
        let r = run(face, shaders, size, display, &home.join(name), &music);
        report.push(format!(
            "{name} ({}x{}): {:.1} frames a second, {:.0} % CPU playing, {:.1} % at rest{}",
            size.0,
            size.1,
            r.fps,
            r.playing_cpu * 100.0,
            r.idle_cpu * 100.0,
            if r.slow { " (a slow renderer)" } else { "" }
        ));
        let cap = match (r.slow, face) {
            (true, "cover") => 4.0,
            (true, _) => 12.0,
            _ => 30.0,
        };
        // (it draws: on a slow renderer the rate may come down to a frame
        // every three seconds where a frame costs more than the budget gives
        // it - review 13: 3 at least took 1.1 to 1.6 cores at 2560 x 1440)
        assert!(
            r.fps >= 0.25,
            "{}: it drew nothing ({report:?}); it said:\n{}",
            r.face,
            r.said
        );
        assert!(
            r.fps <= cap * 1.15,
            "{}: above its cap ({report:?})",
            r.face
        );
        assert!(r.idle_cpu < 0.05, "{}: busy at rest ({report:?})", r.face);
        // (llvmpipe draws the whole window on the CPU: the frame rate is
        // lowered until it takes less than a core - as built for use; a
        // debug build's own work is several times dearer: `cargo test
        // --release --test real_window` holds it to the core)
        let most = match (r.slow, cfg!(debug_assertions)) {
            (true, false) => 1.0,
            (true, true) => 1.5,
            _ => 2.0,
        };
        assert!(
            r.playing_cpu < most,
            "{}: {:.0} % CPU ({report:?})",
            r.face,
            r.playing_cpu * 100.0
        );
    }
    println!("{}", report.join("\n"));
}

/// SIGTERM (a logout, `kill`) closes the window as its close button does -
/// the program ends of itself, its settings kept and what a display view
/// took put back (`on_exit`) - not cut off by the signal.
#[test]
#[cfg(unix)]
fn sigterm_closes_the_window_as_its_close_button_does() {
    let xvfb = ["/usr/bin/Xvfb", "/usr/local/bin/Xvfb"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists());
    let Some(xvfb) = xvfb else {
        let message = "SKIPPED: no Xvfb - the signal was not tried";
        assert!(std::env::var_os("AMP_REQUIRE_X").is_none(), "{message}");
        eprintln!("\n*** {message} ***\n");
        return;
    };
    let home = player::Scratch::new("real-window-sigterm");
    let display = 40 + (std::process::id() + 7) % 50;
    let _x = Killed(
        Command::new(xvfb)
            .arg(format!(":{display}"))
            .args(["-screen", "0", "1280x1024x24"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb starts"),
    );
    std::thread::sleep(Duration::from_secs(1));
    let runtime = home.join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_axross-amp"));
    command
        .args(["--new-instance", "--foreground"])
        .env("DISPLAY", format!(":{display}"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(home.join("app.log")).unwrap());
    let syslib = Path::new("/root/venvs/syslib/usr/lib/x86_64-linux-gnu");
    if syslib.exists() {
        let old = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        command.env("LD_LIBRARY_PATH", format!("{old}:{}", syslib.display()));
    }
    let mut app = Killed(command.spawn().expect("the player starts"));
    // (the window up: its settings written once)
    let own = home.join(".config/axross/axross-amp.json");
    let started = Instant::now();
    while !own.exists() && started.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_secs(2));
    // SAFETY: a signal to our own child
    unsafe { libc::kill(app.0.id() as libc::pid_t, libc::SIGTERM) };
    let ended = Instant::now();
    let status = loop {
        if let Some(s) = app.0.try_wait().unwrap() {
            break Some(s);
        }
        if ended.elapsed() > Duration::from_secs(15) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let said = tail(
        &std::fs::read_to_string(home.join("app.log")).unwrap_or_default(),
        6,
    );
    let status =
        status.unwrap_or_else(|| panic!("it did not end within 15 s of SIGTERM; it said:\n{said}"));
    assert_eq!(
        status.code(),
        Some(0),
        "cut off by the signal, not closed ({status:?}); it said:\n{said}"
    );
}

/// A library the window's toolkit loads by name at its start, missing
/// (GitHub's Ubuntu runners have no libxkbcommon-x11): the program says
/// which and the package that has it, exit status 1 - winit panicked
/// (exit status 101) with "Library libxkbcommon-x11.so could not be
/// loaded.". No display is needed: it is said before one is asked for.
#[test]
#[cfg(target_os = "linux")]
fn a_missing_window_library_is_said_not_a_panic() {
    let home = player::Scratch::new("real-window-nolib");
    // (an empty file of the library's name first in the search path: it
    // cannot be loaded, as where it is not installed)
    let hide = home.join("hide");
    std::fs::create_dir_all(&hide).unwrap();
    std::fs::write(hide.join("libxkbcommon-x11.so.0"), b"").unwrap();
    let old = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
    let runtime = home.join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axross-amp"))
        .args(["--new-instance", "--foreground"])
        .env("DISPLAY", ":97")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WINIT_UNIX_BACKEND")
        .env("HOME", &*home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("LD_LIBRARY_PATH", format!("{}:{old}", hide.display()))
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains("the window cannot be opened: libxkbcommon-x11.so.0 is not installed")
            && said.contains("libxkbcommon-x11-0"),
        "{said}"
    );
    assert!(!said.contains("panicked"), "{said}");
}
