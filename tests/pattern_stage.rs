//! The flash guard's pattern stage on drawn frames: what no face foresaw -
//! a cover of checks laid over the picture, a custom scheme in stripes -
//! drawn, the guard over it, measured by the meter (ITU-R BT.1702: regular
//! stripes over a quarter of the picture moving, 40 % standing). Without
//! the guard they are a hazard; with it, not.

#[path = "support/flashmeter.rs"]
mod flashmeter;

use axross_amp::viz::Thumb;
use axross_amp::viz::flashguard::FlashGuard;
use egui::{Color32, Rect, pos2, vec2};
use egui_kittest::Harness;
use flashmeter::Meter;

const W: f32 = 384.0;
const H: f32 = 216.0;

#[derive(Clone, Copy, PartialEq)]
enum Content {
    /// A cover of checks (24 px), drifting a pixel every other frame.
    Checks,
    /// A scheme's two colours (white and black) in bands of 6 px, drifting.
    Stripes,
}

struct App {
    guard: FlashGuard,
    on: bool,
    content: Content,
    frame: usize,
    texture: Option<egui::TextureHandle>,
    thumb: Option<Thumb>,
}

fn checks() -> egui::ColorImage {
    let n = 384usize;
    let px = (0..n * n)
        .map(|i| {
            let (x, y) = (i % n, i / n);
            if ((x / 24) + (y / 24)) % 2 == 0 {
                Color32::WHITE
            } else {
                Color32::BLACK
            }
        })
        .collect();
    egui::ColorImage::new([n, n], px)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let rect = ui.max_rect();
                if self.texture.is_none() {
                    let image = checks();
                    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    self.thumb = Some(Thumb::from_rgba(image.size[0], image.size[1], &rgba));
                    self.texture = Some(ui.ctx().load_texture(
                        "checks",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                let painter = ui.painter();
                let layer = painter.layer_id();
                let first = ui
                    .ctx()
                    .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
                painter.rect_filled(rect, 0.0, Color32::BLACK);
                let shift = (self.frame / 2) as f32;
                let inner = rect.shrink(4.0);
                match self.content {
                    Content::Checks => {
                        let texture = self.texture.as_ref().unwrap();
                        let u = shift / 384.0;
                        painter.image(
                            texture.id(),
                            inner,
                            Rect::from_min_max(pos2(u, 0.0), pos2(u + 1.0, H / W)),
                            Color32::WHITE,
                        );
                    }
                    Content::Stripes => {
                        let mut x = inner.left() - 12.0 + shift % 12.0;
                        let mut white = true;
                        while x < inner.right() {
                            let band = Rect::from_min_max(
                                pos2(x.max(inner.left()), inner.top()),
                                pos2((x + 6.0).min(inner.right()), inner.bottom()),
                            );
                            if white && band.width() > 0.0 {
                                painter.rect_filled(band, 0.0, Color32::WHITE);
                            }
                            white = !white;
                            x += 6.0;
                        }
                    }
                }
                if self.on {
                    let texture = self
                        .texture
                        .as_ref()
                        .map(|t| t.id())
                        .zip(self.thumb.as_ref());
                    self.guard.guard(
                        painter,
                        first,
                        inner,
                        &[],
                        texture.filter(|_| self.content == Content::Checks),
                        1.0 / 60.0,
                    );
                }
            });
    }
}

/// The frames with a pattern hazard over two seconds.
fn hazards(content: Content, on: bool) -> (usize, f32) {
    let mut guard = FlashGuard::default();
    guard.screen_is = Some(vec2(W, H));
    let mut h = Harness::builder()
        .with_size(vec2(W, H))
        .wgpu()
        .build_eframe(move |_| App {
            guard,
            on,
            content,
            frame: 0,
            texture: None,
            thumb: None,
        });
    let mut meter = Meter::new(W as usize, H as usize);
    for f in 0..120 {
        h.state_mut().frame += 1;
        h.step();
        let img = h.render().unwrap();
        // (AMP_PATTERN_DUMP=dir: the 100th frame kept)
        if let Ok(dir) = std::env::var("AMP_PATTERN_DUMP")
            && f == 100
        {
            img.save(format!("{dir}/{}-{on}.png", content as u8))
                .unwrap();
        }
        meter.push(&img);
        if std::env::var_os("AMP_PATTERN_TRACE").is_some() && f % 10 == 0 {
            println!(
                "frame {f}: softened {:.2}, over {:.2} of it",
                h.state().guard.pattern_strength,
                h.state().guard.pattern_cover()
            );
        }
    }
    (meter.pattern_frames, h.state().guard.pattern_strength)
}

#[test]
fn what_no_face_foresaw_is_softened() {
    for content in [Content::Checks, Content::Stripes] {
        let (without, _) = hazards(content, false);
        let (with, strength) = hazards(content, true);
        println!(
            "{}: {without} frames a hazard without the guard, {with} with it (softened {strength:.2})",
            if content == Content::Checks {
                "checks"
            } else {
                "stripes"
            }
        );
        assert!(without > 60, "the test draws no hazard: {without}");
        // (the first frames, before it has seen a tenth of a second)
        assert!(with <= 10, "{with} frames still a hazard");
    }
}
