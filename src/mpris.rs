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

/// The window's waker, set when the window is there (the connection is
/// made before it, to claim the name).
type Waker = Arc<OnceLock<Arc<dyn Fn() + Send + Sync>>>;

fn wake(w: &Waker) {
    if let Some(f) = w.get() {
        f();
    }
}

struct Instance {
    tx: Sender<Request>,
    wake: Waker,
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
        wake(&self.wake);
    }
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
    wake: Waker,
}

impl Root {
    fn ask(&self, request: Request) {
        let _ = self.tx.send(request);
        wake(&self.wake);
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
    wake: Waker,
    state: Arc<Mutex<Snapshot>>,
    engine: Arc<OnceLock<Arc<Shared>>>,
}

impl Player {
    fn ask(&self, request: Request) {
        let _ = self.tx.send(request);
        wake(&self.wake);
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
        self.engine
            .get()
            .map(|e| (e.position() * 1e6) as i64)
            .unwrap_or(0)
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
    waker: Waker,
    engine: Arc<OnceLock<Arc<Shared>>>,
}

/// What [`Mpris::claim`] found on the bus.
pub enum Claim {
    /// The name is this program's: the player on the bus.
    Mine(Box<Mpris>),
    /// Another player holds it and took the files handed over.
    Handed,
    /// No bus (why).
    NoBus(String),
}

static PREPARED: Mutex<Option<Mpris>> = Mutex::new(None);

impl Mpris {
    /// Connect to the session bus (or the bus at `address`), offer the
    /// player's objects and take its name - at once, before a window is
    /// made, and without queueing for it (DoNotQueue): two starts never
    /// both get a window. When the name is held already and `hand_over`
    /// is given, the files go to the player holding it ([`Claim::Handed`]);
    /// without it (`--new-instance`) a name of its own is taken.
    pub fn claim(address: Option<String>, hand_over: Option<&[String]>) -> Claim {
        let (tx, requests) = crossbeam_channel::unbounded();
        let state = Arc::new(Mutex::new(Snapshot {
            status: "Stopped",
            loop_status: "None",
            ..Default::default()
        }));
        let waker: Waker = Arc::default();
        let engine: Arc<OnceLock<Arc<Shared>>> = Arc::default();
        let root = Root {
            tx: tx.clone(),
            wake: waker.clone(),
        };
        let instance = Instance {
            tx: tx.clone(),
            wake: waker.clone(),
        };
        let player = Player {
            tx,
            wake: waker.clone(),
            state: state.clone(),
            engine: engine.clone(),
        };
        let conn = match objects(address, root, player, instance) {
            Ok(c) => c,
            Err(why) => return Claim::NoBus(why),
        };
        let flags = zbus::fdo::RequestNameFlags::DoNotQueue.into();
        let own = match conn.request_name_with_flags(NAME, flags) {
            Ok(
                zbus::fdo::RequestNameReply::PrimaryOwner
                | zbus::fdo::RequestNameReply::AlreadyOwner,
            ) => true,
            // (held by another: zbus says NameTaken, the bus Exists)
            Ok(_) | Err(zbus::Error::NameTaken) => false,
            Err(e) => return Claim::NoBus(e.to_string()),
        };
        if !own {
            if let Some(items) = hand_over
                && hand(&conn, items)
            {
                return Claim::Handed;
            }
            // (a second player on purpose, or one that does not answer)
            let name = format!("{NAME}.instance{}", std::process::id());
            if conn.request_name_with_flags(name.as_str(), flags).is_err() {
                return Claim::NoBus("the name on the bus is taken".into());
            }
        }
        let cell = OnceLock::new();
        let _ = cell.set(conn);
        Claim::Mine(Box::new(Mpris {
            conn: Arc::new(cell),
            state,
            told: Snapshot::default(),
            requests,
            error: Arc::default(),
            waker,
            engine,
        }))
    }

    /// Keep a player claimed before the window for the window to take.
    pub fn prepare(mpris: Mpris) {
        if let Ok(mut slot) = PREPARED.lock() {
            *slot = Some(mpris);
        }
    }

    /// The player claimed before the window, if there is one.
    pub fn take_prepared() -> Option<Mpris> {
        PREPARED.lock().ok().and_then(|mut s| s.take())
    }

    /// The window is there: its engine (the position) and its waker.
    pub fn attach(&self, engine: Arc<Shared>, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.engine.set(engine);
        let _ = self.waker.set(Arc::new(wake));
        // (requests that came before it)
        if !self.requests.is_empty()
            && let Some(w) = self.waker.get()
        {
            w();
        }
    }

    /// The player on the bus for a window that claimed nothing before
    /// (tests; no bus: a player that tells nobody).
    pub fn start(
        address: Option<String>,
        engine: Arc<Shared>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Mpris {
        let mpris = match Mpris::claim(address, None) {
            Claim::Mine(m) => *m,
            Claim::Handed | Claim::NoBus(_) => Mpris {
                conn: Arc::new(OnceLock::new()),
                state: Arc::default(),
                told: Snapshot::default(),
                requests: crossbeam_channel::never(),
                error: Arc::default(),
                waker: Arc::default(),
                engine: Arc::default(),
            },
        };
        mpris.attach(engine, wake);
        mpris
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

/// A connection with the player's objects on it (no name yet).
fn objects(
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
    builder
        .method_timeout(std::time::Duration::from_secs(3))
        .serve_at(PATH, root)
        .and_then(|b| b.serve_at(PATH, player))
        .and_then(|b| b.serve_at(PATH, instance))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
}

/// Hand `items` to the player holding the name (relative paths made
/// absolute here: the other player has another directory); a moment's
/// retries - it may be just starting.
fn hand(conn: &Connection, items: &[String]) -> bool {
    let items: Vec<String> = items
        .iter()
        .map(|i| {
            if crate::playlist::is_stream(i) || i.starts_with("file://") {
                i.clone()
            } else {
                std::path::absolute(crate::fspath::to_path(i))
                    .map(|p| crate::fspath::to_string(&p))
                    .unwrap_or_else(|_| i.clone())
            }
        })
        .collect();
    for _ in 0..10 {
        if conn
            .call_method(Some(NAME), PATH, Some(INSTANCE), "Open", &(items.clone(),))
            .is_ok()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    false
}

/// The covers shown to the desktop (`mpris:artUrl`): each in a file of its
/// own named by its content (a desktop that has read one URL reads a new
/// one again), in a folder only this user can enter; the one before is
/// removed, and the last when the player goes.
pub struct Covers {
    /// The folder (made when the first cover is written), whether it is ours.
    folder: PathBuf,
    dir: Option<Option<PathBuf>>,
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
            folder: dir,
            dir: None,
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

    fn write(&mut self, bytes: &[u8]) -> Option<PathBuf> {
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
        let folder = &self.folder;
        let dir = self.dir.get_or_insert_with(|| private_folder(folder));
        let path = dir.as_ref()?.join(format!("cover-{hash:016x}.{ext}"));
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
        let scratch = crate::testdir::Scratch::new("covers");
        let base = scratch.join("folder");
        let mut covers = Covers::in_folder(base.clone());
        let a = covers.show(Some(b"\x89PNG first"));
        let mode = std::fs::metadata(&base).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
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
