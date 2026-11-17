//! The system mixer against axross's fake pactl (`tests/fake_pactl.py` of
//! axross: pactl 16's JSON, a state file, a log of every call, knobs for a
//! dead server and failures). AMP_FAKE_PACTL names that script; without it
//! the test passes (CI without axross).

use axross_amp::mixer::{Action, Kind, Mixer, Status};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn install(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let script = PathBuf::from(std::env::var_os("AMP_FAKE_PACTL")?);
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let code = format!(
        "import sys; sys.path.insert(0, {:?}); import fake_pactl; from pathlib import Path; fake_pactl.install(Path({:?}))",
        script.parent().unwrap(),
        dir
    );
    let status = std::process::Command::new(&python)
        .args(["-c", &code])
        .status()
        .ok()?;
    assert!(status.success());
    Some((dir.join("pactl-state.json"), dir.join("pactl-calls.jsonl")))
}

fn wait_for(
    mixer: &Mixer,
    what: &str,
    done: impl Fn(&axross_amp::mixer::MixerView) -> bool,
) -> axross_amp::mixer::MixerView {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let view = mixer.view();
        if done(&view) {
            return view;
        }
        assert!(Instant::now() < end, "{what}: {view:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn edit_state(path: &Path, f: impl FnOnce(&mut Value)) {
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    f(&mut state);
    std::fs::write(path, serde_json::to_string(&state).unwrap()).unwrap();
}

fn calls(log: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            let args = v
                .get("argv")
                .or_else(|| v.get("args"))
                .cloned()
                .unwrap_or(v);
            serde_json::from_value::<Vec<String>>(args).ok()
        })
        .collect()
}

#[test]
fn lists_commands_live_updates_and_failures_against_the_fake_pactl() {
    let dir = std::env::temp_dir().join(format!("amp-mixer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some((state, log)) = install(&dir) else {
        eprintln!("AMP_FAKE_PACTL not set: skipped");
        return;
    };
    // SAFETY: the only test of this binary; set before the mixer runs pactl
    unsafe {
        std::env::set_var("AXROSS_AMP_PACTL", dir.join("bin/pactl"));
        std::env::set_var("FAKE_PACTL_STATE", &state);
        std::env::set_var("FAKE_PACTL_LOG", &log);
    }

    // the lists: devices with their cards, the defaults, monitors marked, streams, profiles
    let mixer = Mixer::start(|| {});
    let view = wait_for(&mixer, "ready", |v| v.status == Status::Ready);
    let s = &view.snapshot;
    let builtin = s.sinks.iter().find(|d| d.index == 56).unwrap();
    assert!(builtin.is_default && builtin.label() == "Built-in Audio Analog Stereo");
    assert_eq!(builtin.channels.len(), 2);
    assert_eq!(builtin.volume_percent(), 60.0);
    assert_eq!(builtin.card_index, Some(41));
    assert!(builtin.ports.len() >= 2 && builtin.ports.iter().any(|p| p.available == Some(false)));
    let headset = s.sinks.iter().find(|d| d.index == 80).unwrap();
    assert_eq!(headset.card_index, Some(77));
    assert_eq!(headset.bus, "bluetooth");
    assert!(s.sources.iter().filter(|d| d.is_monitor).count() == 2);
    assert!(
        s.sources
            .iter()
            .any(|d| d.index == 58 && d.is_default && !d.is_monitor)
    );
    let firefox = s.sink_inputs.iter().find(|st| st.index == 90).unwrap();
    assert_eq!(
        (firefox.application.as_str(), firefox.device_index),
        ("Firefox", Some(56))
    );
    assert_eq!(s.source_outputs.len(), 1);
    let bt = s.cards.iter().find(|c| c.index == 77).unwrap();
    assert_eq!(bt.active_profile, "a2dp-sink");
    assert_eq!(bt.profiles[0].name, "a2dp-sink"); // by priority
    let hdmi = s
        .cards
        .iter()
        .find(|c| c.index == 41)
        .unwrap()
        .profiles
        .iter()
        .find(|p| p.name == "output:hdmi-stereo")
        .unwrap()
        .clone();
    assert!(!hdmi.available);

    // every command: the arguments pactl got, and the lists that follow
    let before = view.refreshes;
    mixer.act(Action::DeviceVolume {
        kind: Kind::Sink,
        name: builtin.name.clone(),
        channels: builtin.channels.clone(),
        percent: 100.0,
    });
    let view = wait_for(&mixer, "volume", |v| {
        v.refreshes > before
            && v.snapshot
                .sinks
                .iter()
                .any(|d| d.index == 56 && d.volume_percent() == 100.0)
    });
    let volume = view.snapshot.sinks.iter().find(|d| d.index == 56).unwrap();
    assert_eq!(volume.channels[1].percent, 50.0, "the balance is kept");

    // a slider dragged: thirty volumes a moment, pactl sets the newest few
    let sets = |log: &Path| {
        calls(log)
            .iter()
            .filter(|c| c.get(1).map(String::as_str) == Some("set-sink-volume"))
            .count()
    };
    let set_before = sets(&log);
    let before = mixer.view().refreshes;
    for step in 0..30 {
        mixer.act(Action::DeviceVolume {
            kind: Kind::Sink,
            name: builtin.name.clone(),
            channels: volume.channels.clone(),
            percent: 70.0 - step as f64,
        });
    }
    wait_for(&mixer, "the dragged volume", |v| {
        v.refreshes > before
            && v.snapshot
                .sinks
                .iter()
                .any(|d| d.index == 56 && d.volume_percent() == 41.0)
    });
    let made = sets(&log) - set_before;
    assert!((1..=5).contains(&made), "{made} pactl calls for 30 volumes");
    for action in [
        Action::DeviceMute {
            kind: Kind::Sink,
            name: headset.name.clone(),
            muted: true,
        },
        Action::Default {
            kind: Kind::Sink,
            name: headset.name.clone(),
        },
        Action::Move {
            kind: Kind::SinkInput,
            index: 90,
            device: headset.name.clone(),
        },
        Action::StreamVolume {
            kind: Kind::SinkInput,
            index: 90,
            channels: firefox.channels.clone(),
            percent: 35.0,
        },
        Action::StreamMute {
            kind: Kind::SinkInput,
            index: 90,
            muted: true,
        },
        Action::Profile {
            card: bt.name.clone(),
            profile: "headset-head-unit".into(),
        },
    ] {
        mixer.act(action);
    }
    let view = wait_for(&mixer, "all applied", |v| {
        let s = &v.snapshot;
        s.default_sink == headset.name
            && s.sinks.iter().any(|d| d.index == 80 && d.muted)
            && s.sink_inputs.iter().any(|st| {
                st.index == 90
                    && st.device_index == Some(80)
                    && st.muted
                    && (st.volume_percent() - 35.0).abs() < 0.6
            })
            && s.cards
                .iter()
                .any(|c| c.index == 77 && c.active_profile == "headset-head-unit")
    });
    assert!(view.error.is_empty(), "{}", view.error);
    let made: Vec<Vec<String>> = calls(&log)
        .into_iter()
        .filter(|c| c.first().map(String::as_str) == Some("--"))
        .collect();
    assert!(
        made.iter().any(|c| c[1] == "set-sink-volume"
            && c[2] == builtin.name
            && c[3] == "65536"
            && c[4] == "32768"),
        "{made:?}"
    );
    assert!(
        made.iter().all(|c| c[0] == "--"),
        "every command puts -- first"
    );

    // a change made elsewhere arrives through pactl subscribe
    let before = mixer.view().refreshes;
    let fake = std::path::Path::new(&std::env::var_os("AMP_FAKE_PACTL").unwrap()).to_path_buf();
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let status = std::process::Command::new(&python)
        .arg(&fake)
        .args(["--", "set-sink-mute", &builtin.name, "1"])
        .status()
        .unwrap();
    assert!(status.success());
    wait_for(&mixer, "the outside change", |v| {
        v.refreshes > before && v.snapshot.sinks.iter().any(|d| d.index == 56 && d.muted)
    });

    // a command the server refuses: a sentence, the lists unchanged
    edit_state(&state, |s| {
        s["fail"] = serde_json::json!({"set-sink-mute": "Failure: No such entity"})
    });
    mixer.act(Action::DeviceMute {
        kind: Kind::Sink,
        name: builtin.name.clone(),
        muted: false,
    });
    let view = wait_for(&mixer, "the refusal", |v| !v.error.is_empty());
    assert_eq!(
        view.error,
        format!(
            "Could not mute {}: it does not exist (any more).",
            builtin.name
        )
    );
    drop(mixer);

    // the server gone: said, not a crash
    edit_state(&state, |s| s["server_down"] = Value::Bool(true));
    let mixer = Mixer::start(|| {});
    let view = wait_for(&mixer, "unavailable", |v| {
        matches!(v.status, Status::Unavailable(_))
    });
    assert!(
        matches!(&view.status, Status::Unavailable(why) if why.contains("No sound server")),
        "{view:?}"
    );
    drop(mixer);

    // a pactl too old for JSON
    edit_state(&state, |s| {
        s["server_down"] = Value::Bool(false);
        s["version"] = Value::String("15.0".into());
    });
    let mixer = Mixer::start(|| {});
    let view = wait_for(&mixer, "too old", |v| {
        matches!(v.status, Status::Unavailable(_))
    });
    assert!(
        matches!(&view.status, Status::Unavailable(why) if why.contains("too old")),
        "{view:?}"
    );

    // no pactl at all
    drop(mixer);
    // SAFETY: as above
    unsafe { std::env::set_var("AXROSS_AMP_PACTL", dir.join("no-such-pactl")) };
    let mixer = Mixer::start(|| {});
    let view = wait_for(&mixer, "missing", |v| {
        matches!(v.status, Status::Unavailable(_))
    });
    assert!(
        matches!(&view.status, Status::Unavailable(why) if why.contains("pactl is not installed")),
        "{view:?}"
    );
}
