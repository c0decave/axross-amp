//! An output that keeps failing (review of 0.17.1, H1 and H2): let go, and
//! tried again once a round, each round waiting twice as long - 5 s, 10,
//! 20, then every 30 (here a 25th of that) -, until a stream has played a
//! while without an error; then a loss is made good at once again. And
//! what it says is said once, then at most every 10 s - also when the
//! stream is made anew. (Its own process: the retry's time is the
//! program's.)

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::engine::{Command, Engine, Event, OUTPUT_RETRY_MS};
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// 200 ms in place of 5 s.
const BASE: u64 = 200;

fn long_wav(path: &std::path::Path, secs: u32) {
    let rate = 22_050u32;
    let frames = rate * secs;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + frames * 2).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 2).to_le_bytes(),
        &2u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        f.write_all(v).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&(frames * 2).to_le_bytes()).unwrap();
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| (((i as f32 * 0.06).sin() * 8000.0) as i16).to_le_bytes())
        .collect();
    f.write_all(&data).unwrap();
}

/// The output's events from now until `done` says so (or `secs`), with
/// when each came.
fn events_until(
    engine: &Engine,
    secs: f32,
    mut done: impl FnMut(&str) -> bool,
) -> Vec<(Duration, String)> {
    let start = Instant::now();
    let mut out = Vec::new();
    while start.elapsed() < Duration::from_secs_f32(secs) {
        if let Ok(Event::Output(o)) = engine.events.recv_timeout(Duration::from_millis(5)) {
            out.push((start.elapsed(), o.clone()));
            if done(&o) {
                break;
            }
        }
    }
    out
}

#[test]
fn a_failing_output_is_tried_once_a_round_each_round_longer() {
    OUTPUT_RETRY_MS.store(BASE, Ordering::Relaxed);
    let dir = scratch::Scratch::new("output-retry");
    let wav = dir.join("long.wav");
    long_wav(&wav, 120);
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: wav.to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    std::thread::sleep(Duration::from_millis(300));
    while engine.events.try_recv().is_ok() {}
    // four losses within ten seconds: three made good at once, then let go
    let mut opened = 0;
    for _ in 0..3 {
        engine.shared.simulate_output_loss();
        let seen = events_until(&engine, 2.0, |_| true);
        assert!(
            seen.iter().any(|(_, o)| !o.starts_with("no sound")),
            "made good at once: {seen:?}"
        );
        opened += 1;
    }
    assert_eq!(opened, 3);
    engine.shared.simulate_output_loss();
    let seen = events_until(&engine, 2.0, |_| true);
    assert!(seen[0].1.contains("keeps failing"), "{seen:?}");
    // then a round at a time: one try, waiting twice as long each time (to
    // six times the first), failing again at once - let go at once
    let mut waits = Vec::new();
    for round in 0..6 {
        let seen = events_until(&engine, 10.0, |o| !o.starts_with("no sound"));
        let (after, last) = seen.last().expect("tried again").clone();
        assert!(!last.starts_with("no sound"), "round {round}: {seen:?}");
        assert_eq!(
            seen.len(),
            1,
            "round {round}: one try, nothing else: {seen:?}"
        );
        waits.push(after.as_millis() as u64);
        engine.shared.simulate_output_loss();
        let gone = events_until(&engine, 2.0, |_| true);
        assert_eq!(gone.len(), 1, "round {round}: {gone:?}");
        assert!(
            gone[0].1.contains("keeps failing"),
            "round {round}: let go at once: {gone:?}"
        );
        assert!(gone[0].0 < Duration::from_millis(150), "{gone:?}");
    }
    println!("waits (ms): {waits:?}");
    let want = [1, 2, 4, 6, 6, 6].map(|k| k * BASE);
    for (got, want) in waits.iter().zip(want) {
        assert!(
            *got + 30 >= want && *got <= want + 150,
            "waits {waits:?}, wanted about {:?}",
            [1, 2, 4, 6, 6, 6].map(|k| k * BASE)
        );
    }
    // a try that holds: well again once it has played a while (six times
    // the first wait) - a loss then is made good at once, as at first
    let seen = events_until(&engine, 10.0, |o| !o.starts_with("no sound"));
    assert!(!seen.last().unwrap().1.starts_with("no sound"));
    std::thread::sleep(Duration::from_millis(6 * BASE + 300));
    while engine.events.try_recv().is_ok() {}
    engine.shared.simulate_output_loss();
    let seen = events_until(&engine, 2.0, |_| true);
    assert!(
        !seen[0].1.starts_with("no sound") && seen[0].0 < Duration::from_millis(150),
        "well again: made good at once: {seen:?}"
    );
}
