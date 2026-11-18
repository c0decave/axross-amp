//! "Show on display": the pictures on a monitor of their own - chosen from
//! the menu or with D, the flash guard sized for that screen, the sound
//! moved to its HDMI output and back (against axross's fake pactl, with an
//! HDMI sink added to its state), the view closed and said when its
//! display is unplugged, and opened there again on the next start.
//!
//! The second window itself cannot be shown headless: egui_kittest runs
//! the display view's viewport embedded in the main one; what is checked
//! is everything around it (see the README).

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::display::Monitor;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use player::{Scratch, settle, until, write_chord};
use std::path::{Path, PathBuf};

fn window(dir: &Path, args: Vec<String>) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

fn monitors() -> Vec<Monitor> {
    vec![
        Monitor {
            connector: "eDP-1".into(),
            width: 1920,
            height: 1080,
            at: Some((0, 0)),
            primary: true,
            ..Default::default()
        },
        Monitor {
            connector: "HDMI-1".into(),
            name: "LG TV".into(),
            width: 3840,
            height: 2160,
            at: Some((1920, 0)),
            ..Default::default()
        },
        Monitor {
            connector: "VGA-1".into(),
            width: 1024,
            height: 768,
            at: Some((5760, 0)),
            ..Default::default()
        },
    ]
}

fn track(home: &Path) -> Vec<String> {
    let a = home.join("Chord.wav");
    write_chord(&a, 20);
    vec![a.to_string_lossy().into_owned()]
}

#[test]
fn the_pictures_go_to_a_display_and_come_back() {
    let home = Scratch::new("display-menu");
    let mut h = window(&home, track(&home));
    h.state_mut().set_monitors(monitors());
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    settle(&mut h, 2);
    // the menu lists them
    h.get_by_label("⚙").click();
    settle(&mut h, 2);
    let item = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Show on display")
        .rect();
    h.hover_at(item.center());
    settle(&mut h, 3);
    for name in ["HDMI-1 (LG TV) 3840 x 2160", "VGA-1 1024 x 768"] {
        assert!(h.query_by_label(name).is_some(), "{name} not offered");
    }
    h.get_by_label("HDMI-1 (LG TV) 3840 x 2160").click();
    settle(&mut h, 3);
    assert_eq!(h.state().display_shown(), Some("HDMI-1"));
    // its flash guard is sized for that screen
    assert_eq!(h.state().guard_screen(), Some(egui::vec2(3840.0, 2160.0)));
    // here: where they are, not the face drawn twice
    assert!(
        h.query_by_label_contains("Showing on HDMI-1").is_some(),
        "no word of where the pictures are"
    );
    // Escape (in the view: headless, egui runs it inside the main
    // window, and the menu's Escape reaches it too) brings them back
    h.key_press(egui::Key::Escape);
    settle(&mut h, 2);
    assert_eq!(h.state().display_shown(), None);
    assert_eq!(h.state().guard_screen(), None);
    // D shows them there again (the display remembered), D back
    h.key_press(egui::Key::D);
    settle(&mut h, 2);
    assert_eq!(h.state().display_shown(), Some("HDMI-1"));
    h.key_press(egui::Key::D);
    settle(&mut h, 2);
    assert_eq!(h.state().display_shown(), None);
}

#[test]
fn an_unplugged_display_closes_its_view_and_says_so() {
    let home = Scratch::new("display-unplug");
    let mut h = window(&home, track(&home));
    h.state_mut().set_monitors(monitors());
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    h.state_mut().show_on("HDMI-1", false).unwrap();
    settle(&mut h, 2);
    assert_eq!(h.state().display_shown(), Some("HDMI-1"));
    // unplugged
    let mut left = monitors();
    left.retain(|m| m.connector != "HDMI-1");
    h.state_mut().set_monitors(left);
    settle(&mut h, 2);
    assert_eq!(h.state().display_shown(), None);
    assert!(
        h.state().notice().contains("HDMI-1 was disconnected"),
        "{}",
        h.state().notice()
    );
    // plugged in again: offered again
    h.state_mut().set_monitors(monitors());
    settle(&mut h, 1);
    assert!(h.state().monitors().iter().any(|m| m.connector == "HDMI-1"));
}

#[test]
fn the_display_view_opens_again_where_it_was() {
    let home = Scratch::new("display-again");
    {
        let mut h = window(&home, vec![]);
        h.state_mut().set_monitors(monitors());
        settle(&mut h, 2);
        h.state_mut().show_on("HDMI-1", false).unwrap();
        // (written to its own file a little later)
        for _ in 0..200 {
            settle(&mut h, 1);
            let own =
                std::fs::read_to_string(home.join("config/axross-amp.json")).unwrap_or_default();
            if own.contains("\"display_open\": true") {
                break;
            }
        }
    }
    let own = std::fs::read_to_string(home.join("config/axross-amp.json")).unwrap();
    assert!(own.contains("\"display_connector\": \"HDMI-1\""), "{own}");
    let mut h = window(&home, vec![]);
    h.state_mut().set_monitors(monitors());
    settle(&mut h, 3);
    assert_eq!(h.state().display_shown(), Some("HDMI-1"));
    // a VGA display: the pictures, never the sound
    h.state_mut().show_on("VGA-1", true).unwrap();
    settle(&mut h, 1);
    assert!(
        h.state().notice().contains("the sound stays here"),
        "{}",
        h.state().notice()
    );
}

/// The sound to the display's HDMI output and back, against axross's fake
/// pactl with an HDMI sink added (without the fake: skipped).
#[test]
fn the_sound_goes_to_the_display_and_comes_back() {
    let Some(script) = std::env::var_os("AMP_FAKE_PACTL").map(PathBuf::from) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    let dir = Scratch::new("display-sound");
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let code = format!(
        "import sys, json; sys.path.insert(0, {:?}); import fake_pactl; from pathlib import Path\n\
         p = fake_pactl.install(Path({:?}))\n\
         def add(s):\n\
         \x20   hdmi = dict(s['sinks'][0]); hdmi['index'] = 70\n\
         \x20   hdmi['name'] = 'alsa_output.pci-0000_00_1f.3.hdmi-stereo'\n\
         \x20   hdmi['description'] = 'Built-in Audio Digital Stereo (HDMI)'\n\
         \x20   s['sinks'].append(hdmi)\n\
         fake_pactl.update_state(p['state'], add)",
        script.parent().unwrap(),
        &*dir
    );
    assert!(
        std::process::Command::new(&python)
            .args(["-c", &code])
            .status()
            .unwrap()
            .success()
    );
    // SAFETY: the only test of this file that sets them; set before the window
    unsafe {
        std::env::set_var("AXROSS_AMP_PACTL", dir.join("bin/pactl"));
        std::env::set_var("FAKE_PACTL_STATE", dir.join("pactl-state.json"));
        std::env::set_var("FAKE_PACTL_LOG", dir.join("pactl-calls.jsonl"));
    }
    let mut h = window(&dir, vec![]);
    h.state_mut().set_monitors(monitors());
    settle(&mut h, 2);
    let before = h.state().output_choice().to_owned();
    h.state_mut().show_on("HDMI-1", true).unwrap();
    settle(&mut h, 2);
    assert_eq!(
        h.state().output_choice(),
        "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
        "{}",
        h.state().notice()
    );
    h.state_mut().close_display(None);
    settle(&mut h, 2);
    assert_eq!(
        h.state().output_choice(),
        before,
        "the sound did not come back"
    );
}
