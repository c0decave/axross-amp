//! Bluetooth through BlueZ on the system D-Bus: the adapters and the
//! devices BlueZ knows, kept up to date from its signals, and what can be
//! done with them - power, scan, pair, trust, connect, disconnect, forget.
//! As axross's Bluetooth panel (`core/bluetooth.py`), from which the rules
//! are taken:
//!
//! * one operation per adapter or device at a time; a result arrives as a
//!   [`Finished`] with "" or a sentence for the user ([`error_text`]);
//! * the pairing agent (`org.bluez.Agent1`) is registered only while a
//!   pairing of ours runs. Every question BlueZ asks it (confirm a passkey,
//!   type a PIN, allow a service) is held until the user answers; nothing
//!   is accepted without an answer. Questions from anyone but BlueZ are
//!   refused;
//! * signals count only from BlueZ itself (its unique name), so another
//!   program on the bus cannot forge a device;
//! * a name a device chose is shown without control and format characters
//!   ([`device_text`]).
//!
//! The bus is the system bus, or the address in `AXROSS_AMP_BLUEZ_BUS`
//! (the tests' private bus with axross's fake BlueZ). The work runs on
//! threads of its own; the window reads [`BluetoothView`].

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use zbus::blocking::Connection;
use zbus::message::Type as MessageType;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{MatchRule, Message};

pub const SERVICE: &str = "org.bluez";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
const AGENT: &str = "org.bluez.Agent1";
const BUS: &str = "org.freedesktop.DBus";
/// Our agent answers every kind of question, so BlueZ picks the strongest
/// pairing method the device offers; each question goes to the user.
const AGENT_CAPABILITY: &str = "KeyboardDisplay";
/// The longest a call waits: pairing waits for the device and the user.
const CALL_TIMEOUT: Duration = Duration::from_secs(125);
/// A scan runs at most this long (it keeps the radio busy).
pub const SCAN_LIMIT: Duration = Duration::from_secs(120);
/// The longest name a device can tell (Bluetooth Core: 248 bytes).
pub const MAX_NAME: usize = 248;
/// Results kept for the window (the newest).
const FINISHED_KEPT: usize = 32;

/// Service UUIDs of a device that takes sound: A2DP sink, headset, hands-free.
const AUDIO_SINK_UUIDS: [&str; 3] = [
    "0000110b-0000-1000-8000-00805f9b34fb",
    "00001108-0000-1000-8000-00805f9b34fb",
    "0000111e-0000-1000-8000-00805f9b34fb",
];
const AUDIO_ICONS: [&str; 3] = ["audio-card", "audio-headset", "audio-headphones"];
const AUDIO_KINDS: [&str; 5] = ["Speaker", "Headset", "Headphones", "Earbuds", "Hearing Aid"];

// -- pure helpers -----------------------------------------------------------------------

/// A D-Bus error from BlueZ as a sentence for the user.
pub fn error_text(name: &str, message: &str) -> String {
    let message = message.trim();
    let low = message.to_lowercase();
    const RFKILL: &str = "Bluetooth is blocked (rfkill). Unblock it with the wireless switch or \
                          `rfkill unblock bluetooth`.";
    const NOT_RUNNING: &str = "The Bluetooth service (bluetoothd) is not running.";
    const SLOW: &str = "BlueZ did not answer in time.";
    if low.contains("rfkill") {
        return RFKILL.into();
    }
    if low.contains("profile-unavailable") {
        return "The device offers no profile this computer can use. For speakers and \
                headphones the sound server needs its Bluetooth support (PipeWire's or \
                PulseAudio's Bluetooth module)."
            .into();
    }
    if low.contains("page-timeout") || low.contains("page timeout") {
        return "The device did not answer - is it switched on and in range?".into();
    }
    let text = match name {
        "org.bluez.Error.NotReady" => {
            "The Bluetooth adapter is not ready - switch the adapter on first."
        }
        "org.bluez.Error.AuthenticationFailed" => {
            "Pairing failed: the PIN or passkey did not match, or the device refused."
        }
        "org.bluez.Error.AuthenticationRejected" => "Pairing was rejected.",
        "org.bluez.Error.AuthenticationCanceled" => "Pairing was canceled.",
        "org.bluez.Error.AuthenticationTimeout" => {
            "Pairing timed out - the device or the answer did not come in time."
        }
        "org.bluez.Error.ConnectionAttemptFailed" => {
            "The connection attempt failed - is the device switched on and in range?"
        }
        "org.bluez.Error.InProgress" => {
            "Another operation on this device is still running - wait for it to finish."
        }
        "org.bluez.Error.AlreadyExists" => "The device is already paired.",
        "org.bluez.Error.AlreadyConnected" => "The device is already connected.",
        "org.bluez.Error.NotConnected" => "The device is not connected.",
        "org.bluez.Error.DoesNotExist" => "The device is no longer known to BlueZ.",
        "org.bluez.Error.NotAvailable" => "This is not available on the device or adapter.",
        "org.bluez.Error.NotSupported" => "The device or adapter does not support this.",
        "org.bluez.Error.NotAuthorized" => "BlueZ refused: not authorized.",
        "org.bluez.Error.NotPermitted" => "The device does not allow this.",
        "org.bluez.Error.Blocked" => RFKILL,
        "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.Timeout" => SLOW,
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner" => NOT_RUNNING,
        "org.freedesktop.DBus.Error.Disconnected" => "The connection to the system D-Bus was lost.",
        "org.freedesktop.DBus.Error.UnknownObject" => "The device or adapter is gone.",
        "org.freedesktop.DBus.Error.AccessDenied" => {
            "D-Bus denied the request - is your user allowed to use Bluetooth?"
        }
        "org.bluez.Error.Failed" => {
            return if message.is_empty() {
                "BlueZ reported a failure.".into()
            } else {
                format!("BlueZ reported a failure: {}", device_text(message))
            };
        }
        "org.bluez.Error.InvalidArguments" => {
            return format!(
                "BlueZ rejected the request as invalid: {}",
                device_text(message)
            );
        }
        _ => "",
    };
    if !text.is_empty() {
        if (name.ends_with("NotSupported")
            || name.ends_with("NotAvailable")
            || name.ends_with("AccessDenied"))
            && !message.is_empty()
        {
            return format!("{text} ({})", device_text(message));
        }
        return text.into();
    }
    match (name.is_empty(), message.is_empty()) {
        (true, true) => "The operation failed.".into(),
        (false, false) => format!("{name}: {}", device_text(message)),
        (false, true) => name.into(),
        (true, false) => device_text(message),
    }
}

/// Characters of the Unicode category Cf (format) that can reorder or hide
/// text: bidirectional overrides and isolates, zero-width characters.
fn is_format(c: char) -> bool {
    matches!(c,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}'
        | '\u{e0000}'..='\u{e007f}')
}

/// A name a device chose (Name, Alias) as it may be shown: without control
/// and format characters - a newline, a NUL or a right-to-left override
/// would forge or reorder the text around it - and at most [`MAX_NAME`]
/// characters.
pub fn device_text(value: &str) -> String {
    let clean = value
        .chars()
        .all(|c| c == ' ' || !(c.is_control() || c.is_whitespace() || is_format(c)));
    let text: String = if clean {
        value.to_owned()
    } else {
        value
            .chars()
            .map(|c| {
                if c.is_control() || c.is_whitespace() || is_format(c) {
                    ' '
                } else {
                    c
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    text.chars().take(MAX_NAME).collect()
}

/// "Speaker", "Headset", "Phone", ... from an exact GAP Appearance (a BLE
/// device's own word), BlueZ's Icon, the Appearance's category, else the
/// Class of Device; "Device" when none says anything.
pub fn device_kind(icon: &str, class: u32, appearance: Option<u16>) -> &'static str {
    let appearance = appearance.filter(|a| *a > 0);
    if let Some(a) = appearance {
        let exact = match a {
            0x03C1 => "Keyboard",
            0x03C2 => "Mouse",
            0x03C3 | 0x03C4 => "Game Controller",
            0x03C5 => "Tablet",
            0x0941 => "Earbuds",
            0x0942 => "Headset",
            0x0943 | 0x0944 => "Headphones",
            _ => "",
        };
        if !exact.is_empty() {
            return exact;
        }
    }
    let by_icon = match icon {
        "audio-card" => "Speaker",
        "audio-headset" => "Headset",
        "audio-headphones" => "Headphones",
        "phone" => "Phone",
        "computer" => "Computer",
        "input-keyboard" => "Keyboard",
        "input-mouse" => "Mouse",
        "input-gaming" => "Game Controller",
        "input-tablet" => "Tablet",
        "camera-video" | "camera-photo" => "Camera",
        "printer" => "Printer",
        "scanner" => "Scanner",
        "modem" => "Modem",
        "network-wireless" => "Network",
        "video-display" => "Display",
        "multimedia-player" => "Media Player",
        _ => "",
    };
    if !by_icon.is_empty() {
        return by_icon;
    }
    if let Some(a) = appearance {
        let category = match a >> 6 {
            0x01 => "Phone",
            0x02 => "Computer",
            0x03 => "Watch",
            0x04 => "Clock",
            0x05 | 0x28 => "Display",
            0x06 | 0x13 => "Remote Control",
            0x07 => "Eyeglasses",
            0x08 | 0x09 => "Tag",
            0x0A | 0x27 => "Media Player",
            0x0B => "Scanner",
            0x0C..=0x0E | 0x10 | 0x31 | 0x32 | 0x34..=0x37 => "Health Device",
            0x0F => "Input Device",
            0x11 | 0x12 | 0x51 => "Fitness Sensor",
            0x14 => "Network",
            0x15 => "Sensor",
            0x16 | 0x1F => "Light",
            0x17..=0x1B => "Climate Device",
            0x1C => "Access Control",
            0x21 => "Speaker",
            0x22 => "Audio Source",
            0x23 => "Vehicle",
            0x24 => "Appliance",
            0x25 => "Headphones",
            0x29 => "Hearing Aid",
            0x2A => "Game Controller",
            _ => "",
        };
        if !category.is_empty() {
            return category;
        }
    }
    if class == 0 {
        return "Device";
    }
    let major = (class >> 8) & 0x1F;
    let minor = (class >> 2) & 0x3F;
    match major {
        1 => "Computer",
        2 => "Phone",
        3 => "Network",
        4 => match minor {
            1 | 2 => "Headset",
            5 | 7 | 8 | 10 => "Speaker",
            6 => "Headphones",
            _ => "Audio Device",
        },
        5 if minor & 0x10 != 0 => "Keyboard",
        5 if minor & 0x20 != 0 => "Mouse",
        5 => "Input Device",
        6 => "Imaging",
        7 => "Wearable",
        8 => "Toy",
        9 => "Health",
        _ => "Device",
    }
}

/// The output among `outputs` (id, description) that is the device's: an id
/// that carries its address (PipeWire `bluez_output.AA_BB_..`, PulseAudio
/// `bluez_sink.aa_bb_..`), else a description equal to its name.
pub fn find_sink<'a>(
    outputs: &'a [(String, String)],
    address: &str,
    names: &[&str],
) -> Option<&'a (String, String)> {
    let address = address.to_lowercase();
    if !address.is_empty() {
        let keys = [address.replace(':', "_"), address.clone()];
        if let Some(found) = outputs
            .iter()
            .find(|(id, _)| keys.iter().any(|k| id.to_lowercase().contains(k.as_str())))
        {
            return Some(found);
        }
    }
    let names: Vec<String> = names
        .iter()
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    outputs
        .iter()
        .find(|(_, d)| names.contains(&d.trim().to_lowercase()))
}

// -- the model ----------------------------------------------------------------------------

/// A property as kept: what the window needs of D-Bus's types.
#[derive(Clone, Debug, PartialEq)]
enum Prop {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
    Other,
}

type Props = BTreeMap<String, Prop>;
type Ifaces = BTreeMap<String, Props>;

fn prop(value: &Value<'_>) -> Prop {
    match value {
        Value::Bool(b) => Prop::Bool(*b),
        Value::U8(n) => Prop::Int(*n as i64),
        Value::I16(n) => Prop::Int(*n as i64),
        Value::U16(n) => Prop::Int(*n as i64),
        Value::I32(n) => Prop::Int(*n as i64),
        Value::U32(n) => Prop::Int(*n as i64),
        Value::I64(n) => Prop::Int(*n),
        Value::Str(s) => Prop::Str(s.as_str().to_owned()),
        Value::ObjectPath(p) => Prop::Str(p.as_str().to_owned()),
        Value::Array(a) => Prop::List(
            a.iter()
                .filter_map(|v| match v {
                    Value::Str(s) => Some(s.as_str().to_owned()),
                    _ => None,
                })
                .collect(),
        ),
        Value::Value(inner) => prop(inner),
        _ => Prop::Other,
    }
}

fn props(raw: &HashMap<String, OwnedValue>) -> Props {
    raw.iter().map(|(k, v)| (k.clone(), prop(v))).collect()
}

fn get_str(p: &Props, key: &str) -> String {
    match p.get(key) {
        Some(Prop::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn get_bool(p: &Props, key: &str) -> bool {
    matches!(p.get(key), Some(Prop::Bool(true)))
}

fn get_int(p: &Props, key: &str) -> Option<i64> {
    match p.get(key) {
        Some(Prop::Int(n)) => Some(*n),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub address: String,
    pub name: String,
    pub alias: String,
    pub powered: bool,
    pub discovering: bool,
}

impl Adapter {
    fn from_props(path: &str, p: &Props) -> Adapter {
        Adapter {
            path: path.to_owned(),
            address: get_str(p, "Address"),
            name: device_text(&get_str(p, "Name")),
            alias: device_text(&get_str(p, "Alias")),
            powered: get_bool(p, "Powered"),
            discovering: get_bool(p, "Discovering"),
        }
    }

    pub fn display_name(&self) -> String {
        [&self.alias, &self.name, &self.address]
            .into_iter()
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| self.path.rsplit('/').next().unwrap_or("").to_owned())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub path: String,
    pub adapter: String,
    pub address: String,
    pub name: String,
    pub alias: String,
    pub icon: String,
    pub class: u32,
    pub appearance: Option<u16>,
    pub rssi: Option<i16>,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    pub blocked: bool,
    pub uuids: Vec<String>,
    pub battery: Option<u8>,
}

impl Device {
    fn from_props(path: &str, p: &Props, battery: Option<&Props>) -> Device {
        let adapter = get_str(p, "Adapter");
        Device {
            path: path.to_owned(),
            adapter: if adapter.is_empty() {
                path.rsplit_once('/')
                    .map(|(a, _)| a.to_owned())
                    .unwrap_or_default()
            } else {
                adapter
            },
            address: device_text(&get_str(p, "Address")),
            name: device_text(&get_str(p, "Name")),
            alias: device_text(&get_str(p, "Alias")),
            icon: get_str(p, "Icon"),
            class: get_int(p, "Class")
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            appearance: get_int(p, "Appearance").and_then(|n| u16::try_from(n).ok()),
            rssi: get_int(p, "RSSI")
                .filter(|n| (-127..=20).contains(n))
                .map(|n| n as i16),
            paired: get_bool(p, "Paired"),
            trusted: get_bool(p, "Trusted"),
            connected: get_bool(p, "Connected"),
            blocked: get_bool(p, "Blocked"),
            uuids: match p.get("UUIDs") {
                Some(Prop::List(l)) => l.iter().map(|u| u.to_lowercase()).collect(),
                _ => Vec::new(),
            },
            battery: battery
                .and_then(|b| get_int(b, "Percentage"))
                .and_then(|n| u8::try_from(n).ok())
                .filter(|n| *n <= 100),
        }
    }

    /// The alias the user gave, else the name the device told, else its
    /// address. (An alias BlueZ made from the address loses to a name that
    /// came later.)
    pub fn display_name(&self) -> String {
        let alias = if self.alias_from_address() {
            ""
        } else {
            self.alias.as_str()
        };
        [alias, &self.name, &self.alias, &self.address]
            .into_iter()
            .find(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_default()
    }

    fn alias_from_address(&self) -> bool {
        !self.alias.is_empty()
            && self.alias.replace('-', ":").to_uppercase() == self.address.to_uppercase()
    }

    /// True when the device told its name (BlueZ makes the alias of a
    /// nameless one from its address).
    pub fn named(&self) -> bool {
        !self.name.is_empty() || (!self.alias.is_empty() && !self.alias_from_address())
    }

    pub fn kind(&self) -> &'static str {
        device_kind(&self.icon, self.class, self.appearance)
    }

    /// A device that takes sound (speaker, headset, headphones).
    pub fn is_audio_output(&self) -> bool {
        self.uuids
            .iter()
            .any(|u| AUDIO_SINK_UUIDS.contains(&u.as_str()))
            || AUDIO_ICONS.contains(&self.icon.as_str())
            || AUDIO_KINDS.contains(&self.kind())
    }
}

/// What a scan looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// Classic and Low Energy.
    Auto,
    /// Classic (BR/EDR) only: speakers and headphones.
    BrEdr,
    /// Low Energy only.
    Le,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Transport::Auto => "auto",
            Transport::BrEdr => "bredr",
            Transport::Le => "le",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Power {
        adapter: String,
        on: bool,
    },
    Scan {
        adapter: String,
        transport: Transport,
    },
    StopScan {
        adapter: String,
    },
    Pair {
        device: String,
    },
    CancelPairing {
        device: String,
    },
    Trust {
        device: String,
        on: bool,
    },
    Connect {
        device: String,
    },
    Disconnect {
        device: String,
    },
    Remove {
        device: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    Power,
    Scan,
    StopScan,
    Pair,
    CancelPairing,
    Trust,
    Connect,
    Disconnect,
    Remove,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Power => "power",
            Op::Scan => "scan",
            Op::StopScan => "stop-scan",
            Op::Pair => "pair",
            Op::CancelPairing => "cancel-pairing",
            Op::Trust => "trust",
            Op::Connect => "connect",
            Op::Disconnect => "disconnect",
            Op::Remove => "remove",
        }
    }
}

/// An operation that ended: `error` is "" when it worked, else a sentence.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub serial: u64,
    pub path: String,
    pub op: Op,
    pub error: String,
    /// The scan ended by itself after [`SCAN_LIMIT`].
    pub timed_out: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentKind {
    /// Does the device show `passkey`?
    Confirm,
    /// Type the PIN the device wants.
    Pin,
    /// Type the 6-digit passkey the device shows.
    Passkey,
    /// Show `passkey` to type on the device (no answer).
    DisplayPasskey,
    /// Show `pin` to type on the device (no answer).
    DisplayPin,
    /// Allow the pairing.
    Authorize,
    /// Allow the service `uuid`.
    AuthorizeService,
}

/// A question BlueZ asks during a pairing.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentRequest {
    pub token: u64,
    pub kind: AgentKind,
    pub device: String,
    pub device_name: String,
    pub passkey: Option<u32>,
    pub pin: String,
    pub uuid: String,
    /// DisplayPasskey: how many digits were typed on the device so far.
    pub entered: Option<u16>,
}

impl AgentRequest {
    pub fn needs_answer(&self) -> bool {
        !matches!(self.kind, AgentKind::DisplayPasskey | AgentKind::DisplayPin)
    }
}

/// The user's answer to a question.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// Yes - with the PIN or the passkey typed, where one is asked.
    Accept(String),
    Reject,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Starting,
    /// No D-Bus (the reason).
    Unavailable(String),
    /// D-Bus there, bluetoothd not.
    NotRunning,
    NoAdapter,
    Ready,
    /// BlueZ answered the first question with an error.
    Error(String),
}

impl Status {
    /// What the window says ("" when ready).
    pub fn text(&self) -> String {
        match self {
            Status::Starting => "Asking BlueZ…".into(),
            Status::Unavailable(why) => format!(
                "The system D-Bus cannot be reached ({why}). Bluetooth is driven through BlueZ \
                 on the system D-Bus."
            ),
            Status::NotRunning => "The Bluetooth service (bluetoothd) is not running. Start it \
                                   (for example: systemctl start bluetooth); this page notices \
                                   when it runs."
                .into(),
            Status::NoAdapter => "No Bluetooth adapter found. Plug one in or unblock it \
                                  (rfkill); this page notices when it appears."
                .into(),
            Status::Ready => String::new(),
            Status::Error(why) => format!("BlueZ could not be read: {why}"),
        }
    }
}

/// What the window shows.
#[derive(Clone, Debug)]
pub struct BluetoothView {
    pub status: Status,
    pub adapters: Vec<Adapter>,
    /// Connected and paired first, then named ones, by name.
    pub devices: Vec<Device>,
    /// The operation running on a path.
    pub busy: BTreeMap<String, Op>,
    /// Adapters our own scan runs on.
    pub scans: BTreeSet<String>,
    /// Questions waiting for the user, oldest first.
    pub requests: Vec<AgentRequest>,
    /// The latest results, oldest first.
    pub finished: Vec<Finished>,
    /// Counts every change (tests wait on it).
    pub changes: u64,
}

impl BluetoothView {
    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.path == path)
    }

    pub fn adapter(&self, path: &str) -> Option<&Adapter> {
        self.adapters.iter().find(|a| a.path == path)
    }
}

// -- the client ---------------------------------------------------------------------------

struct Pending {
    request: AgentRequest,
    /// The call to answer (None: a code shown, nothing to answer).
    call: Option<Message>,
}

struct Inner {
    status: Status,
    /// BlueZ's unique name on the bus ("" while it is not there).
    owner: String,
    objects: BTreeMap<String, Ifaces>,
    busy: BTreeMap<String, Op>,
    /// adapter -> the number of our scan on it
    scans: BTreeMap<String, u64>,
    requests: Vec<Pending>,
    finished: VecDeque<Finished>,
    pair_sent: BTreeSet<String>,
    pair_canceled: BTreeSet<String>,
    agent_registered: bool,
    changes: u64,
    serial: u64,
    shut: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    conn: OnceLock<Connection>,
    wake: Box<dyn Fn() + Send + Sync>,
    agent_path: String,
    /// Held while the agent is registered or taken back.
    agent_lock: Mutex<()>,
}

impl Shared {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let r = f(&mut inner);
        inner.changes += 1;
        drop(inner);
        (self.wake)();
        r
    }

    /// As [`Shared::with`], counted and told only when `f` says it changed something.
    fn with_if(&self, f: impl FnOnce(&mut Inner) -> bool) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if f(&mut inner) {
            inner.changes += 1;
            drop(inner);
            (self.wake)();
        }
    }

    /// BlueZ's unique name now (read only: no change, no repaint).
    fn owner(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .owner
            .clone()
    }

    fn finish(&self, path: &str, op: Op, error: String, timed_out: bool) {
        self.with(|inner| {
            if inner.busy.get(path) == Some(&op) {
                inner.busy.remove(path);
            }
            inner.serial += 1;
            let serial = inner.serial;
            inner.finished.push_back(Finished {
                serial,
                path: path.to_owned(),
                op,
                error,
                timed_out,
            });
            while inner.finished.len() > FINISHED_KEPT {
                inner.finished.pop_front();
            }
        });
    }

    fn call<B>(&self, path: &str, iface: &str, method: &str, body: &B) -> Result<Message, String>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        let conn = self.conn.get().ok_or_else(|| Status::Starting.text())?;
        conn.call_method(Some(SERVICE), path, Some(iface), method, body)
            .map_err(|e| {
                let (name, message) = dbus_error(&e);
                error_text(&name, &message)
            })
    }

    /// A call whose answer nobody waits for (shutting down).
    fn send<B>(&self, path: &str, iface: &str, method: &str, body: &B)
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        let Some(conn) = self.conn.get() else { return };
        let msg = Message::method_call(path, method)
            .and_then(|b| b.destination(SERVICE))
            .and_then(|b| b.interface(iface))
            .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
            .and_then(|b| b.build(body));
        if let Ok(msg) = msg {
            let _ = conn.send(&msg);
        }
    }
}

fn dbus_error(e: &zbus::Error) -> (String, String) {
    match e {
        zbus::Error::MethodError(name, detail, _) => {
            (name.as_str().to_owned(), detail.clone().unwrap_or_default())
        }
        zbus::Error::FDO(fdo) => {
            let text = fdo.to_string();
            match text.split_once(": ") {
                Some((n, m)) if n.starts_with("org.") => (n.to_owned(), m.to_owned()),
                _ => (String::new(), text),
            }
        }
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            ("org.freedesktop.DBus.Error.Timeout".into(), String::new())
        }
        other => (String::new(), other.to_string()),
    }
}

static AGENTS: AtomicU64 = AtomicU64::new(1);

pub struct Bluetooth {
    shared: Arc<Shared>,
    quit: crossbeam_channel::Sender<Work>,
}

enum Work {
    Message(Message),
    Quit,
}

impl Bluetooth {
    /// Connect (on a thread of its own) and load what BlueZ knows; `wake`
    /// is called after every change.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Bluetooth {
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                status: Status::Starting,
                owner: String::new(),
                objects: BTreeMap::new(),
                busy: BTreeMap::new(),
                scans: BTreeMap::new(),
                requests: Vec::new(),
                finished: VecDeque::new(),
                pair_sent: BTreeSet::new(),
                pair_canceled: BTreeSet::new(),
                agent_registered: false,
                changes: 0,
                serial: 0,
                shut: false,
            }),
            conn: OnceLock::new(),
            wake: Box::new(wake),
            agent_path: format!(
                "/org/axross/amp/agent{}_{}",
                std::process::id(),
                AGENTS.fetch_add(1, Ordering::Relaxed)
            ),
            agent_lock: Mutex::new(()),
        });
        let (tx, rx) = crossbeam_channel::unbounded::<Work>();
        let s = shared.clone();
        let reader_tx = tx.clone();
        thread::Builder::new()
            .name("amp-bluetooth".into())
            .spawn(move || worker(s, rx, reader_tx))
            .expect("the Bluetooth thread starts");
        Bluetooth { shared, quit: tx }
    }

    pub fn view(&self) -> BluetoothView {
        let inner = self.shared.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut adapters = Vec::new();
        let mut devices = Vec::new();
        for (path, ifaces) in &inner.objects {
            if let Some(p) = ifaces.get(ADAPTER) {
                adapters.push(Adapter::from_props(path, p));
            }
            if let Some(p) = ifaces.get(DEVICE) {
                devices.push(Device::from_props(path, p, ifaces.get(BATTERY)));
            }
        }
        devices.sort_by_key(|d| {
            (
                !d.connected,
                !d.paired,
                !d.named(),
                d.display_name().to_lowercase(),
                d.address.clone(),
            )
        });
        BluetoothView {
            status: inner.status.clone(),
            adapters,
            devices,
            busy: inner.busy.clone(),
            scans: inner.scans.keys().cloned().collect(),
            requests: inner.requests.iter().map(|p| p.request.clone()).collect(),
            finished: inner.finished.iter().cloned().collect(),
            changes: inner.changes,
        }
    }

    /// Start an operation; its result comes as a [`Finished`].
    pub fn act(&self, action: Action) {
        let shared = self.shared.clone();
        let (path, op, needs) = match &action {
            Action::Power { adapter, .. } => (adapter.clone(), Op::Power, ADAPTER),
            Action::Scan { adapter, .. } => (adapter.clone(), Op::Scan, ADAPTER),
            Action::StopScan { adapter } => (adapter.clone(), Op::StopScan, ADAPTER),
            Action::Pair { device } => (device.clone(), Op::Pair, DEVICE),
            Action::CancelPairing { device } => {
                return cancel_pairing(&shared, device);
            }
            Action::Trust { device, .. } => (device.clone(), Op::Trust, DEVICE),
            Action::Connect { device } => (device.clone(), Op::Connect, DEVICE),
            Action::Disconnect { device } => (device.clone(), Op::Disconnect, DEVICE),
            Action::Remove { device } => (device.clone(), Op::Remove, DEVICE),
        };
        let refused = shared.with(|inner| {
            if inner.shut || shared.conn.get().is_none() {
                return Some(Status::Starting.text());
            }
            if !matches!(inner.status, Status::Ready | Status::NoAdapter) {
                return Some(inner.status.text());
            }
            if !inner
                .objects
                .get(&path)
                .is_some_and(|i| i.contains_key(needs))
            {
                return Some(error_text(
                    if needs == DEVICE {
                        "org.bluez.Error.DoesNotExist"
                    } else {
                        "org.freedesktop.DBus.Error.UnknownObject"
                    },
                    "",
                ));
            }
            if inner.busy.contains_key(&path) {
                return Some(error_text("org.bluez.Error.InProgress", ""));
            }
            inner.busy.insert(path.clone(), op);
            None
        });
        if let Some(why) = refused {
            // (not busy: the result says why, the operation never ran)
            return self.shared.with(|inner| {
                inner.serial += 1;
                let serial = inner.serial;
                inner.finished.push_back(Finished {
                    serial,
                    path,
                    op,
                    error: why,
                    timed_out: false,
                });
                while inner.finished.len() > FINISHED_KEPT {
                    inner.finished.pop_front();
                }
            });
        }
        thread::Builder::new()
            .name(format!("amp-bluetooth-{}", op.as_str()))
            .spawn(move || run(&shared, action, &path, op))
            .expect("a Bluetooth operation starts");
    }

    /// The user's answer to question `token`; one BlueZ took back already
    /// is ignored.
    pub fn answer(&self, token: u64, answer: Answer) {
        reply_request(&self.shared, token, answer, "org.bluez.Error.Rejected");
    }

    /// The agent's object path (tests).
    pub fn agent_path(&self) -> &str {
        &self.shared.agent_path
    }
}

impl Drop for Bluetooth {
    /// Our scans stop, the questions are canceled and the agent goes
    /// (BlueZ also does all that when our connection ends).
    fn drop(&mut self) {
        let (scans, registered, tokens) = self.shared.with(|inner| {
            inner.shut = true;
            (
                std::mem::take(&mut inner.scans),
                std::mem::replace(&mut inner.agent_registered, false),
                inner
                    .requests
                    .iter()
                    .map(|p| p.request.token)
                    .collect::<Vec<_>>(),
            )
        });
        for adapter in scans.keys() {
            self.shared.send(adapter, ADAPTER, "StopDiscovery", &());
        }
        for token in tokens {
            reply_request(
                &self.shared,
                token,
                Answer::Reject,
                "org.bluez.Error.Canceled",
            );
        }
        if registered && let Ok(path) = ObjectPath::try_from(self.shared.agent_path.as_str()) {
            self.shared
                .send("/org/bluez", AGENT_MANAGER, "UnregisterAgent", &(path,));
        }
        let _ = self.quit.send(Work::Quit);
    }
}

fn run(shared: &Arc<Shared>, action: Action, path: &str, op: Op) {
    let result: Result<(), String> = match action {
        Action::Power { on, .. } => shared
            .call(
                path,
                PROPERTIES,
                "Set",
                &(ADAPTER, "Powered", Value::from(on)),
            )
            .map(drop),
        Action::Trust { on, .. } => shared
            .call(
                path,
                PROPERTIES,
                "Set",
                &(DEVICE, "Trusted", Value::from(on)),
            )
            .map(drop),
        Action::Scan { transport, .. } => scan(shared, path, transport),
        Action::StopScan { .. } => {
            let r = shared.call(path, ADAPTER, "StopDiscovery", &()).map(drop);
            if r.is_ok() {
                shared.with(|inner| inner.scans.remove(path));
            }
            r
        }
        Action::Connect { .. } => shared.call(path, DEVICE, "Connect", &()).map(drop),
        Action::Disconnect { .. } => shared.call(path, DEVICE, "Disconnect", &()).map(drop),
        Action::Remove { .. } => {
            let adapter = shared.with(|inner| {
                inner
                    .objects
                    .get(path)
                    .and_then(|i| i.get(DEVICE))
                    .map(|p| get_str(p, "Adapter"))
                    .filter(|a| !a.is_empty())
                    .unwrap_or_else(|| {
                        path.rsplit_once('/')
                            .map(|(a, _)| a.to_owned())
                            .unwrap_or_default()
                    })
            });
            match ObjectPath::try_from(path) {
                Ok(device) => shared
                    .call(&adapter, ADAPTER, "RemoveDevice", &(device,))
                    .map(drop),
                Err(e) => Err(e.to_string()),
            }
        }
        Action::Pair { .. } => pair(shared, path),
        Action::CancelPairing { .. } => Ok(()),
    };
    shared.finish(path, op, result.err().unwrap_or_default(), false);
}

fn scan(shared: &Arc<Shared>, adapter: &str, transport: Transport) -> Result<(), String> {
    let mut filter: HashMap<&str, Value<'_>> = HashMap::new();
    filter.insert("Transport", Value::from(transport.as_str()));
    filter.insert("DuplicateData", Value::from(true));
    if let Err(why) = shared.call(adapter, ADAPTER, "SetDiscoveryFilter", &(filter,)) {
        // a filter the user did not ask for (both radios) that BlueZ refuses does not stop the scan
        if transport != Transport::Auto {
            return Err(why);
        }
    }
    shared.call(adapter, ADAPTER, "StartDiscovery", &())?;
    let number = shared.with(|inner| {
        inner.serial += 1;
        let number = inner.serial;
        inner.scans.insert(adapter.to_owned(), number);
        number
    });
    // the scan ends by itself after SCAN_LIMIT
    let s = shared.clone();
    let adapter = adapter.to_owned();
    let _ = thread::Builder::new()
        .name("amp-bluetooth-scan-limit".into())
        .spawn(move || {
            // (waits in steps: a client that goes, or a scan stopped
            // before, ends the wait - no thread sleeps on for minutes)
            let end = std::time::Instant::now() + SCAN_LIMIT;
            while std::time::Instant::now() < end {
                let over = {
                    let inner = s.inner.lock().unwrap_or_else(|p| p.into_inner());
                    inner.shut || inner.scans.get(&adapter) != Some(&number)
                };
                if over {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let ours = s.with(|inner| {
                let ours = inner.scans.get(&adapter) == Some(&number)
                    && !inner.busy.contains_key(&adapter);
                if ours {
                    inner.busy.insert(adapter.clone(), Op::StopScan);
                }
                ours
            });
            if ours {
                let r = s.call(&adapter, ADAPTER, "StopDiscovery", &()).map(drop);
                if r.is_ok() {
                    s.with(|inner| inner.scans.remove(&adapter));
                }
                s.finish(&adapter, Op::StopScan, r.err().unwrap_or_default(), true);
            }
        });
    Ok(())
}

fn ensure_agent(shared: &Arc<Shared>) {
    let _guard = shared.agent_lock.lock().unwrap_or_else(|p| p.into_inner());
    if shared.with(|inner| inner.agent_registered) {
        return;
    }
    let Ok(path) = ObjectPath::try_from(shared.agent_path.as_str()) else {
        return;
    };
    let ok = match shared.conn.get() {
        Some(conn) => match conn.call_method(
            Some(SERVICE),
            "/org/bluez",
            Some(AGENT_MANAGER),
            "RegisterAgent",
            &(path, AGENT_CAPABILITY),
        ) {
            Ok(_) => true,
            Err(e) => dbus_error(&e).0 == "org.bluez.Error.AlreadyExists",
        },
        None => false,
    };
    // (when it fails, pairing still works for devices that ask nothing)
    shared.with(|inner| inner.agent_registered = ok);
}

fn release_agent(shared: &Arc<Shared>) {
    let _guard = shared.agent_lock.lock().unwrap_or_else(|p| p.into_inner());
    let release = shared.with(|inner| {
        let release =
            inner.agent_registered && !inner.shut && !inner.busy.values().any(|op| *op == Op::Pair);
        if release {
            inner.agent_registered = false;
        }
        release
    });
    if release && let Ok(path) = ObjectPath::try_from(shared.agent_path.as_str()) {
        let _ = shared.call("/org/bluez", AGENT_MANAGER, "UnregisterAgent", &(path,));
    }
}

fn pair(shared: &Arc<Shared>, device: &str) -> Result<(), String> {
    ensure_agent(shared);
    let canceled = shared.with(|inner| {
        let canceled = inner.shut || inner.pair_canceled.remove(device);
        if !canceled {
            inner.pair_sent.insert(device.to_owned());
        }
        canceled
    });
    let result = if canceled {
        Err(error_text("org.bluez.Error.AuthenticationCanceled", ""))
    } else {
        shared.call(device, DEVICE, "Pair", &()).map(drop)
    };
    shared.with(|inner| {
        inner.pair_sent.remove(device);
        inner.pair_canceled.remove(device);
        // a code shown for this pairing has served its purpose
        inner
            .requests
            .retain(|p| p.request.device != device || p.request.needs_answer());
        // (this pairing ends: its Op is taken back before the agent is released)
        if inner.busy.get(device) == Some(&Op::Pair) {
            inner.busy.remove(device);
        }
    });
    release_agent(shared);
    result
}

fn cancel_pairing(shared: &Arc<Shared>, device: &str) {
    let sent = shared.with(|inner| {
        if inner.busy.get(device) == Some(&Op::Pair) && !inner.pair_sent.contains(device) {
            // still registering our agent: the Pair is not sent at all
            inner.pair_canceled.insert(device.to_owned());
            false
        } else {
            true
        }
    });
    if !sent {
        return shared.finish(device, Op::CancelPairing, String::new(), false);
    }
    let s = shared.clone();
    let device = device.to_owned();
    thread::Builder::new()
        .name("amp-bluetooth-cancel-pairing".into())
        .spawn(move || {
            let r = s.call(&device, DEVICE, "CancelPairing", &()).map(drop);
            s.finish(
                &device,
                Op::CancelPairing,
                r.err().unwrap_or_default(),
                false,
            );
        })
        .expect("a Bluetooth operation starts");
}

fn reply_request(shared: &Arc<Shared>, token: u64, answer: Answer, error: &str) {
    let Some(pending) = shared.with(|inner| {
        let at = inner
            .requests
            .iter()
            .position(|p| p.request.token == token)?;
        Some(inner.requests.remove(at))
    }) else {
        return;
    };
    let (Some(call), Some(conn)) = (pending.call, shared.conn.get()) else {
        return;
    };
    let header = call.header();
    let reject = |why: &str| {
        let _ = conn.reply_error(&header, error, &why);
    };
    let text = match answer {
        Answer::Reject => {
            return reject(if error.ends_with("Canceled") {
                "Canceled"
            } else {
                "Rejected by the user"
            });
        }
        Answer::Accept(text) => text,
    };
    let _ = match pending.request.kind {
        AgentKind::Pin => {
            let pin = text.trim();
            if pin.is_empty() || pin.chars().count() > 16 || pin.chars().any(char::is_control) {
                let _ = conn.reply_error(&header, "org.bluez.Error.Rejected", &"no PIN");
                return;
            }
            conn.reply(&header, &pin)
        }
        AgentKind::Passkey => match text.trim().parse::<u32>() {
            Ok(passkey) if passkey <= 999_999 => conn.reply(&header, &passkey),
            _ => {
                let _ = conn.reply_error(&header, "org.bluez.Error.Rejected", &"no passkey");
                return;
            }
        },
        _ => conn.reply(&header, &()),
    };
}

// -- the worker: what comes from the bus --------------------------------------------------

fn worker(
    shared: Arc<Shared>,
    rx: crossbeam_channel::Receiver<Work>,
    tx: crossbeam_channel::Sender<Work>,
) {
    let built = match std::env::var("AXROSS_AMP_BLUEZ_BUS") {
        Ok(address) if !address.is_empty() => {
            zbus::blocking::connection::Builder::address(address.as_str())
        }
        _ => zbus::blocking::connection::Builder::system(),
    }
    .map(|b| b.method_timeout(CALL_TIMEOUT))
    .and_then(|b| b.build());
    let conn = match built {
        Ok(conn) => conn,
        Err(e) => {
            let why = e.to_string();
            return shared.with(|inner| inner.status = Status::Unavailable(why));
        }
    };
    // everything that arrives goes to this thread through a queue: the calls
    // made here never wait behind messages nobody reads
    let reader = conn.clone();
    let _ = thread::Builder::new()
        .name("amp-bluetooth-reader".into())
        .spawn(move || {
            for msg in zbus::blocking::MessageIterator::from(&reader).flatten() {
                if tx.send(Work::Message(msg)).is_err() {
                    break;
                }
            }
        });
    let _ = shared.conn.set(conn.clone());
    let rules = [
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .sender(BUS)
            .and_then(|b| b.interface(BUS))
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.add_arg(SERVICE))
            .map(|b| b.build()),
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(OBJECT_MANAGER)
            .map(|b| b.build()),
        MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface(PROPERTIES)
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.path_namespace("/org/bluez"))
            .map(|b| b.build()),
    ];
    if let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&conn) {
        for rule in rules.into_iter().flatten() {
            let _ = proxy.add_match_rule(rule);
        }
    }
    match bus_call::<String>(&conn, "GetNameOwner", &(SERVICE,)) {
        Ok(owner) => load(&shared, &conn, owner),
        Err(e) => {
            let (name, message) = dbus_error(&e);
            shared.with(|inner| {
                inner.status =
                    if name.ends_with("NameHasNoOwner") || name.ends_with("ServiceUnknown") {
                        Status::NotRunning
                    } else {
                        Status::Unavailable(error_text(&name, &message))
                    }
            });
        }
    }
    while let Ok(work) = rx.recv() {
        match work {
            Work::Quit => break,
            Work::Message(msg) => handle(&shared, &conn, &msg),
        }
    }
    // the connection closed: the reader's wait for the next message ends
    // (it held the connection open, and itself, for as long as the bus was quiet)
    let _ = conn.close();
}

fn bus_call<T>(
    conn: &Connection,
    method: &str,
    body: &(impl serde::ser::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<T>
where
    T: for<'d> serde::de::Deserialize<'d> + zbus::zvariant::Type,
{
    conn.call_method(Some(BUS), "/org/freedesktop/DBus", Some(BUS), method, body)?
        .body()
        .deserialize::<T>()
}

type Managed = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

fn keep(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> Ifaces {
    raw.iter()
        .filter(|(iface, _)| [ADAPTER, DEVICE, BATTERY].contains(&iface.as_str()))
        .map(|(iface, p)| (iface.clone(), props(p)))
        .collect()
}

fn update_status(inner: &mut Inner) {
    inner.status = if inner.objects.values().any(|i| i.contains_key(ADAPTER)) {
        Status::Ready
    } else {
        Status::NoAdapter
    };
}

fn load(shared: &Arc<Shared>, conn: &Connection, owner: String) {
    shared.with(|inner| {
        inner.owner = owner;
        inner.agent_registered = false;
        if inner.status != Status::Ready {
            inner.status = Status::Starting;
        }
    });
    let answer = conn
        .call_method(
            Some(SERVICE),
            "/",
            Some(OBJECT_MANAGER),
            "GetManagedObjects",
            &(),
        )
        .and_then(|m| m.body().deserialize::<Managed>());
    shared.with(|inner| match answer {
        Ok(objects) => {
            inner.objects = objects
                .iter()
                .map(|(path, ifaces)| (path.as_str().to_owned(), keep(ifaces)))
                .filter(|(_, ifaces)| !ifaces.is_empty())
                .collect();
            update_status(inner);
        }
        Err(e) => {
            let (name, message) = dbus_error(&e);
            inner.status = if name.ends_with("ServiceUnknown") || name.ends_with("NameHasNoOwner") {
                Status::NotRunning
            } else {
                Status::Error(error_text(&name, &message))
            };
        }
    });
}

fn service_gone(shared: &Arc<Shared>) {
    let tokens = shared.with(|inner| {
        inner.owner.clear();
        inner.objects.clear();
        inner.scans.clear();
        inner.agent_registered = false;
        inner.status = Status::NotRunning;
        inner
            .requests
            .iter()
            .map(|p| p.request.token)
            .collect::<Vec<_>>()
    });
    for token in tokens {
        // (BlueZ is gone: the questions it asked go too)
        shared.with(|inner| inner.requests.retain(|p| p.request.token != token));
    }
}

fn handle(shared: &Arc<Shared>, conn: &Connection, msg: &Message) {
    let header = msg.header();
    let sender = header
        .sender()
        .map(|s| s.as_str().to_owned())
        .unwrap_or_default();
    let iface = header
        .interface()
        .map(|i| i.as_str().to_owned())
        .unwrap_or_default();
    let member = header
        .member()
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default();
    let path = header
        .path()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_default();
    match header.message_type() {
        MessageType::Signal => {
            if iface == BUS && member == "NameOwnerChanged" {
                if sender != BUS {
                    return;
                }
                if let Ok((name, _old, new)) = msg.body().deserialize::<(String, String, String)>()
                    && name == SERVICE
                {
                    if new.is_empty() {
                        service_gone(shared);
                    } else {
                        load(shared, conn, new);
                    }
                }
                return;
            }
            let owner = shared.owner();
            if owner.is_empty() || sender != owner {
                return; // only BlueZ speaks for BlueZ
            }
            match (iface.as_str(), member.as_str()) {
                (OBJECT_MANAGER, "InterfacesAdded") => {
                    if let Ok((object, ifaces)) = msg.body().deserialize::<(
                        OwnedObjectPath,
                        HashMap<String, HashMap<String, OwnedValue>>,
                    )>() {
                        let kept = keep(&ifaces);
                        if !kept.is_empty() {
                            shared.with(|inner| {
                                inner
                                    .objects
                                    .entry(object.as_str().to_owned())
                                    .or_default()
                                    .extend(kept);
                                update_status(inner);
                            });
                        }
                    }
                }
                (OBJECT_MANAGER, "InterfacesRemoved") => {
                    if let Ok((object, gone)) =
                        msg.body().deserialize::<(OwnedObjectPath, Vec<String>)>()
                    {
                        let object = object.as_str().to_owned();
                        shared.with(|inner| {
                            if let Some(ifaces) = inner.objects.get_mut(&object) {
                                for name in &gone {
                                    ifaces.remove(name);
                                }
                                if !ifaces.contains_key(ADAPTER) && !ifaces.contains_key(DEVICE) {
                                    inner.objects.remove(&object);
                                }
                            }
                            if gone.iter().any(|g| g == ADAPTER) {
                                inner.scans.remove(&object);
                                update_status(inner);
                            }
                        });
                    }
                }
                (PROPERTIES, "PropertiesChanged") => {
                    if let Ok((changed_iface, changed, invalidated)) =
                        msg.body()
                            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                    {
                        // (an object not kept - a media transport's volume, say - is
                        // no change and no repaint)
                        shared.with_if(|inner| {
                            let Some(p) = inner
                                .objects
                                .get_mut(&path)
                                .and_then(|i| i.get_mut(&changed_iface))
                            else {
                                return false;
                            };
                            p.extend(props(&changed));
                            for name in &invalidated {
                                p.remove(name);
                            }
                            if changed_iface == ADAPTER && !get_bool(p, "Discovering") {
                                inner.scans.remove(&path);
                            }
                            true
                        });
                    }
                }
                _ => {}
            }
        }
        MessageType::MethodCall => {
            if path != shared.agent_path {
                return;
            }
            let owner = shared.owner();
            if sender.is_empty() || sender != owner || iface != AGENT {
                let _ = conn.reply_error(&header, "org.bluez.Error.Rejected", &"not BlueZ");
                return;
            }
            agent_call(shared, conn, msg, &member);
        }
        _ => {}
    }
}

fn agent_call(shared: &Arc<Shared>, conn: &Connection, msg: &Message, member: &str) {
    let header = msg.header();
    let body = msg.body();
    let name_of = |inner: &Inner, path: &str| {
        inner
            .objects
            .get(path)
            .and_then(|i| i.get(DEVICE))
            .map(|p| Device::from_props(path, p, None).display_name())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or("").to_owned())
    };
    // the question, before its token and the device's name are filled in
    let ask = |kind: AgentKind, device: OwnedObjectPath| AgentRequest {
        token: 0,
        kind,
        device: device.as_str().to_owned(),
        device_name: String::new(),
        passkey: None,
        pin: String::new(),
        uuid: String::new(),
        entered: None,
    };
    let asked: Option<AgentRequest> = match member {
        "RequestPinCode" => body
            .deserialize::<(OwnedObjectPath,)>()
            .ok()
            .map(|(d,)| ask(AgentKind::Pin, d)),
        "RequestPasskey" => body
            .deserialize::<(OwnedObjectPath,)>()
            .ok()
            .map(|(d,)| ask(AgentKind::Passkey, d)),
        "RequestConfirmation" => body
            .deserialize::<(OwnedObjectPath, u32)>()
            .ok()
            .map(|(d, k)| AgentRequest {
                passkey: Some(k),
                ..ask(AgentKind::Confirm, d)
            }),
        "RequestAuthorization" => body
            .deserialize::<(OwnedObjectPath,)>()
            .ok()
            .map(|(d,)| ask(AgentKind::Authorize, d)),
        "AuthorizeService" => body
            .deserialize::<(OwnedObjectPath, String)>()
            .ok()
            .map(|(d, u)| AgentRequest {
                uuid: device_text(&u),
                ..ask(AgentKind::AuthorizeService, d)
            }),
        "DisplayPasskey" => {
            body.deserialize::<(OwnedObjectPath, u32, u16)>()
                .ok()
                .map(|(d, k, e)| AgentRequest {
                    passkey: Some(k),
                    entered: Some(e),
                    ..ask(AgentKind::DisplayPasskey, d)
                })
        }
        "DisplayPinCode" => body
            .deserialize::<(OwnedObjectPath, String)>()
            .ok()
            .map(|(d, p)| AgentRequest {
                pin: device_text(&p),
                ..ask(AgentKind::DisplayPin, d)
            }),
        "Cancel" | "Release" => {
            let _ = conn.reply(&header, &());
            shared.with(|inner| {
                if member == "Release" {
                    inner.agent_registered = false;
                }
                // BlueZ took its questions back: unanswered ones and the codes shown
                inner.requests.clear();
            });
            return;
        }
        _ => {
            let _ = conn.reply_error(
                &header,
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("no method {member}"),
            );
            return;
        }
    };
    let Some(mut request) = asked else {
        let _ = conn.reply_error(
            &header,
            "org.freedesktop.DBus.Error.InvalidArgs",
            &"bad arguments",
        );
        return;
    };
    let display = !request.needs_answer();
    if display {
        let _ = conn.reply(&header, &());
    }
    let rejected = shared.with(|inner| {
        if inner.shut {
            return true;
        }
        request.device_name = name_of(inner, &request.device);
        // BlueZ calls DisplayPasskey again for every digit typed on the device:
        // the code already shown for it keeps its place and token
        let same = display
            .then(|| {
                inner.requests.iter().position(|p| {
                    p.request.kind == request.kind && p.request.device == request.device
                })
            })
            .flatten();
        request.token = match same {
            Some(at) => inner.requests[at].request.token,
            None => {
                inner.serial += 1;
                inner.serial
            }
        };
        let pending = Pending {
            request,
            call: (!display).then(|| msg.clone()),
        };
        match same {
            Some(at) => inner.requests[at] = pending,
            None => inner.requests.push(pending),
        }
        false
    });
    if rejected {
        let _ = conn.reply_error(&header, "org.bluez.Error.Rejected", &"shutting down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_control_and_format_characters() {
        assert_eq!(device_text("JBL Flip 5"), "JBL Flip 5");
        assert_eq!(device_text("evil\nOK\u{202e}gnp.exe\0"), "evil OK gnp.exe");
        assert_eq!(device_text("a\u{200b}b"), "a b");
        assert_eq!(device_text(&"x".repeat(400)).chars().count(), MAX_NAME);
    }

    #[test]
    fn kinds_from_appearance_icon_and_class() {
        assert_eq!(device_kind("", 0, Some(0x0941)), "Earbuds");
        assert_eq!(device_kind("audio-headset", 0, None), "Headset");
        assert_eq!(device_kind("", 0x240404, None), "Headset"); // audio major, minor 1
        assert_eq!(device_kind("", 0x240418, None), "Headphones"); // minor 6
        assert_eq!(device_kind("", 0x240414, None), "Speaker"); // minor 5
        assert_eq!(device_kind("", 0x5a020c, None), "Phone");
        assert_eq!(device_kind("", 0x000540, None), "Keyboard");
        assert_eq!(device_kind("", 0, Some(0x0280)), "Media Player");
        assert_eq!(device_kind("phone", 0x240404, Some(0x0942)), "Headset"); // exact appearance first
        assert_eq!(device_kind("", 0, None), "Device");
        assert_eq!(device_kind("", 0, Some(0x0841)), "Speaker");
    }

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(
            error_text(
                "org.bluez.Error.AuthenticationFailed",
                "Authentication Failed"
            ),
            "Pairing failed: the PIN or passkey did not match, or the device refused."
        );
        assert!(
            error_text("org.bluez.Error.Failed", "br-connection-page-timeout")
                .contains("did not answer")
        );
        assert!(error_text("org.bluez.Error.Failed", "rfkill").contains("rfkill unblock"));
        assert_eq!(
            error_text("org.bluez.Error.Failed", "x\ny"),
            "BlueZ reported a failure: x y"
        );
        assert_eq!(error_text("", ""), "The operation failed.");
        assert!(error_text("org.bluez.Error.NotSupported", "no LE").ends_with("(no LE)"));
    }

    #[test]
    fn the_output_of_a_device() {
        let outputs = vec![
            (
                "pulseaudio:alsa_output.pci".to_owned(),
                "Built-in".to_owned(),
            ),
            (
                "pulseaudio:bluez_output.AA_BB_CC_DD_EE_01.1".to_owned(),
                "WH".to_owned(),
            ),
            ("alsa:bt".to_owned(), "Speaker Two".to_owned()),
        ];
        assert_eq!(
            find_sink(&outputs, "aa:bb:cc:dd:ee:01", &[]).unwrap().1,
            "WH"
        );
        assert_eq!(
            find_sink(&outputs, "11:22:33:44:55:66", &["speaker two "])
                .unwrap()
                .0,
            "alsa:bt"
        );
        assert!(find_sink(&outputs, "11:22:33:44:55:66", &["", "Other"]).is_none());
    }
}
