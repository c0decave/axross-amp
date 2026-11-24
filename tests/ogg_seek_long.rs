//! A seek in a long Ogg file that is sought by decoding (review 18, M4): a
//! link with pages on which no packet ends (a packet longer than a page:
//! review 17's r17-obig-plain.opus) is read from the last page some seconds
//! before the place - one that begins a packet, its time known from the
//! page scan - not from 0:00. Decoded from its start, a seek to 9:30 of ten
//! minutes was a second and a half of silence in a release build (to 57:00
//! of an hour: 8.65 s), and a drag of the slider was each of its seeks in
//! turn.
//!
//! The file: r17-obig-plain.opus (4 s, a chirp f(t) = 200 + 40 t Hz), its
//! pages of sound written again and again with their granule positions
//! moved on (each by the packets that end on it) - ten minutes of it.

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::engine::{Command, Engine, Event, RenderState};
use std::path::Path;
use std::time::{Duration, Instant};

/// Ogg's page checksum.
fn crc(page: &[u8]) -> u32 {
    let mut crc = 0u32;
    for b in page {
        crc ^= (*b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// `source`'s sound `times` over into `to`; the seconds of one round.
fn lengthen(source: &Path, to: &Path, times: usize) -> f64 {
    let src = std::fs::read(source).unwrap();
    let mut pages = Vec::new();
    let mut at = 0;
    while at < src.len() {
        assert_eq!(&src[at..at + 4], b"OggS");
        let segments = src[at + 26] as usize;
        let body: usize = src[at + 27..at + 27 + segments]
            .iter()
            .map(|l| *l as usize)
            .sum();
        let len = 27 + segments + body;
        pages.push(src[at..at + len].to_vec());
        at += len;
    }
    let granule = |p: &[u8]| i64::from_le_bytes(p[6..14].try_into().unwrap());
    // (the header pages: granule 0)
    let heads = pages.iter().take_while(|p| granule(p) == 0).count();
    let (heads, sound) = pages.split_at(heads);
    // each page's granule position by the packets that end on it (120 ms
    // each, as the encoder was told): the last page's own is the stream's
    // end, less than its packets' samples - written again and again as it
    // is, each round's positions were short of its sound
    const PACKET: i64 = 5760;
    let mut ended = 0i64;
    let positions: Vec<i64> = sound
        .iter()
        .map(|p| {
            let segments = p[26] as usize;
            let ends = p[27..27 + segments].iter().filter(|l| **l < 255).count() as i64;
            ended += ends;
            if ends > 0 { ended * PACKET } else { -1 }
        })
        .collect();
    for (p, at) in sound.iter().zip(&positions).take(sound.len() - 1) {
        assert_eq!(granule(p), *at, "the packets are not 120 ms each");
    }
    let round = ended * PACKET;
    let mut out = Vec::with_capacity(src.len() * times);
    let mut sequence = 0u32;
    let mut emit = |page: &[u8], granule: i64, flags: u8| {
        let mut p = page.to_vec();
        p[5] = flags;
        p[6..14].copy_from_slice(&granule.to_le_bytes());
        p[18..22].copy_from_slice(&sequence.to_le_bytes());
        p[22..26].fill(0);
        let sum = crc(&p);
        p[22..26].copy_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(&p);
        sequence += 1;
    };
    for p in heads {
        emit(p, 0, p[5]);
    }
    for r in 0..times {
        for (k, p) in sound.iter().enumerate() {
            let last = r == times - 1 && k == sound.len() - 1;
            let flags = (p[5] & !4) | if last { 4 } else { 0 };
            let g = positions[k];
            emit(p, if g < 0 { g } else { g + r as i64 * round }, flags);
        }
    }
    std::fs::write(to, out).unwrap();
    round as f64 / 48_000.0
}

fn wait_ready(engine: &Engine, most: Duration) -> bool {
    let started = Instant::now();
    while !engine.shared.ready(480) {
        if started.elapsed() > most {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    true
}

/// The pitch heard next (Hz): by the rises through zero of a fifth of a
/// second of the left channel.
fn heard(engine: &Engine, state: &mut RenderState) -> f64 {
    let frames = 9600;
    let mut mono = Vec::with_capacity(frames);
    let mut buf = vec![0.0f32; 480 * 2];
    while mono.len() < frames {
        assert!(wait_ready(engine, Duration::from_secs(20)), "no sound");
        engine.shared.render(&mut buf, 2, state);
        mono.extend(buf.chunks(2).map(|f| f[0]));
    }
    let rises: Vec<usize> = (1..mono.len())
        .filter(|&i| mono[i - 1] <= 0.0 && mono[i] > 0.0)
        .collect();
    assert!(rises.len() > 8, "silence where sound was sought");
    (rises.len() - 1) as f64 * 48_000.0 / (rises[rises.len() - 1] - rises[0]) as f64
}

#[test]
fn a_seek_in_a_long_ogg_is_decoded_from_a_page_near_it() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/media/r17/r17-obig-plain.opus");
    let dir = scratch::Scratch::new("ogglong");
    let path = dir.join("ten-minutes.opus");
    let round = lengthen(&source, &path, 150);
    // what the page scan keeps: places to decode from, a few seconds apart
    let links = axross_amp::seektable::ogg_links(&path).expect("its links");
    assert_eq!(links.len(), 1);
    let link = &links[0];
    assert!(link.gaps, "the file is not one sought by decoding");
    assert!(
        (link.secs - 150.0 * round).abs() < 0.2,
        "its length: {}",
        link.secs
    );
    assert!(link.sound > link.offset);
    let points = &link.points;
    assert!(points.len() > 60, "{} places to decode from", points.len());
    let widest = points
        .windows(2)
        .map(|w| w[1].1 - w[0].1)
        .fold(points[0].1, f64::max)
        .max(link.secs - points[points.len() - 1].1);
    assert!(
        widest <= 3.0 * axross_amp::seektable::POINT_EVERY,
        "{widest} s between two of them"
    );
    assert!(
        points
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1)
    );

    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: path.to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    assert!(wait_ready(&engine, Duration::from_secs(30)), "never loaded");
    let mut state = RenderState::default();
    let mut duration = None;
    let mut buf = vec![0.0f32; 480 * 2];
    engine.shared.render(&mut buf, 2, &mut state);
    std::thread::sleep(Duration::from_millis(300));
    while let Ok(e) = engine.events.try_recv() {
        if let Event::Loaded { info, .. } = e {
            duration = info.duration;
        }
    }
    let total = duration.expect("its length");
    assert!((total - link.secs).abs() < 0.1, "{total} s");

    // seeks all over it: each heard at once, and at the place - the
    // chirp's pitch there. (It begins anew every round; each to the middle
    // of a round: where one begins, the decoder is a few packets finding
    // its feet - the file's making, not the seek's.)
    let mut slowest = 0.0f64;
    for rounds in [3, 20, 75, 46, 143, 149] {
        let target = (rounds as f64 + 0.5) * round;
        let started = Instant::now();
        engine.send(Command::Seek(target));
        // (the seek taken: the sound of before it is thrown away)
        std::thread::sleep(Duration::from_millis(40));
        loop {
            assert!(
                wait_ready(&engine, Duration::from_secs(60)),
                "no sound after a seek to {target:.1} s"
            );
            if (engine.shared.position() - target).abs() < 1.0 {
                break;
            }
            engine.shared.render(&mut buf, 2, &mut state);
            assert!(started.elapsed() < Duration::from_secs(60));
        }
        let took = started.elapsed().as_secs_f64();
        slowest = slowest.max(took);
        if total - target > 0.5 {
            let within = (engine.shared.position() + 0.1).rem_euclid(round);
            let pitch = heard(&engine, &mut state);
            let at = (pitch - 200.0) / 40.0;
            println!(
                "a seek to {target:7.1} s: heard after {took:.2} s, {pitch:.0} Hz - {at:.2} s into its round, {within:.2} expected"
            );
            let off = (at - within).abs();
            assert!(
                off.min(round - off) < 0.35,
                "a seek to {target:.1} s landed {at:.2} s into a round, not {within:.2}"
            );
        }
    }
    // (decoded from 0:00, the seek to 9:30 alone was 1.5 s in a release
    // build, 5 s and more in this one)
    assert!(
        slowest < 1.0,
        "a seek took {slowest:.2} s until its sound was there"
    );

    // a drag of the slider: ten seeks 50 ms apart, the last one heard soon
    let started = Instant::now();
    for k in 0..10 {
        engine.send(Command::Seek(total * (0.5 + 0.04 * k as f64)));
        std::thread::sleep(Duration::from_millis(50));
    }
    let target = total * 0.86;
    loop {
        assert!(wait_ready(&engine, Duration::from_secs(60)));
        if (engine.shared.position() - target).abs() < 1.0 {
            break;
        }
        engine.shared.render(&mut buf, 2, &mut state);
        assert!(started.elapsed() < Duration::from_secs(60));
    }
    let took = started.elapsed().as_secs_f64();
    println!("a drag of ten seeks: its end heard after {took:.2} s");
    assert!(took < 1.5, "a drag's end was heard after {took:.2} s");
}
