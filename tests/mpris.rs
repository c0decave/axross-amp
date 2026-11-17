//! MPRIS: the player on a private session D-Bus (its own dbus-daemon), as
//! a desktop, media keys or a headset's buttons see and drive it. Without
//! `dbus-daemon` the test says so and passes.
#![cfg(target_os = "linux")]

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use egui_kittest::Harness;
use player::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

struct Daemon(Child, String);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon(dir: &std::path::Path) -> Option<Daemon> {
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .arg(format!("--address=unix:dir={}", dir.display()))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take()?)
        .read_line(&mut line)
        .ok()?;
    Some(Daemon(child, line.trim().to_owned()))
}

fn app_on(dir: &std::path::Path, bus: &str, args: Vec<String>) -> Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: true,
        desktop_bus: Some(bus.to_owned()),
        headless_audio: true,
    };
    Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .build_eframe(move |cc| App::with_setup(cc, args.clone(), setup.clone()))
}

/// The player's interface, read anew each time (no cache: what the
/// player answers now).
fn player<'a>(conn: &'a Connection, name: &'a str) -> Proxy<'a> {
    zbus::blocking::proxy::Builder::new(conn)
        .destination(name.to_owned())
        .unwrap()
        .path("/org/mpris/MediaPlayer2")
        .unwrap()
        .interface("org.mpris.MediaPlayer2.Player")
        .unwrap()
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .unwrap()
}

#[test]
fn the_desktop_sees_and_drives_the_player() {
    let dir = folder("mpris");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: the MPRIS test is skipped");
        return;
    };
    let files = tracks(&dir, &["first.wav", "second.wav"], 4.0);
    let mut h = app_on(&dir, &bus.1, files);
    let client = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    let name = "org.mpris.MediaPlayer2.axross_amp";
    assert!(
        until(&mut h, 5.0, |_| dbus
            .name_has_owner(name.try_into().unwrap())
            .unwrap_or(false)),
        "the player did not come on the bus"
    );
    assert!(until(&mut h, 5.0, |a| a.is_loaded() && a.playing()));
    settle(&mut h, 3);
    let p = player(&client, name);
    let status = |p: &Proxy<'_>| p.get_property::<String>("PlaybackStatus").unwrap();
    assert_eq!(status(&p), "Playing");
    let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap();
    let text = |key: &str| {
        meta.get(key)
            .and_then(|v| String::try_from(v.try_clone().unwrap()).ok())
            .unwrap_or_default()
    };
    assert_eq!(text("xesam:title"), "first");
    assert!(text("xesam:url").starts_with("file:///") && text("xesam:url").ends_with("first.wav"));
    let length: i64 = meta
        .get("mpris:length")
        .and_then(|v| i64::try_from(v.try_clone().unwrap()).ok())
        .unwrap();
    assert!((length - 4_000_000).abs() < 50_000, "{length}");
    let track: OwnedObjectPath =
        OwnedObjectPath::try_from(meta.get("mpris:trackid").unwrap().try_clone().unwrap()).unwrap();
    assert!(p.get_property::<bool>("CanSeek").unwrap());

    // the changes are signalled: PlayPause -> Paused
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")
        .unwrap()
        .member("PropertiesChanged")
        .unwrap()
        .build();
    let mut signals = zbus::blocking::MessageIterator::for_match_rule(rule, &client, None).unwrap();
    p.call_method("PlayPause", &()).unwrap();
    assert!(until(&mut h, 3.0, |a| !a.playing()));
    settle(&mut h, 2);
    assert_eq!(status(&p), "Paused");
    let signal = signals.next().unwrap().unwrap();
    let (iface, changed, _): (String, HashMap<String, OwnedValue>, Vec<String>) =
        signal.body().deserialize().unwrap();
    assert_eq!(iface, "org.mpris.MediaPlayer2.Player");
    assert_eq!(
        changed
            .get("PlaybackStatus")
            .map(|v| String::try_from(v.try_clone().unwrap()).unwrap())
            .as_deref(),
        Some("Paused")
    );
    p.call_method("Play", &()).unwrap();
    assert!(until(&mut h, 3.0, |a| a.playing()));

    // SetPosition on this track, volume, shuffle, repeat
    p.call_method("SetPosition", &(track.as_ref(), 2_500_000i64))
        .unwrap();
    assert!(until(&mut h, 3.0, |a| (a.engine().shared.position() - 2.5)
        .abs()
        < 0.05));
    let position: i64 = p.get_property("Position").unwrap();
    assert!((position - 2_500_000).abs() < 50_000, "{position}");
    p.set_property("Volume", 0.3f64).unwrap();
    assert!(until(&mut h, 3.0, |a| a.snapshot().volume == 30));
    p.set_property("Shuffle", true).unwrap();
    p.set_property("LoopStatus", "Track").unwrap();
    assert!(until(&mut h, 3.0, |a| {
        let s = a.snapshot();
        s.shuffle && s.repeat == "one"
    }));
    settle(&mut h, 2);
    assert_eq!(p.get_property::<String>("LoopStatus").unwrap(), "Track");
    p.set_property("Shuffle", false).unwrap();

    // Next, as a media key does
    p.call_method("Next", &()).unwrap();
    assert!(until(&mut h, 3.0, |a| a.snapshot().current == 1));
    settle(&mut h, 3);
    let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap();
    assert_eq!(
        meta.get("xesam:title")
            .map(|v| String::try_from(v.try_clone().unwrap()).unwrap())
            .as_deref(),
        Some("second")
    );
    // the identity on the root interface
    let root = Proxy::new(
        &client,
        name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2",
    )
    .unwrap();
    assert_eq!(
        root.get_property::<String>("Identity").unwrap(),
        "axross-amp"
    );
}

#[test]
fn a_second_start_hands_its_files_to_the_player_running() {
    let dir = folder("mpris-instance");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    // no player yet: nothing takes them (the second start plays itself)
    assert!(!axross_amp::mpris::forward(Some(&bus.1), &["x.wav".into()]));
    let files = tracks(&dir, &["first.wav"], 3.0);
    let mut h = app_on(&dir, &bus.1, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    let client = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    assert!(until(&mut h, 5.0, |_| dbus
        .name_has_owner("org.mpris.MediaPlayer2.axross_amp".try_into().unwrap())
        .unwrap_or(false)));
    // "open with" from a file manager: a file:// URI with a space, and a folder
    let more = dir.join("more dir");
    std::fs::create_dir_all(&more).unwrap();
    write_wav(&more.join("other one.wav"), 2.0);
    let uri = format!("file://{}", more.join("other one.wav").display()).replace(' ', "%20");
    let forwarding = {
        let address = bus.1.clone();
        std::thread::spawn(move || axross_amp::mpris::forward(Some(&address), &[uri]))
    };
    assert!(until(&mut h, 5.0, |a| a.snapshot().tracks.len() == 2));
    assert!(
        forwarding.join().unwrap(),
        "the player running did not take it"
    );
    assert!(until(&mut h, 5.0, |a| a.snapshot().current == 1 && a.is_loaded()));
    assert!(
        h.state().snapshot().tracks[1]
            .path
            .ends_with("more dir/other one.wav")
    );
}
