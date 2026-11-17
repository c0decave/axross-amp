//! What axross-amp keeps between starts - in axross's own files, so the
//! Python player inside axross and this program share them:
//!
//! * `~/.config/axross/media_player.json` - the playlist, the track and its
//!   position, volume, mute, shuffle, repeat, the visualization with its
//!   colours and options, whether the equalizer and the playlist are open
//!   (axross's `STATE_VERSION` 1);
//! * `~/.config/axross/audio.json` - the one equalizer of axross
//!   (`eq_enabled`, `eq_preset`, `eq_gains`).
//!
//! * `~/.config/axross/axross-amp.json` - what only this program has: its
//!   window's size, ReplayGain (axross's player rewrites its own file with
//!   its own keys only, so these live apart).
//!
//! All are read leniently (a value that makes no sense is left at its
//! default) and written back with every key this program does not know kept
//! as it was, atomically (a temporary file renamed over), mode 0600.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const STATE_VERSION: i64 = 1;
/// axross's limits: the most tracks a playlist keeps, the largest state file read.
pub const MAX_TRACKS: usize = 10_000;
const MAX_FILE_BYTES: u64 = 16 << 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub path: String,
    /// "" for the default (the file's name); `duration_ms` 0 when unknown -
    /// kept in the file as whole seconds, as axross does.
    pub title: String,
    pub duration_ms: u64,
    /// From the tags (read in the background; kept in the file under keys
    /// axross's player does not know - it reads past them): for sorting by
    /// album and by artist. "" / 0 when unknown.
    pub artist: String,
    pub album: String,
    pub disc: u32,
    pub track: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualOptions {
    pub bars: u32,
    pub peak_fall: u32,
    pub smoothing: u32,
    pub fps: u32,
    /// In full screen the mouse pointer hides when it rests (axross's option).
    pub hide_cursor: bool,
}

impl Default for VisualOptions {
    fn default() -> Self {
        // axross's VISUAL_OPTIONS defaults
        VisualOptions {
            bars: 32,
            peak_fall: 45,
            smoothing: 85,
            fps: 30,
            hide_cursor: true,
        }
    }
}

impl VisualOptions {
    pub fn clamped(self) -> Self {
        VisualOptions {
            bars: self.bars.clamp(8, 128),
            peak_fall: self.peak_fall.clamp(5, 300),
            smoothing: self.smoothing.clamp(0, 95),
            fps: self.fps.clamp(10, 60),
            hide_cursor: self.hide_cursor,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub volume: u32,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: String,
    pub tracks: Vec<Track>,
    pub current: i64,
    pub position_ms: u64,
    pub visualizer: String,
    pub visual_scheme: String,
    pub visual_custom: BTreeMap<String, String>,
    pub visual_options: VisualOptions,
    pub equalizer_open: bool,
    pub playlist_open: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState {
            volume: 80,
            muted: false,
            shuffle: false,
            repeat: "off".into(),
            tracks: Vec::new(),
            current: -1,
            position_ms: 0,
            visualizer: "spectrum".into(),
            visual_scheme: "classic".into(),
            visual_custom: BTreeMap::new(),
            visual_options: VisualOptions::default(),
            equalizer_open: true,
            playlist_open: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EqSettings {
    pub enabled: bool,
    pub preset: String,
    pub gains: [f32; 10],
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings {
            enabled: true,
            preset: "Flat".into(),
            gains: [0.0; 10],
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("axross")
}

/// The player's file, axross's audio settings and this program's own, in
/// [`config_dir`].
pub const STATE_FILE: &str = "media_player.json";
pub const AUDIO_FILE: &str = "audio.json";
pub const AMP_FILE: &str = "axross-amp.json";

/// What only axross-amp keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct AmpSettings {
    /// The window's inner size (points); None: the default.
    pub window: Option<[f32; 2]>,
    /// ReplayGain: "off", "track" or "album", and the pre-amplification (dB).
    pub replay_gain: String,
    pub preamp_db: f32,
    /// The gain of a track without ReplayGain tags (dB).
    pub untagged_db: f32,
    /// The loudness meter's target (LUFS): -23 EBU R128, -16 podcasts,
    /// -14 streaming.
    pub loudness_target: f32,
    /// Seconds a track fades into the next (0: none, gapless).
    pub crossfade: f32,
    /// The pictures' colours from the cover when there is one (the chosen
    /// scheme otherwise).
    pub cover_colours: bool,
    /// The waterfall smooth, at high resolution (not axross's bars).
    pub waterfall_hd: bool,
}

/// The loudness targets offered: LUFS and what aims at it.
pub const LOUDNESS_TARGETS: [(f32, &str); 3] = [
    (-23.0, "EBU R128 (broadcast)"),
    (-16.0, "podcasts"),
    (-14.0, "streaming"),
];

impl Default for AmpSettings {
    fn default() -> Self {
        AmpSettings {
            window: None,
            replay_gain: "off".into(),
            preamp_db: 0.0,
            untagged_db: crate::engine::DEFAULT_UNTAGGED_DB,
            loudness_target: -23.0,
            crossfade: 0.0,
            cover_colours: true,
            waterfall_hd: false,
        }
    }
}

pub fn read_amp(path: &Path) -> AmpSettings {
    let map = read_object(path);
    let mut settings = AmpSettings::default();
    if let Some(Value::Array(size)) = map.get("window")
        && let [Some(w), Some(h)] = [size.first(), size.get(1)].map(|v| v.and_then(Value::as_f64))
        && (200.0..=20_000.0).contains(&w)
        && (200.0..=20_000.0).contains(&h)
    {
        settings.window = Some([w as f32, h as f32]);
    }
    if let Some(mode) =
        text(&map, "replay_gain").filter(|m| ["off", "track", "album"].contains(&m.as_str()))
    {
        settings.replay_gain = mode;
    }
    if let Some(db) = map
        .get("preamp_db")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite())
    {
        settings.preamp_db = db.clamp(-15.0, 15.0) as f32;
    }
    if let Some(db) = map
        .get("untagged_db")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite())
    {
        settings.untagged_db = db.clamp(-15.0, 15.0) as f32;
    }
    if let Some(lufs) = map.get("loudness_target").and_then(Value::as_f64)
        && let Some((target, _)) = LOUDNESS_TARGETS
            .iter()
            .find(|(t, _)| (*t as f64 - lufs).abs() < 0.05)
    {
        settings.loudness_target = *target;
    }
    if let Some(secs) = map
        .get("crossfade")
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite())
    {
        settings.crossfade = (secs.clamp(0.0, 12.0) * 2.0).round() as f32 / 2.0;
    }
    if let Some(on) = map.get("cover_colours").and_then(Value::as_bool) {
        settings.cover_colours = on;
    }
    if let Some(on) = map.get("waterfall_hd").and_then(Value::as_bool) {
        settings.waterfall_hd = on;
    }
    settings
}

pub fn write_amp(path: &Path, settings: &AmpSettings) -> std::io::Result<()> {
    let mut map = read_object(path);
    match settings.window {
        Some([w, h]) => map.insert("window".into(), json!([w.round(), h.round()])),
        None => map.remove("window"),
    };
    map.insert("replay_gain".into(), json!(settings.replay_gain));
    map.insert(
        "preamp_db".into(),
        json!((settings.preamp_db as f64 * 10.0).round() / 10.0),
    );
    map.insert(
        "untagged_db".into(),
        json!((settings.untagged_db as f64 * 10.0).round() / 10.0),
    );
    map.insert("loudness_target".into(), json!(settings.loudness_target));
    map.insert("crossfade".into(), json!(settings.crossfade));
    map.insert("cover_colours".into(), json!(settings.cover_colours));
    map.insert("waterfall_hd".into(), json!(settings.waterfall_hd));
    write_object(path, &map)
}

pub fn state_path() -> PathBuf {
    config_dir().join(STATE_FILE)
}

pub fn audio_path() -> PathBuf {
    config_dir().join(AUDIO_FILE)
}

/// The JSON object in `path` (an empty one when it is missing, too big or
/// no object).
fn read_object(path: &Path) -> Map<String, Value> {
    let Ok(meta) = fs::metadata(path) else {
        return Map::new();
    };
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Map::new();
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            if let Value::Object(map) = value {
                Some(map)
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// `object` written to `path` atomically, mode 0600.
fn write_object(path: &Path, object: &Map<String, Value>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // (a name of this write's own: two writes at once never share one)
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.tmp-{}-{n}", std::process::id()));
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        let text = serde_json::to_string_pretty(&Value::Object(object.clone()))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn int(map: &Map<String, Value>, key: &str) -> Option<i64> {
    map.get(key).and_then(Value::as_i64)
}

/// A small count (a disc's, a track's number): 0 when there is none.
fn small(map: &Map<String, Value>, key: &str) -> u32 {
    int(map, key)
        .filter(|n| (0..=100_000).contains(n))
        .unwrap_or(0) as u32
}

fn boolean(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn read_state(path: &Path) -> PlayerState {
    read_state_from(read_object(path))
}

fn read_state_from(map: Map<String, Value>) -> PlayerState {
    let mut state = PlayerState::default();
    if let Some(v) = int(&map, "volume") {
        state.volume = v.clamp(0, 100) as u32;
    }
    state.muted = boolean(&map, "muted").unwrap_or(false);
    state.shuffle = boolean(&map, "shuffle").unwrap_or(false);
    if let Some(r) = text(&map, "repeat").filter(|r| ["off", "all", "one"].contains(&r.as_str())) {
        state.repeat = r;
    }
    if let Some(Value::Array(items)) = map.get("tracks") {
        for item in items.iter().take(MAX_TRACKS) {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(path) = text(obj, "path").filter(|p| !p.is_empty() && !p.contains('\0'))
            else {
                continue;
            };
            state.tracks.push(Track {
                path,
                title: text(obj, "title").unwrap_or_default(),
                // (whole seconds in the file, as axross's player keeps them)
                duration_ms: obj
                    .get("duration")
                    .and_then(Value::as_f64)
                    .filter(|d| d.is_finite() && *d > 0.0)
                    .map(|d| (d.min(1e9) * 1000.0) as u64)
                    .unwrap_or(0),
                artist: text(obj, "artist").unwrap_or_default(),
                album: text(obj, "album").unwrap_or_default(),
                disc: small(obj, "disc"),
                track: small(obj, "track"),
            });
        }
    }
    state.current = int(&map, "current")
        .filter(|c| *c >= 0 && (*c as usize) < state.tracks.len())
        .unwrap_or(-1);
    state.position_ms = int(&map, "position_ms").unwrap_or(0).max(0) as u64;
    if let Some(v) = text(&map, "visualizer") {
        state.visualizer = v;
    }
    if let Some(v) = text(&map, "visual_scheme") {
        state.visual_scheme = v;
    }
    if let Some(Value::Object(custom)) = map.get("visual_custom") {
        for (key, value) in custom {
            if let Some(colour) = value.as_str().filter(|c| is_hex_colour(c)) {
                state.visual_custom.insert(key.clone(), colour.to_owned());
            }
        }
    }
    if let Some(Value::Object(options)) = map.get("visual_options") {
        let defaults = VisualOptions::default();
        let get = |k: &str, d: u32| {
            options
                .get(k)
                .and_then(Value::as_i64)
                .map(|v| v.max(0) as u32)
                .unwrap_or(d)
        };
        state.visual_options = VisualOptions {
            bars: get("bars", defaults.bars),
            peak_fall: get("peak_fall", defaults.peak_fall),
            smoothing: get("smoothing", defaults.smoothing),
            fps: get("fps", defaults.fps),
            hide_cursor: options
                .get("hide_cursor")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.hide_cursor),
        }
        .clamped();
    }
    state.equalizer_open = boolean(&map, "equalizer_open").unwrap_or(true);
    state.playlist_open = boolean(&map, "playlist_open").unwrap_or(true);
    state
}

pub fn is_hex_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

pub fn write_state(path: &Path, state: &PlayerState) -> std::io::Result<()> {
    write_state_merged(path, state, &mut None).map(|_| ())
}

/// The file as it is now (what was read last: the base of a merge).
pub fn read_base(path: &Path) -> Map<String, Value> {
    read_object(path)
}

/// Keys written together: the playlist and the track in it belong to each
/// other. (The position follows the playlist's winner, it decides nothing:
/// a position that moved while playing is no change of the playlist.)
const GROUPS: [&[&str]; 1] = [&["tracks", "current"]];
const FOLLOWS_TRACKS: &str = "position_ms";

/// The lock that keeps two of these programs from reading, merging and
/// writing the file at once (an advisory lock on a file beside it: flock
/// on Unix, LockFileEx on Windows). axross's player takes none - the merge
/// is what protects its writes.
fn lock(path: &Path) -> std::io::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("json.lock"))?;
    file.lock()?;
    Ok(file)
}

/// Write `state` into `path` without undoing what another program (axross's
/// player, a second axross-amp) wrote there: a three-way merge by key
/// against `base` - what this program last wrote of each key. A key (or
/// group) another program changed since, and this one did not, keeps the
/// other's value; one this program changed is its. Returns what was kept
/// of the other's (the window takes it over, so it is not undone later).
pub fn write_state_merged(
    path: &Path,
    state: &PlayerState,
    base: &mut Option<Map<String, Value>>,
) -> std::io::Result<Map<String, Value>> {
    let _lock = lock(path)?;
    let theirs = read_object(path);
    let ours = state_object(state, &theirs);
    let mut out = theirs.clone();
    let mut kept = Map::new();
    let mut written: Vec<String> = Vec::new();
    let mut done: Vec<&str> = vec![FOLLOWS_TRACKS];
    for key in ours.keys() {
        if done.contains(&key.as_str()) {
            continue;
        }
        let group: Vec<&str> = GROUPS
            .iter()
            .find(|g| g.contains(&key.as_str()))
            .map(|g| g.to_vec())
            .unwrap_or_else(|| vec![key.as_str()]);
        done.extend(group.iter().copied());
        let value = |m: &Map<String, Value>| -> Vec<Option<Value>> {
            group.iter().map(|k| m.get(*k).cloned()).collect()
        };
        let mine = value(&ours);
        let theirs_changed = base
            .as_ref()
            .is_some_and(|b| value(b) != value(&theirs) && theirs.contains_key(group[0]));
        let mine_changed = base.as_ref().is_none_or(|b| value(b) != mine);
        let mut keys: Vec<&str> = group.clone();
        if group.contains(&"tracks") {
            keys.push(FOLLOWS_TRACKS);
        }
        if theirs_changed && !mine_changed {
            for k in keys {
                if let Some(v) = theirs.get(k) {
                    kept.insert(k.to_owned(), v.clone());
                }
            }
            continue;
        }
        for k in keys {
            if let Some(v) = ours.get(k) {
                out.insert(k.to_owned(), v.clone());
                written.push(k.to_owned());
            }
        }
    }
    fit(&mut out);
    write_object(path, &out)?;
    // what this program wrote of each key, as written (trimmed to fit)
    let next = base.get_or_insert_with(Map::new);
    for k in written {
        if let Some(v) = out.get(&k) {
            next.insert(k, v.clone());
        }
    }
    Ok(kept)
}

/// The tracks of a `tracks` value of the file (as [`read_state`] reads them).
pub fn tracks_of(value: &Value) -> Vec<Track> {
    let mut map = Map::new();
    map.insert("tracks".into(), value.clone());
    read_state_from(map).tracks
}

/// axross's player reads its file only up to 4 MiB (a larger one it sets
/// aside and starts empty): what is written stays below - first without
/// this program's own track keys, then with as many tracks as fit (as
/// axross trims, keeping the first ones).
pub const MAX_STATE_BYTES: usize = 4 * 1024 * 1024;

fn fit(out: &mut Map<String, Value>) {
    let size = |m: &Map<String, Value>| {
        serde_json::to_string_pretty(&Value::Object(m.clone()))
            .map(|t| t.len())
            .unwrap_or(usize::MAX)
    };
    if size(out) <= MAX_STATE_BYTES {
        return;
    }
    if let Some(Value::Array(tracks)) = out.get_mut("tracks") {
        for t in tracks.iter_mut() {
            if let Value::Object(entry) = t {
                entry.retain(|k, _| ["path", "title", "duration"].contains(&k.as_str()));
            }
        }
    }
    if size(out) <= MAX_STATE_BYTES {
        return;
    }
    let Some(Value::Array(all)) = out.get("tracks").cloned() else {
        return;
    };
    let (mut low, mut high) = (0, all.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        out.insert("tracks".into(), Value::Array(all[..mid].to_vec()));
        if size(out) <= MAX_STATE_BYTES {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    out.insert("tracks".into(), Value::Array(all[..low].to_vec()));
    if out
        .get("current")
        .and_then(Value::as_i64)
        .is_some_and(|c| c >= low as i64)
    {
        out.insert("current".into(), json!(-1));
        out.insert("position_ms".into(), json!(0));
    }
}

/// The keys this program writes, as they are now (`file`: what the file
/// holds, for the nested options it does not know).
fn state_object(state: &PlayerState, file: &Map<String, Value>) -> Map<String, Value> {
    let mut map = Map::new();
    let tracks: Vec<Value> = state
        .tracks
        .iter()
        .take(MAX_TRACKS)
        .map(|t| {
            let mut entry =
                json!({"path": t.path, "title": t.title, "duration": t.duration_ms / 1000});
            // (what only this program reads: written when known)
            for (key, value) in [
                ("artist", json!(t.artist)),
                ("album", json!(t.album)),
                ("disc", json!(t.disc)),
                ("track", json!(t.track)),
            ] {
                if value != json!("") && value != json!(0) {
                    entry[key] = value;
                }
            }
            entry
        })
        .collect();
    let mut options = file
        .get("visual_options")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    options.insert("bars".into(), json!(state.visual_options.bars));
    options.insert("peak_fall".into(), json!(state.visual_options.peak_fall));
    options.insert("smoothing".into(), json!(state.visual_options.smoothing));
    options.insert("fps".into(), json!(state.visual_options.fps));
    options.insert(
        "hide_cursor".into(),
        json!(state.visual_options.hide_cursor),
    );
    for (key, value) in [
        ("version", json!(STATE_VERSION)),
        ("volume", json!(state.volume)),
        ("muted", json!(state.muted)),
        ("shuffle", json!(state.shuffle)),
        ("repeat", json!(state.repeat)),
        ("tracks", Value::Array(tracks)),
        ("current", json!(state.current)),
        ("position_ms", json!(state.position_ms)),
        ("visualizer", json!(state.visualizer)),
        ("visual_scheme", json!(state.visual_scheme)),
        ("visual_custom", json!(state.visual_custom)),
        ("visual_options", Value::Object(options)),
        ("equalizer_open", json!(state.equalizer_open)),
        ("playlist_open", json!(state.playlist_open)),
    ] {
        map.insert(key.into(), value);
    }
    map
}

pub fn read_eq(path: &Path) -> EqSettings {
    let map = read_object(path);
    let mut eq = EqSettings {
        enabled: boolean(&map, "eq_enabled").unwrap_or(true),
        preset: text(&map, "eq_preset").unwrap_or_else(|| "Flat".into()),
        ..EqSettings::default()
    };
    if let Some(Value::Array(values)) = map.get("eq_gains")
        && values.len() == 10
        && values.iter().all(Value::is_number)
    {
        let mut gains = [0.0f32; 10];
        for (g, v) in gains.iter_mut().zip(values) {
            *g = v.as_f64().unwrap_or(0.0) as f32;
        }
        eq.gains = crate::eq::clamp(gains);
    }
    eq
}

pub fn write_eq(path: &Path, eq: &EqSettings) -> std::io::Result<()> {
    let mut map = read_object(path); // the rest of axross's audio settings kept
    map.insert("eq_enabled".into(), json!(eq.enabled));
    map.insert("eq_preset".into(), json!(eq.preset));
    let gains: Vec<f64> = eq
        .gains
        .iter()
        .map(|g| (*g as f64 * 10.0).round() / 10.0)
        .collect();
    map.insert("eq_gains".into(), json!(gains));
    write_object(path, &map)
}

/// The output device axross-amp plays on (`amp_output` in audio.json: a
/// device id of cpal; "" is the system's default). axross's own
/// `media_output` holds a Qt device id - another thing - and is left alone.
pub fn read_output(path: &Path) -> String {
    text(&read_object(path), "amp_output").unwrap_or_default()
}

pub fn write_output(path: &Path, id: &str) -> std::io::Result<()> {
    let mut map = read_object(path);
    map.insert("amp_output".into(), json!(id));
    write_object(path, &map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in a folder of the test's own (removed with the guard).
    fn temp(name: &str) -> (crate::testdir::Scratch, PathBuf) {
        let dir = crate::testdir::Scratch::new(&format!("test-{name}"));
        let file = dir.join("state.json");
        (dir, file)
    }

    #[test]
    fn a_python_state_reads_and_round_trips_keeping_unknown_keys() {
        let (_dir, path) = temp("roundtrip");
        fs::write(
            &path,
            r##"{"version": 1, "volume": 64, "muted": true, "shuffle": true, "repeat": "one",
               "tracks": [{"path": "/m/a.mp3", "title": "A", "duration": 1234},
                          {"path": "", "title": "bad"}, 7],
               "current": 0, "position_ms": 5000, "visualizer": "radial",
               "visual_scheme": "neon", "visual_custom": {"low": "#00ff00", "bad": "red"},
               "visual_options": {"bars": 500, "fps": 60, "hide_cursor": true},
               "equalizer_open": false, "playlist_open": true, "python_only": [1, 2]}"##,
        )
        .unwrap();
        let state = read_state(&path);
        assert_eq!(state.volume, 64);
        assert!(state.muted && state.shuffle);
        assert_eq!(state.repeat, "one");
        assert_eq!(
            state.tracks,
            vec![Track {
                path: "/m/a.mp3".into(),
                title: "A".into(),
                duration_ms: 1_234_000,
                ..Default::default()
            }]
        );
        assert_eq!(state.current, 0);
        assert_eq!(state.visual_options.bars, 128); // clamped
        assert_eq!(state.visual_custom.len(), 1);
        assert!(!state.equalizer_open);
        write_state(&path, &state).unwrap();
        let again: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(again["python_only"], json!([1, 2]));
        assert_eq!(again["visual_options"]["hide_cursor"], json!(true));
        assert_eq!(read_state(&path), state);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn a_track_duration_is_in_seconds_as_axross_writes_it() {
        // axross's player keeps `duration` in whole seconds (`_track_ms // 1000`)
        let (_dir, path) = temp("seconds");
        fs::write(
            &path,
            r#"{"tracks": [{"path": "/m/a.mp3", "title": "", "duration": 125},
                           {"path": "/m/b.mp3", "title": "", "duration": 61.0}]}"#,
        )
        .unwrap();
        let state = read_state(&path);
        assert_eq!(state.tracks[0].duration_ms, 125_000);
        assert_eq!(state.tracks[1].duration_ms, 61_000);
        let mut state = state;
        state.tracks[0].duration_ms = 200_700; // (3:20.7 heard here)
        write_state(&path, &state).unwrap();
        let back: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["tracks"][0]["duration"], json!(200));
        assert_eq!(back["tracks"][1]["duration"], json!(61));
    }

    #[test]
    fn this_programs_own_settings_round_trip() {
        let (_dir, path) = temp("amp");
        assert_eq!(read_amp(&path), AmpSettings::default());
        fs::write(
            &path,
            r#"{"window": [800, 9e9], "replay_gain": "loud", "kept": 1}"#,
        )
        .unwrap();
        assert_eq!(read_amp(&path), AmpSettings::default());
        let settings = AmpSettings {
            window: Some([700.4, 900.0]),
            replay_gain: "album".into(),
            preamp_db: 99.0,
            untagged_db: -8.0,
            loudness_target: -14.0,
            crossfade: 4.5,
            cover_colours: false,
            waterfall_hd: true,
        };
        write_amp(&path, &settings).unwrap();
        let back = read_amp(&path);
        assert_eq!(back.window, Some([700.0, 900.0]));
        assert_eq!(back.loudness_target, -14.0);
        assert_eq!(back.crossfade, 4.5);
        assert!(!back.cover_colours);
        assert!(back.waterfall_hd);
        assert!(AmpSettings::default().cover_colours);
        assert_eq!(back.replay_gain, "album");
        assert_eq!(back.preamp_db, 15.0);
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["kept"], json!(1));
    }

    #[test]
    fn what_another_program_wrote_since_is_not_undone() {
        let (_dir, path) = temp("merge");
        let mut state = PlayerState::default();
        state.tracks.push(Track {
            path: "/m/ours.mp3".into(),
            title: String::new(),
            duration_ms: 0,
            ..Default::default()
        });
        state.current = 0;
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        // axross's player changes the playlist meanwhile (and a key of its own)
        let mut theirs: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        theirs["tracks"] = json!([{"path": "/m/theirs.flac", "title": "", "duration": 0},
                                  {"path": "/m/second.flac", "title": "", "duration": 0}]);
        theirs["current"] = json!(1);
        theirs["python_only"] = json!(true);
        fs::write(&path, theirs.to_string()).unwrap();
        // this program changes only the volume, then the position (it plays)
        state.volume = 33;
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        let file = read_state(&path);
        assert_eq!(file.volume, 33, "ours");
        assert_eq!(file.tracks.len(), 2, "theirs: not clobbered");
        assert_eq!(file.current, 1, "the track goes with its playlist");
        assert!(kept.contains_key("tracks"));
        // a second save (the window did not take theirs over): still theirs
        state.position_ms = 30_000;
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(read_state(&path).tracks.len(), 2, "lost at the second save");
        assert_eq!(read_state(&path).tracks[0].path, "/m/theirs.flac");
        // a change of ours to the playlist wins over theirs
        state.tracks.push(Track {
            path: "/m/new.mp3".into(),
            title: String::new(),
            duration_ms: 0,
            ..Default::default()
        });
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(read_state(&path).tracks.len(), 2);
        assert_eq!(read_state(&path).tracks[1].path, "/m/new.mp3");
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["python_only"], json!(true));
    }

    #[test]
    fn a_tracks_tags_are_kept_beside_axross_keys() {
        let (_dir, path) = temp("tags");
        let mut state = PlayerState::default();
        state.tracks.push(Track {
            path: "/m/a.flac".into(),
            title: "X – Song".into(),
            duration_ms: 61_000,
            artist: "X".into(),
            album: "Album".into(),
            disc: 2,
            track: 7,
        });
        state.tracks.push(Track {
            path: "/m/b.flac".into(),
            ..Default::default()
        });
        write_state(&path, &state).unwrap();
        assert_eq!(read_state(&path).tracks, state.tracks);
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        // (nothing written where nothing is known: axross's own entry)
        assert_eq!(
            raw["tracks"][1],
            json!({"path": "/m/b.flac", "title": "", "duration": 0})
        );
    }

    #[test]
    fn the_file_stays_small_enough_for_axross() {
        let (_dir, path) = temp("big");
        let mut state = PlayerState::default();
        for i in 0..MAX_TRACKS {
            state.tracks.push(Track {
                path: format!("/music/{}/{i}.flac", "a long folder name ".repeat(18)),
                title: "A title of some length – and an artist".into(),
                duration_ms: 200_000,
                artist: "An artist".into(),
                album: "An album with a name".into(),
                disc: 1,
                track: 3,
            });
        }
        state.current = 9_999;
        write_state(&path, &state).unwrap();
        let size = fs::metadata(&path).unwrap().len() as usize;
        assert!(
            size <= MAX_STATE_BYTES,
            "{size} bytes: axross would not read it"
        );
        let back = read_state(&path);
        assert!(back.tracks.len() > 5_000, "{}", back.tracks.len());
        assert_eq!(back.tracks[0].path, state.tracks[0].path);
        assert!(back.current < back.tracks.len() as i64);
    }

    #[test]
    fn writers_at_once_keep_each_others_changes() {
        // four programs, each changing a key of its own, fifty times, at once
        let (_dir, path) = temp("writers");
        write_state(&path, &PlayerState::default()).unwrap();
        let threads: Vec<_> = (0..4)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut base = Some(read_base(&path));
                    let mut state = read_state(&path);
                    for _ in 0..50 {
                        match t {
                            0 => state.volume = 33,
                            1 => state.visualizer = "ring".into(),
                            2 => state.shuffle = true,
                            _ => state.repeat = "all".into(),
                        }
                        write_state_merged(&path, &state, &mut base).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let file = read_state(&path);
        assert_eq!(
            (
                file.volume,
                file.visualizer.as_str(),
                file.shuffle,
                file.repeat.as_str()
            ),
            (33, "ring", true, "all")
        );
    }

    #[test]
    fn nonsense_is_ignored() {
        let (_dir, path) = temp("nonsense");
        fs::write(
            &path,
            r#"{"volume": "loud", "repeat": "forever", "current": 9, "tracks": "x"}"#,
        )
        .unwrap();
        let state = read_state(&path);
        assert_eq!(state, PlayerState::default());
        fs::write(&path, "not json").unwrap();
        assert_eq!(read_state(&path), PlayerState::default());
    }

    #[test]
    fn the_equalizer_is_shared_with_axross() {
        let (_dir, path) = temp("eq");
        fs::write(
            &path,
            r#"{"output": "speakers", "eq_gains": [1,2,3,4,5,6,7,8,9,99]}"#,
        )
        .unwrap();
        let eq = read_eq(&path);
        assert_eq!(eq.gains[9], 12.0);
        write_eq(
            &path,
            &EqSettings {
                enabled: false,
                preset: "Rock".into(),
                gains: crate::eq::PRESETS[5].1,
            },
        )
        .unwrap();
        let back: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["output"], json!("speakers"));
        assert_eq!(back["eq_preset"], json!("Rock"));
        assert!(!read_eq(&path).enabled);
    }
}
