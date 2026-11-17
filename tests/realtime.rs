//! The output callback's rules: it never allocates (an allocation can wait
//! on a lock inside the allocator - a dropout), and nothing it plays steps
//! abruptly - a pause, a seek, a stop or another track fades out and in
//! (a cut in the middle of a wave is a click).
//!
//! A counting allocator watches the thread while it renders.

use axross_amp::engine::{Command, Engine, RenderState};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static WATCHED: Cell<bool> = const { Cell::new(false) };
}

fn watched() -> bool {
    WATCHED.try_with(Cell::get).unwrap_or(false)
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if watched() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if watched() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if watched() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A stereo sine of `secs` at 48 kHz (no resampling: what is played is the file).
fn sine(path: &std::path::Path, secs: f32, hz: f32) {
    let rate = 48_000u32;
    let frames = (rate as f32 * secs) as u32;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &rate.to_le_bytes(),
        &(rate * 4).to_le_bytes(),
        &4u16.to_le_bytes(),
        &16u16.to_le_bytes(),
    ] {
        out.extend_from_slice(v);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(frames * 4).to_le_bytes());
    for i in 0..frames {
        let v = ((i as f64 * hz as f64 * std::f64::consts::TAU / rate as f64).sin() * 0.3 * 32767.0)
            as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("amp-rt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Render `calls` buffers of `frames` (one channel's samples kept), at the
/// pace of a device.
fn render(
    engine: &Engine,
    state: &mut RenderState,
    buf: &mut Vec<f32>,
    frames: usize,
    channels: usize,
    calls: usize,
    heard: &mut Vec<f32>,
) {
    buf.resize(frames * channels, 0.0);
    for _ in 0..calls {
        WATCHED.with(|w| w.set(true));
        engine.shared.render(buf, channels, state);
        WATCHED.with(|w| w.set(false));
        heard.extend(buf.chunks(channels).map(|f| f[0]));
        std::thread::sleep(Duration::from_millis(frames as u64 * 1000 / 48_000));
    }
}

fn load(engine: &Engine, path: &std::path::Path, generation: u64) {
    engine.send(Command::Load {
        path: path.to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation,
    });
}

#[test]
fn the_callback_never_allocates_and_nothing_clicks() {
    let dir = folder("callback");
    sine(&dir.join("a.wav"), 6.0, 440.0);
    sine(&dir.join("b.wav"), 6.0, 330.0);
    let engine = Engine::start_headless(|| {});
    load(&engine, &dir.join("a.wav"), 1);
    std::thread::sleep(Duration::from_millis(200));
    let mut state = RenderState::default();
    let mut buf = Vec::new();
    let mut heard = Vec::new();
    // every size a device may ask for, mono to 5.1
    for (frames, channels) in [(64, 2), (480, 1), (4096, 2), (8192, 6), (16_384, 2)] {
        render(
            &engine, &mut state, &mut buf, frames, channels, 4, &mut heard,
        );
    }
    // a pause and a resume, a seek, a stop and play, another track
    let mut step = |engine: &Engine, command: Option<Command>, heard: &mut Vec<f32>| {
        if let Some(c) = command {
            engine.send(c);
        }
        std::thread::sleep(Duration::from_millis(30));
        render(engine, &mut state, &mut buf, 480, 2, 40, heard);
    };
    step(&engine, None, &mut heard);
    let from = heard.len();
    step(&engine, Some(Command::Pause), &mut heard);
    step(&engine, Some(Command::Play), &mut heard);
    step(&engine, Some(Command::Seek(1.0)), &mut heard);
    step(&engine, Some(Command::Stop), &mut heard);
    step(&engine, Some(Command::Play), &mut heard);
    load(&engine, &dir.join("b.wav"), 2);
    step(&engine, None, &mut heard);
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "the output callback allocated"
    );
    // a 440 Hz sine of 0.24 moves at most 0.014 a sample; a cut is a step of
    // up to 0.24
    let worst = heard[from..]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 0.03, "a click: a step of {worst}");
    assert!(heard[from..].iter().any(|s| s.abs() > 0.1), "nothing heard");
}
