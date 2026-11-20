//! The meters hear every sample, whatever the window does: the integrated
//! loudness of a track is the same with the picture shown, with the
//! picture off, with the window hidden (only `logic` runs) and at a few
//! frames a second (more sound a frame than the picture reads). And from
//! synthesised music, end to end through the player: a chord progression
//! gives its key, a melody its notes.

#[path = "support/player.rs"]
mod player;

use axross_amp::analyzer::loudness::Meter;
use axross_amp::app::{App, Setup};
use axross_amp::engine::RenderState;
use axross_amp::meters::Readings;
use eframe::App as _;
use player::*;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;

fn write_wav(path: &Path, samples: &[f32]) {
    let data = samples.len() as u32 * 2;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &RATE.to_le_bytes(),
        &(RATE * 4).to_le_bytes(),
        &4u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        f.write_all(v).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&data.to_le_bytes()).unwrap();
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s * 32767.0).round() as i16).to_le_bytes())
        .collect();
    f.write_all(&bytes).unwrap();
}

/// Programme: loud, quiet, louder - the gates have something to do.
fn programme() -> Vec<f32> {
    let mut out = Vec::new();
    for (secs, dbfs, hz) in [
        (1.2, -20.0, 1000.0),
        (0.8, -32.0, 440.0),
        (1.5, -14.0, 3000.0),
    ] {
        let amp = 10f32.powf(dbfs / 20.0);
        for i in 0..(secs * RATE as f32) as usize {
            let t = i as f32 / RATE as f32;
            let v = (t * hz * std::f32::consts::TAU).sin() * amp;
            out.push(v);
            out.push(v * 0.8);
        }
    }
    out
}

fn expected(samples: &[f32]) -> f32 {
    // what a meter that hears all of it says (the samples as the file holds them)
    let file: Vec<f32> = samples
        .iter()
        .map(|v| ((v * 32767.0).round() as i16) as f32 / 32768.0)
        .collect();
    let mut m = Meter::new(RATE);
    m.push(&file);
    m.integrated
}

fn setup(dir: &Path) -> Setup {
    Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    }
}

/// How long a test may take at most to play its few seconds (a busy
/// machine: what counts is the sound, not the clock).
const AT_MOST: Duration = Duration::from_secs(90);

/// Play until the meters have read all of the track (`frames`): `frame_ms`
/// of sound taken between two frames of the window (`frame` draws or only
/// runs its logic, and says what the meters read). By the sound, not the
/// clock: a time limit played less of it on a busy machine (1.6 - 1.9 s of
/// 3.5, the loud end unheard: -21.4 LUFS for -14.2).
fn play_through(
    frame: &mut impl FnMut() -> Readings,
    render: &mut impl FnMut(&mut [f32]),
    frame_ms: u64,
    frames: u64,
) -> Readings {
    let end = Instant::now() + AT_MOST;
    let mut buf = vec![0.0f32; 480 * 2];
    loop {
        for _ in 0..(frame_ms / 10).max(1) {
            render(&mut buf);
            std::thread::sleep(Duration::from_millis(10));
        }
        let r = frame();
        if r.frames + r.lost >= frames || Instant::now() > end {
            return r;
        }
    }
}

/// All of it read, nothing lost, and the loudness of all of it.
fn assert_heard_whole(case: &str, r: &Readings, frames: u64, want: f32) {
    assert_eq!(r.lost, 0, "{case}: {} frames lost ({r:?})", r.lost);
    assert_eq!(
        r.frames, frames,
        "{case}: {} of {frames} frames read",
        r.frames
    );
    assert!(
        (r.integrated - want).abs() < 0.1,
        "{case}: {:.2} LUFS, all of it heard reads {want:.2}",
        r.integrated
    );
}

#[test]
fn the_loudness_is_the_same_shown_off_hidden_and_at_three_frames_a_second() {
    let dir = folder("meters-loudness");
    let sound = programme();
    let total = (sound.len() / 2) as u64;
    let file = dir.join("programme.wav");
    write_wav(&file, &sound);
    let want = expected(&sound);
    let args = vec![file.to_string_lossy().into_owned()];
    for case in ["shown", "off", "hidden", "3 fps"] {
        let dir = dir.join(case.replace(' ', "-"));
        let r = if case == "hidden" {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = App::with_setup(&cc, args.clone(), setup(&dir));
            let mut frame = eframe::Frame::_new_kittest();
            let mut out = RenderState::default();
            let engine = app.engine().shared.clone();
            let mut render = |buf: &mut [f32]| engine.render(buf, 2, &mut out);
            let mut step = || {
                let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
                app.meter_readings()
            };
            play_through(&mut step, &mut render, 20, total)
        } else {
            let mut h = harness(&dir, args.clone());
            if case == "off" {
                h.state_mut().set_mode("off");
            }
            let shared = h.state().engine().shared.clone();
            let mut out = RenderState::default();
            let mut render = |buf: &mut [f32]| shared.render(buf, 2, &mut out);
            let frame_ms = if case == "3 fps" { 330 } else { 20 };
            let mut step = || {
                h.run_steps(1);
                h.state().meter_readings()
            };
            play_through(&mut step, &mut render, frame_ms, total)
        };
        assert_heard_whole(case, &r, total, want);
    }
}

/// Slow readers hear it all: the sound taken at its own pace on a thread of
/// its own (as an output's callback) while the window draws a frame a
/// second; and the meters' thread held up for three seconds mid-track (its
/// turn late on a busy machine - within what the tap keeps, 5.5 s at
/// 48 kHz; with the tap of 1.4 s it lost 73,728 frames). Every frame is
/// read and the loudness is that of all of it.
#[test]
fn slow_readers_hear_all_of_it() {
    let dir = folder("meters-slow");
    let sound = programme();
    let total = (sound.len() / 2) as u64;
    let file = dir.join("programme.wav");
    write_wav(&file, &sound);
    let want = expected(&sound);
    let args = vec![file.to_string_lossy().into_owned()];
    for case in ["a frame a second", "the meters held up"] {
        let mut h = harness(&dir.join(case.replace(' ', "-")), args.clone());
        let shared = h.state().engine().shared.clone();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let output = {
            let (shared, done) = (shared.clone(), done.clone());
            std::thread::spawn(move || {
                let mut out = RenderState::default();
                let mut buf = vec![0.0f32; 480 * 2];
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    shared.render(&mut buf, 2, &mut out);
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        };
        let end = Instant::now() + AT_MOST;
        let mut stalled = false;
        let r = loop {
            h.run_steps(1);
            let r = h.state().meter_readings();
            if r.frames + r.lost >= total || Instant::now() > end {
                break r;
            }
            if case == "the meters held up" {
                if !stalled && r.frames > 0 {
                    h.state().stall_meters(3000);
                    stalled = true;
                }
                std::thread::sleep(Duration::from_millis(20));
            } else {
                std::thread::sleep(Duration::from_secs(1));
            }
        };
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        output.join().unwrap();
        assert_heard_whole(case, &r, total, want);
    }
}

fn note(midi: f32, secs: f32, gain: f32) -> Vec<f32> {
    let f = 440.0 * 2f32.powf((midi - 69.0) / 12.0);
    (0..(secs * RATE as f32) as usize)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let envelope = (t / 0.01).min(1.0) * (-t * 0.8).exp();
            (1..=6)
                .map(|h| (t * f * h as f32 * std::f32::consts::TAU).sin() / h as f32)
                .sum::<f32>()
                * envelope
                * gain
        })
        .collect()
}

fn chord(notes: &[f32], secs: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; (secs * RATE as f32) as usize];
    for n in notes {
        for (o, v) in out.iter_mut().zip(note(*n, secs, 0.12)) {
            *o += v;
        }
    }
    out
}

fn stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|v| [*v, *v]).collect()
}

#[test]
fn a_chord_progression_gives_its_key() {
    // Am - F - C - G, twice (A minor, or its relative C major - not F major)
    let dir = folder("meters-key");
    let mut mono = Vec::new();
    for _ in 0..2 {
        for triad in [
            [57.0, 60.0, 64.0],
            [53.0, 57.0, 60.0],
            [48.0, 52.0, 55.0],
            [55.0, 59.0, 62.0],
        ] {
            // the root an octave below too, as a bass plays it
            let mut c = chord(&triad, 1.0);
            for (o, v) in c.iter_mut().zip(note(triad[0] - 12.0, 1.0, 0.15)) {
                *o += v;
            }
            mono.extend(c);
        }
    }
    let file = dir.join("progression.wav");
    write_wav(&file, &stereo(&mono));
    let mut h = harness(&dir, vec![file.to_string_lossy().into_owned()]);
    let mut out = RenderState::default();
    listen(&mut h, &mut out, 8.4);
    let (root, minor, sure) = h.state().meter_readings().key;
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let key = format!("{} {}", names[root], if minor { "minor" } else { "major" });
    assert!(
        key == "A minor" || key == "C major",
        "{key} ({:.0} % sure)",
        sure * 100.0
    );
    assert!(sure >= 0.2, "{key}: only {:.0} % sure", sure * 100.0);
}

#[test]
fn a_melody_gives_its_notes_not_their_harmonics() {
    // C3 E3 G3 C4 G4, each 1.2 s: each heard as itself alone, low or high
    let dir = folder("meters-melody");
    let melody = [48.0, 52.0, 55.0, 60.0, 67.0];
    let mut mono = Vec::new();
    for m in melody {
        mono.extend(note(m, 1.2, 0.4));
    }
    let file = dir.join("melody.wav");
    write_wav(&file, &stereo(&mono));
    let mut h = harness(&dir, vec![file.to_string_lossy().into_owned()]);
    let mut out = RenderState::default();
    let mut seen: Vec<(f32, Vec<i32>)> = Vec::new();
    let start = Instant::now();
    let mut buf = vec![0.0f32; 480 * 2];
    // (until all of it was heard: the start may take a moment)
    while start.elapsed() < Duration::from_secs(20) && seen.last().is_none_or(|(t, _)| *t < 5.95) {
        h.state().engine().shared.render(&mut buf, 2, &mut out);
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(10));
        let r = h.state().meter_readings();
        let lit: Vec<i32> = (0..88)
            .filter(|k| r.keys[*k] > 0.45)
            .map(|k| 21 + k as i32)
            .collect();
        seen.push((r.frames as f32 / RATE as f32, lit));
    }
    for (i, m) in melody.iter().enumerate() {
        // 0.9 s into the note: the long window has nothing of the one before
        let at = i as f32 * 1.2 + 0.95;
        let (_, lit) = seen
            .iter()
            .min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))
            .unwrap();
        assert_eq!(lit, &vec![*m as i32], "at {at} s: {lit:?}, want {m}");
    }
}

#[test]
fn a_new_track_is_counted_from_its_first_sound() {
    // two tracks played gapless: the second's loudness from its first
    // frame on - a short loud burst at its start belongs to it, not to the
    // track before (counted there and thrown away with it)
    let dir = folder("meters-boundary");
    let sine = |dbfs: f32, secs: f32, hz: f32| -> Vec<f32> {
        let amp = 10f32.powf(dbfs / 20.0);
        (0..(secs * RATE as f32) as usize)
            .flat_map(|i| {
                let v = (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * amp;
                [v, v]
            })
            .collect()
    };
    let first = sine(-20.0, 2.0, 500.0);
    let mut second = sine(-6.0, 0.25, 1000.0);
    second.extend(sine(-30.0, 2.5, 1000.0));
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, &first);
    write_wav(&b, &second);
    let want = expected(&second);
    let mut h = harness(
        &dir,
        vec![
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ],
    );
    // the window at two frames a second (busy, or hidden): the track's
    // change reaches it late - the meters must not wait for it. Played
    // until the meters have read all of the second track (by the sound,
    // not the clock: six seconds of it was too few on a busy machine).
    let mut out = RenderState::default();
    let shared = h.state().engine().shared.clone();
    let mut buf = vec![0.0f32; 480 * 2];
    let total = (second.len() / 2) as u64;
    let end = Instant::now() + AT_MOST;
    let mut k = 0;
    loop {
        shared.render(&mut buf, 2, &mut out);
        if k % 50 == 0 {
            h.run_steps(1);
            let r = h.state().meter_readings();
            if (h.state().snapshot().current == 1 && r.frames + r.lost >= total)
                || Instant::now() > end
            {
                break;
            }
        }
        k += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let r = h.state().meter_readings();
    assert_eq!(r.lost, 0, "{} frames lost", r.lost);
    assert!(
        (r.integrated - want).abs() < 0.1,
        "the second track reads {:.2} LUFS, all of it {want:.2}",
        r.integrated
    );
}
