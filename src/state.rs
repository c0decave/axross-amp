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
    /// A track chosen while another plays fades in over it (1 s).
    pub fade_on_skip: bool,
    /// The starfield and the stereo scope with trails and glow (not as
    /// axross draws them).
    pub classic_plus: bool,
    /// How the pictures move: "calm" (calmer, the default) or
    /// "intense" (livelier, still never more than three flashes a second;
    /// chosen only after a photosensitivity warning).
    pub motion: String,
    /// The window's theme (`theme::THEMES`' key); following the desktop's
    /// light or dark, the two it pairs; the accent from the cover (made to
    /// read in the theme) or the theme's own.
    pub theme: String,
    pub follow_system: bool,
    pub light_theme: String,
    pub dark_theme: String,
    pub cover_accent: bool,
    /// "Show on display": the display last chosen (its connector, its
    /// monitor's name), with the sound too or not, and whether the view was
    /// open when the program ended (it opens there again).
    pub display_connector: String,
    pub display_name: String,
    pub display_sound: bool,
    pub display_open: bool,
    /// What the display view's sound took over, to be put back when the
    /// view closes - also after a restart or a crash: the output before,
    /// and the card's profile before it was switched (card, profile).
    pub display_restore_output: Option<String>,
    pub display_restore_profile: Option<(String, String)>,
    /// The window's design (`design::DESIGNS`' key).
    pub design: String,
    /// The updates: not looked for, said (a new installation's default; a
    /// player of 0.15 keeps its choice), or put in place by themselves - at
    /// the start, then once a day (src/update.rs).
    pub update_mode: crate::update::UpdateMode,
    /// The face shown here: one of axross's 18 is the shared file's own,
    /// another is kept here (the shared file says the nearest of axross's:
    /// [`crate::viz::axross_face`]). "" when none was kept yet.
    pub face: String,
    /// Which graphics adapter draws the window (`graphics::Preference`'s
    /// key: "auto", "low", "high"); taken at the start.
    pub graphics: String,
    /// A track without a cover is shown one made for it (src/covers.rs);
    /// false: the empty box with its note, as before 0.17.1 (`"covers"`:
    /// "generated" / "none").
    pub generated_covers: bool,
    /// Titles too long for their box move within it (src/marquee.rs);
    /// false: cut to an ellipsis, their whole in a tooltip - as reduced
    /// motion has them (`"scroll_titles"`; WCAG 2.2.2, review of 0.17.1 M5).
    pub scroll_titles: bool,
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
            waterfall_hd: true,
            fade_on_skip: false,
            classic_plus: false,
            motion: "calm".into(),
            theme: "midnight".into(),
            follow_system: false,
            light_theme: "paper".into(),
            dark_theme: "midnight".into(),
            cover_accent: true,
            display_connector: String::new(),
            display_name: String::new(),
            display_sound: false,
            display_open: false,
            display_restore_output: None,
            display_restore_profile: None,
            design: "modern".into(),
            update_mode: crate::update::UpdateMode::Notify,
            face: String::new(),
            graphics: "auto".into(),
            generated_covers: true,
            scroll_titles: true,
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
    if let Some(on) = map.get("fade_on_skip").and_then(Value::as_bool) {
        settings.fade_on_skip = on;
    }
    if let Some(on) = map.get("classic_plus").and_then(Value::as_bool) {
        settings.classic_plus = on;
    }
    if let Some(m) = text(&map, "motion").filter(|m| m == "calm" || m == "intense") {
        settings.motion = m;
    }
    let known = |k: &String| crate::theme::THEMES.iter().any(|t| t.key == k);
    if let Some(t) = text(&map, "theme").filter(known) {
        settings.theme = t;
    }
    if let Some(t) = text(&map, "light_theme").filter(known) {
        settings.light_theme = t;
    }
    if let Some(t) = text(&map, "dark_theme").filter(known) {
        settings.dark_theme = t;
    }
    if let Some(on) = map.get("follow_system").and_then(Value::as_bool) {
        settings.follow_system = on;
    }
    if let Some(on) = map.get("cover_accent").and_then(Value::as_bool) {
        settings.cover_accent = on;
    }
    if let Some(v) = text(&map, "display_connector").filter(|v| v.len() < 64) {
        settings.display_connector = v;
    }
    if let Some(v) = text(&map, "display_name").filter(|v| v.len() < 128) {
        settings.display_name = v;
    }
    if let Some(on) = map.get("display_sound").and_then(Value::as_bool) {
        settings.display_sound = on;
    }
    if let Some(on) = map.get("display_open").and_then(Value::as_bool) {
        settings.display_open = on;
    }
    if let Some(v) = map.get("display_restore") {
        settings.display_restore_output = v
            .get("output")
            .and_then(Value::as_str)
            .filter(|v| v.len() < 256)
            .map(str::to_owned);
        settings.display_restore_profile = match (
            v.get("card").and_then(Value::as_str),
            v.get("profile").and_then(Value::as_str),
        ) {
            (Some(c), Some(p)) if !c.is_empty() && c.len() < 256 && p.len() < 256 => {
                Some((c.to_owned(), p.to_owned()))
            }
            _ => None,
        };
    }
    if let Some(d) =
        text(&map, "design").filter(|k| crate::design::DESIGNS.iter().any(|d| d.key == k))
    {
        settings.design = d;
    }
    if let Some(g) = text(&map, "graphics") {
        settings.graphics = crate::graphics::Preference::from_key(&g).key().into();
    }
    if let Some(c) = text(&map, "covers") {
        settings.generated_covers = c != "none";
    }
    if let Some(b) = map.get("scroll_titles").and_then(|v| v.as_bool()) {
        settings.scroll_titles = b;
    }
    // (the mode; a player of 0.15 - "update_check" alone, which it always
    // wrote - keeps its choice: off, nothing to the network, unless ticked,
    // then said. Taken for no choice, every 0.15 user was asking GitHub
    // daily after the update - review 16, M3. A new installation, with
    // neither, says when one is out.)
    use crate::update::UpdateMode;
    if let Some(m) = text(&map, "update_mode").and_then(|m| UpdateMode::from_key(&m)) {
        settings.update_mode = m;
    } else if let Some(on) = map.get("update_check").and_then(Value::as_bool) {
        settings.update_mode = if on {
            UpdateMode::Notify
        } else {
            UpdateMode::Off
        };
    }
    if let Some(f) = text(&map, "face").filter(|k| crate::viz::find(k).is_some()) {
        settings.face = f;
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
    map.insert("fade_on_skip".into(), json!(settings.fade_on_skip));
    map.insert("classic_plus".into(), json!(settings.classic_plus));
    map.insert("motion".into(), json!(settings.motion));
    map.insert("theme".into(), json!(settings.theme));
    map.insert("follow_system".into(), json!(settings.follow_system));
    map.insert("light_theme".into(), json!(settings.light_theme));
    map.insert("dark_theme".into(), json!(settings.dark_theme));
    map.insert("cover_accent".into(), json!(settings.cover_accent));
    map.insert(
        "display_connector".into(),
        json!(settings.display_connector),
    );
    map.insert("display_name".into(), json!(settings.display_name));
    map.insert("display_sound".into(), json!(settings.display_sound));
    map.insert("display_open".into(), json!(settings.display_open));
    if settings.display_restore_output.is_some() || settings.display_restore_profile.is_some() {
        let (card, profile) = settings.display_restore_profile.clone().unwrap_or_default();
        map.insert(
            "display_restore".into(),
            json!({
                "output": settings.display_restore_output,
                "card": card,
                "profile": profile,
            }),
        );
    } else {
        map.remove("display_restore");
    }
    map.insert("design".into(), json!(settings.design));
    map.insert("graphics".into(), json!(settings.graphics));
    map.insert("scroll_titles".into(), json!(settings.scroll_titles));
    map.insert(
        "covers".into(),
        json!(if settings.generated_covers {
            "generated"
        } else {
            "none"
        }),
    );
    map.insert("update_mode".into(), json!(settings.update_mode.key()));
    // (for a player of 0.15: whether it checks at its start)
    map.insert(
        "update_check".into(),
        json!(settings.update_mode != crate::update::UpdateMode::Off),
    );
    if !settings.face.is_empty() {
        map.insert("face".into(), json!(settings.face));
    }
    write_object(path, &map)
}

pub fn state_path() -> PathBuf {
    config_dir().join(STATE_FILE)
}

pub fn audio_path() -> PathBuf {
    config_dir().join(AUDIO_FILE)
}

/// The JSON object in `path` (an empty one when it is missing, too big or
/// no object). A file there that is not one - broken JSON, too big - is
/// kept aside first (see [`keep_unreadable_copy`]): the next save would
/// replace it with what is in memory, and it was lost.
fn read_object(path: &Path) -> Map<String, Value> {
    let Ok(meta) = fs::metadata(path) else {
        return Map::new();
    };
    if !meta.is_file() {
        return Map::new();
    }
    // (Python's escapes for names that are no UTF-8 read as their bytes:
    // one such name lost the whole file - review 12)
    let map = (meta.len() <= MAX_FILE_BYTES)
        .then(|| fs::read(path).ok())
        .flatten()
        .and_then(|bytes| {
            serde_json::from_str::<Value>(&crate::fspath::json_from_python(&bytes)).ok()
        })
        .and_then(|value| {
            if let Value::Object(map) = value {
                Some(map)
            } else {
                None
            }
        });
    match map {
        Some(map) => map,
        None => {
            keep_unreadable_copy(path);
            Map::new()
        }
    }
}

/// A copy of the unreadable file at `path` beside it, as the Python player
/// keeps one (core/secure_storage.py): `<name>.unreadable-<YYYYmmdd-HHMMSS>-
/// <8 hex>`, mode 0600 - once a session for a file (read again before it is
/// saved, it is not copied again). Where no copy can be made, the file
/// itself is moved there: the next save then cannot replace it.
fn keep_unreadable_copy(path: &Path) {
    static KEPT: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if kept.iter().any(|p| p == path) {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    // (the civil date of the days since 1970, UTC - Howard Hinnant's)
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let hex = now.subsec_nanos() ^ std::process::id().rotate_left(16);
    let name = format!(
        "{}.unreadable-{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{hex:08x}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        tod / 3600,
        tod / 60 % 60,
        tod % 60
    );
    let backup = path.with_file_name(name);
    let copied = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut out = options.open(&backup)?;
        std::io::copy(&mut fs::File::open(path)?, &mut out)?;
        out.sync_all()
    })();
    if let Err(e) = copied {
        eprintln!(
            "axross-amp: could not keep a copy of the unreadable {} ({e}) - moved aside",
            path.display()
        );
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(path, &backup);
    } else {
        eprintln!(
            "axross-amp: {} could not be read; kept as {}",
            path.display(),
            backup.display()
        );
    }
    kept.push(path.to_path_buf());
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
        // (names that are no UTF-8 as axross writes them: "\udce9")
        let text = crate::fspath::json_for_python(&serde_json::to_string_pretty(&Value::Object(
            object.clone(),
        ))?);
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    crate::fspath::rename_over(&tmp, path).inspect_err(|_| {
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

/// The playlist, the track in it and its position: merged together, by the
/// tracks' paths ([`merge_playlist`]).
const PLAYLIST_KEYS: [&str; 3] = ["tracks", "current", "position_ms"];

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
    merge_playlist(
        &ours,
        &theirs,
        base.as_ref(),
        &mut out,
        &mut kept,
        &mut written,
    );
    for key in ours.keys() {
        if PLAYLIST_KEYS.contains(&key.as_str()) {
            continue;
        }
        let mine = ours.get(key);
        let theirs_changed = base
            .as_ref()
            .is_some_and(|b| b.get(key) != theirs.get(key) && theirs.contains_key(key.as_str()));
        let mine_changed = base.as_ref().is_none_or(|b| b.get(key) != mine);
        if theirs_changed && !mine_changed {
            if let Some(v) = theirs.get(key) {
                kept.insert(key.clone(), v.clone());
            }
            continue;
        }
        if let Some(v) = mine {
            out.insert(key.clone(), v.clone());
            written.push(key.clone());
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

/// The paths of a `tracks` value, in order (None: there is none).
fn paths_of(tracks: Option<&Value>) -> Option<Vec<&str>> {
    tracks.and_then(Value::as_array).map(|list| {
        list.iter()
            .map(|t| t.get("path").and_then(Value::as_str).unwrap_or(""))
            .collect()
    })
}

/// The path of the current track of a state (None: no current track).
fn current_path(m: &Map<String, Value>) -> Option<&str> {
    let i = usize::try_from(m.get("current")?.as_i64()?).ok()?;
    paths_of(m.get("tracks"))?.get(i).copied()
}

/// Three lists of tracks merged by their paths - each copy of a path its
/// own (the second copy of a track is another entry): what both kept, and
/// what either added since `base`; what either removed is gone. The order
/// is the one that changed it: where only the other program reordered the
/// tracks both have, its order, else ours (review 16, M7: a reorder in one
/// program was undone by an edit in the other, the order of whoever wrote
/// last taken); what the other added comes after the track it follows
/// there (added at its end: after the last of its tracks here).
/// Also where each of ours and of theirs is in the result (None: gone).
type Merged = (Vec<Value>, Vec<Option<usize>>, Vec<Option<usize>>);

fn merge_lists(base: &[&str], ours: &[Value], theirs: &[Value]) -> Merged {
    use std::collections::{HashMap, HashSet};
    fn keys<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<(&'a str, usize)> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        paths
            .map(|p| {
                let n = seen.entry(p).or_insert(0);
                *n += 1;
                (p, *n - 1)
            })
            .collect()
    }
    fn path(t: &Value) -> &str {
        t.get("path").and_then(Value::as_str).unwrap_or("")
    }
    let b = keys(base.iter().copied());
    let o = keys(ours.iter().map(path));
    let t = keys(theirs.iter().map(path));
    let (bs, os, ts): (HashSet<_>, HashSet<_>, HashSet<_>) = (
        b.iter().copied().collect(),
        o.iter().copied().collect(),
        t.iter().copied().collect(),
    );
    let kept = |k: &(&str, usize)| {
        (os.contains(k) && ts.contains(k))
            || (os.contains(k) && !bs.contains(k))
            || (ts.contains(k) && !bs.contains(k))
    };
    // (an order changed: the tracks it shares with the base in another order)
    let reordered = |side: &[(&str, usize)], set: &HashSet<(&str, usize)>| {
        let a: Vec<_> = side.iter().filter(|k| bs.contains(k)).collect();
        let z: Vec<_> = b.iter().filter(|k| set.contains(k)).collect();
        a != z
    };
    let theirs_first = reordered(&t, &ts) && !reordered(&o, &os);
    let (first, first_values, second, second_values) = if theirs_first {
        (&t, theirs, &o, ours)
    } else {
        (&o, ours, &t, theirs)
    };
    // (each track's entry: ours where we have it - our tags -, else theirs)
    let ours_at: HashMap<(&str, usize), usize> =
        o.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    let value = |k: &(&str, usize), from: &[Value], at: usize| {
        ours_at
            .get(k)
            .map_or_else(|| from[at].clone(), |&i| ours[i].clone())
    };
    let mut merged: Vec<((&str, usize), Value)> = first
        .iter()
        .enumerate()
        .filter(|(_, k)| kept(k))
        .map(|(i, k)| (*k, value(k, first_values, i)))
        .collect();
    for (i, k) in second.iter().enumerate() {
        if !kept(k) || merged.iter().any(|(m, _)| m == k) {
            continue;
        }
        // (after the nearest track before it there that is here; one added
        // at the end there - nothing kept after it - after the last of
        // those here: the track before it may have been moved away)
        let last = !second[i + 1..].iter().any(&kept);
        let at = if last {
            merged.iter().rposition(|(m, _)| second.contains(m))
        } else {
            second[..i]
                .iter()
                .rev()
                .find_map(|before| merged.iter().position(|(m, _)| m == before))
        }
        .map_or(0, |j| j + 1);
        merged.insert(at, (*k, value(k, second_values, i)));
    }
    let at: HashMap<(&str, usize), usize> = merged
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (*k, i))
        .collect();
    let ours_to = o.iter().map(|k| at.get(k).copied()).collect();
    let theirs_to = t.iter().map(|k| at.get(k).copied()).collect();
    (
        merged.into_iter().map(|(_, v)| v).collect(),
        ours_to,
        theirs_to,
    )
}

/// The playlist, its current track and position written without undoing
/// another program's (review 13: axross's added tracks were lost when only
/// the track playing here changed, or a track's tags were filled in -
/// compared by value, any change here won):
///
/// * the list is compared by its paths (tags filled in here are no edit of
///   it); one the other program changed and this one did not is theirs -
///   with what this program knows of the same tracks added where theirs
///   lacks it - and the track playing here stays current, found there by
///   its path; the window takes it all over;
/// * a list this program changed and the other did not is this program's;
/// * both changed it: three ways ([`merge_lists`]) - what either removed
///   taken out, what either added kept, the order of the one that
///   reordered;
/// * the same list: the current track (by its path) and its position are
///   the other's where only it moved them.
fn merge_playlist(
    ours: &Map<String, Value>,
    theirs: &Map<String, Value>,
    base: Option<&Map<String, Value>>,
    out: &mut Map<String, Value>,
    kept: &mut Map<String, Value>,
    written: &mut Vec<String>,
) {
    fn list(m: &Map<String, Value>) -> Option<Vec<&str>> {
        paths_of(m.get("tracks"))
    }
    let mine_list = base.is_none_or(|b| list(b) != list(ours));
    let theirs_list =
        base.is_some_and(|b| theirs.contains_key("tracks") && list(b) != list(theirs));
    let mine_current = base.is_none_or(|b| current_path(b) != current_path(ours));
    let theirs_current = base
        .is_some_and(|b| theirs.contains_key("current") && current_path(b) != current_path(theirs));
    // (`keep`: the other program's, merged - written to the file and given
    // to the window to take over, but not counted as this program's own:
    // until the window has it, the next save merges again)
    let mut write = |out: &mut Map<String, Value>, key: &str, value: Value, keep: bool| {
        if keep {
            kept.insert(key.to_owned(), value.clone());
        } else {
            written.push(key.to_owned());
        }
        out.insert(key.to_owned(), value);
    };
    let get = |m: &Map<String, Value>, k: &str| m.get(k).cloned().unwrap_or(Value::Null);
    if theirs_list && !mine_list {
        // their list, with our tags for the same tracks
        let ours_by_path: std::collections::HashMap<&str, &Map<String, Value>> = ours
            .get("tracks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t.get("path")?.as_str()?, t.as_object()?)))
            .collect();
        let mut tracks = theirs
            .get("tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for t in &mut tracks {
            let Some(entry) = t.as_object_mut() else {
                continue;
            };
            let Some(mine) = entry
                .get("path")
                .and_then(Value::as_str)
                .and_then(|p| ours_by_path.get(p))
            else {
                continue;
            };
            for (k, v) in mine.iter() {
                let missing = match entry.get(k) {
                    None => true,
                    Some(Value::String(s)) => s.is_empty(),
                    Some(Value::Number(n)) => n.as_f64() == Some(0.0),
                    _ => false,
                };
                if missing {
                    entry.insert(k.clone(), v.clone());
                }
            }
        }
        // the track playing here (when it moved here), where it is in theirs
        let here = mine_current
            .then(|| current_path(ours))
            .flatten()
            .and_then(|p| {
                let at = ours.get("current").and_then(Value::as_i64).unwrap_or(0);
                tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.get("path").and_then(Value::as_str) == Some(p))
                    .min_by_key(|(i, _)| (*i as i64 - at).abs())
                    .map(|(i, _)| i)
            });
        let (current, position) = match here {
            Some(i) => (json!(i), get(ours, "position_ms")),
            None => (get(theirs, "current"), get(theirs, "position_ms")),
        };
        write(out, "tracks", Value::Array(tracks), true);
        write(out, "current", current, true);
        write(out, "position_ms", position, true);
        return;
    }
    if theirs_list && mine_list {
        // both changed the list: three ways, by the tracks' paths - ours
        // without what they removed, with what they added after the track
        // it follows in theirs (review 15: an edit in axross dropped what
        // was added here, a track added here and removed there came back)
        let (Some(b), Some(_)) = (base.and_then(list), list(theirs)) else {
            return;
        };
        let ours_tracks = ours
            .get("tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let theirs_tracks = theirs
            .get("tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let (merged, ours_to, theirs_to) = merge_lists(&b, &ours_tracks, &theirs_tracks);
        // (the track playing: ours where it is kept, else theirs - the very
        // entry, also of a track there twice)
        let entry = |to: &[Option<usize>], m: &Map<String, Value>| {
            m.get("current")
                .and_then(Value::as_i64)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| to.get(i).copied().flatten())
        };
        let (current, position) = match entry(&ours_to, ours) {
            Some(i) => (json!(i), get(ours, "position_ms")),
            None => match entry(&theirs_to, theirs) {
                Some(i) => (json!(i), get(theirs, "position_ms")),
                None => (json!(-1), json!(0)),
            },
        };
        // (taken over by the window: until it has, the next save merges the
        // same way again)
        let changed = merged != ours_tracks;
        write(out, "tracks", Value::Array(merged), changed);
        write(out, "current", current, changed);
        write(out, "position_ms", position, changed);
        return;
    }
    if mine_list || !theirs_current || mine_current {
        for k in PLAYLIST_KEYS {
            if let Some(v) = ours.get(k) {
                write(out, k, v.clone(), false);
            }
        }
        return;
    }
    // the same list; only the other program moved to another track (the
    // list as known here: the same tracks, with their tags)
    write(out, "tracks", get(ours, "tracks"), true);
    write(out, "current", get(theirs, "current"), true);
    write(out, "position_ms", get(theirs, "position_ms"), true);
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
            .map(|t| crate::fspath::json_for_python(&t).len())
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
        // (the nearest of axross's own faces: the one here is kept in
        // this program's file)
        (
            "visualizer",
            json!(crate::viz::axross_face(&state.visualizer)),
        ),
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

/// The output device axross-amp plays on (a device id of cpal; "" is the
/// system's default), in this program's own file in `dir` (`output`) - it
/// was `amp_output` in axross's audio.json, read from there while this
/// program's file has none: axross rewrote that file with its own keys
/// only, and the device chosen here was lost (review 13). axross's own
/// `media_output` holds a Qt device id - another thing - and is left alone.
pub fn read_output(dir: &Path) -> String {
    text(&read_object(&dir.join(AMP_FILE)), "output")
        .or_else(|| text(&read_object(&dir.join(AUDIO_FILE)), "amp_output"))
        .unwrap_or_default()
}

pub fn write_output(dir: &Path, id: &str) -> std::io::Result<()> {
    let path = dir.join(AMP_FILE);
    let mut map = read_object(&path);
    map.insert("output".into(), json!(id));
    write_object(&path, &map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A media_player.json that cannot be read is kept aside, as the Python
    /// player keeps it (`<name>.unreadable-<time>-<hex>`, 0600), before
    /// anything replaces it - it was wiped by the next save (review 11).
    #[test]
    fn an_unreadable_file_is_kept_aside() {
        let dir = std::env::temp_dir().join(format!("amp-unreadable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("media_player.json");
        let broken = r#"{"tracks": [{"path": "/m/a.mp3"}], "volume": 40,, }"#;
        fs::write(&path, broken).unwrap();
        assert!(read_object(&path).is_empty());
        // (read again before it is saved: one copy, not two)
        assert!(read_object(&path).is_empty());
        let kept: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("media_player.json.unreadable-")
            })
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(fs::read_to_string(&kept[0]).unwrap(), broken);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&kept[0]).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // (a good file is not copied)
        let good = dir.join("audio.json");
        fs::write(&good, "{}").unwrap();
        read_object(&good);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

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
            waterfall_hd: false,
            fade_on_skip: true,
            classic_plus: true,
            motion: "intense".into(),
            theme: "sakura".into(),
            follow_system: true,
            light_theme: "daylight".into(),
            dark_theme: "contrast".into(),
            cover_accent: false,
            display_connector: "HDMI-1".into(),
            display_name: "LG TV".into(),
            display_sound: true,
            display_open: true,
            display_restore_output: Some("alsa_output.pci-0000_00_1f.3.analog-stereo".into()),
            display_restore_profile: Some((
                "alsa_card.pci-0000_00_1f.3".into(),
                "output:analog-stereo+input:analog-stereo".into(),
            )),
            design: "cassette".into(),
            update_mode: crate::update::UpdateMode::Install,
            face: "galaxy".into(),
            graphics: "high".into(),
            generated_covers: false,
            scroll_titles: false,
        };
        write_amp(&path, &settings).unwrap();
        let back = read_amp(&path);
        assert_eq!(back.window, Some([700.0, 900.0]));
        assert_eq!(back.loudness_target, -14.0);
        assert_eq!(back.crossfade, 4.5);
        assert!(!back.cover_colours);
        assert!(!back.waterfall_hd);
        assert!(back.fade_on_skip && back.classic_plus);
        let fresh = AmpSettings::default();
        assert!(fresh.waterfall_hd && !fresh.classic_plus && !fresh.fade_on_skip);
        assert_eq!(
            (back.graphics.as_str(), fresh.graphics.as_str()),
            ("high", "auto")
        );
        assert_eq!(back.motion, "intense");
        assert!(!back.generated_covers && fresh.generated_covers);
        assert!(!back.scroll_titles && fresh.scroll_titles);
        assert_eq!(
            (
                back.theme.as_str(),
                back.light_theme.as_str(),
                back.dark_theme.as_str()
            ),
            ("sakura", "daylight", "contrast")
        );
        assert!(back.follow_system && !back.cover_accent);
        assert_eq!(
            (back.display_connector.as_str(), back.display_name.as_str()),
            ("HDMI-1", "LG TV")
        );
        assert!(back.display_sound && back.display_open);
        assert_eq!(back.display_restore_output, settings.display_restore_output);
        assert_eq!(
            back.display_restore_profile,
            settings.display_restore_profile
        );
        assert!(!fresh.display_open && !fresh.display_sound);
        assert_eq!(back.design, "cassette");
        assert_eq!(back.face, "galaxy");
        assert_eq!(fresh.design, "modern", "the default design");
        assert_eq!(fresh.theme, "midnight", "the dark theme by default");
        assert!(fresh.cover_accent && !fresh.follow_system);
        assert_eq!(fresh.motion, "calm", "calm by default");
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
        // a change of ours to the playlist too: both kept, three ways (what
        // they removed - ours - gone, what they added and we added there)
        state.tracks.push(Track {
            path: "/m/new.mp3".into(),
            title: String::new(),
            duration_ms: 0,
            ..Default::default()
        });
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(
            paths(&path),
            ["/m/theirs.flac", "/m/second.flac", "/m/new.mp3"]
        );
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

    fn track(path: &str) -> Track {
        Track {
            path: path.into(),
            ..Default::default()
        }
    }

    fn paths(path: &Path) -> Vec<String> {
        read_state(path)
            .tracks
            .into_iter()
            .map(|t| t.path)
            .collect()
    }

    /// axross adds a track; here only the track playing changes (the next
    /// one), and a track's tags are filled in: axross's track stays, the
    /// track playing here stays current in its list (review 13, M6: the
    /// file went back to this program's list).
    #[test]
    fn a_track_added_by_axross_survives_the_next_track_here() {
        let (_dir, path) = temp("added");
        let mut state = PlayerState {
            tracks: vec![track("/m/1.mp3"), track("/m/2.mp3")],
            current: 0,
            ..Default::default()
        };
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        let mut raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        raw["tracks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"path": "/m/new.mp3", "title": "", "duration": 0}));
        fs::write(&path, raw.to_string()).unwrap();
        // here: the next track, and the tags of the first read
        state.current = 1;
        state.position_ms = 1500;
        state.tracks[0].artist = "Artist".into();
        state.tracks[0].duration_ms = 61_000;
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path), ["/m/1.mp3", "/m/2.mp3", "/m/new.mp3"]);
        let file = read_state(&path);
        assert_eq!(file.current, 1, "the track playing here");
        assert_eq!(file.position_ms, 1500);
        assert_eq!(file.tracks[0].artist, "Artist", "our tags in their list");
        assert_eq!(file.tracks[0].duration_ms, 61_000);
        assert!(kept.contains_key("tracks"), "the window takes it over");
        assert_eq!(tracks_of(&kept["tracks"]).len(), 3);
        // the window took it over: its next save keeps it
        state.tracks = file.tracks.clone();
        state.position_ms = 4000;
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path).len(), 3);
        assert_eq!(read_state(&path).position_ms, 4000);
    }

    /// The track playing here, removed in axross: theirs current then.
    #[test]
    fn a_track_removed_by_axross_takes_its_place_with_it() {
        let (_dir, path) = temp("removed");
        let mut state = PlayerState {
            tracks: vec![track("/m/1.mp3"), track("/m/2.mp3"), track("/m/3.mp3")],
            current: 0,
            ..Default::default()
        };
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        let mut raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        raw["tracks"] = json!([{"path": "/m/1.mp3"}, {"path": "/m/3.mp3"}]);
        raw["current"] = json!(0);
        fs::write(&path, raw.to_string()).unwrap();
        state.current = 1; // (here: on to 2.mp3, which axross removed)
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path), ["/m/1.mp3", "/m/3.mp3"]);
        assert_eq!(read_state(&path).current, 0);
    }

    /// The same list; axross moved to another track while this program did
    /// not: axross's current track and position, this program's tags.
    #[test]
    fn a_track_chosen_in_axross_is_taken_over() {
        let (_dir, path) = temp("moved");
        let mut state = PlayerState {
            tracks: vec![track("/m/1.mp3"), track("/m/2.mp3")],
            current: 0,
            ..Default::default()
        };
        state.tracks[1].album = "Album".into();
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        let mut raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        raw["current"] = json!(1);
        raw["position_ms"] = json!(9000);
        raw["tracks"] = json!([{"path": "/m/1.mp3"}, {"path": "/m/2.mp3"}]);
        fs::write(&path, raw.to_string()).unwrap();
        state.volume = 20;
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        let file = read_state(&path);
        assert_eq!((file.current, file.position_ms, file.volume), (1, 9000, 20));
        assert_eq!(file.tracks[1].album, "Album");
        assert_eq!(kept["current"], json!(1));
    }

    /// The output device is kept in this program's file, not in axross's
    /// audio.json - axross rewrote that with its own keys and the device was
    /// lost (review 13, M7); the old place is read while the new one has
    /// none.
    #[test]
    fn the_output_device_is_kept_in_this_programs_file() {
        let dir = crate::testdir::Scratch::new("output-kept");
        fs::write(dir.join(AUDIO_FILE), r#"{"amp_output": "old-device"}"#).unwrap();
        assert_eq!(read_output(&dir), "old-device", "the old place");
        write_output(&dir, "headphones").unwrap();
        // axross saves its audio settings with its own keys only
        fs::write(
            dir.join(AUDIO_FILE),
            r#"{"version": 1, "eq_enabled": true}"#,
        )
        .unwrap();
        assert_eq!(read_output(&dir), "headphones");
        // and this program's other settings leave it be
        write_amp(&dir.join(AMP_FILE), &AmpSettings::default()).unwrap();
        assert_eq!(read_output(&dir), "headphones");
        let audio: Value =
            serde_json::from_str(&fs::read_to_string(dir.join(AUDIO_FILE)).unwrap()).unwrap();
        assert_eq!(audio, json!({"version": 1, "eq_enabled": true}));
    }

    /// A face axross does not have: the shared file says the nearest of
    /// axross's (axross set any other back to the spectrum, the face lost
    /// each time it ran - review 13, M7), this program's file the face.
    #[test]
    fn a_face_axross_does_not_have_is_kept_here() {
        let (_dir, path) = temp("face");
        let mut state = PlayerState {
            visualizer: "galaxy".into(),
            ..Default::default()
        };
        write_state(&path, &state).unwrap();
        assert_eq!(read_state(&path).visualizer, "starfield");
        assert_eq!(crate::viz::face_to_show("starfield", "galaxy"), "galaxy");
        // chosen in axross since: axross's
        assert_eq!(crate::viz::face_to_show("vu", "galaxy"), "vu");
        // axross's own faces as they are; none kept yet
        state.visualizer = "ring".into();
        write_state(&path, &state).unwrap();
        assert_eq!(read_state(&path).visualizer, "ring");
        assert_eq!(crate::viz::face_to_show("ring", ""), "ring");
        assert_eq!(crate::viz::face_to_show("galaxy", ""), "galaxy");
        // every face has one of axross's
        for f in crate::viz::FACES {
            let near = crate::viz::axross_face(f.key);
            let i = crate::viz::FACES
                .iter()
                .position(|g| g.key == near)
                .unwrap();
            assert!(i < crate::viz::AXROSS_FACES, "{} -> {near}", f.key);
        }
    }

    fn file_tracks(path: &Path, tracks: Value) {
        let mut raw: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        raw["tracks"] = tracks;
        fs::write(path, raw.to_string()).unwrap();
    }

    /// Both programs change the list: three ways, by the tracks' paths -
    /// a track added here survives a track removed there, a track removed
    /// there stays removed, both additions are kept (review 15, M3).
    #[test]
    fn both_programs_changing_the_list_merge_three_ways() {
        let (_dir, path) = temp("three-ways");
        let mut state = PlayerState {
            tracks: vec![track("/m/1.mp3"), track("/m/2.mp3"), track("/m/3.mp3")],
            current: 0,
            ..Default::default()
        };
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        // here: X added (written); there: 2 removed and Y added after 3,
        // from the list it had ([1, 2, 3], merged with ours: [1, 3, X, Y]
        // as axross writes it now) - here the window has not taken that yet
        state.tracks.push(track("/m/X.mp3"));
        write_state_merged(&path, &state, &mut base).unwrap();
        state.tracks.push(track("/m/Z.mp3")); // (and Z added here meanwhile)
        file_tracks(
            &path,
            json!([{"path": "/m/1.mp3"}, {"path": "/m/3.mp3"}, {"path": "/m/Y.mp3"}, {"path": "/m/X.mp3"}]),
        );
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(
            paths(&path),
            ["/m/1.mp3", "/m/3.mp3", "/m/Y.mp3", "/m/X.mp3", "/m/Z.mp3"]
        );
        assert!(kept.contains_key("tracks"), "the window takes it over");
        // a second save before the window has it: the same
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path).len(), 5);
        // the window took it over: its next save is it
        state.tracks = read_state(&path).tracks;
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path).len(), 5);
    }

    /// A reorder in one program and an edit in the other: both kept - the
    /// order of the one that reordered, the track the other added after the
    /// one it follows there (review 16, M7: the order of whoever wrote last
    /// was taken, and the reorder undone); a track there twice, each copy an
    /// entry of its own.
    #[test]
    fn a_reorder_and_an_edit_in_the_other_program_are_both_kept() {
        let base = ["/m/1", "/m/2", "/m/3"];
        let list = |p: &[&str]| p.iter().map(|p| json!({ "path": p })).collect::<Vec<_>>();
        let merged = |b: &[&str], o: &[&str], t: &[&str]| {
            let (m, _, _) = merge_lists(b, &list(o), &list(t));
            m.iter()
                .map(|v| v["path"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        // 3 moved to the top there, X added here
        assert_eq!(
            merged(
                &base,
                &["/m/1", "/m/2", "/m/3", "/m/X"],
                &["/m/3", "/m/1", "/m/2"]
            ),
            ["/m/3", "/m/1", "/m/2", "/m/X"]
        );
        // 3 moved to the top here, Y added there
        assert_eq!(
            merged(
                &base,
                &["/m/3", "/m/1", "/m/2"],
                &["/m/1", "/m/2", "/m/3", "/m/Y"]
            ),
            ["/m/3", "/m/1", "/m/2", "/m/Y"]
        );
        // a second 1 added here, Y there (both at the end: theirs first)
        assert_eq!(
            merged(
                &["/m/1", "/m/2"],
                &["/m/1", "/m/2", "/m/1"],
                &["/m/1", "/m/2", "/m/Y"]
            ),
            ["/m/1", "/m/2", "/m/Y", "/m/1"]
        );
        // the second of two 1s removed here, Y added there
        assert_eq!(
            merged(
                &["/m/1", "/m/2", "/m/1"],
                &["/m/1", "/m/2"],
                &["/m/1", "/m/2", "/m/1", "/m/Y"]
            ),
            ["/m/1", "/m/2", "/m/Y"]
        );
        // and through the file: the reorder there survives a track added here
        let (_dir, path) = temp("reorder-there");
        let mut state = PlayerState {
            tracks: vec![track("/m/1.mp3"), track("/m/2.mp3"), track("/m/3.mp3")],
            current: 1,
            ..Default::default()
        };
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        file_tracks(
            &path,
            json!([{"path": "/m/3.mp3"}, {"path": "/m/1.mp3"}, {"path": "/m/2.mp3"}]),
        );
        state.tracks.push(track("/m/X.mp3"));
        write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(
            paths(&path),
            ["/m/3.mp3", "/m/1.mp3", "/m/2.mp3", "/m/X.mp3"]
        );
        // (the track playing here, 2, where it is now)
        assert_eq!(read_state(&path).current, 2);
    }

    /// A track added here, then removed in axross: removed (review 15, M3:
    /// the guard against an older list put it back for good).
    #[test]
    fn a_track_added_here_and_removed_there_stays_removed() {
        let (_dir, path) = temp("removed-there");
        let mut state = PlayerState {
            tracks: vec![track("/m/1.mp3"), track("/m/2.mp3")],
            current: 0,
            ..Default::default()
        };
        let mut base = None;
        write_state_merged(&path, &state, &mut base).unwrap();
        state.tracks.push(track("/m/X.mp3"));
        write_state_merged(&path, &state, &mut base).unwrap();
        file_tracks(&path, json!([{"path": "/m/1.mp3"}, {"path": "/m/2.mp3"}]));
        let kept = write_state_merged(&path, &state, &mut base).unwrap();
        assert_eq!(paths(&path), ["/m/1.mp3", "/m/2.mp3"]);
        assert_eq!(tracks_of(&kept["tracks"]).len(), 2);
    }
}
