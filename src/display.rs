//! The displays the pictures can be shown on ("Show on display…"): the
//! monitors connected - their connector (HDMI-1, DP-2, VGA-1), the
//! monitor's name where the system says it, the mode and the position -
//! and the sound server's output that plays on a display (its HDMI or
//! DisplayPort sink).
//!
//! On X11 `xrandr --query` lists them with their places on the desktop;
//! elsewhere on Linux the kernel's DRM connectors in `/sys/class/drm` do
//! (no places: a window cannot be put on a chosen output there - Wayland's
//! compositors decide, see the README). Elsewhere (macOS, Windows) there
//! is no list here: only the window's own monitor.

use crate::mixer::{Card, Snapshot};
use std::path::Path;

/// A monitor connected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Monitor {
    /// The connector: "HDMI-1", "DP-2", "VGA-1", "eDP-1" (as xrandr names
    /// it; the kernel's "HDMI-A-1" made "HDMI-1").
    pub connector: String,
    /// The monitor's own name where it is known (EDID), else empty.
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Its place on the desktop (points; None where unknown).
    pub at: Option<(i32, i32)>,
    pub primary: bool,
}

impl Monitor {
    /// What it is called in the window: "HDMI-1 (LG TV) 3840 x 2160".
    pub fn label(&self) -> String {
        let name = if self.name.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.name)
        };
        format!("{}{name} {} x {}", self.connector, self.width, self.height)
    }

    /// Whether it can carry sound (HDMI, DisplayPort; not VGA or DVI).
    pub fn can_play(&self) -> bool {
        let c = self.connector.to_ascii_uppercase();
        c.starts_with("HDMI") || c.starts_with("DP") || c.starts_with("DISPLAYPORT")
    }

    /// Its kind and number: ("HDMI", 1), ("DP", 2).
    fn kind_number(&self) -> (String, u32) {
        let c = self.connector.to_ascii_uppercase();
        let kind: String = c
            .chars()
            .take_while(|ch| ch.is_ascii_alphabetic())
            .collect();
        let number = c
            .rsplit(|ch: char| !ch.is_ascii_digit())
            .find(|s| !s.is_empty())
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        (kind, number)
    }
}

/// The monitors of `xrandr --query`'s text (connected ones).
pub fn parse_xrandr(text: &str) -> Vec<Monitor> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (Some(connector), Some(state)) = (words.next(), words.next()) else {
            continue;
        };
        if state != "connected" || line.starts_with(' ') {
            continue;
        }
        let mut m = Monitor {
            connector: connector.to_owned(),
            ..Default::default()
        };
        for w in line.split_whitespace().skip(2) {
            if w == "primary" {
                m.primary = true;
            } else if let Some((size, place)) = w.split_once('+')
                && let Some((wd, ht)) = size.split_once('x')
                && let (Ok(wd), Ok(ht)) = (wd.parse(), ht.parse())
            {
                m.width = wd;
                m.height = ht;
                let mut p = place.split('+');
                if let (Some(Ok(x)), Some(Ok(y))) =
                    (p.next().map(str::parse), p.next().map(str::parse))
                {
                    m.at = Some((x, y));
                }
            }
        }
        out.push(m);
    }
    out
}

/// The monitors of the kernel's DRM connectors under `dir`
/// (`/sys/class/drm`): connected ones, their first (preferred) mode.
pub fn parse_drm(dir: &Path) -> Vec<Monitor> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("card") && n.contains('-'))
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        let status = std::fs::read_to_string(path.join("status")).unwrap_or_default();
        if status.trim() != "connected" {
            continue;
        }
        // "card0-HDMI-A-1" -> "HDMI-1" (as xrandr says), "card0-DP-2" -> "DP-2"
        let raw = name.split_once('-').map(|(_, c)| c).unwrap_or(&name);
        let connector = raw.replace("HDMI-A-", "HDMI-").replace("HDMI-B-", "HDMI-");
        let mode = std::fs::read_to_string(path.join("modes")).unwrap_or_default();
        let (width, height) = mode
            .lines()
            .next()
            .and_then(|l| l.split_once('x'))
            .and_then(|(w, h)| {
                let h: String = h.chars().take_while(char::is_ascii_digit).collect();
                Some((w.parse().ok()?, h.parse().ok()?))
            })
            .unwrap_or((0, 0));
        out.push(Monitor {
            connector,
            name: edid_name(&std::fs::read(path.join("edid")).unwrap_or_default()),
            width,
            height,
            at: None,
            primary: false,
        });
    }
    out
}

/// The monitor's name in an EDID (the descriptor tagged 0xFC).
pub fn edid_name(edid: &[u8]) -> String {
    if edid.len() < 128 {
        return String::new();
    }
    for d in 0..4 {
        let at = 54 + d * 18;
        let block = &edid[at..at + 18];
        if block[0] == 0 && block[1] == 0 && block[3] == 0xFC {
            let text: String = block[5..]
                .iter()
                .take_while(|b| **b != 0x0A)
                .map(|b| *b as char)
                .collect();
            return text.trim().to_owned();
        }
    }
    String::new()
}

/// The monitors connected now (see the module's text).
pub fn list() -> Vec<Monitor> {
    if cfg!(target_os = "linux") {
        if std::env::var_os("DISPLAY").is_some()
            && let Ok(out) = std::process::Command::new("xrandr")
                .arg("--query")
                .stderr(std::process::Stdio::null())
                .output()
            && out.status.success()
        {
            let found = parse_xrandr(&String::from_utf8_lossy(&out.stdout));
            if !found.is_empty() {
                return found;
            }
        }
        return parse_drm(Path::new("/sys/class/drm"));
    }
    Vec::new()
}

/// How the sound gets to a display: a sink there is, or a card's profile
/// that makes one.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundOn {
    Sink(String),
    Profile { card: String, profile: String },
}

/// The sound server's way to `m`'s speakers: a sink whose description
/// names the monitor (its ELD), else the HDMI / DisplayPort output of its
/// number (HDMI-1: "hdmi-stereo", HDMI-2: "hdmi-stereo-extra1" - as
/// PulseAudio and PipeWire number them), a sink if there is one, else the
/// card's profile that makes it. None for VGA and DVI (no sound) or where
/// no such output exists.
pub fn sound_on(m: &Monitor, s: &Snapshot) -> Option<SoundOn> {
    if !m.can_play() {
        return None;
    }
    let hdmi = |text: &str| {
        let t = text.to_ascii_lowercase();
        t.contains("hdmi") || t.contains("displayport") || t.contains("dp ")
    };
    // the monitor's own name in a sink's description
    if !m.name.is_empty()
        && let Some(sink) = s.sinks.iter().find(|d| {
            d.description
                .to_ascii_lowercase()
                .contains(&m.name.to_ascii_lowercase())
        })
    {
        return Some(SoundOn::Sink(sink.name.clone()));
    }
    let (_, number) = m.kind_number();
    let profile = if number <= 1 {
        "output:hdmi-stereo".to_owned()
    } else {
        format!("output:hdmi-stereo-extra{}", number - 1)
    };
    let short = profile.trim_start_matches("output:");
    // a sink of that output (its name ends in it)
    if let Some(sink) = s.sinks.iter().find(|d| {
        let n = d.name.to_ascii_lowercase();
        n.ends_with(short) || n.ends_with(&format!("{short}.monitor"))
    }) {
        return Some(SoundOn::Sink(sink.name.clone()));
    }
    // a card with that profile, not yet on
    let card: Option<&Card> = s
        .cards
        .iter()
        .find(|c| c.profiles.iter().any(|p| p.name == profile && p.available));
    if let Some(c) = card {
        return Some(SoundOn::Profile {
            card: c.name.clone(),
            profile,
        });
    }
    // the only HDMI / DisplayPort sink there is
    let mut hdmi_sinks = s
        .sinks
        .iter()
        .filter(|d| hdmi(&d.name) || hdmi(&d.description));
    match (hdmi_sinks.next(), hdmi_sinks.next()) {
        (Some(only), None) => Some(SoundOn::Sink(only.name.clone())),
        _ => None,
    }
}

/// The monitor remembered (its connector, else its name), among those
/// connected now; else None.
pub fn find<'a>(monitors: &'a [Monitor], connector: &str, name: &str) -> Option<&'a Monitor> {
    monitors
        .iter()
        .find(|m| !connector.is_empty() && m.connector == connector)
        .or_else(|| monitors.iter().find(|m| !name.is_empty() && m.name == name))
}

/// The monitors connected, asked again every two seconds on a thread of
/// its own (`wake` when the list changes: a display plugged in or out).
pub fn watch(wake: impl Fn() + Send + 'static) -> std::sync::Arc<std::sync::Mutex<Vec<Monitor>>> {
    let out = std::sync::Arc::new(std::sync::Mutex::new(list()));
    let shared = out.clone();
    let _ = std::thread::Builder::new()
        .name("amp-displays".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                // (the window gone: no one holds the other end)
                if std::sync::Arc::strong_count(&shared) == 1 {
                    return;
                }
                let now = list();
                if let Ok(mut m) = shared.lock()
                    && *m != now
                {
                    *m = now;
                    wake();
                }
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{Device, Kind, Profile};

    const XRANDR: &str = "Screen 0: minimum 8 x 8, current 5760 x 2160, maximum 32767 x 32767
eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 194mm
   1920x1080     60.01*+
HDMI-1 connected 3840x2160+1920+0 (normal left inverted right x axis y axis) 1600mm x 900mm
   3840x2160     60.00*+  30.00
DP-1 disconnected (normal left inverted right x axis y axis)
VGA-1 connected (normal left inverted right x axis y axis)
   1024x768      60.00 +
";

    #[test]
    fn xrandrs_list_is_read() {
        let m = parse_xrandr(XRANDR);
        assert_eq!(m.len(), 3, "{m:?}");
        assert_eq!(m[0].connector, "eDP-1");
        assert!(m[0].primary);
        assert_eq!(
            (m[1].width, m[1].height, m[1].at),
            (3840, 2160, Some((1920, 0)))
        );
        // connected but off: no mode, no place
        assert_eq!(
            (m[2].connector.as_str(), m[2].width, m[2].at),
            ("VGA-1", 0, None)
        );
        assert!(m[1].can_play() && !m[2].can_play() && !m[0].can_play());
    }

    #[test]
    fn the_kernels_connectors_are_read() {
        let dir = std::env::temp_dir().join(format!("amp-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, status, modes) in [
            ("card0-HDMI-A-1", "connected", "3840x2160\n1920x1080\n"),
            ("card0-DP-2", "disconnected", ""),
            ("card0-VGA-1", "connected", "1024x768\n"),
        ] {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("status"), format!("{status}\n")).unwrap();
            std::fs::write(d.join("modes"), modes).unwrap();
        }
        // an EDID naming the monitor
        let mut edid = vec![0u8; 128];
        edid[54 + 18 * 3..54 + 18 * 3 + 5].copy_from_slice(&[0, 0, 0, 0xFC, 0]);
        edid[54 + 18 * 3 + 5..54 + 18 * 3 + 12].copy_from_slice(b"LG TV\n ");
        std::fs::write(dir.join("card0-HDMI-A-1/edid"), &edid).unwrap();
        let m = parse_drm(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(m.len(), 2, "{m:?}");
        assert_eq!(m[0].connector, "HDMI-1");
        assert_eq!(m[0].name, "LG TV");
        assert_eq!((m[0].width, m[0].height), (3840, 2160));
        assert_eq!(m[1].connector, "VGA-1");
        assert_eq!(m[0].label(), "HDMI-1 (LG TV) 3840 x 2160");
    }

    fn sink(name: &str, description: &str) -> Device {
        Device {
            kind: Kind::Sink,
            index: 0,
            name: name.into(),
            description: description.into(),
            channels: Vec::new(),
            muted: false,
            state: String::new(),
            ports: Vec::new(),
            active_port: String::new(),
            is_default: false,
            is_monitor: false,
            card_index: None,
            bus: String::new(),
            form_factor: String::new(),
        }
    }

    #[test]
    fn a_displays_sound_is_found() {
        let hdmi1 = Monitor {
            connector: "HDMI-1".into(),
            ..Default::default()
        };
        let hdmi2 = Monitor {
            connector: "HDMI-2".into(),
            ..Default::default()
        };
        let tv = Monitor {
            connector: "DP-1".into(),
            name: "LG TV".into(),
            ..Default::default()
        };
        let vga = Monitor {
            connector: "VGA-1".into(),
            ..Default::default()
        };
        let s = Snapshot {
            sinks: vec![
                sink(
                    "alsa_output.pci-0000_00_1f.3.analog-stereo",
                    "Built-in Audio Analog Stereo",
                ),
                sink(
                    "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1",
                    "GA104 HDMI 1",
                ),
                sink("alsa_output.pci-0000_01_00.1.hdmi-stereo-extra2", "LG TV"),
            ],
            cards: vec![Card {
                index: 1,
                name: "alsa_card.pci-0000_00_1f.3".into(),
                description: "Built-in Audio".into(),
                profiles: vec![Profile {
                    name: "output:hdmi-stereo".into(),
                    description: "Digital Stereo (HDMI) Output".into(),
                    available: true,
                    priority: 5900,
                }],
                active_profile: "output:analog-stereo".into(),
                bus: "pci".into(),
            }],
            ..Default::default()
        };
        // by its number: HDMI-2 is "extra1"
        assert_eq!(
            sound_on(&hdmi2, &s),
            Some(SoundOn::Sink(
                "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1".into()
            ))
        );
        // by the monitor's own name
        assert_eq!(
            sound_on(&tv, &s),
            Some(SoundOn::Sink(
                "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra2".into()
            ))
        );
        // no sink of its number yet: the card's profile that makes one
        assert_eq!(
            sound_on(&hdmi1, &s),
            Some(SoundOn::Profile {
                card: "alsa_card.pci-0000_00_1f.3".into(),
                profile: "output:hdmi-stereo".into()
            })
        );
        // VGA carries no sound
        assert_eq!(sound_on(&vga, &s), None);
    }

    #[test]
    fn the_display_remembered_is_found_again() {
        let ms = parse_xrandr(XRANDR);
        assert_eq!(find(&ms, "HDMI-1", "").map(|m| m.width), Some(3840));
        // its connector changed (another port): by its name
        let named = vec![Monitor {
            connector: "DP-3".into(),
            name: "LG TV".into(),
            ..Default::default()
        }];
        assert_eq!(
            find(&named, "HDMI-1", "LG TV").map(|m| m.connector.as_str()),
            Some("DP-3")
        );
        assert_eq!(find(&ms, "HDMI-9", "nope"), None);
    }
}
