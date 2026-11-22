//! The playlist: files, folders (walked), M3U / PLS playlists and http(s)
//! streams; shuffle and repeat as axross's player has them.

use crate::state::{MAX_TRACKS, Track};
use std::collections::{HashMap, HashSet};
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
/// The most of a playlist file read (its first MAX_TRACKS entries are well
/// within it: 10,000 long paths with their titles are some 2 MiB).
const MAX_PLAYLIST_BYTES: u64 = 64 << 20;
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
    /// Tracks to play next, before the list's own order ("Play next"); the
    /// track the list goes on from after them, and the one handed out last.
    up_next: std::collections::VecDeque<usize>,
    return_to: Option<usize>,
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
            up_next: Default::default(),
            return_to: None,
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
        // the next of those asked to play next: the list goes on later from
        // where it is now; another track chosen: from that one
        if self.up_next.front() == Some(&index) {
            self.return_to = self.return_to.or(self.current);
        } else {
            self.return_to = None;
        }
        self.current = Some(index);
        self.resume_at = None;
        self.up_next.retain(|i| *i != index);
        if let Some(order) = self.order.as_mut() {
            order.retain(|i| *i != index);
        }
    }

    /// Every index kept (current, history, the place of a removed track)
    /// through `map`: None drops it.
    fn remap(&mut self, map: impl Fn(usize) -> Option<usize>) {
        self.version += 1;
        self.return_to = self.return_to.and_then(&map);
        self.up_next = self.up_next.iter().filter_map(|i| map(*i)).collect();
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

    /// The list replaced by `tracks` (another program's), `current` the
    /// track playing in it.
    pub fn replace(&mut self, tracks: Vec<Track>, current: Option<usize>) {
        self.tracks = tracks;
        self.tracks.truncate(MAX_TRACKS);
        self.current = current.filter(|c| *c < self.tracks.len());
        self.order = None;
        self.history.clear();
        self.going_back = None;
        self.resume_at = None;
        self.up_next.clear();
        self.version += 1;
    }

    /// Play `indexes` next, in this order, after what was asked before.
    pub fn play_next(&mut self, indexes: &[usize]) {
        for i in indexes {
            if *i < self.tracks.len() && !self.up_next.contains(i) && Some(*i) != self.current {
                self.up_next.push_back(*i);
            }
        }
        self.version += 1;
    }

    /// Where track `index` is among those to play next (1 the next).
    pub fn up_next_place(&self, index: usize) -> Option<usize> {
        self.up_next.iter().position(|i| *i == index).map(|p| p + 1)
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
        self.up_next.clear();
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

    /// Move the tracks at `picked` (any order) together to where the track
    /// at `onto` is: before it when they come from below it, after it when
    /// from above (as a row dragged onto another). Their new places.
    pub fn move_tracks(&mut self, picked: &[usize], onto: usize) -> Vec<usize> {
        let n = self.tracks.len();
        let mut picked: Vec<usize> = picked.iter().copied().filter(|i| *i < n).collect();
        picked.sort_unstable();
        picked.dedup();
        if picked.is_empty() || onto >= n || picked.contains(&onto) {
            return picked;
        }
        let rest: Vec<usize> = (0..n).filter(|i| !picked.contains(i)).collect();
        let at = rest.iter().position(|i| *i == onto).unwrap_or(rest.len());
        let at = if picked[0] < onto { at + 1 } else { at };
        let mut order = rest.clone();
        order.splice(at..at, picked.iter().copied());
        let mut place = vec![0; n];
        for (new, old) in order.iter().enumerate() {
            place[*old] = new;
        }
        let mut moved: Vec<Track> = order.iter().map(|i| self.tracks[*i].clone()).collect();
        std::mem::swap(&mut self.tracks, &mut moved);
        self.resume_at = None;
        self.remap(|i| place.get(i).copied());
        (at..at + picked.len()).collect()
    }

    /// Remove the tracks at `indexes` (a current one among them plays on, as
    /// with [`Playlist::remove`]).
    pub fn remove_many(&mut self, indexes: &[usize]) {
        let mut indexes = indexes.to_vec();
        indexes.sort_unstable();
        indexes.dedup();
        for i in indexes.into_iter().rev() {
            self.remove(i);
        }
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
        let folder = |t: &Track| {
            Path::new(&t.path)
                .parent()
                .map(|d| d.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        let tracks = &self.tracks;
        // (names compare naturally: "2 - y" before "10 - x")
        let by = |key: &dyn Fn(&Track) -> String| {
            let keys: Vec<String> = tracks.iter().map(key).collect();
            move |a: &usize, b: &usize| natural_cmp(&keys[*a], &keys[*b])
        };
        match key {
            SortKey::Title => order.sort_by(by(&title)),
            SortKey::FileName => order.sort_by(by(&name)),
            SortKey::Path => {
                let (f, n) = (by(&folder), by(&name));
                order.sort_by(|a, b| f(a, b).then_with(|| n(a, b)));
            }
            // an album's tracks in their order (disc, number), the albums by
            // name; tracks without one last
            SortKey::Album | SortKey::Artist => {
                let artist = by(&|t: &Track| t.artist.to_lowercase());
                let album = by(&|t: &Track| t.album.to_lowercase());
                let (t, n) = (by(&title), by(&name));
                let by_artist = key == SortKey::Artist;
                order.sort_by(|a, b| {
                    let (x, y) = (&tracks[*a], &tracks[*b]);
                    let missing = |t: &Track| {
                        if by_artist {
                            t.artist.is_empty()
                        } else {
                            t.album.is_empty()
                        }
                    };
                    missing(x)
                        .cmp(&missing(y))
                        .then_with(|| {
                            if by_artist {
                                artist(a, b)
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        })
                        .then_with(|| album(a, b))
                        .then_with(|| x.disc.cmp(&y.disc))
                        .then_with(|| (x.track == 0).cmp(&(y.track == 0)))
                        .then_with(|| x.track.cmp(&y.track))
                        .then_with(|| n(a, b))
                        .then_with(|| t(a, b))
                });
            }
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
        // what was asked to play next comes first; then the list goes on
        // from where it was
        // (taken from the queue when it is played: Playlist::play)
        if let Some(&next) = self.up_next.front() {
            return Some(next);
        }
        if let Some(back) = self.return_to.filter(|r| *r < self.tracks.len()) {
            let here = self.current.replace(back);
            let chosen = self.choose_in_order(ended, take);
            self.current = here;
            if take {
                self.return_to = None;
            }
            return chosen;
        }
        self.choose_in_order(ended, take)
    }

    fn choose_in_order(&mut self, ended: bool, take: bool) -> Option<usize> {
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
                decoded = url_path(rest.strip_prefix("localhost").unwrap_or(rest));
                &decoded
            }
            None => arg,
        };
        if is_stream(arg) {
            tracks.push(Track {
                path: arg.clone(),
                title: String::new(),
                duration_ms: 0,
                ..Default::default()
            });
            continue;
        }
        let path = crate::fspath::to_path(arg);
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
                ..Default::default()
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
    crate::fspath::to_string(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
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
    entries.sort_by(|a, b| {
        natural_cmp(
            &a.file_name().to_string_lossy(),
            &b.file_name().to_string_lossy(),
        )
    });
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
                ..Default::default()
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
    // (read a line at a time, up to the first MAX_TRACKS entries - a
    // playlist over 1 MiB, the player's own of 10,000 tracks, was refused
    // whole as "too big"; what is read at most is bounded all the same)
    use std::io::{BufRead, Read};
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(file.take(MAX_PLAYLIST_BYTES));
    if reader
        .fill_buf()
        .map_err(|e| e.to_string())?
        .iter()
        .take(8192)
        .any(|b| *b == 0)
    {
        return Err("not a text file".into());
    }
    // UTF-8 (a BOM allowed), else Windows-1252 (axross's fallbacks; a
    // Winamp playlist), line by line - and a line ends at CR too (an old
    // Mac's playlist ends its lines with CR alone: it read as one line)
    let lines = reader.split(b'\n').map_while(Result::ok).flat_map(|bytes| {
        bytes
            .split(|b| *b == b'\r')
            .map(|part| match std::str::from_utf8(part) {
                Ok(text) => text.to_owned(),
                Err(_) => windows_1252(part),
            })
            .collect::<Vec<_>>()
    });
    let base = path.parent().unwrap_or(Path::new("."));
    let pls = extension(path) == "pls";
    // where an entry is: a stream, a local path (a file: URL of this
    // computer, relative to the playlist's folder), or nothing to play
    let resolve = |entry: String| -> Option<String> {
        let entry = if let Some(rest) = entry.strip_prefix("file://") {
            // a file: URL of this computer
            match rest.strip_prefix("localhost").unwrap_or(rest) {
                local if local.starts_with('/') => url_path(local),
                _ => return None,
            }
        } else if let Some(local) = entry.strip_prefix("file:").filter(|l| l.starts_with('/')) {
            // (file:/path, one slash: a local path too, not a relative one)
            url_path(local)
        } else {
            entry
        };
        // a Windows playlist's backslashes (on a system that has none in paths)
        let entry = if cfg!(not(windows)) && entry.contains('\\') && !entry.contains('/') {
            entry.replace('\\', "/")
        } else {
            entry
        };
        if is_stream(&entry) {
            Some(entry)
        } else if entry.contains("://") {
            None // (no other scheme: smb://, sftp:// ... are axross's own business)
        } else {
            let p = Path::new(&entry);
            let full = if p.is_absolute() {
                p.to_path_buf()
            } else {
                base.join(p)
            };
            is_audio(&full).then(|| absolute(&full))
        }
    };
    let mut tracks = Vec::new();
    // (a PLS by its entries' numbers, File10 after File3, each its TitleN)
    let mut numbered: Vec<(u64, usize, String)> = Vec::new();
    let mut titles: HashMap<u64, String> = HashMap::new();
    let mut title = String::new();
    let mut seconds = 0u64;
    for raw in lines {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.len() > MAX_LINE || line.contains('\0') {
            continue;
        }
        if pls {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let number = |name: &str| key.strip_prefix(name).and_then(|n| n.parse::<u64>().ok());
            if let Some(n) = number("title") {
                titles.insert(n, value.trim().to_owned());
            } else if key.starts_with("file") && numbered.len() < MAX_TRACKS * 2 {
                let n = number("file").unwrap_or(u64::MAX);
                numbered.push((n, numbered.len(), value.trim().to_owned()));
            }
            continue;
        }
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
        let Some(resolved) = resolve(line.to_owned()) else {
            continue;
        };
        tracks.push(Track {
            path: resolved,
            title: std::mem::take(&mut title),
            duration_ms: std::mem::take(&mut seconds) * 1000,
            ..Default::default()
        });
        if tracks.len() >= MAX_TRACKS {
            break;
        }
    }
    numbered.sort();
    for (n, _, entry) in numbered {
        let Some(resolved) = resolve(entry) else {
            continue;
        };
        tracks.push(Track {
            path: resolved,
            title: titles.remove(&n).unwrap_or_default(),
            ..Default::default()
        });
        if tracks.len() >= MAX_TRACKS {
            break;
        }
    }
    Ok(tracks)
}

/// Windows-1252 (a Winamp playlist's): Latin-1 but for 0x80 - 0x9F, where
/// it has "€", "’", "—" ... (read as Latin-1 they were C1 controls, and the
/// tracks were not found); the five it leaves out as Latin-1's.
fn windows_1252(bytes: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}',
        '\u{8f}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}',
        '\u{2014}', '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}',
        '\u{178}',
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
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
        // (a name that a reader takes for something else - a comment,
        // starting with '#'; spaces at its ends, trimmed - as a file:// URL
        // of its whole path: "#1 Crush.mp3" and " Intro.mp3" were lost)
        let path = if !is_stream(&t.path)
            && (path.starts_with('#')
                || path.starts_with(char::is_whitespace)
                || path.ends_with(char::is_whitespace)
                || path.contains(['\r', '\n']))
        {
            file_url(&absolute(Path::new(&t.path)))
        } else {
            path
        };
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
/// leaves the old file whole); an existing file's mode is kept. Nothing is
/// written through a link: the temporary file is made anew under a name of
/// its own (O_EXCL - a link planted at that name is refused, not
/// followed), its mode set on the open file, and a link at `path` itself
/// is replaced by the playlist, never written through (review 13: the
/// predictable name `.<name>.tmp-<pid>` planted as a link to another file
/// in a shared folder overwrote that file and made it world-readable).
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    // (a link of the user's own - a playlist kept on a NAS, linked into
    // ~/Music - is the list's place: the list it names is saved, in its own
    // folder; a link someone else made in a shared folder is not followed -
    // review 14: the user's own link was replaced and the list itself never
    // updated)
    let followed = match own_link_target(path) {
        Linked::Follow(target) => Some(target),
        Linked::Replace => None,
        // (a link of the user's own that leads nowhere - a NAS not mounted,
        // a loop, a chain past 40 links: the save refused, the link kept;
        // it was replaced by a plain file - review 16, L10)
        Linked::Refuse(why) => return Err(std::io::Error::other(why)),
    };
    let path = followed.as_deref().unwrap_or(path);
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // (the mode of the file there - of the file itself, not of what a
    // link there points to)
    let mode = fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.file_type().is_file())
        .map(|m| m.permissions());
    let mut tries = 0;
    let (tmp, mut file) = loop {
        let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let tmp = path.with_file_name(format!(
            ".{name}.tmp-{}-{n}-{nanos:08x}",
            std::process::id()
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&tmp) {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && tries < 16 => {
                tries += 1;
            }
            Err(e) => return Err(e),
        }
    };
    let result = file
        .write_all(bytes)
        .and_then(|_| match &mode {
            Some(m) => file.set_permissions(m.clone()),
            None => {
                // (a new playlist: as a file made by the user's umask would be)
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(fs::Permissions::from_mode(0o644 & !umask()))
                }
                #[cfg(not(unix))]
                Ok(())
            }
        })
        .and_then(|_| file.sync_all())
        .and_then(|_| crate::fspath::rename_over(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// What a save does with a link at a playlist's name.
#[derive(Debug, PartialEq)]
enum Linked {
    /// No link there, or one someone else made (or leading through
    /// someone else's link, file or folder): the playlist takes its place.
    Replace,
    /// The user's own chain of links: the file at its end is saved.
    Follow(std::path::PathBuf),
    /// The user's own link leading nowhere (not there, a loop, more than
    /// 40 links): not saved.
    Refuse(String),
}

/// Where a link at `path` that this user made points - one link at a
/// time, each one, the folder it lies in, and the file at the end, the
/// user's own (a link of the user's pointing into a folder others may
/// write in could be turned there to any other file of the user's: review
/// 15; each folder's owner too: review 16, L10).
fn own_link_target(path: &Path) -> Linked {
    let mine = |_: &Path, meta: &fs::Metadata| -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid has no preconditions and cannot fail.
            meta.uid() == unsafe { libc::geteuid() }
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            true
        }
    };
    link_target_by(path, &mine)
}

/// [`own_link_target`] by `mine`: whose each step is.
fn link_target_by(path: &Path, mine: &dyn Fn(&Path, &fs::Metadata) -> bool) -> Linked {
    let mut at = path.to_path_buf();
    let is_link = fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink());
    if !is_link {
        return Linked::Replace;
    }
    // (as the system follows them: 40 at most)
    for hop in 0..=40 {
        let Ok(meta) = fs::symlink_metadata(&at) else {
            return Linked::Refuse(format!(
                "{} leads to {}, which is not there",
                path.display(),
                at.display()
            ));
        };
        if !mine(&at, &meta) {
            return Linked::Replace;
        }
        // (past the first: the folder it lies in the user's, or one only
        // its owner may write in)
        if hop > 0
            && let Some(folder) = at.parent().filter(|f| !f.as_os_str().is_empty())
            && let Ok(fm) = fs::metadata(folder)
            && !mine(folder, &fm)
            && others_write(&fm)
        {
            return Linked::Replace;
        }
        if meta.file_type().is_file() {
            return Linked::Follow(at);
        }
        if !meta.file_type().is_symlink() {
            return Linked::Replace;
        }
        let Ok(next) = fs::read_link(&at) else {
            return Linked::Replace;
        };
        at = match at.parent() {
            Some(parent) if next.is_relative() => parent.join(next),
            _ => next,
        };
    }
    Linked::Refuse(format!("{}: too many links (a loop?)", path.display()))
}

/// Whether others than a folder's owner may write in it.
fn others_write(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o022 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// The process's umask, as Linux says it (/proc/self/status), else 022.
#[cfg(unix)]
fn umask() -> u32 {
    static UMASK: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *UMASK.get_or_init(|| {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Umask:"))
                    .and_then(|v| u32::from_str_radix(v.trim(), 8).ok())
            })
            .unwrap_or(0o022)
    })
}

/// A local path as a file:// URL (what is not plain, percent-encoded). On
/// Windows `C:\Music\a b.mp3` is `file:///C:/Music/a%20b.mp3` (it was
/// `file://C%3A%5CMusic...`, which no reader - this one neither - takes
/// for a local file).
pub fn file_url(path: &str) -> String {
    let mut out = String::from("file://");
    let path = if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    let mut rest = path.as_str();
    if cfg!(windows) && has_drive(rest) {
        out.push('/');
        out.push_str(&rest[..2]);
        rest = &rest[2..];
    }
    for b in rest.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path of a file: URL of this computer, from what follows `file://`
/// and its host (none or `localhost`) - or `file:` alone: `/x/a%20b.mp3` is
/// `/x/a b.mp3`; on Windows `/C:/x/a.mp3` is `C:/x/a.mp3`.
pub fn url_path(local: &str) -> String {
    let path = percent_decode(local);
    match path.strip_prefix('/') {
        Some(drive) if cfg!(windows) && has_drive(drive) => drive.to_owned(),
        _ => path,
    }
}

/// Whether `path` begins with a drive letter and its colon (`C:`).
fn has_drive(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
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
    /// By album, disc and track number (from the tags).
    Album,
    /// By artist, then album, disc and track number.
    Artist,
    Duration,
    Reverse,
}

impl SortKey {
    pub const ALL: [(SortKey, &'static str); 7] = [
        (SortKey::Title, "By title"),
        (SortKey::Artist, "By artist"),
        (SortKey::Album, "By album"),
        (SortKey::FileName, "By file name"),
        (SortKey::Path, "By folder"),
        (SortKey::Duration, "By length"),
        (SortKey::Reverse, "Reverse the order"),
    ];
}

/// Names compared as people read them: runs of digits by their number
/// ("2" before "10"), the rest without regard to case.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut run = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        run.push(c);
                        it.next();
                    }
                    run
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                let order = c.to_lowercase().cmp(d.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
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
            ..Default::default()
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

    /// Names that start with '#' or a space, saved and read back whole
    /// (review 11: "#1 Crush.mp3" was read as a comment, " Intro.mp3" as
    /// "Intro.mp3"); and a playlist of 10,000 tracks - over 1 MiB - read
    /// back (it was refused as "too big"), its first 10,000 entries.
    #[test]
    fn a_saved_playlist_reads_back_whole() {
        let dir = std::env::temp_dir().join(format!("amp-m3u-{}", std::process::id()));
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let names = ["#1 Crush.mp3", " Intro.mp3", "Outro .mp3", "ok.mp3"];
        let tracks: Vec<Track> = names
            .iter()
            .map(|n| Track {
                path: music.join(n).to_string_lossy().into_owned(),
                ..Default::default()
            })
            .collect();
        let list = music.join("list.m3u");
        std::fs::write(&list, to_m3u(&tracks, Some(&music))).unwrap();
        let back = read_playlist(&list).unwrap();
        let paths: Vec<&str> = back.iter().map(|t| t.path.as_str()).collect();
        let want: Vec<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(paths, want);
        let many: Vec<Track> = (0..MAX_TRACKS + 50)
            .map(|i| Track {
                // (a path of this system's: read back as it was written)
                path: music
                    .join("Some Artist")
                    .join("Some Album (Deluxe)")
                    .join(format!("{:02} - A Song Title {i}.flac", i % 20))
                    .to_string_lossy()
                    .into_owned(),
                title: format!("Some Artist - A Song Title Number {i}"),
                duration_ms: 245_000,
                ..Default::default()
            })
            .collect();
        let big = dir.join("big.m3u");
        let text = to_m3u(&many, None);
        assert!(text.len() as u64 > 1 << 20);
        std::fs::write(&big, &text).unwrap();
        let back = read_playlist(&big).unwrap();
        assert_eq!(back.len(), MAX_TRACKS);
        assert_eq!(back[MAX_TRACKS - 1].path, many[MAX_TRACKS - 1].path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn playlists_of_other_systems_read() {
        let dir = crate::testdir::Scratch::new("pl2");
        fs::create_dir_all(dir.join("Ça va")).unwrap();
        // Latin-1 text, a file: URL, Windows' backslashes
        let mut text = b"#EXTM3U\n#EXTINF:5,Caf\xe9\n".to_vec();
        text.extend_from_slice(b"Ca va\\x.mp3\n");
        text.extend_from_slice(
            format!("{}/%C3%87a%20va/y.flac\n", file_url(&dir.to_string_lossy())).as_bytes(),
        );
        text.extend_from_slice(b"file://otherhost/z.mp3\n");
        fs::write(dir.join("list.m3u"), text).unwrap();
        let t = read_playlist(&dir.join("list.m3u")).unwrap();
        assert_eq!(t.len(), 2, "{t:?}");
        assert_eq!(t[0].title, "Café");
        // (on Windows a backslash is a separator of its own)
        assert!(
            Path::new(&t[0].path).ends_with("Ca va/x.mp3"),
            "{}",
            t[0].path
        );
        assert!(
            Path::new(&t[1].path).ends_with("Ça va/y.flac"),
            "{}",
            t[1].path
        );
        fs::write(dir.join("bin.m3u"), b"a.mp3\0").unwrap();
        assert!(read_playlist(&dir.join("bin.m3u")).is_err());
    }

    #[test]
    fn play_next_comes_first_and_follows_the_tracks() {
        let mut p = list(6);
        p.play(0);
        p.play_next(&[4, 2]);
        assert_eq!(p.peek_next(), Some(4));
        // a track above them goes: they move up with their tracks
        p.remove(1);
        assert_eq!((p.up_next_place(3), p.up_next_place(1)), (Some(1), Some(2)));
        assert_eq!(p.next(true), Some(3));
        p.play(3);
        assert_eq!(p.next(true), Some(1));
        p.play(1);
        assert_eq!(
            p.next(true),
            Some(1),
            "then the list goes on from where it was"
        );
    }

    #[test]
    fn several_tracks_move_and_go_together() {
        let mut p = list(6);
        p.play(1);
        // 1 and 2 dragged onto 4: after it
        assert_eq!(p.move_tracks(&[2, 1], 4), vec![3, 4]);
        let names: Vec<&str> = p.tracks.iter().map(|t| &t.path[3..4]).collect();
        assert_eq!(names, ["0", "3", "4", "1", "2", "5"]);
        assert_eq!(p.current, Some(3), "the current track goes with them");
        // 4 and 5 dragged onto 0: before it
        p.move_tracks(&[4, 5], 0);
        let names: Vec<&str> = p.tracks.iter().map(|t| &t.path[3..4]).collect();
        assert_eq!(names, ["2", "5", "0", "3", "4", "1"]);
        p.remove_many(&[0, 5, 2]);
        let names: Vec<&str> = p.tracks.iter().map(|t| &t.path[3..4]).collect();
        assert_eq!(names, ["5", "3", "4"]);
    }

    #[test]
    fn names_sort_as_they_are_read() {
        let mut names = vec!["10 - x", "2 - y", "Track 1", "track 01b", "B", "a", "1"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            ["1", "2 - y", "10 - x", "a", "B", "Track 1", "track 01b"]
        );
        let mut p = Playlist::default();
        let t = |path: &str, album: &str, disc: u32, track: u32| Track {
            path: path.into(),
            album: album.into(),
            disc,
            track,
            ..Default::default()
        };
        p.add_tracks(vec![
            t("/m/z.mp3", "B side", 1, 2),
            t("/m/loose.mp3", "", 0, 0),
            t("/m/y.mp3", "B side", 1, 10),
            t("/m/x.mp3", "B side", 2, 1),
            t("/m/w.mp3", "a side", 0, 1),
            t("/m/10.mp3", "", 0, 0),
            t("/m/9.mp3", "", 0, 0),
        ]);
        p.sort(SortKey::Album);
        let paths: Vec<&str> = p.tracks.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/m/w.mp3",
                "/m/z.mp3",
                "/m/y.mp3",
                "/m/x.mp3",
                "/m/9.mp3",
                "/m/10.mp3",
                "/m/loose.mp3"
            ]
        );
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
                ..Default::default()
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

    /// Playlists from elsewhere (review 12): old Mac line ends (CR only:
    /// read as 0 tracks), a Winamp playlist in Windows-1252 ("€", "’" read
    /// as Latin-1's C1 controls: those tracks not found), a `file:/path`
    /// URL (one slash: taken as relative), a PLS whose File10 comes before
    /// File3 (taken in the file's order; titles by their numbers).
    /// A file: URL of this system's paths and back (on Windows with the
    /// drive after three slashes, as other programs write and read them).
    #[test]
    fn file_urls_of_this_systems_paths() {
        if cfg!(windows) {
            assert_eq!(
                file_url(r"C:\Music\a b#.mp3"),
                "file:///C:/Music/a%20b%23.mp3"
            );
            assert_eq!(url_path("/C:/Music/a%20b%23.mp3"), "C:/Music/a b#.mp3");
        } else {
            assert_eq!(file_url(r"/m/C:\x #.mp3"), "file:///m/C%3A%5Cx%20%23.mp3");
            assert_eq!(url_path("/C:/x%20%23.mp3"), "/C:/x #.mp3");
        }
        assert_eq!(url_path("/m/a%20b.mp3"), "/m/a b.mp3");
    }

    #[test]
    fn playlists_from_elsewhere_read_as_meant() {
        let dir = crate::testdir::Scratch::new("pl-elsewhere");
        for name in ["a.mp3", "b.mp3", "c\u{20ac}.mp3", "d\u{2019}s.mp3", "e.mp3"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let at = |t: &Track| {
            Path::new(&t.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        fs::write(
            dir.join("cr.m3u"),
            "#EXTM3U\r#EXTINF:1,A\ra.mp3\rb.mp3\re.mp3\r",
        )
        .unwrap();
        let cr = read_playlist(&dir.join("cr.m3u")).unwrap();
        assert_eq!(
            cr.iter().map(at).collect::<Vec<_>>(),
            ["a.mp3", "b.mp3", "e.mp3"]
        );
        assert_eq!(cr[0].title, "A");
        // (€ is 0x80, ’ 0x92 in Windows-1252)
        fs::write(dir.join("w.m3u"), b"#EXTM3U\r\nc\x80.mp3\r\nd\x92s.mp3\r\n").unwrap();
        let w = read_playlist(&dir.join("w.m3u")).unwrap();
        assert_eq!(
            w.iter().map(at).collect::<Vec<_>>(),
            ["c\u{20ac}.mp3", "d\u{2019}s.mp3"]
        );
        // (file:///C:/... on Windows; one slash fewer: file:/C:/...)
        let url = file_url(&dir.to_string_lossy());
        let one_slash = format!("file:{}", &url["file://".len()..]);
        fs::write(
            dir.join("urls.m3u"),
            format!("{one_slash}/a.mp3\n{url}/b.mp3\n"),
        )
        .unwrap();
        let urls = read_playlist(&dir.join("urls.m3u")).unwrap();
        assert_eq!(urls.iter().map(at).collect::<Vec<_>>(), ["a.mp3", "b.mp3"]);
        assert!(Path::new(&urls[0].path).exists(), "{}", urls[0].path);
        fs::write(
            dir.join("order.pls"),
            "[playlist]\nNumberOfEntries=4\nFile1=a.mp3\nTitle1=T1\nFile2=b.mp3\nTitle2=T2\n\
             File10=e.mp3\nTitle10=T10\nFile3=c\u{20ac}.mp3\nTitle3=T3\n",
        )
        .unwrap();
        let pls = read_playlist(&dir.join("order.pls")).unwrap();
        assert_eq!(
            pls.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            ["T1", "T2", "T3", "T10"]
        );
        assert_eq!(at(&pls[3]), "e.mp3");
    }

    #[test]
    fn playlists_resolve_relative_paths_and_streams() {
        let dir = crate::testdir::Scratch::new("pl");
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
        assert!(Path::new(&m3u[0].path).ends_with("sub/a.mp3") && m3u[0].title == "Song A");
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
        // (below the playlist's folder: relative, in the system's own way)
        let relative = Path::new("sub").join("a.mp3");
        assert!(
            saved.contains(&format!("#EXTINF:123,Song A\n{}\n", relative.display())),
            "{saved}"
        );
        assert!(saved.contains("https://radio.example/live"));
        for name in ["x.opus", "x.webm", "x.OGG"] {
            assert!(is_audio(Path::new(name)), "{name}");
        }
    }

    /// A link planted where the temporary file would be, or at the
    /// playlist's own name, is never written through (review 13, M11: a
    /// link at `.<name>.tmp-<pid>` to another file overwrote that file and
    /// made it mode 0666).
    #[cfg(unix)]
    #[test]
    fn a_saved_playlist_writes_through_no_link() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = crate::testdir::Scratch::new("pl-links");
        let victim = dir.join("victim.txt");
        fs::write(&victim, b"secret").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        let list = dir.join("list.m3u");
        // the names the temporary file had (the process's id)
        symlink(
            &victim,
            dir.join(format!(".list.m3u.tmp-{}", std::process::id())),
        )
        .unwrap();
        write_atomically(&list, b"#EXTM3U\n").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"secret");
        let mode = |p: &Path| fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&victim), 0o600);
        assert!(fs::symlink_metadata(&list).unwrap().file_type().is_file());
        assert_eq!(fs::read(&list).unwrap(), b"#EXTM3U\n");
        // a link at the playlist's name made by someone else (as root,
        // the test can make one so): replaced by the playlist, the file it
        // pointed to left as it was
        if unsafe { libc::geteuid() } == 0 {
            let linked = dir.join("linked.m3u");
            symlink(&victim, &linked).unwrap();
            // (where the system lets root give it away: not in every sandbox)
            if std::os::unix::fs::lchown(&linked, Some(65534), Some(65534)).is_ok() {
                write_atomically(&linked, b"#EXTM3U\nx\n").unwrap();
                assert_eq!(fs::read(&victim).unwrap(), b"secret");
                assert_eq!(mode(&victim), 0o600);
                assert!(fs::symlink_metadata(&linked).unwrap().file_type().is_file());
            }
        }
        // a playlist's own mode kept
        fs::set_permissions(&list, fs::Permissions::from_mode(0o640)).unwrap();
        write_atomically(&list, b"#EXTM3U\ny\n").unwrap();
        assert_eq!(mode(&list), 0o640);
        // no temporary file left
        let left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-") && !n.ends_with(&format!("-{}", std::process::id())))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    /// A link of the user's own at the playlist's name (a list kept on a
    /// NAS, linked into ~/Music): the list it names is saved, the link
    /// stays (review 14, L4: the link was replaced, the list never updated).
    #[cfg(unix)]
    #[test]
    fn a_playlist_saved_through_the_users_own_link_updates_the_list() {
        let dir = crate::testdir::Scratch::new("pl-own-link");
        std::fs::create_dir_all(dir.join("nas")).unwrap();
        let real = dir.join("nas/list.m3u");
        fs::write(&real, b"#EXTM3U\nold.mp3\n").unwrap();
        let link = dir.join("list.m3u");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomically(&link, b"#EXTM3U\nnew.mp3\n").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"#EXTM3U\nnew.mp3\n");
        let left = fs::read_dir(dir.join("nas")).unwrap().count();
        assert_eq!(left, 1, "a temporary file left");
    }

    /// A chain of links: each link, and the file at its end, the user's own,
    /// else the playlist replaces the first link (review 15, L3: only the
    /// first link's owner was asked).
    #[cfg(unix)]
    #[test]
    fn a_chain_of_links_is_followed_only_while_each_is_the_users() {
        use std::os::unix::fs::symlink;
        let dir = crate::testdir::Scratch::new("pl-link-chain");
        let real = dir.join("list-real.m3u");
        fs::write(&real, b"#EXTM3U\nold\n").unwrap();
        let middle = dir.join("middle.m3u");
        symlink(&real, &middle).unwrap();
        let first = dir.join("list.m3u");
        symlink("middle.m3u", &first).unwrap();
        write_atomically(&first, b"#EXTM3U\nnew\n").unwrap();
        assert_eq!(fs::read(&real).unwrap(), b"#EXTM3U\nnew\n");
        assert!(
            fs::symlink_metadata(&first)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        // a link in the middle, or the file at the end, someone else's: not
        // followed
        let by = |other: &'static str| move |p: &Path, _: &fs::Metadata| !p.ends_with(other);
        assert_eq!(link_target_by(&first, &by("middle.m3u")), Linked::Replace);
        assert_eq!(
            link_target_by(&first, &by("list-real.m3u")),
            Linked::Replace
        );
        assert_eq!(
            link_target_by(&first, &by("nobody")),
            Linked::Follow(dir.join("list-real.m3u"))
        );
        // (and where the system lets root give the middle link away, the
        // first link is replaced)
        if unsafe { libc::geteuid() } == 0
            && std::os::unix::fs::lchown(&middle, Some(65534), Some(65534)).is_ok()
        {
            write_atomically(&first, b"#EXTM3U\nmine\n").unwrap();
            assert_eq!(fs::read(&real).unwrap(), b"#EXTM3U\nnew\n");
            assert!(fs::symlink_metadata(&first).unwrap().file_type().is_file());
        }
    }

    /// The user's own link leading nowhere (a NAS not mounted), or round in
    /// a loop: the save is refused and the link kept - it was replaced by a
    /// plain file; a chain of 20 links of the user's is followed (review 16,
    /// L10: past 7 it was replaced).
    #[cfg(unix)]
    #[test]
    fn a_link_leading_nowhere_is_kept_and_a_long_chain_followed() {
        use std::os::unix::fs::symlink;
        let dir = crate::testdir::Scratch::new("pl-link-nowhere");
        let link = dir.join("list.m3u");
        symlink(dir.join("nas/list.m3u"), &link).unwrap();
        assert!(write_atomically(&link, b"#EXTM3U\nnew\n").is_err());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let looped = dir.join("loop.m3u");
        symlink("loop.m3u", &looped).unwrap();
        assert!(write_atomically(&looped, b"#EXTM3U\nnew\n").is_err());
        assert!(
            fs::symlink_metadata(&looped)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let real = dir.join("real.m3u");
        fs::write(&real, b"#EXTM3U\nold\n").unwrap();
        let mut next = real.clone();
        for k in 0..20 {
            let hop = dir.join(format!("hop{k}.m3u"));
            symlink(&next, &hop).unwrap();
            next = hop;
        }
        write_atomically(&next, b"#EXTM3U\nnew\n").unwrap();
        assert_eq!(fs::read(&real).unwrap(), b"#EXTM3U\nnew\n");
        assert!(
            fs::symlink_metadata(&next)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
