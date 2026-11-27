//! Internet radio and audio files on web servers: an http(s) answer read
//! on a thread of its own into a bounded queue, so whoever decodes it never
//! waits on the network longer than it chooses ([`Body`] gives up after
//! [`STALL`] without a byte, or at once when it is told to stop).
//!
//! * ICY metadata (SHOUTcast / Icecast): asked for, taken out of the sound
//!   and its `StreamTitle` handed on when the decoder reaches it - the song
//!   the radio plays now; the station's name and genre from its headers.
//! * A playlist behind the URL (`.pls`, `.m3u`, as radio directories link
//!   them) is fetched and its first stream played; only http(s) entries
//!   count - a playlist from the web never names a local file.
//! * An old SHOUTcast server's `ICY 200 OK` status line (no HTTP) is
//!   understood.

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The longest a server may take to answer.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// No byte for this long: the stream is broken.
pub const STALL: Duration = Duration::from_secs(20);
/// The largest playlist fetched, and how deep playlists may point to playlists.
const MAX_PLAYLIST_BYTES: u64 = 1 << 20;
const MAX_DEPTH: usize = 3;
/// The bytes read at a time, and how many such reads wait in the queue.
const CHUNK: usize = 16 * 1024;
const QUEUE: usize = 32;
/// ICY: the most metadata a block may carry (255 * 16) and the longest
/// interval between blocks taken seriously.
const MAX_METAINT: usize = 1 << 20;
const MAX_TITLE: usize = 512;
const USER_AGENT: &str = concat!("axross-amp/", env!("CARGO_PKG_VERSION"));

/// What the server said about the stream.
#[derive(Clone, Debug, Default)]
pub struct Answer {
    pub content_type: Option<String>,
    /// `icy-name`, `icy-genre`: the station and its kind of music.
    pub station: String,
    pub genre: String,
    /// The body's length (a file on a web server); None for a radio.
    pub length: Option<u64>,
    /// The server speaks ICY (SHOUTcast / Icecast headers): a radio.
    pub icy: bool,
    /// The URL played (a playlist's entry, where one was resolved).
    pub url: String,
}

/// A titles queue: (the byte of the stream it belongs to, the title).
type Titles = Arc<Mutex<VecDeque<(u64, String)>>>;

/// The body of an answer, as the decoder reads it.
pub struct Body {
    rx: Receiver<Vec<u8>>,
    chunk: Vec<u8>,
    at: usize,
    read: u64,
    /// Set by whoever reads: everything stops.
    cancel: Arc<AtomicBool>,
    /// Set when this body goes: its network thread stops.
    gone: Arc<AtomicBool>,
    titles: Titles,
    on_title: Box<dyn FnMut(String) + Send + Sync>,
    /// Why the network thread stopped, when it was no clean end.
    error: Arc<Mutex<Option<String>>>,
}

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.at >= self.chunk.len() {
            let started = Instant::now();
            loop {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err(io::Error::other("stopped"));
                }
                match self.rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(chunk) => {
                        self.chunk = chunk;
                        self.at = 0;
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if started.elapsed() >= STALL {
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                format!("no data for {} s", STALL.as_secs()),
                            ));
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return match self.error.lock().ok().and_then(|mut e| e.take()) {
                            Some(why) => Err(io::Error::new(io::ErrorKind::ConnectionAborted, why)),
                            None => Ok(0),
                        };
                    }
                }
            }
        }
        let n = buf.len().min(self.chunk.len() - self.at);
        buf[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        self.read += n as u64;
        // a title belongs to the sound after it: told when that is decoded
        loop {
            let due = self.titles.lock().ok().and_then(|mut t| match t.front() {
                Some((at, _)) if *at <= self.read => t.pop_front().map(|(_, s)| s),
                _ => None,
            });
            match due {
                Some(title) => (self.on_title)(title),
                None => break,
            }
        }
        Ok(n)
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        // (the network thread ends at its next read; the queue is closed)
        self.gone.store(true, Ordering::Relaxed);
    }
}

/// Is this the type of a playlist (not of sound)?
fn playlist_type(content_type: &str) -> Option<bool> {
    match content_type {
        "audio/x-scpls" | "application/pls+xml" | "audio/scpls" => Some(true),
        "audio/x-mpegurl"
        | "audio/mpegurl"
        | "application/x-mpegurl"
        | "application/mpegurl"
        | "application/vnd.apple.mpegurl" => Some(false),
        _ => None,
    }
}

/// `.pls` (true) or `.m3u` / `.m3u8` (false) by the URL's path.
fn playlist_extension(url: &str) -> Option<bool> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if path.ends_with(".pls") {
        Some(true)
    } else if path.ends_with(".m3u") || path.ends_with(".m3u8") {
        Some(false)
    } else {
        None
    }
}

/// The value of an ICY `StreamTitle='...';` in a metadata block, clean.
pub fn stream_title(block: &[u8]) -> Option<String> {
    let text = match std::str::from_utf8(block) {
        Ok(t) => t.to_owned(),
        // (many stations send Latin-1)
        Err(_) => block.iter().map(|b| *b as char).collect(),
    };
    let text = text.trim_end_matches('\0');
    let start = text.find("StreamTitle='")? + "StreamTitle='".len();
    let rest = &text[start..];
    let end = rest
        .find("';")
        .or_else(|| rest.rfind('\''))
        .unwrap_or(rest.len());
    Some(clean(&rest[..end]))
}

/// A text from the network as it may be shown: no control or format
/// characters, at most [`MAX_TITLE`] characters.
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE)
        .collect()
}

/// Flags that stop a connection: the whole stream's, and its body's.
#[cfg(not(target_os = "android"))]
type Stops = [Arc<AtomicBool>; 2];

#[cfg(not(target_os = "android"))]
fn stopped(stops: &Stops) -> bool {
    stops.iter().any(|s| s.load(Ordering::Relaxed))
}

/// The agent for one connection: a server that sends nothing for [`STALL`]
/// fails the read; a stop flag set ends a read within a quarter second and
/// the connection with it (its socket closed - a stream let go is let go).
#[cfg(not(target_os = "android"))]
fn agent(stops: Stops) -> ureq::Agent {
    use ureq::unversioned::transport::{Connector, DefaultConnector};
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(CONNECT_TIMEOUT))
        .timeout_per_read(Some(STALL))
        .http_status_as_error(true)
        .user_agent(USER_AGENT)
        .build();
    let connector = DefaultConnector::new().chain(Stoppable { stops });
    ureq::Agent::with_parts(
        config,
        connector,
        ureq::unversioned::resolver::DefaultResolver::default(),
    )
}

#[cfg(not(target_os = "android"))]
use ureq::unversioned::transport::time::Duration as Wait;
#[cfg(not(target_os = "android"))]
use ureq::unversioned::transport::{Buffers, ConnectionDetails, NextTimeout, Transport};

#[cfg(not(target_os = "android"))]
fn exact(wait: Wait) -> Duration {
    match wait {
        Wait::Exact(d) => d,
        Wait::NotHappening => Duration::MAX,
    }
}

/// Wraps a connection so that its reads look at the stop flags.
#[cfg(not(target_os = "android"))]
#[derive(Debug)]
struct Stoppable {
    stops: Stops,
}

#[cfg(not(target_os = "android"))]
impl ureq::unversioned::transport::Connector<Box<dyn Transport>> for Stoppable {
    type Out = StoppableTransport;

    fn connect(
        &self,
        _details: &ConnectionDetails,
        chained: Option<Box<dyn Transport>>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| StoppableTransport {
            inner,
            stops: self.stops.clone(),
        }))
    }
}

#[cfg(not(target_os = "android"))]
#[derive(Debug)]
struct StoppableTransport {
    inner: Box<dyn Transport>,
    stops: Stops,
}

#[cfg(not(target_os = "android"))]
impl Transport for StoppableTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.inner.transmit_output(amount, timeout)
    }

    /// A read in slices of a quarter second: between them the stop flags are
    /// looked at; the time limit it was given stays the limit.
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let started = Instant::now();
        let limit = exact(timeout.after).min(timeout.per_read.map(exact).unwrap_or(Duration::MAX));
        loop {
            if stopped(&self.stops) {
                return Err(ureq::Error::Io(io::Error::other("stopped")));
            }
            let left = limit.saturating_sub(started.elapsed());
            let slice = NextTimeout {
                after: Wait::Exact(
                    left.clamp(Duration::from_millis(1), Duration::from_millis(250)),
                ),
                per_read: None,
                ..timeout
            };
            match self.inner.await_input(slice) {
                Err(ureq::Error::Timeout(_)) if started.elapsed() < limit => continue,
                other => return other,
            }
        }
    }

    fn is_open(&mut self) -> bool {
        !stopped(&self.stops) && self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

/// A socket read in quarter seconds (the old SHOUTcast path): stopped by
/// the flags, failed after [`STALL`] without a byte.
#[cfg(not(target_os = "android"))]
struct StopRead {
    socket: std::net::TcpStream,
    stops: Stops,
}

#[cfg(not(target_os = "android"))]
impl Read for StopRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let started = Instant::now();
        loop {
            if stopped(&self.stops) {
                return Err(io::Error::other("stopped"));
            }
            match self.socket.read(buf) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) && started.elapsed() < STALL =>
                {
                    continue;
                }
                other => return other,
            }
        }
    }
}

/// An answer's head and a reader of its body.
#[cfg(not(target_os = "android"))]
struct Response {
    headers: Vec<(String, String)>,
    body: Box<dyn Read + Send>,
}

#[cfg(not(target_os = "android"))]
impl Response {
    fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim().to_owned())
    }
}

#[cfg(not(target_os = "android"))]
fn request(url: &str, stops: &Stops) -> Result<Response, String> {
    match agent(stops.clone())
        .get(url)
        .header("Icy-MetaData", "1")
        .call()
    {
        Ok(response) => {
            let headers = response
                .headers()
                .iter()
                .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            Ok(Response {
                headers,
                body: Box::new(response.into_body().into_reader()),
            })
        }
        // an answer that is no HTTP: an old SHOUTcast's "ICY 200 OK"
        Err(ureq::Error::Protocol(e)) if url.to_ascii_lowercase().starts_with("http://") => {
            icy_request(url, stops).map_err(|why| format!("{e} ({why})"))
        }
        Err(ureq::Error::StatusCode(code)) => Err(format!("the server answered {code}")),
        Err(e) => Err(e.to_string()),
    }
}

/// HTTP/1.0 by hand for a server that answers `ICY 200 OK`.
#[cfg(not(target_os = "android"))]
fn icy_request(url: &str, stops: &Stops) -> Result<Response, String> {
    let rest = &url["http://".len()..];
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let address = if authority
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
        && !authority.ends_with(']')
    {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    use std::net::ToSocketAddrs;
    let target = address
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let mut socket = std::net::TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)
        .map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(STALL))
        .map_err(|e| e.to_string())?;
    write!(
        socket,
        "GET {path} HTTP/1.0\r\nHost: {authority}\r\nUser-Agent: {USER_AGENT}\r\nIcy-MetaData: 1\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    // the head, byte by byte up to the empty line (at most 16 KiB)
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && !head.ends_with(b"\n\n") {
        if head.len() > 16 * 1024 {
            return Err("an answer with too long a head".into());
        }
        match socket.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            Ok(_) => return Err("the server closed the connection".into()),
            Err(e) => return Err(e.to_string()),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let mut lines = head.lines();
    let status = lines.next().unwrap_or("");
    let code = status.split_whitespace().nth(1).unwrap_or("");
    if !(status.starts_with("ICY ") || status.starts_with("HTTP/")) || code != "200" {
        return Err(format!("the server answered {:?}", clean(status)));
    }
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .collect();
    socket
        .set_read_timeout(Some(Duration::from_millis(250)))
        .map_err(|e| e.to_string())?;
    Ok(Response {
        headers,
        body: Box::new(StopRead {
            socket,
            stops: stops.clone(),
        }),
    })
}

/// The http(s) entries of a playlist's text.
#[cfg(not(target_os = "android"))]
fn playlist_entries(text: &str, pls: bool) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines().take(10_000) {
        let line = line.trim().trim_start_matches('\u{feff}');
        let value = if pls {
            match line.split_once('=') {
                Some((k, v)) if k.trim().to_ascii_lowercase().starts_with("file") => v.trim(),
                _ => continue,
            }
        } else if line.starts_with('#') {
            continue;
        } else {
            line
        };
        if crate::playlist::is_stream(value)
            && value.len() <= 4096
            && !value.contains(char::is_whitespace)
        {
            out.push(value.to_owned());
        }
    }
    out
}

/// Connect to `url`: the answer and its body, read on a thread of its own.
/// `on_title` hears each new ICY title as the decoder reaches it; `cancel`
/// stops everything (the network thread also stops when the [`Body`] goes).
#[cfg(not(target_os = "android"))]
pub fn open(
    url: &str,
    cancel: Arc<AtomicBool>,
    on_title: Box<dyn FnMut(String) + Send + Sync>,
) -> Result<(Answer, Body), String> {
    let mut url = url.to_owned();
    for _ in 0..MAX_DEPTH {
        if cancel.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        let gone = Arc::new(AtomicBool::new(false));
        let response = request(&url, &[cancel.clone(), gone.clone()])?;
        let content_type = response.header("content-type").map(|t| {
            t.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        });
        let length = response
            .header("content-length")
            .and_then(|l| l.parse::<u64>().ok());
        let is_audio = content_type
            .as_deref()
            .is_some_and(|t| t.starts_with("audio/") && playlist_type(t).is_none());
        let pls = content_type
            .as_deref()
            .and_then(playlist_type)
            .or_else(|| (!is_audio).then(|| playlist_extension(&url)).flatten());
        if let Some(pls) = pls {
            // a playlist: its first stream that answers
            let mut text = String::new();
            response
                .body
                .take(MAX_PLAYLIST_BYTES)
                .read_to_string(&mut text)
                .map_err(|e| format!("the playlist could not be read ({e})"))?;
            if text.contains("#EXT-X-") {
                return Err("an HLS stream (segments in an .m3u8) - not supported".into());
            }
            let entries = playlist_entries(&text, pls);
            let Some(first) = entries.first() else {
                return Err("the playlist names no http(s) stream".into());
            };
            url = first.clone();
            continue;
        }
        let metaint = response
            .header("icy-metaint")
            .map(|m| {
                m.parse::<usize>()
                    .ok()
                    .filter(|m| (1..=MAX_METAINT).contains(m))
            })
            .map(|m| m.ok_or("the server sent a broken icy-metaint"))
            .transpose()?;
        let answer = Answer {
            content_type,
            station: response
                .header("icy-name")
                .map(|s| clean(&s))
                .unwrap_or_default(),
            genre: response
                .header("icy-genre")
                .map(|s| clean(&s))
                .unwrap_or_default(),
            length,
            icy: response.headers.iter().any(|(n, _)| {
                let n = n.to_ascii_lowercase();
                n.starts_with("icy-") || n.starts_with("ice-")
            }),
            url: url.clone(),
        };
        let (tx, rx) = crossbeam_channel::bounded(QUEUE);
        let titles: Titles = Arc::default();
        let error = Arc::new(Mutex::new(None));
        let (t, e, c, g) = (titles.clone(), error.clone(), cancel.clone(), gone.clone());
        let reader = response.body;
        thread::Builder::new()
            .name("amp-net".into())
            .spawn(move || pump(reader, metaint, tx, t, e, [c, g]))
            .map_err(|e| e.to_string())?;
        return Ok((
            answer,
            Body {
                rx,
                chunk: Vec::new(),
                at: 0,
                read: 0,
                cancel,
                gone,
                titles,
                on_title,
                error,
            },
        ));
    }
    Err("playlists point to playlists too deep".into())
}

/// Read the body into `tx` (the ICY metadata taken out), until it ends,
/// fails, or nobody reads any more.
#[cfg(not(target_os = "android"))]
fn pump(
    mut reader: Box<dyn Read + Send>,
    metaint: Option<usize>,
    tx: Sender<Vec<u8>>,
    titles: Titles,
    error: Arc<Mutex<Option<String>>>,
    stop: [Arc<AtomicBool>; 2],
) {
    let fail = |why: String| {
        if let Ok(mut e) = error.lock() {
            *e = Some(why);
        }
    };
    let mut sent: u64 = 0;
    let mut since_meta = 0usize;
    let mut last_title: Option<String> = None;
    loop {
        if stop.iter().any(|s| s.load(Ordering::Relaxed)) {
            return;
        }
        let want = match metaint {
            Some(m) => (m - since_meta).min(CHUNK),
            None => CHUNK,
        };
        let mut buf = vec![0u8; want];
        let n = match reader.read(&mut buf) {
            Ok(0) => return, // (the end: the queue closes)
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return fail(e.to_string()),
        };
        buf.truncate(n);
        sent += n as u64;
        since_meta += n;
        if tx.send(buf).is_err() {
            return;
        }
        if let Some(m) = metaint
            && since_meta == m
        {
            since_meta = 0;
            let mut len = [0u8; 1];
            if let Err(e) = reader.read_exact(&mut len) {
                return fail(e.to_string());
            }
            let mut block = vec![0u8; len[0] as usize * 16];
            if let Err(e) = reader.read_exact(&mut block) {
                return fail(e.to_string());
            }
            if let Some(title) = stream_title(&block)
                && last_title.as_ref() != Some(&title)
            {
                last_title = Some(title.clone());
                if let Ok(mut t) = titles.lock() {
                    t.push_back((sent, title));
                    while t.len() > 16 {
                        t.pop_front();
                    }
                }
            }
        }
    }
}

/// This build has no network (Android: the app plays the phone's own files
/// and asks for no network permission): no internet radio, no file from a
/// web server.
#[cfg(target_os = "android")]
pub fn open(
    _url: &str,
    _cancel: Arc<AtomicBool>,
    _on_title: Box<dyn FnMut(String) + Send + Sync>,
) -> Result<(Answer, Body), String> {
    Err("no network in this build: only the device's own files play".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_icy_blocks() {
        assert_eq!(
            stream_title(b"StreamTitle='Artist - Song';StreamUrl='';\0\0\0").as_deref(),
            Some("Artist - Song")
        );
        assert_eq!(
            stream_title(b"StreamTitle='It's mine';").as_deref(),
            Some("It's mine")
        );
        assert_eq!(stream_title(b"StreamTitle='';").as_deref(), Some(""));
        assert_eq!(
            stream_title(b"StreamTitle='Caf\xe9\n\x1b[31m';").as_deref(),
            Some("Café [31m")
        );
        assert_eq!(stream_title(b"StreamUrl='x';"), None);
    }

    #[test]
    fn playlists_from_the_web_name_only_streams() {
        let pls = "[playlist]\nFile1=http://a.example/live\nFile2=/etc/passwd\nTitle1=A\nFile3=file:///x.mp3\n";
        assert_eq!(playlist_entries(pls, true), vec!["http://a.example/live"]);
        let m3u = "#EXTM3U\n#EXTINF:-1,R\nhttps://b.example/s.mp3\nrelative.mp3\n";
        assert_eq!(
            playlist_entries(m3u, false),
            vec!["https://b.example/s.mp3"]
        );
        assert_eq!(playlist_extension("http://x/y.PLS?a=1"), Some(true));
        assert_eq!(playlist_extension("http://x/y.mp3"), None);
    }
}
