//! The Bluetooth tab in the window, against axross's fake BlueZ on a
//! private bus (AMP_FAKE_BLUEZ) and its fake pactl (AMP_FAKE_PACTL): power,
//! a pairing that asks for a passkey to confirm, one that asks for a PIN,
//! *Use as audio output* ending in the headset's sink as the player's
//! output, Forget, and a picture of the tab. Without the fakes the test
//! passes (CI without axross).
#![cfg(target_os = "linux")]

#[path = "support/scratch.rs"]
mod scratch;

#[path = "support/bluez.rs"]
mod bluez;

use axross_amp::app::{App, Tab};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use std::path::PathBuf;

fn steps_until(
    harness: &mut Harness<'_, App>,
    what: &str,
    done: impl Fn(&Harness<'_, App>) -> bool,
) {
    for _ in 0..400 {
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(15));
        if done(harness) {
            harness.run_steps(2); // (the window shows it: the next click lands right)
            return;
        }
    }
    panic!(
        "{what}: never (message: {:?})",
        harness.state().bluetooth_page().message
    );
}

const A2DP: &str = "0000110b-0000-1000-8000-00805f9b34fb";

#[test]
fn the_bluetooth_tab_pairs_connects_and_makes_a_headset_the_output() {
    let Some(pactl) = std::env::var_os("AMP_FAKE_PACTL").map(PathBuf::from) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    let Some(mut bus) = bluez::Bus::start() else {
        eprintln!("AMP_FAKE_BLUEZ or dbus-daemon missing: skipped");
        return;
    };
    let dir = scratch::Scratch::new("bt-ui");
    let code = format!(
        "import sys; sys.path.insert(0, {:?}); import fake_pactl; from pathlib import Path; fake_pactl.install(Path({:?}))",
        pactl.parent().unwrap(),
        &*dir
    );
    assert!(
        std::process::Command::new(&bus.python)
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
        std::env::set_var("AXROSS_AMP_BLUEZ_BUS", &bus.address);
    }
    bus.command(
        "add_adapter",
        json!({"name": "hci0", "props": {"Alias": "Laptop"}}),
    );
    // the headset of the fake pactl (its sink bluez_output.AA_BB_CC_DD_EE_01.1)
    let headset = bus.command(
        "add_device",
        json!({"address": "AA:BB:CC:DD:EE:01", "battery": 70,
               "props": {"Name": "WH-1000XM4", "Alias": "WH-1000XM4", "Icon": "audio-headphones",
                         "UUIDs": [A2DP], "RSSI": -48},
               "behaviour": {"pair": "confirm", "passkey": 4711}}),
    );
    let headset = headset.as_str().unwrap().to_owned();
    bus.command(
        "add_device",
        json!({"address": "22:00:00:00:00:01",
               "props": {"Name": "Keyboard K380", "Class": 0x000540, "Paired": true},
               "behaviour": {"pair": "pin", "pin": "0000"}}),
    );
    let speaker = bus.command(
        "add_device",
        json!({"address": "22:00:00:00:00:02",
               "props": {"Name": "Kitchen Speaker", "Icon": "audio-card", "RSSI": -71},
               "behaviour": {"pair": "pin", "pin": "1234"}}),
    );
    let speaker = speaker.as_str().unwrap().to_owned();
    bus.command(
        "add_device",
        json!({"address": "22:00:00:00:00:03", "props": {"RSSI": -90}}),
    );

    let mut harness = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .wgpu()
        // (no sound card: the engine headless, nothing for ALSA to complain about)
        .build_eframe(|cc| {
            let config = axross_amp::state::config_dir();
            App::with_setup(
                cc,
                vec![],
                axross_amp::app::Setup {
                    config_dir: config,
                    desktop: false,
                    desktop_bus: None,
                    headless_audio: true,
                    stand_in_covers: false,
                },
            )
        });
    harness.state_mut().set_tab(Tab::Bluetooth);
    steps_until(&mut harness, "the adapter", |h| {
        h.query_by_label("Powered").is_some()
    });

    // switch the adapter on
    harness.get_by_label("Powered").click();
    let view = |h: &Harness<'_, App>| h.state().bluetooth_page().client.as_ref().unwrap().view();
    steps_until(&mut harness, "powered", |h| view(h).adapters[0].powered);

    // the headset: select, Pair, confirm the passkey it shows
    harness.get_by_label("WH-1000XM4").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().bluetooth_page().selected.as_deref(),
        Some(headset.as_str())
    );
    harness.get_by_label("Pair").click();
    steps_until(&mut harness, "the question", |h| {
        h.query_by_label("Confirm").is_some()
    });
    assert!(
        harness
            .query_by_label_contains("Does WH-1000XM4 show the passkey 004711?")
            .is_some()
    );
    harness.run_steps(2);
    // (the pointer out of the picture)
    harness.event(egui::Event::PointerGone);
    harness.run_steps(1);
    let shots = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots");
    std::fs::create_dir_all(&shots).unwrap();
    harness
        .render()
        .unwrap()
        .save(shots.join("bluetooth-pairing.png"))
        .unwrap();
    harness.get_by_label("Confirm").click();
    steps_until(&mut harness, "paired", |h| {
        view(h).device(&headset).unwrap().paired
    });
    steps_until(&mut harness, "said", |h| {
        h.state().bluetooth_page().message == "Paired with WH-1000XM4."
    });

    // Use as audio output: connects, finds its sink, the player follows
    harness.get_by_label("Use as audio output").click();
    steps_until(&mut harness, "the output chosen", |h| {
        h.state().output_choice() == "pulseaudio:bluez_output.AA_BB_CC_DD_EE_01.1"
    });
    assert!(view(&harness).device(&headset).unwrap().connected);
    assert_eq!(
        harness.state().bluetooth_page().message,
        "WH-1000XM4 is now this player's output (WH-1000XM4)."
    );
    // (kept in this program's own file: axross rewrites audio.json)
    let own = std::fs::read_to_string(dir.join(".config/axross/axross-amp.json")).unwrap();
    assert!(own.contains("bluez_output.AA_BB_CC_DD_EE_01.1"), "{own}");

    // a speaker that wants a PIN: typed into the dialog
    harness.get_by_label("Kitchen Speaker").click();
    harness.run_steps(2);
    harness.get_by_label("Pair").click();
    steps_until(&mut harness, "the PIN question", |h| {
        h.query_by_label("PIN").is_some()
    });
    assert!(
        harness
            .get_by_label("Send PIN")
            .accesskit_node()
            .is_disabled(),
        "no PIN, nothing to send"
    );
    harness.get_by_label("PIN").type_text("1234");
    harness.run_steps(2);
    harness.get_by_label("Send PIN").click();
    steps_until(&mut harness, "the speaker paired", |h| {
        view(h).device(&speaker).unwrap().paired
    });

    // a double click on a paired device connects it
    assert!(!view(&harness).device(&speaker).unwrap().connected);
    steps_until(&mut harness, "the pairing over", |h| {
        view(h).busy.is_empty() && h.query_by_label("PIN").is_none()
    });
    let at = harness.get_by_label("Kitchen Speaker").rect().center();
    harness.event(egui::Event::PointerMoved(at));
    harness.run_steps(1);
    // (each click whole in a frame of its own, a step of the harness - a
    // quarter second - apart: `Harness::event` gives every event a frame,
    // so a press and its release are a step apart and the second click
    // came half a second after the first - past the 0.3 s egui allows a
    // double click, and it was counted as two single ones)
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.input_mut().events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run_steps(1);
    }
    steps_until(&mut harness, "connected by the double click", |h| {
        view(h).device(&speaker).unwrap().connected
    });
    steps_until(&mut harness, "said", |h| {
        h.state().bluetooth_page().message == "Connected to Kitchen Speaker."
    });

    // the filter: audio only hides the keyboard and the nameless one
    assert!(harness.query_by_label("Keyboard K380").is_some());
    harness.get_by_label("Audio only").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Keyboard K380").is_none());
    assert!(harness.query_by_label("22:00:00:00:00:03").is_none());
    harness.get_by_label("Audio only").click();
    // (the pointer out of the picture)
    harness.event(egui::Event::PointerGone);
    harness.run_steps(1);
    harness.run_steps(3);
    harness
        .render()
        .unwrap()
        .save(shots.join("bluetooth.png"))
        .unwrap();

    // Forget asks first; Keep keeps it, Forget removes it
    harness.get_by_label("Forget…").click();
    harness.run_steps(2);
    harness.get_by_label("Keep").click();
    harness.run_steps(3);
    assert!(view(&harness).device(&speaker).is_some());
    harness.get_by_label("Forget…").click();
    harness.run_steps(2);
    harness.get_by_label("Forget").click();
    steps_until(&mut harness, "forgotten", |h| {
        view(h).device(&speaker).is_none()
    });
    steps_until(&mut harness, "said", |h| {
        h.state().bluetooth_page().message == "Forgot Kitchen Speaker."
    });
}
