//! Where the sound of a file lies, for the files symphonia measures or
//! seeks wrongly - read from the file's own framing, without decoding:
//!
//! * **Chained Ogg** (Vorbis, Opus, FLAC): a file of several links, each a
//!   physical stream of its own (another serial, perhaps another rate) - as
//!   an Icecast station sends one at each song and as a recording of it is
//!   kept. symphonia knows only the link it reads: its length is the first
//!   link's, a seek stays in the link it is in. [`ogg_links`] finds where
//!   each link begins and how long it is.
//! * **An MP3 without a Xing, Info or VBRI header**: symphonia estimates its
//!   length from the bitrate of its first frames - half the length of a VBR
//!   file that is loud first, ten times that of one quiet first - and cuts
//!   the sound at that estimate. [`mp3_frames`] counts its frames and keeps
//!   where every [`MP3_EVERY`]th one begins (a seek from there).
//!
//! [`Window`] gives a file from a byte on, as if it began there: a format
//! reader opened on it reads one link, or the MP3 from a frame on.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// A link of a chained Ogg file: where its first page is (bytes), when its
/// sound begins and how long it is (s).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub offset: u64,
    pub start: f64,
    pub secs: f64,
    /// Where its own time begins in its granule positions (s): 0, or the
    /// station's count where a recording of it joined.
    pub base: f64,
}

/// What an Ogg page's header says.
struct Page {
    flags: u8,
    granule: i64,
    serial: u32,
    /// The first bytes of its body (a first page's: its first packet's start).
    head: Vec<u8>,
}

fn read_page(r: &mut BufReader<File>) -> Option<Page> {
    let mut header = [0u8; 27];
    r.read_exact(&mut header).ok()?;
    if &header[..4] != b"OggS" || header[4] != 0 {
        return None;
    }
    let segments = header[26] as usize;
    let mut table = vec![0u8; segments];
    r.read_exact(&mut table).ok()?;
    let body: u64 = table.iter().map(|s| *s as u64).sum();
    let flags = header[5];
    let mut head = Vec::new();
    let mut skip = body;
    if flags & 0x02 != 0 {
        // (a first page: its first packet says which codec, at what rate)
        let want = body.min(64) as usize;
        head.resize(want, 0);
        r.read_exact(&mut head).ok()?;
        skip -= want as u64;
    }
    r.seek_relative(skip as i64).ok()?;
    Some(Page {
        flags,
        granule: i64::from_le_bytes(header[6..14].try_into().ok()?),
        serial: u32::from_le_bytes(header[14..18].try_into().ok()?),
        head,
    })
}

/// The rate a first packet's granule positions count in, the frames
/// before the sound (Opus' pre-skip), and the most a page of it can hold
/// (255 packets of the longest it has): Opus, Vorbis and FLAC.
/// A link's codec from its first packet: its rate, pre-skip, the most
/// samples a page of it holds, and what a decoder starting in the middle
/// of it - a recording - cannot give of its first packet (Vorbis: a block
/// overlaps the one before it, and the first one read gives nothing).
fn codec_of(packet: &[u8]) -> Option<(u32, u64, u64, u64)> {
    if packet.starts_with(b"OpusHead") && packet.len() >= 12 {
        let preskip = u16::from_le_bytes([packet[10], packet[11]]) as u64;
        // (a packet: at most 120 ms)
        return Some((48_000, preskip, 255 * 5760, 0));
    }
    if packet.starts_with(b"\x01vorbis") && packet.len() >= 29 {
        let rate = u32::from_le_bytes(packet[12..16].try_into().ok()?);
        // (a packet: at most half the long block)
        let long = 1u64 << (packet[28] >> 4).min(13);
        return (rate > 0).then_some((rate, 0, 255 * long / 2, long / 2));
    }
    if packet.starts_with(b"\x7fFLAC") && packet.len() >= 30 && &packet[9..13] == b"fLaC" {
        let s = &packet[17..];
        let rate = ((s[10] as u32) << 12) | ((s[11] as u32) << 4) | ((s[12] as u32) >> 4);
        let block = u16::from_be_bytes([s[2], s[3]]).max(16) as u64;
        return (rate > 0).then_some((rate, 0, 255 * block, 0));
    }
    None
}

/// The links of a chained Ogg file, or of a recording of an Ogg station
/// (one link whose time does not begin at 0) - None otherwise (one link
/// from 0, not Ogg, or a link whose length cannot be told).
pub fn ogg_links(path: &Path) -> Option<Vec<Link>> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut r = BufReader::with_capacity(16 << 10, file);
    struct Scan {
        offset: u64,
        serial: Option<u32>,
        rate: u32,
        preskip: u64,
        /// The most samples a page of it can hold; what a recording's first
        /// packet does not give.
        page_most: u64,
        lost: u64,
        /// The granule positions of its first two audio pages, its last;
        /// whether its last page ends the stream (EOS).
        first: [i64; 2],
        last: i64,
        ended: bool,
    }
    let mut links: Vec<Scan> = Vec::new();
    let mut in_first_pages = false;
    let mut at = 0u64;
    while at < len {
        let Some(page) = read_page(&mut r) else {
            break; // (a broken tail: what came before stands)
        };
        if page.flags & 0x02 != 0 {
            if !in_first_pages {
                links.push(Scan {
                    offset: at,
                    serial: None,
                    rate: 0,
                    preskip: 0,
                    page_most: 0,
                    lost: 0,
                    first: [-1; 2],
                    last: -1,
                    ended: false,
                });
            }
            in_first_pages = true;
            if let Some(link) = links.last_mut()
                && link.serial.is_none()
                && let Some((rate, preskip, most, lost)) = codec_of(&page.head)
            {
                (
                    link.serial,
                    link.rate,
                    link.preskip,
                    link.page_most,
                    link.lost,
                ) = (Some(page.serial), rate, preskip, most, lost);
            }
        } else {
            in_first_pages = false;
            if let Some(link) = links.last_mut()
                && link.serial == Some(page.serial)
                && page.granule >= 0
            {
                // (the first two pages of sound: a header page says 0)
                if page.granule == 0 {
                } else if link.first[0] < 0 {
                    link.first[0] = page.granule;
                } else if link.first[1] < 0 {
                    link.first[1] = page.granule;
                }
                link.last = page.granule;
                link.ended = page.flags & 0x04 != 0;
            }
        }
        at = r.stream_position().ok()?;
    }
    let mut out = Vec::with_capacity(links.len());
    let mut start = 0.0;
    let ended = links.last().is_some_and(|l| l.ended);
    let many = links.len() > 64;
    for (k, link) in links.into_iter().enumerate() {
        link.serial?;
        if link.last < 0 || link.rate == 0 {
            return None;
        }
        // where its granule positions begin: 0 for a file made from the
        // start; a recording of a station joins it with the station's
        // running count (hours in: review 14, a 2 s recording said it was
        // 3 hours long and could not be sought). The first page of sound's
        // own samples counted, packet by packet (symphonia's reader parses
        // them: Vorbis's modes, Opus's TOC), its granule less them is where
        // the link's time begins - exactly: a quiet start's tiny packets in
        // one page of seconds are its own samples (review 15), and a
        // recording joined a few seconds into a song is one (review 16, M6:
        // a page's capacity - 30 s of Opus - was the bound, and such a
        // recording was taken for a file of its own). (In a file of very
        // many links, the first ones counted so, the others by the bound.)
        let [g1, g2] = link.first;
        let counted = if !many || k < 8 {
            first_sample(path, link.offset)
        } else {
            None
        };
        let base = match counted {
            Some(ts) if ts > i64::from(link.rate / 100) => ts as u64,
            Some(_) => 0,
            None if g1 > 0 && g1 as u64 > link.page_most + link.preskip => {
                let page = if g2 > g1 { g2 - g1 } else { 0 };
                (g1 - page).max(0) as u64
            }
            None => 0,
        };
        let rate = link.rate as f64;
        // (a recording's first packet gives nothing: its length is the
        // less - its time is not: the next packet's sound is where it was)
        let lost = if base > 0 { link.lost } else { 0 };
        let secs = (link.last as u64)
            .saturating_sub(base)
            .saturating_sub(link.preskip)
            .saturating_sub(lost) as f64
            / rate;
        out.push(Link {
            offset: link.offset,
            start,
            secs,
            base: base as f64 / rate,
        });
        start += secs;
    }
    // (one link counted from 0, ended: symphonia's own length and seek are
    // right; without its end - a download cut short, a recording stopped -
    // symphonia knows no length, and no seek was offered: review 16, L9)
    let plain = out.len() == 1 && out[0].base == 0.0 && ended;
    (!out.is_empty() && !plain).then_some(out)
}

/// Where the sound of the link at `offset` begins in its granule positions:
/// its first page of sound's granule less the samples of its packets
/// (symphonia's Ogg reader counts them) - 0 or less for a file made from
/// the start; None when it cannot be read so.
fn first_sample(path: &Path, offset: u64) -> Option<i64> {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    let window = Window::open(path, offset).ok()?;
    let mss = MediaSourceStream::new(Box::new(window), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("ogg");
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let track = format.default_track(TrackType::Audio)?;
    Some(track.start_ts.get())
}

/// Ogg's page checksum (CRC-32, polynomial 0x04C11DB7, not reflected,
/// over the page with its checksum field as zeros).
fn ogg_crc(page: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 {
                    (r << 1) ^ 0x04C1_1DB7
                } else {
                    r << 1
                };
            }
            r
        })
    });
    page.iter().enumerate().fold(0u32, |crc, (i, b)| {
        let b = if (22..26).contains(&i) { 0 } else { *b };
        (crc << 8) ^ table[((crc >> 24) as u8 ^ b) as usize]
    })
}

/// Where an Ogg file's first damaged page begins (a page whose checksum
/// fails, or no page where one should be) - None when every page is whole
/// or the file is no Ogg. symphonia's reader refuses such a file at its
/// opening (it reads to the end): what comes before is still to be played
/// (review 14).
pub fn ogg_damage(path: &Path) -> Option<u64> {
    let mut file = BufReader::with_capacity(64 << 10, File::open(path).ok()?);
    let len = file.get_ref().metadata().ok()?.len();
    let mut at = 0u64;
    let mut page = Vec::new();
    while at < len {
        let mut header = [0u8; 27];
        if file.read_exact(&mut header).is_err() || &header[..4] != b"OggS" {
            return (at > 0).then_some(at);
        }
        let mut table = vec![0u8; header[26] as usize];
        if file.read_exact(&mut table).is_err() {
            return Some(at);
        }
        let body: usize = table.iter().map(|s| *s as usize).sum();
        page.clear();
        page.extend_from_slice(&header);
        page.extend_from_slice(&table);
        let start = page.len();
        page.resize(start + body, 0);
        if file.read_exact(&mut page[start..]).is_err() {
            return Some(at);
        }
        let stored = u32::from_le_bytes(header[22..26].try_into().ok()?);
        if ogg_crc(&page) != stored {
            return (at > 0).then_some(at);
        }
        at += page.len() as u64;
    }
    None
}

/// The whole pages of a damaged Ogg file, as byte ranges (start, length) -
/// the damaged ones left out, the reading found again at the next whole
/// page; None when the file is whole or no Ogg. (Damage in its first page
/// of sound left nothing before it to play: review 15.)
pub fn ogg_whole_pages(path: &Path) -> Option<Vec<(u64, u64)>> {
    ogg_damage(path)?;
    let data = std::fs::read(path).ok()?;
    let page_at = |at: usize| -> Option<usize> {
        let header = data.get(at..at + 27)?;
        if &header[..4] != b"OggS" {
            return None;
        }
        let segments = header[26] as usize;
        let table = data.get(at + 27..at + 27 + segments)?;
        let len = 27 + segments + table.iter().map(|s| *s as usize).sum::<usize>();
        let page = data.get(at..at + len)?;
        let stored = u32::from_le_bytes(page[22..26].try_into().ok()?);
        (ogg_crc(page) == stored).then_some(len)
    };
    let mut pieces: Vec<(u64, u64)> = Vec::new();
    let mut at = 0usize;
    while at + 27 <= data.len() {
        match page_at(at) {
            Some(len) => {
                match pieces.last_mut() {
                    Some((s, l)) if *s + *l == at as u64 => *l += len as u64,
                    _ => pieces.push((at as u64, len as u64)),
                }
                at += len;
            }
            None => {
                // (the next page whole after the damage)
                at = match data[at + 1..].windows(4).position(|w| w == b"OggS") {
                    Some(k) => at + 1 + k,
                    None => break,
                };
            }
        }
    }
    (!pieces.is_empty()).then_some(pieces)
}

/// The frames of an MP3 without a Xing, Info or VBRI header.
#[derive(Clone, Debug, PartialEq)]
pub struct Mp3Frames {
    pub rate: u32,
    /// Samples of each frame (1152, or 576 for MPEG-2 layer III, 384 layer I).
    pub per_frame: u32,
    pub frames: u64,
    /// Where frame `i * MP3_EVERY` begins (bytes).
    pub offsets: Vec<u64>,
}

/// One offset of an MP3 frame is kept for this many frames.
pub const MP3_EVERY: u64 = 8;

impl Mp3Frames {
    pub fn secs(&self) -> f64 {
        (self.frames * self.per_frame as u64) as f64 / self.rate as f64
    }
}

/// An MPEG audio frame header: (version, layer, rate, samples, length).
/// version 3 MPEG-1, 2 MPEG-2, 0 MPEG-2.5; layer 3, 2 or 1.
fn mpeg_header(h: [u8; 4]) -> Option<(u8, u8, u32, u32, u32)> {
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (h[1] >> 3) & 3;
    let layer = match (h[1] >> 1) & 3 {
        1 => 3,
        2 => 2,
        3 => 1,
        _ => return None,
    };
    if version == 1 {
        return None;
    }
    let index = (h[2] >> 4) as usize;
    if index == 0 || index == 15 {
        return None; // (free format, or no bitrate)
    }
    const V1: [[u32; 15]; 3] = [
        [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ],
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ],
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ],
    ];
    const V2: [[u32; 15]; 2] = [
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ],
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    ];
    let kbps = if version == 3 {
        V1[layer as usize - 1][index]
    } else {
        V2[usize::from(layer != 1)][index]
    };
    let rate = match ((h[2] >> 2) & 3, version) {
        (3, _) => return None,
        (i, 3) => [44_100, 48_000, 32_000][i as usize],
        (i, 2) => [22_050, 24_000, 16_000][i as usize],
        (i, _) => [11_025, 12_000, 8_000][i as usize],
    };
    let padding = ((h[2] >> 1) & 1) as u32;
    let bits = kbps * 1000;
    let (samples, length) = match layer {
        1 => (384, (12 * bits / rate + padding) * 4),
        2 => (1152, 144 * bits / rate + padding),
        _ if version == 3 => (1152, 144 * bits / rate + padding),
        _ => (576, 72 * bits / rate + padding),
    };
    (length > 4).then_some((version, layer, rate, samples, length))
}

/// The size of an ID3v2 tag at the start of `head` (0 when there is none).
fn id3v2_size(head: &[u8]) -> u64 {
    if head.len() < 10 || &head[..3] != b"ID3" {
        return 0;
    }
    let size = head[6..10]
        .iter()
        .fold(0u64, |s, b| (s << 7) | (*b & 0x7F) as u64);
    let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
    10 + size + footer
}

/// The frames of the MP3 at `path` when it has no Xing, Info or VBRI
/// header (None: it has one - symphonia's length is right then -, or it
/// is not an MP3 this can read: free format, no two frames in a row).
pub fn mp3_frames(path: &Path) -> Option<Mp3Frames> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut r = BufReader::with_capacity(64 << 10, file);
    let mut head = [0u8; 10];
    r.read_exact(&mut head).ok()?;
    let mut at = id3v2_size(&head);
    // the first frame: a header followed by another of the same kind
    let first = {
        let mut found = None;
        let mut buf = Vec::new();
        r.seek(SeekFrom::Start(at)).ok()?;
        r.by_ref().take(1 << 20).read_to_end(&mut buf).ok()?;
        let mut i = 0usize;
        while i + 4 <= buf.len() {
            if let Some(h) = mpeg_header(buf[i..i + 4].try_into().ok()?) {
                let next = i + h.4 as usize;
                if let Some(n) = buf
                    .get(next..next + 4)
                    .and_then(|b| mpeg_header(b.try_into().ok()?))
                    && (n.0, n.1, n.2) == (h.0, h.1, h.2)
                {
                    // (an info frame: symphonia's length is right)
                    let frame = &buf[i..(i + h.4 as usize).min(buf.len())];
                    let info = frame
                        .windows(4)
                        .take(64)
                        .any(|w| w == b"Xing" || w == b"Info" || w == b"VBRI");
                    if info {
                        return None;
                    }
                    found = Some((i as u64, h));
                    break;
                }
            }
            i += 1;
        }
        found?
    };
    at += first.0;
    let (version, layer, rate, per_frame, _) = first.1;
    let mut frames = 0u64;
    let mut offsets = Vec::new();
    r.seek(SeekFrom::Start(at)).ok()?;
    let mut lost = 0u64;
    while at + 4 <= len {
        let mut h = [0u8; 4];
        if r.read_exact(&mut h).is_err() {
            break;
        }
        match mpeg_header(h) {
            Some((v, l, rt, _, length)) if (v, l, rt) == (version, layer, rate) => {
                if at + length as u64 > len {
                    break; // (a frame cut short at the end is not decoded)
                }
                if frames.is_multiple_of(MP3_EVERY) {
                    offsets.push(at);
                }
                frames += 1;
                lost = 0;
                at += length as u64;
                r.seek_relative(length as i64 - 4).ok()?;
            }
            _ => {
                if &h[..3] == b"TAG" || &h == b"APET" || &h == b"LYRI" {
                    break; // (the tags at the end)
                }
                // (junk between frames: a byte on, as the reader does -
                // for a while)
                lost += 1;
                if lost > 64 << 10 {
                    break;
                }
                at += 1;
                r.seek_relative(-3).ok()?;
            }
        }
    }
    (frames > 0).then_some(Mp3Frames {
        rate,
        per_frame,
        frames,
        offsets,
    })
}

/// Pieces of a file read as one (a damaged Ogg file's whole pages).
pub struct Pieces {
    file: File,
    pieces: Vec<(u64, u64)>,
    len: u64,
    at: u64,
}

impl Pieces {
    pub fn open(path: &Path, pieces: Vec<(u64, u64)>) -> std::io::Result<Pieces> {
        let len = pieces.iter().map(|(_, l)| l).sum();
        Ok(Pieces {
            file: File::open(path)?,
            pieces,
            len,
            at: 0,
        })
    }
}

impl Read for Pieces {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut from = 0u64;
        for (start, len) in &self.pieces {
            if self.at < from + len {
                let within = self.at - from;
                let n = buf.len().min((len - within) as usize);
                self.file.seek(SeekFrom::Start(start + within))?;
                let n = self.file.read(&mut buf[..n])?;
                self.at += n as u64;
                return Ok(n);
            }
            from += len;
        }
        Ok(0)
    }
}

impl Seek for Pieces {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let to = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.at as i128 + d as i128,
        };
        if to < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a seek before the start",
            ));
        }
        self.at = to as u64;
        Ok(self.at)
    }
}

impl symphonia::core::io::MediaSource for Pieces {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// A file from byte `start` on, as if it began there.
pub struct Window {
    file: File,
    start: u64,
    len: u64,
}

impl Window {
    pub fn open(path: &Path, start: u64) -> std::io::Result<Window> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len().saturating_sub(start);
        file.seek(SeekFrom::Start(start))?;
        Ok(Window { file, start, len })
    }

    /// The file from `start` to `end` only (what comes after - damage -
    /// is as if it were not there).
    pub fn range(path: &Path, start: u64, end: u64) -> std::io::Result<Window> {
        let mut w = Window::open(path, start)?;
        w.len = w.len.min(end.saturating_sub(start));
        Ok(w)
    }
}

impl Read for Window {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let at = self.file.stream_position()? - self.start;
        let left = self.len.saturating_sub(at) as usize;
        let n = buf.len().min(left);
        self.file.read(&mut buf[..n])
    }
}

impl Seek for Window {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let to = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => (self.file.stream_position()? - self.start) as i128 + d as i128,
        };
        if to < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a seek before the start",
            ));
        }
        Ok(self.file.seek(SeekFrom::Start(self.start + to as u64))? - self.start)
    }
}

impl symphonia::core::io::MediaSource for Window {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/media")
            .join(name)
    }

    #[test]
    fn the_links_of_a_chained_ogg_file() {
        let links = ogg_links(&media("chained.ogg")).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].offset, 0);
        assert!((links[0].secs - 1.5).abs() < 0.03, "{links:?}");
        assert!((links[1].start - links[0].secs).abs() < 1e-9);
        assert!((links[1].secs - 1.5).abs() < 0.03, "{links:?}");
        assert_eq!(links[1].base, 0.0);
        let opus = ogg_links(&media("chained.opus")).unwrap();
        assert!((opus[0].secs - 1.5).abs() < 0.01, "{opus:?}");
        // one link (and anything else) is not a chain
        let one = std::fs::read(media("chained.opus")).unwrap();
        let dir = crate::testdir::Scratch::new("one-link");
        let path = dir.join("one.opus");
        std::fs::write(&path, &one[..opus[1].offset as usize]).unwrap();
        assert_eq!(ogg_links(&path), None);
        assert_eq!(ogg_links(&media("vbr-noxing-short.mp3")), None);
    }

    /// A recording of an Ogg station: its granule positions begin hours in;
    /// its length is what it holds, from there (review 14: 10 802 s for 2 s).
    #[test]
    fn a_recording_of_a_station_is_as_long_as_it_is() {
        let one = ogg_links(&media("capture1.ogg")).unwrap();
        assert_eq!(one.len(), 1);
        assert!((one[0].secs - 2.0).abs() < 0.05, "{one:?}");
        assert!((one[0].base - 3.0 * 3600.0).abs() < 0.1, "{one:?}");
        let two = ogg_links(&media("capture.ogg")).unwrap();
        assert_eq!(two.len(), 2);
        assert!((two[1].start - two[0].secs).abs() < 1e-9);
        assert!((two[0].secs + two[1].secs - 4.0).abs() < 0.1, "{two:?}");
        assert_eq!(two[1].base, 0.0);
    }

    /// Vorbis paged as libogg pages it, its first page seconds of silence:
    /// an ordinary file, not a recording (review 15: 6.24 s said for 8,
    /// every seek 1.76 s late).
    #[test]
    fn a_quiet_start_is_no_recording() {
        for (name, secs) in [
            ("lo-sil3-2tone.ogg", 8.0),
            ("lo-sil2-2tone.ogg", 7.0),
            ("lo-sil3.ogg", 8.0),
            ("lo-sil6.ogg", 0.0),
            ("lo-plain.ogg", 0.0),
        ] {
            // (one link from 0: symphonia's own length stands - None)
            match ogg_links(&media(name)) {
                None => {}
                Some(links) => panic!("{name} ({secs} s) taken for {links:?}"),
            }
        }
        // and a recording still is one
        let cap = ogg_links(&media("cap-2tone.opus")).unwrap();
        assert!(cap[0].base > 3000.0, "{cap:?}");
    }

    #[test]
    fn the_frames_of_an_mp3_without_a_xing_header() {
        for name in ["vbr-noxing-short.mp3", "vbr-noxing-long.mp3"] {
            let found = mp3_frames(&media(name)).unwrap();
            assert_eq!((found.rate, found.per_frame), (44_100, 1152));
            // PyAV decodes 178 560 samples of each: 155 frames
            assert_eq!(found.frames, 155, "{name}");
            assert_eq!(found.offsets.len(), 155usize.div_ceil(8));
        }
    }

    #[test]
    fn a_damaged_ogg_file_says_where_its_damage_begins() {
        for name in ["damaged.ogg", "damaged.opus"] {
            let len = std::fs::metadata(media(name)).unwrap().len();
            let at = ogg_damage(&media(name)).unwrap();
            assert!(
                at > len / 4 && at <= len * 41 / 100,
                "{name}: {at} of {len}"
            );
        }
        assert_eq!(ogg_damage(&media("chained.ogg")), None);
        assert_eq!(ogg_damage(&media("chirp.mp3")), None);
        let dir = crate::testdir::Scratch::new("window-range");
        let path = dir.join("f");
        std::fs::write(&path, b"0123456789").unwrap();
        let mut w = Window::range(&path, 2, 6).unwrap();
        let mut s = String::new();
        w.read_to_string(&mut s).unwrap();
        assert_eq!(s, "2345");
    }

    #[test]
    fn a_window_seeks_within_its_part() {
        let dir = crate::testdir::Scratch::new("window");
        let path = dir.join("f");
        std::fs::write(&path, b"0123456789").unwrap();
        let mut w = Window::open(&path, 4).unwrap();
        let mut s = String::new();
        w.read_to_string(&mut s).unwrap();
        assert_eq!(s, "456789");
        assert_eq!(w.seek(SeekFrom::End(-2)).unwrap(), 4);
        assert_eq!(w.seek(SeekFrom::Current(-1)).unwrap(), 3);
        let mut b = [0u8; 1];
        w.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"7");
        assert!(w.seek(SeekFrom::Current(-9)).is_err());
    }
}
