//! The fonts: egui's own, a bold, a serif and a bold monospace face from
//! the system where it has them, and - for what none of those can write: a
//! title in Chinese, Japanese, Korean, Arabic, Hebrew, Thai, Devanagari,
//! any script the system has a font for - the system's fonts, found as they
//! are needed.
//!
//! Three rules:
//!
//! * **A damaged font file never ends the program.** Every file is gone
//!   through here ([`sound`]) before egui is given it - its tables within
//!   the file, its `head`, each glyph's place, and every glyph drawn once
//!   with a panic of the font's reader caught: egui panics on a font it
//!   cannot parse, and its reader on a glyph in a damaged stretch of the
//!   file (review 20, M1: half a font, a zeroed `head`, a zeroed block of
//!   `glyf` were taken for sound). And for what no check foresees, a
//!   panic raised in a frame by the code that reads fonts is caught by the
//!   window ([`watch_panics`], [`panic_in_a_font`]): the fonts found for
//!   other scripts are let go ([`Fallback::drop_all`]), then the faces of
//!   the start ([`only_built_in`]).
//! * **Nothing of it on the window's thread.** The system's fonts are
//!   listed, asked and read on a thread of their own ([`Fallback`]): a
//!   character no font has yet is drawn as a box for the frames it takes
//!   to find one, and the text is laid out again when one is found. The
//!   three faces of the start are read on a thread too and waited for a
//!   quarter second at most ([`Faces::of_the_system`]).
//! * **The system's own choice first** (fontconfig, CoreText, DirectWrite,
//!   by script and locale - `egui_system_font_provider`), then well-known
//!   files by their paths, for a system whose font matching finds nothing.
//!   `AXROSS_AMP_FONTS` names more files or folders to look in first.

use egui::epaint::text::{
    FallbackRequest, FontData, FontInsert, FontPriority, FontProvider, InsertFontFamily,
};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Bold system faces looked for (Linux, macOS, Windows), in order.
pub const BOLD_FONTS: [&str; 6] = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "C:\\Windows\\Fonts\\segoeuib.ttf",
];

/// Serif faces looked for (the collage).
pub const SERIF_FONTS: [&str; 4] = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSerif-Bold.ttf",
    "/System/Library/Fonts/Supplemental/Georgia Bold.ttf",
    "C:\\Windows\\Fonts\\georgiab.ttf",
];

/// Bold monospace faces looked for (the terminal's titles).
pub const MONO_BOLD_FONTS: [&str; 3] = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf",
    "C:\\Windows\\Fonts\\consolab.ttf",
];

/// No font file larger than this is read (the largest CJK collections are
/// about 120 MB).
const MOST_BYTES: u64 = 192 << 20;

/// The face `index` of `data` where its frame holds: every table it lists
/// lies within the file (a copy that stopped half way lists them all), and
/// the ones every text needs say what egui reckons with - `head` with a
/// size of the em that is one (zeroed, the scale of every glyph is
/// infinite: "Bad px_scale_factor: inf"), `hhea`, `maxp`, a `cmap`.
fn framed(data: &[u8], index: u32) -> Option<skrifa::FontRef<'_>> {
    use skrifa::raw::TableProvider as _;
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let within = font.table_directory().table_records().iter().all(|r| {
        (r.offset() as u64)
            .checked_add(r.length() as u64)
            .is_some_and(|end| end <= data.len() as u64)
    });
    if !within {
        return None;
    }
    let head = font.head().ok()?;
    if !(16..=16384).contains(&head.units_per_em()) {
        return None;
    }
    let (hhea, maxp) = (font.hhea().ok()?, font.maxp().ok()?);
    if maxp.num_glyphs() == 0 || hhea.number_of_h_metrics() == 0 {
        return None;
    }
    font.hmtx().ok()?;
    font.cmap().ok()?;
    // (outlines by `glyf`: each glyph's place in it, in order and within it)
    if let Ok(loca) = font.loca(None) {
        let glyf = font
            .table_data(skrifa::raw::types::Tag::new(b"glyf"))?
            .len() as u64;
        let mut before = 0u64;
        for glyph in 0..=maxp.num_glyphs() as usize {
            let at = match &loca {
                skrifa::raw::tables::loca::Loca::Short(offsets) => {
                    offsets.get(glyph)?.get() as u64 * 2
                }
                skrifa::raw::tables::loca::Loca::Long(offsets) => offsets.get(glyph)?.get() as u64,
            };
            if at < before || at > glyf {
                return None;
            }
            before = at;
        }
    }
    Some(font)
}

/// A pen that draws nothing.
struct NoPen;

impl skrifa::outline::OutlinePen for NoPen {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}

/// Whether every glyph of the face can be drawn as egui draws one: each
/// outline drawn once, into nothing, with a panic of the font's reader
/// caught (a glyph in a zeroed stretch of `glyf` is one: "index out of
/// bounds" in the reader, on the window's thread, at the first title that
/// shows that letter). Some milliseconds for a face of a few thousand
/// glyphs: done once a file, on the fonts' threads.
fn drawn(data: &[u8], index: u32) -> bool {
    use skrifa::MetadataProvider as _;
    use skrifa::raw::TableProvider as _;
    std::panic::catch_unwind(|| {
        let Some(font) = framed(data, index) else {
            return false;
        };
        let glyphs = font.outline_glyphs();
        let count = font.maxp().map_or(0, |m| m.num_glyphs());
        let settings = || {
            skrifa::outline::DrawSettings::unhinted(
                skrifa::instance::Size::new(16.0),
                skrifa::instance::LocationRef::default(),
            )
        };
        // (as egui sets a face up: its metrics, its hinting)
        let _ = font.metrics(
            skrifa::instance::Size::new(16.0),
            skrifa::instance::LocationRef::default(),
        );
        let _ = skrifa::outline::HintingInstance::new(
            &glyphs,
            skrifa::instance::Size::new(16.0),
            skrifa::instance::LocationRef::default(),
            skrifa::outline::HintingOptions::default(),
        );
        for glyph in 0..count as u32 {
            // (a glyph that cannot be drawn is an error, and egui draws
            // nothing for it: only a panic is what must not be)
            if let Some(outline) = glyphs.get(skrifa::GlyphId::new(glyph)) {
                let _ = outline.draw(settings(), &mut NoPen);
            }
        }
        true
    })
    .unwrap_or(false)
}

/// Tests: a face is taken without its glyphs being drawn first - what a
/// damaged font no check here foresees does to a frame.
#[doc(hidden)]
pub static UNDRAWN: AtomicBool = AtomicBool::new(false);

/// Tests: these in place of the system's faces of the start.
#[doc(hidden)]
pub static GIVEN_FACES: Mutex<Option<Faces>> = Mutex::new(None);

/// What [`whole`] found, by the file's content and the face: a file is
/// gone through once.
fn verdicts() -> &'static Mutex<Verdicts> {
    static VERDICTS: std::sync::OnceLock<Mutex<Verdicts>> = std::sync::OnceLock::new();
    VERDICTS.get_or_init(Default::default)
}

/// (a hash of the file, its length, the face) -> whole or not.
type Verdicts = std::collections::HashMap<(u64, usize, u32), bool>;

/// Whether the face `index` of `data` is one egui can be given: its frame
/// holds ([`framed`]) and every glyph of it draws ([`drawn`]).
pub fn whole(data: &[u8], index: u32) -> bool {
    use std::hash::{Hash as _, Hasher as _};
    if framed(data, index).is_none() {
        return false;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    let key = (hasher.finish(), data.len(), index);
    if let Some(known) = verdicts().lock().ok().and_then(|v| v.get(&key).copied()) {
        return known;
    }
    if UNDRAWN.load(Ordering::Relaxed) {
        return true;
    }
    let verdict = drawn(data, index);
    if let Ok(mut verdicts) = verdicts().lock() {
        // (no more than a few hundred kept: a sweep of damaged files)
        if verdicts.len() > 512 {
            verdicts.clear();
        }
        verdicts.insert(key, verdict);
    }
    verdict
}

/// Whether the face has a glyph for `c` (asked of a face found [`whole`]).
fn writes(data: &[u8], index: u32, c: char) -> bool {
    use skrifa::MetadataProvider as _;
    skrifa::FontRef::from_index(data, index)
        .ok()
        .and_then(|font| font.charmap().map(c))
        .is_some_and(|glyph| glyph != skrifa::GlyphId::NOTDEF)
}

/// A pen that counts what it is told to draw.
struct Counting(usize);

impl skrifa::outline::OutlinePen for Counting {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {
        self.0 += 1;
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn close(&mut self) {}
}

/// Whether the face has something to show for `c`: an outline with lines
/// in it (a face whose `loca` is zeroed has a glyph for every letter and
/// every one of them empty: its words would be blanks), a bitmap or a
/// colour glyph; for a space, a glyph.
fn shows(font: &skrifa::FontRef<'_>, c: char) -> bool {
    use skrifa::MetadataProvider as _;
    let Some(glyph) = font.charmap().map(c) else {
        return false;
    };
    if glyph == skrifa::GlyphId::NOTDEF {
        return false;
    }
    if c.is_whitespace() {
        return true;
    }
    let inked = font.outline_glyphs().get(glyph).is_some_and(|outline| {
        let mut pen = Counting(0);
        let settings = skrifa::outline::DrawSettings::unhinted(
            skrifa::instance::Size::new(16.0),
            skrifa::instance::LocationRef::default(),
        );
        outline.draw(settings, &mut pen).is_ok() && pen.0 > 0
    });
    inked
        || font.color_glyphs().get(glyph).is_some()
        || font
            .bitmap_strikes()
            .glyph_for_size(skrifa::instance::Size::unscaled(), glyph)
            .is_some()
}

/// Whether `data` is a font file egui can be given: its face `index` is
/// [`whole`], and it has something to show for `has` - or, where `has` is
/// none, for an A or a digit (a face of letters at all).
pub fn sound(data: &[u8], index: u32, has: Option<char>) -> bool {
    if !whole(data, index) {
        return false;
    }
    let Ok(font) = skrifa::FontRef::from_index(data, index) else {
        return false;
    };
    if UNDRAWN.load(Ordering::Relaxed) {
        return has.is_none_or(|c| writes(data, index, c));
    }
    // (after `whole`: no glyph of it panics as it is drawn)
    match has {
        Some(c) => shows(&font, c),
        None => ['A', '0'].iter().any(|c| shows(&font, *c)),
    }
}

/// The file at `path`, when it is a font file of a size to be read.
fn read(path: &Path) -> Option<Vec<u8>> {
    #[cfg(test)]
    tests::READS.fetch_add(1, Ordering::Relaxed);
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > MOST_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

/// The first of `paths` that is a sound font file.
pub fn first_sound<P: AsRef<Path>>(paths: &[P]) -> Option<Arc<Vec<u8>>> {
    paths
        .iter()
        .filter_map(|p| read(p.as_ref()))
        .find(|data| sound(data, 0, None))
        .map(Arc::new)
}

/// The faces of the start are not used any more ([`only_built_in`]).
static BUILT_IN_ONLY: AtomicBool = AtomicBool::new(false);

/// From now on [`Faces::of_the_system`] gives none: the window's words in
/// egui's own faces. True when that is new.
pub fn only_built_in() -> bool {
    !BUILT_IN_ONLY.swap(true, Ordering::AcqRel)
}

/// Where the last panic of this program was raised (the file), kept by
/// [`watch_panics`].
static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Panics are noted from now on (and reported as before): what
/// [`panic_in_a_font`] asks.
pub fn watch_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Ok(mut last) = LAST_PANIC.lock() {
                *last = info.location().map(|l| l.file().to_owned());
            }
            before(info);
        }));
    });
}

/// Whether the panic last noted was raised in the code that reads fonts
/// and lays text out (taken: asked once a panic).
pub fn panic_in_a_font() -> bool {
    let file = LAST_PANIC.lock().ok().and_then(|mut l| l.take());
    file.is_some_and(|f| of_the_fonts(&f))
}

/// A source file of the crates that read fonts, shape and lay out text.
fn of_the_fonts(file: &str) -> bool {
    let file = file.replace('\\', "/");
    [
        "/read-fonts-",
        "/skrifa-",
        "/font-types-",
        "/harfrust-",
        "/swash-",
    ]
    .iter()
    .any(|c| file.contains(c))
        || (file.contains("/epaint-") && file.contains("/text/"))
        || file.ends_with("src/fonts.rs")
}

/// The faces of the start: bold (titles), serif (the collage's words), bold
/// monospace (the terminal's titles) - each where the system has one.
#[derive(Clone, Default)]
pub struct Faces {
    pub bold: Option<Arc<Vec<u8>>>,
    pub serif: Option<Arc<Vec<u8>>>,
    pub mono_bold: Option<Arc<Vec<u8>>>,
}

impl Faces {
    /// From the first sound file of each list.
    pub fn read<P: AsRef<Path>>(bold: &[P], serif: &[P], mono_bold: &[P]) -> Faces {
        Faces {
            bold: first_sound(bold),
            serif: first_sound(serif),
            mono_bold: first_sound(mono_bold),
        }
    }

    /// The system's, read once on a thread of their own and waited for a
    /// quarter second at most (a folder of fonts on a disk that hangs: the
    /// window opens with egui's own faces).
    pub fn of_the_system() -> Faces {
        static FACES: std::sync::OnceLock<Faces> = std::sync::OnceLock::new();
        // (one of them panicked in a frame: egui's own from then on)
        if BUILT_IN_ONLY.load(Ordering::Acquire) {
            return Faces::default();
        }
        if let Some(given) = GIVEN_FACES.lock().ok().and_then(|g| g.clone()) {
            return given;
        }
        FACES
            .get_or_init(|| {
                let (tx, rx) = std::sync::mpsc::channel();
                let spawned = std::thread::Builder::new()
                    .name("amp-fonts-start".into())
                    .spawn(move || {
                        let _ = tx.send(Faces::read(&BOLD_FONTS, &SERIF_FONTS, &MONO_BOLD_FONTS));
                    });
                match spawned {
                    Ok(_) => rx
                        .recv_timeout(Duration::from_millis(250))
                        .unwrap_or_default(),
                    Err(_) => Faces::default(),
                }
            })
            .clone()
    }

    /// egui's fonts with these: Hack (the monospace font) as the last
    /// fallback of the proportional one; a family "bold" and a family
    /// "serif" (each the proportional one where there is no such face);
    /// `mono`: every text in the monospace face (the terminal's and the 90s
    /// desktop's designs - their rows, their words, not only their titles).
    pub fn definitions(&self, mono: bool) -> egui::FontDefinitions {
        use egui::FontFamily::{Monospace, Name, Proportional};
        let mut fonts = egui::FontDefinitions::default();
        if let Some(family) = fonts.families.get_mut(&Proportional)
            && !family.iter().any(|f| f == "Hack")
        {
            family.push("Hack".to_owned());
        }
        let proportional = fonts
            .families
            .get(&Proportional)
            .cloned()
            .unwrap_or_default();
        let mut with = |name: &str, data: &Option<Arc<Vec<u8>>>, mut family: Vec<String>| {
            // (parsed once more as it is handed over: what is given to egui
            // is sound whatever came here)
            if let Some(data) = data.as_ref().filter(|d| sound(d, 0, None)) {
                let blob: egui::epaint::text::Blob = data.clone();
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::new(FontData::from_blob(blob, 0)));
                family.insert(0, name.to_owned());
            }
            family
        };
        let bold = with("axross-bold", &self.bold, proportional.clone());
        let serif = with("axross-serif", &self.serif, proportional.clone());
        let mut bold_mono = Vec::new();
        if mono {
            let mut all = fonts.families.get(&Monospace).cloned().unwrap_or_default();
            all.extend(proportional.iter().cloned());
            bold_mono = with("axross-mono-bold", &self.mono_bold, all.clone());
            fonts.families.insert(Proportional, all);
        }
        fonts
            .families
            .insert(Name("bold".into()), if mono { bold_mono } else { bold });
        fonts.families.insert(Name("serif".into()), serif);
        fonts
    }
}

/// Well-known font files by what they can write, for a system whose own
/// font matching finds nothing: looked at in order, only where they are.
const KNOWN: [(&str, &[&str]); 7] = [
    (
        "cjk",
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\YuGothR.ttc",
            "C:\\Windows\\Fonts\\meiryo.ttc",
            "C:\\Windows\\Fonts\\malgun.ttf",
            "C:\\Windows\\Fonts\\simsun.ttc",
        ],
    ),
    (
        "arabic",
        &[
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf",
            "/System/Library/Fonts/GeezaPro.ttc",
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\tahoma.ttf",
        ],
    ),
    (
        "hebrew",
        &[
            "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansHebrew-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansHebrew-Regular.ttf",
            "/System/Library/Fonts/Supplemental/Arial Hebrew.ttc",
            "/System/Library/Fonts/ArialHB.ttc",
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ],
    ),
    (
        "thai",
        &[
            "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansThai-Regular.ttf",
            "/usr/share/fonts/truetype/tlwg/Loma.ttf",
            "/usr/share/fonts/truetype/tlwg/Garuda.ttf",
            "/System/Library/Fonts/Supplemental/Thonburi.ttc",
            "/System/Library/Fonts/Thonburi.ttc",
            "C:\\Windows\\Fonts\\leelawui.ttf",
            "C:\\Windows\\Fonts\\tahoma.ttf",
        ],
    ),
    (
        "devanagari",
        &[
            "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/truetype/lohit-devanagari/Lohit-Devanagari.ttf",
            "/System/Library/Fonts/Kohinoor.ttc",
            "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
            "C:\\Windows\\Fonts\\Nirmala.ttc",
            "C:\\Windows\\Fonts\\Nirmala.ttf",
        ],
    ),
    (
        "emoji",
        &[
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/google-noto-color-emoji-fonts/NotoColorEmoji.ttf",
            "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf",
            "/System/Library/Fonts/Apple Color Emoji.ttc",
            "C:\\Windows\\Fonts\\seguiemj.ttf",
            "C:\\Windows\\Fonts\\seguisym.ttf",
        ],
    ),
    (
        "any",
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/unifont/unifont.ttf",
            "/usr/share/fonts/opentype/unifont/unifont.otf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\seguisym.ttf",
        ],
    ),
];

/// Which of [`KNOWN`]'s kinds can write `c`.
fn kind_of(c: char) -> &'static str {
    match c as u32 {
        0x0590..=0x05ff | 0xfb1d..=0xfb4f => "hebrew",
        0x0600..=0x06ff | 0x0750..=0x077f | 0x08a0..=0x08ff | 0xfb50..=0xfdff | 0xfe70..=0xfeff => {
            "arabic"
        }
        0x0900..=0x097f | 0xa8e0..=0xa8ff => "devanagari",
        0x0e00..=0x0e7f => "thai",
        0x1100..=0x11ff
        | 0x2e80..=0x9fff
        | 0xa960..=0xa97f
        | 0xac00..=0xd7ff
        | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f
        | 0xff00..=0xffef
        | 0x20000..=0x3ffff => "cjk",
        0x2190..=0x2bff | 0x1f000..=0x1faff => "emoji",
        _ => "any",
    }
}

/// The files `AXROSS_AMP_FONTS` names: files, and the font files of its
/// folders (not of folders in them), in the order named.
fn named_files(list: &std::ffi::OsStr) -> Vec<PathBuf> {
    #[cfg(test)]
    if let Ok(mut on) = tests::LISTED_ON.lock() {
        *on = std::thread::current().name().map(str::to_owned);
    }
    let mut out = Vec::new();
    for entry in std::env::split_paths(list) {
        if entry.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(&entry)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_lowercase)
                        .is_some_and(|e| ["ttf", "otf", "ttc", "otc"].contains(&e.as_str()))
                })
                .collect();
            inside.sort();
            out.extend(inside);
        } else if entry.is_file() {
            out.push(entry);
        }
    }
    out
}

/// A font found for a character: what egui is given.
#[derive(Clone)]
struct Found {
    name: String,
    data: FontData,
}

#[derive(Default)]
struct Shared {
    /// The fonts found so far, in the order they were.
    found: Mutex<Vec<Found>>,
    /// The characters asked for (each once).
    asked: Mutex<HashSet<char>>,
    /// Those no font here can write.
    missing: Mutex<BTreeSet<char>>,
    /// Characters still being looked for.
    pending: AtomicUsize,
    /// A font was found since the text was last laid out.
    fresh: AtomicBool,
    /// No font of these any more ([`Fallback::drop_all`]).
    dropped: AtomicBool,
}

/// The files to look in first: listed, or as `AXROSS_AMP_FONTS` names them
/// (its folders are listed on the searching thread: one on a disk that
/// hangs, or of 100,000 files, held the window's start).
enum Named {
    Files(Vec<PathBuf>),
    List(std::ffi::OsString),
}

/// The fonts of the system for what egui's own cannot write: a
/// [`FontProvider`] that never waits - it answers from the fonts found so
/// far, and has the rest looked for on a thread of its own.
pub struct Fallback {
    shared: Arc<Shared>,
    look: Mutex<std::sync::mpsc::Sender<char>>,
}

impl Fallback {
    /// With the system's fonts and the well-known files; `ctx` is asked to
    /// draw again when a font is found.
    pub fn of_the_system(ctx: &egui::Context) -> Arc<Fallback> {
        let named = std::env::var_os("AXROSS_AMP_FONTS").unwrap_or_default();
        Self::begin(ctx, Named::List(named), true)
    }

    /// With `files` first, then (`system`) the system's fonts and the
    /// well-known files.
    pub fn start(ctx: &egui::Context, files: Vec<PathBuf>, system: bool) -> Arc<Fallback> {
        Self::begin(ctx, Named::Files(files), system)
    }

    /// With the files and folders `list` names (as `AXROSS_AMP_FONTS`
    /// does: separated as `PATH` is) first.
    pub fn named(ctx: &egui::Context, list: &std::ffi::OsStr, system: bool) -> Arc<Fallback> {
        Self::begin(ctx, Named::List(list.to_owned()), system)
    }

    fn begin(ctx: &egui::Context, named: Named, system: bool) -> Arc<Fallback> {
        let shared = Arc::new(Shared::default());
        let (look, wanted) = std::sync::mpsc::channel::<char>();
        let (for_thread, ctx) = (shared.clone(), ctx.clone());
        // (no thread: nothing is found, and nothing waits)
        let _ = std::thread::Builder::new()
            .name("amp-fonts".into())
            .spawn(move || search(&for_thread, &wanted, named, system, &ctx));
        Arc::new(Fallback {
            shared,
            look: Mutex::new(look),
        })
    }

    /// A font was found since this was last asked: the text is to be laid
    /// out again (the window sets its font providers anew).
    pub fn take_fresh(&self) -> bool {
        self.shared.fresh.swap(false, Ordering::AcqRel)
    }

    /// Nothing is being looked for, and what was found is in use.
    pub fn settled(&self) -> bool {
        self.shared.pending.load(Ordering::Acquire) == 0
            && !self.shared.fresh.load(Ordering::Acquire)
    }

    /// The characters asked for that no font here can write (boxes).
    pub fn missing(&self) -> Vec<char> {
        self.shared
            .missing
            .lock()
            .map(|m| m.iter().copied().collect())
            .unwrap_or_default()
    }

    /// The fonts found so far, by their names.
    pub fn found(&self) -> Vec<String> {
        self.shared
            .found
            .lock()
            .map(|f| f.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Every font found is let go, and none is looked for any more (one
    /// of them panicked in a frame): how many were in use.
    pub fn drop_all(&self) -> usize {
        self.shared.dropped.store(true, Ordering::Release);
        self.shared
            .found
            .lock()
            .map(|mut f| std::mem::take(&mut *f).len())
            .unwrap_or(0)
    }
}

impl FontProvider for Fallback {
    fn font_for(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        let c = request.cluster.chars().next()?;
        if self.shared.dropped.load(Ordering::Acquire) {
            return None;
        }
        // (each of them gone through when it was found: here only whether
        // it has the letter)
        if let Ok(found) = self.shared.found.lock()
            && let Some(font) = found
                .iter()
                .find(|f| writes(f.data.bytes(), f.data.index, c))
        {
            return Some(FontInsert {
                name: font.name.clone(),
                data: font.data.clone(),
                families: vec![InsertFontFamily {
                    family: request.family.clone(),
                    priority: FontPriority::Lowest,
                }],
            });
        }
        // (looked for once, on the other thread: a box until then)
        if self.shared.asked.lock().is_ok_and(|mut a| a.insert(c)) {
            self.shared.pending.fetch_add(1, Ordering::AcqRel);
            let sent = self.look.lock().is_ok_and(|look| look.send(c).is_ok());
            if !sent {
                self.shared.pending.fetch_sub(1, Ordering::AcqRel);
                if let Ok(mut missing) = self.shared.missing.lock() {
                    missing.insert(c);
                }
            }
        }
        None
    }
}

/// The searching thread: each character wanted, in the fonts found so far,
/// then the named files, the system's own choice, the well-known files.
fn search(
    shared: &Shared,
    wanted: &std::sync::mpsc::Receiver<char>,
    named: Named,
    system: bool,
    ctx: &egui::Context,
) {
    #[cfg(not(target_os = "android"))]
    let provider = system.then(egui_system_font_provider::SystemFontProvider::new);
    // (the folders listed at the first character wanted: here)
    let mut named = Some(named);
    let mut files: Vec<PathBuf> = Vec::new();
    let mut letters = Letters::default();
    while let Ok(c) = wanted.recv() {
        if shared.dropped.load(Ordering::Acquire) {
            shared.pending.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        match named.take() {
            Some(Named::Files(listed)) => files = listed,
            Some(Named::List(list)) => files = named_files(&list),
            None => {}
        }
        let known = shared
            .found
            .lock()
            .is_ok_and(|f| f.iter().any(|f| writes(f.data.bytes(), f.data.index, c)));
        let mut font = None;
        if !known {
            font = files.iter().find_map(|p| letters.in_file(p, c));
            #[cfg(not(target_os = "android"))]
            if font.is_none()
                && let Some(provider) = &provider
            {
                font = of_the_system(provider, c);
            }
            if font.is_none() && system {
                font = KNOWN
                    .iter()
                    .filter(|(kind, _)| *kind == kind_of(c) || *kind == "any")
                    .flat_map(|(_, paths)| paths.iter())
                    .find_map(|p| letters.in_file(Path::new(p), c));
            }
        }
        let got = font.is_some();
        match font {
            Some(font) => {
                if let Ok(mut found) = shared.found.lock()
                    && !shared.dropped.load(Ordering::Acquire)
                    && !found.iter().any(|f| f.name == font.name)
                {
                    found.push(font);
                }
            }
            None if !known => {
                if let Ok(mut missing) = shared.missing.lock() {
                    missing.insert(c);
                }
            }
            None => {}
        }
        if known || got {
            // (the box drawn meanwhile is remembered by egui: laid out anew)
            shared.fresh.store(true, Ordering::Release);
        }
        shared.pending.fetch_sub(1, Ordering::AcqRel);
        ctx.request_repaint();
    }
}

/// What each font file looked at can write: its faces' letters, read once
/// (every named file was read whole again for every character not yet
/// found). The file itself is read again only when a face of it is taken.
#[derive(Default)]
struct Letters {
    /// By path: (face, its characters in order) - none for a file that is
    /// no font or not a sound one.
    of: std::collections::HashMap<PathBuf, Vec<(u32, Vec<u32>)>>,
}

impl Letters {
    /// The face of the font file at `path` that can write `c`.
    fn in_file(&mut self, path: &Path, c: char) -> Option<Found> {
        use skrifa::MetadataProvider as _;
        if !self.of.contains_key(path) {
            let mut faces = Vec::new();
            if let Some(data) = read(path) {
                // (a collection: its faces in turn)
                for index in 0..64u32 {
                    if skrifa::FontRef::from_index(&data, index).is_err() {
                        break;
                    }
                    let Some(font) = framed(&data, index) else {
                        continue;
                    };
                    let mut letters: Vec<u32> = std::panic::catch_unwind(|| {
                        font.charmap().mappings().map(|(c, _)| c).collect()
                    })
                    .unwrap_or_default();
                    letters.sort_unstable();
                    letters.dedup();
                    faces.push((index, letters));
                }
            }
            self.of.insert(path.to_owned(), faces);
        }
        let index = self
            .of
            .get(path)?
            .iter()
            .find(|(_, letters)| letters.binary_search(&(c as u32)).is_ok())?
            .0;
        // (taken: read again and gone through, glyph by glyph)
        let taken = read(path).filter(|data| sound(data, index, Some(c)));
        let Some(data) = taken else {
            // (not a sound one after all: not looked at again)
            self.of.insert(path.to_owned(), Vec::new());
            return None;
        };
        let blob: egui::epaint::text::Blob = Arc::new(data);
        Some(Found {
            name: format!("file:{}:{index}", path.display()),
            data: FontData::from_blob(blob, index),
        })
    }
}

/// The system's own choice of a font for `c` (it lists its fonts the first
/// time it is asked: here, not on the window's thread), taken only when
/// the file is sound.
#[cfg(not(target_os = "android"))]
fn of_the_system(
    provider: &egui_system_font_provider::SystemFontProvider,
    c: char,
) -> Option<Found> {
    let cluster = c.to_string();
    let request = FallbackRequest {
        cluster: &cluster,
        family: &egui::FontFamily::Proportional,
    };
    let insert =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| provider.font_for(&request)))
            .ok()
            .flatten()?;
    sound(insert.data.bytes(), insert.data.index, Some(c)).then_some(Found {
        name: insert.name,
        data: insert.data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many times a font file was read, and the thread the named
    /// folders were listed on.
    pub(super) static READS: AtomicUsize = AtomicUsize::new(0);
    pub(super) static LISTED_ON: Mutex<Option<String>> = Mutex::new(None);

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/media/fonts/amp-fixture.ttf")
    }

    /// Review 20, L6: the folders `AXROSS_AMP_FONTS` names were listed on
    /// the window's thread as the player started (361 ms for a folder of
    /// 100,000 files; a folder on a disk that hangs held the window), and
    /// every named file was read whole again for each character not found
    /// yet.
    #[test]
    fn named_folders_are_listed_on_the_fonts_thread_and_a_file_is_read_once() {
        let ctx = egui::Context::default();
        let folder = fixture().parent().unwrap().to_owned();
        *LISTED_ON.lock().unwrap() = None;
        let fallback = Fallback::named(&ctx, folder.as_os_str(), false);
        assert_eq!(
            *LISTED_ON.lock().unwrap(),
            None,
            "listed before a letter was asked for"
        );
        let ask = |c: char| {
            let cluster = c.to_string();
            fallback.font_for(&FallbackRequest {
                cluster: &cluster,
                family: &egui::FontFamily::Proportional,
            })
        };
        let wait = || {
            let end = std::time::Instant::now() + Duration::from_secs(20);
            while !fallback.settled() && !fallback.take_fresh() {
                assert!(std::time::Instant::now() < end, "never settled");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        assert!(ask('日').is_none(), "answered before it was looked for");
        wait();
        assert_eq!(LISTED_ON.lock().unwrap().as_deref(), Some("amp-fonts"));
        assert!(ask('日').is_some(), "{:?}", fallback.missing());
        // the file: read once for its letters, once more as it was taken -
        // and not again for letters it has not, however many are asked
        let mut letters = Letters::default();
        let before = READS.load(Ordering::Relaxed);
        assert!(letters.in_file(&fixture(), '語').is_some());
        let taken = READS.load(Ordering::Relaxed) - before;
        for c in ['ก', 'ह', 'ა', '→', '😀', 'Ω'] {
            assert!(letters.in_file(&fixture(), c).is_none(), "{c}");
        }
        assert_eq!(READS.load(Ordering::Relaxed) - before, taken);
        assert!(taken <= 2, "read {taken} times for one letter");
    }

    #[test]
    fn a_damaged_font_file_is_not_sound() {
        assert!(!sound(b"", 0, None));
        assert!(!sound(b"not a font at all, only words", 0, None));
        assert!(!sound(&[0u8; 4096], 0, None));
        // (a real one, where the system has one: sound whole, not cut short
        // nor with its head overwritten)
        if let Some(data) = BOLD_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
            assert!(sound(&data, 0, None));
            assert!(sound(&data, 0, Some('A')));
            assert!(!sound(&data, 1, None), "a second face in a single font");
            assert!(!sound(&data[..data.len() / 50], 0, None));
            let mut broken = data.clone();
            broken[..64].fill(0xff);
            assert!(!sound(&broken, 0, None));
            assert!(!sound(&data, 0, Some('\u{10ffff}')));
        }
    }

    #[test]
    fn a_character_is_looked_for_among_the_files_of_its_kind() {
        for (c, kind) in [
            ('日', "cjk"),
            ('の', "cjk"),
            ('한', "cjk"),
            ('ع', "arabic"),
            ('ש', "hebrew"),
            ('ก', "thai"),
            ('ह', "devanagari"),
            ('😀', "emoji"),
            ('→', "emoji"),
            ('ა', "any"),
        ] {
            assert_eq!(kind_of(c), kind, "{c}");
        }
    }
}
