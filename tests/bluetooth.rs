//! Bluetooth against axross's fake BlueZ (`tests/fake_bluez.py` of axross:
//! org.bluez modelled on BlueZ 5 - adapters, devices, pairing with every
//! kind of agent question, connect, battery, signatures checked as BlueZ
//! checks them) on a private D-Bus of its own. AMP_FAKE_BLUEZ names that
//! script (AMP_PYTHON the Python with jeepney); without it, or without
//! dbus-daemon, the test passes (CI without axross).
#![cfg(target_os = "linux")]

#[path = "support/bluez.rs"]
mod bluez;
use bluez::Bus;

use axross_amp::bluetooth::{
    Action, AgentKind, Answer, Bluetooth, BluetoothView, Finished, Op, Status, Transport,
};
use serde_json::json;
use std::time::{Duration, Instant};

fn wait_for(
    bt: &Bluetooth,
    what: &str,
    mut done: impl FnMut(&BluetoothView) -> bool,
) -> BluetoothView {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let view = bt.view();
        if done(&view) {
            return view;
        }
        assert!(Instant::now() < end, "{what}: {view:#?}");
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// The result of `op` on `path` that came after `after` (a serial).
fn result(bt: &Bluetooth, path: &str, op: Op, after: u64) -> Finished {
    let view = wait_for(bt, &format!("{op:?} of {path}"), |v| {
        v.finished
            .iter()
            .any(|f| f.serial > after && f.path == path && f.op == op)
    });
    view.finished
        .into_iter()
        .find(|f| f.serial > after && f.path == path && f.op == op)
        .unwrap()
}

fn last_serial(bt: &Bluetooth) -> u64 {
    bt.view().finished.last().map(|f| f.serial).unwrap_or(0)
}

/// Wait until BlueZ's signal about `path` came.
fn known(bt: &Bluetooth, path: &str) {
    wait_for(bt, &format!("{path} known"), |v| {
        v.device(path).is_some() || v.adapter(path).is_some()
    });
}

/// Act and wait for the result.
fn act(bt: &Bluetooth, action: Action, path: &str, op: Op) -> Finished {
    known(bt, path);
    let after = last_serial(bt);
    bt.act(action);
    result(bt, path, op, after)
}

const A2DP: &str = "0000110b-0000-1000-8000-00805f9b34fb";

#[test]
fn power_scan_pair_every_question_connect_and_forget_against_the_fake_bluez() {
    let Some(mut bus) = Bus::start() else {
        eprintln!("AMP_FAKE_BLUEZ or dbus-daemon missing: skipped");
        return;
    };
    // SAFETY: the only test of this binary; set before the client connects
    unsafe { std::env::set_var("AXROSS_AMP_BLUEZ_BUS", &bus.address) };

    // BlueZ without an adapter, then with one
    let bt = Bluetooth::start(|| {});
    wait_for(&bt, "no adapter", |v| v.status == Status::NoAdapter);
    let hci0 = bus.command("add_adapter", json!({"name": "hci0"}));
    let hci0 = hci0.as_str().unwrap().to_owned();
    let view = wait_for(&bt, "ready", |v| v.status == Status::Ready);
    assert!(!view.adapters[0].powered);

    // switched off: an operation says so
    let headphones = bus.command(
        "add_device",
        json!({"address": "AA:BB:CC:DD:EE:01", "battery": 80,
               "props": {"Name": "WH-1000XM4", "Alias": "WH-1000XM4", "Icon": "audio-headphones",
                         "Class": 0x240418, "UUIDs": [A2DP]},
               "behaviour": {"pair": "confirm", "passkey": 123456}}),
    );
    let headphones = headphones.as_str().unwrap().to_owned();
    wait_for(&bt, "the headphones", |v| v.device(&headphones).is_some());
    let r = act(
        &bt,
        Action::Connect {
            device: headphones.clone(),
        },
        &headphones,
        Op::Connect,
    );
    assert_eq!(
        r.error,
        "The Bluetooth adapter is not ready - switch the adapter on first."
    );
    let r = act(
        &bt,
        Action::Power {
            adapter: hci0.clone(),
            on: true,
        },
        &hci0,
        Op::Power,
    );
    assert_eq!(r.error, "");
    wait_for(&bt, "powered", |v| v.adapters[0].powered);

    // the device as shown: kind, battery, audio, name
    let view = wait_for(&bt, "the headphones", |v| v.device(&headphones).is_some());
    let d = view.device(&headphones).unwrap();
    assert_eq!(
        (d.kind(), d.battery, d.is_audio_output()),
        ("Headphones", Some(80), true)
    );
    assert_eq!(d.display_name(), "WH-1000XM4");

    // a scan: what only a scan finds appears; the filter as BlueZ wants it
    let hidden = bus.command(
        "add_device",
        json!({"address": "11:22:33:44:55:66", "on_scan": true,
               "props": {"Name": "Speaker", "Icon": "audio-card", "RSSI": -60}}),
    );
    let hidden = hidden.as_str().unwrap().to_owned();
    assert!(bt.view().device(&hidden).is_none());
    let r = act(
        &bt,
        Action::Scan {
            adapter: hci0.clone(),
            transport: Transport::BrEdr,
        },
        &hci0,
        Op::Scan,
    );
    assert_eq!(r.error, "");
    let view = wait_for(&bt, "found by the scan", |v| v.device(&hidden).is_some());
    assert!(view.scans.contains(&hci0));
    assert_eq!(view.device(&hidden).unwrap().rssi, Some(-60));
    let filters = bus.command("filters", json!({}));
    assert!(
        filters
            .as_object()
            .unwrap()
            .values()
            .any(|f| f["Transport"] == "bredr"),
        "{filters}"
    );
    let r = act(
        &bt,
        Action::StopScan {
            adapter: hci0.clone(),
        },
        &hci0,
        Op::StopScan,
    );
    assert_eq!(r.error, "");
    assert!(bt.view().scans.is_empty());

    // pairing that asks: confirm the passkey; the agent is there only meanwhile
    let after = last_serial(&bt);
    bt.act(Action::Pair {
        device: headphones.clone(),
    });
    let view = wait_for(&bt, "the question", |v| !v.requests.is_empty());
    let q = &view.requests[0];
    assert_eq!(
        (q.kind, q.passkey, q.device_name.as_str()),
        (AgentKind::Confirm, Some(123456), "WH-1000XM4")
    );
    let agents = bus.command("agents", json!({}));
    assert!(agents.to_string().contains(bt.agent_path()), "{agents}");
    assert!(agents.to_string().contains("KeyboardDisplay"));
    bt.answer(q.token, Answer::Accept(String::new()));
    let r = result(&bt, &headphones, Op::Pair, after);
    assert_eq!(r.error, "");
    let view = wait_for(&bt, "paired", |v| v.device(&headphones).unwrap().paired);
    assert!(view.requests.is_empty());
    wait_for(&bt, "the agent taken back", |_| {
        bus_agents_empty(&bt, &mut bus)
    });

    // a PIN: the right one pairs, a wrong one says so
    let keyboard = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:01",
            "props": {"Name": "Keyboard", "Class": 0x000540},
            "behaviour": {"pair": "pin", "pin": "4321"}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        pair_answering(
            &bt,
            &keyboard,
            AgentKind::Pin,
            Answer::Accept("4321".into())
        ),
        ""
    );
    let other = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:02",
            "props": {"Name": "Keyboard Two"}, "behaviour": {"pair": "pin", "pin": "4321"}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        pair_answering(&bt, &other, AgentKind::Pin, Answer::Accept("0000".into())),
        "Pairing failed: the PIN or passkey did not match, or the device refused."
    );
    // a passkey (sent as uint32, as BlueZ checks), a rejection, an authorization
    let watch = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:03",
            "props": {"Name": "Watch"}, "behaviour": {"pair": "passkey", "passkey": 654321}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        pair_answering(
            &bt,
            &watch,
            AgentKind::Passkey,
            Answer::Accept("654321".into())
        ),
        ""
    );
    let phone = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:04",
            "props": {"Name": "Phone"}, "behaviour": {"pair": "confirm", "passkey": 1}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        pair_answering(&bt, &phone, AgentKind::Confirm, Answer::Reject),
        "Pairing was rejected."
    );
    let tablet = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:05",
            "props": {"Name": "Tablet"}, "behaviour": {"pair": "authorize"}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        pair_answering(
            &bt,
            &tablet,
            AgentKind::Authorize,
            Answer::Accept(String::new())
        ),
        ""
    );

    // a code to type on the device: shown while the pairing runs, gone after
    let pad = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:06",
            "props": {"Name": "Pad"},
            "behaviour": {"pair": "display-passkey", "passkey": 4711, "delay": 0.6}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    known(&bt, &pad);
    let after = last_serial(&bt);
    bt.act(Action::Pair {
        device: pad.clone(),
    });
    let view = wait_for(&bt, "the code", |v| !v.requests.is_empty());
    assert_eq!(
        (
            view.requests[0].kind,
            view.requests[0].passkey,
            view.requests[0].needs_answer()
        ),
        (AgentKind::DisplayPasskey, Some(4711), false)
    );
    assert_eq!(result(&bt, &pad, Op::Pair, after).error, "");
    assert!(bt.view().requests.is_empty());

    // nobody answers: BlueZ gives up and takes its question back
    let slow = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:07",
            "props": {"Name": "Slow"},
            "behaviour": {"pair": "confirm", "passkey": 2, "agent_timeout": 0.5}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    known(&bt, &slow);
    let after = last_serial(&bt);
    bt.act(Action::Pair {
        device: slow.clone(),
    });
    wait_for(&bt, "the question", |v| !v.requests.is_empty());
    assert_eq!(
        result(&bt, &slow, Op::Pair, after).error,
        "Pairing timed out - the device or the answer did not come in time."
    );
    wait_for(&bt, "the question taken back", |v| v.requests.is_empty());

    // a pairing canceled by the user
    let stuck = bus
        .command(
            "add_device",
            json!({"address": "22:00:00:00:00:08",
            "props": {"Name": "Stuck"}, "behaviour": {"pair": "hang"}}),
        )
        .as_str()
        .unwrap()
        .to_owned();
    known(&bt, &stuck);
    let after = last_serial(&bt);
    bt.act(Action::Pair {
        device: stuck.clone(),
    });
    wait_for(&bt, "pairing", |v| v.busy.get(&stuck) == Some(&Op::Pair));
    std::thread::sleep(Duration::from_millis(200)); // (the Pair is out)
    bt.act(Action::CancelPairing {
        device: stuck.clone(),
    });
    assert_eq!(
        result(&bt, &stuck, Op::Pair, after).error,
        "Pairing was canceled."
    );
    assert_eq!(result(&bt, &stuck, Op::CancelPairing, after).error, "");

    // trust, connect, one operation at a time, disconnect, a failure in words
    let r = act(
        &bt,
        Action::Trust {
            device: headphones.clone(),
            on: true,
        },
        &headphones,
        Op::Trust,
    );
    assert_eq!(r.error, "");
    wait_for(&bt, "trusted", |v| v.device(&headphones).unwrap().trusted);
    bus.command(
        "behave",
        json!({"path": headphones, "behaviour": {"delay": 0.4}}),
    );
    let after = last_serial(&bt);
    bt.act(Action::Connect {
        device: headphones.clone(),
    });
    bt.act(Action::Disconnect {
        device: headphones.clone(),
    });
    let refused = result(&bt, &headphones, Op::Disconnect, after);
    assert_eq!(
        refused.error,
        "Another operation on this device is still running - wait for it to finish."
    );
    assert_eq!(result(&bt, &headphones, Op::Connect, after).error, "");
    let view = wait_for(&bt, "connected", |v| {
        v.device(&headphones).unwrap().connected
    });
    assert_eq!(view.devices[0].path, headphones, "connected devices first");
    let r = act(
        &bt,
        Action::Disconnect {
            device: headphones.clone(),
        },
        &headphones,
        Op::Disconnect,
    );
    assert_eq!(r.error, "");
    wait_for(&bt, "disconnected", |v| {
        !v.device(&headphones).unwrap().connected
    });
    bus.command(
        "behave",
        json!({"path": hidden, "behaviour": {"connect": "error:org.bluez.Error.Failed:br-connection-page-timeout"}}),
    );
    let r = act(
        &bt,
        Action::Connect {
            device: hidden.clone(),
        },
        &hidden,
        Op::Connect,
    );
    assert_eq!(
        r.error,
        "The device did not answer - is it switched on and in range?"
    );

    // the battery changes and goes
    bus.command("battery", json!({"path": headphones, "percentage": 55}));
    wait_for(&bt, "battery 55", |v| {
        v.device(&headphones).unwrap().battery == Some(55)
    });
    bus.command("battery", json!({"path": headphones, "percentage": null}));
    wait_for(&bt, "no battery", |v| {
        v.device(&headphones).unwrap().battery.is_none()
    });

    // a name that would forge the text around it
    let evil = bus
        .command("add_device", json!({"address": "22:00:00:00:00:09",
            "props": {"Name": "Evil\nPaired \u{202e}3pm.exe", "Alias": "Evil\nPaired \u{202e}3pm.exe"}}))
        .as_str()
        .unwrap()
        .to_owned();
    let view = wait_for(&bt, "the evil one", |v| v.device(&evil).is_some());
    assert_eq!(
        view.device(&evil).unwrap().display_name(),
        "Evil Paired 3pm.exe"
    );

    // another program on the bus cannot speak for BlueZ: forged signals and
    // a forged question are refused
    let forger = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .unwrap();
    let mut changed = std::collections::HashMap::new();
    changed.insert("Connected", zbus::zvariant::Value::from(true));
    forger
        .emit_signal(
            None::<&str>,
            evil.as_str(),
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &("org.bluez.Device1", changed, Vec::<String>::new()),
        )
        .unwrap();
    let unique = bt_unique_name(&mut bus, &bt);
    let forged = forger.call_method(
        Some(unique.as_str()),
        bt.agent_path(),
        Some("org.bluez.Agent1"),
        "RequestConfirmation",
        &(
            zbus::zvariant::ObjectPath::try_from(evil.as_str()).unwrap(),
            1u32,
        ),
    );
    match forged {
        Err(zbus::Error::MethodError(name, _, _)) => {
            assert_eq!(name.as_str(), "org.bluez.Error.Rejected")
        }
        other => panic!("a forged question was not refused: {other:?}"),
    }
    std::thread::sleep(Duration::from_millis(300));
    let view = bt.view();
    assert!(
        !view.device(&evil).unwrap().connected,
        "a forged signal counted"
    );
    assert!(view.requests.is_empty(), "a forged question was asked");

    // forget a device
    let r = act(
        &bt,
        Action::Remove {
            device: evil.clone(),
        },
        &evil,
        Op::Remove,
    );
    assert_eq!(r.error, "");
    wait_for(&bt, "removed", |v| v.device(&evil).is_none());

    // bluetoothd stops and comes back
    bus.kill_fake();
    wait_for(&bt, "not running", |v| {
        v.status == Status::NotRunning && v.devices.is_empty()
    });
    let after = last_serial(&bt);
    bt.act(Action::Power {
        adapter: hci0.clone(),
        on: false,
    });
    let r = result(&bt, &hci0, Op::Power, after);
    assert!(
        r.error.contains("bluetoothd) is not running"),
        "{}",
        r.error
    );
    bus.control = None;
    bus.start_fake();
    wait_for(&bt, "back", |v| v.status == Status::NoAdapter);
    bus.command(
        "add_adapter",
        json!({"name": "hci0", "props": {"Powered": true}}),
    );
    wait_for(&bt, "ready again", |v| {
        v.status == Status::Ready && v.adapters[0].powered
    });
    drop(bt);
}

fn bus_agents_empty(_bt: &Bluetooth, bus: &mut Bus) -> bool {
    bus.command("agents", json!({}))
        .as_object()
        .unwrap()
        .is_empty()
}

/// Our unique name on the bus: the sender of the RegisterAgent the fake saw.
fn bt_unique_name(bus: &mut Bus, bt: &Bluetooth) -> String {
    let calls = bus.command("calls", json!({}));
    calls
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["member"] == "RegisterAgent" && c.to_string().contains(bt.agent_path()))
        .and_then(|c| c["sender"].as_str())
        .unwrap_or_else(|| panic!("no RegisterAgent in {calls}"))
        .to_owned()
}

/// Pair `device`, answer its question of `kind` with `answer`: the error.
fn pair_answering(bt: &Bluetooth, device: &str, kind: AgentKind, answer: Answer) -> String {
    known(bt, device);
    let after = last_serial(bt);
    bt.act(Action::Pair {
        device: device.to_owned(),
    });
    let view = wait_for(bt, "the question", |v| {
        v.requests.iter().any(|q| q.device == device)
    });
    let q = view.requests.iter().find(|q| q.device == device).unwrap();
    assert_eq!(q.kind, kind);
    bt.answer(q.token, answer);
    result(bt, device, Op::Pair, after).error
}
