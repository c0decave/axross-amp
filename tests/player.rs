//! The window as a player: what it does as tracks end, fail and come from
//! the network - the sound taken by the test as a sound card would.

#[path = "support/player.rs"]
mod player;

use axross_amp::engine::RenderState;
use egui_kittest::kittest::Queryable;
use player::*;
use std::io::{Read, Write};

#[test]
fn an_album_plays_through_without_gaps() {
    let dir = folder("player-album");
    let files = tracks(&dir, &["1.wav", "2.wav", "3.wav"], 0.5);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let mut output = RenderState::default();
    let mut heard = Vec::new();
    let mut order = vec![h.state().snapshot().current];
    // (by the program's own progress: until the last track has ended, at
    // most 20 s - 3 s of the clock were not always 1.5 s of sound on a
    // busy machine: review 14)
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let (mut quiet, mut played) = (0, false);
    while std::time::Instant::now() < end && quiet < 5 {
        heard.extend(listen(&mut h, &mut output, 0.1));
        let current = h.state().snapshot().current;
        if order.last() != Some(&current) {
            order.push(current);
        }
        if h.state().playing() {
            (quiet, played) = (0, true);
        } else if played {
            quiet += 1;
        }
    }
    assert_eq!(order, vec![0, 1, 2], "{}", h.state().status());
    assert!(
        gaps(&heard).is_empty(),
        "silence between tracks: {:?}",
        gaps(&heard)
    );
    let sound = heard.iter().filter(|s| **s != 0.0).count();
    assert!(
        (sound as i64 - 72_000).abs() < 600,
        "1.5 s of sound wanted: {sound} frames"
    );
    assert!(!h.state().playing(), "the end of the list stops");
    assert_eq!(h.state().status(), "End of the playlist");
}

#[test]
fn a_track_that_cannot_be_played_is_marked_and_skipped() {
    let dir = folder("player-broken");
    let mut files = tracks(&dir, &["1.wav"], 0.4);
    let bad = dir.join("2.mp3");
    std::fs::write(&bad, vec![0x55u8; 4000]).unwrap();
    files.push(bad.to_string_lossy().into_owned());
    files.extend(tracks(&dir, &["3.wav"], 0.4));
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let mut output = RenderState::default();
    for _ in 0..20 {
        listen(&mut h, &mut output, 0.1);
        if h.state().snapshot().current == 2 {
            break;
        }
    }
    let s = h.state().snapshot();
    assert_eq!(s.current, 2, "{}", h.state().status());
    assert!(
        h.state().broken(&bad.to_string_lossy()).is_some(),
        "the bad track is not marked"
    );
    assert!(
        h.state().status().ends_with("skipped."),
        "{}",
        h.state().status()
    );
}

/// A WAV whose header says `rate` samples a second (0 and 1 Hz: review
/// 11's hostile files), with a second of 16-bit mono silence after it.
fn wav_at(path: &std::path::Path, rate: u32) {
    let frames = 48_000u32;
    let mut b = b"RIFF".to_vec();
    b.extend((36 + frames * 2).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((frames * 2).to_le_bytes());
    b.resize(b.len() + frames as usize * 2, 0);
    std::fs::write(path, b).unwrap();
}

/// A WAV of 0 Hz or 1 Hz: said, marked and skipped - it showed "Playing
/// 0:00" for ever (the 1 Hz one took 560 MB), the next track never came.
#[test]
fn a_track_of_no_sample_rate_is_skipped() {
    let dir = folder("player-rate");
    let mut files = tracks(&dir, &["1.wav"], 0.4);
    let (zero, one) = (dir.join("zero.wav"), dir.join("one.wav"));
    wav_at(&zero, 0);
    wav_at(&one, 1);
    files.push(zero.to_string_lossy().into_owned());
    files.push(one.to_string_lossy().into_owned());
    files.extend(tracks(&dir, &["4.wav"], 0.4));
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let mut output = RenderState::default();
    for _ in 0..40 {
        listen(&mut h, &mut output, 0.1);
        if h.state().snapshot().current == 3 {
            break;
        }
    }
    assert_eq!(h.state().snapshot().current, 3, "{}", h.state().status());
    for bad in [&zero, &one] {
        let why = h.state().broken(&bad.to_string_lossy());
        assert!(why.is_some(), "{} is not marked", bad.display());
    }
}

/// A radio on a local port: ICY titles, a station's name; how many
/// connections it had.
fn radio() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/live",
        listener.local_addr().unwrap().port()
    );
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let c = count.clone();
    std::thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                let mut socket = socket;
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request);
                let rate = 48_000u32;
                let mut out = Vec::new();
                out.extend_from_slice(
                    b"HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nicy-name: Local FM\r\nicy-metaint: 16000\r\n\r\n",
                );
                let mut body = b"RIFF\xff\xff\xff\x7fWAVEfmt ".to_vec();
                for v in [
                    16u32.to_le_bytes().as_slice(),
                    &1u16.to_le_bytes(),
                    &1u16.to_le_bytes(),
                    &rate.to_le_bytes(),
                    &(rate * 2).to_le_bytes(),
                    &2u16.to_le_bytes(),
                    &16u16.to_le_bytes(),
                ] {
                    body.extend_from_slice(v);
                }
                body.extend_from_slice(b"data\x00\x00\xff\x7f");
                let mut since = 0;
                let mut i = 0u64;
                for _ in 0..200 {
                    for _ in 0..4800 {
                        let v = ((i as f32 * 0.05).sin() * 8000.0 + 9000.0) as i16;
                        body.extend_from_slice(&v.to_le_bytes());
                        i += 1;
                    }
                    for b in body.drain(..) {
                        if since == 16000 {
                            since = 0;
                            let mut block = b"StreamTitle='Band - Tune';".to_vec();
                            block.resize(32, 0);
                            out.push(2);
                            out.extend(block);
                        }
                        out.push(b);
                        since += 1;
                    }
                    if socket.write_all(&out).is_err() {
                        return;
                    }
                    out.clear();
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
        }
    });
    (url, count)
}

#[test]
fn a_radio_shows_what_it_plays_and_stop_lets_it_go() {
    let (url, connections) = radio();
    let dir = folder("player-radio");
    let mut h = harness(&dir, vec![url.clone()]);
    let mut output = RenderState::default();
    for _ in 0..30 {
        listen(&mut h, &mut output, 0.1);
        if h.state().title().contains("Band - Tune") {
            break;
        }
    }
    assert_eq!(
        h.state().title(),
        "axross-amp – Band - Tune – Local FM",
        "{} / loaded {}",
        h.state().status(),
        h.state().is_loaded()
    );
    // the playlist names the station
    assert_eq!(h.state().snapshot().tracks[0].title, "Local FM");
    // Stop lets the stream go; Play connects again
    h.get_by_label_contains("Stop").click();
    listen(&mut h, &mut output, 0.3);
    assert!(!h.state().playing());
    h.key_press(egui::Key::Space);
    for _ in 0..30 {
        listen(&mut h, &mut output, 0.1);
        if h.state().playing() && h.state().is_loaded() {
            break;
        }
    }
    assert!(
        h.state().playing() && h.state().is_loaded(),
        "{}",
        h.state().status()
    );
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn previous_on_a_radio_goes_to_the_track_before() {
    // (a stream cannot be sought back to its start: after 3 s, Previous did nothing)
    let (url, _) = radio();
    let dir = folder("player-radio-previous");
    let mut files = tracks(&dir, &["before.wav"], 30.0);
    files.push(url);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.key_press(egui::Key::N);
    let mut output = RenderState::default();
    // (by the sound: up to 20 s for 3.2 s of it - a radio connects, fills
    // its buffer and comes at its own pace; 6 s of the clock were not
    // always enough on a busy machine - review 13)
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < end {
        listen(&mut h, &mut output, 0.1);
        if h.state().is_loaded() && h.state().engine().shared.position() > 3.2 {
            break;
        }
    }
    assert_eq!(h.state().snapshot().current, 1);
    assert!(h.state().engine().shared.position() > 3.0);
    h.key_press(egui::Key::P);
    assert!(until(&mut h, 3.0, |a| a.snapshot().current == 0));
}

/// Playing with the pictures off, the window wakes once a second (the
/// clock): it woke every half second, the whole window drawn again for
/// nothing to see - 13 % of a core on a software renderer (review 11).
#[test]
fn with_the_pictures_off_the_window_wakes_once_a_second() {
    let dir = folder("player-off");
    let files = tracks(&dir, &["1.wav"], 30.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded() && a.playing()));
    let mut output = RenderState::default();
    h.state_mut().set_mode("off");
    listen(&mut h, &mut output, 1.2);
    assert!(h.state().playing());
    assert_eq!(
        h.state().heartbeat(),
        Some(std::time::Duration::from_secs(1))
    );
    h.state_mut().set_mode("spectrum");
    listen(&mut h, &mut output, 1.2);
    assert_eq!(
        h.state().heartbeat(),
        Some(std::time::Duration::from_millis(500))
    );
}
