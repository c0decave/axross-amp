//! A private D-Bus with axross's fake BlueZ (`tests/fake_bluez.py` of
//! axross, named by AMP_FAKE_BLUEZ; AMP_PYTHON: a Python with jeepney).
#![allow(dead_code)]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

pub struct Bus {
    pub dir: PathBuf,
    pub daemon: Child,
    pub fake: Option<Child>,
    pub address: String,
    pub script: PathBuf,
    pub python: String,
    pub control: Option<zbus::blocking::Connection>,
}

impl Bus {
    pub fn start() -> Option<Bus> {
        let script = PathBuf::from(std::env::var_os("AMP_FAKE_BLUEZ")?);
        let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
        let dir = std::env::temp_dir().join(format!("amp-bluez-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .arg(format!("--address=unix:dir={}", dir.display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut line = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let mut bus = Bus {
            dir,
            daemon,
            fake: None,
            address: line.trim().to_owned(),
            script,
            python,
            control: None,
        };
        assert!(!bus.address.is_empty());
        bus.start_fake();
        Some(bus)
    }

    pub fn start_fake(&mut self) {
        let mut fake = Command::new(&self.python)
            .arg(&self.script)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(fake.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), "ready", "the fake BlueZ did not start");
        self.fake = Some(fake);
    }

    pub fn kill_fake(&mut self) {
        if let Some(mut fake) = self.fake.take() {
            let _ = fake.kill();
            let _ = fake.wait();
        }
    }

    pub fn control(&mut self) -> &zbus::blocking::Connection {
        if self.control.is_none() {
            self.control = Some(
                zbus::blocking::connection::Builder::address(self.address.as_str())
                    .unwrap()
                    .build()
                    .unwrap(),
            );
        }
        self.control.as_ref().unwrap()
    }

    pub fn command(&mut self, op: &str, args: Value) -> Value {
        let mut cmd = args;
        cmd["op"] = json!(op);
        let reply = self
            .control()
            .call_method(
                Some("org.bluez"),
                "/org/axross/test",
                Some("org.axross.test.FakeBluez"),
                "Command",
                &(cmd.to_string(),),
            )
            .unwrap_or_else(|e| panic!("fake command {op}: {e}"));
        serde_json::from_str(&reply.body().deserialize::<String>().unwrap()).unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.kill_fake();
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
