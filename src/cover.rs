//! The cover at the size it is shown. The GPU draws a texture made
//! smaller without mipmaps (egui's renderer makes none): a 1024 px cover
//! shown at 136 px aliases - stripes merge, edges step. So the cover is
//! made the size it is shown at (its pixels, not its points) here, on the
//! CPU with a filter that averages what it leaves out, square (cropped to
//! fill, not stretched), on a thread of its own; a few sizes are kept.

use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;

/// The cover's middle square made `side` pixels: cropped to fill, then
/// made smaller (or larger) by a Lanczos filter as wide as the step
/// (every source pixel counted: no aliasing).
pub fn square(rgba: &[u8], width: u32, height: u32, side: u32) -> egui::ColorImage {
    let side = side.max(1);
    let Some(image) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
        return egui::ColorImage::new(
            [side as usize, side as usize],
            vec![egui::Color32::BLACK; (side * side) as usize],
        );
    };
    let edge = width.min(height).max(1);
    let (x, y) = ((width - edge) / 2, (height - edge) / 2);
    let cropped = image::imageops::crop_imm(&image, x, y, edge, edge).to_image();
    let made = image::imageops::resize(&cropped, side, side, image::imageops::FilterType::Lanczos3);
    egui::ColorImage::from_rgba_unmultiplied([side as usize, side as usize], made.as_raw())
}

/// The sizes a cover is wanted at, made on a thread and kept (a few).
pub struct Sizer {
    source: Option<Arc<crate::engine::Art>>,
    generation: u64,
    made: Vec<(u32, egui::TextureHandle)>,
    asked: Vec<u32>,
    tx: Sender<(u64, u32, egui::ColorImage)>,
    rx: Receiver<(u64, u32, egui::ColorImage)>,
}

impl Default for Sizer {
    fn default() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Sizer {
            source: None,
            generation: 0,
            made: Vec::new(),
            asked: Vec::new(),
            tx,
            rx,
        }
    }
}

/// The sizes kept (the card's, the cover face's, the playlist's and the
/// title line's, one more).
const KEPT: usize = 5;

impl Sizer {
    /// A new cover (None: none): what was made of the last one goes.
    pub fn set(&mut self, art: Option<Arc<crate::engine::Art>>) {
        self.source = art;
        self.generation += 1;
        self.made.clear();
        self.asked.clear();
    }

    /// What the threads have made, as textures.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((generation, side, image)) = self.rx.try_recv() {
            self.asked.retain(|s| *s != side);
            if generation != self.generation {
                continue;
            }
            let texture =
                ctx.load_texture(format!("cover-{side}"), image, egui::TextureOptions::LINEAR);
            self.made.retain(|(s, _)| *s != side);
            self.made.insert(0, (side, texture));
            self.made.truncate(KEPT);
        }
    }

    /// The cover `side` pixels across (rounded up to 8): made already, or
    /// asked for now and meanwhile the nearest made (None: none yet).
    pub fn get(&mut self, side: f32) -> Option<&egui::TextureHandle> {
        let art = self.source.clone()?;
        let side = ((side.max(8.0) / 8.0).ceil() * 8.0) as u32;
        if let Some(k) = self.made.iter().position(|(s, _)| *s == side) {
            // (the most lately used first: it is kept)
            let hit = self.made.remove(k);
            self.made.insert(0, hit);
            return self.made.first().map(|(_, t)| t);
        }
        if !self.asked.contains(&side) {
            self.asked.push(side);
            let (tx, generation) = (self.tx.clone(), self.generation);
            let spawned = std::thread::Builder::new()
                .name("amp-cover".into())
                .spawn(move || {
                    let (w, h, rgba) = match &art.large {
                        Some((w, h, rgba)) => (*w, *h, rgba.as_slice()),
                        None => (art.width, art.height, art.rgba.as_slice()),
                    };
                    let _ = tx.send((generation, side, square(rgba, w, h, side)));
                });
            if spawned.is_err() {
                self.asked.retain(|s| *s != side);
            }
        }
        self.made
            .iter()
            .min_by_key(|(s, _)| s.abs_diff(side))
            .map(|(_, t)| t)
    }

    /// Something was asked for and is not made yet (a frame to come).
    pub fn waiting(&self) -> bool {
        !self.asked.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How much neighbouring pixels differ (the high frequencies' energy).
    fn roughness(image: &egui::ColorImage) -> f32 {
        let [w, h] = image.size;
        let mut sum = 0.0;
        for y in 0..h {
            for x in 1..w {
                let (a, b) = (image.pixels[y * w + x], image.pixels[y * w + x - 1]);
                sum += (a.r() as f32 - b.r() as f32).abs();
            }
        }
        sum / (w * h) as f32
    }

    #[test]
    fn a_fine_cover_made_small_is_smooth_not_aliased() {
        // stripes a pixel wide: at an eighth of the size they are grey -
        // sampled without a filter (as the GPU did without mipmaps) they
        // become coarse stripes and steps
        let n = 1024u32;
        let rgba: Vec<u8> = (0..n * n)
            .flat_map(|i| {
                let v = if (i % n).is_multiple_of(2) { 255 } else { 0 };
                [v, v, v, 255]
            })
            .collect();
        let small = square(&rgba, n, n, 128);
        assert!(roughness(&small) < 4.0, "{}", roughness(&small));
        let mean =
            small.pixels.iter().map(|p| p.r() as f32).sum::<f32>() / small.pixels.len() as f32;
        assert!((mean - 127.5).abs() < 20.0, "{mean}");
        // (what point sampling at 2.8 pixels a step gives: stripes)
        let picked: Vec<egui::Color32> = (0..128 * 128)
            .map(|i| {
                let (x, y) = (((i % 128) as f32 * 2.8) as u32 % n, (i / 128) as u32 * 8);
                let v = rgba[((y * n + x) * 4) as usize];
                egui::Color32::from_gray(v)
            })
            .collect();
        let aliased = egui::ColorImage::new([128, 128], picked);
        assert!(roughness(&aliased) > 40.0, "{}", roughness(&aliased));
    }

    #[test]
    fn a_cover_not_square_is_cropped_to_fill_not_stretched() {
        // 400 x 200, the left half red, the right blue: its middle square
        // (100..300) is half red, half blue - stretched, it would be the
        // whole picture squeezed
        let (w, h) = (400u32, 200u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if (i % w) < 200 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        let made = square(&rgba, w, h, 100);
        assert_eq!(made.size, [100, 100]);
        // the left quarter of the square is red, the right quarter blue,
        // and a circle would stay a circle (no stretch): the boundary in
        // the middle
        let at = |x: usize| made.pixels[50 * 100 + x];
        assert!(
            at(10).r() > 200 && at(90).b() > 200,
            "{:?} {:?}",
            at(10),
            at(90)
        );
        assert!(at(45).r() > 128 && at(55).b() > 128);
    }
}
