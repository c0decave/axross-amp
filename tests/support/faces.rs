//! The faces on their own: a window that is nothing but the picture, fed a
//! known signal at a steady 30 frames a second (the same numbers as
//! docs/axross_faces.py feeds axross's own faces), so pictures and costs
//! are the same each run.
#![allow(dead_code)]

use axross_amp::analyzer::Analyzer;
use axross_amp::state::VisualOptions;
use axross_amp::viz::{self, Frame, Overview, VizState};
use egui_kittest::Harness;

pub const RATE: u32 = 48_000;
pub const FPS: f32 = 30.0;

/// The signal: a chord over the whole range, a beat twice a second, a
/// little noise; stereo (the right a little different), `seconds` long.
pub fn signal(seconds: f32) -> Vec<f32> {
    let frames = (RATE as f32 * seconds) as usize;
    let mut seed = 7u32;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f64 / RATE as f64;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 9) as f64 / (1u32 << 23) as f64 - 0.5;
        let beat = (-(t * 2.0).rem_euclid(1.0) * 8.0).exp();
        let base: f64 = [55.0, 110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0, 7040.0]
            .iter()
            .enumerate()
            .map(|(k, hz)| (t * hz * std::f64::consts::TAU).sin() * 0.22 / ((k + 1) as f64).sqrt())
            .sum();
        let l = base * (0.6 + 0.4 * beat) + noise * 0.04;
        let r = base * 0.8 + (t * 330.0 * std::f64::consts::TAU).cos() * 0.15 + noise * 0.04;
        out.push(l.clamp(-1.0, 1.0) as f32);
        out.push(r.clamp(-1.0, 1.0) as f32);
    }
    out
}

pub struct FaceApp {
    pub analyzer: Analyzer,
    pub state: VizState,
    pub key: &'static str,
    pub scheme: String,
    pub options: VisualOptions,
    pub sound: Vec<f32>,
    /// Frames shown so far (the sound is at frame / FPS seconds).
    pub frame: usize,
    /// When set, the time a frame took to analyse and draw is not
    /// taken from the clock: `dt` is always 1 / FPS.
    pub overview: Option<Overview>,
    pub title: String,
    pub live: bool,
    /// wgpu's state, when the harness renders with it.
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
}

impl FaceApp {
    pub fn new(key: &'static str, seconds: f32) -> FaceApp {
        FaceApp {
            analyzer: Analyzer::new(),
            state: VizState::default(),
            key,
            scheme: "classic".into(),
            options: VisualOptions::default(),
            sound: signal(seconds),
            frame: 0,
            overview: None,
            title: "Afterglow – Demo Band".into(),
            live: true,
            render_state: None,
        }
    }

    /// The analysis of the next frame (a 30th of a second further on).
    pub fn step(&mut self) {
        self.frame += 1;
        let frames = self.sound.len() / 2;
        let end = (((self.frame as f32 / FPS) + 0.2) * RATE as f32) as usize;
        let end = end.min(frames);
        let start = end.saturating_sub(8192);
        let tap = &self.sound[start * 2..end * 2];
        self.analyzer.update(
            tap,
            end as u64,
            RATE,
            &self.options,
            1.0 / FPS,
            self.live,
            viz::needs(self.key),
        );
    }

    pub fn paint(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let scheme = viz::scheme(&self.scheme, &Default::default());
        let frame = Frame {
            painter: ui.painter(),
            rect,
            a: &self.analyzer,
            s: &scheme,
            dt: 1.0 / FPS,
            title: &self.title,
            overview: self.overview.as_ref(),
            position: self.frame as f32 / FPS,
            duration: 180.0,
            hover: None,
            opacity: 1.0,
            second: false,
            loudness_target: -23.0,
        };
        viz::draw(&frame, self.key, &mut self.state);
    }
}

impl eframe::App for FaceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.paint(ui));
    }
}

/// A harness that shows `key` at `size`, rendered by wgpu (`gpu`: its
/// shaders set up, else the CPU draws them).
pub fn harness(
    key: &'static str,
    size: egui::Vec2,
    gpu: bool,
    seconds: f32,
) -> Harness<'static, FaceApp> {
    let mut builder = Harness::builder().with_size(size);
    builder = builder.wgpu();
    builder.build_eframe(move |cc| {
        let mut app = FaceApp::new(key, seconds);
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            viz::gpu::install(rs);
            app.render_state = Some(rs.clone());
        }
        // (by the shaders - on lavapipe here - or as the CPU draws them, in
        // step with the frame and at half the resolution)
        app.state.set_gpu(gpu);
        app.state.gpu.sync = true;
        app.state.gpu.cpu_scale = Some(0.5);
        app
    })
}

/// A harness without a renderer (the CPU's cost alone).
pub fn harness_cpu(key: &'static str, size: egui::Vec2, seconds: f32) -> Harness<'static, FaceApp> {
    Harness::builder().with_size(size).build_eframe(move |_| {
        // (the shader faces' CPU pictures made on their own thread, as in
        // the player: what is counted is the window's own work)
        FaceApp::new(key, seconds)
    })
}

/// One frame: the analysis, then egui's step.
pub fn frame(h: &mut Harness<'static, FaceApp>) {
    h.state_mut().step();
    h.step();
}
