//! A file whose reading stalls in the middle (a disk spinning up, a network
//! share that hesitates) while it plays: the reading happens on a thread of
//! its own and seconds ahead, so the sound goes on, and the player takes
//! its commands all the while. (Read on the thread that fed the output,
//! 0.35 s ahead, a stall of a second was a dropout and froze pause and next.)

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::engine::{Command, Engine, Event, READ_STALL_AT, READ_STALL_MS, RenderState};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

fn tone(path: &std::path::Path, secs: f32) {
    let rate = 48_000u32;
    let frames = (rate as f32 * secs) as u32;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 4).to_le_bytes(),
        &4u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        out.extend_from_slice(v);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(frames * 4).to_le_bytes());
    for i in 0..frames {
        let v = (((i as f32 * 0.05).sin() * 0.3 + 0.4) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_read_that_stalls_is_no_dropout_and_holds_up_no_command() {
    let dir = scratch::Scratch::new("stall");
    tone(&dir.join("long.wav"), 5.0);
    tone(&dir.join("other.wav"), 1.0);
    // a stall of 1.2 s some 2.5 s into the file (read by then: about 2 s
    // into the playing - a reader 0.35 s ahead runs dry)
    READ_STALL_AT.store(48_000 * 4 + 300_000, Ordering::Relaxed);
    READ_STALL_MS.store(1200, Ordering::Relaxed);
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: dir.join("long.wav").to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    let mut state = RenderState::default();
    let mut buf = vec![0.0f32; 480 * 2];
    let mut heard = Vec::new();
    let started = Instant::now();
    let mut paused_at: Option<usize> = None;
    let mut slowest = Duration::ZERO;
    while started.elapsed() < Duration::from_secs(3) || paused_at.is_none() {
        engine.shared.render(&mut buf, 2, &mut state);
        heard.extend(buf.chunks(2).map(|f| f[0]));
        // during the stall: a pause and a resume, taken at once
        let t = started.elapsed();
        if t > Duration::from_millis(2900) && paused_at.is_none() {
            let asked = Instant::now();
            engine.send(Command::Pause);
            engine.send(Command::Play);
            engine.send(Command::Seek(0.5));
            // (pause / play / seek go through the decoder thread: it answers
            // with the position moved)
            while (engine.shared.position() - 0.5).abs() > 0.05
                && asked.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            slowest = asked.elapsed();
            paused_at = Some(heard.len());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        slowest < Duration::from_millis(100),
        "a seek waited {slowest:?}"
    );
    // no silence once the sound began, but around the seek's fade
    let first = heard.iter().position(|s| *s != 0.0).unwrap();
    let cut = paused_at.unwrap();
    let silent_before = heard[first..cut.min(heard.len())]
        .iter()
        .filter(|s| **s == 0.0)
        .count();
    let zeros: Vec<usize> = (first..cut.min(heard.len()))
        .filter(|i| heard[*i] == 0.0)
        .collect();
    assert_eq!(
        silent_before,
        0,
        "a dropout while the read stalled: {:?} of {}",
        &zeros[..zeros.len().min(5)],
        heard.len()
    );
    // and another track loads at once while the file is stalled again
    READ_STALL_AT.store(0, Ordering::Relaxed);
    READ_STALL_MS.store(1500, Ordering::Relaxed);
    let asked = Instant::now();
    engine.send(Command::Load {
        path: dir.join("long.wav").to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 2,
    });
    engine.send(Command::Pause);
    while engine.shared.playing.load(Ordering::Relaxed) && asked.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        asked.elapsed() < Duration::from_millis(100),
        "a pause waited for a stalled open: {:?}",
        asked.elapsed()
    );
    let _ = engine.events.try_iter().count();
    let _: Option<Event> = None;
}
