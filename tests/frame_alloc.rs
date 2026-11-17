//! A frame allocates nothing of its own: the analysis none at all, a face
//! only what egui itself needs for its shapes and texts (and the same few
//! whatever the bar count) - counted by a global allocator around them.

#[path = "support/faces.rs"]
mod faces;

use axross_amp::analyzer::{Analyzer, Needs};
use axross_amp::state::VisualOptions;
use axross_amp::viz::{self, FACES, Frame, Group, VizState};
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

fn counted<R>(f: impl FnOnce() -> R) -> (R, usize) {
    COUNT.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let r = f();
    COUNTING.store(false, Ordering::Relaxed);
    (r, COUNT.load(Ordering::Relaxed))
}

// (one test: the counter is the whole program's)
#[test]
fn a_frame_allocates_nothing_of_its_own() {
    let sound = faces::signal(6.0);
    // the analysis: nothing at all, with every part of it on
    let options = VisualOptions {
        bars: 128,
        ..Default::default()
    };
    let mut a = Analyzer::new();
    let needs = Needs { fine: true };
    let window = |k: usize| {
        let end = 8192 + k * 800;
        (&sound[(end - 8192) * 2..end * 2], end as u64)
    };
    for k in 0..90 {
        let (tap, end) = window(k);
        a.update(tap, end, 48_000, &options, 1.0 / 60.0, true, needs);
    }
    for k in 90..240 {
        let (tap, end) = window(k);
        let ((), n) = counted(|| a.update(tap, end, 48_000, &options, 1.0 / 60.0, true, needs));
        assert_eq!(n, 0, "the analysis allocated {n} times in frame {k}");
    }

    // the faces: the meshes are kept; what is left is egui's (a shape's
    // Arc, a text's layout) - a handful, the same for 32 and 128 bars
    let ctx = egui::Context::default();
    let mut report = Vec::new();
    let mut most = 0;
    for face in FACES.iter().filter(|f| f.group != Group::Off) {
        let mut counts = Vec::new();
        for bars in [32, 128] {
            // (the shaders' faces as on a GPU: a paint callback a frame; the
            // CPU's pictures where there is none are made anew each frame)
            let mut state = VizState::default();
            state.set_gpu(true);
            let mut a = Analyzer::new();
            let options = VisualOptions {
                bars,
                ..Default::default()
            };
            let scheme = viz::scheme("classic", &Default::default());
            let overview = viz::Overview {
                columns: vec![[-0.5, 0.5, 0.3]; 900],
                done: 1.0,
            };
            let mut worst = 0;
            for k in 0..40 {
                let (tap, end) = window(k);
                a.update(
                    tap,
                    end,
                    48_000,
                    &options,
                    1.0 / 30.0,
                    true,
                    viz::needs(face.key),
                );
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(620.0, 300.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    let frame = Frame {
                        painter: ui.painter(),
                        rect: ui.max_rect(),
                        a: &a,
                        s: &scheme,
                        dt: 1.0 / 30.0,
                        title: "Afterglow – Demo Band",
                        overview: Some(&overview),
                        position: 30.0,
                        duration: 180.0,
                        hover: None,
                        opacity: 1.0,
                        second: false,
                        loudness_target: -23.0,
                    };
                    let ((), n) = counted(|| viz::draw(&frame, face.key, &mut state));
                    if k >= 20 {
                        worst = worst.max(n);
                    }
                });
                output.textures_delta.clear();
            }
            counts.push(worst);
        }
        report.push(format!(
            "{:<13} {:>4} allocations a frame (32 bars), {:>4} (128 bars)",
            face.key, counts[0], counts[1]
        ));
        most = most.max(counts[1]);
        // not one a bar, a point, a star
        assert!(
            counts[1] <= counts[0] + 4,
            "{}: {} with 32 bars, {} with 128",
            face.key,
            counts[0],
            counts[1]
        );
    }
    println!("{}", report.join("\n"));
    assert!(most <= 120, "{most} allocations in a frame");
}
