//! A file cut short - a copy interrupted, a download that stopped - plays
//! what is there. WAV, FLAC and MP3 did; an AIFF file was refused whole
//! ("the file is empty or cut short": its reader looks for what follows
//! the sound, which is not there), and an Ogg file lost the sound of its
//! last whole page (review 19, L9).

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::engine::{Command, Engine, Event as EngineEvent, RenderState};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Three seconds of a tone as AIFF: 44.1 kHz, 16 bits, two channels.
fn aiff(seconds: f32) -> Vec<u8> {
    let rate = 44_100u32;
    let frames = (seconds * rate as f32) as u32;
    let mut sound = Vec::with_capacity(frames as usize * 4);
    for i in 0..frames {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
        sound.extend_from_slice(&v.to_be_bytes());
        sound.extend_from_slice(&v.to_be_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(4 + 8 + 18 + 8 + 8 + sound.len() as u32).to_be_bytes());
    out.extend_from_slice(b"AIFF");
    out.extend_from_slice(b"COMM");
    out.extend_from_slice(&18u32.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&frames.to_be_bytes());
    out.extend_from_slice(&16u16.to_be_bytes());
    // (44100 as an 80-bit extended float)
    out.extend_from_slice(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&(8 + sound.len() as u32).to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&sound);
    out
}

/// What is heard of `path`: seconds of sound, and why it failed if it did.
fn heard(path: &Path) -> (f64, Option<String>) {
    let (sound, failed, _) = played(path);
    (sound, failed)
}

/// What is heard of `path`, why it failed if it did, and the length shown
/// for it: as it is loaded, and at its end.
fn played(path: &Path) -> (f64, Option<String>, [Option<f64>; 2]) {
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: path.to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    let mut state = RenderState::default();
    let mut buf = vec![0.0f32; 480 * 2];
    let started = Instant::now();
    let (mut sound, mut failed, mut ended) = (0usize, None, false);
    let mut shown = [None; 2];
    while started.elapsed() < Duration::from_secs(30) && !ended && failed.is_none() {
        let wait = Instant::now();
        while !engine.shared.ready(480) && wait.elapsed() < Duration::from_millis(50) {
            std::thread::sleep(Duration::from_millis(1));
        }
        engine.shared.render(&mut buf, 2, &mut state);
        sound += buf.chunks(2).filter(|f| f[0] != 0.0).count();
        while let Ok(e) = engine.events.try_recv() {
            match e {
                EngineEvent::Failed { message, .. } => failed = Some(message),
                EngineEvent::Loaded { info, .. } => shown = [info.duration; 2],
                EngineEvent::Length { secs, .. } => shown[1] = Some(secs),
                EngineEvent::Ended { .. } => ended = true,
                _ => {}
            }
        }
    }
    engine.send(Command::Quit);
    (sound as f64 / 48_000.0, failed, shown)
}

/// A tone as a WAV file: 44.1 kHz, 16 bits, two channels.
fn wav(seconds: f32) -> Vec<u8> {
    let rate = 44_100u32;
    let frames = (seconds * rate as f32) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(frames * 4).to_le_bytes());
    for i in 0..frames {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Review 20, L5: the length shown for a file cut short was its header's:
/// 20 s shown, 10 s played. A WAV and an AIFF file show what is there from
/// the start; an MP3 file (and whatever else says its length in its first
/// bytes) once its end is read.
#[test]
fn a_file_cut_short_shows_how_long_it_is() {
    let home = scratch::Scratch::new("cut-length");
    let media = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/media");
    let mut bad = Vec::new();
    for (name, bytes, at_once) in [
        ("tone.wav", wav(4.0), true),
        ("tone.aiff", aiff(4.0), true),
        (
            "chirp.mp3",
            std::fs::read(media.join("chirp.mp3")).unwrap(),
            false,
        ),
    ] {
        let whole = home.join(format!("whole-{name}"));
        std::fs::write(&whole, &bytes).unwrap();
        let (all, failed, shown) = played(&whole);
        assert!(failed.is_none(), "{name}: {failed:?}");
        let said = shown[0].expect("a length");
        assert_eq!(shown[1], shown[0], "{name}: whole, and its length changed");
        assert!(
            (said - all).abs() < 0.2,
            "{name}: {said} s shown, {all} s heard"
        );
        for share in [0.5, 0.31, 0.9] {
            let path = home.join(format!("{share}-{name}"));
            std::fs::write(&path, &bytes[..(bytes.len() as f64 * share) as usize | 1]).unwrap();
            let (sound, failed, shown) = played(&path);
            println!("{name} cut to {share}: {sound:.2} s heard, shown {shown:?}");
            let off = |k: usize| shown[k].is_none_or(|s| (s - sound).abs() > 0.3);
            if failed.is_some() || off(1) || (at_once && off(0)) {
                bad.push(format!(
                    "{name} cut to {share}: {sound:.2} s heard, {shown:?} shown, {failed:?}"
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The granule position (where its sound ends) of the last whole page of
/// an Ogg file that says one - or of the last page begun (`and_cut`); None
/// for another kind of file.
fn last_granule(data: &[u8], and_cut: bool) -> Option<u64> {
    if data.get(..4) != Some(b"OggS") {
        return None;
    }
    let (mut at, mut last) = (0usize, None);
    while let Some(header) = data.get(at..at + 27) {
        if &header[..4] != b"OggS" {
            break;
        }
        let segments = header[26] as usize;
        let Some(table) = data.get(at + 27..at + 27 + segments) else {
            break;
        };
        let len = 27 + segments + table.iter().map(|s| *s as usize).sum::<usize>();
        if at + len > data.len() && !and_cut {
            break;
        }
        let granule = u64::from_le_bytes(header[6..14].try_into().unwrap());
        if granule != u64::MAX && granule > 0 {
            last = Some(granule);
        }
        at += len;
    }
    last
}

#[test]
fn a_file_cut_short_plays_what_is_there() {
    let home = scratch::Scratch::new("cut-short");
    let media = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/media");
    let mut files: Vec<(String, Vec<u8>)> = vec![("tone.aiff".into(), aiff(3.0))];
    for name in ["lo-plain.ogg", "chirp.opus"] {
        files.push((name.into(), std::fs::read(media.join(name)).unwrap()));
    }
    // (and the corpus's, where there is one: larger pages)
    if let Some(corpus) = std::env::var_os("AMP_CORPUS").map(PathBuf::from) {
        for name in ["tone.ogg", "tone.opus", "tone.aiff"] {
            if let Ok(bytes) = std::fs::read(corpus.join(name)) {
                files.push((format!("corpus-{name}"), bytes));
            }
        }
    }
    let mut bad = Vec::new();
    for (name, bytes) in &files {
        let whole_path = home.join(format!("whole-{name}"));
        std::fs::write(&whole_path, bytes).unwrap();
        let (whole, failed) = heard(&whole_path);
        assert!(
            failed.is_none() && whole > 0.9,
            "{name}: {whole} s, {failed:?}"
        );
        for (what, share) in [("half", 0.5), ("nine-tenths", 0.9), ("a-third", 0.33)] {
            let cut = (bytes.len() as f64 * share) as usize;
            let path = home.join(format!("{what}-{name}"));
            std::fs::write(&path, &bytes[..cut]).unwrap();
            let (sound, failed) = heard(&path);
            // (how much of the sound is there: of an Ogg file, between
            // the end of its last whole page and of the page it is cut in)
            let (least, most) = match (
                last_granule(&bytes[..cut], false),
                last_granule(&bytes[..cut], true),
                last_granule(bytes, false),
            ) {
                (whole_pages, Some(with_cut), Some(all)) => (
                    whole * whole_pages.unwrap_or(0) as f64 / all as f64,
                    whole * with_cut as f64 / all as f64,
                ),
                _ => (whole * share, whole * share),
            };
            println!(
                "{name:<16} {what:<12}: {sound:.2} s of {whole:.2} s; {least:.2} - {most:.2} s are there, {failed:?}"
            );
            if failed.is_some() || sound < least - 0.06 || sound > most + 0.06 {
                bad.push(format!(
                    "{name} cut to {what}: {sound:.2} s heard, {least:.2} - {most:.2} s are there, {failed:?}"
                ));
            }
            // (and of the page it is cut in, what is whole: a file of one
            // bitrate plays as much of its sound as there is of its bytes)
            if name.ends_with(".opus") && (sound - whole * share).abs() > 0.12 {
                bad.push(format!(
                    "{name} cut to {what}: {sound:.2} s heard of {:.2} s",
                    whole * share
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
