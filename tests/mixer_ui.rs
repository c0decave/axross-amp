//! The Mixer tab in the window, against axross's fake pactl
//! (AMP_FAKE_PACTL; without it the test passes): the lists show, a click
//! reaches pactl, and a picture of the tab is saved.

use axross_amp::app::{App, Tab};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::path::PathBuf;

#[test]
fn the_mixer_tab_shows_the_sound_server_and_acts() {
    let Some(script) = std::env::var_os("AMP_FAKE_PACTL").map(PathBuf::from) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("amp-mixer-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let code = format!(
        "import sys; sys.path.insert(0, {:?}); import fake_pactl; from pathlib import Path; fake_pactl.install(Path({:?}))",
        script.parent().unwrap(),
        dir
    );
    assert!(
        std::process::Command::new(&python)
            .args(["-c", &code])
            .status()
            .unwrap()
            .success()
    );
    // SAFETY: the only test of this binary
    unsafe {
        std::env::set_var("HOME", &dir);
        std::env::set_var("AXROSS_AMP_PACTL", dir.join("bin/pactl"));
        std::env::set_var("FAKE_PACTL_STATE", dir.join("pactl-state.json"));
        std::env::set_var("FAKE_PACTL_LOG", dir.join("pactl-calls.jsonl"));
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 1100.0))
        .wgpu()
        .build_eframe(|cc| App::new(cc, vec![]));
    harness.state_mut().set_tab(Tab::Mixer);
    let mut ready = false;
    for _ in 0..300 {
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        if harness
            .query_by_label("Volume of output Built-in Audio Analog Stereo")
            .is_some()
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "the outputs never showed");
    assert!(
        harness
            .query_by_label("Volume of output WH-1000XM4")
            .is_some()
    );
    assert!(harness.query_by_label("Device of Firefox").is_some());
    assert!(harness.query_by_label("Profile of WH-1000XM4").is_some());
    harness.run_steps(3);
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots");
    std::fs::create_dir_all(&shots).unwrap();
    harness
        .render()
        .unwrap()
        .save(shots.join("mixer.png"))
        .unwrap();

    // the headset's mute button and Make default reach pactl
    harness.get_by_label("Mute output WH-1000XM4").click();
    harness
        .get_all_by_label("Make default")
        .next()
        .unwrap()
        .click();
    let log = dir.join("pactl-calls.jsonl");
    let mut calls = String::new();
    for _ in 0..200 {
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        calls = std::fs::read_to_string(&log).unwrap_or_default();
        if calls.contains("set-sink-mute") && calls.contains("set-default-sink") {
            break;
        }
    }
    assert!(
        calls.contains("\"set-sink-mute\", \"bluez_output.AA_BB_CC_DD_EE_01.1\", \"1\""),
        "{calls}"
    );
    assert!(calls.contains("set-default-sink"), "{calls}");

    // a volume slider sets the volume while it is dragged, not only when let go
    let rect = harness
        .get_by_label("Volume of output Built-in Audio Analog Stereo")
        .rect();
    let at = |x: f32| egui::pos2(rect.left() + rect.width() * x, rect.center().y);
    harness.hover_at(at(0.2));
    harness.run_steps(1);
    harness.event(egui::Event::PointerButton {
        pos: at(0.2),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.run_steps(1);
    let before = calls.matches("set-sink-volume").count();
    let mut live = false;
    for step in 0..100 {
        harness.event(egui::Event::PointerMoved(at(0.2 + 0.004 * step as f32)));
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let now = std::fs::read_to_string(&log).unwrap_or_default();
        if now.matches("set-sink-volume").count() > before {
            live = true;
            break;
        }
    }
    assert!(live, "no volume set while the slider was held");
    harness.event(egui::Event::PointerButton {
        pos: at(0.6),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    harness.run_steps(1);
}
