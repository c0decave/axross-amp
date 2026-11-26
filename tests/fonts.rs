//! The fonts (src/fonts.rs; review 19, M3): a damaged font file never
//! stops the start; a title in a script egui's own fonts cannot write is
//! written with a font found for it - off the window's thread, a box until
//! then; right-to-left text reads from the right.
//!
//! The scripts come from the tests' own font (tests/media/fonts, made by
//! make_font.py: a few characters of Japanese, Chinese, Korean, Hebrew and
//! Arabic, each a box of its own width), so the tests do not depend on what
//! fonts the machine has; one test asks the machine's own.

#[path = "support/scratch.rs"]
mod scratch;

use axross_amp::fonts::{self, Faces, Fallback};
use egui_kittest::Harness;
use std::path::PathBuf;
use std::sync::Arc;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/media/fonts/amp-fixture.ttf")
}

/// A window that shows `text`, boxes allowed (the harness panics on a
/// character no font has, else).
fn showing(text: &'static str) -> Harness<'static> {
    Harness::builder()
        .allow_missing_glyphs()
        .build_ui(move |ui| {
            ui.label(text);
        })
}

/// Frames until nothing is being looked for (the text laid out anew when a
/// font was found, as the player does each frame).
fn settle(h: &mut Harness<'_>, fallback: &Arc<Fallback>) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        h.run_steps(1);
        if fallback.take_fresh() {
            h.ctx.set_font_providers(vec![fallback.clone()]);
            continue;
        }
        if fallback.settled() {
            h.run_steps(2);
            if fallback.settled() {
                return;
            }
        }
        assert!(std::time::Instant::now() < end, "the fonts never settled");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_damaged_font_file_does_not_stop_the_start() {
    let dir = scratch::Scratch::new("fonts-damaged");
    let good = std::fs::read(fixture()).unwrap();
    let garbage = dir.join("garbage.ttf");
    std::fs::write(&garbage, vec![0x5au8; 20_000]).unwrap();
    let cut = dir.join("cut.ttf");
    std::fs::write(&cut, &good[..good.len() / 6]).unwrap();
    let empty = dir.join("empty.ttf");
    std::fs::write(&empty, b"").unwrap();
    let headless = dir.join("headless.ttf");
    let mut broken = good.clone();
    broken[..12].fill(0);
    std::fs::write(&headless, &broken).unwrap();
    let missing = dir.join("not-there.ttf");
    // none of them is taken for a font ...
    for path in [&garbage, &cut, &empty, &headless, &missing] {
        assert!(
            fonts::first_sound(&[path]).is_none(),
            "{} was taken for a font",
            path.display()
        );
    }
    // ... and the first sound file after them is (a system's bold face,
    // where there is one)
    let bold: Vec<PathBuf> = [&garbage, &cut, &empty, &headless, &missing]
        .into_iter()
        .cloned()
        .chain(fonts::BOLD_FONTS.iter().map(PathBuf::from))
        .collect();
    let faces = Faces::read(
        &bold,
        std::slice::from_ref(&garbage),
        std::slice::from_ref(&cut),
    );
    assert!(faces.serif.is_none() && faces.mono_bold.is_none());
    let has_bold = fonts::BOLD_FONTS
        .iter()
        .any(|p| std::path::Path::new(p).exists());
    assert_eq!(faces.bold.is_some(), has_bold);
    // whatever is handed to egui parses: faces made by hand of damaged
    // files (as if the files had been damaged after they were read) are
    // left out, and the window draws its titles
    for mono in [false, true] {
        for faces in [
            faces.clone(),
            Faces {
                bold: Some(Arc::new(std::fs::read(&garbage).unwrap())),
                serif: Some(Arc::new(std::fs::read(&cut).unwrap())),
                mono_bold: Some(Arc::new(broken.clone())),
            },
        ] {
            let definitions = faces.definitions(mono);
            assert!(
                !definitions.font_data.contains_key("axross-serif")
                    && !definitions.font_data.contains_key("axross-mono-bold")
            );
            let mut given = false;
            let mut h = Harness::builder().build_ui(move |ui| {
                if !std::mem::replace(&mut given, true) {
                    ui.ctx().set_fonts(definitions.clone());
                    return;
                }
                for family in ["bold", "serif"] {
                    ui.label(
                        egui::RichText::new("A title, drawn")
                            .family(egui::FontFamily::Name(family.into())),
                    );
                }
                ui.label("and plain words");
            });
            h.run_steps(3);
        }
    }
}

#[test]
fn a_title_in_another_script_is_written_with_a_font_found_for_it() {
    let text = "日本語の音楽 / 中文 / 한국어 / אבגד / بتث";
    let mut h = showing(text);
    let fallback = Fallback::start(&h.ctx, vec![fixture()], false);
    h.ctx.set_font_providers(vec![fallback.clone()]);
    // the first frame does not wait: the font is found on another thread
    let started = std::time::Instant::now();
    h.run_steps(1);
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
    settle(&mut h, &fallback);
    assert_eq!(fallback.missing(), Vec::<char>::new(), "boxes left");
    assert_eq!(fallback.found().len(), 1, "{:?}", fallback.found());
    // every character has its glyph now: drawn with boxes forbidden
    h.ctx
        .set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);
    h.run_steps(3);
    // a script no font here has stays boxes, and is said
    let mut h = showing("ไทย and 日本");
    let fallback = Fallback::start(&h.ctx, vec![fixture()], false);
    h.ctx.set_font_providers(vec![fallback.clone()]);
    settle(&mut h, &fallback);
    assert_eq!(fallback.missing(), vec!['ท', 'ย', 'ไ']);
    // a damaged file among those looked in is passed over
    let dir = scratch::Scratch::new("fonts-fallback");
    let garbage = dir.join("garbage.ttf");
    std::fs::write(&garbage, vec![0x5au8; 20_000]).unwrap();
    let mut h = showing("音楽");
    let fallback = Fallback::start(&h.ctx, vec![garbage, fixture()], false);
    h.ctx.set_font_providers(vec![fallback.clone()]);
    settle(&mut h, &fallback);
    assert!(fallback.missing().is_empty());
}

/// Where each character of `text` is drawn, left to right: the characters
/// in the order the eye meets them (as the player hands a line to egui:
/// through `bidi::seen`).
fn as_seen(h: &Harness<'_>, text: &str) -> String {
    let galley = h.ctx.fonts_mut(|f| {
        f.layout_no_wrap(
            axross_amp::bidi::seen(text).into_owned(),
            egui::FontId::proportional(20.0),
            egui::Color32::WHITE,
        )
    });
    // (a glyph of no width is the shaper's place-holder, not a letter)
    let mut glyphs: Vec<(f32, char)> = galley
        .rows
        .iter()
        .flat_map(|row| row.row.glyphs.iter())
        .filter(|g| g.advance_width > 0.0)
        .map(|g| (g.pos.x, g.chr))
        .collect();
    glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
    glyphs.into_iter().map(|g| g.1).collect()
}

#[test]
fn right_to_left_text_reads_from_the_right() {
    let mut h = showing("אבגד بتث");
    let fallback = Fallback::start(&h.ctx, vec![fixture()], false);
    h.ctx.set_font_providers(vec![fallback.clone()]);
    settle(&mut h, &fallback);
    for (text, seen) in [
        // Hebrew alone: its first letter at the right
        ("אבגד", "דגבא"),
        // two words: the first at the right (each was read rightly, in the
        // wrong order)
        ("אב גד", "דג בא"),
        // Arabic alone
        ("بتث", "ثتب"),
        // a Hebrew word in an English line: the line runs left to right,
        // the word right to left in it
        ("ab אבג cd", "ab גבא cd"),
        // a line that begins in Hebrew runs right to left: its Latin word
        // and its number keep their own direction
        ("אב cd גד", "דג cd בא"),
        ("אבג 12", "12 גבא"),
        ("אב (cd)", "(cd) בא"),
        // Latin as ever
        ("abc 12", "abc 12"),
    ] {
        assert_eq!(as_seen(&h, text), seen, "{text:?}");
    }
}

#[test]
fn the_systems_own_fonts_are_asked_off_the_windows_thread() {
    // what this machine can write beyond egui's fonts: where one of the
    // well-known files has Hebrew, the Hebrew is found (by the system's
    // own matching, or by the file's path) - and the frame that first
    // shows it does not wait for the system's list of fonts
    let hebrew = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
    ]
    .iter()
    .filter_map(|p| std::fs::read(p).ok())
    .any(|data| fonts::sound(&data, 0, Some('ש')));
    let mut h = showing("שלום");
    let fallback = Fallback::start(&h.ctx, Vec::new(), true);
    h.ctx.set_font_providers(vec![fallback.clone()]);
    let started = std::time::Instant::now();
    h.run_steps(1);
    let first = started.elapsed();
    assert!(
        first < std::time::Duration::from_millis(250),
        "the first frame with a Hebrew title took {first:?}"
    );
    settle(&mut h, &fallback);
    println!(
        "found {:?}, missing {:?}",
        fallback.found(),
        fallback.missing()
    );
    if hebrew {
        assert!(fallback.missing().is_empty(), "{:?}", fallback.missing());
        assert!(!fallback.found().is_empty());
    }
}

#[test]
fn the_player_writes_a_title_no_font_of_its_own_can() {
    // the tests' font named to the player as the user would name one
    // (AXROSS_AMP_FONTS: a folder), a track whose file is named in
    // Japanese and Hebrew: its title on the card and in the playlist
    // SAFETY: the one test of this file that starts the player; the others
    // hand their fonts over themselves and read no environment
    unsafe {
        std::env::set_var(
            "AXROSS_AMP_FONTS",
            fixture().parent().expect("the font's folder"),
        );
    }
    let dir = scratch::Scratch::new("fonts-player");
    let track = dir.join("日本語の音楽 אבגד.wav");
    // (a second of a tone)
    let mut wav = Vec::new();
    let n = 48_000u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + n * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&96_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 8000.0) as i16;
        wav.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&track, wav).unwrap();
    let setup = axross_amp::app::Setup {
        config_dir: dir.join("config"),
        desktop: false,
        desktop_bus: None,
        headless_audio: true,
        stand_in_covers: false,
    };
    let args = vec![track.to_string_lossy().into_owned()];
    let mut h = Harness::builder()
        .with_size(egui::vec2(620.0, 860.0))
        .with_step_dt(0.05)
        .allow_missing_glyphs()
        .build_eframe(move |cc| axross_amp::app::App::with_setup(cc, args.clone(), setup.clone()));
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        h.run_steps(2);
        std::thread::sleep(std::time::Duration::from_millis(10));
        if h.state().is_loaded() && h.state().fonts_settled() {
            h.run_steps(3);
            if h.state().fonts_settled() {
                break;
            }
        }
        assert!(std::time::Instant::now() < end, "never loaded and settled");
    }
    assert_eq!(h.state().fonts_missing(), Vec::<char>::new(), "boxes");
    // every word of the window drawn with boxes forbidden
    h.ctx
        .set_missing_glyph_policy(egui::MissingGlyphPolicy::Panic);
    h.run_steps(4);
}

const LATIN: &str = "The quick brown fox jumps over the lazy dog 0123456789 ABCXYZ";
const OTHERS: &str = "日本語の音楽 中文 한국어 אבגד بتث שלום עולם مرحبا → ← ა ∑ ☀";

/// What a panic said.
fn said(panic: Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or(panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "?".into())
}

/// Text drawn with `data` as the bold, the serif and the bold monospace
/// face of the start, as the player sets them (`Faces::definitions`): what
/// a panic said, if one was raised.
fn drawn_as_a_face_of_the_start(data: &[u8]) -> Result<(), String> {
    let data = Arc::new(data.to_vec());
    std::panic::catch_unwind(move || {
        let faces = Faces {
            bold: Some(data.clone()),
            serif: Some(data.clone()),
            mono_bold: Some(data),
        };
        for mono in [false, true] {
            let definitions = faces.definitions(mono);
            let mut given = false;
            let mut h = Harness::builder()
                .allow_missing_glyphs()
                .with_size(egui::vec2(900.0, 400.0))
                .build_ui(move |ui| {
                    if !std::mem::replace(&mut given, true) {
                        ui.ctx().set_fonts(definitions.clone());
                        return;
                    }
                    for family in ["bold", "serif"] {
                        for size in [9.0, 14.0, 40.0] {
                            ui.label(
                                egui::RichText::new(LATIN)
                                    .family(egui::FontFamily::Name(family.into()))
                                    .size(size),
                            );
                        }
                    }
                    ui.label(LATIN);
                });
            h.run_steps(3);
        }
    })
    .map_err(said)
}

/// Text of other scripts drawn with the file at `path` named as
/// `AXROSS_AMP_FONTS` names one.
fn drawn_as_a_named_font(path: &std::path::Path) -> Result<(), String> {
    let path = path.to_owned();
    std::panic::catch_unwind(move || {
        let mut h = Harness::builder()
            .allow_missing_glyphs()
            .with_size(egui::vec2(900.0, 300.0))
            .build_ui(move |ui| {
                for size in [10.0, 30.0] {
                    ui.label(egui::RichText::new(OTHERS).size(size));
                }
            });
        let fallback = Fallback::start(&h.ctx, vec![path], false);
        h.ctx.set_font_providers(vec![fallback.clone()]);
        settle(&mut h, &fallback);
        h.run_steps(3);
    })
    .map_err(said)
}

/// A font file's tables: (tag, offset, length).
fn tables(data: &[u8]) -> Vec<(String, usize, usize)> {
    let n = u16::from_be_bytes([data[4], data[5]]) as usize;
    (0..n)
        .map(|i| {
            let r = &data[12 + 16 * i..28 + 16 * i];
            (
                String::from_utf8_lossy(&r[0..4]).into_owned(),
                u32::from_be_bytes([r[8], r[9], r[10], r[11]]) as usize,
                u32::from_be_bytes([r[12], r[13], r[14], r[15]]) as usize,
            )
        })
        .collect()
}

/// `good` damaged in every way review 20 damaged a font: a block of
/// `block` bytes zeroed at every `block`-th offset (and at odd ones), one
/// of 0xff at every fourth, each table zeroed and each filled with
/// garbage, the file cut to eight lengths.
fn damaged(good: &[u8], block: usize) -> Vec<(String, Vec<u8>)> {
    let mut cases = Vec::new();
    let mut with = |what: String, from: usize, to: usize, fill: &dyn Fn(usize) -> u8| {
        let mut d = good.to_vec();
        let to = to.min(d.len());
        for (i, b) in d[from..to].iter_mut().enumerate() {
            *b = fill(i);
        }
        cases.push((what, d));
    };
    let mut at = 0;
    while at < good.len() {
        let k = at / block;
        with(format!("zeroed at {at}"), at, at + block, &|_| 0);
        if k.is_multiple_of(4) {
            with(format!("0xff at {at}"), at, at + block, &|_| 0xff);
        }
        if k.is_multiple_of(5) && at + block * 3 / 10 < good.len() {
            let odd = at + block * 3 / 10 + 5;
            with(
                format!("zeroed at {odd}"),
                odd.min(good.len()),
                odd + block,
                &|_| 0,
            );
        }
        at += block;
    }
    for (tag, at, len) in tables(good) {
        with(format!("table {tag} zeroed"), at, at + len, &|_| 0);
        with(format!("table {tag} garbage"), at, at + len, &|i| {
            (i as u32).wrapping_mul(2_654_435_761).to_be_bytes()[0]
        });
    }
    for part in [2usize, 3, 4, 10] {
        cases.push((
            format!("cut to 1/{part}"),
            good[..good.len() / part].to_vec(),
        ));
        let most = part * 3;
        cases.push((
            format!("cut to {}/{most}", most - 1),
            good[..good.len() * (most - 1) / most].to_vec(),
        ));
    }
    cases
}

/// Review 20, M1: a font cut to a half, a third, a quarter or a tenth,
/// one with its `head` zeroed and one with a zeroed stretch of `glyf` were
/// taken for sound and panicked the window's thread as a text was laid
/// out - 102 of 878 damaged files as the bold face, 41 as a font named in
/// `AXROSS_AMP_FONTS` ("index out of bounds" in the font's reader, "Bad
/// px_scale_factor: inf"). The same sweep, on the tests' font and on the
/// system's faces where it has them: whatever the damage, a text is drawn
/// and nothing panics.
#[test]
fn no_damaged_font_panics_as_a_face_of_the_start_or_as_a_named_font() {
    // (the panics caught here are not written out, hundreds of times)
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let dir = scratch::Scratch::new("fonts-sweep");
    let mut fonts: Vec<(String, Vec<u8>, usize)> = vec![(
        "the tests' font".into(),
        std::fs::read(fixture()).unwrap(),
        96,
    )];
    for path in fonts::BOLD_FONTS
        .iter()
        .chain(&fonts::MONO_BOLD_FONTS)
        .chain(&["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"])
    {
        if let Ok(data) = std::fs::read(path) {
            fonts.push(((*path).to_owned(), data, 4096));
        }
    }
    let (mut cases, mut refused, mut bad) = (0, 0, Vec::new());
    for (name, good, block) in &fonts {
        assert!(fonts::sound(good, 0, None), "{name} is not sound whole");
        assert_eq!(drawn_as_a_face_of_the_start(good), Ok(()), "{name}");
        for (i, (what, data)) in damaged(good, *block).iter().enumerate() {
            cases += 1;
            refused += !fonts::sound(data, 0, None) as usize;
            if let Err(panic) = drawn_as_a_face_of_the_start(data) {
                bad.push(format!("{name}, {what}, as a face of the start: {panic}"));
            }
            let path = dir.join(format!("case-{i}.ttf"));
            std::fs::write(&path, data).unwrap();
            if let Err(panic) = drawn_as_a_named_font(&path) {
                bad.push(format!("{name}, {what}, as a named font: {panic}"));
            }
            let _ = std::fs::remove_file(&path);
        }
    }
    std::panic::set_hook(before);
    println!(
        "{} fonts, {cases} damaged files: {refused} refused, {} panics",
        fonts.len(),
        bad.len()
    );
    bad.truncate(30);
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The plain cases by name: half a file, a zeroed `head`, a zeroed `glyf`
/// are not sound - each was.
#[test]
fn half_a_font_a_zeroed_head_and_a_zeroed_glyf_are_not_sound() {
    let good = std::fs::read(fixture()).unwrap();
    assert!(fonts::sound(&good, 0, None));
    for part in [2, 3, 4, 10] {
        assert!(
            !fonts::sound(&good[..good.len() / part], 0, None),
            "cut to 1/{part}"
        );
    }
    for table in ["head", "glyf", "loca", "hhea", "maxp", "cmap"] {
        let (_, at, len) = tables(&good)
            .into_iter()
            .find(|(tag, _, _)| tag == table)
            .unwrap();
        let mut d = good.clone();
        d[at..at + len].fill(0);
        assert!(!fonts::sound(&d, 0, None), "{table} zeroed");
        assert!(!fonts::sound(&d, 0, Some('A')), "{table} zeroed");
    }
}
