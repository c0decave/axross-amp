//! The flash guard's veil stays off where nothing flashes - for every
//! face, on a light ground, in a small picture, through several kinds of
//! music: a song of ordinary music (a kick, a bass, a pad, hi-hats, a
//! melody panned to and fro, a quiet break; nothing gated) in the window's
//! Paper theme and in black on white at 240 x 135 and in Paper at 620 x
//! 300; and a second song (an arpeggio, a groove, a build with a snare
//! roll, a loud drop, a tail), drum and bass and speech in Paper and black
//! on white at 240 x 135, Sakura at 200 x 112 and a cream scheme of the
//! user's at 300 x 170; and all four in Midnight at 240 x 135. (Review 18,
//! M1: the guard's estimate held a dark wedge no face draws on every light
//! ground, and the veil - a blur of the estimate - showed it, a third of
//! the song over the Circular waveform.
//! Review 19, L3: the test held for its one song; with drum and bass
//! Synthwave was veiled a sixth of the track in Paper.) Where the veil
//! does come, the face's own picture is read with the guard off: a veil
//! over a picture whose strict reading is four transitions a second or
//! fewer fails.
#[path = "support/faces.rs"]
mod faces;
#[path = "support/flashmeter.rs"]
mod flashmeter;
#[path = "support/song.rs"]
mod song;

use axross_amp::viz::{FACES, Group};
use egui::vec2;

fn harness(
    key: &'static str,
    scheme: &str,
    size: egui::Vec2,
    music: &[f32],
) -> egui_kittest::Harness<'static, faces::FaceApp> {
    let mut h = faces::harness(key, size, true, 1.0);
    let app = h.state_mut();
    let custom: &[(&str, &str)] = match scheme {
        // (the window's theme: set by the caller, once for all its faces)
        "paper" | "sakura" | "daylight" | "midnight" => {
            app.scheme = "theme".into();
            &[]
        }
        "cream" => &[
            ("background", "#f4ecd8"),
            ("low", "#2a6f97"),
            ("high", "#c1121f"),
            ("peak", "#222222"),
        ],
        _ => &[
            ("background", "#ffffff"),
            ("low", "#000000"),
            ("high", "#000000"),
            ("peak", "#000000"),
        ],
    };
    if !custom.is_empty() {
        app.scheme = "custom".into();
        for (c, v) in custom {
            app.custom.insert((*c).into(), (*v).into());
        }
    }
    app.sound = music.to_vec();
    h
}

fn frames(music: &[f32]) -> usize {
    (music.len() / 2 / 1600).min(480)
}

/// The share of the music the veil is over the picture.
fn veiled(key: &'static str, scheme: &str, size: egui::Vec2, music: &[f32]) -> f32 {
    let mut h = harness(key, scheme, size, music);
    let mut veiled = 0;
    for _ in 0..frames(music) {
        faces::frame(&mut h);
        veiled += (h.state().state.flash.strength > 0.3) as usize;
    }
    veiled as f32 / frames(music) as f32
}

/// The face's own picture, the guard off: the most transitions in a second
/// by the strict reading.
fn strict(key: &'static str, scheme: &str, size: egui::Vec2, music: &[f32]) -> usize {
    let mut h = harness(key, scheme, size, music);
    h.state_mut().state.flash_off = true;
    let mut meter = flashmeter::Meter::with_fps(size.x as usize, size.y as usize, 30);
    for _ in 0..frames(music) {
        faces::frame(&mut h);
        meter.push(&h.render().unwrap());
    }
    meter.most_strict().0
}

/// The one veil known to be more than is needed (docs/LIMITATIONS.md): the
/// line in black on white through drum and bass - the guard reads the fill
/// under a line that crosses half the picture's height many times a
/// second as the area it could cover.
fn known(key: &str, scheme: &str, music: &str) -> bool {
    key == "line" && scheme == "wb" && music == "dnb"
}

/// (scheme, size, which of the musics - 0 the song, 1 the second song, 2
/// drum and bass, 3 speech)
type Case = (&'static str, egui::Vec2, &'static [usize]);

#[test]
fn the_veil_stays_off_on_light_grounds_in_small_pictures() {
    let failures = unjustified(&[
        ("paper", vec2(240.0, 135.0), &[0, 1, 2, 3]),
        ("paper", vec2(620.0, 300.0), &[0]),
        ("wb", vec2(240.0, 135.0), &[0, 1, 2, 3]),
        ("sakura", vec2(200.0, 112.0), &[1, 2, 3]),
        ("cream", vec2(300.0, 170.0), &[1, 2, 3]),
    ]);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// And on the dark ground the window has by default (Midnight), small:
/// found on the way - a thread of this test drew in Midnight where it
/// meant Paper - Synthwave veiled a fifth of the song there, the
/// Spirograph a fifth of drum and bass.
#[test]
fn the_veil_stays_off_on_the_dark_ground_in_a_small_picture() {
    let failures = unjustified(&[("midnight", vec2(240.0, 135.0), &[0, 1, 2, 3])]);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Review 20, L9: the guard's pattern stage - what softens regular lines -
/// was on in 85 - 90 % of the frames for the Flow field in Paper at 240 x
/// 135: all its hairs, as close as they lie in a small picture, were rows
/// of lines. Fewer of them are drawn there.
#[test]
fn the_flow_field_in_a_small_light_picture_is_not_softened_most_of_the_time() {
    let mut bad = Vec::new();
    for (name, music) in [
        ("song", song::song("song")),
        ("second song", song::other("groove")),
        ("dnb", song::other("dnb")),
    ] {
        axross_amp::theme::set("paper");
        let mut h = harness("flowfield", "paper", vec2(240.0, 135.0), &music);
        let mut softened = 0;
        for _ in 0..frames(&music) {
            faces::frame(&mut h);
            softened += (h.state().state.flash.pattern_cover() > 0.05) as usize;
        }
        axross_amp::theme::set("midnight");
        let share = softened as f32 / frames(&music) as f32;
        println!(
            "flowfield paper 240x135 {name}: softened {:.0} % of the frames",
            share * 100.0
        );
        if share > 0.4 {
            bad.push(format!(
                "{name}: softened {:.0} % of the frames",
                share * 100.0
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The cases in which the veil is over a face more than a twentieth of
/// the music while its own picture, the guard off, reads four transitions
/// a second or fewer.
fn unjustified(cases: &[Case]) -> Vec<String> {
    let only = std::env::var("AMP_VEIL_FACES").ok();
    let keys: Vec<&'static str> = FACES
        .iter()
        .filter(|f| f.group != Group::Off)
        .map(|f| f.key)
        .filter(|key| {
            only.as_deref()
                .is_none_or(|o| o.split(',').any(|k| k == *key))
        })
        .collect();
    let musics: Vec<(&'static str, Vec<f32>)> = vec![
        ("song", song::song("song")),
        ("second song", song::other("groove")),
        ("dnb", song::other("dnb")),
        ("voice", song::other("voice")),
    ];
    let failures = std::sync::Mutex::new(Vec::new());
    let threads: usize = std::env::var("AMP_VEIL_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    for &(scheme, size, which) in cases {
        // (the window's theme is a thread's own: set in each that draws)
        let theme = match scheme {
            "paper" | "sakura" | "daylight" => scheme,
            _ => "midnight",
        };
        let jobs: Vec<(&'static str, usize)> = keys
            .iter()
            .flat_map(|key| which.iter().map(move |m| (*key, *m)))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    axross_amp::theme::set(theme);
                    assert_eq!(axross_amp::theme::current().key, theme);
                    loop {
                        let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(key, m)) = jobs.get(at) else { break };
                        let (name, music) = (&musics[m].0, &musics[m].1);
                        let share = veiled(key, scheme, size, music);
                        if share <= 0.05 {
                            continue;
                        }
                        let read = strict(key, scheme, size, music);
                        let line = format!(
                            "{key} {scheme} {}x{} {name}: veiled {:.0} % of it, its own picture {read} transitions a second (strict)",
                            size.x,
                            size.y,
                            share * 100.0
                        );
                        println!("{line}");
                        if read <= 4 && !known(key, scheme, name) {
                            failures.lock().unwrap().push(line);
                        }
                    }
                });
            }
        });
    }
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    failures
}

/// To look at one case: AMP_VEIL_PROBE=face,scheme,WxH,music - a line a
/// half second (the veil's strength, the guard's count of transitions raw
/// and shown), pictures where the veil comes on.
#[test]
#[ignore]
fn probe() {
    let what = std::env::var("AMP_VEIL_PROBE").expect("AMP_VEIL_PROBE");
    let parts: Vec<&str> = what.split(',').collect();
    let key: &'static str = FACES.iter().find(|f| f.key == parts[0]).unwrap().key;
    let (w, t) = parts[2].split_once('x').unwrap();
    let size = vec2(w.parse().unwrap(), t.parse().unwrap());
    let music = match parts[3] {
        "song" => song::song("song"),
        other => song::other(other),
    };
    axross_amp::theme::set(match parts[1] {
        "paper" | "sakura" | "daylight" => parts[1],
        _ => "midnight",
    });
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/screenshots/veil");
    std::fs::create_dir_all(&dir).unwrap();
    if let Ok(warm) = std::env::var("AMP_VEIL_WARM") {
        for w in warm.split(',') {
            let m = if w == "song" {
                song::song("song")
            } else {
                song::other(w)
            };
            println!(
                "before it, {w}: veiled {:.2}",
                veiled(key, parts[1], size, &m)
            );
        }
    }
    println!("alone: veiled {:.2}", veiled(key, parts[1], size, &music));
    for off in [false, true] {
        let mut h = harness(key, parts[1], size, &music);
        h.state_mut().state.flash_off = off;
        if std::env::var_os("AMP_VEIL_CPU").is_some() {
            h.state_mut().state.set_gpu(false);
        }
        let mut meter = flashmeter::Meter::with_fps(size.x as usize, size.y as usize, 30);
        let mut was = false;
        for f in 1..=frames(&music) {
            faces::frame(&mut h);
            let shot = h.render().unwrap();
            meter.push(&shot);
            let g = &h.state().state.flash;
            let on = g.strength > 0.3;
            if !off && (on != was || f % 15 == 0) {
                println!(
                    "f{f:>3} veil {:.2} pattern {:.2} pressure {:?}",
                    g.strength,
                    g.pattern_cover(),
                    g.pressure()
                );
            }
            if (on && !was) || f % 60 == 0 {
                shot.save(dir.join(format!(
                    "{}-{}-{}-{f}.png",
                    parts[0],
                    parts[1],
                    if off { "off" } else { "on" }
                )))
                .unwrap();
            }
            was = on;
        }
        println!(
            "guard {}: transitions {:?}, strict {:?}",
            if off { "off" } else { "on" },
            meter.most(),
            meter.most_strict()
        );
    }
    axross_amp::theme::set("midnight");
}
