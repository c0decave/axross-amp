//! The audio engine: a decoder thread (symphonia: MP3, FLAC, Vorbis, AAC,
//! ALAC, WAV / AIFF / CAF, MP4 / MKV containers) that turns a file or an
//! http(s) stream into stereo samples at the output's rate (resampled,
//! equalized) and a cpal output stream that plays them, with volume, mute
//! and the samples the visualizations read.
//!
//! The UI talks to it with [`Command`]s and hears back [`Event`]s; the
//! position, volume and the recent samples are in [`Shared`] (atomics and
//! short locks - the audio callback never waits: a lock it cannot take at
//! once is a buffer of silence).

use crate::eq::Equalizer;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::{MetadataOptions, StandardTag, StandardVisualKey};
use symphonia::core::units::{Time, TimeBase, Timestamp};

/// Seconds of sound decoded ahead of the output.
const AHEAD_SECS: f32 = 0.35;
/// Stereo frames of the most recent output the visualizations may read.
pub const TAP_FRAMES: usize = 8192;
/// The longest a stream's server may take to answer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Command {
    /// Open `path` (a file or an http(s) URL) at `start_ms`, playing or not.
    Load {
        path: String,
        start_ms: u64,
        play: bool,
        generation: u64,
    },
    Play,
    Pause,
    Stop,
    Seek(f64),
    Equalizer {
        gains: [f32; 10],
        enabled: bool,
    },
    Quit,
}

#[derive(Clone, Debug, Default)]
pub struct TrackInfo {
    pub duration: Option<f64>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<Vec<u8>>,
    pub seekable: bool,
    pub rate: u32,
    pub channels: usize,
    pub codec: String,
}

pub enum Event {
    Loaded {
        generation: u64,
        info: TrackInfo,
    },
    Ended {
        generation: u64,
    },
    Failed {
        generation: u64,
        message: String,
    },
    /// The output device in use (or why there is no sound).
    Output(String),
}

/// What the UI reads while the engine runs.
pub struct Shared {
    buffer: Mutex<VecDeque<f32>>,
    played_frames: AtomicU64,
    base_ms: AtomicU64,
    pub playing: AtomicBool,
    volume_bits: AtomicU32,
    pub muted: AtomicBool,
    pub device_rate: AtomicU32,
    pub tap: Mutex<VecDeque<f32>>,
}

impl Shared {
    fn new() -> Self {
        Shared {
            buffer: Mutex::new(VecDeque::with_capacity(48_000)),
            played_frames: AtomicU64::new(0),
            base_ms: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            volume_bits: AtomicU32::new(0.8f32.to_bits()),
            muted: AtomicBool::new(false),
            device_rate: AtomicU32::new(48_000),
            tap: Mutex::new(VecDeque::with_capacity(TAP_FRAMES * 2)),
        }
    }

    /// The position of the sound heard (seconds).
    pub fn position(&self) -> f64 {
        let rate = self.device_rate.load(Ordering::Relaxed).max(1) as f64;
        self.base_ms.load(Ordering::Relaxed) as f64 / 1000.0
            + self.played_frames.load(Ordering::Relaxed) as f64 / rate
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    /// Drop what is decoded and not yet heard; the position starts at `ms`.
    fn restart_at(&self, ms: u64) {
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.clear();
        }
        self.base_ms.store(ms, Ordering::Relaxed);
        self.played_frames.store(0, Ordering::Relaxed);
    }

    fn buffered_frames(&self) -> usize {
        self.buffer.lock().map(|b| b.len() / 2).unwrap_or(0)
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
    pub fn start(wake: impl Fn() + Send + 'static) -> Engine {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (etx, events) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared::new());
        let thread_shared = shared.clone();
        thread::Builder::new()
            .name("amp-decoder".into())
            .spawn(move || run(rx, etx, thread_shared, Box::new(wake)))
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

/// The default output (PulseAudio / PipeWire first on Linux, else the
/// system's default host), as (stream, rate, channels).
fn open_output(shared: Arc<Shared>) -> Result<(cpal::Stream, u32, String), String> {
    let mut hosts: Vec<cpal::Host> = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(host) = cpal::host_from_id(cpal::HostId::PulseAudio) {
        hosts.push(host);
    }
    hosts.push(cpal::default_host());
    let mut last = String::from("no audio output");
    for host in hosts {
        let Some(device) = host.default_output_device() else {
            continue;
        };
        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let name = format!("{:?}", host.id());
        let rate = config.sample_rate();
        let result = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, config.into(), shared.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, config.into(), shared.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, config.into(), shared.clone()),
            cpal::SampleFormat::I32 => build::<i32>(&device, config.into(), shared.clone()),
            other => Err(format!("sample format {other:?} not supported")),
        };
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
    let channels = config.channels as usize;
    let mut gain = 0.0f32; // the volume reached (ramped: no click on a change)
    let mut frame = Vec::<f32>::with_capacity(4096);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels.max(1);
                frame.clear();
                let playing = shared.playing.load(Ordering::Relaxed);
                if playing && let Ok(mut buffer) = shared.buffer.try_lock() {
                    let take = (frames * 2).min(buffer.len());
                    frame.extend(buffer.drain(..take));
                }
                let heard = frame.len() / 2;
                frame.resize(frames * 2, 0.0);
                let target = if shared.muted.load(Ordering::Relaxed) {
                    0.0
                } else {
                    shared.volume()
                };
                let step = (target - gain) / frames.max(1) as f32;
                for (i, out) in data.chunks_mut(channels.max(1)).enumerate() {
                    gain += step;
                    let (l, r) = (frame[i * 2] * gain, frame[i * 2 + 1] * gain);
                    for (c, sample) in out.iter_mut().enumerate() {
                        let v = match c {
                            0 => l,
                            1 => r,
                            _ => 0.0,
                        };
                        *sample = T::from_sample(v.clamp(-1.0, 1.0));
                    }
                }
                gain = target;
                shared
                    .played_frames
                    .fetch_add(heard as u64, Ordering::Relaxed);
                if heard > 0
                    && let Ok(mut tap) = shared.tap.try_lock()
                {
                    tap.extend(frame[..heard * 2].iter().copied());
                    let excess = tap.len().saturating_sub(TAP_FRAMES * 2);
                    tap.drain(..excess);
                }
            },
            |err| eprintln!("axross-amp: output: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}

// -- resampling --------------------------------------------------------------------------------

/// Stereo resampling by cubic (Catmull-Rom) interpolation.
struct Resampler {
    step: f64,
    pos: f64,
    hist: Vec<[f32; 2]>,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        Resampler {
            step: from as f64 / to as f64,
            pos: 1.0,
            hist: vec![[0.0; 2]],
        }
    }

    fn bypass(&self) -> bool {
        (self.step - 1.0).abs() < 1e-9
    }

    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.bypass() {
            out.extend_from_slice(input);
            return;
        }
        self.hist
            .extend(input.as_chunks::<2>().0.iter().map(|f| [f[0], f[1]]));
        while self.pos + 2.0 < self.hist.len() as f64 {
            let i = self.pos.floor() as usize;
            let t = (self.pos - i as f64) as f32;
            for c in 0..2 {
                let (y0, y1, y2, y3) = (
                    self.hist[i - 1][c],
                    self.hist[i][c],
                    self.hist[i + 1][c],
                    self.hist[i + 2][c],
                );
                let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
                let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                let c1 = -0.5 * y0 + 0.5 * y2;
                out.push(((a * t + b) * t + c1) * t + y1);
            }
            self.pos += self.step;
        }
        let drop = (self.pos.floor() as usize).saturating_sub(1);
        self.hist.drain(..drop);
        self.pos -= drop as f64;
    }
}

// -- decoding ----------------------------------------------------------------------------------

struct Decoding {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    resampler: Resampler,
    eof: bool,
    generation: u64,
    seekable: bool,
    scratch: Vec<f32>,
    stereo: Vec<f32>,
    resampled: Vec<f32>,
    /// The source's rate, the frames still to drop before the sound (encoder
    /// delay, or the part of a packet before a seek's target), the frames
    /// still to play (an edit list's length: the encoder's padding is not)
    /// and where the sound begins in the media (frames).
    rate: u32,
    skip: u64,
    left: Option<u64>,
    edit: Option<crate::mp4edit::Edit>,
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
            "audio/ogg" | "application/ogg" | "audio/vorbis" => Some("ogg"),
            "audio/flac" | "audio/x-flac" => Some("flac"),
            "audio/wav" | "audio/x-wav" => Some("wav"),
            _ => None,
        };
        if let Some(ext) = ext {
            hint.with_extension(ext);
        }
    }
    hint
}

/// A media source for `path`: the file, or the body of an http(s) answer.
fn open_source(path: &str) -> Result<(MediaSourceStream<'static>, Hint, bool), String> {
    if crate::playlist::is_stream(path) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(CONNECT_TIMEOUT))
            .build()
            .into();
        let response = agent
            .get(path)
            .header("Icy-MetaData", "0")
            .call()
            .map_err(|e| e.to_string())?;
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let reader = response.into_body().into_reader();
        let source = ReadOnlySource::new(reader);
        let mss = MediaSourceStream::new(Box::new(source), Default::default());
        return Ok((mss, hint_for(path, content_type.as_deref()), false));
    }
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    Ok((
        MediaSourceStream::new(Box::new(file), Default::default()),
        hint_for(path, None),
        true,
    ))
}

fn tag_text(tags: &[symphonia::core::meta::Tag], info: &mut TrackInfo) {
    for tag in tags {
        match &tag.std {
            Some(StandardTag::TrackTitle(v)) if info.title.is_empty() => info.title = v.to_string(),
            Some(StandardTag::Artist(v)) if info.artist.is_empty() => info.artist = v.to_string(),
            Some(StandardTag::Album(v)) if info.album.is_empty() => info.album = v.to_string(),
            _ => {}
        }
    }
}

fn open(path: &str, out_rate: u32, generation: u64) -> Result<(Decoding, TrackInfo), String> {
    let (mss, hint, seekable) = open_source(path)?;
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("not a supported audio file ({e})"))?;
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
    let decoder = codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("cannot decode it ({e})"))?;
    let rate = params.sample_rate.unwrap_or(44_100);
    let mut info = TrackInfo {
        rate,
        channels: params.channels.as_ref().map(|c| c.count()).unwrap_or(2),
        seekable,
        codec: codecs()
            .get_audio_decoder(params.codec)
            .map(|d| d.codec.info.short_name.to_uppercase())
            .unwrap_or_else(|| "audio".into()),
        ..Default::default()
    };
    if let (Some(tb), Some(frames)) = (track.time_base, track.num_frames)
        && let Ok(ts) = Timestamp::try_from(frames)
    {
        info.duration = tb.calc_time(ts).map(|t| t.as_secs_f64());
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
        crate::mp4edit::read_edit(Path::new(path))
    } else {
        None
    };
    if let Some(length) = edit.and_then(|e| e.length) {
        info.duration = Some(length as f64 / rate as f64);
    }
    let decoding = Decoding {
        format,
        decoder,
        track_id: track.id,
        time_base: track.time_base,
        resampler: Resampler::new(rate, out_rate),
        eof: false,
        generation,
        seekable,
        scratch: Vec::new(),
        stereo: Vec::new(),
        resampled: Vec::new(),
        rate,
        skip: edit.map(|e| e.skip).unwrap_or(0),
        left: edit.and_then(|e| e.length),
        edit,
    };
    Ok((decoding, info))
}

impl Decoding {
    /// Seek to `secs`; returns where it landed (ms).
    fn seek(&mut self, secs: f64) -> Option<u64> {
        if !self.seekable {
            return None;
        }
        let secs = secs.max(0.0);
        // in the media, the sound begins after the edit's delay
        let delay = self.edit.map(|e| e.skip).unwrap_or(0);
        let media = secs + delay as f64 / self.rate.max(1) as f64;
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
        self.eof = false;
        // An accurate seek lands at the start of the packet holding the time
        // asked for: the frames before it are dropped (the sound began about
        // a packet early).
        let to_secs = |ts| {
            self.time_base
                .and_then(|tb| tb.calc_time(ts))
                .map(|t| t.as_secs_f64())
        };
        let (required, actual) = (to_secs(seeked.required_ts), to_secs(seeked.actual_ts));
        self.skip = match (required, actual) {
            (Some(r), Some(a)) if r > a => ((r - a) * self.rate as f64).round() as u64,
            _ => 0,
        };
        let played = (secs * self.rate as f64) as u64;
        self.left = self
            .edit
            .and_then(|e| e.length)
            .map(|l| l.saturating_sub(played));
        let landed = required
            .map(|r| r - delay as f64 / self.rate.max(1) as f64)
            .unwrap_or(secs);
        Some((landed.max(0.0) * 1000.0).round() as u64)
    }

    /// Decode the next packet into `out` (stereo at the output's rate,
    /// equalized). False at the end.
    fn step(&mut self, eq: &mut Equalizer, out: &mut Vec<f32>) -> Result<bool, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(SymError::IoError(_)) => return Ok(false), // (a stream that ended)
                Err(SymError::ResetRequired) => return Ok(false),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    to_stereo(&buffer, &mut self.scratch, &mut self.stereo);
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
                    if self.stereo.is_empty() {
                        continue;
                    }
                    self.resampled.clear();
                    self.resampler.process(&self.stereo, &mut self.resampled);
                    eq.process(&mut self.resampled);
                    out.extend_from_slice(&self.resampled);
                    return Ok(true);
                }
                Err(SymError::DecodeError(_)) => continue, // (a damaged packet: the next one)
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

/// Decode all of `path` as the player would (stereo at `rate`): the
/// frames made and the track's info. For tests and measurements.
pub fn decode_file(path: &str, rate: u32) -> Result<(usize, TrackInfo), String> {
    let (mut decoding, info) = open(path, rate, 0)?;
    let mut eq = Equalizer::new(rate);
    let mut out = Vec::new();
    let mut frames = 0;
    while decoding.step(&mut eq, &mut out)? {
        frames += out.len() / 2;
        out.clear();
    }
    Ok((frames, info))
}

/// symphonia's decoders and Opus (libopus, built in: symphonia has no Opus
/// decoder of its own; Ogg Opus and WebM / MKV Opus).
fn codecs() -> &'static symphonia::core::codecs::registry::CodecRegistry {
    static REGISTRY: std::sync::OnceLock<symphonia::core::codecs::registry::CodecRegistry> =
        std::sync::OnceLock::new();
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

/// Any channel layout as stereo: mono doubled, more than two folded into
/// left and right.
fn to_stereo(buffer: &GenericAudioBufferRef<'_>, scratch: &mut Vec<f32>, stereo: &mut Vec<f32>) {
    let channels = buffer.spec().channels().count().max(1);
    scratch.resize(buffer.samples_interleaved(), 0.0);
    buffer.copy_to_slice_interleaved(&mut scratch[..]);
    stereo.clear();
    match channels {
        1 => stereo.extend(scratch.iter().flat_map(|s| [*s, *s])),
        2 => stereo.extend_from_slice(scratch),
        n => {
            for frame in scratch.chunks_exact(n) {
                let (mut l, mut r) = (frame[0], frame[1]);
                for (i, s) in frame.iter().enumerate().skip(2) {
                    if i % 2 == 0 {
                        l += s * 0.5
                    } else {
                        r += s * 0.5
                    }
                }
                stereo.push(l * 0.7);
                stereo.push(r * 0.7);
            }
        }
    }
}

// -- the thread --------------------------------------------------------------------------------

fn run(
    rx: Receiver<Command>,
    events: Sender<Event>,
    shared: Arc<Shared>,
    wake: Box<dyn Fn() + Send>,
) {
    let send = |event: Event| {
        let _ = events.send(event);
        wake();
    };
    let output = open_output(shared.clone());
    let (_stream, out_rate) = match output {
        Ok((stream, rate, name)) => {
            shared.device_rate.store(rate, Ordering::Relaxed);
            send(Event::Output(name));
            (Some(stream), rate)
        }
        Err(why) => {
            send(Event::Output(format!("no sound: {why}")));
            (None, 48_000)
        }
    };
    let mut eq = Equalizer::new(out_rate);
    let mut current: Option<Decoding> = None;
    let mut chunk = Vec::<f32>::with_capacity(16_384);
    let ahead = (out_rate as f32 * AHEAD_SECS) as usize;
    loop {
        let busy =
            current.as_ref().is_some_and(|d| !d.eof) && shared.playing.load(Ordering::Relaxed);
        let command = if busy && shared.buffered_frames() < ahead {
            rx.try_recv().ok()
        } else {
            match rx.recv_timeout(Duration::from_millis(15)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        match command {
            Some(Command::Quit) => return,
            Some(Command::Load {
                path,
                start_ms,
                play,
                generation,
            }) => {
                shared.playing.store(false, Ordering::Relaxed);
                shared.restart_at(0);
                eq.reset();
                current = None;
                match open(&path, out_rate, generation) {
                    Ok((mut decoding, info)) => {
                        let at = if start_ms > 0 {
                            decoding.seek(start_ms as f64 / 1000.0).unwrap_or(0)
                        } else {
                            0
                        };
                        shared.restart_at(at);
                        send(Event::Loaded { generation, info });
                        shared.playing.store(play, Ordering::Relaxed);
                        current = Some(decoding);
                    }
                    Err(message) => send(Event::Failed {
                        generation,
                        message,
                    }),
                }
            }
            Some(Command::Play) => shared.playing.store(current.is_some(), Ordering::Relaxed),
            Some(Command::Pause) => shared.playing.store(false, Ordering::Relaxed),
            Some(Command::Stop) => {
                shared.playing.store(false, Ordering::Relaxed);
                if let Some(d) = current.as_mut() {
                    let at = d.seek(0.0).unwrap_or(0);
                    shared.restart_at(at);
                    eq.reset();
                }
            }
            Some(Command::Seek(secs)) => {
                if let Some(d) = current.as_mut()
                    && let Some(at) = d.seek(secs)
                {
                    shared.restart_at(at);
                    eq.reset();
                }
            }
            Some(Command::Equalizer { gains, enabled }) => {
                eq.set_gains(gains);
                eq.enabled = enabled;
            }
            None => {}
        }
        let Some(decoding) = current.as_mut() else {
            continue;
        };
        if !shared.playing.load(Ordering::Relaxed) {
            continue;
        }
        if decoding.eof {
            if shared.buffered_frames() == 0 {
                let generation = decoding.generation;
                shared.playing.store(false, Ordering::Relaxed);
                current = None;
                send(Event::Ended { generation });
            }
            continue;
        }
        if shared.buffered_frames() >= ahead {
            continue;
        }
        chunk.clear();
        match decoding.step(&mut eq, &mut chunk) {
            Ok(true) => {
                if let Ok(mut buffer) = shared.buffer.lock() {
                    buffer.extend(chunk.iter().copied());
                }
            }
            Ok(false) => decoding.eof = true,
            Err(message) => {
                let generation = decoding.generation;
                current = None;
                shared.playing.store(false, Ordering::Relaxed);
                send(Event::Failed {
                    generation,
                    message,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resampler_keeps_the_length_ratio_and_a_tone() {
        let mut r = Resampler::new(44_100, 48_000);
        let input: Vec<f32> = (0..44_100)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44_100.0).sin();
                [v, v]
            })
            .collect();
        let mut out = Vec::new();
        for chunk in input.chunks(1024) {
            r.process(chunk, &mut out);
        }
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() < 8, "{frames}");
        // the tone stays a tone: about the same RMS
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        assert!((rms - 0.707).abs() < 0.02, "{rms}");
    }

    #[test]
    fn a_wav_file_decodes_seeks_and_ends() {
        let path = std::env::temp_dir().join(format!("amp-{}.wav", std::process::id()));
        write_wav(&path, 22_050, 1, 22_050 * 2); // two seconds of mono
        let (mut d, info) = open(path.to_str().unwrap(), 44_100, 1).unwrap();
        assert!(info.seekable && info.rate == 22_050 && info.channels == 1);
        assert!((info.duration.unwrap() - 2.0).abs() < 0.01);
        let mut eq = Equalizer::new(44_100);
        let mut out = Vec::new();
        while d.step(&mut eq, &mut out).unwrap() {}
        let frames = out.len() / 2;
        assert!((frames as i64 - 88_200).abs() < 2048, "{frames}");
        let at = d.seek(1.5).unwrap();
        assert!((at as i64 - 1500).abs() < 50, "{at}");
        assert!(open("/nonexistent.mp3", 44_100, 2).is_err());
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

#[cfg(test)]
mod resample_more {
    use super::Resampler;

    fn peak_hz(out: &[f32], rate: f32) -> f32 {
        // zero crossings of the left channel
        let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        let crossings = left
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        crossings as f32 * rate / left.len() as f32
    }

    #[test]
    fn downsampling_by_two_keeps_the_tone() {
        let mut r = Resampler::new(96_000, 48_000);
        let input: Vec<f32> = (0..96_000)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * 440.0 * i as f32 / 96_000.0).sin();
                [v, v]
            })
            .collect();
        let mut out = Vec::new();
        for c in input.chunks(4608) {
            r.process(c, &mut out);
        }
        println!("frames {} hz {}", out.len() / 2, peak_hz(&out, 48_000.0));
        assert!((peak_hz(&out, 48_000.0) - 440.0).abs() < 5.0);
    }
}
