//! An output failing at every write - ALSA's file plugin writing to a full
//! disk (/dev/full), as with its reader gone - says so once, then at most
//! every 10 s with how many more: the program's own lines and alsa-lib's
//! (review of 0.17.1, H1: 38 000 lines of alsa-lib and 93 of the program's
//! a minute, 290 MB an hour of log). And it is tried again once a round,
//! each round longer (H2: some 13 streams every 5 s). Each kind of line on
//! its own: alsa-lib's flood does not keep the output's own errors unsaid
//! (review 2 of 0.17.1, L-N4), and alsa-lib's are said with their
//! arguments (L-N3). A log that cannot be written loses lines, nothing
//! else (N1: the player aborted). Linux with ALSA; its own process, its own
//! ~/.asoundrc.

#![cfg(target_os = "linux")]

#[path = "support/scratch.rs"]
mod scratch;

use std::io::Write;
use std::time::{Duration, Instant};

const SECS: u64 = 25;

/// ALSA's file plugin writing to a full disk: every write fails.
const FULL_DISK: &str =
    "pcm.!default {\n type file\n slave.pcm \"null\"\n file \"/dev/full\"\n format \"raw\"\n}\n";
/// A default output that is not there: alsa-lib says so at every open.
const NO_SUCH_PCM: &str = "pcm.!default {\n type plug\n slave.pcm \"nosuchpcm\"\n}\n";

/// The engine playing for `secs` in a process of its own (this test,
/// `test`, again: [`child`]), with `asoundrc`, its stderr to `stderr` (None:
/// read).
fn engine_alone(
    test: &str,
    name: &str,
    asoundrc: &str,
    secs: u64,
    stderr: Option<&str>,
) -> std::process::Output {
    let home = scratch::Scratch::new(name);
    std::fs::write(home.join(".asoundrc"), asoundrc).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", test, "--nocapture", "--test-threads", "1"])
        .env("AMP_FLOOD_CHILD", secs.to_string())
        .env("HOME", home.path())
        .env("PULSE_SERVER", "unix:/nonexistent")
        .env_remove("PIPEWIRE_REMOTE");
    if let Some(path) = stderr {
        child.stderr(std::fs::OpenOptions::new().write(true).open(path).unwrap());
    }
    child.output().unwrap()
}

#[test]
fn an_output_failing_at_every_write_says_so_seldom() {
    if std::env::var_os("AMP_FLOOD_CHILD").is_some() {
        return child();
    }
    let out = engine_alone(
        "an_output_failing_at_every_write_says_so_seldom",
        "output-flood",
        FULL_DISK,
        SECS,
        None,
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let alsa = stderr.lines().filter(|l| l.starts_with("ALSA lib")).count();
    let ours = stderr
        .lines()
        .filter(|l| l.starts_with("axross-amp: output"))
        .count();
    println!("{SECS} s: {alsa} lines of alsa-lib, {ours} of the output's\n{stdout}");
    assert!(out.status.success(), "{stdout}\n{stderr}");
    // (each kind said once, then every 10 s: a few in 25 s)
    let mut kinds = std::collections::HashMap::<&str, usize>::new();
    for line in stderr
        .lines()
        .filter(|l| l.starts_with("ALSA lib") || l.starts_with("axross-amp: output"))
    {
        let kind = line.split(" (").next().unwrap_or(line);
        *kinds.entry(kind).or_default() += 1;
    }
    assert!(
        alsa + ours <= 12 && kinds.values().all(|n| *n <= 3),
        "{alsa} + {ours} lines, {kinds:?}:\n{}",
        &stderr[..stderr.len().min(3000)]
    );
    // (the output's own error said too, not hidden by alsa-lib's flood)
    assert!(ours >= 1, "the output's own unsaid:\n{stderr}");
    // (alsa-lib's line with its arguments: which file - not "%s")
    assert!(
        stderr.contains("/dev/full write failed") && !stderr.contains("%s"),
        "{stderr}"
    );
    // the streams made: four at first (three made good), then one a round
    // - after 5 s, 10 s more
    let opens: usize = stdout
        .lines()
        .find_map(|l| l.strip_prefix("opens "))
        .and_then(|n| n.trim().parse().ok())
        .expect("the child said how many");
    assert!(
        (1..=7).contains(&opens),
        "{opens} streams made in {SECS} s:\n{stdout}"
    );
}

/// A log that cannot be written (stderr on a full disk): lines lost, the
/// player on - it aborted at alsa-lib's first line, a panic out of its C
/// callback (review 2 of 0.17.1, N1). With an output failing at every
/// write, and with one that is not there.
#[test]
fn a_log_that_cannot_be_written_loses_lines_and_nothing_else() {
    if std::env::var_os("AMP_FLOOD_CHILD").is_some() {
        return child();
    }
    for (name, asoundrc) in [("full-disk", FULL_DISK), ("no-such-pcm", NO_SUCH_PCM)] {
        let out = engine_alone(
            "a_log_that_cannot_be_written_loses_lines_and_nothing_else",
            &format!("log-full-{name}"),
            asoundrc,
            5,
            Some("/dev/full"),
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("opens "),
            "{name}: {:?}\n{stdout}",
            out.status
        );
    }
}

/// The engine on the failing output, playing, for the seconds it was given
/// (no thread of it may panic).
fn child() {
    let secs: u64 = std::env::var("AMP_FLOOD_CHILD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SECS);
    static PANICKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        PANICKED.store(true, std::sync::atomic::Ordering::Relaxed);
        println!("panicked: {info}");
        hook(info);
    }));
    let dir = scratch::Scratch::new("output-flood-child");
    let wav = dir.join("long.wav");
    let rate = 48_000u32;
    let frames = rate * 120;
    let mut f = std::fs::File::create(&wav).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + frames * 4).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 4).to_le_bytes(),
        &4u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        f.write_all(v).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&(frames * 4).to_le_bytes()).unwrap();
    f.write_all(&vec![1u8; (frames * 4) as usize]).unwrap();
    drop(f);
    let engine = axross_amp::engine::Engine::start(|| {});
    engine.send(axross_amp::engine::Command::Load {
        path: wav.to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    let start = Instant::now();
    let mut opens = 0;
    while start.elapsed() < Duration::from_secs(secs) {
        while let Ok(e) = engine.events.try_recv() {
            if let axross_amp::engine::Event::Output(o) = e {
                println!("{:6.2} s: {o}", start.elapsed().as_secs_f32());
                if !o.starts_with("no sound") {
                    opens += 1;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !PANICKED.load(std::sync::atomic::Ordering::Relaxed),
        "a thread of the engine panicked"
    );
    println!("opens {opens}");
}
