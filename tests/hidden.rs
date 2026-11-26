//! A minimised or covered window: eframe then calls only `App::logic`, never
//! `App::ui`. The player must go on all the same - the next track after
//! one that ended or failed, the desktop's media keys, the sleep timer, the
//! state saved. Here `ui` is never called at all.

#[path = "support/player.rs"]
mod player;

use axross_amp::app::{App, Setup, Sleep};
use axross_amp::engine::RenderState;
use eframe::App as _;
use player::*;
use std::time::{Duration, Instant};

struct Hidden {
    ctx: egui::Context,
    frame: eframe::Frame,
    app: App,
    output: RenderState,
}

impl Hidden {
    fn new(dir: &std::path::Path, args: Vec<String>) -> Hidden {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let app = App::with_setup(
            &cc,
            args,
            Setup {
                config_dir: dir.join("config"),
                desktop: false,
                desktop_bus: None,
                headless_audio: true,
                stand_in_covers: false,
            },
        );
        Hidden {
            ctx,
            frame: eframe::Frame::_new_kittest(),
            app,
            output: RenderState::default(),
        }
    }

    /// `secs` of the window hidden: only logic, and the sound card taking sound.
    fn run(&mut self, secs: f32) {
        let end = Instant::now() + Duration::from_secs_f32(secs);
        let mut buf = vec![0.0f32; 480 * 2];
        while Instant::now() < end {
            self.app
                .engine()
                .shared
                .render(&mut buf, 2, &mut self.output);
            let (app, frame) = (&mut self.app, &mut self.frame);
            let _ = self
                .ctx
                .run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, frame));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn a_hidden_window_plays_on_and_its_timer_fires() {
    let dir = folder("hidden");
    // a track, one that cannot be played, another: the list goes on by itself
    let mut files = tracks(&dir, &["1.wav"], 0.4);
    let bad = dir.join("2.mp3");
    std::fs::write(&bad, b"not audio at all").unwrap();
    files.push(bad.to_string_lossy().into_owned());
    files.extend(tracks(&dir, &["3.wav"], 3.0));
    let mut h = Hidden::new(&dir, files);
    h.run(1.5);
    assert_eq!(h.app.snapshot().current, 2, "{}", h.app.status());
    assert!(h.app.playing());
    // the sleep timer fires with nobody looking
    h.app
        .set_sleep(Some(Sleep::At(Instant::now() + Duration::from_millis(300))));
    h.run(0.6);
    assert!(!h.app.playing(), "the sleep timer did not pause");
    // and the state is written (no frame of the window ever drawn)
    h.run(3.5);
    let saved = axross_amp::state::read_state(&dir.join("config/media_player.json"));
    assert_eq!(saved.tracks.len(), 3);
    assert_eq!(saved.current, 2);
}
