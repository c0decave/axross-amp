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

    /// Take back what was written from `at` on (not yet read: the reader
    /// is before it). False when the reader is already past it.
    pub fn unwrite_to(&self, at: u64) -> bool {
        if self.tail.load(Ordering::Acquire) > at || at > self.head.load(Ordering::Relaxed) {
            return false;
        }
        self.head.store(at, Ordering::Release);
        true
    }

    /// Frames written and still to be heard (what a pending flush skips not counted).
    pub fn buffered(&self) -> u64 {
        let head = self.head.load(Ordering::Relaxed);
        let from = self
            .tail
            .load(Ordering::Acquire)
            .max(self.flush_at.load(Ordering::Relaxed));
        head.saturating_sub(from)
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
        Reading {
            head: self.head.load(Ordering::Acquire),
            tail: self.tail.load(Ordering::Relaxed),
            flush: (flushes != honoured).then(|| self.flush_at.load(Ordering::Relaxed)),
            flushes,
        }
    }

    /// The frame at the reading's place, which moves on (None: nothing there
    /// before the end or the flush point).
    pub fn next(&self, r: &mut Reading) -> Option<(f32, f32)> {
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
    }
}

/// One call's reading of the ring (see [`Ring::begin`]).
pub struct Reading {
    head: u64,
    tail: u64,
    pub flush: Option<u64>,
    flushes: u64,
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
    fn the_tap_keeps_the_latest() {
        let tap = Tap::new(4);
        tap.write(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
        let mut out = Vec::new();
        tap.read_into(&mut out);
        assert_eq!(out, [2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
    }
}
