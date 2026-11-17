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

/// The output: `secs` of sound rendered at a device's pace (10 ms a call).
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
        let end = Instant::now() + Duration::from_secs_f32(secs);
        let mut buf = vec![0.0f32; 480 * 2];
        while Instant::now() < end {
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
