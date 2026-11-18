//! The system's sound mixer through `pactl` - PulseAudio, and PipeWire
//! through `pipewire-pulse` (the same protocol): outputs and inputs with
//! their volume, mute, port and which one is the default; the applications
//! playing and recording, their volume, mute and device; the cards and their
//! profiles (a Bluetooth headset: A2DP or headset). As axross's Audio Mixer
//! (`core/system_mixer.py`), from which the model, the parsing of `pactl -f
//! json` and the rules of the commands are taken:
//!
//! * nothing passes through a shell; every command puts `--` before the
//!   subcommand, and a name that starts with `-` or holds a control
//!   character is refused (it would be read as an option);
//! * volumes are clamped to 0-150 %; setting one keeps the balance between
//!   channels;
//! * every `pactl` has a time limit (a hanging one wedges nothing);
//! * `pactl subscribe` runs alongside: a burst of events is one refresh.
//!
//! The work runs on a thread of its own; the window reads [`MixerView`].

use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const VOLUME_NORM: f64 = 65536.0;
pub const MAX_PERCENT: f64 = 150.0;
const MAX_NAME: usize = 512;
const INVALID_INDEX: i64 = u32::MAX as i64;
const TIMEOUT: Duration = Duration::from_secs(5);
const COALESCE: Duration = Duration::from_millis(150);
const SUBSCRIBE_RESTARTS: u32 = 5;
const MIN_MAJOR: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sink,
    Source,
    SinkInput,
    SourceOutput,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Sink => "sink",
            Kind::Source => "source",
            Kind::SinkInput => "sink-input",
            Kind::SourceOutput => "source-output",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub name: String,
    pub percent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub name: String,
    pub description: String,
    pub available: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub kind: Kind,
    pub index: u32,
    pub name: String,
    pub description: String,
    pub channels: Vec<Channel>,
    pub muted: bool,
    pub state: String,
    pub ports: Vec<Port>,
    pub active_port: String,
    pub is_default: bool,
    pub is_monitor: bool,
    pub card_index: Option<u32>,
    pub bus: String,
    pub form_factor: String,
    /// The display it plays on, as the display says (its ELD: an HDMI /
    /// DisplayPort sink's `device.product.name`, a port's name for it in
    /// brackets); empty for the others.
    pub monitors: Vec<String>,
}

impl Device {
    pub fn volume_percent(&self) -> f64 {
        self.channels.iter().map(|c| c.percent).fold(0.0, f64::max)
    }

    pub fn label(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub kind: Kind,
    pub index: u32,
    pub application: String,
    pub media: String,
    pub device_index: Option<u32>,
    pub channels: Vec<Channel>,
    pub muted: bool,
    pub corked: bool,
    pub pid: Option<u32>,
}

impl Stream {
    pub fn volume_percent(&self) -> f64 {
        self.channels.iter().map(|c| c.percent).fold(0.0, f64::max)
    }

    /// A stream of this program (the player itself).
    pub fn is_own(&self) -> bool {
        self.pid == Some(std::process::id())
    }

    pub fn label(&self) -> String {
        if !self.application.is_empty() {
            self.application.clone()
        } else if !self.media.is_empty() {
            self.media.clone()
        } else {
            format!("Stream #{}", self.index)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub description: String,
    pub available: bool,
    pub priority: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub profiles: Vec<Profile>,
    pub active_profile: String,
    pub bus: String,
    /// Its ports with the display on each (HDMI / DisplayPort: the ELD's
    /// monitor name) and the profiles that use them.
    pub ports: Vec<CardPort>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardPort {
    pub name: String,
    /// The display on it (ELD), else empty.
    pub monitor: String,
    pub profiles: Vec<String>,
}

/// A display's name a sink or a port carries: `device.product.name` in
/// its properties, else the part in brackets of a port's description
/// ("HDMI / DisplayPort (LG TV)").
fn monitor_names(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let product = prop(v, "device.product.name");
    if !product.is_empty() {
        out.push(product);
    }
    let bracketed = |d: &str| {
        let (_, rest) = d.rsplit_once('(')?;
        let name = rest.strip_suffix(')')?.trim();
        (!name.is_empty()).then(|| name.to_owned())
    };
    let ports: Vec<Value> = match v.get("ports") {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(o)) => o.values().cloned().collect(),
        _ => Vec::new(),
    };
    for p in &ports {
        let name = prop(p, "device.product.name");
        if !name.is_empty() {
            out.push(name);
        } else if let Some(name) = bracketed(&text(p, "description")) {
            out.push(name);
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub sinks: Vec<Device>,
    pub sources: Vec<Device>,
    pub sink_inputs: Vec<Stream>,
    pub source_outputs: Vec<Stream>,
    pub cards: Vec<Card>,
    pub default_sink: String,
    pub default_source: String,
}

// -- parsing pactl -f json ----------------------------------------------------------------------

fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn index_of(v: &Value, key: &str) -> Option<u32> {
    int(v.get(key))
        .filter(|i| (0..INVALID_INDEX).contains(i))
        .map(|i| i as u32)
}

fn boolean(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.to_ascii_lowercase().as_str(), "yes" | "true" | "1"),
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
}

fn props(v: &Value) -> &serde_json::Map<String, Value> {
    static EMPTY: std::sync::OnceLock<serde_json::Map<String, Value>> = std::sync::OnceLock::new();
    v.get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| EMPTY.get_or_init(Default::default))
}

fn prop(v: &Value, key: &str) -> String {
    props(v)
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn channels(v: &Value) -> Vec<Channel> {
    let Some(volume) = v.get("volume").and_then(Value::as_object) else {
        return Vec::new();
    };
    volume
        .iter()
        .filter_map(|(name, entry)| {
            let percent = match int(entry.get("value")) {
                Some(raw) => raw as f64 * 100.0 / VOLUME_NORM,
                None => entry
                    .get("value_percent")?
                    .as_str()?
                    .trim()
                    .trim_end_matches('%')
                    .trim()
                    .parse()
                    .ok()?,
            };
            Some(Channel {
                name: name.clone(),
                percent: (percent * 100.0).round() / 100.0,
            })
        })
        .collect()
}

fn ports(v: &Value) -> Vec<Port> {
    let list: Vec<Value> = match v.get("ports") {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Object(o)) => o
            .iter()
            .map(|(k, p)| {
                let mut p = p.clone();
                if let Value::Object(m) = &mut p {
                    m.insert("name".into(), Value::String(k.clone()));
                }
                p
            })
            .collect(),
        _ => Vec::new(),
    };
    list.iter()
        .filter(|p| !text(p, "name").is_empty())
        .map(|p| Port {
            name: text(p, "name"),
            description: text(p, "description"),
            available: match text(p, "availability").to_ascii_lowercase().as_str() {
                "available" => Some(true),
                "not available" => Some(false),
                _ => None,
            },
        })
        .collect()
}

fn list(raw: &str, what: &str) -> Result<Vec<Value>, String> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(items)) => Ok(items.into_iter().filter(Value::is_object).collect()),
        Ok(_) => Err(format!("pactl's list of {what} is no list")),
        Err(e) => Err(format!("pactl's list of {what} is no JSON ({e})")),
    }
}

pub fn parse_devices(raw: &str, kind: Kind, default: &str) -> Result<Vec<Device>, String> {
    let mut out: Vec<Device> = list(raw, kind.key())?
        .iter()
        .filter_map(|e| {
            let name = text(e, "name");
            let monitor_of = if kind == Kind::Source {
                text(e, "monitor_of_sink")
            } else {
                String::new()
            };
            Some(Device {
                kind,
                index: index_of(e, "index")?,
                description: {
                    let d = text(e, "description");
                    if d.is_empty() {
                        prop(e, "device.description")
                    } else {
                        d
                    }
                },
                channels: channels(e),
                muted: boolean(e.get("mute")),
                state: text(e, "state").to_ascii_lowercase(),
                ports: ports(e),
                active_port: text(e, "active_port"),
                is_default: !name.is_empty() && name == default,
                is_monitor: (!monitor_of.is_empty() && monitor_of != "n/a")
                    || prop(e, "device.class") == "monitor",
                card_index: None,
                bus: prop(e, "device.bus"),
                form_factor: prop(e, "device.form_factor"),
                monitors: monitor_names(e),
                name,
            })
        })
        .collect();
    out.sort_by_key(|d| d.index);
    Ok(out)
}

pub fn parse_streams(raw: &str, kind: Kind) -> Result<Vec<Stream>, String> {
    let device_key = if kind == Kind::SinkInput {
        "sink"
    } else {
        "source"
    };
    let mut out: Vec<Stream> = list(raw, kind.key())?
        .iter()
        .filter_map(|e| {
            let binary = prop(e, "application.process.binary");
            let application = prop(e, "application.name");
            Some(Stream {
                kind,
                index: index_of(e, "index")?,
                application: if application.is_empty() {
                    binary
                } else {
                    application
                },
                media: prop(e, "media.name"),
                device_index: index_of(e, device_key),
                channels: channels(e),
                muted: boolean(e.get("mute")),
                corked: boolean(e.get("corked")),
                pid: prop(e, "application.process.id").parse().ok(),
            })
        })
        .collect();
    out.sort_by_key(|s| s.index);
    Ok(out)
}

pub fn parse_cards(raw: &str) -> Result<Vec<Card>, String> {
    let mut out: Vec<Card> = list(raw, "cards")?
        .iter()
        .filter_map(|e| {
            let mut profiles: Vec<Profile> = e
                .get("profiles")
                .and_then(Value::as_object)
                .map(|o| {
                    o.iter()
                        .filter(|(_, p)| p.is_object())
                        .map(|(name, p)| Profile {
                            name: name.clone(),
                            description: {
                                let d = text(p, "description");
                                if d.is_empty() { name.clone() } else { d }
                            },
                            available: p.get("available").is_none_or(|a| boolean(Some(a))),
                            priority: int(p.get("priority")).unwrap_or(0),
                        })
                        .collect()
                })
                .unwrap_or_default();
            profiles.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.name.cmp(&b.name)));
            let name = text(e, "name");
            Some(Card {
                index: index_of(e, "index")?,
                description: {
                    let d = prop(e, "device.description");
                    if d.is_empty() { name.clone() } else { d }
                },
                name,
                profiles,
                active_profile: text(e, "active_profile"),
                bus: prop(e, "device.bus"),
                ports: match e.get("ports") {
                    Some(Value::Object(o)) => {
                        o.iter().map(|(k, p)| (k.clone(), p)).collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                }
                .into_iter()
                .map(|(port, p)| CardPort {
                    monitor: monitor_names(&serde_json::json!({
                        "properties": p.get("properties").cloned().unwrap_or_default()
                    }))
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
                    profiles: p
                        .get("profiles")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    name: port,
                })
                .collect(),
            })
        })
        .collect();
    out.sort_by_key(|c| c.index);
    Ok(out)
}

/// The card a device belongs to: the same PipeWire device id, the same ALSA
/// card, or the card's name inside the device's.
fn card_for(
    device_name: &str,
    device: &Value,
    cards: &[Card],
    card_values: &[Value],
) -> Option<u32> {
    let dev_id = prop(device, "device.id");
    let alsa = prop(device, "alsa.card");
    let props_of = |c: &Card| {
        card_values
            .iter()
            .find(|v| index_of(v, "index") == Some(c.index))
    };
    if !dev_id.is_empty()
        && let Some(c) = cards
            .iter()
            .find(|c| props_of(c).is_some_and(|v| prop(v, "object.id") == dev_id))
    {
        return Some(c.index);
    }
    if !alsa.is_empty()
        && let Some(c) = cards
            .iter()
            .find(|c| props_of(c).is_some_and(|v| prop(v, "alsa.card") == alsa))
    {
        return Some(c.index);
    }
    let rest = device_name.split_once('.').map(|(_, r)| r).unwrap_or("");
    cards
        .iter()
        .find(|c| {
            let key = c.name.split_once("_card.").map(|(_, k)| k).unwrap_or("");
            !key.is_empty() && (rest == key || rest.starts_with(&format!("{key}.")))
        })
        .map(|c| c.index)
}

/// The texts of one refresh: sinks, sources, sink-inputs, source-outputs,
/// cards (JSON) and the two defaults.
pub fn build_snapshot(
    outputs: &std::collections::HashMap<&str, String>,
) -> Result<Snapshot, String> {
    let get = |k: &str| outputs.get(k).map(String::as_str).unwrap_or("[]");
    let default_sink = outputs
        .get("default-sink")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let default_source = outputs
        .get("default-source")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    let cards = parse_cards(get("cards"))?;
    let card_values = list(get("cards"), "cards")?;
    let link = |raw: &str, kind: Kind, default: &str| -> Result<Vec<Device>, String> {
        let values = list(raw, kind.key())?;
        let mut devices = parse_devices(raw, kind, default)?;
        for d in &mut devices {
            if let Some(v) = values
                .iter()
                .find(|v| index_of(v, "index") == Some(d.index))
            {
                d.card_index = card_for(&d.name, v, &cards, &card_values);
            }
        }
        Ok(devices)
    };
    Ok(Snapshot {
        sinks: link(get("sinks"), Kind::Sink, &default_sink)?,
        sources: link(get("sources"), Kind::Source, &default_source)?,
        sink_inputs: parse_streams(get("sink-inputs"), Kind::SinkInput)?,
        source_outputs: parse_streams(get("source-outputs"), Kind::SourceOutput)?,
        cards,
        default_sink,
        default_source,
    })
}

// -- commands -----------------------------------------------------------------------------------

/// A device, card, port or profile name that is safe as one pactl argument.
pub fn valid_name(name: &str, what: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err(format!("the {what} is empty"));
    }
    if name.starts_with('-') {
        return Err(format!(
            "the {what} {name:?} starts with '-' and would be read as an option"
        ));
    }
    if name.len() > MAX_NAME || name.chars().any(|c| (c as u32) < 32 || c as u32 == 127) {
        return Err(format!(
            "the {what} {:?} is not a valid name",
            name.chars().take(60).collect::<String>()
        ));
    }
    Ok(name.to_owned())
}

pub fn clamp_percent(percent: f64) -> Result<f64, String> {
    if percent.is_nan() {
        return Err("NaN is not a volume".into());
    }
    Ok(percent.clamp(0.0, MAX_PERCENT))
}

fn raw(percent: f64) -> String {
    ((percent.clamp(0.0, MAX_PERCENT) * VOLUME_NORM / 100.0).round() as i64).to_string()
}

/// The VOLUME arguments: the loudest channel to `percent`, the balance kept.
pub fn volume_values(channels: &[Channel], percent: f64) -> Result<Vec<String>, String> {
    let target = clamp_percent(percent)?;
    let peak = channels.iter().map(|c| c.percent).fold(0.0, f64::max);
    if channels.len() < 2 || peak <= 0.0 {
        return Ok(vec![raw(target)]);
    }
    Ok(channels
        .iter()
        .map(|c| raw(c.percent * target / peak))
        .collect())
}

/// What the window asks of the mixer.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    DeviceVolume {
        kind: Kind,
        name: String,
        channels: Vec<Channel>,
        percent: f64,
    },
    StreamVolume {
        kind: Kind,
        index: u32,
        channels: Vec<Channel>,
        percent: f64,
    },
    DeviceMute {
        kind: Kind,
        name: String,
        muted: bool,
    },
    StreamMute {
        kind: Kind,
        index: u32,
        muted: bool,
    },
    Default {
        kind: Kind,
        name: String,
    },
    Move {
        kind: Kind,
        index: u32,
        device: String,
    },
    Port {
        kind: Kind,
        device: String,
        port: String,
    },
    Profile {
        card: String,
        profile: String,
    },
}

impl Action {
    /// The pactl arguments, and what it does in words (for an error).
    pub fn command(&self) -> Result<(Vec<String>, String), String> {
        let s = |x: &str| x.to_owned();
        Ok(match self {
            Action::DeviceVolume {
                kind,
                name,
                channels,
                percent,
            } => {
                let mut args = vec![
                    s("--"),
                    format!("set-{}-volume", kind.key()),
                    valid_name(name, "device name")?,
                ];
                args.extend(volume_values(channels, *percent)?);
                (args, format!("set the volume of {name}"))
            }
            Action::StreamVolume {
                kind,
                index,
                channels,
                percent,
            } => {
                let mut args = vec![
                    s("--"),
                    format!("set-{}-volume", kind.key()),
                    index.to_string(),
                ];
                args.extend(volume_values(channels, *percent)?);
                (args, "set the volume of the stream".into())
            }
            Action::DeviceMute { kind, name, muted } => (
                vec![
                    s("--"),
                    format!("set-{}-mute", kind.key()),
                    valid_name(name, "device name")?,
                    s(if *muted { "1" } else { "0" }),
                ],
                format!("mute {name}"),
            ),
            Action::StreamMute { kind, index, muted } => (
                vec![
                    s("--"),
                    format!("set-{}-mute", kind.key()),
                    index.to_string(),
                    s(if *muted { "1" } else { "0" }),
                ],
                "mute the stream".into(),
            ),
            Action::Default { kind, name } => {
                if !matches!(kind, Kind::Sink | Kind::Source) {
                    return Err("only an output or an input can be the default".into());
                }
                (
                    vec![
                        s("--"),
                        format!("set-default-{}", kind.key()),
                        valid_name(name, "device name")?,
                    ],
                    format!("make {name} the default"),
                )
            }
            Action::Move {
                kind,
                index,
                device,
            } => {
                if !matches!(kind, Kind::SinkInput | Kind::SourceOutput) {
                    return Err("only a stream can be moved".into());
                }
                (
                    vec![
                        s("--"),
                        format!("move-{}", kind.key()),
                        index.to_string(),
                        valid_name(device, "device name")?,
                    ],
                    format!("move the stream to {device}"),
                )
            }
            Action::Port { kind, device, port } => (
                vec![
                    s("--"),
                    format!("set-{}-port", kind.key()),
                    valid_name(device, "device name")?,
                    valid_name(port, "port name")?,
                ],
                format!("switch {device} to {port}"),
            ),
            Action::Profile { card, profile } => (
                vec![
                    s("--"),
                    s("set-card-profile"),
                    valid_name(card, "card name")?,
                    valid_name(profile, "profile name")?,
                ],
                format!("switch {card} to {profile}"),
            ),
        })
    }
}

/// `Could not <what>: <reason>.` from pactl's stderr.
pub fn error_sentence(what: &str, stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let low = line.to_ascii_lowercase();
    let reason = [
        ("no such entity", "it does not exist (any more)"),
        ("access denied", "the sound server refused it"),
        (
            "invalid argument",
            "the sound server did not accept the value",
        ),
        ("not supported", "the sound server does not support it"),
    ]
    .iter()
    .find(|(k, _)| low.contains(k))
    .map(|(_, t)| (*t).to_owned())
    .unwrap_or_else(|| {
        if line.is_empty() {
            "pactl failed without saying why".into()
        } else {
            line.to_owned()
        }
    });
    format!("Could not {what}: {}.", reason.trim_end_matches('.'))
}

// -- running pactl ------------------------------------------------------------------------------

/// The pactl to run: AXROSS_AMP_PACTL (tests), else `pactl` on the PATH.
fn pactl() -> String {
    std::env::var("AXROSS_AMP_PACTL").unwrap_or_else(|_| "pactl".into())
}

/// Run pactl with `args`: (exit ok, stdout, stderr); killed after TIMEOUT.
pub fn run(args: &[String]) -> Result<(bool, String, String), String> {
    let mut child: Child = Command::new(pactl())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C")
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "pactl is not installed".to_owned()
            } else {
                e.to_string()
            }
        })?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err = thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "pactl {} took longer than {} s",
                args.join(" "),
                TIMEOUT.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(5));
    };
    Ok((
        status.success(),
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    ))
}

fn is_connection_failure(stderr: &str) -> bool {
    let low = stderr.to_ascii_lowercase();
    low.contains("connection failure")
        || low.contains("connection refused")
        || low.contains("no such file or directory")
}

pub fn parse_version(text: &str) -> Option<(u32, u32)> {
    let digits: String = text.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    let mut parts = digits
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty());
    Some((
        parts.next()?.parse().ok()?,
        parts.next().and_then(|m| m.parse().ok()).unwrap_or(0),
    ))
}

/// One refresh: every list and the defaults.
/// The lists as they are now (one round of pactl calls).
pub fn refresh() -> Result<Snapshot, String> {
    let commands: [(&str, &[&str]); 7] = [
        ("sinks", &["-f", "json", "list", "sinks"]),
        ("sources", &["-f", "json", "list", "sources"]),
        ("sink-inputs", &["-f", "json", "list", "sink-inputs"]),
        ("source-outputs", &["-f", "json", "list", "source-outputs"]),
        ("cards", &["-f", "json", "list", "cards"]),
        ("default-sink", &["get-default-sink"]),
        ("default-source", &["get-default-source"]),
    ];
    let mut outputs = std::collections::HashMap::new();
    for (key, args) in commands {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let (ok, out, err) = run(&args)?;
        if !ok {
            if is_connection_failure(&err) {
                return Err(
                    "No sound server answers (PulseAudio or PipeWire is not running).".into(),
                );
            }
            return Err(error_sentence(&format!("read the {key}"), &err));
        }
        outputs.insert(key, out);
    }
    build_snapshot(&outputs)
}

// -- the mixer ----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Starting,
    /// No pactl, a pactl too old for JSON, or no server: the sentence.
    Unavailable(String),
    Ready,
}

#[derive(Clone, Debug)]
pub struct MixerView {
    pub status: Status,
    pub snapshot: Snapshot,
    /// The last command that failed, in words.
    pub error: String,
    pub refreshes: u64,
}

enum Job {
    Refresh,
    Act(Action),
    /// The newest volume asked for this device or stream (in `Volumes`).
    Volume(String),
    Quit,
}

/// Volumes asked for and not yet set, by what they are for: a slider
/// dragged asks many times a second, pactl sets only the newest.
type Volumes = Arc<Mutex<std::collections::HashMap<String, Action>>>;

/// What a volume action is for ("sink:name", "sink-input#7"); None for others.
fn volume_target(action: &Action) -> Option<String> {
    match action {
        Action::DeviceVolume { kind, name, .. } => Some(format!("{}:{name}", kind.key())),
        Action::StreamVolume { kind, index, .. } => Some(format!("{}#{index}", kind.key())),
        _ => None,
    }
}

/// The mixer: a worker thread runs pactl, a second follows `pactl subscribe`.
pub struct Mixer {
    view: Arc<Mutex<MixerView>>,
    jobs: crossbeam_channel::Sender<Job>,
    volumes: Volumes,
}

impl Mixer {
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Mixer {
        let view = Arc::new(Mutex::new(MixerView {
            status: Status::Starting,
            snapshot: Snapshot::default(),
            error: String::new(),
            refreshes: 0,
        }));
        let (jobs, rx) = crossbeam_channel::unbounded::<Job>();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let worker_view = view.clone();
        let worker_wake = wake.clone();
        let subscribe_jobs = jobs.clone();
        let volumes: Volumes = Arc::default();
        let worker_volumes = volumes.clone();
        thread::Builder::new()
            .name("amp-mixer".into())
            .spawn(move || worker(rx, worker_view, worker_wake, subscribe_jobs, worker_volumes))
            .expect("the mixer thread starts");
        let _ = jobs.send(Job::Refresh);
        Mixer {
            view,
            jobs,
            volumes,
        }
    }

    pub fn view(&self) -> MixerView {
        self.view
            .lock()
            .map(|v| v.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    /// Do `action` (on the worker; a volume asked again before the last one
    /// was set replaces it).
    pub fn act(&self, action: Action) {
        if let Some(target) = volume_target(&action) {
            let waiting = self
                .volumes
                .lock()
                .map(|mut v| v.insert(target.clone(), action).is_some())
                .unwrap_or(false);
            if !waiting {
                let _ = self.jobs.send(Job::Volume(target));
            }
            return;
        }
        let _ = self.jobs.send(Job::Act(action));
    }

    pub fn refresh(&self) {
        let _ = self.jobs.send(Job::Refresh);
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        let _ = self.jobs.send(Job::Quit);
    }
}

fn worker(
    rx: crossbeam_channel::Receiver<Job>,
    view: Arc<Mutex<MixerView>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    jobs: crossbeam_channel::Sender<Job>,
    volumes: Volumes,
) {
    let newest = |target: &str| volumes.lock().ok().and_then(|mut v| v.remove(target));
    let set = |f: &dyn Fn(&mut MixerView)| {
        if let Ok(mut v) = view.lock() {
            f(&mut v);
        }
        wake();
    };
    // pactl there, and new enough to print JSON?
    match run(&["--version".to_owned()]) {
        Err(why) => {
            return set(&|v| {
                v.status = Status::Unavailable(format!("The system mixer needs pactl: {why}."))
            });
        }
        Ok((_, out, _)) => {
            if let Some((major, _)) = parse_version(&out)
                && major < MIN_MAJOR
            {
                return set(&|v| {
                    v.status = Status::Unavailable(format!(
                        "pactl {major} is too old: the system mixer needs pactl 16 or newer."
                    ))
                });
            }
        }
    }
    let subscriber = Subscriber::start(jobs.clone());
    while let Ok(job) = rx.recv() {
        match job {
            Job::Quit => break,
            Job::Refresh => {
                // a burst of refreshes is one
                while let Ok(next) = rx.try_recv() {
                    match next {
                        Job::Act(a) => act(&a, &set),
                        Job::Volume(target) => {
                            if let Some(a) = newest(&target) {
                                act(&a, &set);
                            }
                        }
                        Job::Quit => {
                            subscriber.stop();
                            return;
                        }
                        Job::Refresh => {}
                    }
                }
                let result = refresh();
                set(&|v| match &result {
                    Ok(snapshot) => {
                        v.snapshot = snapshot.clone();
                        v.status = Status::Ready;
                        v.refreshes += 1;
                    }
                    Err(why) => v.status = Status::Unavailable(why.clone()),
                });
            }
            Job::Act(action) => {
                act(&action, &set);
                let _ = jobs.send(Job::Refresh);
            }
            Job::Volume(target) => {
                if let Some(action) = newest(&target) {
                    act(&action, &set);
                    let _ = jobs.send(Job::Refresh);
                }
            }
        }
    }
    subscriber.stop();
}

/// How the worker changes the shared view: it is handed a change to make.
type SetView<'a> = &'a dyn Fn(&dyn Fn(&mut MixerView));

fn act(action: &Action, set: SetView<'_>) {
    let outcome = action.command().and_then(|(args, what)| match run(&args) {
        Ok((true, _, _)) => Ok(()),
        Ok((false, _, err)) => Err(error_sentence(&what, &err)),
        Err(why) => Err(format!("Could not {what}: {why}.")),
    });
    set(&|v| v.error = outcome.clone().err().unwrap_or_default());
}

/// `pactl subscribe`: an event that changes the lists asks for a refresh
/// (a burst within COALESCE is one); a subscribe that dies is started again
/// a few times.
struct Subscriber {
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Subscriber {
    fn start(jobs: crossbeam_channel::Sender<Job>) -> Subscriber {
        let child = Arc::new(Mutex::new(None::<Child>));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (c, s) = (child.clone(), stop.clone());
        thread::Builder::new()
            .name("amp-mixer-subscribe".into())
            .spawn(move || {
                let mut restarts = 0;
                while !s.load(std::sync::atomic::Ordering::Relaxed)
                    && restarts <= SUBSCRIBE_RESTARTS
                {
                    let started = Instant::now();
                    let Ok(mut process) = Command::new(pactl())
                        .arg("subscribe")
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .env("LC_ALL", "C")
                        .spawn()
                    else {
                        return;
                    };
                    let Some(stdout) = process.stdout.take() else {
                        let _ = process.kill();
                        let _ = process.wait();
                        return;
                    };
                    if let Ok(mut slot) = c.lock() {
                        *slot = Some(process);
                    }
                    // (stopped while it started: stop() found no process to end)
                    if s.load(std::sync::atomic::Ordering::Relaxed) {
                        if let Ok(mut slot) = c.lock()
                            && let Some(mut p) = slot.take()
                        {
                            let _ = p.kill();
                            let _ = p.wait();
                        }
                        return;
                    }
                    let mut last: Option<Instant> = None;
                    for line in BufReader::new(stdout).lines() {
                        let Ok(line) = line else { break };
                        if wants_refresh(&line) && last.is_none_or(|t| t.elapsed() >= COALESCE) {
                            last = Some(Instant::now());
                            let jobs = jobs.clone();
                            // (the burst that follows within COALESCE is one refresh)
                            thread::spawn(move || {
                                thread::sleep(COALESCE);
                                let _ = jobs.send(Job::Refresh);
                            });
                        }
                    }
                    if let Ok(mut slot) = c.lock()
                        && let Some(mut p) = slot.take()
                    {
                        let _ = p.kill();
                        let _ = p.wait();
                    }
                    restarts = if started.elapsed() > Duration::from_secs(30) {
                        1
                    } else {
                        restarts + 1
                    };
                    thread::sleep(Duration::from_secs(1));
                }
            })
            .expect("the subscribe thread starts");
        Subscriber { child, stop }
    }

    fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut slot) = self.child.lock()
            && let Some(mut p) = slot.take()
        {
            let _ = p.kill();
            let _ = p.wait();
        }
    }
}

/// A line of `pactl subscribe` about the lists ("Event 'change' on sink #56").
pub fn wants_refresh(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("Event '")
        && [
            "sink",
            "source",
            "sink-input",
            "source-output",
            "card",
            "server",
        ]
        .iter()
        .any(|f| line.contains(&format!(" on {f} #")) || line.ends_with(&format!(" on {f}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_would_be_options_or_carry_controls_are_refused() {
        assert!(valid_name("-x", "device name").is_err());
        assert!(valid_name("a\nb", "device name").is_err());
        assert!(valid_name("", "device name").is_err());
        assert_eq!(
            valid_name("alsa_output.pci", "device name").unwrap(),
            "alsa_output.pci"
        );
        let action = Action::DeviceVolume {
            kind: Kind::Sink,
            name: "--help".into(),
            channels: vec![],
            percent: 50.0,
        };
        assert!(action.command().is_err());
    }

    #[test]
    fn a_volume_keeps_the_balance_and_is_clamped() {
        let ch = vec![
            Channel {
                name: "front-left".into(),
                percent: 60.0,
            },
            Channel {
                name: "front-right".into(),
                percent: 30.0,
            },
        ];
        assert_eq!(volume_values(&ch, 100.0).unwrap(), vec!["65536", "32768"]);
        assert_eq!(volume_values(&ch, 999.0).unwrap()[0], "98304"); // 150 %
        assert!(volume_values(&ch, f64::NAN).is_err());
        let (args, _) = Action::Move {
            kind: Kind::SinkInput,
            index: 7,
            device: "bt".into(),
        }
        .command()
        .unwrap();
        assert_eq!(args, vec!["--", "move-sink-input", "7", "bt"]);
        assert!(
            Action::Move {
                kind: Kind::Sink,
                index: 7,
                device: "bt".into()
            }
            .command()
            .is_err()
        );
    }

    #[test]
    fn events_and_versions_and_errors() {
        assert!(wants_refresh("Event 'change' on sink #56"));
        assert!(wants_refresh("Event 'new' on sink-input #120"));
        assert!(!wants_refresh("Event 'change' on client #3"));
        assert_eq!(
            parse_version("pactl 16.1\nCompiled with libpulse 16.1.0"),
            Some((16, 1))
        );
        assert_eq!(
            error_sentence("mute x", "Failure: No such entity\n"),
            "Could not mute x: it does not exist (any more)."
        );
    }
}
