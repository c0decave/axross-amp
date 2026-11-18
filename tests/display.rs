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
    let _one = PACTL.lock().unwrap_or_else(|e| e.into_inner());
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
    wait_for(&mut h, |a| a.output_choice().ends_with("hdmi-stereo"));
    assert_eq!(
        h.state().output_choice(),
        "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
        "{}",
        h.state().notice()
    );
    h.state_mut().close_display(None);
    wait_for(&mut h, |a| a.output_choice() == before);
    assert_eq!(
        h.state().output_choice(),
        before,
        "the sound did not come back"
    );
}

/// axross's fake pactl in `dir`, as Intel HDA has it: one card, analog OR
/// HDMI - its HDMI profile available, no HDMI sink until the card is
/// switched to it (`swaps`: the switch makes the HDMI sink, as on a real
/// card; else no sink ever comes). The environment set for it.
fn hda_card(dir: &Path, swaps: bool) -> Option<PathBuf> {
    let script = std::env::var_os("AMP_FAKE_PACTL").map(PathBuf::from)?;
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let fake_dir = script.parent().unwrap().to_owned();
    let code = format!(
        "import sys, json; sys.path.insert(0, {fake_dir:?}); import fake_pactl; from pathlib import Path\n\
         p = fake_pactl.install(Path({dir:?}))\n\
         def ch(s):\n\
         \x20   s['cards'][0]['profiles']['output:hdmi-stereo']['available'] = True\n\
         fake_pactl.update_state(p['state'], ch)\n\
         print(p['state'])",
        dir = dir
    );
    let out = std::process::Command::new(&python)
        .args(["-c", &code])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let state = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let swap = dir.join("swap.py");
    std::fs::write(
        &swap,
        format!(
            "import sys\nsys.path.insert(0, {fake_dir:?}); import fake_pactl\nfrom pathlib import Path\n\
             def ch(s):\n\
             \x20   a = [x for x in s['sinks'] if 'analog' in x['name'] or 'hdmi' in x['name']]\n\
             \x20   if not a: return\n\
             \x20   x = dict(a[0])\n\
             \x20   s['sinks'] = [y for y in s['sinks'] if y not in a]\n\
             \x20   if sys.argv[1] == 'output:hdmi-stereo':\n\
             \x20       x['name'] = 'alsa_output.pci-0000_00_1f.3.hdmi-stereo'; x['description'] = 'Built-in Audio Digital Stereo (HDMI)'; x['index'] = 70\n\
             \x20   else:\n\
             \x20       x['name'] = 'alsa_output.pci-0000_00_1f.3.analog-stereo'; x['description'] = 'Built-in Audio Analog Stereo'; x['index'] = 56\n\
             \x20   s['sinks'].append(x)\n\
             fake_pactl.update_state(Path({state:?}), ch)\n"
        ),
    )
    .unwrap();
    let pactl = dir.join("pactl-hda");
    let swap_line = if swaps {
        format!(
            "case \"$*\" in *set-card-profile*) for a in \"$@\"; do last=\"$a\"; done; \"{python}\" \"{}\" \"$last\";; esac\n",
            swap.display()
        )
    } else {
        String::new()
    };
    std::fs::write(
        &pactl,
        format!(
            "#!/bin/sh\n\"{}\" \"$@\"; rc=$?\n{swap_line}exit $rc\n",
            dir.join("bin/pactl").display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&pactl, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: set before the window, read by the pactl it runs
    unsafe {
        std::env::set_var("AXROSS_AMP_PACTL", &pactl);
        std::env::set_var("FAKE_PACTL_STATE", &state);
        std::env::set_var("FAKE_PACTL_LOG", dir.join("pactl-calls.jsonl"));
    }
    Some(PathBuf::from(state))
}

/// The card's profile now, as the fake has it.
fn card_profile(state: &Path) -> String {
    let text = std::fs::read_to_string(state).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["cards"][0]["active_profile"].as_str().unwrap().to_owned()
}

fn hdmi_only() -> Vec<Monitor> {
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
            width: 1920,
            height: 1080,
            at: Some((1920, 0)),
            ..Default::default()
        },
    ]
}

/// Waits (the sound is moved on a thread of its own) until `done`.
fn wait_for(h: &mut Harness<'static, App>, done: impl Fn(&App) -> bool) -> bool {
    // (a card switched and its output waited for: 3 s and the fake
    // pactl's own time, longer on a busy machine)
    for _ in 0..800 {
        settle(h, 1);
        if done(h.state()) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

// (the tests that set AXROSS_AMP_PACTL run one at a time: the variable is
// the process's)
static PACTL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One card, analog or HDMI: the sound to the display switches the card
/// to its HDMI profile; closing the view puts the profile back as well as
/// the output - else the analog output is gone for every program. And the
/// window is never held while pactl works.
#[test]
fn the_cards_profile_comes_back_with_the_sound() {
    let _one = PACTL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = Scratch::new("display-profile");
    let Some(state) = hda_card(&dir, true) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    let mut h = window(&dir, vec![]);
    h.state_mut().set_monitors(hdmi_only());
    settle(&mut h, 2);
    let before = h.state().output_choice().to_owned();
    let started = std::time::Instant::now();
    h.state_mut().show_on("HDMI-1", true).unwrap();
    let held = started.elapsed();
    assert!(held.as_millis() < 100, "the window was held {held:?}");
    assert!(
        wait_for(&mut h, |a| a.output_choice().ends_with("hdmi-stereo")),
        "{}",
        h.state().notice()
    );
    assert_eq!(card_profile(&state), "output:hdmi-stereo");
    h.state_mut().close_display(None);
    assert!(wait_for(&mut h, |a| a.output_choice() == before));
    assert!(
        wait_for(&mut h, |_| card_profile(&state)
            == "output:analog-stereo+input:analog-stereo"),
        "the card stayed on {}",
        card_profile(&state)
    );
}

/// The card switched, but no HDMI output came: the profile is put back at
/// once (not left on HDMI with no sound anywhere), and it is said.
#[test]
fn a_failed_switch_puts_the_profile_back() {
    let _one = PACTL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = Scratch::new("display-profile-fail");
    let Some(state) = hda_card(&dir, false) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    let mut h = window(&dir, vec![]);
    h.state_mut().set_monitors(hdmi_only());
    settle(&mut h, 2);
    let started = std::time::Instant::now();
    h.state_mut().show_on("HDMI-1", true).unwrap();
    assert!(started.elapsed().as_millis() < 100, "the window was held");
    assert!(wait_for(&mut h, |a| a
        .notice()
        .contains("the sound stays here")));
    assert!(
        wait_for(&mut h, |_| card_profile(&state)
            == "output:analog-stereo+input:analog-stereo"),
        "the card stayed on {}",
        card_profile(&state)
    );
}

/// The view open with the sound when the program ends; opened again on the
/// next start - and closed, the sound and the card are back where they were
/// before the view, not on HDMI for good.
#[test]
fn the_sound_comes_back_after_a_restart() {
    let _one = PACTL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = Scratch::new("display-restart");
    let Some(state) = hda_card(&dir, true) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    let before;
    {
        let mut h = window(&dir, vec![]);
        h.state_mut().set_monitors(hdmi_only());
        settle(&mut h, 2);
        before = h.state().output_choice().to_owned();
        h.state_mut().show_on("HDMI-1", true).unwrap();
        assert!(wait_for(&mut h, |a| a
            .output_choice()
            .ends_with("hdmi-stereo")));
        for _ in 0..200 {
            settle(&mut h, 1);
            let own =
                std::fs::read_to_string(dir.join("config/axross-amp.json")).unwrap_or_default();
            if own.contains("\"display_open\": true") && own.contains("display_restore") {
                break;
            }
        }
    }
    let mut h = window(&dir, vec![]);
    h.state_mut().set_monitors(hdmi_only());
    assert!(wait_for(&mut h, |a| a.display_shown() == Some("HDMI-1")));
    assert!(wait_for(&mut h, |a| a
        .output_choice()
        .ends_with("hdmi-stereo")));
    h.state_mut().close_display(None);
    assert!(
        wait_for(&mut h, |a| a.output_choice() == before),
        "the sound stayed on {:?}",
        h.state().output_choice()
    );
    assert!(
        wait_for(&mut h, |_| card_profile(&state)
            == "output:analog-stereo+input:analog-stereo"),
        "the card stayed on {}",
        card_profile(&state)
    );
}

/// On a desktop scaled 2 x, the display's flash guard is given that screen
/// in points - as the faces are drawn - not in pixels (four times too
/// large a screen: a field four times too large, the guard four times too
/// lenient).
#[test]
fn the_guard_is_given_the_display_in_points() {
    let home = Scratch::new("display-scale");
    let setup = Setup {
        config_dir: home.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_pixels_per_point(2.0)
        .with_step_dt(0.05)
        .wgpu()
        .build_eframe(move |cc| App::with_setup(cc, vec![], setup.clone()));
    h.state_mut().set_monitors(monitors());
    settle(&mut h, 2);
    h.state_mut().show_on("HDMI-1", false).unwrap();
    settle(&mut h, 2);
    assert_eq!(h.state().guard_screen(), Some(egui::vec2(1920.0, 1080.0)));
}
