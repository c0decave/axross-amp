//! The real output callback (cpal, ALSA's `file` plugin into a null
//! device - no sound card needed) allocates nothing: a counting allocator
//! watches the audio thread between the callback's first and last line.

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::engine::{CALLBACK_HOOK, Command, Engine, Event};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static CALLBACKS: AtomicUsize = AtomicUsize::new(0);

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

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if watched() {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn hook(entering: bool) {
    if entering {
        CALLBACKS.fetch_add(1, Ordering::Relaxed);
    }
    let _ = WATCHED.try_with(|w| w.set(entering));
}

#[test]
fn the_real_callback_allocates_nothing() {
    let Some(corpus) = std::env::var_os("AMP_CORPUS").map(std::path::PathBuf::from) else {
        eprintln!("AMP_CORPUS not set: skipped");
        return;
    };
    let home = scratch::Scratch::new("rt-device");
    std::fs::write(
        home.join(".asoundrc"),
        format!(
            "pcm.!default {{\n type file\n slave.pcm \"null\"\n file \"{}\"\n format \"wav\"\n}}\n",
            home.join("capture.wav").display()
        ),
    )
    .unwrap();
    // SAFETY: the only test of this binary; set before ALSA reads its configuration
    unsafe { std::env::set_var("HOME", &home) };
    let _ = CALLBACK_HOOK.set(hook);
    let engine = Engine::start(|| {});
    engine.send(Command::Load {
        path: corpus.join("tone.flac").to_string_lossy().into_owned(),
        start_ms: 0,
        play: true,
        generation: 1,
    });
    let end = Instant::now() + Duration::from_secs(20);
    let mut ended = false;
    while Instant::now() < end && !ended {
        if let Ok(Event::Ended { .. }) = engine.events.recv_timeout(Duration::from_millis(100)) {
            ended = true;
        }
        // a pause, a seek and a resume on the way
        if CALLBACKS.load(Ordering::Relaxed) == 50 {
            engine.send(Command::Pause);
            engine.send(Command::Seek(1.0));
            engine.send(Command::Play);
        }
    }
    drop(engine);
    assert!(ended, "the track did not end");
    assert!(
        CALLBACKS.load(Ordering::Relaxed) > 10,
        "the callback did not run"
    );
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "the output callback allocated"
    );
}
