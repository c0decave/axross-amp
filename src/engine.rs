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
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::collections::VecDeque;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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

/// Seconds of sound decoded ahead of the output.
const AHEAD_SECS: f32 = 0.35;
/// Stereo frames of the most recent output the visualizations may read.
pub const TAP_FRAMES: usize = 8192;
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
    /// How ReplayGain levels the tracks (from the next track on).
    Loudness(Loudness),
    /// Play on this output device (an id of [`output_devices`]; None: the
    /// default) - also while playing.
    Output(Option<String>),
    Quit,
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

/// ReplayGain's settings: the mode and a pre-amplification (dB) for the
/// tracks that have a gain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loudness {
    pub mode: GainMode,
    pub preamp_db: f32,
}

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
            return 1.0;
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
    /// Every text tag as the file has it: (key, value) - the track's details.
    pub tags: Vec<(String, String)>,
    pub cover: Option<Vec<u8>>,
    pub seekable: bool,
    pub rate: u32,
    pub channels: usize,
    pub bits: Option<u32>,
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

/// What the UI reads while the engine runs.
pub struct Shared {
    buffer: Mutex<VecDeque<f32>>,
    /// Frames heard since the engine started (never set back), the count at
    /// which the current track's position began, and that position (ms).
    heard: AtomicU64,
    origin: AtomicU64,
    base_ms: AtomicU64,
    /// Where (in `heard`) the queued track begins (u64::MAX: none decoded).
    boundary: AtomicU64,
    pub playing: AtomicBool,
    volume_bits: AtomicU32,
    pub muted: AtomicBool,
    pub device_rate: AtomicU32,
    pub tap: Mutex<VecDeque<f32>>,
}

/// What the output callback keeps between its calls.
pub struct RenderState {
    /// The volume reached (ramped towards the one set: no click).
    gain: f32,
    frame: Vec<f32>,
}

impl Default for RenderState {
    fn default() -> Self {
        RenderState {
            gain: 0.0,
            frame: Vec::with_capacity(4096),
        }
    }
}

impl Shared {
    fn new() -> Self {
        Shared {
            buffer: Mutex::new(VecDeque::with_capacity(48_000)),
            heard: AtomicU64::new(0),
            origin: AtomicU64::new(0),
            base_ms: AtomicU64::new(0),
            boundary: AtomicU64::new(u64::MAX),
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
        let frames = self
            .heard
            .load(Ordering::Relaxed)
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

    /// Drop what is decoded and not yet heard; the position starts at `ms`.
    fn restart_at(&self, ms: u64) {
        let buffer = self.buffer.lock();
        if let Ok(mut buffer) = buffer {
            buffer.clear();
        }
        self.origin
            .store(self.heard.load(Ordering::Relaxed), Ordering::Relaxed);
        self.base_ms.store(ms, Ordering::Relaxed);
        self.boundary.store(u64::MAX, Ordering::Relaxed);
    }

    fn buffered_frames(&self) -> usize {
        self.buffer.lock().map(|b| b.len() / 2).unwrap_or(0)
    }

    /// Where the sound decoded next will be heard (in `heard`).
    fn end_of_buffer(&self) -> u64 {
        match self.buffer.lock() {
            // (heard is counted under the same lock: the two agree)
            Ok(b) => self.heard.load(Ordering::Relaxed) + (b.len() / 2) as u64,
            Err(_) => self.heard.load(Ordering::Relaxed),
        }
    }

    fn push(&self, samples: &[f32]) {
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.extend(samples.iter().copied());
        }
    }

    /// Fill `out` (interleaved, `channels` a frame) with what is to be heard:
    /// the stereo sound at the volume (ramped over the buffer: no click),
    /// both sides mixed for one channel, nothing on the channels past two.
    /// What the output's callback does; tests call it for an output of their own.
    pub fn render(&self, out: &mut [f32], channels: usize, state: &mut RenderState) {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        state.frame.clear();
        if self.playing.load(Ordering::Relaxed)
            && let Ok(mut buffer) = self.buffer.try_lock()
        {
            let take = (frames * 2).min(buffer.len());
            state.frame.extend(buffer.drain(..take));
            self.heard.fetch_add((take / 2) as u64, Ordering::Relaxed);
        }
        let heard = state.frame.len() / 2;
        state.frame.resize(frames * 2, 0.0);
        let target = if self.muted.load(Ordering::Relaxed) {
            0.0
        } else {
            self.volume()
        };
        let step = (target - state.gain) / frames.max(1) as f32;
        for (i, slot) in out.chunks_exact_mut(channels).enumerate() {
            state.gain += step;
            let (l, r) = (
                state.frame[i * 2] * state.gain,
                state.frame[i * 2 + 1] * state.gain,
            );
            if channels == 1 {
                slot[0] = ((l + r) * 0.5).clamp(-1.0, 1.0);
                continue;
            }
            for (c, sample) in slot.iter_mut().enumerate() {
                *sample = match c {
                    0 => l.clamp(-1.0, 1.0),
                    1 => r.clamp(-1.0, 1.0),
                    _ => 0.0,
                };
            }
        }
        // (a remainder that is no whole frame: silence)
        let whole = frames * channels;
        out[whole..].iter_mut().for_each(|s| *s = 0.0);
        state.gain = target;
        if heard > 0
            && let Ok(mut tap) = self.tap.try_lock()
        {
            tap.extend(state.frame[..heard * 2].iter().copied());
            let excess = tap.len().saturating_sub(TAP_FRAMES * 2);
            tap.drain(..excess);
        }
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
    let mut hosts: Vec<cpal::Host> = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(host) = cpal::host_from_id(cpal::HostId::PulseAudio) {
        hosts.push(host);
    }
    hosts.push(cpal::default_host());
    hosts
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
    let mut state = RenderState::default();
    let mut mixed = Vec::<f32>::with_capacity(8192);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                mixed.resize(data.len(), 0.0);
                shared.render(&mut mixed, channels, &mut state);
                for (out, v) in data.iter_mut().zip(&mixed) {
                    *out = T::from_sample(*v);
                }
            },
            |err| eprintln!("axross-amp: output: {err}"),
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
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    resampler: Resampler,
    generation: u64,
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
    info: TrackInfo,
    /// At its end (and the resampler's tail given out).
    eof: bool,
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

/// A local file as a media source.
fn open_file(path: &str) -> Result<(MediaSourceStream<'static>, Hint), String> {
    let meta = std::fs::metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "no such file".to_owned()
        } else {
            e.to_string()
        }
    })?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    Ok((
        MediaSourceStream::new(Box::new(file), Default::default()),
        hint_for(path, None),
    ))
}

/// The decoder for a media source; `seekable` for a file.
fn open_media(
    mss: MediaSourceStream<'static>,
    hint: Hint,
    seekable: bool,
    path: &str,
    out_rate: u32,
    generation: u64,
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
    let decoder = codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("cannot decode it ({e})"))?;
    let rate = params.sample_rate.unwrap_or(44_100);
    let mut info = TrackInfo {
        rate,
        channels: params.channels.as_ref().map(|c| c.count()).unwrap_or(2),
        bits: params.bits_per_sample,
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
        crate::mp4edit::read_edit(Path::new(path)).map(|e| e.frames(rate))
    } else {
        None
    };
    if let Some(length) = edit.and_then(|(_, l)| l) {
        info.duration = Some(length as f64 / rate as f64);
    }
    let decoding = Decoding {
        format,
        decoder,
        track_id: track.id,
        time_base: track.time_base,
        resampler: Resampler::new(rate, out_rate),
        generation,
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
        info: info.clone(),
        eof: false,
    };
    Ok((decoding, info))
}

/// A local file, opened to play.
fn open(
    path: &str,
    out_rate: u32,
    generation: u64,
    loudness: Loudness,
) -> Result<(Decoding, TrackInfo), String> {
    let (mss, hint) = open_file(path)?;
    open_media(mss, hint, true, path, out_rate, generation, loudness)
}

impl Decoding {
    /// Seek to `secs`; returns where it landed (ms).
    fn seek(&mut self, secs: f64) -> Option<u64> {
        if !self.seekable {
            return None;
        }
        let secs = secs.max(0.0);
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
        self.eof = false;
        self.resampler = Resampler::new(self.rate, self.resampler.to);
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
                Err(SymError::ResetRequired) => return Ok(false),
                Err(SymError::IoError(e)) => return Err(e.to_string()),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    to_stereo(
                        &buffer,
                        (&mut self.layout, &mut self.matrix),
                        &mut self.scratch,
                        &mut self.stereo,
                    );
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
                    if self.gain != 1.0 {
                        for s in &mut self.stereo {
                            *s *= self.gain;
                        }
                    }
                    self.resampler.process(&self.stereo, out);
                    return Ok(true);
                }
                Err(SymError::DecodeError(_)) => continue, // (a damaged packet: the next one)
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// The end: what the resampler still holds.
    fn finish(&mut self, out: &mut Vec<f32>) {
        if !self.eof {
            self.resampler.flush(out);
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
    open(path, 48_000, 0, Loudness::default()).map(|(_, info)| info)
}

/// Decode all of `path` as the player would (stereo at `rate`): the
/// frames made and the track's info. For tests and measurements.
pub fn decode_file(path: &str, rate: u32) -> Result<(usize, TrackInfo), String> {
    let (mut decoding, info) = open(path, rate, 0, Loudness::default())?;
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
                    generation,
                    Loudness::default(),
                ) {
                    Err(why) => why,
                    Ok((mut decoding, mut info)) => {
                        info.station = answer.station.clone();
                        info.station_genre = answer.genre.clone();
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
    File(Box<Decoding>),
    Stream(StreamSource),
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
    /// The track still heard while the queued one is decoded after it, and
    /// the queued one with its info (told when it is heard).
    ending: Option<(Box<Decoding>, Queued, TrackInfo)>,
    chunk: Vec<f32>,
}

impl Player {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    fn connect(&mut self, device: Option<&str>) {
        if self.headless {
            self.shared.device_rate.store(48_000, Ordering::Relaxed);
            self.out_rate = 48_000;
            return self.send(Event::Output("no device (headless)".into()));
        }
        match open_output(self.shared.clone(), device) {
            Ok((stream, rate, name)) => {
                self.shared.device_rate.store(rate, Ordering::Relaxed);
                self.stream = Some(stream);
                self.out_rate = rate;
                self.send(Event::Output(name));
            }
            Err(why) => {
                self.out_rate = 48_000;
                self.send(Event::Output(format!("no sound: {why}")));
            }
        }
        self.ahead = (self.out_rate as f32 * AHEAD_SECS) as usize;
    }

    /// Take back a queued track already decoded after the current one (a
    /// seek or a new output in the track still heard).
    fn cancel_advance(&mut self) {
        if let Some((decoding, queued, _)) = self.ending.take() {
            self.current = Some(Source::File(decoding));
            self.queued = Some(queued);
        }
        self.shared.boundary.store(u64::MAX, Ordering::Relaxed);
    }

    fn reset(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.shared.restart_at(0);
        self.eq.reset();
        self.current = None;
        self.ending = None;
        self.queued = None;
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
                self.reset();
                if crate::playlist::is_stream(&path) {
                    self.current = Some(Source::Stream(start_stream(
                        path,
                        generation,
                        self.out_rate,
                        self.events.clone(),
                        self.wake.clone(),
                    )));
                    self.shared.playing.store(play, Ordering::Relaxed);
                    return true;
                }
                match open(&path, self.out_rate, generation, self.loudness) {
                    Ok((mut decoding, info)) => {
                        let at = if start_ms > 0 {
                            decoding.seek(start_ms as f64 / 1000.0).unwrap_or(0)
                        } else {
                            0
                        };
                        self.shared.restart_at(at);
                        self.send(Event::Loaded { generation, info });
                        self.shared.playing.store(play, Ordering::Relaxed);
                        self.current = Some(Source::File(Box::new(decoding)));
                    }
                    Err(message) => self.send(Event::Failed {
                        generation,
                        message,
                    }),
                }
            }
            Command::Queue(queued) => {
                // (while the queued one is already decoded, the next is told after it is heard)
                if self.ending.is_none() {
                    self.queued = queued;
                }
            }
            Command::Play => self
                .shared
                .playing
                .store(self.current.is_some(), Ordering::Relaxed),
            Command::Pause => self.shared.playing.store(false, Ordering::Relaxed),
            Command::Stop => {
                self.shared.playing.store(false, Ordering::Relaxed);
                self.cancel_advance();
                if let Some(Source::Stream(_)) = self.current {
                    // a stream is let go (its connection closed); Play connects anew
                    self.current = None;
                    self.shared.restart_at(0);
                } else if let Some(Source::File(d)) = self.current.as_mut() {
                    let at = d.seek(0.0).unwrap_or(0);
                    self.shared.restart_at(at);
                    self.eq.reset();
                }
            }
            Command::Seek(secs) => {
                if let Some(Source::File(_)) = self.current {
                    self.cancel_advance();
                }
                if let Some(Source::File(d)) = self.current.as_mut()
                    && let Some(at) = d.seek(secs)
                {
                    self.shared.restart_at(at);
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
                if let Some(Source::File(d)) = self.current.as_mut() {
                    d.gain = loudness.factor(&d.info);
                }
            }
            Command::Output(device) => {
                let playing = self.shared.playing.load(Ordering::Relaxed);
                self.cancel_advance();
                let at = self.shared.position();
                drop(self.stream.take()); // (the old device lets go first)
                let before = self.out_rate;
                self.connect(device.as_deref());
                if self.out_rate != before {
                    // another rate: the equalizer anew, the sound decoded
                    // ahead dropped, the track resampled from where it was
                    self.eq = Equalizer::new(self.out_rate);
                    self.eq.set_gains(self.gains.0);
                    self.eq.enabled = self.gains.1;
                    match self.current.as_mut() {
                        Some(Source::File(d)) => {
                            d.set_out_rate(self.out_rate);
                            let landed = d.seek(at).unwrap_or(0);
                            self.shared.restart_at(landed);
                        }
                        Some(Source::Stream(s)) => {
                            s.rate.store(self.out_rate, Ordering::Relaxed);
                            while s.pcm.try_recv().is_ok() {}
                            s.pending.clear();
                            s.pending_frames = 0;
                            s.primed = false;
                            self.shared.restart_at(0);
                        }
                        None => {}
                    }
                }
                self.shared
                    .playing
                    .store(playing && self.current.is_some(), Ordering::Relaxed);
            }
        }
        true
    }

    /// The queued track heard now: its position begins.
    fn check_boundary(&mut self) {
        let boundary = self.shared.boundary.load(Ordering::Relaxed);
        if boundary == u64::MAX || self.shared.heard.load(Ordering::Relaxed) < boundary {
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

    /// At the end of a file: the queued one decoded on after it, if it opens.
    fn advance(&mut self) -> bool {
        let Some(queued) = self.queued.clone() else {
            return false;
        };
        if self.ending.is_some() || crate::playlist::is_stream(&queued.path) {
            return false;
        }
        let Some(Source::File(current)) = self.current.as_mut() else {
            return false;
        };
        let Ok((mut next, info)) = open(
            &queued.path,
            self.out_rate,
            queued.generation,
            self.loudness,
        ) else {
            // (it fails when its turn comes, and says why)
            self.queued = None;
            return false;
        };
        if next.rate == current.rate {
            // the same resampler goes on: not even the filter's edge between them
            next.resampler = current.resampler.clone();
        } else {
            let mut tail = Vec::new();
            current.finish(&mut tail);
            self.eq.process(&mut tail);
            self.shared.push(&tail);
        }
        current.eof = true;
        self.shared
            .boundary
            .store(self.shared.end_of_buffer(), Ordering::Relaxed);
        self.queued = None;
        if let Some(Source::File(old)) = self.current.replace(Source::File(Box::new(next))) {
            self.ending = Some((old, queued, info));
        }
        true
    }

    /// Decode what the output needs next; false when there is nothing to do.
    fn tick(&mut self) -> bool {
        if !self.shared.playing.load(Ordering::Relaxed) {
            return false;
        }
        let buffered = self.shared.buffered_frames();
        match self.current {
            None => false,
            Some(Source::File(_)) => self.tick_file(buffered),
            Some(Source::Stream(_)) => self.tick_stream(buffered),
        }
    }

    fn tick_file(&mut self, buffered: usize) -> bool {
        let Some(Source::File(d)) = self.current.as_ref() else {
            return false;
        };
        let generation = d.generation;
        if d.eof {
            if buffered == 0 && self.shared.boundary.load(Ordering::Relaxed) == u64::MAX {
                self.shared.playing.store(false, Ordering::Relaxed);
                self.current = None;
                self.send(Event::Ended { generation });
            }
            return false;
        }
        if buffered >= self.ahead {
            return false;
        }
        self.chunk.clear();
        let result = match self.current.as_mut() {
            Some(Source::File(d)) => d.step(&mut self.chunk),
            _ => return false,
        };
        match result {
            Ok(true) => {}
            Ok(false) => {
                if !self.advance()
                    && let Some(Source::File(d)) = self.current.as_mut()
                {
                    d.finish(&mut self.chunk);
                }
            }
            Err(message) => {
                self.current = None;
                self.ending = None;
                self.shared.playing.store(false, Ordering::Relaxed);
                self.send(Event::Failed {
                    generation,
                    message,
                });
                return false;
            }
        }
        self.eq.process(&mut self.chunk);
        self.shared.push(&self.chunk);
        true
    }

    fn tick_stream(&mut self, buffered: usize) -> bool {
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
        self.shared.push(&chunk);
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
        ending: None,
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
        player.check_boundary();
        busy = player.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn one_output_channel_hears_both_sides() {
        let shared = Shared::new();
        shared.playing.store(true, Ordering::Relaxed);
        shared.set_volume(1.0);
        shared.push(&[0.5, 0.25, 0.5, 0.25]);
        let mut state = RenderState {
            gain: 1.0,
            ..Default::default()
        };
        let mut out = [9.0f32; 3];
        shared.render(&mut out, 1, &mut state);
        assert_eq!(out, [0.375, 0.375, 0.0]);
        // four channels: the stereo on the first two, silence on the others
        shared.push(&[0.5, 0.25]);
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
        };
        assert!((db(track.factor(&info)) + 6.0).abs() < 0.01);
        let album = Loudness {
            mode: GainMode::Album,
            preamp_db: 0.0,
        };
        assert!((db(album.factor(&info)) + 3.0).abs() < 0.01);
        // +9 dB in all would take a 0.5 peak past full scale: held at 2
        let loud = Loudness {
            mode: GainMode::Track,
            preamp_db: 15.0,
        };
        assert!((loud.factor(&info) - 2.0).abs() < 1e-4);
        assert_eq!(track.factor(&TrackInfo::default()), 1.0);
        assert_eq!(gain_value("-6.54 dB"), Some(-6.54));
        assert_eq!(gain_value("+1.2dB"), Some(1.2));
        assert_eq!(gain_value("0.988"), Some(0.988));
        assert_eq!(gain_value("loud"), None);
    }

    #[test]
    fn a_wav_file_decodes_seeks_and_ends() {
        let path = std::env::temp_dir().join(format!("amp-{}.wav", std::process::id()));
        write_wav(&path, 22_050, 1, 22_050 * 2); // two seconds of mono
        let (mut d, info) = open(path.to_str().unwrap(), 44_100, 1, Loudness::default()).unwrap();
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
            open("/nonexistent.mp3", 44_100, 2, Loudness::default())
                .err()
                .as_deref(),
            Some("no such file")
        );
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
