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
}

impl Default for Playlist {
    fn default() -> Self {
        Playlist {
            tracks: Vec::new(),
            current: None,
            shuffle: false,
            repeat: Repeat::Off,
            order: None,
            seed: 0x9e3779b97f4a7c15,
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
        added
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.tracks.len() {
            return;
        }
        self.tracks.remove(index);
        self.current = match self.current {
            Some(c) if c == index => None,
            Some(c) if c > index => Some(c - 1),
            other => other,
        };
        self.order = None;
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current = None;
        self.order = None;
    }

    /// Move the track at `from` to `to` (the current one stays current).
    pub fn move_track(&mut self, from: usize, to: usize) {
        if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
            return;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        self.current = self.current.map(|c| {
            if c == from {
                to
            } else if from < c && c <= to {
                c - 1
            } else if to <= c && c < from {
                c + 1
            } else {
                c
            }
        });
        self.order = None;
    }

    /// The track after the current one at the end of a track (`ended`) or
    /// on Next: repeat one plays it again only when it ended by itself.
    pub fn next(&mut self, ended: bool) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if ended && self.repeat == Repeat::One {
            return self.current;
        }
        if self.shuffle {
            if self.order.as_ref().is_some_and(Vec::is_empty) {
                if self.repeat == Repeat::Off && ended {
                    self.order = None; // (every track played once: the end; Next starts anew)
                    return None;
                }
                self.order = None;
            }
            if self.order.is_none() {
                self.reshuffle();
            }
            return self.order.as_mut().and_then(Vec::pop).or(self.current);
        }
        match self.current {
            None => Some(0),
            Some(c) if c + 1 < self.tracks.len() => Some(c + 1),
            Some(_) if self.repeat != Repeat::Off || !ended => Some(0),
            Some(_) => None,
        }
    }

    pub fn previous(&self) -> Option<usize> {
        match self.current {
            None if !self.tracks.is_empty() => Some(0),
            Some(0) => Some(self.tracks.len().saturating_sub(1)),
            Some(c) => Some(c - 1),
            None => None,
        }
    }
}

/// The tracks of `args`: audio files, folders (walked, sorted), playlists
/// and stream URLs, in their order; with what was skipped.
pub fn collect(args: &[String]) -> (Vec<Track>, Vec<String>) {
    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = HashSet::new();
    for arg in args {
        if tracks.len() >= MAX_TRACKS {
            break;
        }
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
            walk(&path, 0, &mut tracks, &mut seen);
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
            skipped.push(arg.clone());
        }
    }
    tracks.truncate(MAX_TRACKS);
    (tracks, skipped)
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Audio files below `dir`, sorted by name; symbolic links to folders are
/// not followed (a loop), each real folder walked once.
fn walk(dir: &Path, depth: usize, out: &mut Vec<Track>, seen: &mut HashSet<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_TRACKS {
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
            walk(&path, depth + 1, out, seen);
        } else if (kind.is_file() || kind.is_symlink()) && is_audio(&path) && path.is_file() {
            out.push(Track {
                path: absolute(&path),
                title: String::new(),
                duration_ms: 0,
            });
        }
        if out.len() >= MAX_TRACKS {
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
    let text = String::from_utf8_lossy(&bytes);
    let base = path.parent().unwrap_or(Path::new("."));
    let pls = extension(path) == "pls";
    let mut tracks = Vec::new();
    let mut title = String::new();
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
                title = info
                    .split_once(',')
                    .map(|(_, t)| t.trim().to_owned())
                    .unwrap_or_default();
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            line.to_owned()
        };
        let resolved = if is_stream(&entry) {
            entry
        } else if entry.contains("://") {
            continue; // (no other scheme: file://, smb:// ... are axross's own business)
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
            duration_ms: 0,
        });
        if tracks.len() >= MAX_TRACKS {
            break;
        }
    }
    Ok(tracks)
}

/// The playlist as an extended M3U.
pub fn to_m3u(tracks: &[Track]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for t in tracks {
        let secs = if t.duration_ms > 0 {
            (t.duration_ms / 1000) as i64
        } else {
            -1
        };
        let title = if t.title.is_empty() {
            default_title(&t.path)
        } else {
            t.title.clone()
        };
        out.push_str(&format!(
            "#EXTINF:{secs},{}\n{}\n",
            title.replace('\n', " "),
            t.path
        ));
    }
    out
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
        assert_eq!(p.previous(), Some(2));
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
        assert_eq!(m3u[1].path, "https://radio.example/live");
        let pls = read_playlist(&dir.join("radio.pls")).unwrap();
        assert_eq!(
            (pls[0].path.as_str(), pls[0].title.as_str()),
            ("http://r.example/s", "Radio")
        );
        let (found, skipped) = collect(&[dir.to_string_lossy().into_owned(), "nope.xyz".into()]);
        assert_eq!(found.len(), 2); // b.flac, sub/a.mp3 (playlists are not walked into)
        assert_eq!(skipped, vec!["nope.xyz".to_string()]);
        assert!(to_m3u(&m3u).contains("#EXTINF:-1,Song A"));
        for name in ["x.opus", "x.webm", "x.OGG"] {
            assert!(is_audio(Path::new(name)), "{name}");
        }
    }
}
