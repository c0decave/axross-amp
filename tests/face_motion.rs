//! How the newer faces move (review 18): the Spectral group calmer in Calm
//! and calmer still in Reduced motion (M2); the Plasma globe's filaments
//! coming and going without a jump (L1); the Strange attractor whole in a
//! loud passage, on a light ground too (M5); the bouncing balls never at
//! the picture's top in a row (L2); the Pitch spiral's names whole within
//! the picture (L4).
#[path = "support/faces.rs"]
mod faces;
#[path = "support/song.rs"]
mod song;

use axross_amp::analyzer::guard::Motion;
use axross_amp::viz::{FACES, Group};
use egui::vec2;

/// The mean difference of two pictures' pixels (0..255).
fn diff(a: &image::RgbaImage, b: &image::RgbaImage) -> f32 {
    let mut s = 0u64;
    for (p, q) in a.pixels().zip(b.pixels()) {
        for c in 0..3 {
            s += (p[c] as i32 - q[c] as i32).unsigned_abs() as u64;
        }
    }
    s as f32 / (a.width() * a.height() * 3) as f32
}

fn light(h: &mut egui_kittest::Harness<'static, faces::FaceApp>) {
    let app = h.state_mut();
    app.scheme = "custom".into();
    for (c, v) in [
        ("background", "#ffffff"),
        ("low", "#000000"),
        ("high", "#000000"),
        ("peak", "#000000"),
    ] {
        app.custom.insert(c.into(), v.into());
    }
}

/// How much a face moves through the song's two choruses in `motion`: the
/// mean change of a pixel from one frame to the next.
fn moves(key: &'static str, motion: Motion) -> f32 {
    let mut h = faces::harness(key, vec2(320.0, 180.0), true, 1.0);
    h.state_mut().sound = song::song("song");
    h.state_mut().analyzer.motion = motion;
    let mut last: Option<image::RgbaImage> = None;
    let (mut sum, mut n) = (0.0, 0);
    for f in 1..=170usize {
        faces::frame(&mut h);
        if f >= 90 {
            let shot = h.render().unwrap();
            if let Some(l) = &last {
                sum += diff(l, &shot);
                n += 1;
            }
            last = Some(shot);
        }
    }
    sum / n as f32
}

#[test]
fn the_spectral_faces_are_calmer_in_calm_and_in_reduced_motion() {
    let mut bad = Vec::new();
    for face in FACES.iter().filter(|f| f.group == Group::Spectral) {
        let [intense, calm, reduced] =
            [Motion::Intense, Motion::Calm, Motion::Reduced].map(|m| moves(face.key, m));
        println!(
            "{:<12} intense {intense:.3}  calm {calm:.3}  reduced {reduced:.3}",
            face.key
        );
        // (as fast in Reduced as in Intense, all of them, before: within
        // 1 - 7 % for seven of the nine)
        if calm > intense * 0.93 || reduced > calm * 0.85 || reduced > intense * 0.7 {
            bad.push(format!(
                "{}: intense {intense:.3}, calm {calm:.3}, reduced {reduced:.3}",
                face.key
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The Contour map's hills followed the bands at once whatever the motion
/// setting: Calm moved 0.99 of what Intense does, Reduced 0.82 (review 19,
/// L5; the other nineteen of the newer faces 0.70 - 0.91 and 0.46 - 0.61).
#[test]
fn the_contour_map_is_calmer_in_calm_and_in_reduced_motion() {
    let [intense, calm, reduced] =
        [Motion::Intense, Motion::Calm, Motion::Reduced].map(|m| moves("contours", m));
    println!("contours intense {intense:.3}  calm {calm:.3}  reduced {reduced:.3}");
    assert!(
        calm < intense * 0.9 && reduced < calm * 0.85 && reduced < intense * 0.65,
        "intense {intense:.3}, calm {calm:.3}, reduced {reduced:.3}"
    );
}

#[test]
fn the_plasma_globes_filaments_come_and_go_without_a_jump() {
    let mut h = faces::harness("filaments", vec2(620.0, 300.0), true, 1.0);
    h.state_mut().sound = song::song("song");
    let mut last: Option<image::RgbaImage> = None;
    let mut rows = Vec::new();
    for f in 1..=330usize {
        faces::frame(&mut h);
        let shot = h.render().unwrap();
        if let Some(l) = &last
            && f > 30
        {
            rows.push((diff(l, &shot), f));
        }
        last = Some(shot);
    }
    let mut sorted: Vec<f32> = rows.iter().map(|r| r.0).collect();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let (most, at) = rows
        .iter()
        .cloned()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap();
    println!(
        "median change {median:.3}, most {most:.3} at frame {at}: {:.1} x",
        most / median
    );
    // (5.2 - 7.6 times the median wherever the filaments' number changed;
    // no other face of the twenty above 3.6)
    assert!(
        most <= median * 4.0,
        "a jump at frame {at}: {most:.3}, {:.1} times the median {median:.3}",
        most / median
    );
}

/// The share of the picture that is not its ground (by more than a tenth
/// of the way from white to black).
fn inked(shot: &image::RgbaImage) -> f32 {
    let n = shot
        .pixels()
        .filter(|p| (765 - p[0] as i32 - p[1] as i32 - p[2] as i32) > 77)
        .count();
    n as f32 / (shot.width() * shot.height()) as f32
}

#[test]
fn the_strange_attractor_stays_whole_through_whole_songs() {
    // black on white: what the orbit draws is all there is to see. Five
    // kinds of music, each three times over (51 s, 39 s the first: several
    // phrases, several changes of shape), looked at twice a second. (It
    // fell to a dozen blobs in a loud chorus - review 18 - and, that
    // mended for the three and a half seconds a test read, to hairlines
    // and dots for seconds between any two phrases, the picture 1 - 5 %
    // inked where the whole figure is a quarter: review 19, M1.)
    let mut bad = Vec::new();
    for kind in ["song", "groove", "dnb", "loud", "voice"] {
        let one = if kind == "song" {
            song::song("song")
        } else {
            song::other(kind)
        };
        let music = [one.clone(), one.clone(), one].concat();
        let frames = music.len() / 2 / 1600 - 10;
        let mut h = faces::harness("attractor", vec2(620.0, 300.0), true, 1.0);
        light(&mut h);
        h.state_mut().sound = music;
        let mut ink = Vec::new();
        for f in 1..=frames {
            faces::frame(&mut h);
            if f >= 60 && f % 15 == 0 {
                ink.push((inked(&h.render().unwrap()), f));
            }
        }
        let mut sorted: Vec<f32> = ink.iter().map(|i| i.0).collect();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        let (least, at) = ink
            .iter()
            .cloned()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        println!(
            "{kind:<7} inked: median {median:.3}, least {least:.3} at {:.1} s",
            at as f32 / 30.0
        );
        // a figure all the while: never under a third of what it mostly
        // is, nor under a twentieth of the picture
        if least < median * 0.33 || least < 0.05 {
            bad.push(format!(
                "{kind}: {least:.3} of the picture inked at {:.1} s (the median {median:.3})",
                at as f32 / 30.0
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_bouncing_balls_are_never_in_a_row_at_the_top() {
    // (thrown to the picture's top whatever the band's level, the treble's
    // caps stood there in a line over bars a third as high)
    let mut h = faces::harness("bouncing", vec2(620.0, 300.0), true, 1.0);
    h.state_mut().sound = song::song("song");
    let ground = {
        faces::frame(&mut h);
        *h.render().unwrap().get_pixel(310, 6)
    };
    let mut most = 0.0f32;
    for f in 2..=330usize {
        faces::frame(&mut h);
        if f % 3 != 0 {
            continue;
        }
        let shot = h.render().unwrap();
        // the picture's top tenth: how much of its width holds anything
        let mut columns = 0;
        for x in 0..shot.width() {
            let any = (0..30).any(|y| {
                let p = shot.get_pixel(x, y);
                (0..3).any(|c| (p[c] as i32 - ground[c] as i32).abs() > 40)
            });
            columns += any as u32;
        }
        most = most.max(columns as f32 / shot.width() as f32);
    }
    println!("at most {most:.2} of the top's width holds a ball");
    assert!(most < 0.25, "balls along the top: {most:.2} of its width");
}

#[test]
fn the_pitch_spirals_names_are_whole_within_the_picture() {
    for size in [vec2(620.0, 300.0), vec2(240.0, 135.0), vec2(300.0, 300.0)] {
        let mut h = faces::harness("spiral", size, true, 1.0);
        light(&mut h);
        h.state_mut().sound = song::song("song");
        for _ in 0..60 {
            faces::frame(&mut h);
        }
        let shot = h.render().unwrap();
        // nothing drawn in the picture's outermost rows and columns (a
        // name cut by the edge reaches them; the picture's rounded corners
        // aside)
        let (w, ht) = (shot.width(), shot.height());
        let mut touched = 0;
        for x in w / 5..w * 4 / 5 {
            for y in [0, 1, 2, ht - 3, ht - 2, ht - 1] {
                touched += (inked_at(&shot, x, y)) as u32;
            }
        }
        for y in ht / 5..ht * 4 / 5 {
            for x in [0, 1, 2, w - 3, w - 2, w - 1] {
                touched += (inked_at(&shot, x, y)) as u32;
            }
        }
        assert_eq!(
            touched, 0,
            "{size:?}: the picture's edge cuts what is drawn"
        );
    }
}

fn inked_at(shot: &image::RgbaImage, x: u32, y: u32) -> bool {
    let p = shot.get_pixel(x, y);
    (765 - p[0] as i32 - p[1] as i32 - p[2] as i32) > 150
}

/// A face in Paper or in black on white through `music`: its picture at
/// `at` frames (620 x 300), and how much it moves from frame to frame in
/// the second before (the mean change of a pixel).
fn pictured(key: &'static str, scheme: &str, music: &[f32], at: usize) -> (image::RgbaImage, f32) {
    if scheme == "paper" {
        axross_amp::theme::set("paper");
    }
    let mut h = faces::harness(key, vec2(620.0, 300.0), true, 1.0);
    match scheme {
        "paper" => h.state_mut().scheme = "theme".into(),
        "wb" => light(&mut h),
        other => h.state_mut().scheme = other.into(),
    }
    h.state_mut().sound = music.to_vec();
    let mut last: Option<image::RgbaImage> = None;
    let (mut sum, mut n) = (0.0, 0);
    for f in 1..=at {
        faces::frame(&mut h);
        if f + 30 >= at {
            let shot = h.render().unwrap();
            if let Some(l) = &last {
                sum += diff(l, &shot);
                n += 1;
            }
            last = Some(shot);
        }
    }
    axross_amp::theme::set("midnight");
    (last.unwrap(), sum / n.max(1) as f32)
}

/// The RMS contrast of a picture: the standard deviation of its pixels'
/// luma (0..255).
fn contrast(shot: &image::RgbaImage) -> f32 {
    let luma: Vec<f32> = shot
        .pixels()
        .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
        .collect();
    let mean = luma.iter().sum::<f32>() / luma.len() as f32;
    (luma.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / luma.len() as f32).sqrt()
}

/// Review 19, L4: the faces weak beside the best, each lifted - and held
/// there. The Water surface in Paper was a pale sheet, all but blank (the
/// lowest contrast of all the newer faces); the Contour map in black on
/// white one grey slab; the Circular waveform in a loud passage nearly a
/// plain circle - the quiet arpeggio moved it more; Cymatics moved no
/// more with the song than it does in silence.
#[test]
fn the_faces_weak_beside_the_best_stay_lifted() {
    let song = song::other("groove");
    let mut said = Vec::new();
    let mut bad = Vec::new();
    let mut hold = |what: &str, value: f32, least: f32| {
        said.push(format!("{what}: {value:.2} (at least {least})"));
        if value < least {
            bad.push(format!("{what}: {value:.2}, not {least} or more"));
        }
    };
    // the water in Paper, at three moments of the song
    let water = [150, 255, 330]
        .map(|at| contrast(&pictured("water", "paper", &song, at).0))
        .iter()
        .sum::<f32>()
        / 3.0;
    hold("water, Paper: contrast", water, WATER_PAPER);
    // the map in black on white: how much of it is pale land (lighter
    // than a middle grey by a fifth of the way to white)
    let map = [150, 255, 330]
        .map(|at| {
            let shot = pictured("contours", "wb", &song, at).0;
            shot.pixels().filter(|p| p[1] > 153).count() as f32
                / (shot.width() * shot.height()) as f32
        })
        .iter()
        .sum::<f32>()
        / 3.0;
    hold("contours, black on white: pale land", map, CONTOURS_WB);
    // the ring in the loud drop (10 - 13 s of the song) against the quiet
    // arpeggio (its first two seconds)
    let quiet = pictured("circlewave", "classic", &song, 60).1;
    let loud = pictured("circlewave", "classic", &song, 345).1;
    hold(
        "circlewave: loud against quiet",
        loud / quiet.max(1e-3),
        RING_LOUD,
    );
    // the plate with a kick against the plate in silence
    let kick = pictured("cymatics", "classic", &song, 255).1;
    let silent = pictured("cymatics", "classic", &song::song("silence"), 255).1;
    hold(
        "cymatics: the song against silence",
        kick / silent.max(1e-3),
        PLATE_SONG,
    );
    // the radar with a song against the radar in silence (review 20, L8:
    // it swept the same with music and without)
    let swept = pictured("radar", "classic", &song, 255).1;
    let idle = pictured("radar", "classic", &song::song("silence"), 255).1;
    hold(
        "radar: the song against silence",
        swept / idle.max(1e-3),
        RADAR_SONG,
    );
    println!("{said:#?}");
    assert!(bad.is_empty(), "{bad:#?}");
}

// (before: 8.75, 0.09, 1.41, 2.48; after: 13.5, 0.46, 1.67, 4.09)
const WATER_PAPER: f32 = 11.5;
const CONTOURS_WB: f32 = 0.3;
const RING_LOUD: f32 = 1.55;
const PLATE_SONG: f32 = 3.3;
// (the radar: 1.2 before)
const RADAR_SONG: f32 = 1.7;
