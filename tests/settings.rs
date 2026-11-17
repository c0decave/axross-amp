//! What a long evening needs: the sleep timer (a fade, then a pause; or the
//! end of the track), the track's details, and what this program keeps of
//! its own (the window's size).

#[path = "support/player.rs"]
mod player;

use axross_amp::app::Sleep;
use axross_amp::engine::RenderState;
use egui_kittest::kittest::Queryable;
use player::*;
use std::time::{Duration, Instant};

#[test]
fn the_sleep_timer_fades_out_and_pauses() {
    let dir = folder("sleep-fade");
    let files = tracks(&dir, &["long.wav"], 30.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded() && a.playing()));
    let full = h.state().engine().shared.volume();
    h.state_mut()
        .set_sleep(Some(Sleep::At(Instant::now() + Duration::from_secs(3))));
    settle(&mut h, 2);
    assert!(
        h.get_by_label_contains("Sleep: 1 min").rect().width() > 0.0,
        "the timer is shown"
    );
    // within the last 8 s: fading
    std::thread::sleep(Duration::from_millis(1500));
    settle(&mut h, 2);
    let fading = h.state().engine().shared.volume();
    assert!(fading < full * 0.3 && fading > 0.0, "{fading} of {full}");
    assert!(until(&mut h, 3.0, |a| !a.playing()), "it did not pause");
    assert_eq!(h.state().status(), "The sleep timer paused the music.");
    // the volume is back for the morning
    assert_eq!(h.state().engine().shared.volume(), full);
    assert!(h.query_by_label_contains("Sleep:").is_none());
}

#[test]
fn the_sleep_timer_stops_at_the_end_of_the_track() {
    let dir = folder("sleep-end");
    let files = tracks(&dir, &["a.wav", "b.wav"], 0.5);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.get_by_label("More").click();
    settle(&mut h, 2);
    h.get_by_label("Sleep timer").click();
    settle(&mut h, 2);
    h.get_by_label("At the end of the track").click();
    settle(&mut h, 2);
    let mut output = RenderState::default();
    let mut heard = Vec::new();
    for _ in 0..15 {
        heard.extend(listen(&mut h, &mut output, 0.1));
    }
    assert!(!h.state().playing());
    // the first track whole, then nothing of the next (it was already
    // decoded behind it: a few ms of it may fade out)
    let sound = heard.iter().filter(|s| **s != 0.0).count();
    assert!((23_500..=25_500).contains(&sound), "{sound} frames heard");
    assert!(h.state().engine().shared.position() < 0.01);
    assert_eq!(
        h.state().status(),
        "The sleep timer stopped the music at the end of the track."
    );
}

#[test]
fn a_tracks_details_and_the_window_size_kept() {
    let dir = folder("details");
    let files = tracks(&dir, &["a.wav", "b.wav"], 1.0);
    let mut h = harness_sized(&dir, files, egui::vec2(700.0, 900.0));
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    // the details of the track not playing: read on a thread
    h.get_by_label("b").click_secondary();
    settle(&mut h, 2);
    h.get_by_label("ℹ Details…").click();
    let mut shown = false;
    for _ in 0..100 {
        settle(&mut h, 1);
        if h.query_by_label_contains("22050 Hz, 1 channel, 16 bit")
            .is_some()
        {
            shown = true;
            break;
        }
    }
    assert!(shown, "no format shown");
    assert!(h.query_by_label_contains("b.wav").is_some());
    drop(h);
    let kept = axross_amp::state::read_amp(&dir.join("config/axross-amp.json"));
    assert_eq!(kept.window, Some([700.0, 900.0]));
}

#[test]
fn an_equalizer_slider_dragged_is_one_write_off_the_window() {
    // (each frame of a drag wrote, fsynced and renamed audio.json on the
    // window's thread)
    let dir = folder("eq-writes");
    let files = tracks(&dir, &["a.wav"], 1.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    // (the state written after the start is behind us: what is written now is the equalizer)
    std::thread::sleep(Duration::from_millis(3200));
    settle(&mut h, 5);
    std::thread::sleep(Duration::from_millis(200));
    let before = h.state().files_written();
    for step in 0..20 {
        h.get_by_label("Equalizer 1k")
            .set_value_accesskit(step as f64 * 0.5);
        h.run_steps(1);
    }
    let during = h.state().files_written() - before;
    assert_eq!(during, 0, "written while the slider moved");
    std::thread::sleep(Duration::from_millis(300));
    settle(&mut h, 5);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(h.state().files_written() - before, 1);
    let audio = std::fs::read_to_string(dir.join("config/audio.json")).unwrap();
    assert!(audio.contains("9.5"), "{audio}");
}
