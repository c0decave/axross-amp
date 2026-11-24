//! The audio engine: a decoder thread (symphonia: MP3, FLAC, Vorbis, AAC,
//! ALAC, WAV / AIFF / CAF, MP4 / MKV containers; Opus by libopus) that turns
//! a file into stereo samples at the output's rate (downmixed, resampled,
//! levelled by ReplayGain, equalized) and a cpal output stream that plays
//! them, with volume, mute and the samples the visualizations read.
//!
//! * **Gapless**: the track to follow is named ahead ([`Command::Queue`]);
//!   at the end of a track it is opened and decoded on into the same buffer,
//!   so not a sample of silence comes between them ([`Event::Advanced`] when
//!   it is heard).
//! * **Streams** (http / https, internet radio) are read and decoded on a
//!   thread of their own ([`crate::net`]): a server that stalls never holds
//!   up a command; a radio that breaks off is connected again; its ICY
//!   titles come as [`Event::StreamTitle`].
//!
//! The UI talks to it with [`Command`]s and hears back [`Event`]s; the
//! position, volume and the recent samples are in [`Shared`] (atomics and
//! short locks - the audio callback never waits: a lock it cannot take at
//! once is a buffer of silence).

use crate::eq::Equalizer;
use crate::ring::{Ring, Tap};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use symphonia::core::audio::{Channels, GenericAudioBufferRef, Position};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::{MetadataOptions, StandardTag, StandardVisualKey};
use symphonia::core::units::{Time, TimeBase, Timestamp};

/// Seconds of sound equalized ahead of the output (an equalizer change is
/// heard this much later), and seconds decoded ahead of that - the margin a
/// slow disk, a network share or a big cover art has before it is heard
/// (the next track is opened ahead on a thread of its own besides).
/// The sample rates a file may have to be played (Hz): telephone's 8 kHz
/// and lower down to 1 kHz, up to 768 kHz.
const MIN_RATE: u32 = 1_000;
const MAX_RATE: u32 = 768_000;

const AHEAD_SECS: f32 = 0.35;
const STAGE_SECS: f32 = 3.0;
/// An Ogg link sought by decoding is read from this long before the place
/// at least (s): the decoder's run-up (Opus: 80 ms), a pre-skip.
const SEEK_LEAD: f64 = 1.0;

/// Tests: told when the output callback begins (true) and ends (false) -
/// on the audio thread itself (an allocation counter watches it).
#[doc(hidden)]
pub static CALLBACK_HOOK: OnceLock<fn(bool)> = OnceLock::new();

/// Tests: every file takes this long to open (a sleeping disk, NFS).
#[doc(hidden)]
pub static OPEN_DELAY_MS: AtomicU64 = AtomicU64::new(0);
/// Tests: no output device can be opened (as on a system without sound).
#[doc(hidden)]
pub static NO_OUTPUT: AtomicBool = AtomicBool::new(false);
/// A take-back of the ring keeps clear of the output by this many frames
/// (more than it reads at a call).
const CUT_MARGIN: u64 = 16_384;

/// How often the output is tried again while there is none.
const OUTPUT_RETRY: Duration = Duration::from_secs(5);
/// Stereo frames of the most recent output the visualizations and the
/// meters may read: 5.5 s at 48 kHz, 1.4 s at 192 kHz (2 MiB) - the time
/// the meters' thread may go without a turn on a busy machine and still
/// hear every frame (it was 1.4 s at 48 kHz, 0.34 s at 192 kHz). The
/// pictures read at most [`TAP_READ`] of it a frame, the meters 16384.
pub const TAP_FRAMES: usize = 1 << 18;
/// The most of the tap the pictures read at a time (the FFT, the scopes).
pub const TAP_READ: usize = 8192;
/// The output's buffer asked for (frames): small, so a pause, a seek or a
/// stop are heard at once (PulseAudio's default is about 2 s).
pub const OUTPUT_BUFFER: u32 = 1024;
/// A radio that breaks off is connected again this often (waiting 1, 2, 4 s).
pub const RECONNECTS: u32 = 3;
/// Decoded chunks of a stream waiting for the output (about 1.5 s), and
/// the seconds of a stream buffered before it plays.
const STREAM_CHUNKS: usize = 64;
const STREAM_PRIME_SECS: f32 = 0.75;

pub enum Command {
    /// Open `path` (a file or an http(s) URL) at `start_ms`, playing or not.
    Load {
        path: String,
        start_ms: u64,
        play: bool,
        generation: u64,
    },
    /// The track to play after the current one, without a gap (None: none).
    Queue(Option<Queued>),
    Play,
    Pause,
    Stop,
    Seek(f64),
    Equalizer {
        gains: [f32; 10],
        enabled: bool,
    },
    /// How ReplayGain levels the tracks (the current one too, from what is
    /// decoded next).
    Loudness(Loudness),
    /// Play on this output device (an id of [`output_devices`]; None: the
    /// default) - also while playing.
    Output(Option<String>),
    /// Fade a track into the next over this many seconds (0: none - gapless);
    /// never between two tracks of one album.
    Crossfade(f32),
    /// A track chosen while another plays fades in over it (1 s).
    FadeOnSkip(bool),
    Quit,
}

/// Whether the track `next` fades in over `current` (a crossfade of
/// `secs`): not between two tracks of one album (an album plays gapless).
pub fn crossfades(secs: f32, current: &TrackInfo, next: &TrackInfo) -> bool {
    crossfades_between(secs, ("", current), ("", next))
}

/// As [`crossfades`], with the tracks' paths. Tagged with albums (both):
/// the same album (and album artist) plays gapless. Untagged (both): two
/// files of one folder numbered one after the other (track numbers, or
/// the names' leading numbers: "01 …", "02 …") are an untagged album's
/// rips. Anything else - one tagged and one not, a flat folder of
/// unrelated files - fades.
pub fn crossfades_between(
    secs: f32,
    (current_path, current): (&str, &TrackInfo),
    (next_path, next): (&str, &TrackInfo),
) -> bool {
    if secs <= 0.0 {
        return false;
    }
    let album = match (current.album.is_empty(), next.album.is_empty()) {
        (false, false) => current.album == next.album && current.album_artist == next.album_artist,
        (true, true) => {
            let folder = |p: &str| std::path::Path::new(p).parent().map(|d| d.to_path_buf());
            let same_folder = !current_path.is_empty()
                && !crate::playlist::is_stream(current_path)
                && folder(current_path) == folder(next_path);
            let leading = |p: &str| -> Option<u64> {
                let name = std::path::Path::new(p)
                    .file_name()?
                    .to_string_lossy()
                    .into_owned();
                let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            };
            let numbered = match (current.track_number, next.track_number) {
                (Some(a), Some(b)) => b == a + 1,
                _ => matches!(
                    (leading(current_path), leading(next_path)),
                    (Some(a), Some(b)) if b == a + 1
                ),
            };
            same_folder && numbered
        }
        _ => false,
    };
    !album
}

/// The track fading out under the next one: its source, its resampler,
/// what of it is resampled and waiting, how far the fade is.
struct Fading {
    src: FileSource,
    resampler: Resampler,
    buf: Vec<f32>,
    total: usize,
    done: usize,
    ended: bool,
    /// How alike the two tracks are (correlation 0..1, smoothed): the fade's
    /// law follows it, so that the sum keeps its level.
    alike: f32,
}

/// The gains of a fade at `x` (0..1) - in, out - for two tracks correlated
/// `rho` (0..1): the one coming in rises as a sine; the one going out falls
/// so that the sum's power stays the same (a^2 + b^2 + 2 rho a b = 1): equal
/// power for unrelated tracks, equal amplitude for alike ones.
pub fn fade_gains(x: f32, rho: f32) -> (f32, f32) {
    if x >= 1.0 {
        return (1.0, 0.0);
    }
    let rho = rho.clamp(0.0, 1.0);
    let a = (x.max(0.0) * std::f32::consts::FRAC_PI_2).sin();
    let b = -rho * a + (1.0 - a * a * (1.0 - rho * rho)).max(0.0).sqrt();
    (a, b.max(0.0))
}

/// A track named to follow the current one.
#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    pub path: String,
    pub generation: u64,
}

/// ReplayGain: which gain of a track's tags is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GainMode {
    #[default]
    Off,
    Track,
    Album,
}

impl GainMode {
    pub fn key(self) -> &'static str {
        match self {
            GainMode::Off => "off",
            GainMode::Track => "track",
            GainMode::Album => "album",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "track" => GainMode::Track,
            "album" => GainMode::Album,
            _ => GainMode::Off,
        }
    }
}

/// ReplayGain's settings: the mode, a pre-amplification (dB) for the
/// tracks that have a gain, and the gain of those that have none (an
/// untagged track of today is mastered louder than ReplayGain's level).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    pub mode: GainMode,
    pub preamp_db: f32,
    pub untagged_db: f32,
}

impl Default for Loudness {
    fn default() -> Self {
        Loudness {
            mode: GainMode::Off,
            preamp_db: 0.0,
            untagged_db: DEFAULT_UNTAGGED_DB,
        }
    }
}

/// The gain of a track without ReplayGain tags while ReplayGain is on: about
/// how much louder than its 89 dB level a track mastered today is.
pub const DEFAULT_UNTAGGED_DB: f32 = -6.0;

impl Loudness {
    /// The factor for a track with these tags: the gain chosen (the other
    /// one where it is missing) plus the pre-amplification, held below the
    /// peak so it does not clip; 1 without a gain.
    pub fn factor(&self, info: &TrackInfo) -> f32 {
        let (gain, peak) = match self.mode {
            GainMode::Off => return 1.0,
            GainMode::Track => (
                info.track_gain.or(info.album_gain),
                info.track_peak.or(info.album_peak),
            ),
            GainMode::Album => (
                info.album_gain.or(info.track_gain),
                info.album_peak.or(info.track_peak),
            ),
        };
        let Some(gain) = gain else {
            return 10f32.powf(self.untagged_db.clamp(-15.0, 15.0) / 20.0);
        };
        let mut factor = 10f32.powf((gain + self.preamp_db.clamp(-15.0, 15.0)) / 20.0);
        if let Some(peak) = peak.filter(|p| *p > 0.0) {
            factor = factor.min(1.0 / peak);
        }
        factor.clamp(0.01, 8.0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrackInfo {
    pub duration: Option<f64>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub date: String,
    pub track_number: Option<u64>,
    pub disc_number: Option<u64>,
    /// Every text tag as the file has it: (key, value) - the track's details.
    pub tags: Vec<(String, String)>,
    /// The cover as the file has it (read where it is played: made into
    /// `art` there and let go - a picture of 16 MB never reaches the window).
    pub cover: Option<Vec<u8>>,
    /// The cover made small (at most 256 px), on the reading thread.
    pub art: Option<Arc<Art>>,
    pub seekable: bool,
    pub rate: u32,
    pub channels: usize,
    pub bits: Option<u32>,
    /// The mean bit rate (kbit/s): the file's size over its length.
    pub bitrate: Option<u32>,
    pub codec: String,
    /// ReplayGain from the tags (dB, and the peak as a factor of full scale).
    pub track_gain: Option<f32>,
    pub album_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_peak: Option<f32>,
    /// A radio's name and genre (its ICY headers).
    pub station: String,
    pub station_genre: String,
}

pub enum Event {
    Loaded {
        generation: u64,
        info: TrackInfo,
    },
    /// The queued track is heard now, right after the one before.
    Advanced {
        generation: u64,
        info: TrackInfo,
    },
    Ended {
        generation: u64,
    },
    /// The track is this long, not as long as its file said (it was cut
    /// short): known when its end is read.
    Length {
        generation: u64,
        secs: f64,
    },
    Failed {
        generation: u64,
        message: String,
    },
    /// What a radio says it plays now ("" when it says nothing).
    StreamTitle {
        generation: u64,
        title: String,
    },
    /// Something to tell while it goes on (a stream connecting again); ""
    /// takes it back.
    Notice {
        generation: u64,
        message: String,
    },
    /// The output device in use (or why there is no sound).
    Output(String),
}

/// A cover made small for showing: its pixels (RGBA) and as a PNG (for the
/// desktop's MPRIS).
#[derive(Debug, PartialEq)]
pub struct Art {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub png: Vec<u8>,
    /// The cover large (up to 1024 px), where it is larger than the small
    /// one: for the cover as the picture, the card on a large or HiDPI
    /// window. Width, height, RGBA.
    pub large: Option<(u32, u32, Vec<u8>)>,
}

/// A cover's picture decoded, within limits: at most 8192 px a side and 192
/// MiB to decode it into (an 11000 x 11000 PNG took 552 MB: review 11) - a
/// larger one is no cover.
fn decode_cover(bytes: &[u8]) -> image::ImageResult<image::DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(192 << 20);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits);
    reader.decode()
}

/// The cover of `info` made small, the large one let go.
fn make_art(info: &mut TrackInfo) {
    let Some(bytes) = info.cover.take() else {
        return;
    };
    let Ok(image) = decode_cover(&bytes) else {
        return;
    };
    let small = image.thumbnail(256, 256).to_rgba8();
    let large = (image.width().max(image.height()) > 256).then(|| {
        let large = image.thumbnail(1024, 1024).to_rgba8();
        (large.width(), large.height(), large.into_raw())
    });
    let mut png = Vec::new();
    let encoded = image::DynamicImage::ImageRgba8(small.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    info.art = Some(Arc::new(Art {
        width: small.width(),
        height: small.height(),
        rgba: small.into_raw(),
        png: if encoded.is_ok() { png } else { Vec::new() },
        large,
    }));
}

/// What the UI reads while the engine runs. Nothing in it is locked: the
/// output callback must never wait or allocate (it reads the [`Ring`]).
pub struct Shared {
    ring: Ring,
    /// Where (in the ring's frames read) the current track's position began,
    /// and that position (ms).
    origin: AtomicU64,
    base_ms: AtomicU64,
    /// Where (in the ring's frames) the queued track begins (u64::MAX: none decoded).
    boundary: AtomicU64,
    pub playing: AtomicBool,
    volume_bits: AtomicU32,
    pub muted: AtomicBool,
    pub device_rate: AtomicU32,
    /// The latest frames played, for the visualizations.
    pub tap: Tap,
    /// The output reported that it is gone (the decoder thread opens it anew).
    lost: AtomicBool,
    /// The sound in the ring ends there (a track's end, nothing after it):
    /// running dry is no stall to fade over.
    ending: AtomicBool,
    /// Frames read from the ring that are not heard yet: the output's
    /// latency (what the device says) and the buffer it was handed.
    latency: AtomicU64,
    /// Where (in the ring's frames) the next track's sound begins, and -
    /// once the output plays it - where that is in the tap and how many
    /// tracks began so far (the meters start counting anew there).
    track_mark: AtomicU64,
    track_tap: AtomicU64,
    tracks: AtomicU64,
}

/// What goes to the device: the sample as it is within full scale (bit for
/// bit - nothing bends a hot master at unity gain); beyond it, full scale.
/// A track made louder is kept below the ceiling before it gets here
/// ([`crate::limiter`]), and so are the equalizer's boosts (the output's
/// limiter). A NaN is silence.
pub fn clip(x: f32) -> f32 {
    if x.is_nan() { 0.0 } else { x.clamp(-1.0, 1.0) }
}

/// The fades that keep pause, seek, stop and a new track from clicking.
pub const FADE_SECS: f32 = 0.008;

/// What the output callback keeps between its calls.
pub struct RenderState {
    /// The volume reached (ramped towards the one set: no click).
    gain: f32,
    /// 0..1: faded out (paused, before a flush) or in.
    fade: f32,
    /// The ring's flushes honoured.
    flushes: u64,
    /// Frames of silence handed over since the last sound.
    silent: u64,
}

impl Default for RenderState {
    fn default() -> Self {
        RenderState {
            gain: 0.0,
            fade: 0.0,
            flushes: 0,
            silent: 0,
        }
    }
}

/// The ring's size: about 21 s at 48 kHz (decoded ahead: [`AHEAD_SECS`]).
const RING_FRAMES: usize = 1 << 20;

impl Shared {
    fn new() -> Self {
        Shared {
            ring: Ring::new(RING_FRAMES),
            origin: AtomicU64::new(0),
            base_ms: AtomicU64::new(0),
            boundary: AtomicU64::new(u64::MAX),
            playing: AtomicBool::new(false),
            volume_bits: AtomicU32::new(0.8f32.to_bits()),
            muted: AtomicBool::new(false),
            device_rate: AtomicU32::new(48_000),
            tap: Tap::new(TAP_FRAMES),
            lost: AtomicBool::new(false),
            ending: AtomicBool::new(false),
            latency: AtomicU64::new(0),
            track_mark: AtomicU64::new(u64::MAX),
            track_tap: AtomicU64::new(0),
            tracks: AtomicU64::new(0),
        }
    }

    /// The output's latency, in frames read but not yet heard (the output
    /// callback says it; tests too).
    pub fn set_latency(&self, frames: u64) {
        self.latency.store(frames, Ordering::Relaxed);
    }

    /// The frames heard so far (read from the ring, less those still on
    /// their way to the speakers).
    fn heard(&self) -> u64 {
        self.ring
            .read()
            .saturating_sub(self.latency.load(Ordering::Relaxed))
    }

    /// The latest frames heard (the visualizations: in step with the sound,
    /// not ahead of it by the output's latency).
    pub fn tap_into(&self, out: &mut Vec<f32>) -> u64 {
        let delay = self
            .latency
            .load(Ordering::Relaxed)
            .min((TAP_FRAMES - TAP_READ) as u64);
        self.tap.read_delayed(out, delay, TAP_READ as u64)
    }

    /// The heard frames from `from` on, as many as there are (at most
    /// 16384), for a reader that keeps its own place (the meters): where
    /// they begin and end.
    pub fn tap_from(&self, from: u64, out: &mut Vec<f32>) -> (u64, u64) {
        let delay = self
            .latency
            .load(Ordering::Relaxed)
            .min((TAP_FRAMES - TAP_READ) as u64);
        self.tap.read_from(from, delay, 16_384, out)
    }

    /// As [`Shared::tap_from`], up to (not including) the tap's frame `end`.
    pub fn tap_from_to(&self, from: u64, end: u64, out: &mut Vec<f32>) -> (u64, u64) {
        let written = self.tap.written();
        let max = end.saturating_sub(from).min(16_384);
        self.tap
            .read_from(from, written.saturating_sub(end), max, out)
    }

    /// As when the output reports that its device is gone (tests).
    #[doc(hidden)]
    pub fn simulate_output_loss(&self) {
        self.lost.store(true, Ordering::Relaxed);
    }

    /// The position of the sound heard (seconds).
    pub fn position(&self) -> f64 {
        let rate = self.device_rate.load(Ordering::Relaxed).max(1) as f64;
        let frames = self
            .heard()
            .saturating_sub(self.origin.load(Ordering::Relaxed));
        self.base_ms.load(Ordering::Relaxed) as f64 / 1000.0 + frames as f64 / rate
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    /// Drop what is decoded and not yet heard (the output fades it out);
    /// the position starts at `ms`.
    fn restart_at(&self, ms: u64) -> u64 {
        let at = self.ring.flush();
        self.origin.store(at, Ordering::Relaxed);
        self.base_ms.store(ms, Ordering::Relaxed);
        self.boundary.store(u64::MAX, Ordering::Relaxed);
        at
    }

    /// A new track's sound begins at `at` (in the ring's frames).
    fn mark_track(&self, at: u64) {
        self.track_mark.store(at, Ordering::Release);
    }

    /// Where in the tap the latest track began (the tap's count of frames)
    /// and how many have begun: the meters start anew at it.
    pub fn track_start(&self) -> (u64, u64) {
        let tracks = self.tracks.load(Ordering::Acquire);
        (self.track_tap.load(Ordering::Acquire), tracks)
    }

    fn buffered_frames(&self) -> usize {
        self.ring.buffered() as usize
    }

    /// Whether `frames` of sound are decoded and waiting, or all there is
    /// (the sound ends in the ring): an output of the tests' own takes its
    /// next frames once they are, as a device's buffer covers a busy
    /// decoder (tests).
    #[doc(hidden)]
    pub fn ready(&self, frames: u64) -> bool {
        self.ring.buffered() >= frames || self.ending.load(Ordering::Relaxed)
    }

    /// Where the sound decoded next will be heard (in the ring's frames).
    fn end_of_buffer(&self) -> u64 {
        self.ring.written()
    }

    /// Samples into the ring: how many frames went in.
    fn push(&self, samples: &[f32]) -> usize {
        self.ring.push(samples)
    }

    /// [`Shared::render`] for an output whose sound is heard `latency`
    /// frames after it is handed over (its buffer included): the position
    /// and the pictures follow what is heard - also while paused, as the
    /// last of the sound still plays out of the device.
    pub fn render_at(
        &self,
        out: &mut [f32],
        channels: usize,
        state: &mut RenderState,
        latency: u64,
    ) {
        self.render(out, channels, state);
        // (silence handed over after the last sound carries none of it)
        self.set_latency(latency.saturating_sub(state.silent));
    }

    /// Fill `out` (interleaved, `channels` a frame) with what is to be heard:
    /// the stereo sound at the volume (ramped over the buffer: no click),
    /// both sides mixed for one channel, nothing on the channels past two.
    /// A pause, a seek, a new track fade out over [`FADE_SECS`] and fade in
    /// again. No lock, no allocation: what the output's callback does; tests
    /// call it for an output of their own.
    pub fn render(&self, out: &mut [f32], channels: usize, state: &mut RenderState) {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        let playing = self.playing.load(Ordering::Relaxed);
        let target = if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            self.volume()
        };
        let step = (target - state.gain) / frames.max(1) as f32;
        let fade_step =
            1.0 / (self.device_rate.load(Ordering::Relaxed).max(1) as f32 * FADE_SECS).max(1.0);
        let ending = self.ending.load(Ordering::Relaxed);
        let mark = self.track_mark.load(Ordering::Acquire);
        let mut reading = self.ring.begin(state.flushes);
        for slot in out.chunks_exact_mut(channels) {
            state.gain += step;
            // paused, or a flush waiting: what is left fades out first; so
            // when the ring is about to run dry (a stall, not a track's end):
            // a fade, not a cut in the middle of a wave
            let dry = playing
                && !ending
                && (self.ring.left(&reading) as f32) < state.fade / fade_step + 1.0;
            let hold = !playing || reading.flush.is_some() || dry;
            state.fade = if hold {
                (state.fade - fade_step).max(0.0)
            } else {
                (state.fade + fade_step).min(1.0)
            };
            if state.fade <= 0.0 && reading.flush.is_some() {
                state.flushes = self.ring.skip(&mut reading);
            }
            // (faded out and held: nothing is taken - paused, or waiting for
            // the sound to come)
            let place = self.ring.place(&reading);
            let frame = if state.fade > 0.0 || (playing && !hold) {
                self.ring.next(&mut reading)
            } else {
                None
            };
            if frame.is_some() && place == mark {
                // a track's first sound: where it lies in the tap
                self.track_tap.store(self.tap.written(), Ordering::Release);
                self.tracks.fetch_add(1, Ordering::AcqRel);
            }
            let (l, r) = match frame {
                Some((l, r)) => {
                    state.silent = 0;
                    self.tap.put(l, r);
                    let g = state.gain * state.fade;
                    (l * g, r * g)
                }
                None => {
                    // (nothing there: what comes next fades in)
                    if !hold {
                        state.fade = 0.0;
                    }
                    state.silent += 1;
                    (0.0, 0.0)
                }
            };
            if channels == 1 {
                slot[0] = clip((l + r) * 0.5);
                continue;
            }
            for (c, sample) in slot.iter_mut().enumerate() {
                *sample = match c {
                    0 => clip(l),
                    1 => clip(r),
                    _ => 0.0,
                };
            }
        }
        self.ring.end(reading);
        // (a remainder that is no whole frame: silence)
        let whole = frames * channels;
        out[whole..].iter_mut().for_each(|s| *s = 0.0);
        state.gain = target;
    }
}

pub struct Engine {
    pub tx: Sender<Command>,
    pub events: Receiver<Event>,
    pub shared: Arc<Shared>,
}

impl Engine {
    /// Start the engine; `wake` is called when an event is waiting (the UI
    /// repaints).
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Engine {
        Self::start_with(wake, false)
    }

    /// The engine without a sound device: nothing is heard until
    /// [`Shared::render`] is called (tests, at 48 kHz).
    pub fn start_headless(wake: impl Fn() + Send + Sync + 'static) -> Engine {
        Self::start_with(wake, true)
    }

    fn start_with(wake: impl Fn() + Send + Sync + 'static, headless: bool) -> Engine {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (etx, events) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared::new());
        let thread_shared = shared.clone();
        thread::Builder::new()
            .name("amp-decoder".into())
            .spawn(move || run(rx, etx, thread_shared, Arc::new(wake), headless))
            .expect("the decoder thread starts");
        Engine { tx, events, shared }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Quit);
    }
}

// -- output ------------------------------------------------------------------------------------

/// The hosts to try: PulseAudio / PipeWire first on Linux, then the
/// system's default (ALSA, CoreAudio, WASAPI).
fn hosts() -> Vec<cpal::Host> {
    #[cfg(target_os = "linux")]
    let first = cpal::host_from_id(cpal::HostId::PulseAudio).ok();
    #[cfg(not(target_os = "linux"))]
    let first: Option<cpal::Host> = None;
    first.into_iter().chain([cpal::default_host()]).collect()
}

/// The output devices to choose from: (id, name).
pub fn output_devices() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for host in hosts() {
        let Ok(devices) = host.output_devices() else {
            continue;
        };
        for device in devices {
            let (Ok(id), Ok(description)) = (device.id(), device.description()) else {
                continue;
            };
            let id = id.to_string();
            if !out.iter().any(|(i, _)| *i == id) {
                out.push((id, description.name().to_owned()));
            }
        }
        if !out.is_empty() {
            break; // (the first host that has devices: the others list the same ones)
        }
    }
    out
}

/// The output stream on `device` (an id of [`output_devices`]), else the
/// default: (stream, rate, what it is).
fn open_output(
    shared: Arc<Shared>,
    device: Option<&str>,
) -> Result<(cpal::Stream, u32, String), String> {
    let mut last = String::from("no audio output");
    for host in hosts() {
        let chosen = device
            .and_then(|id| id.parse::<cpal::DeviceId>().ok())
            .and_then(|id| host.device_by_id(&id));
        if device.is_some() && chosen.is_none() {
            continue;
        }
        let Some(device) = chosen.or_else(|| host.default_output_device()) else {
            continue;
        };
        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let name = match device.description() {
            Ok(d) => format!("{} ({:?})", d.name(), host.id()),
            Err(_) => format!("{:?}", host.id()),
        };
        let rate = config.sample_rate();
        let format = config.sample_format();
        // a small buffer where the device takes one (else its own)
        let small = match config.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => {
                Some(cpal::BufferSize::Fixed(OUTPUT_BUFFER.clamp(*min, *max)))
            }
            cpal::SupportedBufferSize::Unknown => None,
        };
        let sizes = small.into_iter().chain([cpal::BufferSize::Default]);
        let mut result = Err(String::from("no buffer size"));
        for size in sizes {
            let mut stream_config: cpal::StreamConfig = config.into();
            stream_config.buffer_size = size;
            let s = shared.clone();
            result = match format {
                cpal::SampleFormat::F32 => build::<f32>(&device, stream_config, s),
                cpal::SampleFormat::I16 => build::<i16>(&device, stream_config, s),
                cpal::SampleFormat::U16 => build::<u16>(&device, stream_config, s),
                cpal::SampleFormat::I32 => build::<i32>(&device, stream_config, s),
                other => Err(format!("sample format {other:?} not supported")),
            };
            if result.is_ok() {
                break;
            }
        }
        match result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    last = e.to_string();
                    continue;
                }
                return Ok((stream, rate, name));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    let mut state = RenderState::default();
    // (made once: the callback renders in pieces of this size, it never allocates)
    let piece = (8192 / channels).max(1) * channels;
    let mut mixed = vec![0.0f32; piece];
    let lost = shared.clone();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let hook = CALLBACK_HOOK.get();
                if let Some(h) = hook {
                    h(true);
                }
                // what is written now is heard this much later, the buffer's
                // length on top: the position and the pictures wait for it
                let ts = info.timestamp();
                let ahead = ts.playback.duration_since(ts.callback).as_secs_f64();
                let rate = shared.device_rate.load(Ordering::Relaxed) as f64;
                // (ten seconds at most: a timestamp gone wild)
                let latency =
                    ((ahead * rate) as u64).min(rate as u64 * 10) + (data.len() / channels) as u64;
                for chunk in data.chunks_mut(piece) {
                    let mixed = &mut mixed[..chunk.len()];
                    shared.render_at(mixed, channels, &mut state, latency);
                    for (out, v) in chunk.iter_mut().zip(mixed.iter()) {
                        *out = T::from_sample(*v);
                    }
                }
                if let Some(h) = hook {
                    h(false);
                }
            },
            move |err| {
                eprintln!("axross-amp: output: {err}");
                // a device that went away, a stream to be made anew: the
                // decoder thread opens the output again
                if matches!(
                    err.kind(),
                    cpal::ErrorKind::DeviceNotAvailable
                        | cpal::ErrorKind::StreamInvalidated
                        | cpal::ErrorKind::HostUnavailable
                ) {
                    lost.lost.store(true, Ordering::Relaxed);
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

// -- resampling --------------------------------------------------------------------------------

/// The windowed-sinc kernel: `taps` on each side of the point, `PHASES`
/// steps between two input frames, cut off at `cutoff` (of the input's
/// Nyquist frequency) - a low-pass, so going down in rate does not alias.
struct Kernel {
    taps: usize,
    table: Vec<f32>,
}

const PHASES: usize = 256;

impl Kernel {
    fn new(cutoff: f64, taps: usize) -> Kernel {
        // Kaiser window, beta 9: about 90 dB down outside the passband
        let beta = 9.0f64;
        let bessel = |x: f64| {
            let (mut sum, mut term, mut k) = (1.0f64, 1.0f64, 1.0f64);
            while term > 1e-12 * sum {
                term *= (x / (2.0 * k)).powi(2);
                sum += term;
                k += 1.0;
            }
            sum
        };
        let norm = bessel(beta);
        let width = taps as f64;
        let mut table = Vec::with_capacity((PHASES + 1) * 2 * taps);
        for phase in 0..=PHASES {
            let frac = phase as f64 / PHASES as f64;
            for k in 0..2 * taps {
                // the distance of input frame k (taps-1 before the point .. taps after)
                let t = k as f64 - (taps as f64 - 1.0) - frac;
                let x = t * cutoff;
                let sinc = if x.abs() < 1e-9 {
                    1.0
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                let r = t / width;
                let window = if r.abs() >= 1.0 {
                    0.0
                } else {
                    bessel(beta * (1.0 - r * r).sqrt()) / norm
                };
                table.push((cutoff * sinc * window) as f32);
            }
        }
        Kernel { taps, table }
    }

    /// The kernel for a point `frac` (0..1) past an input frame.
    fn weights(&self, frac: f64, out: &mut [f32]) {
        let p = frac * PHASES as f64;
        let i = (p as usize).min(PHASES - 1);
        let f = (p - i as f64) as f32;
        let n = 2 * self.taps;
        let (a, b) = (
            &self.table[i * n..(i + 1) * n],
            &self.table[(i + 1) * n..(i + 2) * n],
        );
        for ((w, x), y) in out.iter_mut().zip(a).zip(b) {
            *w = x + (y - x) * f;
        }
    }
}

/// Kernels by their (cutoff, taps), shared by every resampler.
fn kernel(from: u32, to: u32) -> Arc<Kernel> {
    type Kernels = Mutex<Vec<((u32, u32), Arc<Kernel>)>>;
    static KERNELS: OnceLock<Kernels> = OnceLock::new();
    let ratio = (to as f64 / from as f64).min(1.0);
    // the passband ends at 92 % of the lower Nyquist frequency; going down,
    // the kernel is longer by as much (the same steepness at the output's rate)
    let cutoff = ratio * 0.92;
    let taps = ((24.0 / ratio).ceil() as usize).clamp(24, 256);
    let key = ((cutoff * 1e6) as u32, taps as u32);
    let kernels = KERNELS.get_or_init(Default::default);
    let mut kernels = kernels.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, k)) = kernels.iter().find(|(k, _)| *k == key) {
        return k.clone();
    }
    let k = Arc::new(Kernel::new(cutoff, taps));
    kernels.push((key, k.clone()));
    k
}

/// Stereo resampling by a windowed sinc (band-limited: no aliasing going
/// down, no imaging going up); a bypass at the same rate.
#[derive(Clone)]
struct Resampler {
    from: u32,
    to: u32,
    step: f64,
    /// The input position of the next output frame, in `hist`.
    pos: f64,
    hist: Vec<[f32; 2]>,
    kernel: Option<Arc<Kernel>>,
    weights: Vec<f32>,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        let (from, to) = (from.max(1), to.max(1));
        let kernel = (from != to).then(|| kernel(from, to));
        let taps = kernel.as_ref().map(|k| k.taps).unwrap_or(0);
        Resampler {
            from,
            to,
            step: from as f64 / to as f64,
            // (taps-1 frames of silence before the first: no delay, no lost start)
            pos: taps.saturating_sub(1) as f64,
            hist: vec![[0.0; 2]; taps.saturating_sub(1)],
            kernel,
            weights: vec![0.0; 2 * taps],
        }
    }

    /// The output frame at `self.pos` (enough frames after it in `hist`).
    fn frame(&mut self, kernel: &Kernel) -> [f32; 2] {
        let taps = kernel.taps;
        let i = self.pos.floor() as usize;
        kernel.weights(self.pos - i as f64, &mut self.weights);
        let first = i + 1 - taps;
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for (w, f) in self.weights.iter().zip(&self.hist[first..first + 2 * taps]) {
            l += w * f[0];
            r += w * f[1];
        }
        [l, r]
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let Some(kernel) = self.kernel.clone() else {
            out.extend_from_slice(input);
            return;
        };
        let taps = kernel.taps;
        self.hist
            .extend(input.as_chunks::<2>().0.iter().map(|f| [f[0], f[1]]));
        // an output frame needs `taps` input frames after its point
        while (self.pos.floor() as usize) + taps < self.hist.len() {
            out.extend(self.frame(&kernel));
            self.pos += self.step;
        }
        let drop = (self.pos.floor() as usize).saturating_sub(taps - 1);
        let drop = drop.min(self.hist.len());
        self.hist.drain(..drop);
        self.pos -= drop as f64;
    }

    /// The frames still held back for the kernel (the end of the sound):
    /// those up to the last input frame, silence after it.
    fn flush(&mut self, out: &mut Vec<f32>) {
        let Some(kernel) = self.kernel.clone() else {
            return;
        };
        let end = self.hist.len() as f64;
        self.hist
            .extend(std::iter::repeat_n([0.0f32; 2], kernel.taps + 1));
        while self.pos < end {
            out.extend(self.frame(&kernel));
            self.pos += self.step;
        }
        // (used up: anything after this starts from silence)
        *self = Resampler::new(self.from, self.to);
    }
}

// -- channels ----------------------------------------------------------------------------------

/// Each channel's share of left and right: a downmix as ITU-R BS.775 has
/// it (centre and surrounds at -3 dB, the LFE left out), scaled so the
/// loudest side cannot clip; mono on both sides at full level.
pub fn downmix_matrix(channels: &Channels) -> Vec<(f32, f32)> {
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let count = channels.count().max(1);
    if count == 1 {
        return vec![(1.0, 1.0)];
    }
    let positions: Vec<Position> = match channels {
        Channels::Positioned(p) => p.iter().collect(),
        // no positions: the order WAVE files use (WAVEFORMATEXTENSIBLE's)
        _ => {
            let order = [
                Position::FRONT_LEFT,
                Position::FRONT_RIGHT,
                Position::FRONT_CENTER,
                Position::LFE1,
                Position::REAR_LEFT,
                Position::REAR_RIGHT,
                Position::SIDE_LEFT,
                Position::SIDE_RIGHT,
            ];
            let layout: &[Position] = match count {
                2 => &order[..2],
                3 => &order[..3],
                4 => &[order[0], order[1], order[4], order[5]],
                5 => &[order[0], order[1], order[2], order[4], order[5]],
                6 => &order[..6],
                7 => &[
                    order[0],
                    order[1],
                    order[2],
                    order[3],
                    Position::REAR_CENTER,
                    order[6],
                    order[7],
                ],
                _ => &order,
            };
            (0..count)
                .map(|i| layout.get(i).copied().unwrap_or(Position::empty()))
                .collect()
        }
    };
    if positions.len() == 1 {
        return vec![(1.0, 1.0)];
    }
    let mut matrix: Vec<(f32, f32)> = positions
        .iter()
        .map(|p| {
            let p = *p;
            if p == Position::FRONT_LEFT {
                (1.0, 0.0)
            } else if p == Position::FRONT_RIGHT {
                (0.0, 1.0)
            } else if p == Position::FRONT_CENTER {
                (H, H)
            } else if p == Position::LFE1 || p == Position::LFE2 {
                (0.0, 0.0)
            } else if [
                Position::REAR_LEFT,
                Position::SIDE_LEFT,
                Position::FRONT_LEFT_CENTER,
                Position::FRONT_LEFT_WIDE,
                Position::TOP_FRONT_LEFT,
                Position::TOP_REAR_LEFT,
                Position::TOP_SIDE_LEFT,
            ]
            .contains(&p)
            {
                (H, 0.0)
            } else if [
                Position::REAR_RIGHT,
                Position::SIDE_RIGHT,
                Position::FRONT_RIGHT_CENTER,
                Position::FRONT_RIGHT_WIDE,
                Position::TOP_FRONT_RIGHT,
                Position::TOP_REAR_RIGHT,
                Position::TOP_SIDE_RIGHT,
            ]
            .contains(&p)
            {
                (0.0, H)
            } else {
                // centres of the back and the top, and what has no position
                (0.5, 0.5)
            }
        })
        .collect();
    let (left, right) = matrix
        .iter()
        .fold((0.0f32, 0.0f32), |(l, r), (a, b)| (l + a, r + b));
    let loudest = left.max(right);
    if loudest > 1.0 {
        for (a, b) in &mut matrix {
            *a /= loudest;
            *b /= loudest;
        }
    }
    matrix
}

/// Interleaved frames of `matrix.len()` channels as stereo.
fn downmix(input: &[f32], matrix: &[(f32, f32)], stereo: &mut Vec<f32>) {
    stereo.clear();
    let n = matrix.len().max(1);
    if n == 2 && matrix == [(1.0, 0.0), (0.0, 1.0)] {
        stereo.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(n) {
        let (mut l, mut r) = (0.0, 0.0);
        for (s, (a, b)) in frame.iter().zip(matrix) {
            l += s * a;
            r += s * b;
        }
        stereo.push(l);
        stereo.push(r);
    }
}

// -- decoding ----------------------------------------------------------------------------------

struct Decoding {
    /// Of an Ogg file cut in the middle of a page: where that page begins,
    /// and what is whole of it as a page (`seektable::ogg_cut_page`).
    ogg_tail: Option<(u64, Arc<Vec<u8>>)>,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    resampler: Resampler,
    /// Resampled to the output's rate here (a stream), or handed over at
    /// the source's rate (a file: its reader resamples, gapless across tracks).
    resample: bool,
    seekable: bool,
    scratch: Vec<f32>,
    stereo: Vec<f32>,
    /// The channel layout `matrix` is for.
    layout: Option<Channels>,
    matrix: Vec<(f32, f32)>,
    /// The source's rate, the frames still to drop before the sound (encoder
    /// delay, or the part of a packet before a seek's target), the frames
    /// still to play (an edit list's length: the encoder's padding is not)
    /// and where the sound begins in the media (frames).
    rate: u32,
    skip: u64,
    left: Option<u64>,
    delay: u64,
    length: Option<u64>,
    /// ReplayGain's factor for this track.
    gain: f32,
    /// Holds the peaks below the ceiling once the factor is above 1.
    limiter: crate::limiter::Limiter,
    /// At its end (and the resampler's tail given out).
    eof: bool,
    /// The file (None: a stream), to be opened again at a chained Ogg
    /// file's link or an MP3's frame ([`crate::seektable`]); the links and
    /// the one read now; the frames of an MP3 without a Xing header.
    file: Option<String>,
    links: Option<Vec<crate::seektable::Link>>,
    link: usize,
    mp3: Option<crate::seektable::Mp3Frames>,
    options: AudioDecoderOptions,
    /// The rate of the link read now (another than `rate` in a chained
    /// file: resampled to `rate` by `link_resampler`, into `linked`).
    link_rate: u32,
    link_resampler: Option<Resampler>,
    linked: Vec<f32>,
    /// Sound dropped until this timestamp of the reader (opened again at a
    /// frame before a seek's target: by the packets' own timestamps).
    until_ts: Option<i64>,
    /// A new link's title (an Ogg radio's next song), to be said.
    link_title: Option<String>,
    /// Sound came (a file's damage after it ends the track), and the
    /// errors in a row since.
    sounded: bool,
    damaged: u32,
    /// An Opus track's pre-skip (frames), and whether its decoder is still
    /// to drop it (libopus' adapter drops it from the first packet it
    /// decodes, not again after a seek - where the packets' timestamps do
    /// not count it).
    preskip: i64,
    preskip_armed: bool,
    /// Packets decoded and dropped on the way to a place sought.
    dropped: u32,
}

/// The pre-skip of an Opus track (its OpusHead), else 0.
fn opus_preskip(params: &symphonia::core::codecs::audio::AudioCodecParameters) -> i64 {
    if params.codec != symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS {
        return 0;
    }
    params
        .extra_data
        .as_deref()
        .filter(|d| d.len() >= 12 && d.starts_with(b"OpusHead"))
        .map(|d| u16::from_le_bytes([d[10], d[11]]) as i64)
        .unwrap_or(0)
}

fn hint_for(path: &str, content_type: Option<&str>) -> Hint {
    let mut hint = Hint::new();
    let lower = path.to_ascii_lowercase();
    let ext = Path::new(lower.split(['?', '#']).next().unwrap_or(""))
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_owned);
    if let Some(ext) = ext {
        hint.with_extension(&ext);
    }
    if let Some(ct) = content_type {
        let ct = ct
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        hint.mime_type(&ct);
        let ext = match ct.as_str() {
            "audio/mpeg" | "audio/mp3" => Some("mp3"),
            "audio/aac" | "audio/aacp" | "audio/x-aac" => Some("aac"),
            "audio/ogg" | "application/ogg" | "audio/vorbis" | "audio/opus" => Some("ogg"),
            "audio/flac" | "audio/x-flac" => Some("flac"),
            "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
            _ => None,
        };
        if let Some(ext) = ext {
            hint.with_extension(ext);
        }
    }
    hint
}

/// A ReplayGain value: "-6.54 dB" (a gain) or "0.98" (a peak).
fn gain_value(text: &str) -> Option<f32> {
    let number: String = text
        .trim()
        .trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    number.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn tag_text(tags: &[symphonia::core::meta::Tag], info: &mut TrackInfo) {
    for tag in tags {
        let value = tag.raw.value.to_string();
        if !matches!(tag.raw.value, symphonia::core::meta::RawValue::Binary(_))
            && value.len() <= 4096
            && info.tags.len() < 200
            && !info
                .tags
                .iter()
                .any(|(k, v)| *k == tag.raw.key && *v == value)
        {
            info.tags
                .push((crate::net::clean(&tag.raw.key), crate::net::clean(&value)));
        }
        let set = |slot: &mut String, v: &str| {
            if slot.is_empty() {
                *slot = crate::net::clean(v);
            }
        };
        match &tag.std {
            Some(StandardTag::TrackTitle(v)) => set(&mut info.title, v),
            Some(StandardTag::Artist(v)) => set(&mut info.artist, v),
            Some(StandardTag::Album(v)) => set(&mut info.album, v),
            Some(StandardTag::AlbumArtist(v)) => set(&mut info.album_artist, v),
            Some(StandardTag::Genre(v)) => set(&mut info.genre, v),
            Some(StandardTag::RecordingDate(v) | StandardTag::ReleaseDate(v)) => {
                set(&mut info.date, v)
            }
            Some(StandardTag::TrackNumber(n)) if info.track_number.is_none() => {
                info.track_number = Some(*n)
            }
            Some(StandardTag::DiscNumber(n)) if info.disc_number.is_none() => {
                info.disc_number = Some(*n)
            }
            Some(StandardTag::ReplayGainTrackGain(v)) => {
                info.track_gain = info.track_gain.or(gain_value(v))
            }
            Some(StandardTag::ReplayGainAlbumGain(v)) => {
                info.album_gain = info.album_gain.or(gain_value(v))
            }
            Some(StandardTag::ReplayGainTrackPeak(v)) => {
                info.track_peak = info.track_peak.or(gain_value(v))
            }
            Some(StandardTag::ReplayGainAlbumPeak(v)) => {
                info.album_peak = info.album_peak.or(gain_value(v))
            }
            _ => {
                // what symphonia does not name: Opus's R128 gains (1/256 dB,
                // against -23 LUFS: ReplayGain's 89 dB is 5 dB louder)
                let key = tag.raw.key.to_ascii_uppercase();
                let r128 = || gain_value(&value).map(|q| q / 256.0 + 5.0);
                match key.as_str() {
                    "R128_TRACK_GAIN" => info.track_gain = info.track_gain.or(r128()),
                    "R128_ALBUM_GAIN" => info.album_gain = info.album_gain.or(r128()),
                    "REPLAYGAIN_TRACK_GAIN" => {
                        info.track_gain = info.track_gain.or(gain_value(&value))
                    }
                    "REPLAYGAIN_ALBUM_GAIN" => {
                        info.album_gain = info.album_gain.or(gain_value(&value))
                    }
                    _ => {}
                }
            }
        }
    }
    if info.artist.is_empty() && !info.album_artist.is_empty() {
        info.artist = info.album_artist.clone();
    }
}

/// A codec as people call it ("pcm_s16le": "PCM 16-bit").
pub fn codec_name(short: &str) -> String {
    let lower = short.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("pcm_") {
        // pcm_s16le, pcm_f32be, pcm_u8, pcm_alaw ...
        let bits: String = rest.chars().filter(char::is_ascii_digit).collect();
        return match (rest.starts_with('f'), bits.is_empty()) {
            (_, true) => format!("PCM ({})", rest.to_ascii_uppercase()),
            (true, false) => format!("PCM {bits}-bit float"),
            (false, false) => format!("PCM {bits}-bit"),
        };
    }
    match lower.as_str() {
        "" => "audio".into(),
        "mp3" | "mp2" | "mp1" => lower.to_ascii_uppercase(),
        "flac" => "FLAC".into(),
        "vorbis" => "Vorbis".into(),
        "opus" => "Opus".into(),
        "aac" => "AAC".into(),
        "alac" => "ALAC (Apple Lossless)".into(),
        "adpcm_ima_wav" | "adpcm_ms" => "ADPCM".into(),
        other => other.to_ascii_uppercase(),
    }
}

/// A local file as a media source.
fn open_file(path: &str) -> Result<(MediaSourceStream<'static>, Hint), String> {
    // (a name in bytes that are no UTF-8, as the playlist keeps it)
    let path_os = crate::fspath::to_path(path);
    let meta = std::fs::metadata(&path_os).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "no such file".to_owned()
        } else {
            e.to_string()
        }
    })?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() == 0 {
        return Err("the file is empty".into());
    }
    let file = File::open(&path_os).map_err(|e| e.to_string())?;
    let source: Box<dyn symphonia::core::io::MediaSource> =
        if READ_STALL_MS.load(Ordering::Relaxed) > 0 {
            Box::new(Stalling { file, read: 0 })
        } else {
            Box::new(file)
        };
    Ok((
        MediaSourceStream::new(source, Default::default()),
        hint_for(path, None),
    ))
}

/// An AIFF file shorter than its header says (a copy interrupted): while
/// it is opened, zeros up to the length it gives - its reader steps over
/// the sound to look for the chunks after it - and once `opened`, its true
/// end: the sound that is there is played, and the track ends with it.
struct CutShort {
    file: File,
    at: u64,
    /// The file's true length, and the one its header gives.
    there: u64,
    said: u64,
    opened: Arc<AtomicBool>,
}

impl CutShort {
    /// `path` as such a source, where it is an AIFF file shorter than its
    /// FORM chunk says.
    fn open(path: &std::path::Path) -> Option<CutShort> {
        use std::io::{Read, Seek};
        let mut file = File::open(path).ok()?;
        let mut head = [0u8; 12];
        file.read_exact(&mut head).ok()?;
        if &head[..4] != b"FORM" || !matches!(&head[8..12], b"AIFF" | b"AIFC") {
            return None;
        }
        let said = 8 + u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as u64;
        let there = file.metadata().ok()?.len();
        file.seek(std::io::SeekFrom::Start(0)).ok()?;
        (there < said).then(|| CutShort {
            file,
            at: 0,
            there,
            said,
            opened: Arc::new(AtomicBool::new(false)),
        })
    }
}

impl std::io::Read for CutShort {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = if self.at < self.there {
            self.file.read(buf)?
        } else if self.at < self.said && !self.opened.load(Ordering::Relaxed) {
            let n = buf.len().min((self.said - self.at) as usize);
            buf[..n].fill(0);
            n
        } else {
            0
        };
        self.at += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for CutShort {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let to = match pos {
            std::io::SeekFrom::Start(to) => Some(to),
            std::io::SeekFrom::Current(by) => self.at.checked_add_signed(by),
            std::io::SeekFrom::End(by) => self.said.checked_add_signed(by),
        }
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        self.at = to;
        if to <= self.there {
            self.file.seek(std::io::SeekFrom::Start(to))?;
        }
        Ok(to)
    }
}

impl symphonia::core::io::MediaSource for CutShort {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.said)
    }
}

/// Tests: the next file read stalls this long once, after READ_STALL_AT bytes
/// (a disk that sleeps, a network share that hesitates in the middle).
#[doc(hidden)]
pub static READ_STALL_MS: AtomicU64 = AtomicU64::new(0);
#[doc(hidden)]
pub static READ_STALL_AT: AtomicU64 = AtomicU64::new(0);

struct Stalling {
    file: File,
    read: u64,
}

impl std::io::Read for Stalling {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read >= READ_STALL_AT.load(Ordering::Relaxed) {
            let ms = READ_STALL_MS.swap(0, Ordering::Relaxed);
            if ms > 0 {
                thread::sleep(Duration::from_millis(ms));
            }
        }
        let n = self.file.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for Stalling {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(pos)
    }
}

impl symphonia::core::io::MediaSource for Stalling {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.file.metadata().ok().map(|m| m.len())
    }
}

/// The decoder for a media source; `seekable` for a file.
fn open_media(
    mss: MediaSourceStream<'static>,
    hint: Hint,
    seekable: bool,
    path: &str,
    out_rate: u32,
    measure: bool,
    loudness: Loudness,
) -> Result<(Decoding, TrackInfo), String> {
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| match e {
            SymError::IoError(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                "the file is empty or cut short".to_owned()
            }
            SymError::IoError(e) => e.to_string(),
            e => format!("not a supported audio file ({e})"),
        })?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?
        .clone();
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("no audio codec")?
        .clone();
    // what symphonia cannot tell of the file, read from its framing: the
    // links of a chained Ogg file, the frames of an MP3 without a Xing
    // header (its length is only estimated: decoded without gapless
    // trimming - the estimate cut the sound short)
    let short = format.format_info().short_name;
    // (`measure` false: a stream - nothing to read ahead in)
    let on_disk = (seekable && measure).then(|| crate::fspath::to_path(path));
    let mp3 = match &on_disk {
        Some(p) if matches!(short, "mp3" | "mp2" | "mp1") && track.delay.is_none() => {
            crate::seektable::mp3_frames(p)
        }
        _ => None,
    };
    let links = match &on_disk {
        Some(p) if short == "ogg" => crate::seektable::ogg_links(p),
        _ => None,
    };
    let options = AudioDecoderOptions::default().gapless(mp3.is_none());
    let decoder = codecs()
        .make_audio_decoder(&params, &options)
        .map_err(|e| format!("cannot decode it ({e})"))?;
    let rate = params.sample_rate.unwrap_or(44_100);
    // (a header's rate of 0 or 1 Hz - a broken or hostile file - played
    // "0:00" for ever, the 1 Hz one resampled into 560 MB: refused here,
    // said, and the next track played)
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(format!(
            "its sample rate, {rate} Hz, is not one that can be played"
        ));
    }
    let mut info = TrackInfo {
        rate,
        channels: params.channels.as_ref().map(|c| c.count()).unwrap_or(2),
        bits: params.bits_per_sample,
        seekable,
        codec: codec_name(
            codecs()
                .get_audio_decoder(params.codec)
                .map(|d| d.codec.info.short_name)
                .unwrap_or(""),
        ),
        ..Default::default()
    };
    if let (Some(tb), Some(frames)) = (track.time_base, track.num_frames)
        && let Ok(ts) = Timestamp::try_from(frames)
    {
        info.duration = tb.calc_time(ts).map(|t| t.as_secs_f64());
    }
    if let Some(frames) = &mp3 {
        info.duration = Some(frames.secs());
    }
    if let Some(links) = &links {
        info.duration = Some(links.iter().map(|l| l.secs).sum());
    }
    // Every revision of the metadata (a file may carry several tags - an ID3v2
    // before an APE, two ID3v2): the newest first, an older one fills in what
    // it lacks (the cover was only in the first tag of a file that had two).
    let mut revisions = Vec::new();
    {
        let mut metadata = format.metadata();
        while let Some(revision) = metadata.pop() {
            revisions.push(revision);
        }
        if let Some(revision) = metadata.current() {
            revisions.push(revision.clone());
        }
    }
    for revision in revisions.iter().rev() {
        tag_text(&revision.media.tags, &mut info);
        for track in &revision.per_track {
            tag_text(&track.metadata.tags, &mut info);
        }
    }
    let visuals = || {
        revisions
            .iter()
            .rev()
            .flat_map(|r| r.media.visuals.iter())
            .filter(|v| v.data.len() < 16 << 20)
    };
    let front = visuals().find(|v| matches!(v.usage, Some(StandardVisualKey::FrontCover)));
    info.cover = front.or_else(|| visuals().next()).map(|v| v.data.to_vec());
    // MP4 / M4A: the edit list says where the sound begins and how long it
    // is (symphonia does not apply it yet)
    let edit = if seekable && is_mp4(path) {
        crate::mp4edit::read_edit(&crate::fspath::to_path(path)).map(|e| e.frames(rate))
    } else {
        None
    };
    if let Some(length) = edit.and_then(|(_, l)| l) {
        info.duration = Some(length as f64 / rate as f64);
    }
    if seekable
        && let (Ok(meta), Some(secs)) = (
            std::fs::metadata(crate::fspath::to_path(path)),
            info.duration,
        )
        && secs > 0.5
    {
        info.bitrate = Some((meta.len() as f64 * 8.0 / secs / 1000.0).round() as u32);
    }
    let decoding = Decoding {
        format,
        decoder,
        track_id: track.id,
        time_base: track.time_base,
        resampler: Resampler::new(rate, out_rate),
        resample: true,
        seekable,
        scratch: Vec::new(),
        stereo: Vec::new(),
        layout: None,
        matrix: Vec::new(),
        rate,
        skip: edit.map(|(s, _)| s).unwrap_or(0),
        left: edit.and_then(|(_, l)| l),
        delay: edit.map(|(s, _)| s).unwrap_or(0),
        length: edit.and_then(|(_, l)| l),
        gain: loudness.factor(&info),
        limiter: crate::limiter::Limiter::new(rate),
        eof: false,
        file: seekable.then(|| path.to_owned()),
        links,
        link: 0,
        mp3,
        options,
        link_rate: rate,
        link_resampler: None,
        linked: Vec::new(),
        until_ts: None,
        link_title: None,
        sounded: false,
        damaged: 0,
        ogg_tail: None,
        preskip: opus_preskip(&params),
        preskip_armed: true,
        dropped: 0,
    };
    Ok((decoding, info))
}

/// A local file, opened to play.
fn open(
    path: &str,
    out_rate: u32,
    measure: bool,
    loudness: Loudness,
) -> Result<(Decoding, TrackInfo), String> {
    let delay = OPEN_DELAY_MS.load(Ordering::Relaxed);
    if delay > 0 {
        thread::sleep(Duration::from_millis(delay));
    }
    let (mss, hint) = open_file(path)?;
    let on_disk = crate::fspath::to_path(path);
    // an Ogg file cut in the middle of a page (a copy that stopped): its
    // whole pages, and of that page the packets that are whole - the
    // reader, where it opens such a file at all, stops at its last whole
    // page: up to a second and more of sound on the disk not played
    // (review 19, L9)
    let cut = crate::seektable::ogg_cut_page(&on_disk).map(|(at, page)| (at, Arc::new(page)));
    let in_pieces = |why: String| -> Result<(Decoding, TrackInfo), String> {
        // an Ogg file damaged in its middle: symphonia's reader reads to its
        // end as it opens it, and refused it whole - what comes before the
        // damage is played (review 14)
        // (its whole pages: also those after damage in its first page of
        // sound, where nothing came before it - review 15)
        let Some(pages) = crate::seektable::ogg_whole_pages(&on_disk) else {
            return Err(why);
        };
        let mut pieces =
            crate::seektable::Pieces::open(&on_disk, pages).map_err(|_| why.clone())?;
        let tail = cut.clone().filter(|(at, _)| *at == pieces.end());
        if let Some((_, page)) = &tail {
            pieces = pieces.with_tail(page.clone());
        }
        let mss = MediaSourceStream::new(Box::new(pieces), Default::default());
        let (mut decoding, info) = open_media(
            mss,
            hint_for(path, None),
            true,
            path,
            out_rate,
            measure,
            loudness,
        )
        .map_err(|_| why)?;
        decoding.ogg_tail = tail;
        Ok((decoding, info))
    };
    if cut.is_some()
        && let Ok(opened) = in_pieces(String::new())
    {
        return Ok(opened);
    }
    let opened = open_media(mss, hint, true, path, out_rate, measure, loudness).or_else(|why| {
        // an AIFF file cut short: its reader goes past the sound for what
        // may follow it, and refused the file whole where a WAV file cut
        // at the same place plays - read as long as it says it is while
        // it is opened, and to its true end after (review 19, L9)
        if let Some(cut) = CutShort::open(&on_disk) {
            let opened = cut.opened.clone();
            let mss = MediaSourceStream::new(Box::new(cut), Default::default());
            if let Ok(decoding) = open_media(
                mss,
                hint_for(path, None),
                true,
                path,
                out_rate,
                measure,
                loudness,
            ) {
                opened.store(true, Ordering::Relaxed);
                return Ok(decoding);
            }
        }
        in_pieces(why)
    })?;
    Ok(pcm_there(&on_disk, opened))
}

/// A WAV or AIFF file of plain samples that is shorter than its first
/// bytes say (a copy that stopped): its length by the bytes that are
/// there, not by its header - 20 s were shown over 10 s of sound (review
/// 20, L5). What else is cut short says its true length when its end is
/// read ([`Event::Length`]).
fn pcm_there(
    path: &std::path::Path,
    (decoding, mut info): (Decoding, TrackInfo),
) -> (Decoding, TrackInfo) {
    use std::io::Read as _;
    let said = || -> Option<u64> {
        let mut head = [0u8; 12];
        File::open(path).ok()?.read_exact(&mut head).ok()?;
        let length = [head[4], head[5], head[6], head[7]];
        match (&head[..4], &head[8..12]) {
            (b"RIFF", b"WAVE") => Some(8 + u32::from_le_bytes(length) as u64),
            (b"FORM", b"AIFF" | b"AIFC") => Some(8 + u32::from_be_bytes(length) as u64),
            _ => None,
        }
    };
    if let (true, Some(secs), Some(bits), Some(said), Ok(meta)) = (
        info.codec.starts_with("PCM"),
        info.duration,
        info.bits,
        said(),
        std::fs::metadata(path),
    ) {
        let rate = (info.rate as u64 * info.channels as u64 * bits as u64 / 8) as f64;
        let there = meta.len();
        if rate > 0.0 && there < said {
            // (what the file says it has before and after its samples)
            let around = (said as f64 - secs * rate).max(12.0);
            info.duration = Some(((there as f64 - around).max(0.0) / rate).min(secs));
        }
    }
    (decoding, info)
}

impl Decoding {
    /// Seek to `secs`; returns where it landed (ms).
    fn seek(&mut self, secs: f64) -> Option<u64> {
        if !self.seekable {
            return None;
        }
        let secs = secs.max(0.0);
        if let Some(links) = &self.links {
            // a chained file: the link holding `secs` opened (the reader
            // seeks within the link it reads), then the seek in it
            let i = links.iter().rposition(|l| l.start <= secs).unwrap_or(0);
            let (offset, start, base) = (links[i].offset, links[i].start, links[i].base);
            let within = secs - start;
            if links[i].gaps {
                // pages where no packet ends lead symphonia's seek astray: the
                // link decoded from a page some seconds before the place (its
                // header pages, then its sound from there - from its start
                // where it has none so early), the sound before `within`
                // dropped (review 17; review 18, M4: from its start always,
                // a seek near the end of an hour was 9 s of silence)
                let media = within + base + self.delay as f64 / self.rate.max(1) as f64;
                let pieces = self.file.as_ref().and_then(|path| {
                    let len = std::fs::metadata(crate::fspath::to_path(path)).ok()?.len();
                    links[i].pieces_from(media, SEEK_LEAD, len)
                });
                match pieces {
                    // (the reader, begun in the stream's middle, tells its
                    // packets' times by the pages' granule positions: the
                    // link's own time, as from its start)
                    Some(pieces) => self.reopen_pieces(pieces).ok()?,
                    None => self.reopen(offset).ok()?,
                }
                self.link = i;
                let tb = self.time_base?;
                let ts = media * tb.denom.get() as f64 / tb.numer.get() as f64;
                self.until_ts = Some(ts.round() as i64);
                let played = ((within + base) * self.rate as f64) as u64;
                self.left = self.length.map(|l| l.saturating_sub(played));
                return Some((secs * 1000.0).round() as u64);
            }
            if i != self.link {
                self.reopen(offset).ok()?;
                self.link = i;
                if within < 0.001 {
                    return Some((start * 1000.0).round() as u64);
                }
            }
            // (the link's own time: from where its granules begin - a
            // recording's are hours in)
            let landed = self.seek_in(within + base)?;
            let base_ms = (base * 1000.0).round() as u64;
            return Some(landed.saturating_sub(base_ms) + (start * 1000.0).round() as u64);
        }
        if let Some(mp3) = &self.mp3 {
            // an MP3 without a Xing header: its own seek is bound by the
            // estimated length - opened again a few frames before the target
            // (the bit reservoir), the sound before it dropped
            let target = (secs * mp3.rate as f64) as u64;
            let every = crate::seektable::MP3_EVERY;
            let frame = (target / mp3.per_frame as u64).saturating_sub(4) / every;
            let frame = frame.min(mp3.offsets.len().saturating_sub(1) as u64);
            let offset = *mp3.offsets.get(frame as usize)?;
            let from = frame * every * mp3.per_frame as u64;
            self.reopen(offset).ok()?;
            self.until_ts = Some((target - from.min(target)) as i64);
            return Some((secs * 1000.0).round() as u64);
        }
        self.seek_in(secs)
    }

    /// Open the file again at `offset` (a link's first page, an MP3 frame):
    /// its reader and decoder anew, from there.
    fn reopen(&mut self, offset: u64) -> Result<(), String> {
        let path = self.file.clone().ok_or("not a file")?;
        let window = crate::seektable::Window::open(&crate::fspath::to_path(&path), offset)
            .map_err(|e| e.to_string())?;
        self.reopen_from(&path, Box::new(window))
    }

    /// Open `pieces` of the file again, read as one (an Ogg link's header
    /// pages, then its sound from a page in its middle).
    fn reopen_pieces(&mut self, pieces: Vec<(u64, u64)>) -> Result<(), String> {
        let path = self.file.clone().ok_or("not a file")?;
        let mut pieces = crate::seektable::Pieces::open(&crate::fspath::to_path(&path), pieces)
            .map_err(|e| e.to_string())?;
        // (a file cut in a page: what is whole of that page after them)
        if let Some((at, page)) = &self.ogg_tail
            && *at == pieces.end()
        {
            pieces = pieces.with_tail(page.clone());
        }
        self.reopen_from(&path, Box::new(pieces))
    }

    fn reopen_from(
        &mut self,
        path: &str,
        source: Box<dyn symphonia::core::io::MediaSource>,
    ) -> Result<(), String> {
        let mss = MediaSourceStream::new(source, Default::default());
        let format = symphonia::default::get_probe()
            .probe(
                &hint_for(path, None),
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| e.to_string())?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or("no audio track")?
            .clone();
        self.format = format;
        self.use_track(&track)?;
        self.limiter.reset();
        self.eof = false;
        self.skip = 0;
        self.until_ts = None;
        self.resampler = Resampler::new(self.rate, self.resampler.to);
        Ok(())
    }

    /// Decode `track` from here on: its decoder, its rate (resampled to the
    /// file's first one when another).
    fn use_track(&mut self, track: &symphonia::core::formats::Track) -> Result<(), String> {
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or("no audio codec")?
            .clone();
        self.decoder = codecs()
            .make_audio_decoder(&params, &self.options)
            .map_err(|e| format!("cannot decode it ({e})"))?;
        self.track_id = track.id;
        self.time_base = track.time_base;
        let rate = params.sample_rate.unwrap_or(self.rate);
        if !(MIN_RATE..=MAX_RATE).contains(&rate) {
            return Err(format!(
                "its sample rate, {rate} Hz, is not one that can be played"
            ));
        }
        self.link_rate = rate;
        self.link_resampler = (rate != self.rate).then(|| Resampler::new(rate, self.rate));
        self.layout = None;
        self.preskip = opus_preskip(&params);
        self.preskip_armed = true;
        Ok(())
    }

    /// A packet of another track: the next link of a chained Ogg stream (a
    /// new physical stream - an Icecast station's next song) when the
    /// reader's track is now that one; false for a track of another kind
    /// (a container's second track: passed over).
    /// (`id` None: the reader said a new stream began - its track is the
    /// one from here on, whatever its id.)
    fn next_link(&mut self, id: Option<u32>) -> Result<bool, String> {
        let Some(track) = self.format.default_track(TrackType::Audio) else {
            return Ok(false);
        };
        if id.is_some_and(|id| track.id != id || track.id == self.track_id) {
            return Ok(false);
        }
        let track = track.clone();
        self.use_track(&track)?;
        if self.links.as_ref().is_some_and(|l| self.link + 1 < l.len()) {
            self.link += 1;
        }
        let mut info = TrackInfo::default();
        let mut metadata = self.format.metadata();
        while metadata.pop().is_some() {}
        if let Some(revision) = metadata.current() {
            tag_text(&revision.media.tags, &mut info);
            for t in &revision.per_track {
                tag_text(&t.metadata.tags, &mut info);
            }
        }
        if !info.title.is_empty() {
            self.link_title = Some(if info.artist.is_empty() {
                info.title
            } else {
                format!("{} - {}", info.artist, info.title)
            });
        }
        Ok(true)
    }

    /// Seek the reader to `secs` of what it reads (a link's own time in a
    /// chained file); where it landed (ms).
    fn seek_in(&mut self, secs: f64) -> Option<u64> {
        // in the media, the sound begins after the edit's delay
        let media = secs + self.delay as f64 / self.rate.max(1) as f64;
        let time = Time::try_from_secs_f64(media)?;
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .ok()?;
        self.decoder.reset();
        self.limiter.reset();
        self.eof = false;
        self.resampler = Resampler::new(self.rate, self.resampler.to);
        if self.link_resampler.is_some() {
            self.link_resampler = Some(Resampler::new(self.link_rate, self.rate));
        }
        // An accurate seek lands at the start of the packet holding the time
        // asked for: the frames before it are dropped (the sound began about
        // a packet early).
        let to_secs = |ts| {
            self.time_base
                .and_then(|tb| tb.calc_time(ts))
                .map(|t| t.as_secs_f64())
        };
        let required = to_secs(seeked.required_ts);
        // (by the packets' own timestamps - the decoder's trimming of an
        // encoder's delay counted: a seek to 0 dropped an MP3's first 331 to
        // 1204 frames, and played an Opus file's 312 of pre-skip - review 13)
        self.skip = 0;
        self.until_ts = Some(seeked.required_ts.get());
        let played = (secs * self.rate as f64) as u64;
        self.left = self.length.map(|l| l.saturating_sub(played));
        let landed = required
            .map(|r| r - self.delay as f64 / self.rate.max(1) as f64)
            .unwrap_or(secs);
        Some((landed.max(0.0) * 1000.0).round() as u64)
    }

    /// The output's rate changed: resample to `rate` from here on.
    fn set_out_rate(&mut self, rate: u32) {
        if rate != self.resampler.to {
            self.resampler = Resampler::new(self.rate, rate);
        }
    }

    /// Decode the next packet into `out` (stereo at the output's rate,
    /// levelled). False at the end (the resampler's tail still held: see
    /// [`Decoding::finish`]).
    fn step(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                // (a chained Ogg stream's next link: its own decoder from here)
                Err(SymError::ResetRequired) if self.next_link(None)? => continue,
                Err(SymError::ResetRequired) => return Ok(false),
                // damage in the middle of a file: read on past it (a few
                // times), else the track ends there - what came before is
                // heard (it failed the whole track, and up to 3 s of good
                // sound were dropped); a stream's error is its own (it is
                // connected again)
                Err(e) if self.seekable && self.sounded => {
                    self.damaged += 1;
                    if self.damaged > 16 {
                        eprintln!("axross-amp: {e} - the track ends there");
                        return Ok(false);
                    }
                    continue;
                }
                Err(SymError::IoError(e)) => return Err(e.to_string()),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id != self.track_id && !self.next_link(Some(packet.track_id))? {
                continue;
            }
            // where the decoder's sound of this packet begins in the reader's
            // time (its own trimming counted)
            let begins = packet.pts.get()
                + if self.options.gapless {
                    packet.trim_start.get() as i64
                } else {
                    0
                }
                - if self.preskip_armed { 0 } else { self.preskip };
            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    self.preskip_armed = false;
                    to_stereo(
                        &buffer,
                        (&mut self.layout, &mut self.matrix),
                        &mut self.scratch,
                        &mut self.stereo,
                    );
                    // (a NaN or an infinity in a file - one is enough - kept
                    // the equalizer's filters at NaN to the track's end)
                    for s in &mut self.stereo {
                        if !s.is_finite() {
                            *s = 0.0;
                        }
                    }
                    if let Some(until) = self.until_ts {
                        let frames = (self.stereo.len() / 2) as i64;
                        let drop = (until - begins).clamp(0, frames) as usize;
                        self.stereo.drain(..drop * 2);
                        if begins + frames >= until {
                            self.until_ts = None;
                        } else if frames > 0 {
                            // (still before the place sought: back to the
                            // caller now and then - a seek that overtakes
                            // this one need not wait for it)
                            self.dropped += 1;
                            if self.dropped.is_multiple_of(64) {
                                return Ok(true);
                            }
                        }
                    }
                    if self.skip > 0 {
                        let drop = (self.skip as usize * 2).min(self.stereo.len());
                        self.stereo.drain(..drop);
                        self.skip -= (drop / 2) as u64;
                    }
                    if let Some(left) = self.left.as_mut() {
                        let keep = (*left as usize * 2).min(self.stereo.len());
                        self.stereo.truncate(keep);
                        *left -= (keep / 2) as u64;
                        if *left == 0 && self.stereo.is_empty() {
                            return Ok(false); // the padding after the sound
                        }
                    }
                    if let Some(link) = self.link_resampler.as_mut() {
                        // (a link at another rate: to the file's first one)
                        self.linked.clear();
                        link.process(&self.stereo, &mut self.linked);
                        std::mem::swap(&mut self.stereo, &mut self.linked);
                    }
                    if self.stereo.is_empty() {
                        continue;
                    }
                    if self.gain != 1.0 {
                        for s in &mut self.stereo {
                            *s *= self.gain;
                        }
                    }
                    // made louder: no peak above the ceiling (at unity gain
                    // the samples stay as they are, bit for bit)
                    if self.gain > 1.0 || self.limiter.active {
                        self.limiter.process(&mut self.stereo);
                        if self.stereo.is_empty() {
                            continue;
                        }
                    }
                    if self.resample {
                        self.resampler.process(&self.stereo, out);
                    } else {
                        out.extend_from_slice(&self.stereo);
                    }
                    self.sounded = true;
                    self.damaged = 0;
                    return Ok(true);
                }
                Err(SymError::DecodeError(_)) => continue, // (a damaged packet: the next one)
                Err(e) if self.seekable && self.sounded => {
                    eprintln!("axross-amp: {e} - the track ends there");
                    return Ok(false);
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// The end: what the resampler still holds.
    fn finish(&mut self, out: &mut Vec<f32>) {
        if !self.eof {
            // what the limiter held back
            self.stereo.clear();
            self.limiter.flush(&mut self.stereo);
            if self.resample {
                if !self.stereo.is_empty() {
                    self.resampler.process(&self.stereo, out);
                }
                self.resampler.flush(out);
            } else {
                out.extend_from_slice(&self.stereo);
            }
            self.eof = true;
        }
    }
}

/// A decoded buffer as stereo, by the downmix of its layout (made anew
/// when the layout changes).
fn to_stereo(
    buffer: &GenericAudioBufferRef<'_>,
    (layout, matrix): (&mut Option<Channels>, &mut Vec<(f32, f32)>),
    scratch: &mut Vec<f32>,
    stereo: &mut Vec<f32>,
) {
    let channels = buffer.spec().channels();
    if layout.as_ref() != Some(channels) {
        *matrix = downmix_matrix(channels);
        *layout = Some(channels.clone());
    }
    scratch.resize(buffer.samples_interleaved(), 0.0);
    buffer.copy_to_slice_interleaved(&mut scratch[..]);
    downmix(scratch, matrix, stereo);
}

/// The tags and length of a local file, without playing it (the playlist
/// learns titles and lengths in the background).
pub fn probe(path: &str) -> Result<TrackInfo, String> {
    // (measured as when it plays: a VBR MP3 without a Xing header said
    // symphonia's estimate in the playlist, a chained Ogg its first link's
    // length - review 14; the prober runs on a thread of its own)
    open(path, 48_000, true, Loudness::default()).map(|(_, info)| info)
}

/// Decode all of `path` as the player would (stereo at `rate`): the
/// frames made and the track's info. For tests and measurements.
pub fn decode_file(path: &str, rate: u32) -> Result<(usize, TrackInfo), String> {
    let (mut decoding, info) = open(path, rate, true, Loudness::default())?;
    let mut out = Vec::new();
    let mut frames = 0;
    while decoding.step(&mut out)? {
        frames += out.len() / 2;
        out.clear();
    }
    decoding.finish(&mut out);
    frames += out.len() / 2;
    Ok((frames, info))
}

/// The waveform of all of `path` (the overview face): `columns` of the
/// mono mix's lowest and highest sample and its RMS, over the track's
/// length; `progress` is told the columns so far and the share read (now
/// and then, and at the end). Gives up when `cancel` is set.
pub fn waveform(
    path: &str,
    columns: usize,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&[[f32; 3]], f32),
) -> Result<(), String> {
    let (mut decoding, info) = open(path, 48_000, true, Loudness::default())?;
    let rate = if decoding.resample {
        48_000
    } else {
        decoding.rate.max(1)
    } as f64;
    let total = info
        .duration
        .map(|d| (d * rate) as u64)
        .filter(|f| *f > 0)
        .ok_or("the track's length is not known")?;
    let per = (total / columns.max(1) as u64).max(1);
    let mut out = Vec::new();
    let mut made: Vec<[f32; 3]> = Vec::with_capacity(columns + 1);
    let (mut lo, mut hi, mut sum, mut count) = (0.0f32, 0.0f32, 0.0f64, 0u64);
    let mut frames = 0u64;
    let mut told = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let more = decoding.step(&mut out)?;
        if !more {
            decoding.finish(&mut out);
        }
        for f in out.as_chunks::<2>().0 {
            let v = (f[0] + f[1]) * 0.5;
            lo = lo.min(v);
            hi = hi.max(v);
            sum += (v * v) as f64;
            count += 1;
            frames += 1;
            if count == per {
                made.push([lo, hi, (sum / count as f64).sqrt() as f32]);
                (lo, hi, sum, count) = (0.0, 0.0, 0.0, 0);
            }
        }
        out.clear();
        if !more {
            break;
        }
        if told.elapsed() > Duration::from_millis(150) {
            told = Instant::now();
            progress(&made, (frames as f64 / total as f64).min(0.99) as f32);
        }
    }
    if count > 0 {
        made.push([lo, hi, (sum / count as f64).sqrt() as f32]);
    }
    progress(&made, 1.0);
    Ok(())
}

/// symphonia's decoders and Opus (libopus, built in: symphonia has no Opus
/// decoder of its own; Ogg Opus and WebM / MKV Opus).
fn codecs() -> &'static symphonia::core::codecs::registry::CodecRegistry {
    static REGISTRY: OnceLock<symphonia::core::codecs::registry::CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = symphonia::core::codecs::registry::CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        registry
    })
}

fn is_mp4(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".m4a", ".m4b", ".mp4", ".m4v", ".mov", ".3gp"]
        .iter()
        .any(|e| lower.ends_with(e))
}

// -- streams -----------------------------------------------------------------------------------

/// An http(s) source, decoded on a thread of its own into `pcm`.
struct StreamSource {
    pcm: Receiver<Vec<f32>>,
    cancel: Arc<AtomicBool>,
    /// The output's rate the thread resamples to.
    rate: Arc<AtomicU32>,
    /// Why the thread gave up (None: the stream's clean end).
    error: Arc<Mutex<Option<String>>>,
    generation: u64,
    /// The queue closed (the thread ended).
    eof: bool,
    /// Sound taken from the queue, not yet given to the output, and whether
    /// enough of it came to start (again, after the output ran dry).
    pending: VecDeque<Vec<f32>>,
    pending_frames: usize,
    primed: bool,
}

impl StreamSource {
    /// Take what the thread decoded, as far as `frames` (or all there is).
    fn gather(&mut self, frames: usize) {
        while self.pending_frames < frames {
            match self.pcm.try_recv() {
                Ok(chunk) => {
                    self.pending_frames += chunk.len() / 2;
                    self.pending.push_back(chunk);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.eof = true;
                    break;
                }
            }
        }
    }
}

impl Drop for StreamSource {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

type Wake = Arc<dyn Fn() + Send + Sync>;

fn start_stream(
    url: String,
    generation: u64,
    out_rate: u32,
    events: Sender<Event>,
    wake: Wake,
) -> StreamSource {
    let (tx, pcm) = crossbeam_channel::bounded(STREAM_CHUNKS);
    let cancel = Arc::new(AtomicBool::new(false));
    let rate = Arc::new(AtomicU32::new(out_rate));
    let error = Arc::new(Mutex::new(None));
    let (c, r, e) = (cancel.clone(), rate.clone(), error.clone());
    let spawned = thread::Builder::new()
        .name("amp-stream".into())
        .spawn(move || {
            let why = produce(&url, generation, &r, &events, &wake, &c, &tx);
            if let (Some(why), Ok(mut slot)) = (why, e.lock()) {
                *slot = Some(why);
            }
            // (the queue closes when the thread ends: the decoder thread sees it)
        });
    if let Err(why) = spawned
        && let Ok(mut slot) = error.lock()
    {
        *slot = Some(why.to_string());
    }
    StreamSource {
        pcm,
        cancel,
        rate,
        error,
        generation,
        eof: false,
        pending: VecDeque::new(),
        pending_frames: 0,
        primed: false,
    }
}

/// A pause of `secs` that ends early when `cancel` is set.
fn wait(secs: u64, cancel: &AtomicBool) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end && !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
    }
}

/// Connect, decode, send; connect again when a radio breaks off. Returns
/// why it gave up (None: the end of a file on a web server, or stopped).
fn produce(
    url: &str,
    generation: u64,
    rate: &AtomicU32,
    events: &Sender<Event>,
    wake: &Wake,
    cancel: &Arc<AtomicBool>,
    pcm: &Sender<Vec<f32>>,
) -> Option<String> {
    let send = |event: Event| {
        let _ = events.send(event);
        wake();
    };
    let mut loaded = false;
    let mut tries = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let started = Instant::now();
        let (ev, wk) = (events.clone(), wake.clone());
        let on_title = Box::new(move |title: String| {
            let _ = ev.send(Event::StreamTitle { generation, title });
            wk();
        });
        let why = match crate::net::open(url, cancel.clone(), on_title) {
            Err(why) => why,
            Ok((answer, body)) => {
                let mss =
                    MediaSourceStream::new(Box::new(ReadOnlySource::new(body)), Default::default());
                let hint = hint_for(&answer.url, answer.content_type.as_deref());
                let out_rate = rate.load(Ordering::Relaxed);
                match open_media(
                    mss,
                    hint,
                    false,
                    &answer.url,
                    out_rate,
                    false,
                    Loudness::default(),
                ) {
                    Err(why) => why,
                    Ok((mut decoding, mut info)) => {
                        info.station = answer.station.clone();
                        info.station_genre = answer.genre.clone();
                        make_art(&mut info);
                        let duration = info.duration;
                        if loaded {
                            send(Event::Notice {
                                generation,
                                message: String::new(),
                            });
                        } else {
                            send(Event::Loaded { generation, info });
                            loaded = true;
                        }
                        let mut played = 0.0f64;
                        let ended = loop {
                            if cancel.load(Ordering::Relaxed) {
                                return None;
                            }
                            let out_rate = rate.load(Ordering::Relaxed);
                            decoding.set_out_rate(out_rate);
                            let mut out = Vec::with_capacity(4096);
                            match decoding.step(&mut out) {
                                Ok(true) => {
                                    played += (out.len() / 2) as f64 / out_rate.max(1) as f64;
                                    if let Some(title) = decoding.link_title.take() {
                                        send(Event::StreamTitle { generation, title });
                                    }
                                    if pcm.send(out).is_err() {
                                        return None;
                                    }
                                }
                                Ok(false) => {
                                    decoding.finish(&mut out);
                                    if !out.is_empty() && pcm.send(out).is_err() {
                                        return None;
                                    }
                                    break Ok(());
                                }
                                Err(why) => break Err(why),
                            }
                        };
                        // The end of the body: the end of the sound (a file on a
                        // web server: its length, or as long as it says it is,
                        // or nothing says it is a radio), else a radio that
                        // broke off.
                        let cut_short = duration.is_some_and(|d| played + 1.0 < d);
                        let radio = answer.length.is_none() && (answer.icy || cut_short);
                        match ended {
                            Ok(()) if !radio => return None,
                            Ok(()) => "the server ended it".to_owned(),
                            Err(why) => why,
                        }
                    }
                }
            }
        };
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if !loaded {
            return Some(why); // (the first connection: why, at once)
        }
        if started.elapsed() > Duration::from_secs(30) {
            tries = 0; // (it played a while: a new break, a new count)
        }
        tries += 1;
        if tries > RECONNECTS {
            return Some(format!("the stream broke off ({why})"));
        }
        send(Event::Notice {
            generation,
            message: format!(
                "The stream broke off ({why}) - connecting again ({tries} of {RECONNECTS})…"
            ),
        });
        wait(1 << (tries - 1), cancel);
    }
}

// -- the thread --------------------------------------------------------------------------------

enum Source {
    File(FileSource),
    Stream(StreamSource),
}

/// What a file's reading thread hands over.
enum Feed {
    Opened {
        info: Box<TrackInfo>,
        rate: u32,
        landed_ms: u64,
    },
    /// Stereo at the file's rate, levelled; of the seek `epoch`.
    Pcm {
        epoch: u64,
        samples: Vec<f32>,
    },
    Seeked {
        epoch: u64,
        landed_ms: u64,
    },
    End {
        epoch: u64,
    },
    Failed(String),
}

enum Control {
    Seek { secs: f64, epoch: u64 },
    Gain(f32),
}

/// A file read and decoded on a thread of its own, up to [`STAGE_SECS`]
/// ahead: a slow disk, a network share that hesitates, a file that takes
/// long to open hold up only that thread - the ring is filled, commands
/// are taken all the while.
struct FileSource {
    path: String,
    rx: Receiver<Feed>,
    ctl: Sender<Control>,
    /// Frames handed over and not yet taken (the thread reads ahead to its
    /// budget).
    queued: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    generation: u64,
    epoch: u64,
    info: Option<Box<TrackInfo>>,
    rate: u32,
    /// The end of what it reads came (for this epoch), and everything of it
    /// was taken (the resampler's tail given out).
    end: bool,
    finished: bool,
    /// Where its sound taken so far began (ms) and the frames since (how
    /// much of it is left: a crossfade's start).
    from_ms: u64,
    decoded: u64,
}

impl Drop for FileSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl FileSource {
    fn start(path: String, generation: u64, start_ms: u64, loudness: Loudness) -> FileSource {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ctl, controls) = crossbeam_channel::unbounded::<Control>();
        let queued = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (q, st) = (queued.clone(), stop.clone());
        let file = path.clone();
        let spawned = thread::Builder::new()
            .name("amp-read".into())
            .spawn(move || read_file(file, start_ms, loudness, tx, controls, q, st));
        let (failed_tx, failed_rx) = crossbeam_channel::bounded(1);
        let rx = match spawned {
            Ok(_) => rx,
            Err(e) => {
                let _ = failed_tx.send(Feed::Failed(e.to_string()));
                failed_rx
            }
        };
        FileSource {
            path,
            rx,
            ctl,
            queued,
            stop,
            generation,
            epoch: 0,
            info: None,
            rate: 0,
            end: false,
            finished: false,
            from_ms: start_ms,
            decoded: 0,
        }
    }

    /// Opened by now (its first message taken)? Err: it failed.
    fn opened(&mut self) -> Result<bool, String> {
        if self.info.is_some() {
            return Ok(true);
        }
        match self.rx.try_recv() {
            Ok(Feed::Opened { info, rate, .. }) => {
                self.rate = rate;
                self.info = Some(info);
                Ok(true)
            }
            Ok(Feed::Failed(why)) => Err(why),
            Ok(_) => Ok(false),
            Err(TryRecvError::Empty) => Ok(false),
            Err(TryRecvError::Disconnected) => Err("it could not be read".into()),
        }
    }

    fn seek(&mut self, secs: f64) {
        self.epoch += 1;
        self.end = false;
        self.finished = false;
        let _ = self.ctl.send(Control::Seek {
            secs,
            epoch: self.epoch,
        });
    }
}

/// The reading thread of a [`FileSource`].
#[allow(clippy::too_many_arguments)]
fn read_file(
    path: String,
    start_ms: u64,
    loudness: Loudness,
    tx: Sender<Feed>,
    controls: Receiver<Control>,
    queued: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
) {
    let (mut d, mut info) = match open(&path, 48_000, true, loudness) {
        Ok(opened) => opened,
        Err(why) => {
            let _ = tx.send(Feed::Failed(why));
            return;
        }
    };
    d.resample = false;
    make_art(&mut info);
    let landed_ms = if start_ms > 0 {
        d.seek(start_ms as f64 / 1000.0).unwrap_or(0)
    } else {
        0
    };
    let rate = d.rate;
    if tx
        .send(Feed::Opened {
            info: Box::new(info),
            rate,
            landed_ms,
        })
        .is_err()
    {
        return;
    }
    let budget = (rate as f32 * STAGE_SECS) as usize;
    let mut epoch = 0;
    let mut ended = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let handle = |c: Control, epoch: &mut u64, ended: &mut bool, d: &mut Decoding| match c {
            Control::Seek { secs, epoch: e } => {
                *epoch = e;
                *ended = false;
                let landed_ms = d.seek(secs).unwrap_or((secs * 1000.0) as u64);
                let _ = tx.send(Feed::Seeked {
                    epoch: e,
                    landed_ms,
                });
            }
            Control::Gain(g) => d.gain = g,
        };
        // (of the seeks waiting - a drag of the slider - the last one only:
        // each was decoded to its place before the next, review 18, M4)
        let mut seek = None;
        while let Ok(c) = controls.try_recv() {
            match c {
                Control::Seek { .. } => seek = Some(c),
                gain => handle(gain, &mut epoch, &mut ended, &mut d),
            }
        }
        if let Some(c) = seek {
            handle(c, &mut epoch, &mut ended, &mut d);
        }
        if ended || queued.load(Ordering::Relaxed) >= budget {
            match controls.recv_timeout(Duration::from_millis(10)) {
                Ok(c) => handle(c, &mut epoch, &mut ended, &mut d),
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            continue;
        }
        let mut samples = Vec::with_capacity(4096);
        match d.step(&mut samples) {
            // (nothing yet: on the way to a place sought)
            Ok(true) if samples.is_empty() => {}
            Ok(true) => {
                queued.fetch_add(samples.len() / 2, Ordering::Relaxed);
                if tx.send(Feed::Pcm { epoch, samples }).is_err() {
                    return;
                }
            }
            Ok(false) => {
                // what the limiter still holds back, before the end
                d.finish(&mut samples);
                if !samples.is_empty() {
                    queued.fetch_add(samples.len() / 2, Ordering::Relaxed);
                    if tx.send(Feed::Pcm { epoch, samples }).is_err() {
                        return;
                    }
                }
                ended = true;
                d.eof = false; // (a seek back reads on)
                if tx.send(Feed::End { epoch }).is_err() {
                    return;
                }
            }
            Err(why) => {
                let _ = tx.send(Feed::Failed(why));
                return;
            }
        }
    }
}

/// The decoder thread's state.
struct Player {
    shared: Arc<Shared>,
    events: Sender<Event>,
    wake: Wake,
    headless: bool,
    stream: Option<cpal::Stream>,
    out_rate: u32,
    ahead: usize,
    eq: Equalizer,
    gains: ([f32; 10], bool),
    loudness: Loudness,
    current: Option<Source>,
    queued: Option<Queued>,
    /// The output device chosen (None: the default), and when it failed lately.
    device: Option<String>,
    losses: Vec<Instant>,
    /// The track still heard while the queued one is decoded after it, and
    /// the queued one with its info (told when it is heard).
    /// The track decoded behind which the next one begins (its source:
    /// None in a crossfade, where it is fading), the next, its info.
    ending: Option<(Option<FileSource>, Queued, TrackInfo)>,
    /// A crossfade's length (s; 0: gapless), whether a track chosen while
    /// another plays fades in over it (1 s), and the track fading out.
    crossfade: f32,
    fade_on_skip: bool,
    fading: Option<Fading>,
    /// The queued track, opened and read ahead on its own thread.
    prepared: Option<(Queued, FileSource)>,
    /// A file's sound to the output's rate - one resampler on across a
    /// gapless join (the same rate: not even the filter's edge between them).
    resampler: Resampler,
    /// The output's limiter, always in the path (its 4 ms of delay never
    /// come and go), holding the peaks below RESAMPLED_DB wherever the sound
    /// may go over full scale: resampled (the peaks between samples become
    /// samples), two tracks in a crossfade (their sum), the equalizer
    /// raising a band. Elsewhere the samples go through bit for bit.
    out_limiter: crate::limiter::Limiter,
    /// The queued track's first sound is the next to go into the ring; and
    /// how far after it the new track counts as begun (a crossfade's
    /// middle: the meters start the new track there).
    pending_boundary: bool,
    mark_after: u64,
    /// Equalized and not yet taken by the ring (it was full).
    carry: Vec<f32>,
    /// An output to play on (else nothing plays), and when it was last tried.
    output_ok: bool,
    output_tried: Instant,
    chunk: Vec<f32>,
}

impl Player {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    /// Open the output `device` (None: the default); one that is not there
    /// (unplugged, a headset gone) gives way to the default output.
    fn connect(&mut self, device: Option<&str>) {
        self.device = device.map(str::to_owned);
        self.shared.lost.store(false, Ordering::Relaxed);
        self.output_tried = Instant::now();
        if NO_OUTPUT.load(Ordering::Relaxed) {
            self.output_ok = false;
            self.out_rate = 48_000;
            self.shared.playing.store(false, Ordering::Relaxed);
            return self.send(Event::Output("no sound: no audio device".into()));
        }
        self.output_ok = true;
        if self.headless {
            self.shared.device_rate.store(48_000, Ordering::Relaxed);
            self.out_rate = 48_000;
            return self.send(Event::Output("no device (headless)".into()));
        }
        let opened = match open_output(self.shared.clone(), device) {
            Err(why) if device.is_some() => open_output(self.shared.clone(), None)
                .map(|(s, r, name)| (s, r, format!("{name} - the output chosen is not there")))
                .map_err(|_| why),
            other => other,
        };
        match opened {
            Ok((stream, rate, name)) => {
                self.shared.device_rate.store(rate, Ordering::Relaxed);
                self.stream = Some(stream);
                self.out_rate = rate;
                self.send(Event::Output(name));
            }
            Err(why) => {
                self.out_rate = 48_000;
                self.output_ok = false;
                self.shared.playing.store(false, Ordering::Relaxed);
                self.send(Event::Output(format!("no sound: {why}")));
            }
        }
        self.ahead = (self.out_rate as f32 * AHEAD_SECS) as usize;
    }

    /// The output anew on `device`, the track going on where it was (at
    /// another rate: resampled anew from there).
    fn reopen(&mut self, device: Option<String>) {
        let playing = self.shared.playing.load(Ordering::Relaxed);
        let at = self.shared.position();
        drop(self.stream.take()); // (the old device lets go first)
        let before = self.out_rate;
        self.connect(device.as_deref());
        // (at the same rate what is decoded ahead plays on as it is - the
        // next track behind this one, a crossfade under way: taken back and
        // decoded again, the next track's start already in the ring was
        // heard twice - review 13)
        if self.out_rate != before {
            // another rate: the equalizer anew, the sound decoded
            // ahead dropped, the track resampled from where it was
            self.cancel_advance();
            self.eq = Equalizer::new(self.out_rate);
            self.eq.set_gains(self.gains.0);
            self.eq.enabled = self.gains.1;
            match self.current.as_mut() {
                Some(Source::File(f)) => {
                    // read again from what was heard (what was read ahead is
                    // at the old rate in the ring)
                    f.seek(at);
                    self.resampler = Resampler::new(f.rate.max(1), self.out_rate);
                    self.flush_at((at * 1000.0) as u64);
                }
                Some(Source::Stream(s)) => {
                    s.rate.store(self.out_rate, Ordering::Relaxed);
                    while s.pcm.try_recv().is_ok() {}
                    s.pending.clear();
                    s.pending_frames = 0;
                    s.primed = false;
                    self.flush_at(0);
                }
                None => {}
            }
        }
        self.shared
            .playing
            .store(playing && self.current.is_some(), Ordering::Relaxed);
    }

    /// The output failed (a device unplugged, the sound server gone):
    /// opened again - up to three times in ten seconds, then it says so.
    fn recover(&mut self) {
        if !self.shared.lost.swap(false, Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();
        self.losses
            .retain(|t| now.duration_since(*t) < Duration::from_secs(10));
        self.losses.push(now);
        if self.losses.len() > 3 {
            drop(self.stream.take());
            self.shared.playing.store(false, Ordering::Relaxed);
            return self.send(Event::Output(
                "no sound: the output keeps failing - choose one in the Mixer tab".into(),
            ));
        }
        let device = self.device.clone();
        self.reopen(device);
    }

    /// Take back a queued track already decoded after the current one (a
    /// seek or a new output in the track still heard).
    fn cancel_advance(&mut self) {
        if let Some((old, queued, _)) = self.ending.take() {
            match (old, self.fading.take()) {
                (Some(old), _) => {
                    self.resampler = Resampler::new(old.rate.max(1), self.out_rate);
                    self.current = Some(Source::File(old));
                }
                // (in a crossfade: the one fading out is the one heard)
                (None, Some(fading)) => {
                    self.resampler = fading.resampler;
                    self.current = Some(Source::File(fading.src));
                }
                (None, None) => {}
            }
            self.prepared = None;
            self.prepare(Some(queued));
        }
        self.shared.boundary.store(u64::MAX, Ordering::Relaxed);
        self.pending_boundary = false;
    }

    /// The queued track decoded behind the current one, taken back before
    /// it is heard (the current one ends then); false when it is too late.
    fn take_back_advance(&mut self) -> bool {
        if self.ending.is_none() {
            return true;
        }
        if self
            .ending
            .as_ref()
            .is_some_and(|(old, _, _)| old.is_none())
        {
            return false; // (a crossfade is under way: too late)
        }
        let boundary = self.shared.boundary.load(Ordering::Relaxed);
        if boundary != u64::MAX {
            // its start is in the ring already: taken out of it
            // (not when the output is within a call's reading of it)
            if !self.shared.ring.cut(boundary, CUT_MARGIN) {
                return false;
            }
            self.carry.clear();
        }
        self.shared.boundary.store(u64::MAX, Ordering::Relaxed);
        self.pending_boundary = false;
        if let Some((Some(mut old), _, _)) = self.ending.take() {
            // (it was at its end, all of it given out)
            old.end = true;
            old.finished = true;
            self.current = Some(Source::File(old));
        }
        true
    }

    /// What is decoded and not heard goes (faded out); the position starts at `ms`.
    fn flush_at(&mut self, ms: u64) -> u64 {
        self.out_limiter.reset();
        self.fading = None;
        let at = self.shared.restart_at(ms);
        self.shared.ending.store(false, Ordering::Relaxed);
        self.pending_boundary = false;
        self.carry.clear();
        at
    }

    /// Name the track to follow: opened from now on, on a thread of its own.
    fn prepare(&mut self, queued: Option<Queued>) {
        let same = match (&self.prepared, &queued) {
            (Some((p, _)), Some(q)) => p == q,
            (None, None) => true,
            _ => false,
        };
        self.queued = queued.clone();
        if same {
            return;
        }
        self.prepared = queued
            .filter(|q| !crate::playlist::is_stream(&q.path))
            .map(|q| {
                let f = FileSource::start(q.path.clone(), q.generation, 0, self.loudness);
                (q, f)
            });
    }

    fn reset(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.flush_at(0);
        self.eq.reset();
        self.current = None;
        self.ending = None;
        self.queued = None;
        self.prepared = None;
    }

    /// Play only with an output to play on (tried again first).
    fn may_play(&mut self) -> bool {
        if !self.output_ok {
            let device = self.device.clone();
            self.reopen(device);
        }
        self.output_ok
    }

    fn command(&mut self, command: Command) -> bool {
        match command {
            Command::Quit => return false,
            Command::Load {
                path,
                start_ms,
                play,
                generation,
            } => {
                // a track chosen while another plays: that one fades out
                // under it over a second (when asked for), from where it is
                // heard
                let playing = self.shared.playing.load(Ordering::Relaxed);
                let fades = self.fade_on_skip
                    && play
                    && playing
                    && !crate::playlist::is_stream(&path)
                    && matches!(&self.current, Some(Source::File(f)) if f.rate > 0 && !f.end && f.info.as_ref().is_some_and(|i| i.seekable));
                let heard = self.shared.position();
                let old = if fades {
                    match self.current.take() {
                        Some(Source::File(f)) => Some(f),
                        other => {
                            self.current = other;
                            None
                        }
                    }
                } else {
                    None
                };
                self.reset();
                // (the new track's sound begins where the ring goes on; in a
                // fade, counted from its middle)
                let total = self.out_rate as usize;
                let after = if old.is_some() { total as u64 / 2 } else { 0 };
                self.shared.mark_track(self.shared.end_of_buffer() + after);
                if crate::playlist::is_stream(&path) {
                    self.current = Some(Source::Stream(start_stream(
                        path,
                        generation,
                        self.out_rate,
                        self.events.clone(),
                        self.wake.clone(),
                    )));
                    let play = play && self.may_play();
                    self.shared.playing.store(play, Ordering::Relaxed);
                    return true;
                }
                // opened on its reading thread: a file that takes long to open
                // (a sleeping disk, a hung share) holds up no command
                self.flush_at(start_ms);
                self.current = Some(Source::File(FileSource::start(
                    path,
                    generation,
                    start_ms,
                    self.loudness,
                )));
                if let Some(mut old) = old {
                    old.seek(heard);
                    let resampler = Resampler::new(old.rate.max(1), self.out_rate);
                    self.fading = Some(Fading {
                        src: old,
                        resampler,
                        buf: Vec::new(),
                        total,
                        done: 0,
                        ended: false,
                        alike: 0.0,
                    });
                }
                let play = play && self.may_play();
                self.shared.playing.store(play, Ordering::Relaxed);
            }
            Command::Queue(queued) => {
                // while the queued one is already decoded behind the current
                // one, the next is told after it is heard - unless it is no
                // longer the one to follow (the end of the track after all,
                // another track): then it is taken back while it can be
                let pending = self.ending.as_ref().map(|(_, q, _)| q.clone());
                match pending {
                    None => self.prepare(queued),
                    Some(q) if Some(&q) != queued.as_ref() => {
                        if self.take_back_advance() {
                            self.prepare(queued);
                        }
                    }
                    Some(_) => {}
                }
            }
            Command::Play => {
                let play = self.current.is_some() && self.may_play();
                self.shared.playing.store(play, Ordering::Relaxed);
            }
            Command::Pause => self.shared.playing.store(false, Ordering::Relaxed),
            Command::Stop => {
                self.shared.playing.store(false, Ordering::Relaxed);
                self.cancel_advance();
                if let Some(Source::Stream(_)) = self.current {
                    // a stream is let go (its connection closed); Play connects anew
                    self.current = None;
                    self.flush_at(0);
                } else if let Some(Source::File(f)) = self.current.as_mut() {
                    f.seek(0.0);
                    self.resampler = Resampler::new(f.rate.max(1), self.out_rate);
                    self.flush_at(0);
                    self.eq.reset();
                } else {
                    self.flush_at(0); // (after the end: back to 0:00)
                }
            }
            Command::Seek(secs) => {
                if let Some(Source::File(_)) = self.current {
                    self.cancel_advance();
                }
                if let Some(Source::File(f)) = self.current.as_mut()
                    && f.info.as_ref().is_none_or(|i| i.seekable)
                {
                    f.seek(secs);
                    self.resampler = Resampler::new(f.rate.max(1), self.out_rate);
                    self.flush_at((secs.max(0.0) * 1000.0) as u64);
                    self.eq.reset();
                }
            }
            Command::Equalizer { gains, enabled } => {
                self.gains = (gains, enabled);
                self.eq.set_gains(gains);
                self.eq.enabled = enabled;
            }
            Command::Loudness(loudness) => {
                self.loudness = loudness;
                if let Some(Source::File(f)) = self.current.as_ref()
                    && let Some(info) = &f.info
                {
                    let _ = f.ctl.send(Control::Gain(loudness.factor(info)));
                }
                // the next track, opened ahead with the gain before: read
                // again with this one (it kept the old gain - review 13)
                if let Some((queued, _)) = self.prepared.take() {
                    self.queued = None;
                    self.prepare(Some(queued));
                }
            }
            Command::Crossfade(secs) => self.crossfade = secs.clamp(0.0, 12.0),
            Command::FadeOnSkip(on) => self.fade_on_skip = on,
            Command::Output(device) => {
                self.losses.clear();
                self.reopen(device);
            }
        }
        true
    }

    /// The queued track heard now: its position begins.
    fn check_boundary(&mut self) {
        let boundary = self.shared.boundary.load(Ordering::Relaxed);
        if boundary == u64::MAX || self.shared.heard() < boundary {
            return;
        }
        self.shared.origin.store(boundary, Ordering::Relaxed);
        self.shared.base_ms.store(0, Ordering::Relaxed);
        self.shared.boundary.store(u64::MAX, Ordering::Relaxed);
        if let Some((_, queued, info)) = self.ending.take() {
            self.send(Event::Advanced {
                generation: queued.generation,
                info,
            });
        }
    }

    /// At the end of a file: the queued one on after it, once it is open.
    /// Some(false): nothing to go on with; None: still opening.
    fn advance(&mut self) -> Option<bool> {
        if self.ending.is_some() {
            return Some(false);
        }
        let Some((_, next)) = self.prepared.as_mut() else {
            return Some(false);
        };
        match next.opened() {
            Ok(false) => return None,
            Ok(true) => {}
            Err(_) => {
                // (one that fails says so when its turn comes)
                self.prepared = None;
                self.queued = None;
                return Some(false);
            }
        }
        let (queued, next) = self.prepared.take()?;
        self.queued = None;
        let info = next.info.as_deref().cloned().unwrap_or_default();
        let Some(Source::File(current)) = self.current.as_ref() else {
            return Some(false);
        };
        if next.rate != current.rate {
            // another rate: what the resampler still holds of this one first
            let mut tail = Vec::new();
            self.resampler.flush(&mut tail);
            self.eq.process(&mut tail);
            if self.out_limiter.active {
                self.out_limiter.process(&mut tail);
            }
            self.carry.extend_from_slice(&tail);
            self.resampler = Resampler::new(next.rate.max(1), self.out_rate);
        }
        self.pending_boundary = true;
        if let Some(Source::File(old)) = self.current.replace(Source::File(next)) {
            self.ending = Some((Some(old), queued, info));
        }
        Some(true)
    }

    /// Near the end of the current track (as near as the crossfade is long),
    /// with the next one open and of another album: the next becomes the
    /// current one, the current one fades out under it.
    fn start_crossfade(&mut self) {
        if self.crossfade <= 0.0 || self.fading.is_some() || self.ending.is_some() {
            return;
        }
        let Some(Source::File(f)) = self.current.as_ref() else {
            return;
        };
        let (Some(info), true) = (f.info.as_deref(), f.rate > 0) else {
            return;
        };
        let Some(duration) = info.duration else {
            return;
        };
        let at = f.from_ms as f64 / 1000.0 + f.decoded as f64 / f.rate as f64;
        let left = duration - at;
        if left > self.crossfade as f64 || left < 0.3 {
            return;
        }
        let Some((_, next)) = self.prepared.as_mut() else {
            return;
        };
        if !matches!(next.opened(), Ok(true)) {
            return;
        }
        let next_info = next.info.as_deref().cloned().unwrap_or_default();
        if !crossfades_between(self.crossfade, (&f.path, info), (&next.path, &next_info)) {
            return;
        }
        let Some((queued, next)) = self.prepared.take() else {
            return;
        };
        self.queued = None;
        let resampler = std::mem::replace(
            &mut self.resampler,
            Resampler::new(next.rate.max(1), self.out_rate),
        );
        let Some(Source::File(old)) = self.current.replace(Source::File(next)) else {
            return;
        };
        self.fading = Some(Fading {
            src: old,
            resampler,
            buf: Vec::new(),
            total: ((left * self.out_rate as f64) as usize).max(1),
            done: 0,
            ended: false,
            alike: 0.0,
        });
        let total = ((left * self.out_rate as f64) as usize).max(1);
        self.ending = Some((None, queued, next_info));
        // (the next track is heard from the fade's start; the meters count
        // it from the fade's middle, where it is the louder)
        self.pending_boundary = true;
        self.mark_after = total as u64 / 2;
    }

    /// The track fading out read ahead of the one fading in (enough for
    /// its next chunk); false while it is not (wait for it).
    fn pump_fading(&mut self) -> bool {
        let Some(fading) = self.fading.as_mut() else {
            return true;
        };
        let need = 16_384 * 2;
        while !fading.ended && fading.buf.len() < need {
            match fading.src.rx.try_recv() {
                Ok(Feed::Pcm { epoch, samples }) => {
                    fading
                        .src
                        .queued
                        .fetch_sub(samples.len() / 2, Ordering::Relaxed);
                    if epoch == fading.src.epoch {
                        fading.resampler.process(&samples, &mut fading.buf);
                    }
                }
                Ok(Feed::End { .. }) | Ok(Feed::Failed(_)) | Err(TryRecvError::Disconnected) => {
                    fading.resampler.flush(&mut fading.buf);
                    fading.ended = true;
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => return false,
            }
        }
        true
    }

    /// The chunk of the track fading in, over what is left of the one fading
    /// out: equal power (cosine out, sine in) over the fade.
    fn mix_fading(&mut self) {
        let Some(fading) = self.fading.as_mut() else {
            return;
        };
        let frames = self.chunk.len() / 2;
        let old = fading.buf.len() / 2;
        // how alike they are in this chunk
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..frames.min(old) {
            for c in 0..2 {
                let (x, y) = (self.chunk[2 * i + c] as f64, fading.buf[2 * i + c] as f64);
                xy += x * y;
                xx += x * x;
                yy += y * y;
            }
        }
        if xx > 1e-9 && yy > 1e-9 {
            let rho = (xy / (xx * yy).sqrt()) as f32;
            fading.alike += (rho.clamp(0.0, 1.0) - fading.alike) * 0.3;
        }
        for i in 0..frames {
            let x = (fading.done + i) as f32 / fading.total as f32;
            let (fade_in, fade_out) = fade_gains(x, fading.alike);
            let (l, r) = if i < old {
                (fading.buf[2 * i], fading.buf[2 * i + 1])
            } else {
                (0.0, 0.0)
            };
            self.chunk[2 * i] = self.chunk[2 * i] * fade_in + l * fade_out;
            self.chunk[2 * i + 1] = self.chunk[2 * i + 1] * fade_in + r * fade_out;
        }
        let used = frames.min(old);
        fading.buf.drain(..used * 2);
        fading.done += frames;
        // (to its end, the one fading out silent once it has nothing more:
        // the one fading in rises evenly all the way)
        if fading.done >= fading.total {
            self.fading = None;
        }
    }

    /// The chunk through the output's limiter: holding its peaks where the
    /// sound may go over full scale (resampled, `fading` - two tracks
    /// summed - or the equalizer raising a band), else bit for bit.
    fn limit_output(&mut self, fading: bool) {
        let resampling = self.resampler.from != self.resampler.to;
        let raised = self.eq.raises();
        if !self.out_limiter.active {
            self.out_limiter =
                crate::limiter::Limiter::with_ceiling(self.out_rate, crate::limiter::RESAMPLED_DB);
        }
        self.out_limiter
            .process_holding(&mut self.chunk, resampling || fading || raised);
    }

    /// A file being opened: opened (told, also while nothing plays), or failed.
    fn poll_open(&mut self) {
        let Some(Source::File(f)) = self.current.as_mut() else {
            return;
        };
        if f.info.is_some() {
            return;
        }
        let generation = f.generation;
        match f.rx.try_recv() {
            Ok(Feed::Opened {
                info,
                rate,
                landed_ms,
            }) => {
                f.rate = rate;
                f.info = Some(info.clone());
                self.resampler = Resampler::new(rate.max(1), self.out_rate);
                if f.epoch == 0 {
                    self.shared.base_ms.store(landed_ms, Ordering::Relaxed);
                }
                self.send(Event::Loaded {
                    generation,
                    info: *info,
                });
            }
            Ok(Feed::Failed(message)) => {
                self.current = None;
                self.shared.playing.store(false, Ordering::Relaxed);
                self.send(Event::Failed {
                    generation,
                    message,
                });
            }
            Ok(_) | Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.current = None;
                self.send(Event::Failed {
                    generation,
                    message: "it could not be read".into(),
                });
            }
        }
    }

    /// Push what waits, true when all of it went in.
    fn push_carry(&mut self) -> bool {
        if !self.carry.is_empty() {
            let n = self.shared.push(&self.carry);
            self.carry.drain(..n * 2);
        }
        self.carry.is_empty()
    }

    /// Decode what the output needs next; false when there is nothing to do.
    fn tick(&mut self) -> bool {
        self.poll_open();
        if !self.shared.playing.load(Ordering::Relaxed) {
            // a stream that could not connect says so also when nothing plays
            // (paused while it connected, no output to play on)
            if let Some(Source::Stream(st)) = self.current.as_ref()
                && let Some(message) = st.error.lock().ok().and_then(|mut e| e.take())
            {
                let generation = st.generation;
                self.current = None;
                self.send(Event::Failed {
                    generation,
                    message,
                });
            }
            return false;
        }
        let buffered = self.shared.buffered_frames();
        match self.current {
            None => false,
            Some(Source::File(_)) => self.tick_file(buffered),
            Some(Source::Stream(_)) => self.tick_stream(buffered),
        }
    }

    /// A file's sound from its reading thread into the ring (resampled,
    /// equalized), as far as the output needs it; its end, the next track
    /// after it, its failure.
    fn tick_file(&mut self, _buffered: usize) -> bool {
        let mut moved = false;
        loop {
            if !self.push_carry() {
                return moved; // (the ring is full)
            }
            let Some(Source::File(f)) = self.current.as_mut() else {
                return moved;
            };
            if f.end {
                return self.at_end(moved);
            }
            if self.shared.buffered_frames() >= self.ahead {
                return moved;
            }
            self.start_crossfade();
            if !self.pump_fading() {
                return moved; // (the track fading out is still being read)
            }
            let Some(Source::File(f)) = self.current.as_mut() else {
                return moved;
            };
            let feed = match f.rx.try_recv() {
                Ok(feed) => feed,
                Err(TryRecvError::Empty) => return moved,
                Err(TryRecvError::Disconnected) => Feed::Failed("it could not be read".into()),
            };
            let generation = f.generation;
            match feed {
                Feed::Opened {
                    info,
                    rate,
                    landed_ms,
                } => {
                    f.rate = rate;
                    f.info = Some(info.clone());
                    f.from_ms = landed_ms;
                    f.decoded = 0;
                    self.resampler = Resampler::new(rate.max(1), self.out_rate);
                    if f.epoch == 0 {
                        self.shared.base_ms.store(landed_ms, Ordering::Relaxed);
                    }
                    self.send(Event::Loaded {
                        generation,
                        info: *info,
                    });
                }
                Feed::Seeked { epoch, landed_ms } => {
                    if epoch == f.epoch {
                        self.shared.base_ms.store(landed_ms, Ordering::Relaxed);
                        f.from_ms = landed_ms;
                        f.decoded = 0;
                    }
                }
                Feed::Pcm { epoch, samples } => {
                    f.queued.fetch_sub(samples.len() / 2, Ordering::Relaxed);
                    if epoch != f.epoch {
                        continue; // (read before a seek)
                    }
                    f.decoded += (samples.len() / 2) as u64;
                    // (a crossfade near: the limiter holds already - what
                    // it has held back when the fade begins is held too)
                    let soon = self.crossfade > 0.0
                        && f.rate > 0
                        && f.info.as_ref().and_then(|i| i.duration).is_some_and(|d| {
                            let at = f.from_ms as f64 / 1000.0 + f.decoded as f64 / f.rate as f64;
                            d - at <= self.crossfade as f64 + 0.3
                        });
                    if std::mem::take(&mut self.pending_boundary) {
                        // the queued track's first sound: heard from here
                        let at = self.shared.end_of_buffer();
                        self.shared.boundary.store(at, Ordering::Relaxed);
                        self.shared
                            .mark_track(at + std::mem::take(&mut self.mark_after));
                    }
                    self.chunk.clear();
                    self.resampler.process(&samples, &mut self.chunk);
                    let fading = self.fading.is_some() || soon;
                    self.mix_fading();
                    self.eq.process(&mut self.chunk);
                    self.limit_output(fading);
                    let n = self.shared.push(&self.chunk);
                    self.carry.extend_from_slice(&self.chunk[n * 2..]);
                    moved = true;
                }
                Feed::End { epoch } => {
                    if epoch == f.epoch {
                        f.end = true;
                        // a file shorter than it says (a copy that
                        // stopped: FLAC and MP3 say their length in their
                        // first bytes): its true length, now that its end
                        // is read - the header's 20 s stood over 10 s of
                        // sound (review 20, L5)
                        let at =
                            f.from_ms as f64 / 1000.0 + f.decoded as f64 / f.rate.max(1) as f64;
                        if f.rate > 0
                            && let Some(info) = f.info.as_mut()
                            && info.duration.is_some_and(|d| d - at > 0.5)
                        {
                            info.duration = Some(at);
                            self.send(Event::Length {
                                generation,
                                secs: at,
                            });
                        }
                    }
                }
                Feed::Failed(message) => {
                    self.current = None;
                    self.ending = None;
                    self.fading = None;
                    self.flush_at(0);
                    self.shared.playing.store(false, Ordering::Relaxed);
                    self.send(Event::Failed {
                        generation,
                        message,
                    });
                    return false;
                }
            }
        }
    }

    /// The current file read to its end: the queued one on after it, or
    /// (nothing named while much is still to be heard) wait, or its end.
    fn at_end(&mut self, moved: bool) -> bool {
        let Some(Source::File(f)) = self.current.as_ref() else {
            return moved;
        };
        if !f.finished {
            match self.advance() {
                Some(true) => return true,
                None => return moved, // (still opening)
                Some(false) => {}
            }
            let left = self.carry.len() / 2 + self.shared.buffered_frames();
            if self.queued.is_none() && self.prepared.is_none() && left > self.ahead {
                // the track to follow may still be named (it is named when the
                // one before is heard - a short one is read long before)
                return moved;
            }
            let mut tail = Vec::new();
            self.resampler.flush(&mut tail);
            self.eq.process(&mut tail);
            let resampling = self.resampler.from != self.resampler.to;
            self.out_limiter.process_holding(&mut tail, resampling);
            self.out_limiter.flush(&mut tail);
            self.carry.extend_from_slice(&tail);
            if let Some(Source::File(f)) = self.current.as_mut() {
                f.finished = true;
            }
            return true;
        }
        let drained = self.carry.is_empty();
        self.shared.ending.store(drained, Ordering::Relaxed);
        if drained
            && self.shared.buffered_frames() == 0
            && self.shared.boundary.load(Ordering::Relaxed) == u64::MAX
        {
            let generation = f.generation;
            self.shared.playing.store(false, Ordering::Relaxed);
            self.current = None;
            self.send(Event::Ended { generation });
        }
        moved
    }

    fn tick_stream(&mut self, buffered: usize) -> bool {
        if !self.carry.is_empty() {
            let n = self.shared.push(&self.carry);
            self.carry.drain(..n * 2);
            if !self.carry.is_empty() {
                return false;
            }
        }
        let prime = (self.out_rate as f32 * STREAM_PRIME_SECS) as usize;
        let ahead = self.ahead;
        let Some(Source::Stream(s)) = self.current.as_mut() else {
            return false;
        };
        if !s.primed {
            // a buffer of sound before it plays (and again after it ran dry):
            // the network's jitter is no stutter
            s.gather(prime);
            if s.pending_frames < prime && !s.eof {
                return false;
            }
            s.primed = true;
        }
        if buffered >= ahead {
            return false;
        }
        s.gather(1);
        let Some(mut chunk) = s.pending.pop_front() else {
            if !s.eof {
                if buffered == 0 {
                    s.primed = false; // (the output ran dry: fill up again)
                }
                return false;
            }
            if buffered == 0 {
                let generation = s.generation;
                let error = s.error.lock().ok().and_then(|mut e| e.take());
                self.shared.playing.store(false, Ordering::Relaxed);
                self.current = None;
                self.send(match error {
                    Some(message) => Event::Failed {
                        generation,
                        message,
                    },
                    None => Event::Ended { generation },
                });
            }
            return false;
        };
        s.pending_frames -= chunk.len() / 2;
        self.eq.process(&mut chunk);
        // (a band raised: its peaks held below full scale, as a file's -
        // the equalizer takes no level off for its boosts)
        let raised = self.eq.raises();
        if raised || self.out_limiter.active {
            if !self.out_limiter.active {
                self.out_limiter = crate::limiter::Limiter::with_ceiling(
                    self.out_rate,
                    crate::limiter::RESAMPLED_DB,
                );
            }
            self.out_limiter.process_holding(&mut chunk, raised);
        }
        let n = self.shared.push(&chunk);
        self.carry.extend_from_slice(&chunk[n * 2..]);
        true
    }
}

fn run(
    rx: Receiver<Command>,
    events: Sender<Event>,
    shared: Arc<Shared>,
    wake: Wake,
    headless: bool,
) {
    let mut player = Player {
        shared,
        events,
        wake,
        headless,
        stream: None,
        out_rate: 48_000,
        ahead: (48_000.0 * AHEAD_SECS) as usize,
        eq: Equalizer::new(48_000),
        gains: ([0.0; 10], true),
        loudness: Loudness::default(),
        current: None,
        queued: None,
        device: None,
        losses: Vec::new(),
        ending: None,
        crossfade: 0.0,
        fade_on_skip: false,
        fading: None,
        prepared: None,
        resampler: Resampler::new(48_000, 48_000),
        out_limiter: crate::limiter::Limiter::with_ceiling(48_000, crate::limiter::RESAMPLED_DB),
        pending_boundary: false,
        mark_after: 0,
        carry: Vec::new(),
        output_ok: false,
        output_tried: Instant::now(),
        chunk: Vec::with_capacity(16_384),
    };
    player.connect(None);
    player.eq = Equalizer::new(player.out_rate);
    let mut busy = false;
    loop {
        let command = if busy {
            match rx.try_recv() {
                Ok(c) => Some(c),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            // (a stream's sound arrives on its own: looked for often)
            let wait = if matches!(player.current, Some(Source::Stream(_))) {
                5
            } else {
                15
            };
            match rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        if let Some(command) = command
            && !player.command(command)
        {
            return;
        }
        player.recover();
        // no output: tried again now and then (a sound server that starts, a device plugged in)
        if !player.output_ok && player.output_tried.elapsed() >= OUTPUT_RETRY {
            let device = player.device.clone();
            player.reopen(device);
        }
        player.check_boundary();
        busy = player.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fade_keeps_the_sums_level_for_tracks_alike_and_unlike() {
        // unrelated (rho 0): equal power; alike (rho 1): equal amplitude
        // (equal power rose 3 dB in the middle for them); between: the
        // sum's power a^2 + b^2 + 2 rho a b stays 1
        for rho in [0.0f32, 0.3, 0.7, 1.0] {
            for k in 0..=20 {
                let (a, b) = fade_gains(k as f32 / 20.0, rho);
                let power = a * a + b * b + 2.0 * rho * a * b;
                assert!((power - 1.0).abs() < 1e-4, "rho {rho} at {k}: {power}");
            }
        }
        let (a, b) = fade_gains(0.5, 1.0);
        assert!((a + b - 1.0).abs() < 1e-5, "alike: amplitudes add to one");
        let (a, b) = fade_gains(0.5, 0.0);
        assert!((a - b).abs() < 1e-5 && (a * a + b * b - 1.0).abs() < 1e-5);
    }

    /// A PNG that says it is 11000 x 11000 (its header alone): refused by
    /// the limits before anything is allocated for it.
    #[test]
    fn a_huge_cover_is_refused_before_it_is_decoded() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8], data: &[u8]| {
            png.extend((data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend(data);
            png.extend(&body);
            // (the CRC-32 of the type and data)
            let mut crc = 0xffff_ffffu32;
            for b in &body {
                crc ^= *b as u32;
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        (crc >> 1) ^ 0xedb8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            png.extend((!crc).to_be_bytes());
        };
        let mut ihdr = Vec::new();
        ihdr.extend(11_000u32.to_be_bytes());
        ihdr.extend(11_000u32.to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        chunk(b"IEND", &[]);
        match decode_cover(&png) {
            Err(image::ImageError::Limits(_)) => {}
            other => panic!("not refused by the limits: {:?}", other.map(|i| i.width())),
        }
        let mut info = TrackInfo {
            cover: Some(png),
            ..Default::default()
        };
        make_art(&mut info);
        assert!(info.art.is_none());
    }

    #[test]
    fn a_large_cover_is_kept_large_for_the_picture() {
        // a 1500 px cover: 256 px for the list and the colours, 1024 px for
        // the cover as the picture (it was stretched from 256: blurred)
        let image = image::RgbaImage::from_fn(1500, 1500, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut info = TrackInfo {
            cover: Some(png),
            ..Default::default()
        };
        make_art(&mut info);
        let art = info.art.expect("art");
        assert_eq!((art.width, art.height), (256, 256));
        let (w, h, rgba) = art.large.as_ref().expect("a large one");
        assert_eq!((*w, *h, rgba.len()), (1024, 1024, 1024 * 1024 * 4));
        // a small cover: no large copy of it
        let image = image::RgbaImage::from_pixel(200, 200, image::Rgba([1, 2, 3, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut info = TrackInfo {
            cover: Some(png),
            ..Default::default()
        };
        make_art(&mut info);
        assert!(info.art.expect("art").large.is_none());
    }

    fn tone(hz: f64, rate: u32, frames: usize) -> Vec<f32> {
        // (the phase in f64: f32's grows too coarse over a second at 96 kHz)
        (0..frames)
            .flat_map(|i| {
                let v = (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin() as f32;
                [v, v]
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    fn resample(from: u32, to: u32, input: &[f32]) -> Vec<f32> {
        let mut r = Resampler::new(from, to);
        let mut out = Vec::new();
        for chunk in input.chunks(1024) {
            r.process(chunk, &mut out);
        }
        r.flush(&mut out);
        out
    }

    #[test]
    fn the_resampler_keeps_the_length_ratio_and_a_tone() {
        let out = resample(44_100, 48_000, &tone(440.0, 44_100, 44_100));
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 2, "{frames}");
        // the tone stays a tone: about the same RMS (away from the edges)
        let level = rms(&out[4000..90_000]);
        assert!((level - 0.707).abs() < 0.01, "{level}");
    }

    #[test]
    fn the_resampler_does_not_delay_the_sound() {
        // a click at frame 1000 of 44.1 kHz lands at 1000 * 48000 / 44100
        let mut input = vec![0.0f32; 4410 * 2];
        input[2000] = 1.0;
        input[2001] = 1.0;
        let out = resample(44_100, 48_000, &input);
        let peak = out
            .chunks(2)
            .enumerate()
            .max_by(|a, b| a.1[0].total_cmp(&b.1[0]))
            .unwrap()
            .0;
        assert!((peak as i64 - 1088).abs() <= 1, "{peak}");
    }

    #[test]
    fn downsampling_does_not_alias() {
        // 30 kHz at 96 kHz is above 48 kHz's Nyquist: cubic interpolation
        // folded it to 18 kHz; a band-limited resampler takes it away
        let out = resample(96_000, 48_000, &tone(30_000.0, 96_000, 96_000));
        let level = rms(&out[2000..out.len() - 2000]);
        assert!(20.0 * level.log10() < -60.0, "{} dB", 20.0 * level.log10());
        // and keeps what is below it: 18 kHz passes
        let out = resample(96_000, 48_000, &tone(18_000.0, 96_000, 96_000));
        let level = rms(&out[2000..out.len() - 2000]);
        assert!((level - 0.707).abs() < 0.03, "{level}");
        // 440 Hz down by two keeps its pitch
        let out = resample(96_000, 48_000, &tone(440.0, 96_000, 96_000));
        assert!((out.len() as i64 / 2 - 48_000).abs() <= 2);
    }

    #[test]
    fn a_downmix_puts_every_channel_where_it_belongs() {
        let m = downmix_matrix(&Channels::Discrete(6)); // 5.1 by WAVE's order
        assert_eq!(m.len(), 6);
        let (fl, fr, fc, lfe, rl, rr) = (m[0], m[1], m[2], m[3], m[4], m[5]);
        assert!(fl.0 > 0.0 && fl.1 == 0.0, "front left only left");
        assert!(fr.1 > 0.0 && fr.0 == 0.0, "front right only right");
        assert!(fc.0 > 0.0 && (fc.0 - fc.1).abs() < 1e-6, "centre on both");
        assert_eq!(lfe, (0.0, 0.0), "the LFE is left out");
        assert!(rl.0 > 0.0 && rl.1 == 0.0 && rr.1 > 0.0 && rr.0 == 0.0);
        // the loudest side sums to 1: nothing clips
        let left: f32 = m.iter().map(|c| c.0).sum();
        assert!((left - 1.0).abs() < 1e-5, "{left}");
        // mono plays on both sides at its level; stereo stays as it is
        assert_eq!(downmix_matrix(&Channels::Discrete(1)), vec![(1.0, 1.0)]);
        let stereo = Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT);
        assert_eq!(downmix_matrix(&stereo), vec![(1.0, 0.0), (0.0, 1.0)]);
        let mut out = Vec::new();
        downmix(&[0.5, -0.5], &downmix_matrix(&stereo), &mut out);
        assert_eq!(out, vec![0.5, -0.5]);
    }

    #[test]
    fn running_dry_fades_and_loud_sound_bends() {
        // a stall: the ring holds 1000 frames of a steady 0.5 and no more
        let shared = Shared::new();
        shared.playing.store(true, Ordering::Relaxed);
        shared.set_volume(1.0);
        shared.push(&[0.5f32; 2000]);
        let mut state = RenderState {
            gain: 1.0,
            fade: 1.0,
            flushes: 0,
            silent: 0,
        };
        let mut out = vec![0.0f32; 1500 * 2];
        shared.render(&mut out, 2, &mut state);
        let worst = out
            .chunks(2)
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.02, "a cut of {worst} where the sound ran out");
        // what is within full scale goes out as it is
        assert_eq!(clip(0.95), 0.95);
        assert_eq!(clip(-1.5), -1.0);
    }

    #[test]
    fn the_position_and_the_pictures_wait_for_the_output() {
        let shared = Shared::new();
        shared.playing.store(true, Ordering::Relaxed);
        let samples: Vec<f32> = (0..9600).flat_map(|i| [i as f32, 0.0]).collect();
        shared.push(&samples);
        let mut state = RenderState::default();
        let mut out = vec![0.0f32; 4800 * 2];
        shared.render(&mut out, 2, &mut state);
        assert!((shared.position() - 0.1).abs() < 1e-9);
        // 50 ms on their way to the speakers
        shared.set_latency(2400);
        assert!((shared.position() - 0.05).abs() < 1e-9);
        let mut tap = Vec::new();
        shared.tap_into(&mut tap);
        assert_eq!(
            tap[tap.len() - 2],
            2399.0,
            "the frame heard now, not the last read"
        );
        // a device half a second behind: while paused the last of the sound
        // plays on out of it, the position following it, then standing
        let shared = Shared::new();
        shared.playing.store(true, Ordering::Relaxed);
        let samples: Vec<f32> = (0..96_000)
            .flat_map(|i| [(i % 100) as f32 / 200.0 + 0.1, 0.0])
            .collect();
        shared.push(&samples);
        let mut state = RenderState::default();
        let mut out = vec![0.0f32; 480 * 2];
        for _ in 0..100 {
            shared.render_at(&mut out, 2, &mut state, 24_000 + 480);
        }
        let at_pause = shared.position();
        assert!(
            (at_pause - (48_000.0 - 24_480.0) / 48_000.0).abs() < 0.001,
            "{at_pause}"
        );
        shared.playing.store(false, Ordering::Relaxed);
        for _ in 0..25 {
            shared.render_at(&mut out, 2, &mut state, 24_000 + 480);
        }
        let mid = shared.position();
        assert!(mid > at_pause + 0.2 && mid < at_pause + 0.4, "{mid}");
        for _ in 0..100 {
            shared.render_at(&mut out, 2, &mut state, 24_000 + 480);
        }
        let end = shared.position();
        let read = shared.ring.read() as f64 / 48_000.0;
        assert!(
            (end - read).abs() < 0.001,
            "{end} vs everything read {read}"
        );
    }

    #[test]
    fn one_output_channel_hears_both_sides() {
        let shared = Shared::new();
        shared.playing.store(true, Ordering::Relaxed);
        shared.set_volume(1.0);
        shared.ending.store(true, Ordering::Relaxed); // (no stall: the end)
        shared.push(&[0.5, 0.25, 0.5, 0.25]);
        let mut state = RenderState {
            gain: 1.0,
            fade: 1.0,
            flushes: 0,
            silent: 0,
        };
        let mut out = [9.0f32; 3];
        shared.render(&mut out, 1, &mut state);
        assert_eq!(out, [0.375, 0.375, 0.0]);
        // four channels: the stereo on the first two, silence on the others
        shared.push(&[0.5, 0.25]);
        state.fade = 1.0; // (it ran dry above: what comes now would fade in)
        let mut out = [9.0f32; 4];
        shared.render(&mut out, 4, &mut state);
        assert_eq!(out, [0.5, 0.25, 0.0, 0.0]);
    }

    #[test]
    fn replay_gain_levels_and_never_clips() {
        let info = TrackInfo {
            track_gain: Some(-6.0),
            album_gain: Some(-3.0),
            track_peak: Some(0.5),
            ..Default::default()
        };
        let db = |f: f32| 20.0 * f.log10();
        let off = Loudness::default();
        assert_eq!(off.factor(&info), 1.0);
        let track = Loudness {
            mode: GainMode::Track,
            preamp_db: 0.0,
            ..Default::default()
        };
        assert!((db(track.factor(&info)) + 6.0).abs() < 0.01);
        let album = Loudness {
            mode: GainMode::Album,
            preamp_db: 0.0,
            ..Default::default()
        };
        assert!((db(album.factor(&info)) + 3.0).abs() < 0.01);
        // +9 dB in all would take a 0.5 peak past full scale: held at 2
        let loud = Loudness {
            mode: GainMode::Track,
            preamp_db: 15.0,
            ..Default::default()
        };
        assert!((loud.factor(&info) - 2.0).abs() < 1e-4);
        // a track without tags: the gain for those (by default -6 dB)
        let untagged = track.factor(&TrackInfo::default());
        assert!(
            (db(untagged) - DEFAULT_UNTAGGED_DB).abs() < 0.01,
            "{}",
            db(untagged)
        );
        assert_eq!(gain_value("-6.54 dB"), Some(-6.54));
        assert_eq!(gain_value("+1.2dB"), Some(1.2));
        assert_eq!(gain_value("0.988"), Some(0.988));
        assert_eq!(gain_value("loud"), None);
    }

    #[test]
    fn a_wav_file_decodes_seeks_and_ends() {
        let dir = crate::testdir::Scratch::new("wav");
        let path = dir.join("x.wav");
        write_wav(&path, 22_050, 1, 22_050 * 2); // two seconds of mono
        let (mut d, info) =
            open(path.to_str().unwrap(), 44_100, true, Loudness::default()).unwrap();
        assert!(info.seekable && info.rate == 22_050 && info.channels == 1);
        assert!((info.duration.unwrap() - 2.0).abs() < 0.01);
        let mut out = Vec::new();
        while d.step(&mut out).unwrap() {}
        d.finish(&mut out);
        let frames = out.len() / 2;
        assert!((frames as i64 - 88_200).abs() <= 2, "{frames}");
        let at = d.seek(1.5).unwrap();
        assert!((at as i64 - 1500).abs() < 50, "{at}");
        assert_eq!(
            open("/nonexistent.mp3", 44_100, true, Loudness::default())
                .err()
                .as_deref(),
            Some("no such file")
        );
        assert_eq!(info.codec, "PCM 16-bit");
        let empty = path.with_extension("mp3");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(
            open(empty.to_str().unwrap(), 44_100, true, Loudness::default())
                .err()
                .as_deref(),
            Some("the file is empty")
        );
        assert_eq!(codec_name("pcm_f32le"), "PCM 32-bit float");
        assert_eq!(codec_name("vorbis"), "Vorbis");
    }

    pub fn write_wav(path: &Path, rate: u32, channels: u16, frames: u32) {
        use std::io::Write;
        let data_len = frames * channels as u32 * 2;
        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * channels as u32 * 2).to_le_bytes())
            .unwrap();
        f.write_all(&(channels * 2).to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        for i in 0..frames {
            let v = ((2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 12_000.0)
                as i16;
            for _ in 0..channels {
                f.write_all(&v.to_le_bytes()).unwrap();
            }
        }
    }
}
