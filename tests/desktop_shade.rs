//! "Follow the desktop's light or dark" on Linux, where winit says
//! nothing: the XDG desktop portal's `color-scheme`, served by a fake
//! portal on a private bus - light, then dark while the window is open.
#![cfg(target_os = "linux")]

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use egui_kittest::Harness;
use player::{Scratch, settle};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

struct Daemon(std::process::Child, String);

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

/// The portal's Settings, its color-scheme as asked (1 dark, 2 light).
struct Portal(Arc<AtomicU32>);

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Portal {
    fn read_one(
        &self,
        namespace: &str,
        key: &str,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
        if namespace == "org.freedesktop.appearance" && key == "color-scheme" {
            Ok(zbus::zvariant::OwnedValue::from(
                self.0.load(Ordering::Relaxed),
            ))
        } else {
            Err(zbus::fdo::Error::Failed("no such setting".into()))
        }
    }
}

#[test]
fn the_desktops_light_or_dark_is_followed() {
    let dir = Scratch::new("shade");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    let scheme = Arc::new(AtomicU32::new(2));
    let portal_conn = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Portal(scheme.clone()))
        .unwrap()
        .build()
        .unwrap();
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: true,
        desktop_bus: Some(bus.1.clone()),
        headless_audio: true,
        stand_in_covers: false,
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .build_eframe(move |cc| App::with_setup(cc, vec![], setup.clone()));
    h.state_mut().follow_desktop(true);
    let until = |h: &mut Harness<'static, App>, key: &str| {
        for _ in 0..300 {
            settle(h, 1);
            if h.state().theme_shown() == key {
                return true;
            }
        }
        false
    };
    // light: the light one of the pair (Paper by default)
    assert!(
        until(&mut h, "paper"),
        "light desktop, shown {}",
        h.state().theme_shown()
    );
    // dark, while the window is open (the portal says so: SettingChanged)
    scheme.store(1, Ordering::Relaxed);
    portal_conn
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "SettingChanged",
            &(
                "org.freedesktop.appearance",
                "color-scheme",
                zbus::zvariant::Value::from(1u32),
            ),
        )
        .unwrap();
    assert!(
        until(&mut h, "midnight"),
        "dark desktop, shown {}",
        h.state().theme_shown()
    );
}

/// How many connections were made to the bus so far (each new one gets the
/// next unique name: a probe's own number, less one).
fn connections_so_far(bus: &str) -> u64 {
    let c = zbus::blocking::connection::Builder::address(bus)
        .unwrap()
        .build()
        .unwrap();
    let name = c.unique_name().unwrap().to_string();
    name.rsplit('.').next().unwrap().parse::<u64>().unwrap()
}

/// Not following the desktop, nothing asks it; following, one connection
/// for as long as the window is open - not one every two seconds.
#[test]
fn the_desktop_is_asked_on_one_connection_and_only_when_followed() {
    let dir = Scratch::new("shade-conns");
    let Some(bus) = daemon(&dir) else {
        eprintln!("no dbus-daemon: skipped");
        return;
    };
    let scheme = Arc::new(AtomicU32::new(2));
    let _portal = zbus::blocking::connection::Builder::address(bus.1.as_str())
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Portal(scheme.clone()))
        .unwrap()
        .build()
        .unwrap();
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: true,
        desktop_bus: Some(bus.1.clone()),
        headless_audio: true,
        stand_in_covers: false,
    };
    let mut h = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .build_eframe(move |cc| App::with_setup(cc, vec![], setup.clone()));
    let wait = |h: &mut Harness<'static, App>, secs: f32| {
        let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
        while std::time::Instant::now() < end {
            settle(h, 1);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    wait(&mut h, 1.0);
    let start = connections_so_far(&bus.1);
    wait(&mut h, 5.0);
    let off = connections_so_far(&bus.1) - start - 1;
    assert_eq!(off, 0, "{off} connections in 5 s while not following");
    h.state_mut().follow_desktop(true);
    wait(&mut h, 1.0);
    let start = connections_so_far(&bus.1);
    wait(&mut h, 5.0);
    let on = connections_so_far(&bus.1) - start - 1;
    assert_eq!(on, 0, "{on} new connections in 5 s while following");
}
