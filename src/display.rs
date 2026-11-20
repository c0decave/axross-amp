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

use crate::mixer::Snapshot;
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

/// What xrandr is asked: the state as it is (`--query` probes every
/// output's EDID again, which can make a display blink).
pub const XRANDR_ARGS: [&str; 1] = ["--current"];

/// The monitors connected now (see the module's text).
pub fn list() -> Vec<Monitor> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
    let xrandr = (!wayland && std::env::var_os("DISPLAY").is_some())
        .then(|| {
            std::process::Command::new("xrandr")
                .args(XRANDR_ARGS)
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        })
        .flatten();
    choose_list(wayland, xrandr.as_deref(), || {
        parse_drm(Path::new("/sys/class/drm"))
    })
}

/// Which list to trust: under Wayland the kernel's (XWayland sets DISPLAY
/// too, and its xrandr says XWAYLAND0, XWAYLAND1 - no real connector, no
/// sound); on X11 xrandr's (with the places on the desktop), else the
/// kernel's.
pub fn choose_list(
    wayland: bool,
    xrandr: Option<&str>,
    drm: impl FnOnce() -> Vec<Monitor>,
) -> Vec<Monitor> {
    if !wayland && let Some(text) = xrandr {
        let found: Vec<Monitor> = parse_xrandr(text)
            .into_iter()
            .filter(|m| !m.connector.starts_with("XWAYLAND"))
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    drm()
}

/// How the sound gets to a display: a sink there is, or a card's profile
/// that makes one.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundOn {
    Sink(String),
    Profile { card: String, profile: String },
}

/// Whether a sink or port is an HDMI / DisplayPort one (by its name, its
/// description or its port: never a Bluetooth or USB device).
fn is_hdmi(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("hdmi") || t.contains("displayport") || t.contains("iec958-stereo-hdmi")
}

/// The same name, as two sources write it (case and spaces aside) -
/// never a part of one: "TV" is not "TV Soundbar".
fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// The sound server's way to `m`'s speakers:
/// - the HDMI / DisplayPort sink, or card port, whose monitor (the ELD the
///   display sends: PulseAudio's and PipeWire's `device.product.name`,
///   in a port's description in brackets) is `m`'s own name (its EDID) -
///   the same name, never a part of one;
/// - else, where it cannot be mistaken, the one HDMI / DisplayPort output
///   there is for the one display that can carry sound: the only such sink,
///   else the only card profile that makes one.
///
/// None for VGA and DVI (no sound), where no such output exists, or where
/// it cannot be told which output plays on `m` (two displays, two HDMI
/// outputs and no names: its number is no guide - DP-1 and HDMI-1 are
/// both a card's first HDMI output, NVIDIA's HDMI-0 and DP-0 alike).
pub fn sound_on(m: &Monitor, s: &Snapshot) -> Option<SoundOn> {
    sound_on_among(m, std::slice::from_ref(m), s)
}

/// As [`sound_on`], `monitors` the displays connected (the one display
/// that can carry sound, or several).
pub fn sound_on_among(m: &Monitor, monitors: &[Monitor], s: &Snapshot) -> Option<SoundOn> {
    if !m.can_play() {
        return None;
    }
    let hdmi_sinks: Vec<&crate::mixer::Device> = s
        .sinks
        .iter()
        .filter(|d| {
            !d.is_monitor
                && d.bus != "bluetooth"
                && (is_hdmi(&d.name) || is_hdmi(&d.description) || is_hdmi(&d.active_port))
        })
        .collect();
    // (a card's own sinks: by its index, else by the name they share -
    // "alsa_card.pci-0000_00_1f.3", "alsa_output.pci-0000_00_1f.3.hdmi-stereo")
    let of_card = |d: &crate::mixer::Device, card: &crate::mixer::Card| {
        d.card_index == Some(card.index)
            || card
                .name
                .split_once('.')
                .is_some_and(|(_, key)| !key.is_empty() && d.name.contains(key))
    };
    // by the monitor's name (ELD = EDID) - only where that name is its
    // alone: two displays of one make and model name both, and which
    // output is whose cannot be told
    if !m.name.is_empty() {
        let twins = monitors
            .iter()
            .filter(|o| o.connector != m.connector && same_name(&o.name, &m.name))
            .count();
        if twins > 0 {
            return None;
        }
        if let Some(sink) = hdmi_sinks.iter().find(|d| {
            s.displays
                .iter()
                .any(|(sink, n)| *sink == d.name && same_name(n, &m.name))
        }) {
            return Some(SoundOn::Sink(sink.name.clone()));
        }
        for card in &s.cards {
            for port in card.ports.iter().filter(|p| same_name(&p.monitor, &m.name)) {
                // its sink there already: on that port, or the card's HDMI
                // sink while one of the port's profiles is on
                let on_port = hdmi_sinks
                    .iter()
                    .find(|d| of_card(d, card) && d.active_port == port.name);
                let active = port.profiles.contains(&card.active_profile);
                let sink = on_port.or_else(|| {
                    active
                        .then(|| hdmi_sinks.iter().find(|d| of_card(d, card)))
                        .flatten()
                });
                if let Some(sink) = sink {
                    return Some(SoundOn::Sink(sink.name.clone()));
                }
                if active {
                    // (on, but its sink not there yet: the profile as it is)
                    return Some(SoundOn::Profile {
                        card: card.name.clone(),
                        profile: card.active_profile.clone(),
                    });
                }
                if let Some(profile) = choose_profile(card, &port.profiles) {
                    return Some(SoundOn::Profile {
                        card: card.name.clone(),
                        profile,
                    });
                }
            }
        }
    }
    // the one there is, for the one display that can carry sound
    let others = monitors
        .iter()
        .filter(|o| o.can_play() && !o.primary && o.connector != m.connector)
        .count();
    if others > 0 {
        return None;
    }
    match hdmi_sinks.as_slice() {
        [only] => return Some(SoundOn::Sink(only.name.clone())),
        [] => {}
        _ => return None,
    }
    // the one HDMI output a card can switch to (its profiles with an
    // input kept count as the same output)
    let mut found: Option<(&crate::mixer::Card, String)> = None;
    for card in &s.cards {
        let outputs: std::collections::BTreeSet<&str> = card
            .profiles
            .iter()
            .filter(|p| p.available && p.name.starts_with("output:") && is_hdmi(&p.name))
            .map(|p| p.name.split('+').next().unwrap_or(&p.name))
            .collect();
        match outputs.len() {
            0 => {}
            1 if found.is_none() => {
                let names: Vec<String> = card.profiles.iter().map(|p| p.name.clone()).collect();
                found = choose_profile(card, &names).map(|p| (card, p));
            }
            _ => return None,
        }
    }
    found.map(|(card, profile)| SoundOn::Profile {
        card: card.name.clone(),
        profile,
    })
}

/// Of `candidates`, the HDMI / DisplayPort profile to switch `card` to: an
/// available one that keeps the input the card has now (the analog mic of
/// "output:analog-stereo+input:analog-stereo" stays with
/// "output:hdmi-stereo+input:analog-stereo"), else the output alone.
fn choose_profile(card: &crate::mixer::Card, candidates: &[String]) -> Option<String> {
    let usable: Vec<&String> = candidates
        .iter()
        .filter(|p| {
            p.starts_with("output:")
                && is_hdmi(p)
                && card.profiles.iter().any(|q| &q.name == *p && q.available)
        })
        .collect();
    let input = card.active_profile.split_once('+').map(|(_, i)| i);
    input
        .and_then(|i| usable.iter().find(|p| p.ends_with(&format!("+{i}"))))
        .or_else(|| usable.iter().find(|p| !p.contains('+')))
        .or(usable.first())
        .map(|p| (*p).clone())
}

/// The sound moved to a display: its sink, and the card's profile before
/// it was switched for it (card, profile), where one was.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub sink: String,
    pub switched: Option<(String, String)>,
}

/// Switch `card` to `profile` (pactl).
pub fn set_profile(card: &str, profile: &str) -> Result<(), String> {
    let (args, what) = crate::mixer::Action::Profile {
        card: card.to_owned(),
        profile: profile.to_owned(),
    }
    .command()?;
    let (ok, _, err) = crate::mixer::run(&args)?;
    if ok {
        Ok(())
    } else {
        Err(crate::mixer::error_sentence(&what, &err))
    }
}

/// Where the card's profile before a display took the sound is kept - in
/// its own small file, written before the card is switched (not with the
/// window's settings a moment later: a crash, a quit or a closed view in
/// between must not lose it).
pub fn restore_file(dir: &Path) -> std::path::PathBuf {
    dir.join("axross-amp-display-restore.json")
}

/// Keep (card, profile) as what to put back - unless something is kept
/// already: the first "before" is the one to go back to.
pub fn remember_profile(dir: &Path, card: &str, profile: &str) {
    let path = restore_file(dir);
    if path.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let text = serde_json::json!({ "card": card, "profile": profile }).to_string();
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// What was kept, taken (the file removed).
pub fn take_profile(dir: &Path) -> Option<(String, String)> {
    let path = restore_file(dir);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let card = v.get("card")?.as_str()?.to_owned();
    let profile = v.get("profile")?.as_str()?.to_owned();
    (!card.is_empty() && !profile.is_empty()).then_some((card, profile))
}

/// The sound server's output on `m` (blocking: pactl, and a card switched
/// to its HDMI / DisplayPort profile waited for - run it on a thread of
/// its own). The card's profile before is kept in `dir` before it is
/// switched (see [`remember_profile`]); a switch whose output does not
/// come is undone at once, and so is one whose view was closed meanwhile
/// (`cancel`): the card back on the profile it had.
pub fn move_sound(
    m: &Monitor,
    monitors: &[Monitor],
    dir: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Moved, String> {
    use std::sync::atomic::Ordering;
    if !m.can_play() {
        return Err(format!("{} carries no sound", m.connector));
    }
    let snapshot = crate::mixer::refresh()?;
    match sound_on_among(m, monitors, &snapshot) {
        Some(SoundOn::Sink(sink)) => Ok(Moved {
            sink,
            switched: None,
        }),
        Some(SoundOn::Profile { card, profile }) => {
            let before = snapshot
                .cards
                .iter()
                .find(|c| c.name == card)
                .map(|c| c.active_profile.clone())
                .unwrap_or_default();
            if cancel.load(Ordering::Relaxed) {
                return Err("the view was closed".into());
            }
            // (never a profile switched that is on already: its sink is
            // waited for)
            let switching = profile != before;
            if switching && !before.is_empty() {
                remember_profile(dir, &card, &before);
            }
            let undo = || {
                if switching && !before.is_empty() {
                    let _ = set_profile(&card, &before);
                    let _ = take_profile(dir);
                }
            };
            if switching && let Err(why) = set_profile(&card, &profile) {
                undo();
                return Err(why);
            }
            // (its sink comes a moment later)
            for _ in 0..15 {
                if cancel.load(Ordering::Relaxed) {
                    undo();
                    return Err("the view was closed".into());
                }
                let now = crate::mixer::refresh()?;
                if let Some(SoundOn::Sink(sink)) = sound_on_among(m, monitors, &now) {
                    if cancel.load(Ordering::Relaxed) {
                        undo();
                        return Err("the view was closed".into());
                    }
                    return Ok(Moved {
                        sink,
                        switched: (switching && !before.is_empty())
                            .then(|| (card.clone(), before.clone())),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            undo();
            Err(format!("no output came for {profile}"))
        }
        None => Err(format!(
            "no sound output that is surely {}'s (choose it in the Mixer tab)",
            m.connector
        )),
    }
}

/// The monitor remembered (its connector, else its name), among those
/// connected now; else None - also where its connector is gone and two or
/// more are of its name (no guess: the view opened on the first of two
/// alike monitors, as the sound would have, and is not).
pub fn find<'a>(monitors: &'a [Monitor], connector: &str, name: &str) -> Option<&'a Monitor> {
    if let Some(m) = monitors
        .iter()
        .find(|m| !connector.is_empty() && m.connector == connector)
    {
        return Some(m);
    }
    let mut named = monitors
        .iter()
        .filter(|m| !name.is_empty() && m.name == name);
    match (named.next(), named.next()) {
        (Some(m), None) => Some(m),
        _ => None,
    }
}

/// Whether the monitor remembered is not found only because two or more are
/// of its name (its connector gone).
pub fn ambiguous(monitors: &[Monitor], connector: &str, name: &str) -> bool {
    find(monitors, connector, name).is_none()
        && !name.is_empty()
        && monitors.iter().filter(|m| m.name == name).count() > 1
}

/// The monitors connected: asked when the window opens, then every two
/// seconds while `wanted` (the display menu open, the display view shown or
/// to be opened again - an unplugged display seen at once there), on a
/// thread of its own (`wake` when the list changes); not otherwise - no
/// prober running for the whole session.
pub fn watch(
    wanted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake: impl Fn() + Send + 'static,
) -> std::sync::Arc<std::sync::Mutex<Vec<Monitor>>> {
    watch_with(list, wanted, std::time::Duration::from_secs(2), wake)
}

/// As [`watch`], with the lister and the period given (tests).
pub fn watch_with(
    lister: impl Fn() -> Vec<Monitor> + Send + 'static,
    wanted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    period: std::time::Duration,
    wake: impl Fn() + Send + 'static,
) -> std::sync::Arc<std::sync::Mutex<Vec<Monitor>>> {
    let out = std::sync::Arc::new(std::sync::Mutex::new(lister()));
    let shared = out.clone();
    let _ = std::thread::Builder::new()
        .name("amp-displays".into())
        .spawn(move || {
            loop {
                std::thread::sleep(period);
                // (the window gone: no one holds the other end)
                if std::sync::Arc::strong_count(&shared) == 1 {
                    return;
                }
                if !wanted.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }
                let now = lister();
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
    use crate::mixer::{Card, CardPort, Device, Kind, Profile};

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

    /// The (sink, display) pairs of sinks that say whose they are.
    fn says(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect()
    }

    fn mon(connector: &str, name: &str) -> Monitor {
        Monitor {
            connector: connector.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    fn hdmi_card(profiles: &[&str], ports: Vec<CardPort>) -> Card {
        Card {
            index: 1,
            name: "alsa_card.pci-0000_00_1f.3".into(),
            description: "Built-in Audio".into(),
            profiles: profiles
                .iter()
                .map(|p| Profile {
                    name: (*p).into(),
                    description: String::new(),
                    available: true,
                    priority: 5900,
                })
                .collect(),
            active_profile: "output:analog-stereo".into(),
            bus: "pci".into(),
            ports,
        }
    }

    #[test]
    fn a_displays_sound_is_found_by_its_name_or_where_it_cannot_be_mistaken() {
        let analog = sink(
            "alsa_output.pci-0000_00_1f.3.analog-stereo",
            "Built-in Audio Analog Stereo",
        );
        // two displays, two HDMI outputs, each saying whose it is (ELD)
        let s = Snapshot {
            sinks: vec![
                analog.clone(),
                sink("alsa_output.pci-0000_01_00.1.hdmi-stereo", "GA104 HDMI 0"),
                sink(
                    "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1",
                    "GA104 HDMI 1",
                ),
            ],
            displays: says(&[
                ("alsa_output.pci-0000_01_00.1.hdmi-stereo", "DELL U2415"),
                ("alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1", "LG TV"),
            ]),
            ..Default::default()
        };
        let dell = mon("DP-1", "DELL U2415");
        let tv = mon("HDMI-1", "LG TV");
        let both = [dell.clone(), tv.clone()];
        assert_eq!(
            sound_on_among(&tv, &both, &s),
            Some(SoundOn::Sink(
                "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1".into()
            ))
        );
        assert_eq!(
            sound_on_among(&dell, &both, &s),
            Some(SoundOn::Sink(
                "alsa_output.pci-0000_01_00.1.hdmi-stereo".into()
            ))
        );
        // DP-1 and HDMI-1, NVIDIA's DP-0 and HDMI-0, AMD's DisplayPort-0
        // and -1, no names: their numbers are no guide - none
        let s_nameless = Snapshot {
            sinks: vec![
                analog.clone(),
                sink("alsa_output.pci-0000_01_00.1.hdmi-stereo", "GA104 HDMI 0"),
                sink(
                    "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1",
                    "GA104 HDMI 1",
                ),
            ],
            ..Default::default()
        };
        for (a, b) in [
            ("DP-1", "HDMI-1"),
            ("DP-0", "HDMI-0"),
            ("DisplayPort-0", "DisplayPort-1"),
        ] {
            let pair = [mon(a, ""), mon(b, "")];
            assert_eq!(sound_on_among(&pair[0], &pair, &s_nameless), None, "{a}");
            assert_eq!(sound_on_among(&pair[1], &pair, &s_nameless), None, "{b}");
        }
        // one display that can carry sound, one HDMI output: that one
        let one = Snapshot {
            sinks: vec![
                analog.clone(),
                sink(
                    "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
                    "Built-in Audio Digital Stereo (HDMI)",
                ),
            ],
            ..Default::default()
        };
        let lone = mon("HDMI-1", "");
        assert_eq!(
            sound_on_among(&lone, std::slice::from_ref(&lone), &one),
            Some(SoundOn::Sink(
                "alsa_output.pci-0000_00_1f.3.hdmi-stereo".into()
            ))
        );
        // a display called "TV" and a Bluetooth soundbar called "TV
        // Soundbar": never the soundbar
        let mut bar = sink("bluez_output.11_22.1", "TV Soundbar");
        bar.bus = "bluetooth".into();
        let tv_short = mon("HDMI-1", "TV");
        let s_bar = Snapshot {
            sinks: vec![analog.clone(), bar],
            ..Default::default()
        };
        assert_eq!(
            sound_on_among(&tv_short, std::slice::from_ref(&tv_short), &s_bar),
            None
        );
        // nothing on yet: the card's port that has the display on it (ELD)
        let s_card = Snapshot {
            sinks: vec![analog.clone()],
            cards: vec![hdmi_card(
                &["output:hdmi-stereo", "output:hdmi-stereo-extra1"],
                vec![
                    CardPort {
                        name: "hdmi-output-0".into(),
                        monitor: "DELL U2415".into(),
                        profiles: vec!["output:hdmi-stereo".into()],
                    },
                    CardPort {
                        name: "hdmi-output-1".into(),
                        monitor: "LG TV".into(),
                        profiles: vec!["output:hdmi-stereo-extra1".into()],
                    },
                ],
            )],
            ..Default::default()
        };
        assert_eq!(
            sound_on_among(&tv, &both, &s_card),
            Some(SoundOn::Profile {
                card: "alsa_card.pci-0000_00_1f.3".into(),
                profile: "output:hdmi-stereo-extra1".into()
            })
        );
        // the one profile there is, for the one display
        let s_one_profile = Snapshot {
            sinks: vec![analog],
            cards: vec![hdmi_card(&["output:hdmi-stereo"], Vec::new())],
            ..Default::default()
        };
        assert_eq!(
            sound_on_among(&lone, std::slice::from_ref(&lone), &s_one_profile),
            Some(SoundOn::Profile {
                card: "alsa_card.pci-0000_00_1f.3".into(),
                profile: "output:hdmi-stereo".into()
            })
        );
        // VGA carries no sound
        assert_eq!(sound_on(&mon("VGA-1", ""), &one), None);
    }

    #[test]
    fn a_name_on_the_cards_port_and_its_profile_on_already() {
        // PulseAudio as it lists it: the display's name on the card's port,
        // the HDMI profile (with the mic) on already, its sink there with
        // no name of its own - that sink, nothing switched
        let mut hd = sink(
            "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
            "Built-in Audio Digital Stereo (HDMI)",
        );
        hd.active_port = "hdmi-output-0".into();
        let profiles = |active: &str| {
            let mut c = hdmi_card(
                &[
                    "output:analog-stereo+input:analog-stereo",
                    "output:hdmi-stereo",
                    "output:hdmi-stereo+input:analog-stereo",
                ],
                vec![CardPort {
                    name: "hdmi-output-0".into(),
                    monitor: "LG TV SSCR2".into(),
                    profiles: vec![
                        "output:hdmi-stereo".into(),
                        "output:hdmi-stereo+input:analog-stereo".into(),
                    ],
                }],
            );
            c.index = 0;
            c.active_profile = active.into();
            c
        };
        let tv = mon("HDMI-1", "LG TV SSCR2");
        let lap = Monitor {
            primary: true,
            ..mon("eDP-1", "")
        };
        let both = [lap, tv.clone()];
        let on = Snapshot {
            sinks: vec![hd],
            displays: says(&[(
                "alsa_output.pci-0000_00_1f.3.hdmi-stereo",
                "Cannon Lake PCH cAVS",
            )]),
            cards: vec![profiles("output:hdmi-stereo+input:analog-stereo")],
            ..Default::default()
        };
        assert_eq!(
            sound_on_among(&tv, &both, &on),
            Some(SoundOn::Sink(
                "alsa_output.pci-0000_00_1f.3.hdmi-stereo".into()
            ))
        );
        // analog with the mic on: switched to HDMI with the mic kept
        let off = Snapshot {
            sinks: vec![sink("alsa_output.pci-0000_00_1f.3.analog-stereo", "Analog")],
            cards: vec![profiles("output:analog-stereo+input:analog-stereo")],
            ..Default::default()
        };
        assert_eq!(
            sound_on_among(&tv, &both, &off),
            Some(SoundOn::Profile {
                card: "alsa_card.pci-0000_00_1f.3".into(),
                profile: "output:hdmi-stereo+input:analog-stereo".into()
            })
        );
    }

    #[test]
    fn two_displays_of_one_name_get_no_sound_by_guessing() {
        let s = Snapshot {
            sinks: vec![
                sink("alsa_output.pci-0000_01_00.1.hdmi-stereo", "GA104 HDMI 0"),
                sink(
                    "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1",
                    "GA104 HDMI 1",
                ),
            ],
            displays: says(&[
                ("alsa_output.pci-0000_01_00.1.hdmi-stereo", "DELL U2723QE"),
                (
                    "alsa_output.pci-0000_01_00.1.hdmi-stereo-extra1",
                    "DELL U2723QE",
                ),
            ]),
            ..Default::default()
        };
        let a = mon("DP-1", "DELL U2723QE");
        let b = mon("DP-2", "DELL U2723QE");
        let both = [a.clone(), b.clone()];
        assert_eq!(sound_on_among(&a, &both, &s), None);
        assert_eq!(sound_on_among(&b, &both, &s), None);
    }

    #[test]
    fn under_wayland_the_kernel_says_which_displays_not_xwayland() {
        const XWAYLAND: &str =
            "Screen 0: minimum 16 x 16, current 5760 x 2160, maximum 32767 x 32767
XWAYLAND0 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 340mm x 190mm
   1920x1080     59.96*+
XWAYLAND1 connected 3840x2160+1920+0 (normal left inverted right x axis y axis) 600mm x 340mm
   3840x2160     59.98*+
";
        let drm = || vec![mon("eDP-1", ""), mon("HDMI-1", "LG TV")];
        // a Wayland session (XWayland sets DISPLAY too): the kernel's list
        let got = choose_list(true, Some(XWAYLAND), drm);
        assert_eq!(
            got.iter().map(|m| m.connector.as_str()).collect::<Vec<_>>(),
            ["eDP-1", "HDMI-1"]
        );
        // XWayland's outputs never taken for real ones
        let got = choose_list(false, Some(XWAYLAND), drm);
        assert!(
            got.iter().all(|m| !m.connector.starts_with("XWAYLAND")),
            "{got:?}"
        );
        // X11: xrandr's, with the places on the desktop
        let got = choose_list(false, Some(XRANDR), drm);
        assert_eq!(got[0].connector, "eDP-1");
        assert_eq!(got[1].at, Some((1920, 0)));
    }

    #[test]
    fn xrandr_is_asked_for_the_current_state_and_only_when_wanted() {
        // (--current: no probing of the displays, which can make them blink)
        assert_eq!(XRANDR_ARGS, ["--current"]);
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let wanted = std::sync::Arc::new(AtomicBool::new(false));
        let watch = watch_with(
            move || {
                counted.fetch_add(1, Ordering::Relaxed);
                Vec::new()
            },
            wanted.clone(),
            std::time::Duration::from_millis(20),
            || {},
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
        // once when the window opens; not again while nothing wants it
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        wanted.store(true, Ordering::Relaxed);
        // (asked again and again while wanted - every 20 ms; up to 10 s for
        // three more on a busy machine, where the thread may wait its turn)
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while calls.load(Ordering::Relaxed) <= 3 && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(calls.load(Ordering::Relaxed) > 3, "not asked while wanted");
        drop(watch);
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
        // two of that name, its connector gone: no guess (review 10: the
        // view opened on the first of two "DELL U2723QE"s)
        let two = vec![
            Monitor {
                connector: "DP-1".into(),
                name: "DELL U2723QE".into(),
                ..Default::default()
            },
            Monitor {
                connector: "DP-2".into(),
                name: "DELL U2723QE".into(),
                ..Default::default()
            },
        ];
        assert_eq!(find(&two, "HDMI-3", "DELL U2723QE"), None);
        assert!(ambiguous(&two, "HDMI-3", "DELL U2723QE"));
        // its connector still there: that one, whatever the names
        assert_eq!(
            find(&two, "DP-2", "DELL U2723QE").map(|m| m.connector.as_str()),
            Some("DP-2")
        );
        assert!(!ambiguous(&two, "DP-2", "DELL U2723QE"));
    }
}
