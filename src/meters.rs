//! The meters that must hear every sample - loudness after EBU R128, the
//! true peak, the beat and the tempo, the notes and the track's key - on a
//! thread of their own, reading what is played from the engine's tap with
//! a read position of their own. They count whether the window is shown,
//! hidden, on another tab or with the picture off, at any rate and frame
//! rate; the window only reads their results.

use crate::analyzer::beat::Beat;
use crate::analyzer::fine::KEYS;
use crate::analyzer::loudness::Meter;
use crate::analyzer::notes::{self, Notes};
use crate::engine::Shared;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the meters say (copied out whole).
#[derive(Clone, Copy, Debug)]
pub struct Readings {
    pub momentary: f32,
    pub short_term: f32,
    pub integrated: f32,
    pub range: f32,
    pub momentary_max: f32,
    pub short_term_max: f32,
    pub sample_peak: f32,
    pub true_peak: f32,
    /// Seconds measured of this track.
    pub seconds: f32,
    pub beats: u64,
    pub bpm: f32,
    pub bpm_confidence: f32,
    pub beat_strength: f32,
    pub keys: [f32; KEYS],
    pub chroma: [f32; 12],
    /// The track's key so far: root (0 = C), minor, how sure (0..1: the
    /// profile's correlation, less where the next best is as good).
    pub key: (usize, bool, f32),
    /// Frames heard, and frames the meters could not read in time.
    pub frames: u64,
    pub lost: u64,
}

impl Default for Readings {
    fn default() -> Self {
        Readings {
            momentary: f32::NEG_INFINITY,
            short_term: f32::NEG_INFINITY,
            integrated: f32::NEG_INFINITY,
            range: 0.0,
            momentary_max: f32::NEG_INFINITY,
            short_term_max: f32::NEG_INFINITY,
            sample_peak: f32::NEG_INFINITY,
            true_peak: f32::NEG_INFINITY,
            seconds: 0.0,
            beats: 0,
            bpm: 0.0,
            bpm_confidence: 0.0,
            beat_strength: 0.0,
            keys: [0.0; KEYS],
            chroma: [0.0; 12],
            key: (0, false, 0.0),
            frames: 0,
            lost: 0,
        }
    }
}

pub struct Meters {
    readings: Arc<Mutex<Readings>>,
    track: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    /// A stall asked of the meters' thread (ms; tests).
    stall: Arc<AtomicU64>,
}

impl Drop for Meters {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// How often the notes are worked out (frames: a 20th of a second at 48 kHz).
const NOTES_EVERY: u64 = 2_400;

impl Meters {
    pub fn start(shared: Arc<Shared>) -> Meters {
        let readings = Arc::new(Mutex::new(Readings::default()));
        let track = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let stall = Arc::new(AtomicU64::new(0));
        let (r, t, s, st) = (readings.clone(), track.clone(), stop.clone(), stall.clone());
        let _ = std::thread::Builder::new()
            .name("amp-meters".into())
            .spawn(move || run(&shared, &r, &t, &s, &st));
        Meters {
            readings,
            track,
            stop,
            stall,
        }
    }

    /// As when the meters' thread gets no turn for `ms` (a busy machine):
    /// it sleeps that long before its next read (tests).
    #[doc(hidden)]
    pub fn stall(&self, ms: u64) {
        self.stall.store(ms, Ordering::Relaxed);
    }

    /// The latest readings.
    pub fn read(&self) -> Readings {
        self.readings.lock().map(|r| *r).unwrap_or_default()
    }

    /// A new track: its loudness and key counted from nothing.
    pub fn new_track(&self) {
        self.track.fetch_add(1, Ordering::Relaxed);
    }
}

/// Krumhansl-Kessler's key profiles (major, minor), from the tonic.
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// The key a chroma follows best (Krumhansl-Schmuckler): root, minor, and
/// how sure - the best correlation less the next best that is not its
/// relative key (the relative major / minor shares its notes), 0..1.
pub fn key_of(chroma: &[f64; 12]) -> (usize, bool, f32) {
    let mean = chroma.iter().sum::<f64>() / 12.0;
    if mean <= 0.0 {
        return (0, false, 0.0);
    }
    let mut scores = [(0usize, false, 0.0f64); 24];
    for (m, (minor, profile)) in [(false, &MAJOR), (true, &MINOR)].into_iter().enumerate() {
        let pm = profile.iter().sum::<f64>() / 12.0;
        for root in 0..12 {
            let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
            for k in 0..12 {
                let x = chroma[(root + k) % 12] - mean;
                let y = profile[k] - pm;
                num += x * y;
                da += x * x;
                db += y * y;
            }
            scores[m * 12 + root] = (root, minor, num / (da * db).sqrt().max(1e-12));
        }
    }
    scores.sort_by(|a, b| b.2.total_cmp(&a.2));
    let best = scores[0];
    // (the relative key: a minor third below a major one's root, above a minor's)
    let relative = |k: &(usize, bool, f64)| {
        k.1 != best.1
            && k.0
                == if best.1 {
                    (best.0 + 3) % 12
                } else {
                    (best.0 + 9) % 12
                }
    };
    let next = scores[1..]
        .iter()
        .find(|k| !relative(k))
        .map(|k| k.2)
        .unwrap_or(0.0);
    let sure = (best.2.clamp(0.0, 1.0) * ((best.2 - next) * 5.0).clamp(0.0, 1.0)) as f32;
    (best.0, best.1, sure)
}

fn run(
    shared: &Shared,
    readings: &Mutex<Readings>,
    track: &AtomicU64,
    stop: &AtomicBool,
    stall: &AtomicU64,
) {
    let mut rate = 0u32;
    let mut meter = Meter::new(48_000);
    let mut beat = Beat::new();
    let mut notes = Notes::new();
    // the latest frames (stereo, and mono for the notes), oldest first
    let keep = notes::LONG;
    let mut stereo: Vec<f32> = Vec::with_capacity(keep * 2 + 16_384);
    let mut mono: Vec<f32> = Vec::with_capacity(keep);
    let mut fresh: Vec<f32> = Vec::with_capacity(16_384 * 2);
    let mut read_to = 0u64;
    let mut track_seen = track.load(Ordering::Relaxed);
    let mut tracks_seen = shared.track_start().1;
    let mut since_notes = 0u64;
    let mut frames = 0u64;
    let mut lost = 0u64;
    let mut key = (0usize, false, 0.0f32);
    while !stop.load(Ordering::Relaxed) {
        let stalled = stall.swap(0, Ordering::Relaxed);
        if stalled > 0 {
            std::thread::sleep(Duration::from_millis(stalled));
        }
        let device = shared.device_rate.load(Ordering::Relaxed).max(8_000);
        if device != rate {
            rate = device;
            meter = Meter::new(rate);
            beat = Beat::new();
        }
        // a new track: counted from its first sound on (what of the track
        // before is still unread counted to it first)
        let (start, tracks) = shared.track_start();
        let now = track.load(Ordering::Relaxed);
        if tracks != tracks_seen || now != track_seen {
            if tracks != tracks_seen && start > read_to {
                let (_, to) = shared.tap_from_to(read_to, start, &mut fresh);
                if !fresh.is_empty() {
                    meter.push(&fresh);
                }
                read_to = to;
            }
            if tracks != tracks_seen {
                read_to = start;
            }
            tracks_seen = tracks;
            track_seen = now;
            meter.reset();
            notes.new_track();
            key = (0, false, 0.0);
            frames = 0;
        }
        let (from, to) = shared.tap_from(read_to, &mut fresh);
        if from > read_to && read_to > 0 {
            lost += from - read_to;
        }
        read_to = to;
        let new = fresh.len() / 2;
        if new == 0 {
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }
        frames += new as u64;
        meter.push(&fresh);
        // the beat reads windows that end in what is new
        stereo.extend_from_slice(&fresh);
        let excess = stereo.len().saturating_sub(keep * 2);
        stereo.drain(..excess);
        beat.push(&stereo, new, rate);
        beat.frame(new as f32 / rate as f32, true);
        since_notes += new as u64;
        if since_notes >= NOTES_EVERY * rate as u64 / 48_000 {
            since_notes = 0;
            mono.clear();
            mono.extend(
                stereo
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|f| (f[0] + f[1]) * 0.5),
            );
            notes.update(&mono, rate);
            key = key_of(&notes.track_chroma);
        }
        if let Ok(mut r) = readings.lock() {
            *r = Readings {
                momentary: meter.momentary,
                short_term: meter.short_term,
                integrated: meter.integrated,
                range: meter.range,
                momentary_max: meter.momentary_max,
                short_term_max: meter.short_term_max,
                sample_peak: meter.sample_peak,
                true_peak: meter.true_peak,
                seconds: meter.seconds,
                beats: beat.count,
                bpm: beat.bpm,
                bpm_confidence: beat.confidence,
                beat_strength: beat.strength,
                keys: notes.keys,
                chroma: notes.chroma,
                key,
                frames,
                lost,
            };
        }
        // (a little at a time: the beat's pulse comes in step)
        std::thread::sleep(Duration::from_millis(8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_of_scales_and_their_relatives() {
        // C major's scale, its tonic and fifth strongest
        let mut c = [0.0f64; 12];
        for (k, v) in [
            (0, 1.0),
            (2, 0.5),
            (4, 0.8),
            (5, 0.5),
            (7, 0.9),
            (9, 0.5),
            (11, 0.4),
        ] {
            c[k] = v;
        }
        let (root, minor, sure) = key_of(&c);
        assert_eq!((root, minor), (0, false));
        assert!(sure > 0.3, "{sure}");
        // nothing heard: not sure at all
        assert_eq!(key_of(&[0.0; 12]).2, 0.0);
        // every note alike: no key
        assert!(key_of(&[1.0; 12]).2 < 0.1);
    }
}
