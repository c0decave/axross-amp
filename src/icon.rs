//! The program's icon (packaging/axross-amp.svg) drawn at a size, for the
//! window's own (`_NET_WM_ICON` on X11, the taskbar's elsewhere): the
//! rounded square, its rim, the five bars each in its gradient, their
//! peaks - the same shapes, a few lines of code rather than an SVG
//! renderer. (There was none: review 12.)

/// The SVG's units: a 128 x 128 square.
const UNITS: f32 = 128.0;

/// A rounded rectangle (x, y, width, height, radius) holds a point.
fn inside(x: f32, y: f32, (rx, ry, w, h, r): (f32, f32, f32, f32, f32)) -> bool {
    if x < rx || y < ry || x > rx + w || y > ry + h {
        return false;
    }
    let cx = x.clamp(rx + r, rx + w - r);
    let cy = y.clamp(ry + r, ry + h - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn rgb(hex: u32) -> [f32; 3] {
    [(hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff].map(|c| c as f32 / 255.0)
}

/// The colour and coverage (0 - 1) of the icon at a point (SVG units).
fn sample(x: f32, y: f32) -> ([f32; 3], f32) {
    let mut colour = [0.0; 3];
    let mut alpha = 0.0;
    let mut over = |c: [f32; 3], a: f32| {
        colour = mix(colour, c, a);
        alpha = alpha + a * (1.0 - alpha);
    };
    if inside(x, y, (4.0, 4.0, 120.0, 120.0, 26.0)) {
        over(rgb(0x0d1117), 1.0);
    }
    // (the rim: a 3-unit stroke, 0.6 opaque)
    if inside(x, y, (2.5, 2.5, 123.0, 123.0, 27.5)) && !inside(x, y, (5.5, 5.5, 117.0, 117.0, 24.5))
    {
        over(rgb(0x4c9aff), 0.6);
    }
    // the bars, each in its own gradient from its foot to its top
    for (bx, top, peak) in [
        (22.0, 70.0, 62.0),
        (40.0, 46.0, 38.0),
        (58.0, 28.0, 20.0),
        (76.0, 54.0, 46.0),
        (94.0, 38.0, 30.0),
    ] {
        let height = 106.0 - top;
        if inside(x, y, (bx, top, 12.0, height, 3.0)) {
            let t = (106.0 - y) / height;
            let c = if t < 0.55 {
                mix(rgb(0x2ecc71), rgb(0x4c9aff), t / 0.55)
            } else {
                mix(rgb(0x4c9aff), rgb(0xa5d6ff), (t - 0.55) / 0.45)
            };
            over(c, 1.0);
        }
        if inside(x, y, (bx, peak, 12.0, 4.0, 2.0)) {
            over([1.0; 3], 1.0);
        }
    }
    (colour, alpha)
}

/// The icon `size` pixels square, RGBA (straight alpha), each pixel the
/// mean of 4 x 4 samples.
pub fn rgba(size: u32) -> Vec<u8> {
    let scale = UNITS / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut c, mut a) = ([0.0f32; 3], 0.0f32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (px as f32 + (sx as f32 + 0.5) / 4.0) * scale;
                    let y = (py as f32 + (sy as f32 + 0.5) / 4.0) * scale;
                    let (colour, alpha) = sample(x, y);
                    for i in 0..3 {
                        c[i] += colour[i] * alpha;
                    }
                    a += alpha;
                }
            }
            let rgb = c.map(|v| if a > 0.0 { v / a } else { 0.0 });
            out.extend(rgb.map(|v| (v * 255.0).round() as u8));
            out.push((a / 16.0 * 255.0).round() as u8);
        }
    }
    out
}

/// The windows' icon (drawn once).
pub fn window_icon() -> std::sync::Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<std::sync::Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let size = 64;
        std::sync::Arc::new(egui::IconData {
            rgba: rgba(size),
            width: size,
            height: size,
        })
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_drawn_as_its_svg() {
        let size = 128u32;
        let px = rgba(size);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // the corners outside the rounded square: clear
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(127, 127)[3], 0);
        // the ground: #0d1117
        assert_eq!(at(14, 110), [0x0d, 0x11, 0x17, 255]);
        // the middle bar: green at its foot, light blue at its top
        let foot = at(64, 104);
        assert!(foot[1] > foot[0] && foot[1] > 150, "{foot:?}");
        let top = at(64, 30);
        assert!(top[2] > 220 && top[0] > 130, "{top:?}");
        // its peak: white
        assert_eq!(at(64, 22), [255, 255, 255, 255]);
        assert_eq!(window_icon().rgba.len(), 64 * 64 * 4);
    }
}
