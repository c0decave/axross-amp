//! The engine without a sound card: the test is the output (it calls
//! `Shared::render` at the pace a device would), so what would be heard is
//! measured sample by sample - gapless joins, and streams from local HTTP
//! servers that stall, break off, carry ICY titles or answer as old
//! SHOUTcast servers do.

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::engine::{Command, Engine, Event, Queued, RenderState};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;

/// A stereo 16-bit WAV of a tone: `secs` at `rate` (a header claiming
/// `claimed_frames` when given: an endless stream's).
fn wav_bytes(rate: u32, secs: f32, hz: f32, claimed_frames: Option<u32>) -> Vec<u8> {
    let frames = (rate as f32 * secs) as u32;
    let data = claimed_frames.unwrap_or(frames) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
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
    out.extend_from_slice(&data.to_le_bytes());
    out.extend(pcm(rate, frames, hz, 0));
    out
}

/// `frames` of a tone (16-bit stereo), from frame `from` on (a continuing phase).
fn pcm(rate: u32, frames: u32, hz: f32, from: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames as usize * 4);
    for i in 0..frames as u64 {
        let t = (from + i) as f64 / rate as f64;
        // (a tone with an offset: no sample of it is 0, a gap is)
        let v = (((t * hz as f64 * std::f64::consts::TAU).sin() * 0.3 + 0.4) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn folder(name: &str) -> scratch::Scratch {
    scratch::Scratch::new(&format!("engine-{name}"))
}

/// The output: `secs` of sound rendered at a device's pace (10 ms a call),
/// each 10 ms taken once the decoder has it ready (or has nothing more).
/// (By the sound, not the clock: it played as much as the clock allowed, so
/// on a busy machine too little - "advanced 2" and no end, a slice of 0.63
/// s for 0.9 - and a decoder not given its turn ran the output dry: gaps.
/// A device's buffer covers such a moment.)
struct Output {
    state: RenderState,
    heard: Vec<f32>,
}

impl Output {
    fn new() -> Self {
        Output {
            state: RenderState::default(),
            heard: Vec::new(),
        }
    }

    fn play(&mut self, engine: &Engine, secs: f32, mut each: impl FnMut(&Engine, &Event)) {
        let mut buf = vec![0.0f32; 480 * 2];
        for _ in 0..(secs * 100.0).round() as usize {
            // (playing: the next 10 ms decoded first - at most 5 s)
            let wait = Instant::now() + Duration::from_secs(5);
            while engine
                .shared
                .playing
                .load(std::sync::atomic::Ordering::Relaxed)
                && !engine.shared.ready(480)
                && Instant::now() < wait
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            engine.shared.render(&mut buf, 2, &mut self.state);
            self.heard.extend_from_slice(&buf);
            while let Ok(event) = engine.events.try_recv() {
                each(engine, &event);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Runs of silence (8 frames or more) between the first and the last sound.
    fn gaps(&self) -> Vec<(usize, usize)> {
        let frames: Vec<f32> = self.heard.chunks(2).map(|f| f[0]).collect();
        let first = frames.iter().position(|s| *s != 0.0).unwrap_or(0);
        let last = frames.iter().rposition(|s| *s != 0.0).unwrap_or(0);
        let mut gaps = Vec::new();
        let mut run = 0;
        for (i, s) in frames[first..=last].iter().enumerate() {
            if *s == 0.0 {
                run += 1;
            } else {
                if run >= 8 {
                    gaps.push((first + i - run, run));
                }
                run = 0;
            }
        }
        gaps
    }
}

fn wait_for(engine: &Engine, secs: f32, mut want: impl FnMut(&Event) -> bool) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs_f32(secs);
    let mut seen = Vec::new();
    while Instant::now() < end {
        if let Ok(event) = engine.events.recv_timeout(Duration::from_millis(50)) {
            seen.push(describe(&event));
            if want(&event) {
                return seen;
            }
        }
    }
    panic!("not within {secs} s; events: {seen:?}");
}

fn describe(event: &Event) -> String {
    match event {
        Event::Loaded { generation, info } => {
            format!("loaded {generation} station {:?}", info.station)
        }
        Event::Advanced { generation, .. } => format!("advanced {generation}"),
        Event::Ended { generation } => format!("ended {generation}"),
        Event::Length { generation, secs } => format!("length {generation}: {secs:.1}"),
        Event::Failed {
            generation,
            message,
        } => format!("failed {generation}: {message}"),
        Event::StreamTitle { generation, title } => format!("title {generation}: {title}"),
        Event::Notice {
            generation,
            message,
        } => format!("notice {generation}: {message}"),
        Event::Output(o) => format!("output {o}"),
    }
}

fn load(engine: &Engine, path: &str, generation: u64) {
    engine.send(Command::Load {
        path: path.to_owned(),
        start_ms: 0,
        play: true,
        generation,
    });
}

#[test]
fn queued_tracks_follow_without_a_sample_of_silence() {
    let dir = folder("gapless");
    // the same rate as the output, and one resampled (44.1 kHz)
    for (name, rate) in [("a.wav", RATE), ("b.wav", RATE), ("c.wav", 44_100)] {
        std::fs::write(dir.join(name), wav_bytes(rate, 0.6, 330.0, None)).unwrap();
    }
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let engine = Engine::start_headless(|| {});
    load(&engine, &path("a.wav"), 1);
    engine.send(Command::Queue(Some(Queued {
        path: path("b.wav"),
        generation: 2,
    })));
    let mut output = Output::new();
    let mut events = Vec::new();
    let mut position_after_advance = None;
    output.play(&engine, 2.4, |engine, event| {
        events.push(describe(event));
        if let Event::Advanced { generation: 2, .. } = event {
            position_after_advance = Some(engine.shared.position());
            engine.send(Command::Queue(Some(Queued {
                path: path("c.wav"),
                generation: 3,
            })));
        }
    });
    assert!(
        events.contains(&"advanced 2".to_owned()) && events.contains(&"advanced 3".to_owned()),
        "{events:?}"
    );
    assert!(events.contains(&"ended 3".to_owned()), "{events:?}");
    assert!(
        !events
            .iter()
            .any(|e| e.starts_with("ended 1") || e.starts_with("ended 2"))
    );
    let gaps = output.gaps();
    assert!(gaps.is_empty(), "silence between the tracks: {gaps:?}");
    // all three heard whole: 1.8 s of sound
    let sound = output.heard.chunks(2).filter(|f| f[0] != 0.0).count();
    assert!(
        (sound as i64 - (1.8 * RATE as f64) as i64).abs() < 200,
        "{sound} frames"
    );
    // the position starts again with the next track
    assert!(
        position_after_advance.is_some_and(|p| p < 0.1),
        "{position_after_advance:?}"
    );
}

#[test]
fn a_seek_in_the_ending_track_takes_the_next_one_back() {
    let dir = folder("gapless-seek");
    std::fs::write(dir.join("a.wav"), wav_bytes(RATE, 0.5, 330.0, None)).unwrap();
    std::fs::write(dir.join("b.wav"), wav_bytes(RATE, 0.5, 330.0, None)).unwrap();
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let engine = Engine::start_headless(|| {});
    load(&engine, &path("a.wav"), 1);
    engine.send(Command::Queue(Some(Queued {
        path: path("b.wav"),
        generation: 2,
    })));
    let mut output = Output::new();
    let mut state = RenderState::default();
    let mut buf = vec![0.0f32; 480 * 2];
    // 0.3 s heard: a's end (0.5 s) is within the 0.35 s decoded ahead
    for _ in 0..30 {
        engine.shared.render(&mut buf, 2, &mut state);
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    engine.send(Command::Seek(0.1));
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        (engine.shared.position() - 0.1).abs() < 0.02,
        "{}",
        engine.shared.position()
    );
    let mut events = Vec::new();
    output.state = state;
    output.play(&engine, 1.2, |_, e| events.push(describe(e)));
    // a plays from 0.1 s to its end, then b: once
    assert_eq!(
        events.iter().filter(|e| *e == "advanced 2").count(),
        1,
        "{events:?}"
    );
    assert!(output.gaps().is_empty(), "{:?}", output.gaps());
}

// -- streams ---------------------------------------------------------------------------------

/// A local HTTP server: `answer` gets each connection (its number and the
/// socket, the request read).
fn server(answer: impl Fn(usize, TcpStream) + Send + Sync + 'static) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let answer = Arc::new(answer);
    std::thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            let n = c.fetch_add(1, Ordering::SeqCst);
            let answer = answer.clone();
            std::thread::spawn(move || {
                let mut socket = socket;
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    if socket.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    request.push(byte[0]);
                }
                answer(n, socket);
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), count)
}

/// An endless WAV stream: a header, then the tone in real time (`every`
/// bytes an ICY block when `metaint` is given) for `secs`, then the end.
fn radio(socket: &mut TcpStream, head: &str, metaint: Option<usize>, secs: f32, title: &str) {
    let _ = socket.write_all(head.as_bytes());
    let mut body = wav_bytes(RATE, 0.0, 330.0, Some(u32::MAX / 8));
    let mut frame = 0u64;
    let mut since_meta = 0usize;
    let end = Instant::now() + Duration::from_secs_f32(secs);
    let mut sent_title = false;
    while Instant::now() < end {
        body.extend(pcm(RATE, 4800, 330.0, frame));
        frame += 4800;
        let mut out = Vec::new();
        for b in body.drain(..) {
            if let Some(m) = metaint
                && since_meta == m
            {
                since_meta = 0;
                if sent_title {
                    out.push(0);
                } else {
                    let mut block = format!("StreamTitle='{title}';").into_bytes();
                    block.resize(block.len().div_ceil(16) * 16, 0);
                    out.push((block.len() / 16) as u8);
                    out.extend(block);
                    sent_title = true;
                }
            }
            out.push(b);
            since_meta += 1;
        }
        if socket.write_all(&out).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_stalled_server_holds_up_nothing() {
    // the server answers, sends a few bytes of a header and then nothing; it
    // notes when the player lets go of the connection
    let closed = Arc::new(Mutex::new(None::<Instant>));
    let c = closed.clone();
    let (base, _) = server(move |_, mut socket| {
        let _ = socket.write_all(
            b"HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\n\r\nRIFF\xff\xff\xff\xffWAVE",
        );
        let mut byte = [0u8; 1];
        let _ = socket.set_read_timeout(Some(Duration::from_secs(60)));
        while matches!(socket.read(&mut byte), Ok(n) if n > 0) {}
        *c.lock().unwrap() = Some(Instant::now());
    });
    let dir = folder("stall");
    std::fs::write(dir.join("a.wav"), wav_bytes(RATE, 0.5, 330.0, None)).unwrap();
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/live"), 1);
    std::thread::sleep(Duration::from_millis(300));
    // a pause, then another track: at once
    engine.send(Command::Pause);
    let asked = Instant::now();
    load(&engine, &dir.join("a.wav").to_string_lossy(), 2);
    wait_for(&engine, 2.0, |e| {
        matches!(e, Event::Loaded { generation: 2, .. })
    });
    assert!(
        asked.elapsed() < Duration::from_secs(1),
        "{:?}",
        asked.elapsed()
    );
    // and the stalled connection is closed (its thread did not wait on for ever)
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end && closed.lock().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    let at = closed
        .lock()
        .unwrap()
        .expect("the stalled connection stayed open");
    assert!(at.duration_since(asked) < Duration::from_secs(2));
}

#[test]
fn a_radio_says_its_name_and_what_it_plays() {
    let (base, _) = server(|_, mut socket| {
        radio(
            &mut socket,
            "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nicy-metaint: 8192\r\nicy-name: Test FM\r\nicy-genre: Tones\r\n\r\n",
            Some(8192),
            5.0,
            "Artist - Song \u{e9}",
        );
    });
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/live"), 1);
    let mut output = Output::new();
    let mut events = Vec::new();
    let mut station = String::new();
    output.play(&engine, 1.5, |_, e| {
        if let Event::Loaded { info, .. } = e {
            station = format!("{} / {}", info.station, info.station_genre);
        }
        events.push(describe(e));
    });
    assert_eq!(station, "Test FM / Tones", "{events:?}");
    assert!(
        events.contains(&"title 1: Artist - Song \u{e9}".to_owned()),
        "{events:?}"
    );
    // the metadata is out of the sound: the tone plays on, without a click
    let frames: Vec<f32> = output.heard.chunks(2).map(|f| f[0]).collect();
    let start = frames.iter().position(|s| *s != 0.0).unwrap();
    let jumps = frames[start..]
        .windows(2)
        .filter(|w| w[0] != 0.0 && w[1] != 0.0 && (w[1] - w[0]).abs() > 0.05)
        .count();
    assert_eq!(jumps, 0, "the ICY blocks were played as sound");
    // a buffer of sound before it starts: no stutter once it plays
    assert!(output.gaps().is_empty(), "{:?}", output.gaps());
}

#[test]
fn an_old_shoutcast_answer_plays() {
    let (base, _) = server(|_, mut socket| {
        radio(
            &mut socket,
            "ICY 200 OK\r\nicy-name: Old Cast\r\ncontent-type: audio/wav\r\n\r\n",
            None,
            3.0,
            "",
        );
    });
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/"), 1);
    let seen = wait_for(&engine, 5.0, |e| {
        matches!(e, Event::Loaded { .. } | Event::Failed { .. })
    });
    assert!(
        seen.last()
            .unwrap()
            .contains("loaded 1 station \"Old Cast\""),
        "{seen:?}"
    );
}

#[test]
fn a_radio_that_breaks_off_is_connected_again() {
    // the first connection ends after a second, the second goes on
    let (base, count) = server(|n, mut socket| {
        let head = "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\n\r\n";
        radio(&mut socket, head, None, if n == 0 { 1.0 } else { 6.0 }, "");
    });
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/live"), 1);
    let mut output = Output::new();
    let mut events = Vec::new();
    output.play(&engine, 4.0, |_, e| events.push(describe(e)));
    assert_eq!(count.load(Ordering::SeqCst), 2, "{events:?}");
    assert!(
        events
            .iter()
            .any(|e| e.starts_with("notice 1: The stream broke off")),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.starts_with("ended") || e.starts_with("failed")),
        "{events:?}"
    );
    // it plays after the break
    let tail = &output.heard[output.heard.len() - 48_000..];
    assert!(tail.iter().any(|s| *s != 0.0), "silent after the break");
}

#[test]
fn a_playlist_url_plays_its_stream() {
    let streams = Arc::new(Mutex::new(String::new()));
    let s = streams.clone();
    let (base, _) = server(move |n, mut socket| {
        if n == 0 {
            let target = s.lock().unwrap().clone();
            let body = format!(
                "[playlist]\nNumberOfEntries=2\nFile1=/etc/passwd\nFile2={target}\nTitle2=Radio\n"
            );
            let _ = write!(
                socket,
                "HTTP/1.0 200 OK\r\nContent-Type: audio/x-scpls\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
        } else {
            radio(
                &mut socket,
                "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nicy-name: Listed\r\n\r\n",
                None,
                3.0,
                "",
            );
        }
    });
    *streams.lock().unwrap() = format!("{base}/live");
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/radio.pls"), 1);
    let seen = wait_for(&engine, 5.0, |e| {
        matches!(e, Event::Loaded { .. } | Event::Failed { .. })
    });
    assert!(
        seen.last().unwrap().contains("station \"Listed\""),
        "{seen:?}"
    );
}

#[test]
fn stop_lets_a_stream_go() {
    let closed = Arc::new(Mutex::new(None::<Instant>));
    let c = closed.clone();
    let (base, _) = server(move |_, mut socket| {
        radio(
            &mut socket,
            "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\n\r\n",
            None,
            20.0,
            "",
        );
        *c.lock().unwrap() = Some(Instant::now());
    });
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/live"), 1);
    let mut output = Output::new();
    output.play(&engine, 1.0, |_, _| {});
    let stopped = Instant::now();
    engine.send(Command::Stop);
    // the server sees the connection end (its writes fail) within a few seconds
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && closed.lock().unwrap().is_none() {
        output.play(&engine, 0.1, |_, _| {});
    }
    let at = closed
        .lock()
        .unwrap()
        .expect("the connection stayed open after Stop");
    assert!(at.duration_since(stopped) < Duration::from_secs(4));
    // and nothing more is heard
    let before = output.heard.len();
    output.play(&engine, 0.3, |_, _| {});
    assert!(output.heard[before..].iter().all(|s| *s == 0.0));
}

#[test]
fn a_failed_file_is_said_and_a_folder_is_no_file() {
    let engine = Engine::start_headless(|| {});
    load(&engine, "/nonexistent/x.mp3", 1);
    let seen = wait_for(&engine, 2.0, |e| matches!(e, Event::Failed { .. }));
    assert!(seen.last().unwrap().ends_with("no such file"), "{seen:?}");
}

#[test]
fn a_lost_output_is_opened_again_and_a_failing_one_is_said() {
    let dir = folder("lost-output");
    std::fs::write(dir.join("a.wav"), wav_bytes(RATE, 3.0, 330.0, None)).unwrap();
    let engine = Engine::start_headless(|| {});
    load(&engine, &dir.join("a.wav").to_string_lossy(), 1);
    let mut output = Output::new();
    let mut events = Vec::new();
    output.play(&engine, 0.3, |_, e| events.push(describe(e)));
    // the device goes (a headset switched off): the output is opened again,
    // the track plays on
    engine.shared.simulate_output_loss();
    output.play(&engine, 0.3, |_, e| events.push(describe(e)));
    assert_eq!(
        events.iter().filter(|e| e.starts_with("output")).count(),
        2,
        "{events:?}"
    );
    assert!(engine.shared.playing.load(Ordering::Relaxed));
    let heard = output.heard.len();
    output.play(&engine, 0.2, |_, _| {});
    assert!(
        output.heard[heard..].iter().any(|s| *s != 0.0),
        "silent after"
    );
    // it fails again and again: it gives up and says so
    for _ in 0..3 {
        engine.shared.simulate_output_loss();
        output.play(&engine, 0.1, |_, e| events.push(describe(e)));
    }
    assert!(
        events.last().unwrap().contains("keeps failing"),
        "{events:?}"
    );
    assert!(!engine.shared.playing.load(Ordering::Relaxed));
}

/// A 16-bit stereo WAV of `samples` (interleaved, -1..1).
fn wav_of(rate: u32, samples: &[f32]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
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
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// A "modern master": dense, loud, its peaks at -0.3 dBFS (where the old
/// soft knee above 0.9 bent it).
fn hot_master(secs: f32) -> Vec<f32> {
    let frames = (RATE as f32 * secs) as usize;
    let mut raw: Vec<f32> = (0..frames)
        .flat_map(|i| {
            let t = i as f32 / RATE as f32;
            let kick = (-(t * 2.0).fract() * 12.0).exp() * (t * 60.0 * std::f32::consts::TAU).sin();
            let pad: f32 = [110.0, 165.0, 220.0, 330.0, 2_200.0]
                .iter()
                .map(|hz| (t * hz * std::f32::consts::TAU).sin())
                .sum::<f32>()
                / 5.0;
            [kick * 0.7 + pad * 0.6, kick * 0.6 + pad * 0.7]
        })
        .collect();
    let peak = raw.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let target = 10f32.powf(-0.3 / 20.0);
    for v in &mut raw {
        // (as 16-bit can hold it: what the file gives back exactly)
        *v = ((*v / peak * target) * 32767.0).round() / 32767.0;
    }
    raw
}

/// What was heard of the track, from its first sound (the fade-in left out).
fn heard_from_start(heard: &[f32]) -> &[f32] {
    let first = heard.iter().position(|s| *s != 0.0).unwrap_or(0);
    &heard[first - first % 2..]
}

#[test]
fn at_unity_gain_a_hot_master_is_played_bit_for_bit() {
    // nothing may bend a master that peaks just under full scale: no
    // limiter, no soft knee - what the file holds is what the device gets
    let dir = folder("unity");
    let master = hot_master(1.0);
    std::fs::write(dir.join("hot.wav"), wav_of(RATE, &master)).unwrap();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    load(&engine, &dir.join("hot.wav").to_string_lossy(), 1);
    let mut output = Output::new();
    output.play(&engine, 1.6, |_, _| {});
    let heard = heard_from_start(&output.heard);
    let want: Vec<f32> = master
        .iter()
        .map(|v| ((v * 32767.0).round() as i16) as f32 / 32768.0)
        .collect();
    // (after the 8 ms fade-in at the start; where the file's frames begin
    // among the first heard: its first ones are near silence)
    let from = 2 * 2048;
    let compared = &heard[from..from + 2 * 30_000];
    let shift = (0..16)
        .min_by_key(|k| {
            let w = &want[from + 2 * k..from + 2 * k + 64];
            (compared[..64]
                .iter()
                .zip(w)
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                * 1e6) as u64
        })
        .unwrap();
    let want = &want[from + 2 * shift..from + 2 * shift + 2 * 30_000];
    let differ = compared.iter().zip(want).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} samples changed at unity gain");
}

#[test]
fn a_track_made_louder_is_held_below_the_ceiling() {
    // ReplayGain's gain for an untagged track at +9 dB: 8.7 dB too loud for
    // a master at -0.3 dBFS - the limiter keeps the peaks at -1 dBTP,
    // smoothly (no flat tops of a clipper)
    let dir = folder("louder");
    let master = hot_master(1.0);
    std::fs::write(dir.join("hot.wav"), wav_of(RATE, &master)).unwrap();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    engine.send(Command::Loudness(axross_amp::engine::Loudness {
        mode: axross_amp::engine::GainMode::Track,
        preamp_db: 0.0,
        untagged_db: 9.0,
    }));
    load(&engine, &dir.join("hot.wav").to_string_lossy(), 1);
    let mut output = Output::new();
    output.play(&engine, 1.6, |_, _| {});
    let heard = heard_from_start(&output.heard);
    let highest = heard.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let ceiling = 10f32.powf(-1.0 / 20.0);
    assert!(highest <= ceiling + 1e-3, "a peak at {highest}");
    // between the samples too, as a BS.1770 meter reads it
    let mut meter = axross_amp::analyzer::loudness::Meter::new(RATE);
    meter.push(heard);
    assert!(meter.true_peak <= -0.9, "{} dBTP", meter.true_peak);
    assert!(highest > ceiling * 0.9, "it was made louder: {highest}");
    // no flat tops: the loudest samples are not held at one value
    let at_top = heard.iter().filter(|v| v.abs() > highest - 1e-5).count();
    assert!(at_top < 40, "{at_top} samples at the top: clipped");
    // and the whole track was heard (the held back frames at its end too)
    let sound = heard
        .chunks(2)
        .filter(|f| f[0] != 0.0 || f[1] != 0.0)
        .count();
    assert!((sound as i64 - RATE as i64).abs() < 100, "{sound} frames");
}

#[test]
fn a_hot_master_played_at_another_rate_is_held_below_full_scale() {
    // 44.1 kHz on a 48 kHz output: the resampler turns the peaks between a
    // hot master's samples into samples - above full scale, where they were
    // cut flat. The output's limiter holds them at -0.3 dBTP instead.
    let dir = folder("resampled");
    let rate = 44_100u32;
    let master: Vec<f32> = (0..rate as usize)
        .flat_map(|i| {
            let t = i as f32 / rate as f32;
            // a quarter of the rate, sampled 45 degrees off its peaks: the
            // samples at -0.7 dBFS, the sound between them at +2.3 dBFS
            let v = (std::f32::consts::TAU * 11_025.0 * t + std::f32::consts::FRAC_PI_4).sin()
                * 1.3
                * (0.97 + 0.03 * (std::f32::consts::TAU * 3.0 * t).sin());
            [v, v]
        })
        .collect();
    std::fs::write(dir.join("hot.wav"), wav_of(rate, &master)).unwrap();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    load(&engine, &dir.join("hot.wav").to_string_lossy(), 1);
    let mut output = Output::new();
    output.play(&engine, 1.6, |_, _| {});
    let heard = heard_from_start(&output.heard);
    let highest = heard.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(highest < 0.98, "a sample at {highest}: full scale reached");
    let mut meter = axross_amp::analyzer::loudness::Meter::new(RATE);
    meter.push(heard);
    assert!(meter.true_peak <= -0.2, "{} dBTP", meter.true_peak);
    // no flat tops, and all of it heard
    let at_top = heard.iter().filter(|v| v.abs() > highest - 1e-5).count();
    assert!(at_top < 40, "{at_top} samples at the top: clipped");
    let sound = heard
        .chunks(2)
        .filter(|f| f[0] != 0.0 || f[1] != 0.0)
        .count();
    assert!((sound as i64 - RATE as i64).abs() < 200, "{sound} frames");
}

/// The power of `hz` in `frames` (left channel, interleaved stereo) by
/// Goertzel's filter.
fn power_at(frames: &[f32], hz: f32) -> f32 {
    let k = 2.0 * (std::f32::consts::TAU * hz / RATE as f32).cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    let n = frames.len() / 2;
    for f in frames.chunks(2) {
        let s = f[0] + k * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - k * s1 * s2) / (n * n) as f32
}

fn tone(hz: f32, secs: f32) -> Vec<f32> {
    (0..(RATE as f32 * secs) as usize)
        .flat_map(|i| {
            let v = (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin() * 0.3;
            [v, v]
        })
        .collect()
}

#[test]
fn a_crossfade_fades_one_track_into_the_next() {
    // (two folders: two files of one folder in order are an album's)
    let dir = folder("crossfade");
    std::fs::create_dir_all(dir.join("other")).unwrap();
    std::fs::write(dir.join("a.wav"), wav_of(RATE, &tone(440.0, 3.0))).unwrap();
    std::fs::write(dir.join("other/b.wav"), wav_of(RATE, &tone(660.0, 3.0))).unwrap();
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    engine.send(Command::Crossfade(1.0));
    load(&engine, &path("a.wav"), 1);
    engine.send(Command::Queue(Some(Queued {
        path: path("other/b.wav"),
        generation: 2,
    })));
    let mut output = Output::new();
    let mut events = Vec::new();
    // (where the meters start each track: the first's start, the second's
    // in the fade's middle - not at its start, where the first is louder)
    output.play(&engine, 0.6, |_, e| events.push(describe(e)));
    let (first, count) = engine.shared.track_start();
    output.play(&engine, 5.4, |_, e| events.push(describe(e)));
    let (second, count2) = engine.shared.track_start();
    assert_eq!(count2, count + 1, "{events:?}");
    let begun = (second - first) as f32 / RATE as f32;
    assert!(
        (begun - 2.5).abs() < 0.1,
        "the second counted from {begun} s, not the fade's middle (2.5 s)"
    );
    assert!(events.contains(&"advanced 2".to_owned()), "{events:?}");
    assert!(events.contains(&"ended 2".to_owned()), "{events:?}");
    let heard = heard_from_start(&output.heard);
    // 3 s and 3 s overlapping by 1 s: 5 s of sound, no gap
    let sound = heard.chunks(2).rposition(|f| f[0] != 0.0).unwrap_or(0);
    assert!(
        (sound as f32 / RATE as f32 - 5.0).abs() < 0.1,
        "{} s of sound",
        sound as f32 / RATE as f32
    );
    assert!(output.gaps().is_empty(), "{:?}", output.gaps());
    let at = |secs: f32| {
        &heard[(secs * RATE as f32) as usize * 2..((secs + 0.1) * RATE as f32) as usize * 2]
    };
    let (a, b) = (power_at(at(1.0), 440.0), power_at(at(1.0), 660.0));
    assert!(a > 100.0 * b, "before the fade only the first: {a} {b}");
    let (a, b) = (power_at(at(2.45), 440.0), power_at(at(2.45), 660.0));
    assert!(a > 0.2 * b && b > 0.2 * a, "in the fade both: {a} {b}");
    let (a, b) = (power_at(at(4.0), 440.0), power_at(at(4.0), 660.0));
    assert!(b > 100.0 * a, "after it only the second: {a} {b}");
    // equal power: the level holds through the fade (no dip, no bump)
    let rms = |w: &[f32]| (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
    let steady = rms(at(1.0));
    for k in 0..20 {
        let r = rms(at(2.0 + k as f32 * 0.05));
        assert!(
            (r / steady - 1.0).abs() < 0.2,
            "at {} s: {r} against {steady}",
            2.0 + k as f32 * 0.05
        );
    }
}

#[test]
fn an_album_plays_gapless_even_with_a_crossfade_set() {
    use axross_amp::engine::{TrackInfo, crossfades};
    let track = |album: &str| TrackInfo {
        album: album.into(),
        ..Default::default()
    };
    assert!(
        !crossfades(4.0, &track("Blue"), &track("Blue")),
        "inside an album"
    );
    assert!(crossfades(4.0, &track("Blue"), &track("Red")));
    assert!(
        crossfades(4.0, &track(""), &track("")),
        "untagged tracks fade"
    );
    assert!(!crossfades(0.0, &track("Blue"), &track("Red")), "off");
    // untagged rips of one album: one folder, numbered in order - no fade
    use axross_amp::engine::crossfades_between;
    let numbered = |n: u64| TrackInfo {
        track_number: Some(n),
        ..Default::default()
    };
    let tagged = |album: &str, n: u64| TrackInfo {
        album: album.into(),
        track_number: Some(n),
        ..Default::default()
    };
    let none = TrackInfo::default();
    let gapless = |a: (&str, &TrackInfo), b: (&str, &TrackInfo)| !crossfades_between(4.0, a, b);
    assert!(gapless(
        ("/live/01 intro.flac", &none),
        ("/live/02 song.flac", &none)
    ));
    assert!(gapless(
        ("/live/x.flac", &numbered(3)),
        ("/live/y.flac", &numbered(4))
    ));
    // ... but not two unrelated files of a flat folder (Music/, Downloads/)
    assert!(!gapless(
        ("/Music/apple.wav", &none),
        ("/Music/banana.wav", &none)
    ));
    assert!(!gapless(
        ("/live/x.flac", &numbered(3)),
        ("/live/y.flac", &numbered(7))
    ));
    assert!(!gapless(
        ("/a/01 song.flac", &none),
        ("/b/02 other.flac", &none)
    ));
    assert!(!gapless(
        ("/live/02 b.flac", &none),
        ("/live/01 a.flac", &none)
    ));
    // tagged: the album decides, not the folder or the numbers
    assert!(!gapless(
        ("/Music/x.flac", &tagged("Blue", 3)),
        ("/Music/y.flac", &tagged("Red", 4))
    ));
    assert!(gapless(
        ("/a/x.flac", &tagged("Blue", 3)),
        ("/b/y.flac", &tagged("Blue", 9))
    ));
    // one tagged, one not: fades
    assert!(!gapless(
        ("/live/01 a.flac", &tagged("Blue", 1)),
        ("/live/02 b.flac", &none)
    ));
}

/// The true peak by 8x oversampling (a long windowed sinc between the
/// samples): finer than BS.1770's 4x filter, which reads broadband sound low.
fn true_peak_8x(frames: &[f32]) -> f32 {
    const HALF: i64 = 48;
    let n = frames.len() as i64 / 2;
    let mut peak = 0.0f32;
    for c in 0..2 {
        let x = |i: i64| {
            if (0..n).contains(&i) {
                frames[(2 * i + c) as usize] as f64
            } else {
                0.0
            }
        };
        // (not within the filter's reach of the excerpt's cut ends: a cut
        // rings)
        for i in HALF..n - HALF {
            peak = peak.max(x(i).abs() as f32);
            for k in 1..8 {
                let f = k as f64 / 8.0;
                let mut v = 0.0f64;
                for j in -HALF + 1..=HALF {
                    let d = j as f64 - f;
                    let sinc = (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d);
                    let w = 0.5 + 0.5 * (std::f64::consts::PI * d / HALF as f64).cos();
                    v += x(i + j) * sinc * w;
                }
                peak = peak.max(v.abs() as f32);
            }
        }
    }
    20.0 * peak.max(1e-9).log10()
}

#[test]
fn a_crossfade_of_two_hot_masters_is_held_below_full_scale() {
    // two loud, dense masters at -0.3 dBFS (most of their samples near the
    // top, as a modern master's): an equal power fade sums them to up to
    // 1.41 times that in its middle - held at -0.3 dBTP by the output's
    // limiter; before the fade, bit for bit
    let dir = folder("hot-fade");
    std::fs::create_dir_all(dir.join("b")).unwrap();
    let loud = |hz: f32| -> Vec<f32> {
        let top = 10f32.powf(-0.3 / 20.0);
        (0..3 * RATE as usize)
            .flat_map(|i| {
                let t = i as f32 / RATE as f32;
                let v = (4.0 * (t * hz * std::f32::consts::TAU).sin()).tanh() / 4f32.tanh();
                let v = ((v * top) * 32767.0).round() / 32767.0;
                [v, v]
            })
            .collect()
    };
    let master = loud(110.0);
    std::fs::write(dir.join("a.wav"), wav_of(RATE, &master)).unwrap();
    std::fs::write(dir.join("b/b.wav"), wav_of(RATE, &loud(165.0))).unwrap();
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    engine.send(Command::Crossfade(2.0));
    load(&engine, &path("a.wav"), 1);
    engine.send(Command::Queue(Some(Queued {
        path: path("b/b.wav"),
        generation: 2,
    })));
    let mut output = Output::new();
    output.play(&engine, 5.5, |_, _| {});
    let heard = heard_from_start(&output.heard);
    let highest = heard.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let at_full = heard.iter().filter(|v| v.abs() >= 0.999).count();
    assert!(
        highest < 0.98 && at_full == 0,
        "a sample at {highest} ({at_full} at full scale)"
    );
    let fade = &heard[(0.9 * RATE as f32) as usize * 2..(3.2 * RATE as f32) as usize * 2];
    let tp = true_peak_8x(fade);
    assert!(tp <= -0.25, "{tp} dBTP in the fade (8x)");
    // before it, the first master as it is (only 4 ms later)
    let want: Vec<f32> = master
        .iter()
        .map(|v| ((v * 32767.0).round() as i16) as f32 / 32768.0)
        .collect();
    let from = 2 * 4096;
    let compared = &heard[from..from + 2 * 20_000];
    let shift = (0..400)
        .min_by_key(|k| {
            let w = &want[from + 2 * k..from + 2 * k + 64];
            (compared[..64]
                .iter()
                .zip(w)
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                * 1e6) as u64
        })
        .unwrap();
    let want = &want[from + 2 * shift..from + 2 * shift + 2 * 20_000];
    let differ = compared.iter().zip(want).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} samples changed before the fade");
}

#[test]
fn a_dense_master_resampled_stays_below_its_ceiling_measured_8x() {
    // broadband sound peaking at -0.1 dBFS at 44.1 kHz on a 48 kHz output:
    // what the 4x filter reads low, an 8x look finds; held at -0.3 dBTP
    let dir = folder("dense");
    let rate = 44_100u32;
    let mut seed = 99u32;
    let mut raw: Vec<f32> = (0..rate as usize)
        .flat_map(|i| {
            let t = i as f32 / rate as f32;
            let tones: f32 = (1..40)
                .map(|k| (t * 517.0 * k as f32 * std::f32::consts::TAU + k as f32 * 1.7).sin())
                .sum::<f32>()
                / 12.0;
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
            let v = tones + noise;
            [v, v * 0.95]
        })
        .collect();
    let peak = raw.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    for v in &mut raw {
        *v = ((*v / peak * 10f32.powf(-0.1 / 20.0)) * 32767.0).round() / 32767.0;
    }
    std::fs::write(dir.join("dense.wav"), wav_of(rate, &raw)).unwrap();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    load(&engine, &dir.join("dense.wav").to_string_lossy(), 1);
    let mut output = Output::new();
    output.play(&engine, 1.4, |_, _| {});
    let heard = heard_from_start(&output.heard);
    let part = &heard[(0.2 * RATE as f32) as usize * 2..(0.9 * RATE as f32) as usize * 2];
    let tp = true_peak_8x(part);
    assert!(tp <= -0.25, "{tp} dBTP (8x)");
}

#[test]
fn a_track_chosen_while_another_plays_fades_in_over_it_when_asked() {
    let dir = folder("skip-fade");
    std::fs::create_dir_all(dir.join("b")).unwrap();
    std::fs::write(dir.join("a.wav"), wav_of(RATE, &tone(440.0, 6.0))).unwrap();
    std::fs::write(dir.join("b/b.wav"), wav_of(RATE, &tone(660.0, 4.0))).unwrap();
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    // both: a cut (off) and a fade of a second (on)
    for on in [false, true] {
        let engine = Engine::start_headless(|| {});
        engine.shared.set_volume(1.0);
        engine.send(Command::FadeOnSkip(on));
        load(&engine, &path("a.wav"), 1);
        let mut output = Output::new();
        output.play(&engine, 1.5, |_, _| {});
        load(&engine, &path("b/b.wav"), 2);
        output.play(&engine, 2.0, |_, _| {});
        let heard = &output.heard;
        let window = |k: usize| &heard[k * 2400 * 2..(k + 1) * 2400 * 2];
        // where the second is first heard (50 ms windows)
        let first = (0..heard.len() / 4800)
            .find(|k| power_at(window(*k), 660.0) > 1e-4)
            .expect("the second heard");
        // 0.3 s on: both (a fade) or only the second (a cut)
        let (a, b) = (
            power_at(window(first + 6), 440.0),
            power_at(window(first + 6), 660.0),
        );
        if on {
            assert!(a > 0.1 * b && b > 0.1 * a, "fading: both {a} {b}");
        } else {
            assert!(b > 100.0 * a, "cut: only the second {a} {b}");
        }
        // and a second later only the second
        let (a, b) = (
            power_at(window(first + 24), 440.0),
            power_at(window(first + 24), 660.0),
        );
        assert!(b > 100.0 * a, "after: {a} {b} ({on})");
    }
}

/// A dense master `secs` long: chords of many harmonics driven into a
/// soft clip, at `peak_db` dBFS (sample peak); `seed` makes another one.
fn dense_master(secs: f32, seed: u32, drive: f32, peak_db: f32) -> Vec<f32> {
    let mut s = seed;
    let mut rnd = || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (s >> 8) as f32 / (1u32 << 24) as f32
    };
    let parts: Vec<(f32, f32, f32)> = (0..24)
        .map(|_| {
            (
                55.0 * (1.0 + 60.0 * rnd()),
                rnd() * std::f32::consts::TAU,
                0.3 + rnd(),
            )
        })
        .collect();
    let mut out: Vec<f32> = (0..(RATE as f32 * secs) as usize)
        .flat_map(|i| {
            let t = i as f32 / RATE as f32;
            let v: f32 = parts
                .iter()
                .map(|(hz, ph, a)| a * (t * hz * std::f32::consts::TAU + ph).sin())
                .sum::<f32>()
                / 8.0;
            // (driven hard: clipped flat, as a loud master's peaks are)
            let v = if drive > 4.0 {
                (drive * v).clamp(-1.0, 1.0)
            } else {
                (drive * v).tanh()
            };
            [v, v * 0.97]
        })
        .collect();
    let top = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let k = 10f32.powf(peak_db / 20.0) / top;
    for v in &mut out {
        *v = ((*v * k) * 32767.0).round() / 32767.0;
    }
    out
}

/// A master as loud as they come (as judge13/hot*.wav): bass, tones and
/// a hiss of noise, driven far into clipping - its peaks between the
/// samples ~3 dB over full scale, the highs loud.
fn loud_master(secs: f32, seed: u32) -> Vec<f32> {
    let mut s = seed;
    let mut rnd = || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (s >> 8) as f32 / (1u32 << 24) as f32
    };
    let shift = rnd() * 6.0;
    let mut out: Vec<f32> = (0..(RATE as f32 * secs) as usize)
        .flat_map(|i| {
            let t = i as f32 / RATE as f32;
            let bass = (t * 55.0 * std::f32::consts::TAU + shift).sin() * 0.7;
            let tone = (t * (440.0 + 50.0 * shift) * std::f32::consts::TAU).sin() * 0.3;
            let v = ((bass + tone + (rnd() - 0.5) * 1.2) * 2.5).clamp(-1.0, 1.0);
            [v, v * 0.98]
        })
        .collect();
    let k = 10f32.powf(-0.1 / 20.0);
    for v in &mut out {
        *v = ((*v * k) * 32767.0).round() / 32767.0;
    }
    out
}

fn rms_db(x: &[f32]) -> f32 {
    10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32)
        .max(1e-12)
        .log10()
}

/// Two tracks crossfaded over 2 s (a in one folder, b in another): what
/// was heard, from the first sound.
fn crossfaded(dir: &scratch::Scratch, a: &[f32], b: &[f32]) -> Vec<f32> {
    std::fs::create_dir_all(dir.join("b")).unwrap();
    std::fs::write(dir.join("a.wav"), wav_of(RATE, a)).unwrap();
    std::fs::write(dir.join("b/b.wav"), wav_of(RATE, b)).unwrap();
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    engine.send(Command::Crossfade(2.0));
    load(&engine, &path("a.wav"), 1);
    engine.send(Command::Queue(Some(Queued {
        path: path("b/b.wav"),
        generation: 2,
    })));
    let mut output = Output::new();
    output.play(
        &engine,
        (a.len() + b.len()) as f32 / 2.0 / RATE as f32 - 1.0,
        |_, _| {},
    );
    heard_from_start(&output.heard).to_vec()
}

#[test]
fn a_crossfade_of_masters_over_full_scale_stays_below_the_ceiling() {
    // dense masters going ~3 dB over full scale between their samples (as
    // judge13/hot*.wav): through the fade, held at -0.3 dBTP measured at 8x
    let (a, b) = (loud_master(4.0, 1), loud_master(4.0, 2));
    let own = true_peak_8x(&a[..2 * RATE as usize]);
    assert!(own > 2.5, "the master itself goes over: {own} dBTP");
    let heard = crossfaded(&folder("over-fade"), &a, &b);
    let fade = &heard[(2.1 * RATE as f32) as usize * 2..(3.9 * RATE as f32) as usize * 2];
    let tp = true_peak_8x(fade);
    assert!(tp <= -0.25, "{tp} dBTP in the fade (8x)");
    // the level: the limiter takes what over-full-scale masters must give
    // up, no more (measured, not hidden: the dip is printed)
    let outside = rms_db(&heard[RATE as usize * 2..(1.8 * RATE as f32) as usize * 2]);
    let inside = rms_db(fade);
    println!("over full scale: outside {outside:.1} dB, in the fade {inside:.1} dB");
}

#[test]
fn a_crossfade_keeps_the_level() {
    // masters well below full scale (no limiting): every 100 ms of the
    // fade within 1.5 dB of the level outside it (the law's sum for
    // tracks alike is tested in the engine's own tests: fade_gains)
    let unlike = (
        dense_master(4.0, 3, 2.0, -9.0),
        dense_master(4.0, 4, 2.0, -9.0),
    );
    for (name, (a, b)) in [("unlike", unlike)] {
        let heard = crossfaded(&folder(&format!("level-{name}")), &a, &b);
        let outside = rms_db(&heard[RATE as usize * 2..(1.8 * RATE as f32) as usize * 2]);
        for k in 0..18 {
            let from = ((2.05 + k as f32 * 0.1) * RATE as f32) as usize * 2;
            let r = rms_db(&heard[from..from + (0.1 * RATE as f32) as usize * 2]);
            assert!(
                (r - outside).abs() <= 1.5,
                "{name}: {r:.1} dB at {:.2} s against {outside:.1} outside",
                2.05 + k as f32 * 0.1
            );
        }
    }
}

/// Two WAV files' samples (16-bit stereo at 48 kHz), from AMP_HOT_FILES
/// ("a.wav:b.wav"): masters to crossfade (`--ignored`).
#[test]
#[ignore]
fn given_masters_crossfaded_stay_below_the_ceiling() {
    let Some(files) = std::env::var_os("AMP_HOT_FILES") else {
        eprintln!("AMP_HOT_FILES not set");
        return;
    };
    let files = files.to_string_lossy().into_owned();
    let read = |p: &str| -> Vec<f32> {
        let bytes = std::fs::read(p).unwrap();
        let data = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        bytes[data..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
            .collect()
    };
    let mut names = files.split(':');
    let (a, b) = (read(names.next().unwrap()), read(names.next().unwrap()));
    let (a, b) = (
        &a[..a.len().min(16 * RATE as usize)],
        &b[..b.len().min(16 * RATE as usize)],
    );
    let heard = crossfaded(&folder("given-fade"), a, b);
    let start = (a.len() / 2) as f32 / RATE as f32 - 2.0;
    let fade = &heard
        [((start + 0.1) * RATE as f32) as usize * 2..((start + 1.9) * RATE as f32) as usize * 2];
    let tp = true_peak_8x(fade);
    let outside = rms_db(&heard[RATE as usize * 2..((start - 0.2) * RATE as f32) as usize * 2]);
    println!(
        "given: {tp:.2} dBTP in the fade; level outside {outside:.1}, inside {:.1}",
        rms_db(fade)
    );
    if std::env::var_os("AMP_HOT_DETAIL").is_some() {
        for k in 0..((heard.len() / 2) as f32 / RATE as f32 / 0.2) as usize {
            let from = (k as f32 * 0.2 * RATE as f32) as usize * 2;
            let to = (from + (0.2 * RATE as f32) as usize * 2).min(heard.len());
            println!(
                "{:.1} s: {:.2} dBTP {:.1} dB",
                k as f32 * 0.2,
                true_peak_8x(&heard[from..to]),
                rms_db(&heard[from..to])
            );
        }
    }
    assert!(tp <= -0.25, "{tp} dBTP");
}

// -- review 13: chained Ogg, VBR MP3 without a Xing header, NaN, a new output --

fn media(name: &str) -> String {
    format!("{}/tests/media/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// The frequency of a tone in `frames` (interleaved stereo at RATE), by its
/// zero crossings on the left.
fn frequency(frames: &[f32]) -> f32 {
    let left: Vec<f32> = frames.chunks(2).map(|f| f[0]).collect();
    let crossings = left
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    crossings as f32 / 2.0 / (left.len() as f32 / RATE as f32)
}

/// A chained Ogg file - a new physical stream (another serial) for each
/// link, as an Icecast station sends at each song - plays to the end of its
/// last link and says its whole length; the second link of chained.ogg is
/// at another rate than the first (review 13, H1: the engine dropped every
/// packet after the first link).
#[test]
fn a_chained_ogg_file_plays_every_link() {
    for name in ["chained.ogg", "chained.opus"] {
        let (frames, info) = axross_amp::engine::decode_file(&media(name), RATE).unwrap();
        let secs = frames as f64 / RATE as f64;
        assert!((secs - 3.0).abs() < 0.06, "{name}: {secs:.3} s of 3");
        let duration = info.duration.unwrap_or(0.0);
        assert!(
            (duration - 3.0).abs() < 0.06,
            "{name}: said {duration:.3} s"
        );
        // what is heard: the first link's 440 Hz, then the second's 660 Hz
        let engine = Engine::start_headless(|| {});
        load(&engine, &media(name), 1);
        let mut output = Output::new();
        output.play(&engine, 3.4, |_, _| {});
        let frames: Vec<f32> = output.heard.clone();
        let first = frames.chunks(2).position(|f| f[0] != 0.0).unwrap();
        let at = |s: f32| first * 2 + (s * RATE as f32) as usize * 2;
        let (a, b) = (
            frequency(&frames[at(0.3)..at(1.2)]),
            frequency(&frames[at(1.8)..at(2.7)]),
        );
        assert!((a - 440.0).abs() < 8.0, "{name}: the first link at {a} Hz");
        assert!((b - 660.0).abs() < 8.0, "{name}: the second link at {b} Hz");
    }
}

/// A seek into a chained file's second link lands there.
#[test]
fn a_chained_ogg_file_seeks_into_a_later_link() {
    for name in ["chained.ogg", "chained.opus"] {
        let engine = Engine::start_headless(|| {});
        engine.send(Command::Load {
            path: media(name),
            start_ms: 2000,
            play: true,
            generation: 1,
        });
        let mut output = Output::new();
        output.play(&engine, 0.6, |_, _| {});
        let first = output.heard.chunks(2).position(|f| f[0] != 0.0).unwrap();
        let heard = &output.heard[first * 2..];
        let hz = frequency(&heard[..heard.len().min(RATE as usize / 3 * 2)]);
        assert!(
            (hz - 660.0).abs() < 8.0,
            "{name}: {hz} Hz after a seek to 2 s"
        );
        let position = engine.shared.position();
        assert!((2.0..2.8).contains(&position), "{name}: at {position}");
    }
}

/// An Ogg radio goes on across its songs (a new link each): its second
/// song is heard, and its title said.
#[test]
fn an_ogg_radio_plays_on_into_the_next_song() {
    let body = std::fs::read(media("chained.opus")).unwrap();
    let (base, _) = server(move |_, mut socket| {
        let _ = socket
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Type: audio/ogg\r\nicy-name: Ogg FM\r\n\r\n");
        let _ = socket.write_all(&body);
        std::thread::sleep(Duration::from_secs(8));
    });
    let engine = Engine::start_headless(|| {});
    load(&engine, &format!("{base}/live.opus"), 1);
    let mut output = Output::new();
    let mut events = Vec::new();
    output.play(&engine, 4.5, |_, e| events.push(describe(e)));
    let sounding =
        output.heard.chunks(2).filter(|f| f[0].abs() > 0.01).count() as f32 / RATE as f32;
    assert!(sounding > 2.5, "{sounding:.2} s heard of 3; {events:?}");
    assert!(
        events.iter().any(|e| e == "title 1: Second"),
        "the second song's title: {events:?}"
    );
}

/// A VBR MP3 without a Xing or VBRI header: played whole, its length that
/// of its frames (review 13, M3: cut at symphonia's estimate - half of it -
/// or said to be ten times as long).
#[test]
fn a_vbr_mp3_without_a_xing_header_plays_whole_and_says_its_length() {
    for name in ["vbr-noxing-short.mp3", "vbr-noxing-long.mp3"] {
        let (frames, info) = axross_amp::engine::decode_file(&media(name), 44_100).unwrap();
        let secs = frames as f64 / 44_100.0;
        assert!((secs - 4.05).abs() < 0.08, "{name}: {secs:.3} s of 4.05");
        let duration = info.duration.unwrap_or(0.0);
        assert!(
            (duration - 4.05).abs() < 0.08,
            "{name}: said {duration:.3} s"
        );
    }
    // and a seek beyond the estimate of the short one lands where asked
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: media("vbr-noxing-short.mp3"),
        start_ms: 3000,
        play: true,
        generation: 1,
    });
    let mut output = Output::new();
    output.play(&engine, 0.5, |_, _| {});
    let first = output.heard.chunks(2).position(|f| f[0] != 0.0);
    assert!(first.is_some(), "nothing heard after a seek to 3 s");
    let position = engine.shared.position();
    assert!((3.0..3.7).contains(&position), "at {position}");
    let heard = &output.heard[first.unwrap() * 2..];
    let hz = frequency(&heard[..heard.len().min(RATE as usize / 4 * 2)]);
    assert!((hz - 440.0).abs() < 10.0, "{hz} Hz after the seek");
}

/// A float WAV of `samples` (interleaved stereo, 48 kHz).
fn float_wav(samples: &[f32]) -> Vec<u8> {
    let data = samples.len() as u32 * 4;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [
        16u32.to_le_bytes().as_slice(),
        &3u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &RATE.to_le_bytes(),
        &(RATE * 8).to_le_bytes(),
        &8u16.to_le_bytes(),
        &32u16.to_le_bytes(),
    ] {
        out.extend_from_slice(v);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// One NaN or infinite sample does not silence the equalized track to its
/// end (review 13, M1): what is heard is finite, and sound after it.
#[test]
fn a_nan_or_an_infinite_sample_does_not_poison_the_equalizer() {
    let dir = folder("nan");
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut samples = tone(330.0, 2.0);
        samples[RATE as usize] = bad; // (at 0.5 s, the left channel)
        let path = dir.join("bad.wav");
        std::fs::write(&path, float_wav(&samples)).unwrap();
        let engine = Engine::start_headless(|| {});
        let mut gains = [0.0f32; 10];
        gains[0] = 6.0;
        engine.send(Command::Equalizer {
            gains,
            enabled: true,
        });
        load(&engine, &path.to_string_lossy(), 1);
        let mut output = Output::new();
        output.play(&engine, 1.6, |_, _| {});
        let wrong = output.heard.iter().filter(|s| !s.is_finite()).count();
        assert_eq!(wrong, 0, "{bad}: {wrong} samples not finite heard");
        let first = output.heard.chunks(2).position(|f| f[0] != 0.0).unwrap();
        let after = &output.heard[(first + RATE as usize) * 2..(first + RATE as usize * 3 / 2) * 2];
        let loud = after.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(loud > 0.05, "{bad}: silent after it ({loud})");
    }
}

/// The output lost (and opened again at the same rate) while the next
/// track is decoded behind the current one: every frame of each heard once
/// (review 13, M2: 96 ms of the next track were heard twice).
#[test]
fn a_new_output_while_the_next_track_is_queued_repeats_nothing() {
    let dir = folder("reopen-gapless");
    let dc = |v: f32| -> Vec<f32> { vec![v; RATE as usize * 2] };
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    std::fs::write(&a, wav_of(RATE, &dc(0.25))).unwrap();
    std::fs::write(&b, wav_of(RATE, &dc(0.5))).unwrap();
    for lose in [false, true] {
        let engine = Engine::start_headless(|| {});
        load(&engine, &a.to_string_lossy(), 1);
        engine.send(Command::Queue(Some(Queued {
            path: b.to_string_lossy().into_owned(),
            generation: 2,
        })));
        let mut output = Output::new();
        output.play(&engine, 0.8, |_, _| {});
        if lose {
            engine.shared.simulate_output_loss();
        }
        output.play(&engine, 1.8, |_, _| {});
        // (at the output's volume: B is twice as loud as A)
        let loudest = output.heard.iter().fold(0.0f32, |m, s| m.max(*s));
        let count = |v: f32| {
            output
                .heard
                .chunks(2)
                .filter(|f| (f[0] - v).abs() < 0.002)
                .count() as i64
        };
        let (na, nb) = (count(loudest / 2.0), count(loudest));
        assert!((na - RATE as i64).abs() <= 400, "lost {lose}: A {na}");
        assert!((nb - RATE as i64).abs() <= 400, "lost {lose}: B {nb}");
    }
}

/// Damage in the middle of a file ends the track there (what came before
/// is played), where it failed the whole track (review 13, L3) - also an
/// Ogg file whose damage symphonia's reader finds as it opens it (review
/// 14, L2: refused at the opening).
#[test]
fn damage_in_the_middle_ends_the_track_there() {
    for name in ["damaged.ogg", "damaged.opus"] {
        let decoded = axross_amp::engine::decode_file(&media(name), RATE);
        let (frames, _) = decoded.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(frames as u32 > RATE * 2 / 3, "{name}: {frames} frames");
    }
    // damage in the first page of sound: the whole pages after it play
    // (review 15, L2: nothing came before it, and the file was refused)
    for name in ["damaged-first.ogg", "damaged-first.opus"] {
        let decoded = axross_amp::engine::decode_file(&media(name), RATE);
        let (frames, _) = decoded.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(frames as u32 > RATE * 3 / 2, "{name}: {frames} frames");
    }
    // (review 13's file: its first two pages of sound damaged, the last -
    // 14 ms - whole: opened, not refused)
    let decoded = axross_amp::engine::decode_file(&media("corrupt-first-page.opus"), RATE);
    decoded.unwrap_or_else(|e| panic!("corrupt-first-page.opus: {e}"));
    let dir = folder("damaged");
    for name in ["chained.opus", "chained.ogg"] {
        let mut bytes = std::fs::read(media(name)).unwrap();
        let n = bytes.len();
        let mut seed = 3u32;
        for frac in [0.4, 0.6] {
            let at = (n as f64 * frac) as usize;
            for b in &mut bytes[at..at + 2048] {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *b = (seed >> 24) as u8;
            }
        }
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let decoded = axross_amp::engine::decode_file(&path.to_string_lossy(), RATE);
        let (frames, _) = decoded.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(frames as u32 > RATE * 4 / 5, "{name}: {frames} frames");
    }
}

/// ReplayGain changed while the next track is opened ahead: the next one
/// is heard at the new gain too (review 13, L4: it kept the old one).
#[test]
fn a_new_replaygain_reaches_the_next_track() {
    let dir = folder("gain-next");
    let dc = |v: f32| -> Vec<f32> { vec![v; RATE as usize * 2] };
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    std::fs::write(&a, wav_of(RATE, &dc(0.4))).unwrap();
    std::fs::write(&b, wav_of(RATE, &dc(0.4))).unwrap();
    let engine = Engine::start_headless(|| {});
    load(&engine, &a.to_string_lossy(), 1);
    engine.send(Command::Queue(Some(Queued {
        path: b.to_string_lossy().into_owned(),
        generation: 2,
    })));
    let mut output = Output::new();
    output.play(&engine, 0.4, |_, _| {});
    // ReplayGain on: an untagged track at -6 dB
    engine.send(Command::Loudness(axross_amp::engine::Loudness {
        mode: axross_amp::engine::GainMode::Track,
        preamp_db: 0.0,
        untagged_db: -6.0,
    }));
    output.play(&engine, 2.0, |_, _| {});
    let left: Vec<f32> = output.heard.chunks(2).map(|f| f[0]).collect();
    let first = left.iter().position(|s| *s != 0.0).unwrap();
    // A (decoded before the change, at 0 dB) at 0.1 .. 0.3 s, B at 1.2 ..
    // 1.6 s: 6 dB lower
    let level = |from: f32, to: f32| {
        let part =
            &left[first + (from * RATE as f32) as usize..first + (to * RATE as f32) as usize];
        part.iter().sum::<f32>() / part.len() as f32
    };
    let (la, lb) = (level(0.1, 0.3), level(1.2, 1.6));
    let db = 20.0 * (lb / la).log10();
    assert!((db + 6.0).abs() < 0.2, "A {la}, B {lb}: {db} dB");
}

/// What is heard of `path` from its start: played at once (`seek` false),
/// or loaded paused, sought to 0 and played (Stop, Home, Previous).
fn start_of(path: &str, seek: bool) -> Vec<f32> {
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: path.to_owned(),
        start_ms: 0,
        play: !seek,
        generation: 1,
    });
    if seek {
        wait_for(&engine, 5.0, |e| matches!(e, Event::Loaded { .. }));
        std::thread::sleep(Duration::from_millis(300));
        engine.send(Command::Seek(0.0));
        std::thread::sleep(Duration::from_millis(300));
        engine.send(Command::Play);
    }
    let mut output = Output::new();
    output.play(&engine, 0.5, |_, _| {});
    let left: Vec<f32> = output.heard.chunks(2).map(|f| f[0]).collect();
    let first = left.iter().position(|s| s.abs() > 1e-4).unwrap_or(0);
    left[first..].to_vec()
}

/// A seek to 0 lands where playing from the start does - the encoder's
/// delay of a LAME MP3 dropped once (not twice), an Opus file's pre-skip
/// dropped (not played) (review 13, L1: 331 to 1204 frames of an MP3 lost,
/// 312 of pre-skip heard).
#[test]
fn a_seek_to_the_start_lands_where_the_start_is() {
    for name in ["chirp.mp3", "chirp.opus"] {
        let (a, b) = (start_of(&media(name), false), start_of(&media(name), true));
        // the lag of b against a, by a window of a after its start
        let window = &a[2000..4000];
        let mut best = (f32::MAX, 0i64);
        for lag in -1500i64..1500 {
            let at = 2000 + lag;
            if at < 0 || at as usize + 2000 > b.len() {
                continue;
            }
            let at = at as usize;
            let error: f32 = window
                .iter()
                .zip(&b[at..at + 2000])
                .map(|(x, y)| (x - y).powi(2))
                .sum();
            if error < best.0 {
                best = (error, lag);
            }
        }
        assert!(best.1.abs() <= 2, "{name}: shifted by {} frames", best.1);
    }
}

/// A recording of an Ogg station - its granule positions begin hours in,
/// where it joined the station: as long as it is, and sought within (review
/// 14, M2: a 2 s recording said 10 802 s, a seek to 1 s ended it, a second
/// song after it could not be reached).
#[test]
fn a_recording_of_an_ogg_station_is_timed_and_sought_as_it_is() {
    for (name, secs) in [("capture1.ogg", 2.0), ("capture.ogg", 4.0)] {
        let (frames, info) = axross_amp::engine::decode_file(&media(name), RATE).unwrap();
        let played = frames as f64 / RATE as f64;
        assert!((played - secs).abs() < 0.1, "{name}: {played:.3} s");
        let said = info.duration.unwrap_or(0.0);
        assert!((said - secs).abs() < 0.1, "{name}: said {said:.3} s");
    }
    for (name, at, hz) in [("capture1.ogg", 1000, 440.0), ("capture.ogg", 3000, 660.0)] {
        let engine = Engine::start_headless(|| {});
        engine.send(Command::Load {
            path: media(name),
            start_ms: at,
            play: true,
            generation: 1,
        });
        let mut output = Output::new();
        output.play(&engine, 0.5, |_, _| {});
        let first = output.heard.chunks(2).position(|f| f[0] != 0.0);
        let first = first.unwrap_or_else(|| panic!("{name}: nothing after a seek to {at} ms"));
        let heard = &output.heard[first * 2..];
        let found = frequency(&heard[..heard.len().min(RATE as usize / 4 * 2)]);
        assert!((found - hz).abs() < 10.0, "{name} at {at} ms: {found} Hz");
        let position = engine.shared.position() * 1000.0;
        assert!(
            (at as f64..at as f64 + 700.0).contains(&position),
            "{name}: at {position} ms"
        );
    }
}

/// An equalizer of cuts alone can still raise a frequency a little (its
/// fit to axross's curve ripples): a master at full scale there is held
/// below it by the output's limiter, not clipped (review 14, L3: the
/// limiter held only where a band was above 0 dB).
#[test]
fn an_equalizer_of_cuts_only_never_clips_a_hot_master() {
    let mut gains = [0.0f32; 10];
    for g in gains.iter_mut().skip(1).step_by(2) {
        *g = -12.0;
    }
    let mut eq = axross_amp::eq::Equalizer::new(RATE);
    eq.set_gains(gains);
    let hz = (0..400)
        .map(|i| 20.0 * 1000f64.powf(i as f64 / 399.0))
        .max_by(|a, b| eq.response_db(*a).total_cmp(&eq.response_db(*b)))
        .unwrap();
    assert!(
        eq.response_db(hz) > 0.3,
        "cuts only raise {hz} Hz by {}",
        eq.response_db(hz)
    );
    let dir = folder("cuts-only");
    let peak = 10f32.powf(-0.1 / 20.0);
    let master: Vec<f32> = (0..RATE as usize * 2)
        .flat_map(|i| {
            let v = (std::f64::consts::TAU * hz * i as f64 / RATE as f64).sin() as f32 * peak;
            [v, v]
        })
        .collect();
    std::fs::write(dir.join("hot.wav"), wav_of(RATE, &master)).unwrap();
    let engine = Engine::start_headless(|| {});
    engine.shared.set_volume(1.0);
    engine.send(Command::Equalizer {
        gains,
        enabled: true,
    });
    load(&engine, &dir.join("hot.wav").to_string_lossy(), 1);
    let mut output = Output::new();
    output.play(&engine, 1.6, |_, _| {});
    let heard = heard_from_start(&output.heard);
    let at_top = heard.iter().filter(|v| v.abs() >= 0.9999).count();
    assert_eq!(at_top, 0, "{at_top} samples clipped at full scale");
}

/// The playlist's lengths (read before a track plays) are the measured
/// ones (review 14, L5: a VBR MP3 without a Xing header said 40 s for 4, a
/// chained Ogg its first link's length).
#[test]
fn a_tracks_length_before_it_plays_is_the_measured_one() {
    for (name, secs) in [
        ("vbr-noxing-long.mp3", 4.05),
        ("vbr-noxing-short.mp3", 4.05),
        ("chained.ogg", 3.0),
        ("capture1.ogg", 2.0),
    ] {
        let info = axross_amp::engine::probe(&media(name)).unwrap();
        let said = info.duration.unwrap_or(0.0);
        assert!((said - secs).abs() < 0.1, "{name}: {said:.2} s");
    }
}

/// Vorbis paged as libogg (oggenc) pages it, beginning with seconds of
/// silence (its first page holds them all): its length and its seeks as
/// they are (review 15, M2: taken for a station's recording - 6.24 s said
/// for 8, a seek to 5 s at 6.76 s).
#[test]
fn an_ogg_file_beginning_quietly_is_timed_and_sought_as_it_is() {
    for (name, secs) in [("lo-sil3-2tone.ogg", 8.0), ("lo-sil2-2tone.ogg", 7.0)] {
        let (frames, info) = axross_amp::engine::decode_file(&media(name), RATE).unwrap();
        assert!(
            (frames as f64 / RATE as f64 - secs).abs() < 0.1,
            "{name}: decoded"
        );
        let said = info.duration.unwrap_or(0.0);
        assert!((said - secs).abs() < 0.1, "{name}: said {said:.2} s");
        let probed = axross_amp::engine::probe(&media(name))
            .unwrap()
            .duration
            .unwrap_or(0.0);
        assert!((probed - secs).abs() < 0.1, "{name}: probed {probed:.2} s");
    }
    // 3 s of silence, 2.5 s at 440 Hz, 2.5 s at 660 Hz: 5 s is the first tone
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: media("lo-sil3-2tone.ogg"),
        start_ms: 5000,
        play: true,
        generation: 1,
    });
    let mut output = Output::new();
    output.play(&engine, 0.5, |_, _| {});
    let first = output.heard.chunks(2).position(|f| f[0] != 0.0).unwrap();
    let heard = &output.heard[first * 2..];
    let hz = frequency(&heard[..heard.len().min(RATE as usize / 4 * 2)]);
    assert!((hz - 440.0).abs() < 10.0, "{hz} Hz after a seek to 5 s");
    let position = engine.shared.position();
    assert!((5.0..5.7).contains(&position), "at {position}");
}
