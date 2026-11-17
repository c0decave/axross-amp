//! The playlist: files, folders (walked), M3U / PLS playlists and http(s)
//! streams; shuffle and repeat as axross's player has them.

use crate::state::{MAX_TRACKS, Track};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// What the decoder reads (symphonia's formats).
pub const AUDIO_EXTENSIONS: [&str; 17] = [
    "mp3", "flac", "ogg", "oga", "opus", "wav", "wave", "aif", "aiff", "aifc", "m4a", "m4b", "mp4",
    "aac", "caf", "mka", "webm",
];
pub const PLAYLIST_EXTENSIONS: [&str; 3] = ["m3u", "m3u8", "pls"];
/// How deep a folder is walked, and the largest playlist file read (axross's).
const MAX_DEPTH: usize = 16;
const MAX_PLAYLIST_BYTES: u64 = 1 << 20;
const MAX_LINE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}

impl Repeat {
    pub fn key(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "all" => Repeat::All,
            "one" => Repeat::One,
            _ => Repeat::Off,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

pub fn is_stream(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn is_audio(path: &Path) -> bool {
    AUDIO_EXTENSIONS.contains(&extension(path).as_str())
}

/// The title a track shows without tags: its file name without extension,
/// or a stream's address.
pub fn default_title(path: &str) -> String {
    if is_stream(path) {
        return path.to_owned();
    }
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_owned())
}

pub struct Playlist {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// The shuffled order of the indexes still to play (shuffle on); None
    /// before a round began, empty when every track of it was played.
    order: Option<Vec<usize>>,
    seed: u64,
    /// The tracks played before the current one, oldest first (Previous
    /// with shuffle goes back through them; axross keeps 1000).
    history: Vec<usize>,
    /// Previous handed out the last of `history`: playing it is no new step.
    going_back: Option<usize>,
    /// Where the current track was when it was removed (it plays on; the
    /// one after it follows).
    resume_at: Option<usize>,
    /// Counts the changes of `tracks` (a view of them knows when to look again).
    pub version: u64,
}

/// How many tracks Previous can go back through.
const HISTORY: usize = 1000;

impl Default for Playlist {
    fn default() -> Self {
        Playlist {
            tracks: Vec::new(),
            current: None,
            shuffle: false,
            repeat: Repeat::Off,
            order: None,
            seed: 0x9e3779b97f4a7c15,
            history: Vec::new(),
            going_back: None,
            resume_at: None,
            version: 0,
        }
    }
}

impl Playlist {
    /// A small xorshift: shuffling needs no cryptography.
    fn random(&mut self) -> u64 {
        if self.seed == 0x9e3779b97f4a7c15 {
            self.seed ^= std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
        }
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.seed = x;
        x
    }

    fn reshuffle(&mut self) {
        let mut order: Vec<usize> = (0..self.tracks.len())
            .filter(|i| Some(*i) != self.current)
            .collect();
        for i in (1..order.len()).rev() {
            let j = (self.random() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        self.order = Some(order);
    }

    /// `index` is the track playing now.
    pub fn play(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }
        if self.going_back.take() != Some(index)
            && let Some(c) = self.current
            && c != index
        {
            self.history.push(c);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
        }
        self.current = Some(index);
        self.resume_at = None;
        if let Some(order) = self.order.as_mut() {
            order.retain(|i| *i != index);
        }
    }

    /// Every index kept (current, history, the place of a removed track)
    /// through `map`: None drops it.
    fn remap(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.version += 1;
        self.current = self.current.and_then(&map);
        self.history = self.history.iter().filter_map(|i| map(*i)).collect();
        self.history.dedup();
        self.going_back = None;
        self.order = None;
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on;
        self.order = None;
    }

    /// Add tracks (paths or stream URLs); returns how many were added.
    pub fn add_tracks(&mut self, tracks: Vec<Track>) -> usize {
        let room = MAX_TRACKS.saturating_sub(self.tracks.len());
        let added = tracks.len().min(room);
        self.tracks.extend(tracks.into_iter().take(room));
        self.order = None;
        self.version += 1;
        added
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }
        self.tracks.remove(index);
        if self.current == Some(index) {
            self.resume_at = Some(index);
        } else if let Some(r) = self.resume_at.as_mut()
            && *r > index
        {
            *r -= 1;
        }
        self.remap(|i| match i.cmp(&index) {
            std::cmp::Ordering::Less => Some(i),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(i - 1),
        });
    }

    pub fn clear(&mut self) {
        self.version += 1;
        self.tracks.clear();
        self.current = None;
        self.order = None;
        self.history.clear();
        self.going_back = None;
        self.resume_at = None;
    }

    /// Move the track at `from` to `to` (the current one stays current).
    pub fn move_track(&mut self, from: usize, to: usize) {
        if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
            return;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        let moved = |c: usize| {
            if c == from {
                to
            } else if from < c && c <= to {
                c - 1
            } else if to <= c && c < from {
                c + 1
            } else {
                c
            }
        };
        self.resume_at = None;
        self.remap(|i| Some(moved(i)));
    }

    /// Sort the list (the current track stays current, the history follows).
    pub fn sort(&mut self, key: SortKey) {
        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        let title = |t: &Track| {
            if t.title.is_empty() {
                default_title(&t.path)
            } else {
                t.title.clone()
            }
            .to_lowercase()
        };
        let name = |t: &Track| {
            Path::new(&t.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| t.path.to_lowercase())
        };
        let tracks = &self.tracks;
        match key {
            SortKey::Title => order.sort_by_cached_key(|i| title(&tracks[*i])),
            SortKey::FileName => order.sort_by_cached_key(|i| name(&tracks[*i])),
            SortKey::Path => order.sort_by_cached_key(|i| {
                let p = Path::new(&tracks[*i].path);
                let folder = p
                    .parent()
                    .map(|d| d.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                (folder, name(&tracks[*i]))
            }),
            // (unknown lengths last)
            SortKey::Duration => order.sort_by_key(|i| {
                let d = tracks[*i].duration_ms;
                (d == 0, d)
            }),
            SortKey::Reverse => order.reverse(),
        }
        let mut place = vec![0; order.len()];
        for (new, old) in order.iter().enumerate() {
            place[*old] = new;
        }
        let mut sorted: Vec<Track> = order.iter().map(|i| self.tracks[*i].clone()).collect();
        std::mem::swap(&mut self.tracks, &mut sorted);
        self.resume_at = None;
        self.remap(|i| place.get(i).copied());
    }

    /// The track after the current one at the end of a track (`ended`) or
    /// on Next: repeat one plays it again only when it ended by itself.
    pub fn next(&mut self, ended: bool) -> Option<usize> {
        self.choose(ended, true)
    }

    /// The track that will follow the current one when it ends (what
    /// `next(true)` will answer), to have it ready: gapless.
    pub fn peek_next(&mut self) -> Option<usize> {
        self.current?;
        self.choose(true, false)
    }

    fn choose(&mut self, ended: bool, take: bool) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if ended && self.repeat == Repeat::One && self.current.is_some() {
            return self.current;
        }
        if self.current.is_none()
            && !self.shuffle
            && let Some(at) = self.resume_at
        {
            // the current track was removed: the one that came after it
            return if at < self.tracks.len() {
                Some(at)
            } else if self.repeat != Repeat::Off || !ended {
                Some(0)
            } else {
                None
            };
        }
        if self.shuffle {
            if self.order.as_ref().is_some_and(Vec::is_empty) {
                if self.repeat == Repeat::Off && ended {
                    if take {
                        self.order = None; // (every track played once: the end; Next starts anew)
                    }
                    return None;
                }
                self.order = None;
            }
            if self.order.is_none() {
                self.reshuffle();
            }
            let chosen = if take {
                self.order.as_mut().and_then(Vec::pop)
            } else {
                self.order.as_ref().and_then(|o| o.last().copied())
            };
            if let Some(i) = chosen {
                return Some(i);
            }
            // a round of one track (the current one): it again only with repeat
            // or on Next
            return self
                .current
                .filter(|_| self.repeat != Repeat::Off || !ended)
                .or_else(|| (!ended).then_some(0));
        }
        match self.current {
            None => Some(0),
            Some(c) if c + 1 < self.tracks.len() => Some(c + 1),
            Some(_) if self.repeat != Repeat::Off || !ended => Some(0),
            Some(_) => None,
        }
    }

    /// The track before: with shuffle the one played before (as long as
    /// the history goes back), else the one above; at the first one the last
    /// with repeat all, else the first again.
    pub fn previous(&mut self) -> Option<usize> {
        if self.shuffle
            && let Some(i) = self.history.pop()
        {
            self.going_back = Some(i);
            return Some(i);
        }
        match self.current.or(self.resume_at) {
            None if !self.tracks.is_empty() => Some(0),
            None => None,
            Some(0) if self.repeat == Repeat::All => Some(self.tracks.len().saturating_sub(1)),
            Some(0) => Some(0),
            Some(c) => Some((c - 1).min(self.tracks.len().saturating_sub(1))),
        }
    }
}

/// What [`collect`] found.
#[derive(Debug, Default)]
pub struct Collected {
    pub tracks: Vec<Track>,
    /// What was not taken, and why where there is a why.
    pub skipped: Vec<String>,
    /// There were more than [`MAX_TRACKS`]: the rest was left out.
    pub truncated: bool,
}

/// The tracks of `args`: audio files, folders (walked, sorted), playlists
/// and stream URLs, in their order; with what was skipped.
pub fn collect(args: &[String]) -> Collected {
    collect_with(args, &mut |_| {})
}

/// [`collect`], telling `progress` how many tracks it found so far (a
/// folder on a network share takes its time).
pub fn collect_with(args: &[String], progress: &mut dyn FnMut(usize)) -> Collected {
    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = HashSet::new();
    for arg in args {
        if tracks.len() > MAX_TRACKS {
            break;
        }
        // a file:// URI (a desktop's "open with" passes them)
        let decoded;
        let arg = match arg.strip_prefix("file://") {
            Some(rest) => {
                decoded = percent_decode(rest.strip_prefix("localhost").unwrap_or(rest));
                &decoded
            }
            None => arg,
        };
        if is_stream(arg) {
            tracks.push(Track {
                path: arg.clone(),
                title: String::new(),
                duration_ms: 0,
            });
            continue;
        }
        let path = PathBuf::from(arg);
        if path.is_dir() {
            walk(&path, 0, &mut tracks, &mut seen, progress);
        } else if PLAYLIST_EXTENSIONS.contains(&extension(&path).as_str()) {
            match read_playlist(&path) {
                Ok(found) => tracks.extend(found),
                Err(why) => skipped.push(format!("{arg}: {why}")),
            }
        } else if path.is_file() && is_audio(&path) {
            tracks.push(Track {
                path: absolute(&path),
                title: String::new(),
                duration_ms: 0,
            });
        } else {
            skipped.push(arg.to_owned());
        }
        progress(tracks.len());
    }
    let truncated = tracks.len() > MAX_TRACKS;
    tracks.truncate(MAX_TRACKS);
    Collected {
        tracks,
        skipped,
        truncated,
    }
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Audio files below `dir`, sorted by name; symbolic links to folders are
/// not followed (a loop), each real folder walked once.
fn walk(
    dir: &Path,
    depth: usize,
    out: &mut Vec<Track>,
    seen: &mut HashSet<PathBuf>,
    progress: &mut dyn FnMut(usize),
) {
    // (one past the limit: whether there was more is known)
    if depth > MAX_DEPTH || out.len() > MAX_TRACKS {
        return;
    }
    let Ok(real) = fs::canonicalize(dir) else {
        return;
    };
    if !seen.insert(real) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for entry in entries {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, depth + 1, out, seen, progress);
        } else if (kind.is_file() || kind.is_symlink()) && is_audio(&path) && path.is_file() {
            out.push(Track {
                path: absolute(&path),
                title: String::new(),
                duration_ms: 0,
            });
            if out.len().is_multiple_of(100) {
                progress(out.len());
            }
        }
        if out.len() > MAX_TRACKS {
            return;
        }
    }
}

/// The entries of an M3U / M3U8 / PLS file: paths relative to it, absolute
/// paths, http(s) streams; titles from #EXTINF / TitleN.
pub fn read_playlist(path: &Path) -> Result<Vec<Track>, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_PLAYLIST_BYTES {
        return Err("too big".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.contains(&0) {
        return Err("not a text file".into());
    }
    // UTF-8 (a BOM allowed), else Windows' Latin-1 (axross's fallbacks)
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => e.into_bytes().iter().map(|b| *b as char).collect(),
    };
    let base = path.parent().unwrap_or(Path::new("."));
    let pls = extension(path) == "pls";
    let mut tracks = Vec::new();
    let mut title = String::new();
    let mut seconds = 0u64;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.len() > MAX_LINE || line.contains('\0') {
            continue;
        }
        let entry = if pls {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            if key.starts_with("title") {
                if let Some(last) = tracks.last_mut() {
                    let last: &mut Track = last;
                    last.title = value.trim().to_owned();
                }
                continue;
            }
            if !key.starts_with("file") {
                continue;
            }
            value.trim().to_owned()
        } else {
            if let Some(info) = line.strip_prefix("#EXTINF:") {
                let (length, name) = info.split_once(',').unwrap_or((info, ""));
                title = name.trim().to_owned();
                // "-1" (a stream) or nonsense: unknown
                seconds = length
                    .split_whitespace()
                    .next()
                    .and_then(|l| l.parse::<f64>().ok())
                    .filter(|l| l.is_finite() && *l > 0.0)
                    .map(|l| l.min(1e9) as u64)
                    .unwrap_or(0);
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            line.to_owned()
        };
        let entry = match entry.strip_prefix("file://") {
            // a file: URL of this computer
            Some(rest) => match rest.strip_prefix("localhost").unwrap_or(rest) {
                local if local.starts_with('/') => percent_decode(local),
                _ => continue,
            },
            None => entry,
        };
        // a Windows playlist's backslashes (on a system that has none in paths)
        let entry = if cfg!(not(windows)) && entry.contains('\\') && !entry.contains('/') {
            entry.replace('\\', "/")
        } else {
            entry
        };
        let resolved = if is_stream(&entry) {
            entry
        } else if entry.contains("://") {
            continue; // (no other scheme: smb://, sftp:// ... are axross's own business)
        } else {
            let p = Path::new(&entry);
            let full = if p.is_absolute() {
                p.to_path_buf()
            } else {
                base.join(p)
            };
            if !is_audio(&full) {
                continue;
            }
            absolute(&full)
        };
        tracks.push(Track {
            path: resolved,
            title: std::mem::take(&mut title),
            duration_ms: std::mem::take(&mut seconds) * 1000,
        });
        if tracks.len() >= MAX_TRACKS {
            break;
        }
    }
    Ok(tracks)
}

/// The playlist as an extended M3U (UTF-8): paths below `base` (the
/// playlist's folder) relative to it, so the folder can move with it.
pub fn to_m3u(tracks: &[Track], base: Option<&Path>) -> String {
    let mut out = String::from("#EXTM3U\n");
    for t in tracks {
        let title = if t.title.is_empty() {
            default_title(&t.path)
        } else {
            t.title.clone()
        };
        let secs = if t.duration_ms > 0 {
            (t.duration_ms / 1000) as i64
        } else {
            -1
        };
        let path = base
            .filter(|_| !is_stream(&t.path))
            .and_then(|b| Path::new(&t.path).strip_prefix(b).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| t.path.clone());
        let flat = |s: &str| s.replace(['\r', '\n'], " ");
        out.push_str(&format!(
            "#EXTINF:{secs},{}\n{}\n",
            flat(&title),
            flat(&path)
        ));
    }
    out
}

/// `bytes` into `path` through a temporary file renamed over it (a crash
/// leaves the old file whole); an existing file's mode is kept.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_file_name(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        std::process::id()
    ));
    let mut file = fs::File::create(&tmp)?;
    let result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| {
            if let Ok(meta) = fs::metadata(path) {
                fs::set_permissions(&tmp, meta.permissions())?;
            }
            fs::rename(&tmp, path)
        });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// %XX sequences back to their bytes (a file: URL's path).
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// How the playlist can be sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Title,
    FileName,
    /// By folder, then file name: an album's tracks together, in order.
    Path,
    Duration,
    Reverse,
}

impl SortKey {
    pub const ALL: [(SortKey, &'static str); 5] = [
        (SortKey::Title, "By title"),
        (SortKey::FileName, "By file name"),
        (SortKey::Path, "By folder"),
        (SortKey::Duration, "By length"),
        (SortKey::Reverse, "Reverse the order"),
    ];
}

/// Does the track match every word of `query` (in its title or path, case
/// not counted)?
pub fn matches(track: &Track, words: &[String]) -> bool {
    if words.is_empty() {
        return true;
    }
    let title = if track.title.is_empty() {
        default_title(&track.path)
    } else {
        track.title.clone()
    };
    let hay = format!("{} {}", title, track.path).to_lowercase();
    words.iter().all(|w| hay.contains(w.as_str()))
}

/// A filter's words (lower case).
pub fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: usize) -> Track {
        Track {
            path: format!("/m/{n}.mp3"),
            title: String::new(),
            duration_ms: 0,
        }
    }

    fn list(n: usize) -> Playlist {
        let mut p = Playlist::default();
        p.add_tracks((0..n).map(track).collect());
        p
    }

    #[test]
    fn next_and_previous_follow_repeat() {
        let mut p = list(3);
        assert_eq!(p.next(false), Some(0));
        p.current = Some(2);
        assert_eq!(p.next(true), None); // the end
        assert_eq!(p.next(false), Some(0)); // Next wraps
        p.repeat = Repeat::All;
        assert_eq!(p.next(true), Some(0));
        p.repeat = Repeat::One;
        assert_eq!(p.next(true), Some(2));
        assert_eq!(p.next(false), Some(0)); // Next leaves the one
        p.current = Some(0);
        assert_eq!(p.previous(), Some(0)); // (wraps with repeat all only)
    }

    #[test]
    fn shuffle_plays_every_track_once_before_the_end() {
        let mut p = list(20);
        p.set_shuffle(true);
        p.current = Some(5);
        let mut seen = HashSet::new();
        while let Some(i) = p.next(true) {
            assert!(seen.insert(i), "{i} twice");
            p.current = Some(i);
            if seen.len() > 30 {
                break;
            }
        }
        assert_eq!(seen.len(), 19);
        assert!(!seen.contains(&5));
    }

    #[test]
    fn moving_and_removing_keep_the_current_track() {
        let mut p = list(5);
        p.current = Some(2);
        p.move_track(0, 4);
        assert_eq!(p.tracks[p.current.unwrap()].path, "/m/2.mp3");
        p.remove(0);
        assert_eq!(p.tracks[p.current.unwrap()].path, "/m/2.mp3");
        let current = p.current.unwrap();
        p.remove(current);
        assert_eq!(p.current, None);
    }

    #[test]
    fn the_track_to_follow_is_known_ahead() {
        for shuffle in [false, true] {
            for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
                let mut p = list(6);
                p.set_shuffle(shuffle);
                p.repeat = repeat;
                p.play(2);
                for _ in 0..14 {
                    let peeked = p.peek_next();
                    assert_eq!(peeked, p.peek_next(), "a second look sees the same");
                    let next = p.next(true);
                    assert_eq!(peeked, next, "shuffle {shuffle} repeat {repeat:?}");
                    match next {
                        Some(i) => p.play(i),
                        None => break,
                    }
                }
            }
        }
    }

    #[test]
    fn shuffle_of_one_track_ends_after_it() {
        let mut p = list(1);
        p.set_shuffle(true);
        p.play(0);
        assert_eq!(p.next(true), None, "the only track played twice");
        p.repeat = Repeat::All;
        assert_eq!(p.next(true), Some(0));
    }

    #[test]
    fn the_track_after_a_removed_current_one_comes_next() {
        // (axross: the track removed keeps playing; Next continues from its place)
        let mut p = list(5);
        p.play(2);
        p.remove(2);
        assert_eq!(p.current, None);
        assert_eq!(p.next(true), Some(2), "/m/3.mp3 moved up to 2");
        p.play(3);
        p.remove(3);
        assert_eq!(p.next(true), None, "it was the last one");
        assert_eq!(p.next(false), Some(0), "Next wraps");
    }

    #[test]
    fn previous_with_shuffle_goes_back_the_way_it_came() {
        let mut p = list(10);
        p.set_shuffle(true);
        p.play(3);
        let mut played = vec![3];
        for _ in 0..4 {
            let i = p.next(true).unwrap();
            p.play(i);
            played.push(i);
        }
        for want in played.iter().rev().skip(1) {
            let back = p.previous().unwrap();
            assert_eq!(back, *want);
            p.play(back);
        }
        // the history used up: the track before in the list
        assert_eq!(p.previous(), Some(2));
    }

    #[test]
    fn previous_at_the_first_track_wraps_only_with_repeat_all() {
        let mut p = list(3);
        p.play(0);
        assert_eq!(p.previous(), Some(0));
        p.repeat = Repeat::All;
        assert_eq!(p.previous(), Some(2));
    }

    #[test]
    fn playlists_of_other_systems_read() {
        let dir = std::env::temp_dir().join(format!("amp-pl2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Ça va")).unwrap();
        // Latin-1 text, a file: URL, Windows' backslashes
        let mut text = b"#EXTM3U\n#EXTINF:5,Caf\xe9\n".to_vec();
        text.extend_from_slice(b"Ca va\\x.mp3\n");
        text.extend_from_slice(
            format!("file://{}/%C3%87a%20va/y.flac\n", dir.display()).as_bytes(),
        );
        text.extend_from_slice(b"file://otherhost/z.mp3\n");
        fs::write(dir.join("list.m3u"), text).unwrap();
        let t = read_playlist(&dir.join("list.m3u")).unwrap();
        assert_eq!(t.len(), 2, "{t:?}");
        assert_eq!(t[0].title, "Café");
        assert!(t[0].path.ends_with("/Ca va/x.mp3"), "{}", t[0].path);
        assert!(t[1].path.ends_with("/Ça va/y.flac"), "{}", t[1].path);
        fs::write(dir.join("bin.m3u"), b"a.mp3\0").unwrap();
        assert!(read_playlist(&dir.join("bin.m3u")).is_err());
    }

    #[test]
    fn sorting_keeps_the_current_track_and_the_history() {
        let mut p = Playlist::default();
        for (name, title, secs) in [
            ("/z/b.mp3", "Gamma", 30),
            ("/a/c.mp3", "alpha", 0),
            ("/a/a.mp3", "Beta", 10),
        ] {
            p.add_tracks(vec![Track {
                path: name.into(),
                title: title.into(),
                duration_ms: secs * 1000,
            }]);
        }
        p.play(0);
        p.play(2);
        let names = |p: &Playlist| p.tracks.iter().map(|t| t.title.clone()).collect::<Vec<_>>();
        p.sort(SortKey::Title);
        assert_eq!(names(&p), ["alpha", "Beta", "Gamma"]);
        assert_eq!(p.tracks[p.current.unwrap()].title, "Beta");
        p.sort(SortKey::Path);
        assert_eq!(names(&p), ["Beta", "alpha", "Gamma"]);
        p.sort(SortKey::Duration);
        assert_eq!(names(&p), ["Beta", "Gamma", "alpha"]);
        p.sort(SortKey::Reverse);
        assert_eq!(names(&p), ["alpha", "Gamma", "Beta"]);
        p.shuffle = true;
        // Previous goes back to Gamma (played before Beta), wherever it is now
        let back = p.previous().unwrap();
        assert_eq!(p.tracks[back].title, "Gamma");
        let words = words("GAM  /z/");
        assert!(matches(&p.tracks[1], &words) && !matches(&p.tracks[0], &words));
    }

    #[test]
    fn playlists_resolve_relative_paths_and_streams() {
        let dir = std::env::temp_dir().join(format!("amp-pl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a.mp3"), b"x").unwrap();
        fs::write(dir.join("b.flac"), b"x").unwrap();
        fs::write(
            dir.join("list.m3u"),
            "#EXTM3U\n#EXTINF:123,Song A\nsub/a.mp3\nhttps://radio.example/live\nfile:///etc/passwd\nnotes.txt\n",
        )
        .unwrap();
        fs::write(
            dir.join("radio.pls"),
            "[playlist]\nFile1=http://r.example/s\nTitle1=Radio\n",
        )
        .unwrap();
        let m3u = read_playlist(&dir.join("list.m3u")).unwrap();
        assert_eq!(m3u.len(), 2);
        assert!(m3u[0].path.ends_with("sub/a.mp3") && m3u[0].title == "Song A");
        assert_eq!((m3u[0].duration_ms, m3u[1].duration_ms), (123_000, 0));
        assert_eq!(m3u[1].path, "https://radio.example/live");
        let pls = read_playlist(&dir.join("radio.pls")).unwrap();
        assert_eq!(
            (pls[0].path.as_str(), pls[0].title.as_str()),
            ("http://r.example/s", "Radio")
        );
        let found = collect(&[dir.to_string_lossy().into_owned(), "nope.xyz".into()]);
        assert_eq!(found.tracks.len(), 2); // b.flac, sub/a.mp3 (playlists are not walked into)
        assert_eq!(found.skipped, vec!["nope.xyz".to_string()]);
        assert!(!found.truncated);
        let saved = to_m3u(&m3u, Some(&dir));
        assert!(saved.contains("#EXTINF:123,Song A\nsub/a.mp3\n"), "{saved}");
        assert!(saved.contains("https://radio.example/live"));
        for name in ["x.opus", "x.webm", "x.OGG"] {
            assert!(is_audio(Path::new(name)), "{name}");
        }
    }
}
