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
        // (covers made for tracks without one, as the player has them: their
        // coming and their accent are part of what is tested - review of
        // 0.17.1, L4)
        stand_in_covers: true,
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
    // Seek, as the desktop's 15 s buttons do: back 15 s from 2.5 s, to
    // 0:00 and no further; on by a second from there
    p.call_method("Seek", &(-15_000_000i64)).unwrap();
    assert!(until(&mut h, 3.0, |a| a.engine().shared.position() < 0.3));
    p.call_method("Seek", &(1_000_000i64)).unwrap();
    assert!(until(&mut h, 3.0, |a| (a.engine().shared.position() - 1.0)
        .abs()
        < 0.3));
    assert_eq!(h.state().snapshot().current, 0, "Seek left the track");
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
    use axross_amp::mpris::{Claim, Mpris};
    let dir = folder("mpris-instance");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    let files = tracks(&dir, &["first.wav"], 3.0);
    let mut h = app_on(&dir, &bus.1, files);
    assert!(until(&mut h, 5.0, |a| a.is_loaded()));
    // "open with" from a file manager: a file:// URI with a space
    let more = dir.join("more dir");
    std::fs::create_dir_all(&more).unwrap();
    write_wav(&more.join("other one.wav"), 2.0);
    let uri = format!("file://{}", more.join("other one.wav").display()).replace(' ', "%20");
    let address = bus.1.clone();
    let second = std::thread::spawn(move || {
        matches!(Mpris::claim(Some(address), Some(&[uri])), Claim::Handed)
    });
    assert!(until(&mut h, 5.0, |a| a.snapshot().tracks.len() == 2));
    assert!(second.join().unwrap(), "the player running did not take it");
    assert!(until(&mut h, 5.0, |a| a.snapshot().current == 1 && a.is_loaded()));
    assert!(
        h.state().snapshot().tracks[1]
            .path
            .ends_with("more dir/other one.wav")
    );
}

#[test]
fn starts_at_the_same_moment_make_one_player() {
    // (the name was looked at by each start and taken later by its window:
    // three starts at once made three windows)
    use axross_amp::mpris::{Claim, Mpris, Request};
    let dir = folder("mpris-race");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let starts: Vec<_> = (0..3)
        .map(|i| {
            let (address, barrier) = (bus.1.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                Mpris::claim(Some(address), Some(&[format!("/m/{i}.wav")]))
            })
        })
        .collect();
    let claims: Vec<Claim> = starts.into_iter().map(|t| t.join().unwrap()).collect();
    let mut mine: Vec<Mpris> = Vec::new();
    let mut handed = 0;
    for claim in claims {
        match claim {
            Claim::Mine(m) => mine.push(*m),
            Claim::Handed => handed += 1,
            Claim::NoBus(why) => panic!("{why}"),
        }
    }
    assert_eq!((mine.len(), handed), (1, 2), "one player, two handing over");
    let opened: Vec<Request> = mine[0]
        .requests()
        .into_iter()
        .filter(|r| matches!(r, Request::Open(_)))
        .collect();
    assert_eq!(opened.len(), 2, "{opened:?}");
}

/// Paused, the desktop's Next, Previous and a Seek past the end change the
/// track and leave it paused (MPRIS: "if playback is paused or stopped, it
/// remains that way"); two quick Seek(+15 s) add up - each from where the
/// one before asked, not from what is heard yet.
#[test]
fn paused_stays_paused_and_seeks_add_up() {
    let dir = folder("mpris-paused");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: the MPRIS test is skipped");
        return;
    };
    let files = tracks(&dir, &["first.wav", "second.wav"], 40.0);
    let mut h = app_on(&dir, &bus.1, files);
    let client = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    let name = "org.mpris.MediaPlayer2.axross_amp";
    assert!(until(&mut h, 5.0, |_| dbus
        .name_has_owner(name.try_into().unwrap())
        .unwrap_or(false)));
    assert!(until(&mut h, 5.0, |a| a.is_loaded() && a.playing()));
    let p = player(&client, name);
    let call = |h: &mut Harness<'static, App>, method: &str| {
        let _: () = p.call(method, &()).unwrap();
        settle(h, 6);
        assert!(until(h, 3.0, |a| a.is_loaded()));
        settle(h, 3);
    };
    call(&mut h, "Pause");
    assert!(!h.state().playing());
    call(&mut h, "Next");
    assert_eq!(h.state().snapshot().current, 1);
    assert!(!h.state().playing(), "Next started it while paused");
    call(&mut h, "Previous");
    assert_eq!(h.state().snapshot().current, 0);
    assert!(!h.state().playing(), "Previous started it while paused");
    let _: () = p.call("Seek", &(60_000_000i64,)).unwrap();
    settle(&mut h, 6);
    assert!(until(&mut h, 3.0, |a| a.is_loaded() && a.snapshot().current == 1));
    settle(&mut h, 3);
    assert!(!h.state().playing(), "a Seek past the end started it");
    // two quick Seek(+15 s) from 0:00: at 0:30
    let _: () = p.call("Seek", &(15_000_000i64,)).unwrap();
    let _: () = p.call("Seek", &(15_000_000i64,)).unwrap();
    settle(&mut h, 6);
    let at = h.state_mut().shown_position();
    assert!((at - 30.0).abs() < 1.0, "two Seek(+15 s) came to {at:.1}");
}

/// Seek(i64::MAX) on the first of two tracks goes on to the second, playing
/// (review 11: it stopped - asked as the player came up, while the track
/// was still opening, it was dropped); Seek(i64::MIN) goes to the track's
/// start.
///
/// The seek is asked while the first track opens, every time: from the
/// second track playing, Previous (the first opened anew) and the seek are
/// on the bus before the player's next frame, which takes them in
/// together. (Asked as the player came up, the track had sometimes opened
/// already in the harness's first frames on a busy machine: the case went
/// untested.)
#[test]
fn the_largest_seeks_go_to_the_next_track_or_the_start() {
    let dir = folder("mpris-seek-max");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: the MPRIS test is skipped");
        return;
    };
    let files = tracks(&dir, &["first.wav", "second.wav"], 40.0);
    let mut h = app_on(&dir, &bus.1, files);
    let client = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    let name = "org.mpris.MediaPlayer2.axross_amp";
    assert!(until(&mut h, 5.0, |_| dbus
        .name_has_owner(name.try_into().unwrap())
        .unwrap_or(false)));
    assert!(until(&mut h, 5.0, |a| a.is_loaded()
        && a.playing()
        && a.snapshot().current == 0));
    let p = player(&client, name);
    let _: () = p.call("Next", &()).unwrap();
    assert!(until(&mut h, 5.0, |a| a.is_loaded()
        && a.playing()
        && a.snapshot().current == 1));
    // (no frame between them: the player takes the two in one)
    let _: () = p.call("Previous", &()).unwrap();
    let _: () = p.call("Seek", &(i64::MAX,)).unwrap();
    h.run_steps(1);
    // (the seek was asked with the first track opening anew)
    assert_eq!(h.state().snapshot().current, 0, "Previous");
    assert!(
        !h.state().is_loaded(),
        "the first track open within the frame that asked it"
    );
    settle(&mut h, 6);
    assert!(until(&mut h, 3.0, |a| a.is_loaded() && a.snapshot().current == 1));
    settle(&mut h, 3);
    assert!(h.state().playing(), "Seek(i64::MAX) stopped it");
    let _: () = p.call("Seek", &(10_000_000i64,)).unwrap();
    settle(&mut h, 6);
    let _: () = p.call("Seek", &(i64::MIN,)).unwrap();
    settle(&mut h, 6);
    assert_eq!(h.state().snapshot().current, 1);
    let at = h.state_mut().shown_position();
    assert!(at < 1.0, "Seek(i64::MIN) came to {at:.1}");
    assert!(h.state().playing());
}

/// The program the updater starts (Restart now) waits for the player's
/// name and takes it when the one it replaces ends - so the next file
/// opened from the file manager goes to it, not to a second player
/// (review 13, M5: it took a name of its own for good).
#[test]
fn the_updated_program_takes_the_name_when_the_old_one_ends() {
    use axross_amp::mpris::{Claim, Mpris};
    let dir = folder("mpris-after-update");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    let name = "org.mpris.MediaPlayer2.axross_amp";
    let client = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    let owner = || {
        dbus.get_name_owner(name.try_into().unwrap())
            .ok()
            .map(|o| o.to_string())
    };
    let Claim::Mine(old) = Mpris::claim(Some(bus.1.clone()), None) else {
        panic!("the first player has no name");
    };
    let before = owner().expect("the old player holds the name");
    let Claim::Mine(new) = Mpris::claim_after(Some(bus.1.clone())) else {
        panic!("the new program did not wait for the name");
    };
    assert_eq!(
        owner().as_deref(),
        Some(before.as_str()),
        "taken from the old one"
    );
    drop(old);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while owner().as_deref() == Some(before.as_str()) && std::time::Instant::now() < end {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let after = owner();
    assert!(
        after.is_some() && after.as_deref() != Some(before.as_str()),
        "the name is not the new program's: {after:?}"
    );
    // a second start now hands its files to it
    let handed = Mpris::claim(Some(bus.1.clone()), Some(&["/m/x.wav".to_owned()]));
    assert!(matches!(handed, Claim::Handed));
    drop(new);
}
