//! An output failing at every write - ALSA's file plugin writing to a full
//! disk (/dev/full), as with its reader gone - says so once, then at most
//! every 10 s with how many more: the program's own lines and alsa-lib's
//! (review of 0.17.1, H1: 38 000 lines of alsa-lib and 93 of the program's
//! a minute, 290 MB an hour of log). And it is tried again once a round,
//! each round longer (H2: some 13 streams every 5 s). Linux with ALSA; its
//! own process, its own ~/.asoundrc.

#![cfg(target_os = "linux")]

#[path = "support/scratch.rs"]
mod scratch;

use std::io::Write;
use std::time::{Duration, Instant};

const SECS: u64 = 25;

#[test]
fn an_output_failing_at_every_write_says_so_seldom() {
    if std::env::var_os("AMP_FLOOD_CHILD").is_some() {
        return child();
    }
    let home = scratch::Scratch::new("output-flood");
    std::fs::write(
        home.join(".asoundrc"),
        "pcm.!default {\n type file\n slave.pcm \"null\"\n file \"/dev/full\"\n format \"raw\"\n}\n",
    )
    .unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "an_output_failing_at_every_write_says_so_seldom",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("AMP_FLOOD_CHILD", "1")
        .env("HOME", home.path())
        .env("PULSE_SERVER", "unix:/nonexistent")
        .env_remove("PIPEWIRE_REMOTE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let alsa = stderr.lines().filter(|l| l.starts_with("ALSA lib")).count();
    let ours = stderr
        .lines()
        .filter(|l| l.starts_with("axross-amp: output"))
        .count();
    println!("{SECS} s: {alsa} lines of alsa-lib, {ours} of the output's\n{stdout}");
    assert!(out.status.success(), "{stdout}\n{stderr}");
    // (said once, then every 10 s: a few in 25 s)
    assert!(
        alsa + ours <= 6,
        "{alsa} + {ours} lines:\n{}",
        &stderr[..stderr.len().min(3000)]
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

/// The engine on the failing output, playing, for [`SECS`].
fn child() {
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
    while start.elapsed() < Duration::from_secs(SECS) {
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
    println!("opens {opens}");
}
