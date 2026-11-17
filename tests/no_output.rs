//! No sound device: nothing pretends to play (the position at 0:00 for
//! ever, MPRIS saying Playing, no end ever coming) - the track is loaded,
//! not played, the window says why and offers Retry; once there is an
//! output again (tried every few seconds, or Retry), Play plays.

#[path = "support/player.rs"]
mod player;

use axross_amp::engine::NO_OUTPUT;
use egui_kittest::kittest::Queryable;
use player::*;
use std::sync::atomic::Ordering;

#[test]
fn without_an_output_nothing_pretends_to_play() {
    NO_OUTPUT.store(true, Ordering::Relaxed);
    let dir = folder("no-output");
    let files = tracks(&dir, &["a.wav"], 2.0);
    let mut h = harness(&dir, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 3);
    assert!(!h.state().playing(), "playing without an output");
    assert!(
        h.query_by_label_contains("No sound").is_some(),
        "the window does not say there is no sound"
    );
    // Space: still nothing to play on
    h.key_press(egui::Key::Space);
    settle(&mut h, 3);
    assert!(!h.state().playing());
    // a device comes; Retry finds it, and Play plays
    NO_OUTPUT.store(false, Ordering::Relaxed);
    h.get_by_label("Retry").click();
    assert!(until(&mut h, 3.0, |a| !a.output().starts_with("no sound")));
    h.key_press(egui::Key::Space);
    assert!(until(&mut h, 3.0, |a| a.playing()));
    assert!(h.query_by_label_contains("No sound").is_none());
}
