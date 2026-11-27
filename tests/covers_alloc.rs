//! A cover made for a track costs the frame nothing: it is made once, on a
//! thread of its own; asking for one kept and looking for one made
//! allocate nothing; and a frame showing it allocates no more than one
//! showing a track's own cover - counted by a global allocator.

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup};
use axross_amp::covers;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// (on this thread's work only: the others - the cover's maker - are idle
/// while counted)
fn counted<R>(f: impl FnOnce() -> R) -> (R, usize) {
    COUNT.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let r = f();
    COUNTING.store(false, Ordering::Relaxed);
    (r, COUNT.load(Ordering::Relaxed))
}

fn window(dir: &std::path::Path) -> egui_kittest::Harness<'static, App> {
    let setup = Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: true,
    };
    egui_kittest::Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .build_eframe(move |cc| App::with_setup(cc, Vec::new(), setup.clone()))
}

// (one test: the counter is the whole program's)
#[test]
fn a_cover_made_for_a_track_costs_the_frame_nothing() {
    // the worker: asked once, made once; then looked for and asked again
    // without a byte
    let mut c = covers::Covers::start(None, || {});
    let seed = covers::seed("/m/x.flac", "X");
    assert!(c.get(seed).is_none());
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while c.poll().is_none() {
        assert!(std::time::Instant::now() < end, "never made");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (_, polled) = counted(|| (0..100).filter(|_| c.poll().is_some()).count());
    assert_eq!(polled, 0, "looking for a cover made allocates");
    let (kept, got) = counted(|| c.get(seed).is_some());
    assert!(kept);
    assert_eq!(got, 0, "a cover kept, asked for again, allocates");

    // frames: a cover made for the track against the track's own
    let home = player::Scratch::new("covers-alloc");
    let mut frames = Vec::new();
    for made in [true, false] {
        let mut h = window(home.path());
        h.state_mut().set_mode("off");
        h.state_mut().pose(
            vec![axross_amp::state::Track {
                path: "/m/Album/01.flac".into(),
                title: "T".into(),
                album: "Album".into(),
                ..Default::default()
            }],
            0,
            200.0,
            1.0,
        );
        let art = (!made).then(|| {
            let a = covers::art(5, 99);
            std::sync::Arc::new(a)
        });
        let ctx = h.ctx.clone();
        h.state_mut().show_info(
            &ctx,
            axross_amp::engine::TrackInfo {
                art,
                album: "Album".into(),
                ..Default::default()
            },
        );
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !h.state().has_cover() {
            assert!(std::time::Instant::now() < end, "no cover");
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // (its crossfade over, the sizes made)
        for _ in 0..40 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let ((), n) = counted(|| h.run_steps(30));
        frames.push(n);
    }
    let (made, own) = (frames[0], frames[1]);
    println!("30 frames: {made} allocations with a cover made, {own} with its own");
    // (a few a frame at most: what egui's texts and shapes take differ
    // by the picture's size - review of 0.17.1, L2: the bound was 65 a
    // frame)
    assert!(
        made <= own + 3 * 30,
        "a cover made costs the frames: {made} against {own}"
    );

    // a title moving: what a frame of it allocates over one that fits - its
    // laid-out text looked up (a String and a job, as every label's), the
    // gap's - a handful, no more
    let mut frames = Vec::new();
    for title in ["Short", TITLE] {
        let mut h = window(home.path());
        h.state_mut().set_mode("off");
        h.state_mut().pose(
            vec![axross_amp::state::Track {
                path: "/m/x.flac".into(),
                title: title.into(),
                ..Default::default()
            }],
            0,
            200.0,
            1.0,
        );
        // (past its wait: moving)
        h.run_steps(60);
        let ((), n) = counted(|| h.run_steps(30));
        frames.push(n);
    }
    let per_frame = (frames[1] as f64 - frames[0] as f64) / 30.0;
    println!("a moving title: {per_frame:.1} allocations a frame more ({frames:?})");
    assert!(per_frame <= 12.0, "{per_frame} a frame: {frames:?}");
}

const TITLE: &str =
    "'Back To The 80's' _ Best of Synthwave And Retro Electro Music Mix for 2 Hours _ Vol. 5";
