//! Gapless with a slow disk: every file takes 0.6 s to open (a disk that
//! sleeps, a network share). The next track is opened on a thread of its
//! own as soon as it is named, and seconds are decoded ahead: the join
//! has no gap. (Opened when the first ran out, it was a gap of the time
//! to open it every time.)

use axross_amp::engine::{Command, Engine, Event, OPEN_DELAY_MS, Queued, RenderState};
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
        // (an offset: no sample is 0, a gap is)
        let v = (((i as f32 * 0.05).sin() * 0.3 + 0.4) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_slow_disk_leaves_no_gap_between_tracks() {
    let dir = std::env::temp_dir().join(format!("amp-slow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    tone(&dir.join("a.wav"), 1.5);
    tone(&dir.join("b.wav"), 1.0);
    OPEN_DELAY_MS.store(600, Ordering::Relaxed);
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: dir.join("a.wav").to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    engine.send(Command::Queue(Some(Queued {
        path: dir.join("b.wav").to_string_lossy().into_owned(),
        generation: 2,
    })));
    let mut state = RenderState::default();
    let mut buf = vec![0.0f32; 480 * 2];
    let mut heard = Vec::new();
    let mut events = Vec::new();
    let end = Instant::now() + Duration::from_secs(4);
    while Instant::now() < end && !events.iter().any(|e| matches!(e, Event::Ended { .. })) {
        engine.shared.render(&mut buf, 2, &mut state);
        heard.extend(buf.chunks(2).map(|f| f[0]));
        events.extend(engine.events.try_iter());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Advanced { generation: 2, .. }))
    );
    let first = heard.iter().position(|s| *s != 0.0).unwrap();
    let last = heard.iter().rposition(|s| *s != 0.0).unwrap();
    let silent = heard[first..=last].iter().filter(|s| **s == 0.0).count();
    assert_eq!(silent, 0, "{silent} frames of silence between the tracks");
}
