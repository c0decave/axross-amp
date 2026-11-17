//! The player on the desktop's session D-Bus as MPRIS 2
//! (`org.mpris.MediaPlayer2`): media keys, a headset's buttons (AVRCP
//! through BlueZ), the desktop's sound menu and lock screen see what plays
//! and control it - play, pause, next, previous, seek, volume, shuffle,
//! repeat, raise and quit.
//!
//! The D-Bus side runs on zbus's thread: what it is asked comes to the
//! window as [`Request`]s (and wakes it); what it answers is the
//! [`Snapshot`] the window gives it each frame - changes are signalled
//! (`PropertiesChanged`, `Seeked`) as the specification wants.

use crate::engine::Shared;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

pub const PATH: &str = "/org/mpris/MediaPlayer2";
pub const NAME: &str = "org.mpris.MediaPlayer2.axross_amp";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// What the desktop asks of the player.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Seek by this many microseconds (forward or back).
    Seek(i64),
    /// Go to this position (µs) of the track with this generation.
    SetPosition(u64, i64),
    Volume(f64),
    Shuffle(bool),
    /// "None", "Track" or "Playlist".
    Loop(String),
    OpenUri(String),
    /// Another start of the program handed these over (files, folders,
    /// URLs): added, the first played.
    Open(Vec<String>),
    Raise,
    Quit,
}

/// The interface through which a second start hands its files to the
/// player already running (one player, not two playing over each other).
pub const INSTANCE: &str = "org.axross.amp.Instance";

struct Instance {
    tx: Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

#[zbus::interface(name = "org.axross.amp.Instance")]
impl Instance {
    /// Add and play `items` (absolute paths, URLs); none: show the window.
    fn open(&self, items: Vec<String>) {
        let items: Vec<String> = items
            .into_iter()
            .filter(|i| !i.is_empty() && i.len() <= 4096 && !i.contains('\0'))
            .take(10_000)
            .collect();
        if !items.is_empty() {
            let _ = self.tx.send(Request::Open(items));
        }
        let _ = self.tx.send(Request::Raise);
        (self.wake)();
    }
}

/// Hand `items` to the player already running on the session bus (or the
/// bus at `address`): true when there is one and it took them. Relative
/// paths are made absolute here (the other player has another directory).
pub fn forward(address: Option<&str>, items: &[String]) -> bool {
    let built = match address {
        Some(a) => zbus::blocking::connection::Builder::address(a),
        None => zbus::blocking::connection::Builder::session(),
    }
    .and_then(|b| b.method_timeout(std::time::Duration::from_secs(3)).build());
    let Ok(conn) = built else {
        return false;
    };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from(NAME) else {
        return false;
    };
    if !dbus.name_has_owner(name).unwrap_or(false) {
        return false;
    }
    let items: Vec<String> = items
        .iter()
        .map(|i| {
            if crate::playlist::is_stream(i) || i.starts_with("file://") {
                i.clone()
            } else {
                std::path::absolute(i)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| i.clone())
            }
        })
        .collect();
    conn.call_method(Some(NAME), PATH, Some(INSTANCE), "Open", &(items,))
        .is_ok()
}

/// What plays, as the desktop is told.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// The track's generation (its id on the bus).
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub url: String,
    pub length_us: Option<i64>,
    /// file:// of the cover written out.
    pub art_url: String,
}

/// The player's state as MPRIS has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// "Playing", "Paused" or "Stopped".
    pub status: &'static str,
    /// "None", "Track" or "Playlist".
    pub loop_status: &'static str,
    pub shuffle: bool,
    pub volume: f64,
    pub track: Option<Track>,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

fn track_path(id: u64) -> String {
    format!("/org/axross/amp/track/{id}")
}

fn metadata(track: &Option<Track>) -> HashMap<String, OwnedValue> {
    let mut map: HashMap<String, OwnedValue> = HashMap::new();
    let mut put = |key: &str, value: Value<'_>| {
        if let Ok(v) = OwnedValue::try_from(value) {
            map.insert(key.to_owned(), v);
        }
    };
    let Some(t) = track else {
        if let Ok(p) = ObjectPath::try_from("/org/mpris/MediaPlayer2/TrackList/NoTrack") {
            put("mpris:trackid", Value::from(p));
        }
        return map;
    };
    let id = track_path(t.id);
    if let Ok(p) = ObjectPath::try_from(id.as_str()) {
        put("mpris:trackid", Value::from(p));
    }
    if let Some(us) = t.length_us {
        put("mpris:length", Value::from(us));
    }
    put("xesam:title", Value::from(t.title.as_str()));
    if !t.artist.is_empty() {
        put("xesam:artist", Value::from(vec![t.artist.as_str()]));
    }
    if !t.album.is_empty() {
        put("xesam:album", Value::from(t.album.as_str()));
    }
    if !t.url.is_empty() {
        put("xesam:url", Value::from(t.url.as_str()));
    }
    if !t.art_url.is_empty() {
        put("mpris:artUrl", Value::from(t.art_url.as_str()));
    }
    map
}

struct Root {
    tx: Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Root {
    fn ask(&self, request: Request) {
        let _ = self.tx.send(request);
        (self.wake)();
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.ask(Request::Raise);
    }

    fn quit(&self) {
        self.ask(Request::Quit);
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "axross-amp"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "axross-amp"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<&str> {
        vec!["file", "http", "https"]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<&str> {
        vec![
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/opus",
            "audio/mp4",
            "audio/aac",
            "audio/wav",
            "audio/x-wav",
            "audio/aiff",
            "audio/x-mpegurl",
            "audio/x-scpls",
        ]
    }
}

struct Player {
    tx: Sender<Request>,
    wake: Arc<dyn Fn() + Send + Sync>,
    state: Arc<Mutex<Snapshot>>,
    engine: Arc<Shared>,
}

impl Player {
    fn ask(&self, request: Request) {
        let _ = self.tx.send(request);
        (self.wake)();
    }

    fn snapshot(&self) -> Snapshot {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.ask(Request::Next);
    }

    fn previous(&self) {
        self.ask(Request::Previous);
    }

    fn pause(&self) {
        self.ask(Request::Pause);
    }

    fn play_pause(&self) {
        self.ask(Request::PlayPause);
    }

    fn stop(&self) {
        self.ask(Request::Stop);
    }

    fn play(&self) {
        self.ask(Request::Play);
    }

    fn seek(&self, offset: i64) {
        self.ask(Request::Seek(offset));
    }

    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        // (a position for another track than this one is ignored, as the
        // specification says)
        let Some(id) = track_id
            .as_str()
            .strip_prefix("/org/axross/amp/track/")
            .and_then(|n| n.parse().ok())
        else {
            return;
        };
        self.ask(Request::SetPosition(id, position));
    }

    fn open_uri(&self, uri: String) {
        self.ask(Request::OpenUri(uri));
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.snapshot().status.to_owned()
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.snapshot().loop_status.to_owned()
    }

    #[zbus(property)]
    fn set_loop_status(&mut self, value: String) {
        self.ask(Request::Loop(value));
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_rate(&mut self, _value: f64) {}

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.snapshot().shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&mut self, value: bool) {
        self.ask(Request::Shuffle(value));
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.snapshot().track)
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.snapshot().volume
    }

    #[zbus(property)]
    fn set_volume(&mut self, value: f64) {
        if value.is_finite() {
            self.ask(Request::Volume(value.clamp(0.0, 1.0)));
        }
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        (self.engine.position() * 1e6) as i64
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.snapshot().can_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.snapshot().can_previous
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.snapshot().track.is_some()
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.snapshot().track.is_some()
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.snapshot().can_seek
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// The player on the bus (the connection is made on a thread of its own;
/// until it is there, nothing is told).
pub struct Mpris {
    conn: Arc<OnceLock<Connection>>,
    state: Arc<Mutex<Snapshot>>,
    told: Snapshot,
    requests: Receiver<Request>,
    /// Why there is no connection (none yet: "").
    pub error: Arc<Mutex<String>>,
}

impl Mpris {
    /// Offer the player on the session bus (or the bus at `address`).
    pub fn start(
        address: Option<String>,
        engine: Arc<Shared>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Mpris {
        let (tx, requests) = crossbeam_channel::unbounded();
        let state = Arc::new(Mutex::new(Snapshot {
            status: "Stopped",
            loop_status: "None",
            ..Default::default()
        }));
        let conn = Arc::new(OnceLock::new());
        let error = Arc::new(Mutex::new(String::new()));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let (c, s, e) = (conn.clone(), state.clone(), error.clone());
        let _ = std::thread::Builder::new()
            .name("amp-mpris".into())
            .spawn(move || {
                let root = Root {
                    tx: tx.clone(),
                    wake: wake.clone(),
                };
                let instance = Instance {
                    tx: tx.clone(),
                    wake: wake.clone(),
                };
                let player = Player {
                    tx,
                    wake,
                    state: s,
                    engine,
                };
                match connect(address, root, player, instance) {
                    Ok(connection) => {
                        let _ = c.set(connection);
                    }
                    Err(why) => {
                        if let Ok(mut slot) = e.lock() {
                            *slot = why;
                        }
                    }
                }
            });
        Mpris {
            conn,
            state,
            told: Snapshot::default(),
            requests,
            error,
        }
    }

    /// Whether the player is on the bus.
    pub fn ready(&self) -> bool {
        self.conn.get().is_some()
    }

    /// What the desktop asked since the last frame.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.try_iter().collect()
    }

    /// The state now: kept for the properties, and what changed signalled.
    pub fn update(&mut self, now: Snapshot) {
        if now == self.told {
            return;
        }
        if let Ok(mut s) = self.state.lock() {
            *s = now.clone();
        }
        let Some(conn) = self.conn.get() else {
            return; // (told once there is a connection: `told` stays behind)
        };
        let before = std::mem::replace(&mut self.told, now.clone());
        let mut changed: HashMap<&str, Value<'_>> = HashMap::new();
        if before.status != now.status {
            changed.insert("PlaybackStatus", Value::from(now.status));
        }
        if before.loop_status != now.loop_status {
            changed.insert("LoopStatus", Value::from(now.loop_status));
        }
        if before.shuffle != now.shuffle {
            changed.insert("Shuffle", Value::from(now.shuffle));
        }
        if before.volume != now.volume {
            changed.insert("Volume", Value::from(now.volume));
        }
        if before.track != now.track {
            let map: HashMap<String, Value<'_>> = metadata(&now.track)
                .into_iter()
                .map(|(k, v)| (k, Value::from(v)))
                .collect();
            changed.insert("Metadata", Value::from(map));
            if before.track.is_some() != now.track.is_some() {
                changed.insert("CanPlay", Value::from(now.track.is_some()));
                changed.insert("CanPause", Value::from(now.track.is_some()));
            }
        }
        if before.can_seek != now.can_seek {
            changed.insert("CanSeek", Value::from(now.can_seek));
        }
        if before.can_next != now.can_next {
            changed.insert("CanGoNext", Value::from(now.can_next));
        }
        if before.can_previous != now.can_previous {
            changed.insert("CanGoPrevious", Value::from(now.can_previous));
        }
        if !changed.is_empty() {
            let _ = conn.emit_signal(
                None::<()>,
                PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(PLAYER, changed, Vec::<&str>::new()),
            );
        }
    }

    /// The position jumped (a seek): the desktop is told where to.
    pub fn seeked(&self, position_secs: f64) {
        if let Some(conn) = self.conn.get() {
            let _ = conn.emit_signal(
                None::<()>,
                PATH,
                PLAYER,
                "Seeked",
                &((position_secs.max(0.0) * 1e6) as i64,),
            );
        }
    }
}

fn connect(
    address: Option<String>,
    root: Root,
    player: Player,
    instance: Instance,
) -> Result<Connection, String> {
    let builder = match address {
        Some(a) => zbus::blocking::connection::Builder::address(a.as_str()),
        None => zbus::blocking::connection::Builder::session(),
    }
    .map_err(|e| e.to_string())?;
    let conn = builder
        .serve_at(PATH, root)
        .and_then(|b| b.serve_at(PATH, player))
        .and_then(|b| b.serve_at(PATH, instance))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())?;
    // the name; a second player takes a name of its own (the specification's way)
    let names = [
        NAME.to_owned(),
        format!("{NAME}.instance{}", std::process::id()),
    ];
    for name in names {
        if conn.request_name(name.as_str()).is_ok() {
            return Ok(conn);
        }
    }
    Err("the name on the bus is taken".into())
}

/// The covers shown to the desktop (`mpris:artUrl`): each in a file of its
/// own named by its content (a desktop that has read one URL reads a new
/// one again), in a folder only this user can enter; the one before is
/// removed, and the last when the player goes.
pub struct Covers {
    dir: Option<PathBuf>,
    current: Option<PathBuf>,
}

impl Default for Covers {
    fn default() -> Self {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_dir());
        Covers::in_folder(match base {
            Some(run) => run.join("axross-amp"),
            None => std::env::temp_dir().join(format!("axross-amp-{}", uid().unwrap_or(0))),
        })
    }
}

/// This process's user (who owns /proc/self).
fn uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// `dir` as a private folder: made with mode 0700, or one that is there is
/// a real folder (no symbolic link) of this user that nobody else can enter.
fn private_folder(dir: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let _ = std::fs::DirBuilder::new().mode(0o700).create(dir);
    let meta = std::fs::symlink_metadata(dir).ok()?;
    let ours = meta.file_type().is_dir()
        && Some(meta.uid()) == uid()
        && meta.permissions().mode() & 0o077 == 0;
    ours.then(|| dir.to_path_buf())
}

impl Covers {
    pub fn in_folder(dir: PathBuf) -> Covers {
        Covers {
            dir: private_folder(&dir),
            current: None,
        }
    }

    /// Show `bytes` (None: no cover): its file:// URL ("" when there is none).
    pub fn show(&mut self, bytes: Option<&[u8]>) -> String {
        let path = bytes.and_then(|b| self.write(b));
        if self.current != path
            && let Some(old) = self.current.take()
        {
            let _ = std::fs::remove_file(old);
        }
        self.current = path.clone();
        path.map(|p| format!("file://{}", p.display()))
            .unwrap_or_default()
    }

    fn write(&self, bytes: &[u8]) -> Option<PathBuf> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let ext = if bytes.starts_with(b"\x89PNG") {
            "png"
        } else if bytes.starts_with(&[0xff, 0xd8]) {
            "jpg"
        } else {
            return None;
        };
        // FNV-1a of the picture: the same cover, the same name
        let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
        });
        let path = self.dir.as_ref()?.join(format!("cover-{hash:016x}.{ext}"));
        if self.current.as_ref() == Some(&path) {
            return Some(path);
        }
        // a new file only (create_new: nothing there is followed or overwritten)
        let _ = std::fs::remove_file(&path);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .ok()?;
        file.write_all(bytes).ok()?;
        Some(path)
    }
}

impl Drop for Covers {
    fn drop(&mut self) {
        if let Some(old) = self.current.take() {
            let _ = std::fs::remove_file(old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_file_is_the_one_mpris_names() {
        // DesktopEntry "axross-amp": packaging/axross-amp.desktop, its icon there
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging");
        let entry = std::fs::read_to_string(dir.join("axross-amp.desktop")).unwrap();
        for line in [
            "Type=Application",
            "Exec=axross-amp %U",
            "Icon=axross-amp",
            "Terminal=false",
        ] {
            assert!(entry.lines().any(|l| l == line), "{line}");
        }
        let mimes = entry
            .lines()
            .find_map(|l| l.strip_prefix("MimeType="))
            .unwrap();
        for m in ["audio/mpeg", "audio/flac", "audio/ogg", "audio/x-scpls"] {
            assert!(mimes.split(';').any(|x| x == m), "{m}");
        }
        assert!(dir.join("axross-amp.svg").exists());
    }

    #[test]
    fn each_cover_a_file_of_its_own_in_a_private_folder() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("amp-covers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut covers = Covers::in_folder(base.clone());
        let mode = std::fs::metadata(&base).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let a = covers.show(Some(b"\x89PNG first"));
        let b = covers.show(Some(b"\xff\xd8 second"));
        assert!(a.ends_with(".png") && b.ends_with(".jpg") && a != b);
        assert!(
            !Path::new(&a["file://".len()..]).exists(),
            "the one before stays"
        );
        assert_eq!(
            std::fs::read(&b["file://".len()..]).unwrap(),
            b"\xff\xd8 second"
        );
        assert_eq!(covers.show(None), "");
        assert!(!Path::new(&b["file://".len()..]).exists());
        // a folder that others can enter is not used
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o777)).unwrap();
        let mut open = Covers::in_folder(base.clone());
        assert_eq!(open.show(Some(b"\x89PNG x")), "");
        // a symbolic link where a cover is written is not followed
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let victim = base.with_extension("victim");
        std::fs::write(&victim, b"keep").unwrap();
        let mut covers = Covers::in_folder(base.clone());
        let url = covers.show(Some(b"\x89PNG y"));
        let name = &url["file://".len()..];
        covers.show(None);
        std::os::unix::fs::symlink(&victim, name).unwrap();
        covers.show(Some(b"\x89PNG y"));
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
    }
}
