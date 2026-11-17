//! The engine end to end with real sound: ALSA's `file` plugin (no sound
//! card needed) writes everything the output plays into a WAV file, which is
//! then measured - the tone of each channel (FFT), the length, the level.
//!
//! The corpus (left 440 Hz, right 660 Hz, mono 440 Hz, three seconds, tagged)
//! is made by a script with PyAV; set AMP_CORPUS to its folder. Without it
//! the test says so and passes (CI without the corpus).

// (the WAV reader reads more plainly with chunks_exact)
#![allow(clippy::chunks_exact_to_as_chunks)]

use axross_amp::engine::{Command, Engine, Event, TrackInfo};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn corpus() -> Option<PathBuf> {
    std::env::var_os("AMP_CORPUS")
        .map(PathBuf::from)
        .filter(|p| p.join("tone.flac").exists())
}

/// ALSA's default device writes into `capture` (a WAV file).
fn route_alsa_into(home: &Path, capture: &Path) {
    std::fs::write(
        home.join(".asoundrc"),
        format!(
            "pcm.!default {{\n type file\n slave.pcm \"null\"\n file \"{}\"\n format \"wav\"\n}}\n",
            capture.display()
        ),
    )
    .unwrap();
}

struct Played {
    events: Vec<String>,
    info: Option<TrackInfo>,
    ended: bool,
    failed: Option<String>,
}

/// Play `path` to its end (or failure) on a fresh engine; the capture is
/// complete once the engine is gone.
fn play(path: &str, start_ms: u64, setup: impl FnOnce(&Engine)) -> Played {
    let engine = Engine::start(|| {});
    setup(&engine);
    engine.send(Command::Load {
        path: path.to_owned(),
        start_ms,
        play: true,
        generation: 1,
    });
    let mut played = Played {
        events: Vec::new(),
        info: None,
        ended: false,
        failed: None,
    };
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && !played.ended && played.failed.is_none() {
        match engine.events.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::Output(name)) => played.events.push(format!("output {name}")),
            Ok(Event::Loaded { info, .. }) => played.info = Some(info),
            Ok(Event::Ended { .. }) => played.ended = true,
            Ok(Event::Failed { message, .. }) => played.failed = Some(message),
            Ok(_) | Err(_) => {}
        }
    }
    drop(engine);
    std::thread::sleep(Duration::from_millis(300)); // (the stream closes, the WAV header is written)
    played
}

struct Capture {
    rate: u32,
    channels: usize,
    samples: Vec<f32>,
}

fn read_wav(path: &Path) -> Capture {
    let bytes = std::fs::read(path).expect("a capture");
    assert_eq!(&bytes[0..4], b"RIFF");
    let mut pos = 12;
    let (mut rate, mut channels, mut bits, mut format) = (0u32, 0usize, 0u16, 0u16);
    let mut samples = Vec::new();
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        if id == b"fmt " {
            format = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]) as usize;
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            // (ALSA may leave the size 0 or too big while streaming: take what is there)
            let body = &bytes[pos + 8..];
            // ALSA's file plugin writes format 1 (integer PCM) also for float
            // samples: 32-bit data that reads as floats within ±1 are floats
            let as_float: Vec<f32> = body
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                .collect();
            // (silence may be -0.0, which as an integer is the most negative one)
            let floats = bits == 32 && as_float.iter().all(|v| v.is_finite() && v.abs() <= 1.5);
            samples = match (format, bits) {
                (3, 32) => as_float,
                (_, 32) if floats => as_float,
                (_, 16) => body
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                    .collect(),
                (_, 32) => body
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f32 / 2147483648.0)
                    .collect(),
                other => panic!("capture format {other:?}"),
            };
            break;
        }
        pos += 8 + size + (size & 1);
    }
    Capture {
        rate,
        channels,
        samples,
    }
    .compact()
}

impl Capture {
    /// The capture without the silence the output put in between: the null
    /// device has no clock and takes what is there at once, so whenever the
    /// decoder had not caught up the output filled the pause with exact
    /// zeros (a real device paces the output and has no such pauses). The
    /// sound itself arrives whole, packet after packet - put back together
    /// it is the track as played.
    fn compact(mut self) -> Self {
        let ch = self.channels;
        let frames: Vec<&[f32]> = self.samples.chunks_exact(ch).collect();
        let mut keep = vec![true; frames.len()];
        let mut run = 0usize;
        for (i, f) in frames.iter().enumerate() {
            if f.iter().all(|s| *s == 0.0) {
                run += 1;
            } else {
                if run >= 8 {
                    keep[i - run..i].iter_mut().for_each(|k| *k = false);
                }
                run = 0;
            }
        }
        let n = frames.len();
        if run > 0 {
            keep[n - run..].iter_mut().for_each(|k| *k = false);
        }
        self.samples = frames
            .iter()
            .zip(keep)
            .filter(|(_, k)| *k)
            .flat_map(|(f, _)| f.iter().copied())
            .collect();
        self
    }

    fn channel(&self, c: usize) -> Vec<f32> {
        self.samples
            .chunks_exact(self.channels)
            .map(|f| f[c])
            .collect()
    }

    /// How long the sound played (the capture without its pauses).
    fn audible_secs(&self) -> f64 {
        (self.samples.len() / self.channels.max(1)) as f64 / self.rate as f64
    }
}

/// The frequency of `signal` (its strongest, from an FFT with a Hann window
/// and parabolic interpolation between bins) and its RMS level.
fn tone_of(signal: &[f32], rate: u32) -> (f32, f32) {
    // the middle, away from the edges of the track
    let edge = (signal.len() / 20).min(4096);
    let body = &signal[edge..signal.len() - edge];
    let n = body.len();
    assert!(
        n >= 4096,
        "no tone in the capture ({} samples)",
        signal.len()
    );
    let mut buf: Vec<Complex<f32>> = body
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Complex::new(
                s * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()),
                0.0,
            )
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let mags: Vec<f32> = buf[..n / 2].iter().map(|c| c.norm().max(1e-12)).collect();
    let k = (1..mags.len() - 1)
        .max_by(|a, b| mags[*a].total_cmp(&mags[*b]))
        .unwrap();
    let (y0, y1, y2) = (mags[k - 1].ln(), mags[k].ln(), mags[k + 1].ln());
    let offset = 0.5 * (y0 - y2) / (y0 - 2.0 * y1 + y2);
    let hz = (k as f32 + offset) * rate as f32 / n as f32;
    let rms = (body.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / n as f64).sqrt();
    (hz, rms as f32)
}

fn near(value: f32, want: f32, tolerance: f32) -> bool {
    (value - want).abs() <= tolerance
}

/// A tiny HTTP server for one file (a stream to the engine); `status` other
/// than 200 answers that and nothing else.
fn serve(path: PathBuf, status: u16, content_type: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request);
            if status != 200 {
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\n\r\n"
                );
                return;
            }
            let body = std::fs::read(&path).unwrap();
            // no Content-Length: a radio's endless body ends when the connection does
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n"
            );
            let _ = socket.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/live")
}

/// `source` with an ID3v2.3 tag carrying a PNG cover (APIC) in front.
fn with_cover(source: &Path, png: &[u8], target: &Path) {
    let mut frame = vec![0u8]; // text encoding
    frame.extend_from_slice(b"image/png\0");
    frame.push(3); // front cover
    frame.push(0); // empty description
    frame.extend_from_slice(png);
    with_frames(source, &[(*b"APIC", frame)], target);
}

/// `source` with an ID3v2.3 tag of `frames` (id, body) in front.
fn with_frames(source: &Path, frames: &[([u8; 4], Vec<u8>)], target: &Path) {
    let mut tag = Vec::new();
    for (id, frame) in frames {
        tag.extend_from_slice(id);
        tag.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        tag.extend_from_slice(&[0, 0]);
        tag.extend_from_slice(frame);
    }
    let size = tag.len() as u32;
    let syncsafe = [
        (size >> 21) as u8 & 0x7f,
        (size >> 14) as u8 & 0x7f,
        (size >> 7) as u8 & 0x7f,
        size as u8 & 0x7f,
    ];
    let mut out = b"ID3\x03\x00\x00".to_vec();
    out.extend_from_slice(&syncsafe);
    out.extend_from_slice(&tag);
    out.extend_from_slice(&std::fs::read(source).unwrap());
    std::fs::write(target, out).unwrap();
}

#[test]
fn real_sound_from_every_format_with_eq_volume_seek_streams_and_errors() {
    let Some(corpus) = corpus() else {
        eprintln!("AMP_CORPUS not set: the sound test is skipped");
        return;
    };
    let home = std::env::temp_dir().join(format!("amp-audio-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let capture = home.join("capture.wav");
    // SAFETY: the only test of this binary; set before ALSA reads its configuration
    unsafe { std::env::set_var("HOME", &home) };
    route_alsa_into(&home, &capture);
    let mut report = Vec::new();

    // every format: both channels carry their own tone, the whole length, tags read
    for (name, stereo, title) in [
        ("tone.mp3", true, true),
        ("tone.flac", true, true),
        ("tone24.flac", true, true),
        ("tone.ogg", true, true),
        ("tone.m4a", true, true),
        ("tone-alac.m4a", true, true),
        ("tone.wav", false, true),
        ("tone51.wav", true, true),
        ("tone.aiff", true, false),
        ("tone.opus", true, true),
    ] {
        let path = corpus.join(name);
        let played = play(path.to_str().unwrap(), 0, |_| {});
        assert!(played.failed.is_none(), "{name}: {:?}", played.failed);
        assert!(played.ended, "{name}: no end ({:?})", played.events);
        let info = played.info.expect("loaded");
        let cap = read_wav(&capture);
        let (left_hz, left_rms) = tone_of(&cap.channel(0), cap.rate);
        let (right_hz, _) = tone_of(&cap.channel(1), cap.rate);
        let secs = cap.audible_secs();
        report.push(format!(
            "{name}: {} {} Hz {} ch -> out {} Hz: L {left_hz:.0} Hz (rms {left_rms:.2}) R {right_hz:.0} Hz, {secs:.2} s, title {:?} artist {:?}",
            info.codec, info.rate, info.channels, cap.rate, info.title, info.artist
        ));
        assert!(near(left_hz, 440.0, 1.0), "{name}: left {left_hz}");
        assert!(
            near(right_hz, if stereo { 660.0 } else { 440.0 }, 1.0),
            "{name}: right {right_hz}"
        );
        assert!((2.99..=3.01).contains(&secs), "{name}: {secs} s");
        // (5.1 is downmixed as ITU-R BS.775 has it, scaled so it cannot
        // clip: the front's tone is about 7.7 dB below its stereo level)
        let least = if name == "tone51.wav" { 0.08 } else { 0.12 };
        assert!(left_rms > least, "{name}: level {left_rms}");
        if title && name != "tone51.wav" && name != "tone.wav" {
            assert_eq!(info.title, format!("Tone {name}"), "{name}");
            assert_eq!(info.artist, "Test Artist", "{name}");
        }
        assert!(
            info.duration.is_some_and(|d| (d - 3.0).abs() < 0.1),
            "{name}: duration {:?}",
            info.duration
        );
    }

    // the equalizer: -12 dB at 500 Hz takes the 440 Hz tone down by about 9 dB
    let flac = corpus.join("tone.flac");
    play(flac.to_str().unwrap(), 0, |_| {});
    let (_, flat) = tone_of(&read_wav(&capture).channel(0), 48_000);
    play(flac.to_str().unwrap(), 0, |e| {
        let mut gains = [0.0; 10];
        gains[4] = -12.0;
        e.send(Command::Equalizer {
            gains,
            enabled: true,
        });
    });
    let (_, cut) = tone_of(&read_wav(&capture).channel(0), 48_000);
    let db = 20.0 * (cut / flat).log10();
    report.push(format!(
        "equalizer -12 dB at 500 Hz: 440 Hz tone {db:.1} dB"
    ));
    assert!((-12.0..=-6.0).contains(&db), "eq {db}");

    // volume and mute
    play(flac.to_str().unwrap(), 0, |e| e.shared.set_volume(0.25));
    let (_, quiet) = tone_of(&read_wav(&capture).channel(0), 48_000);
    report.push(format!(
        "volume 0.25 against 0.8: level x{:.3} (0.3125 wanted)",
        quiet / flat
    ));
    // (the reference played at the engine's default volume, 0.8)
    assert!(
        near(quiet / flat, 0.25 / 0.8, 0.02),
        "volume {}",
        quiet / flat
    );
    play(flac.to_str().unwrap(), 0, |e| {
        e.shared
            .muted
            .store(true, std::sync::atomic::Ordering::Relaxed)
    });
    let muted = read_wav(&capture);
    let loudest = muted.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        loudest < 1e-4,
        "muted but audible: {} samples, loudest {loudest}, first {:?}",
        muted.samples.len(),
        &muted.samples[..muted.samples.len().min(8)]
    );
    // (a mute ramps down within the first buffer: at most that is left)

    // an output chosen that is not there (a headset switched off): the default plays
    let played = play(flac.to_str().unwrap(), 0, |e| {
        e.send(Command::Output(Some("alsa:no-such-device".into())))
    });
    report.push(format!("missing output: {:?}", played.events));
    assert!(
        played
            .events
            .iter()
            .any(|e| e.contains("the output chosen is not there")),
        "{:?}",
        played.events
    );
    assert!(played.ended && read_wav(&capture).audible_secs() > 2.9);

    // a start position: only the last second is heard
    let played = play(flac.to_str().unwrap(), 2000, |_| {});
    let secs = read_wav(&capture).audible_secs();
    report.push(format!("start at 2 s: {secs:.2} s heard"));
    assert!(
        played.ended && (0.99..=1.01).contains(&secs),
        "start: {secs}"
    );

    // an http stream (an endless body, unseekable)
    let url = serve(corpus.join("tone.mp3"), 200, "audio/mpeg");
    let played = play(&url, 0, |_| {});
    assert!(played.failed.is_none(), "stream: {:?}", played.failed);
    let info = played.info.unwrap();
    assert!(!info.seekable);
    let cap = read_wav(&capture);
    let (hz, _) = tone_of(&cap.channel(0), cap.rate);
    report.push(format!(
        "http stream: {hz:.0} Hz, {:.2} s",
        cap.audible_secs()
    ));
    assert!(near(hz, 440.0, 4.0) && cap.audible_secs() > 2.8);

    // errors are said, nothing hangs
    for (what, path) in [
        (
            "empty",
            corpus.join("empty.mp3").to_string_lossy().into_owned(),
        ),
        (
            "garbage",
            corpus.join("garbage.mp3").to_string_lossy().into_owned(),
        ),
        ("missing", "/nonexistent/x.mp3".to_owned()),
        ("folder", corpus.to_string_lossy().into_owned()),
        (
            "http 404",
            serve(corpus.join("tone.mp3"), 404, "audio/mpeg"),
        ),
        ("refused port", "http://127.0.0.1:9/x.mp3".to_owned()),
    ] {
        let played = play(&path, 0, |_| {});
        report.push(format!("{what}: {:?}", played.failed));
        assert!(played.failed.is_some(), "{what} did not fail");
    }

    // cover art from an ID3 tag
    let png = std::fs::read(corpus.join("cover.png")).unwrap();
    let covered = home.join("covered.mp3");
    with_cover(&corpus.join("tone.mp3"), &png, &covered);
    let played = play(covered.to_str().unwrap(), 0, |_| {});
    let cover = played.info.and_then(|i| i.cover);
    report.push(format!("cover: {:?} bytes", cover.as_ref().map(Vec::len)));
    assert_eq!(cover.as_deref(), Some(png.as_slice()));

    // ReplayGain: a track gain of -6 dB, the track mode on: half the level
    let txxx = |key: &str, value: &str| {
        let mut f = vec![0u8];
        f.extend_from_slice(key.as_bytes());
        f.push(0);
        f.extend_from_slice(value.as_bytes());
        (*b"TXXX", f)
    };
    let gained = home.join("gained.mp3");
    with_frames(
        &corpus.join("tone.mp3"),
        &[
            txxx("REPLAYGAIN_TRACK_GAIN", "-6.02 dB"),
            txxx("REPLAYGAIN_TRACK_PEAK", "0.400000"),
        ],
        &gained,
    );
    let plain = play(corpus.join("tone.mp3").to_str().unwrap(), 0, |_| {});
    let (_, level) = tone_of(&read_wav(&capture).channel(0), 48_000);
    assert!(plain.info.unwrap().track_gain.is_none());
    let played = play(gained.to_str().unwrap(), 0, |e| {
        e.send(Command::Loudness(axross_amp::engine::Loudness {
            mode: axross_amp::engine::GainMode::Track,
            preamp_db: 0.0,
            ..Default::default()
        }))
    });
    let info = played.info.unwrap();
    assert_eq!((info.track_gain, info.track_peak), (Some(-6.02), Some(0.4)));
    let (_, levelled) = tone_of(&read_wav(&capture).channel(0), 48_000);
    report.push(format!(
        "ReplayGain -6.02 dB: level x{:.3}",
        levelled / level
    ));
    assert!(near(levelled / level, 0.5, 0.02), "{}", levelled / level);
    // off: as it is
    play(gained.to_str().unwrap(), 0, |_| {});
    let (_, off) = tone_of(&read_wav(&capture).channel(0), 48_000);
    assert!(near(off / level, 1.0, 0.02), "{}", off / level);

    println!("{}", report.join("\n"));
}
