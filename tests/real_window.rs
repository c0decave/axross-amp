//! The player in a real window (Xvfb, wgpu on whatever it finds - here
//! Mesa's llvmpipe), its sound through ALSA into a FIFO taken at a sound
//! card's pace: while it plays it draws no more frames than its cap, and
//! once the track has ended it costs (nearly) nothing - for a bar face and
//! for a shader face, whose pictures the CPU may draw. Needs Xvfb; without
//! it the test says loudly that it did not run.

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
}

fn run(face: &'static str, shaders: Option<&str>, display: u32, home: &Path, music: &Path) -> Run {
    let config = home.join(".config/axross");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("media_player.json"),
        format!(r#"{{"visualizer": "{face}", "volume": 100, "playlist_open": true}}"#),
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
    // playing: 2 s to settle, then 3 s counted
    std::thread::sleep(Duration::from_secs(4));
    let (f0, c0, t0) = (frames(&stats), cpu_seconds(pid), Instant::now());
    std::thread::sleep(Duration::from_secs(3));
    let secs = t0.elapsed().as_secs_f64();
    let (f1, c1) = (frames(&stats), cpu_seconds(pid));
    // the track (8 s) has ended: 3 s for the picture to fall to rest, 3 s counted
    std::thread::sleep(Duration::from_secs(5));
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
            .args(["-screen", "0", "1024x900x24"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Xvfb starts"),
    );
    std::thread::sleep(Duration::from_secs(1));
    let music = home.join("music.wav");
    player::write_chord(&music, 8);
    let mut report = Vec::new();
    // a bar face, a shader face as measured best, and the shader face drawn
    // by the CPU's thread (which once went on drawing at rest: 390 %)
    for (face, shaders, name) in [
        ("spectrum", None, "spectrum"),
        ("plasma", None, "plasma"),
        ("plasma", Some("0"), "plasma-cpu"),
    ] {
        let r = run(face, shaders, display, &home.join(name), &music);
        report.push(format!(
            "{name}: {:.1} frames a second, {:.0} % CPU playing, {:.1} % at rest{}",
            r.fps,
            r.playing_cpu * 100.0,
            r.idle_cpu * 100.0,
            if r.slow {
                " (a slow renderer: 20 fps)"
            } else {
                ""
            }
        ));
        let cap = if r.slow { 20.0 } else { 30.0 };
        assert!(r.fps > 3.0, "{}: it drew nothing ({report:?})", r.face);
        assert!(
            r.fps <= cap * 1.15,
            "{}: above its cap ({report:?})",
            r.face
        );
        assert!(r.idle_cpu < 0.05, "{}: busy at rest ({report:?})", r.face);
        // (llvmpipe draws the whole window on the CPU: a few cores' worth,
        // not more)
        assert!(
            r.playing_cpu < 2.6,
            "{}: {:.0} % CPU ({report:?})",
            r.face,
            r.playing_cpu * 100.0
        );
    }
    println!("{}", report.join("\n"));
}
