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

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub path: String,
    /// "" for the default (the file's name); `duration_ms` 0 when unknown -
    /// kept in the file as whole seconds, as axross does.
    pub title: String,
    pub duration_ms: u64,
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
}

impl Default for AmpSettings {
    fn default() -> Self {
        AmpSettings {
            window: None,
            replay_gain: "off".into(),
            preamp_db: 0.0,
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
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
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

fn boolean(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn read_state(path: &Path) -> PlayerState {
    let map = read_object(path);
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

/// Keys written together: the playlist, the track in it and its position
/// belong to each other.
const GROUPS: [&[&str]; 1] = [&["tracks", "current", "position_ms"]];

/// Write `state` into `path` without undoing what another program (axross's
/// player, a second axross-amp) wrote there since `base` - what this
/// program last read or wrote: a three-way merge by key. A key (or group)
/// this program did not change since `base` and the other did keeps the
/// other's value; one this program changed is its. `base` becomes what was
/// written. Returns the keys of the other program's that were kept.
pub fn write_state_merged(
    path: &Path,
    state: &PlayerState,
    base: &mut Option<Map<String, Value>>,
) -> std::io::Result<Vec<String>> {
    let theirs = read_object(path);
    let ours = state_object(state, &theirs);
    let mut out = theirs.clone();
    let mut kept = Vec::new();
    let mut done: Vec<&str> = Vec::new();
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
        if theirs_changed && !mine_changed {
            kept.extend(group.iter().map(|k| (*k).to_owned()));
            continue;
        }
        for k in &group {
            if let Some(v) = ours.get(*k) {
                out.insert((*k).to_owned(), v.clone());
            }
        }
    }
    write_object(path, &out)?;
    *base = Some(out);
    Ok(kept)
}

/// The keys this program writes, as they are now (`file`: what the file
/// holds, for the nested options it does not know).
fn state_object(state: &PlayerState, file: &Map<String, Value>) -> Map<String, Value> {
    let mut map = Map::new();
    let tracks: Vec<Value> = state
        .tracks
        .iter()
        .take(MAX_TRACKS)
        .map(|t| json!({"path": t.path, "title": t.title, "duration": t.duration_ms / 1000}))
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

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("amp-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("state.json")
    }

    #[test]
    fn a_python_state_reads_and_round_trips_keeping_unknown_keys() {
        let path = temp("roundtrip");
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
                duration_ms: 1_234_000
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
        let path = temp("seconds");
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
        let path = temp("amp");
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
        };
        write_amp(&path, &settings).unwrap();
        let back = read_amp(&path);
        assert_eq!(back.window, Some([700.0, 900.0]));
        assert_eq!(back.replay_gain, "album");
        assert_eq!(back.preamp_db, 15.0);
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["kept"], json!(1));
    }

    #[test]
    fn what_another_program_wrote_since_is_not_undone() {
        let path = temp("merge");
        let mut state = PlayerState::default();
        state.tracks.push(Track {
            path: "/m/ours.mp3".into(),
            title: String::new(),
            duration_ms: 0,
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
        // this program changes only the volume
        state.volume = 33;
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        let file = read_state(&path);
        assert_eq!(file.volume, 33, "ours");
        assert_eq!(file.tracks.len(), 2, "theirs: not clobbered");
        assert_eq!(file.current, 1, "the track goes with its playlist");
        assert!(kept.contains(&"tracks".to_owned()));
        // a change of ours to the playlist wins over theirs
        state.tracks.push(Track {
            path: "/m/new.mp3".into(),
            title: String::new(),
            duration_ms: 0,
        });
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(read_state(&path).tracks.len(), 2);
        assert_eq!(read_state(&path).tracks[1].path, "/m/new.mp3");
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["python_only"], json!(true));
    }

    #[test]
    fn nonsense_is_ignored() {
        let path = temp("nonsense");
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
        let path = temp("eq");
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
