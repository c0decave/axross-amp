//! What the audio callback reads without a lock and without allocating:
//!
//! * [`Ring`] - a single-producer single-consumer ring of stereo frames
//!   (the decoder thread writes, the output callback reads). Its counters
//!   count frames since the start and never go back; a *flush* (a seek, a
//!   new track) is a request the reader honours on its next call - it fades
//!   out what it still has and then skips to the flush point - so the
//!   writer never touches the reader's side.
//! * [`Tap`] - the most recent frames the output played, overwritten in a
//!   circle, for the visualizations (a reader may see a frame torn between
//!   two writes; a picture does not mind).
//!
//! Samples are kept as the bits of an f32 in `AtomicU32`s: no `unsafe`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub struct Ring {
    buf: Box<[AtomicU32]>,
    mask: u64,
    /// Frames written, frames read or skipped (since the start).
    head: AtomicU64,
    tail: AtomicU64,
    /// Where the last flush asked the reader to skip to, and how many flushes
    /// there were (the reader keeps the count it honoured).
    flush_at: AtomicU64,
    flushes: AtomicU64,
    /// A cut: frames from..to written but not to be read (taken back),
    /// as two halves of one u128 would be - the generation tells them apart.
    cut_from: AtomicU64,
    cut_to: AtomicU64,
    cuts: AtomicU64,
    /// The cuts the reader has seen (honoured, or passed already).
    cuts_seen: AtomicU64,
}

impl Ring {
    /// A ring of at least `frames` stereo frames.
    pub fn new(frames: usize) -> Ring {
        let frames = frames.next_power_of_two().max(1024);
        Ring {
            buf: (0..frames * 2).map(|_| AtomicU32::new(0)).collect(),
            mask: frames as u64 - 1,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            flush_at: AtomicU64::new(0),
            flushes: AtomicU64::new(0),
            cut_from: AtomicU64::new(0),
            cut_to: AtomicU64::new(0),
            cuts: AtomicU64::new(0),
            cuts_seen: AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> u64 {
        self.mask + 1
    }

    // -- the writer --------------------------------------------------------------------------

    /// Interleaved stereo `samples` in, as many frames as there is room for:
    /// how many went in.
    pub fn push(&self, samples: &[f32]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let room = self.capacity() - (head - tail);
        let n = ((samples.len() / 2) as u64).min(room);
        for i in 0..n {
            let at = (((head + i) & self.mask) * 2) as usize;
            let s = (i * 2) as usize;
            self.buf[at].store(samples[s].to_bits(), Ordering::Relaxed);
            self.buf[at + 1].store(samples[s + 1].to_bits(), Ordering::Relaxed);
        }
        self.head.store(head + n, Ordering::Release);
        n as usize
    }

    /// Everything written so far is to be skipped: where the next frame
    /// written will be read (in frames since the start).
    pub fn flush(&self) -> u64 {
        let head = self.head.load(Ordering::Relaxed);
        self.flush_at.store(head, Ordering::Relaxed);
        self.flushes.fetch_add(1, Ordering::Release);
        head
    }

    /// Take back what was written from `at` on: the reader skips it when it
    /// gets there. The writer never moves back (what the reader may be
    /// reading is never written over); a reader already past `at` keeps
    /// what it has. False when the reader is that close (`margin` frames:
    /// more than it reads at a call) - then nothing is taken back.
    pub fn cut(&self, at: u64, margin: u64) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        if at > head || self.tail.load(Ordering::Acquire) + margin > at {
            return false;
        }
        self.cut_from.store(at, Ordering::Relaxed);
        self.cut_to.store(head, Ordering::Relaxed);
        self.cuts.fetch_add(1, Ordering::Release);
        true
    }

    /// Frames written and still to be heard (what a pending flush skips not counted).
    pub fn buffered(&self) -> u64 {
        let head = self.head.load(Ordering::Relaxed);
        let from = self
            .tail
            .load(Ordering::Acquire)
            .max(self.flush_at.load(Ordering::Relaxed));
        let mut left = head.saturating_sub(from);
        // (a cut the reader has not reached: not to be heard)
        if self.cuts.load(Ordering::Acquire) != self.cuts_seen.load(Ordering::Acquire) {
            let (cf, ct) = (
                self.cut_from.load(Ordering::Relaxed),
                self.cut_to.load(Ordering::Relaxed),
            );
            if from <= cf {
                left = left.saturating_sub(ct - cf);
            }
        }
        left
    }

    /// Where the next frame written will be read.
    pub fn written(&self) -> u64 {
        self.head.load(Ordering::Relaxed)
    }

    /// Frames read or skipped since the start (the reader's place).
    pub fn read(&self) -> u64 {
        self.tail.load(Ordering::Acquire)
    }

    // -- the reader ---------------------------------------------------------------------------

    /// A reader's view for one call: where it may read up to, and a flush
    /// it has not honoured yet.
    pub fn begin(&self, honoured: u64) -> Reading {
        let flushes = self.flushes.load(Ordering::Acquire);
        let cuts = self.cuts.load(Ordering::Acquire);
        let cut = (cuts != self.cuts_seen.load(Ordering::Relaxed)).then(|| {
            (
                self.cut_from.load(Ordering::Relaxed),
                self.cut_to.load(Ordering::Relaxed),
            )
        });
        let tail = self.tail.load(Ordering::Relaxed);
        Reading {
            head: self.head.load(Ordering::Acquire),
            tail,
            flush: (flushes != honoured).then(|| self.flush_at.load(Ordering::Relaxed)),
            flushes,
            // (a cut the reader is past already: what it read stays read)
            cut: cut.filter(|(from, _)| tail <= *from),
            cuts,
        }
    }

    /// The frame at the reading's place, which moves on (None: nothing there
    /// before the end or the flush point).
    pub fn next(&self, r: &mut Reading) -> Option<(f32, f32)> {
        if let Some((from, to)) = r.cut
            && r.tail >= from
        {
            r.tail = r.tail.max(to);
            r.cut = None;
        }
        let limit = r.flush.unwrap_or(r.head);
        if r.tail >= limit {
            return None;
        }
        let at = ((r.tail & self.mask) * 2) as usize;
        r.tail += 1;
        Some((
            f32::from_bits(self.buf[at].load(Ordering::Relaxed)),
            f32::from_bits(self.buf[at + 1].load(Ordering::Relaxed)),
        ))
    }

    /// Frames there to read from the reading's place (before a flush point;
    /// a cut ahead not counted).
    pub fn left(&self, r: &Reading) -> u64 {
        let limit = r.flush.unwrap_or(r.head);
        let mut left = limit.saturating_sub(r.tail);
        if let Some((from, to)) = r.cut
            && r.tail <= from
        {
            left = left.saturating_sub(to.min(limit) - from.min(limit));
        }
        left
    }

    /// Skip to the pending flush's point; the flush count it honoured.
    pub fn skip(&self, r: &mut Reading) -> u64 {
        if let Some(at) = r.flush.take() {
            r.tail = r.tail.max(at);
        }
        r.flushes
    }

    /// The reading done: its place is the ring's.
    pub fn end(&self, r: Reading) {
        self.tail.store(r.tail, Ordering::Release);
        // (the cut seen: honoured, or passed already)
        if r.cut.is_none() {
            self.cuts_seen.store(r.cuts, Ordering::Release);
        }
    }
}

/// One call's reading of the ring (see [`Ring::begin`]).
pub struct Reading {
    head: u64,
    tail: u64,
    pub flush: Option<u64>,
    flushes: u64,
    cut: Option<(u64, u64)>,
    cuts: u64,
}

/// The latest frames played, for the visualizations.
pub struct Tap {
    buf: Box<[AtomicU32]>,
    frames: usize,
    written: AtomicU64,
}

impl Tap {
    pub fn new(frames: usize) -> Tap {
        Tap {
            buf: (0..frames * 2).map(|_| AtomicU32::new(0)).collect(),
            frames,
            written: AtomicU64::new(0),
        }
    }

    /// One frame played (the output callback).
    pub fn put(&self, l: f32, r: f32) {
        let n = self.written.load(Ordering::Relaxed);
        let at = (n % self.frames as u64) as usize * 2;
        self.buf[at].store(l.to_bits(), Ordering::Relaxed);
        self.buf[at + 1].store(r.to_bits(), Ordering::Relaxed);
        self.written.store(n + 1, Ordering::Release);
    }

    /// Interleaved frames (tests, pictures).
    pub fn write(&self, samples: &[f32]) {
        for f in samples.as_chunks::<2>().0 {
            self.put(f[0], f[1]);
        }
    }

    /// The latest frames, oldest first, into `out` (interleaved): as many
    /// as were played, at most the tap's size.
    pub fn read_into(&self, out: &mut Vec<f32>) {
        out.clear();
        let n = self.written.load(Ordering::Acquire);
        let count = n.min(self.frames as u64);
        for k in (n - count)..n {
            let at = (k % self.frames as u64) as usize * 2;
            out.push(f32::from_bits(self.buf[at].load(Ordering::Relaxed)));
            out.push(f32::from_bits(self.buf[at + 1].load(Ordering::Relaxed)));
        }
    }

    /// As [`Tap::read_into`], the newest `delay` frames left out, at most
    /// `max` frames; the count of frames played up to the last one read
    /// (what came since the last read is the difference).
    pub fn read_delayed(&self, out: &mut Vec<f32>, delay: u64, max: u64) -> u64 {
        out.clear();
        let n = self.written.load(Ordering::Acquire).saturating_sub(delay);
        let count = n
            .min(self.frames as u64 - delay.min(self.frames as u64))
            .min(max);
        for k in (n - count)..n {
            let at = (k % self.frames as u64) as usize * 2;
            out.push(f32::from_bits(self.buf[at].load(Ordering::Relaxed)));
            out.push(f32::from_bits(self.buf[at + 1].load(Ordering::Relaxed)));
        }
        n
    }

    /// The frames from `from` on (counted as [`Tap::read_delayed`] counts
    /// them), the newest `delay` left out, `max` at most, into `out`
    /// (interleaved): where they begin and end. Those already written over
    /// are skipped (the start is later than `from`).
    pub fn read_from(&self, from: u64, delay: u64, max: u64, out: &mut Vec<f32>) -> (u64, u64) {
        out.clear();
        let end = self.written.load(Ordering::Acquire).saturating_sub(delay);
        // (what is left of the ring after the newest `delay` frames, with a
        // margin for the writer running on meanwhile)
        let oldest = end.saturating_sub(self.frames as u64 - delay.min(self.frames as u64) - 1024);
        let start = from.max(oldest).min(end);
        let end = end.min(start + max);
        for k in start..end {
            let at = (k % self.frames as u64) as usize * 2;
            out.push(f32::from_bits(self.buf[at].load(Ordering::Relaxed)));
            out.push(f32::from_bits(self.buf[at + 1].load(Ordering::Relaxed)));
        }
        (start, end)
    }

    /// Forget what was played (nothing to show).
    pub fn clear(&self) {
        // (the frames stay; the count says none was played)
        self.written.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_go_round_and_a_flush_skips_what_was_written() {
        let ring = Ring::new(1024);
        let frames: Vec<f32> = (0..1500).flat_map(|i| [i as f32, -(i as f32)]).collect();
        assert_eq!(ring.push(&frames), 1024, "full");
        let mut r = ring.begin(0);
        for i in 0..600 {
            assert_eq!(ring.next(&mut r), Some((i as f32, -(i as f32))));
        }
        ring.end(r);
        assert_eq!(ring.push(&frames[1024 * 2..]), 476, "round the end");
        assert_eq!(ring.buffered(), 900);
        // a flush: what is there is skipped, what comes after is read
        let at = ring.flush();
        assert_eq!(ring.buffered(), 0);
        ring.push(&[7.0, 8.0]);
        let mut r = ring.begin(0);
        assert_eq!(r.flush, Some(at));
        assert_eq!(
            ring.next(&mut r),
            Some((600.0, -600.0)),
            "old ones up to the flush"
        );
        let honoured = ring.skip(&mut r);
        assert_eq!(ring.next(&mut r), Some((7.0, 8.0)));
        assert_eq!(ring.next(&mut r), None);
        ring.end(r);
        assert!(ring.begin(honoured).flush.is_none());
        assert_eq!(ring.read(), at + 1);
    }

    #[test]
    fn taking_back_never_writes_over_what_is_read() {
        // a writer that takes back its last frames now and then, a reader
        // reading all the while: every frame read is newer than the one
        // before (a frame written over, a writer gone back below the reader
        // would show; the old take-back could even panic in debug)
        let ring = std::sync::Arc::new(Ring::new(4096));
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (r, d) = (ring.clone(), done.clone());
        let reader = std::thread::spawn(move || {
            let mut last = -1.0f32;
            let mut seen = 0usize;
            while !d.load(Ordering::Relaxed) || r.buffered() > 0 {
                let mut reading = r.begin(0);
                for _ in 0..97 {
                    match r.next(&mut reading) {
                        Some((v, _)) => {
                            assert!(v > last, "{v} after {last}");
                            last = v;
                            seen += 1;
                        }
                        None => break,
                    }
                }
                r.end(reading);
            }
            seen
        });
        let mut n = 0u64;
        let mut taken = 0;
        for round in 0..20_000u64 {
            let frames: Vec<f32> = (0..32).flat_map(|i| [(n + i) as f32, 0.0]).collect();
            let mut put = 0;
            while put < 32 {
                put += ring.push(&frames[put * 2..]);
            }
            n += 32;
            if round % 7 == 0 && ring.cut(ring.written() - 16, 64) {
                taken += 1;
            }
        }
        done.store(true, Ordering::Relaxed);
        let seen = reader.join().unwrap();
        assert!(taken > 0 && seen > 0, "{taken} cuts, {seen} read");
    }

    #[test]
    fn the_tap_keeps_the_latest() {
        let tap = Tap::new(4);
        tap.write(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
        let mut out = Vec::new();
        tap.read_into(&mut out);
        assert_eq!(out, [2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
    }
}
