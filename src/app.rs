//! The window: now playing (cover, title, artist), the visualization (a
//! click shows the next face), the position, the transport, the equalizer
//! and the playlist - all folding away - in a dark look of its own.

use crate::analyzer::Analyzer;
use crate::engine::{Command, Engine, Event, TrackInfo};
use crate::eq;
use crate::playlist::{self, Playlist, Repeat};
use crate::state::{self, EqSettings, PlayerState, Track, VisualOptions};
use crate::viz::{self, VizState};
use egui::{
    Align, Color32, CornerRadius, Key, Layout, RichText, Sense, Stroke, TextureHandle, Ui, vec2,
};
use egui_file_dialog::FileDialog;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The theme shown (its tokens: see `theme`).
fn th() -> &'static crate::theme::Theme {
    crate::theme::current()
}
const SAVE_EVERY: Duration = Duration::from_secs(3);
/// How long a message stays in the status line.
const STATUS_FOR: Duration = Duration::from_secs(8);
/// The frame rate's cap on a slow (software) renderer.
const SLOW_FPS: f32 = 12.0;
/// On a slow renderer, the share of a core the window may take while it
/// plays: the frame rate is lowered (to MIN_SLOW_FPS) to keep within it.
const SLOW_BUDGET_CORES: f64 = 0.6;
const MIN_SLOW_FPS: f32 = 3.0;
/// A calm face (the cover) needs few frames: its glow moves slowly.
const CALM_FPS: f32 = 8.0;
/// The least height the playlist keeps under the picture (three rows and
/// its filter), whatever the window.
const LEAST_LIST: f32 = 170.0;
const POSITION_EVERY: Duration = Duration::from_secs(30);
/// The equalizer is written this long after its last change (axross's 250 ms).
const EQ_SETTLE: Duration = Duration::from_millis(250);

#[derive(PartialEq)]
enum Picking {
    Nothing,
    Files,
    Folder,
    SavePlaylist,
}

/// The pictures shown on a display of their own.
struct External {
    monitor: crate::display::Monitor,
    /// The sound moved there too, and the output it was on before.
    with_sound: bool,
    previous_output: Option<String>,
    /// The track's words over the picture: which track, since when.
    told: String,
    told_at: f64,
}

pub struct App {
    engine: Engine,
    playlist: Playlist,
    info: TrackInfo,
    cover: Option<TextureHandle>,
    /// The cover as read (the glass design's haze is made from it).
    cover_art: Option<Arc<crate::engine::Art>>,
    /// The glass design's ground: the cover's haze, for the theme and
    /// accent it was made for (made again when either changes).
    haze: Option<(&'static str, Color32, TextureHandle)>,
    /// The cover at the sizes it is shown (made on the CPU, filtered).
    sizer: crate::cover::Sizer,
    /// The pictures' colours from the cover (when it has colour).
    cover_scheme: Option<viz::Scheme>,
    /// The accent: from the cover (its strongest colour, made to read in
    /// the theme), else the theme's own.
    accent: Color32,
    /// The cover's strongest hue, as it is.
    cover_hue: Option<Color32>,
    /// The theme and accent the window shows (set again only when either
    /// changes: no cost a frame), and the one shown while the pointer is on
    /// it in the picker.
    theme_shown: (&'static str, Color32, &'static str),
    /// The design shown (and the chosen one, whose type and room the
    /// window keeps while another is shown in the picker), and the one the
    /// pointer is on there.
    design_shown: (&'static str, &'static str),
    design_preview: Option<&'static str>,
    /// Where each design panel lay last frame (the card, the playlist):
    /// painted there under what lies on it; and the reels' turn.
    panel_rects: [egui::Rect; 2],
    /// Where the position and the buttons lay last frame (zen quiets them
    /// while the pointer is elsewhere).
    controls_rect: egui::Rect,
    reels_turn: f32,
    theme_preview: Option<&'static str>,
    /// "Show on display": the monitors connected (asked on a thread every
    /// two seconds where the window belongs to a desktop; set by tests),
    /// the view open on one, and whether to open it again (where it was
    /// open when the program ended).
    monitors: Arc<Mutex<Vec<crate::display::Monitor>>>,
    external: Option<External>,
    reopen_display: bool,
    analyzer: Analyzer,
    /// Loudness, beat, notes, key: on their own thread, every sample heard.
    meters: crate::meters::Meters,
    viz_state: VizState,
    tap: Vec<f32>,
    mode: &'static str,
    scheme_key: String,
    custom_colours: std::collections::BTreeMap<String, String>,
    options: VisualOptions,
    volume: u32,
    muted: bool,
    eq: EqSettings,
    eq_open: bool,
    /// The lower part shows the equalizer (else the playlist).
    eq_page: bool,
    /// A slow renderer (software): the picture at most SLOW_FPS frames a
    /// second, and fewer as a frame costs more (a large window): the rate
    /// the CPU's budget allows, measured (when, CPU seconds, frames).
    slow: bool,
    budget_fps: f32,
    cost_from: Option<(Instant, f64, u64)>,
    frames_drawn: u64,
    /// Tests (AMP_STATS=FILE): frames drawn, written there now and then.
    stats: Option<(PathBuf, u64, Instant)>,
    playlist_open: bool,
    big_visual: bool,
    /// The playlist shown under the large picture too.
    big_list: bool,
    /// The photosensitivity warning shown (the intense motion asked for),
    /// and whether the desktop asks for reduced motion (None: not known).
    intense_asked: bool,
    reduced_motion: Arc<Mutex<Option<bool>>>,
    /// The desktop's light or dark, asked (winit says nothing on Linux);
    /// None where the window does not belong to a desktop (tests).
    desktop_shade: Option<crate::desktop_theme::Shade>,
    /// Where the desktop's shade is asked (Some(None): the session bus;
    /// None: not at all - no desktop).
    shade_bus: Option<Option<String>>,
    /// The height of what lies between the picture and the playlist (the
    /// position, the transport, the switch), as last laid out.
    controls_height: f32,
    /// The current track's waveform, read for the overview face: its
    /// path, what is read so far, the flag that stops the reading.
    overview: Option<(String, Arc<Mutex<viz::Overview>>, Arc<AtomicBool>)>,
    /// A click on the picture waits until it cannot become a double-click
    /// (which shows it large and leaves the face as it is).
    pending_click: Option<f64>,
    generation: u64,
    /// The last generation handed out (the current track's or the queued one's).
    generations: u64,
    loaded: Option<u64>,
    /// The track asked for is still opening (a stream connects).
    opening: bool,
    /// Stopped (by Stop, or at the end of the list) rather than paused.
    stopped: bool,
    failures: usize,
    /// Tracks that could not be played, by path: why (shown in the playlist).
    broken: std::collections::HashMap<String, String>,
    /// The track named to the engine to follow: (index, path, generation).
    queued: Option<(usize, String, u64)>,
    /// What a radio says it plays, and what the engine tells meanwhile.
    stream_title: String,
    notice: String,
    /// The window's title as last set.
    window_title: String,
    /// The player on the desktop's bus (MPRIS), and its cover's file:// URL.
    #[cfg(target_os = "linux")]
    mpris: Option<crate::mpris::Mpris>,

    art_url: String,
    /// The track playing came by itself (the one before ended): a track that
    /// cannot be played is then skipped; one the user chose stays, with why.
    automatic: bool,
    duration: Option<f64>,
    seek_drag: Option<f64>,
    /// A seek asked for and when: shown (the time, the slider) until the
    /// sound has caught up with it - at once, not a frame or two later -
    /// and the next step of ±15 s goes on from it.
    seek_asked: Option<(f64, Instant)>,
    output: String,
    status: String,
    /// The status line as it was last seen and since when (a message goes
    /// after a while).
    status_seen: (String, Instant),
    url: String,
    url_open: bool,
    dialog: FileDialog,
    picking: Picking,
    selected: Option<usize>,
    /// Frames the analysis ran.
    analyzer_frames: u64,
    /// Rows chosen together (Ctrl / Shift-click); empty: only `selected`.
    chosen: Vec<usize>,
    /// The playlist before its last removals (Ctrl+Z), and the track playing then.
    undo: Vec<(Vec<Track>, Option<String>)>,
    dragging: Option<usize>,
    dirty: bool,
    eq_dirty: bool,
    last_save: Instant,
    last_frame: Instant,
    tab: Tab,
    mixer_page: crate::mixer_ui::MixerPage,
    #[cfg(target_os = "linux")]
    bluetooth_page: crate::bluetooth_ui::BluetoothPage,
    /// The output device chosen for the player ("" the default).
    output_choice: String,
    gate: KeyGate,
    /// The mouse wheel's movement not yet a notch; a row to scroll to.
    wheel: f32,
    scroll_to: Option<usize>,
    /// Files and folders being added (walked on threads of their own), in
    /// the order they were given.
    adding: std::collections::VecDeque<Adding>,
    /// Reads titles and lengths of the tracks in the background.
    scanner: Scanner,
    /// Albums' covers: asked for (none yet, or none at all), and the reader.
    album_covers: std::collections::HashMap<String, Option<TextureHandle>>,
    album_loader: AlbumCovers,
    /// The playlist's filter, and the rows it lets through (for its
    /// words and the playlist's version).
    filter: String,
    focus_filter: bool,
    visible: Vec<usize>,
    visible_for: (String, u64),
    ctx: egui::Context,
    /// What only this program keeps (window, ReplayGain), where, and
    /// whether it changed.
    amp: state::AmpSettings,
    amp_dirty: bool,
    /// Writes the files on a thread of its own; when the equalizer changed last.
    saver: crate::saver::Saver,
    eq_changed: Instant,
    /// The sleep timer (not kept, as axross).
    sleep: Option<Sleep>,
    /// The track details shown (a window).
    details: Option<Details>,
    /// Full screen (the picture only); the face whose name shows, until when.
    fullscreen: bool,
    label_for: &'static str,
    label_until: f64,
}

/// The sleep timer: the music pauses at a time (fading out over its last
/// seconds), or stops at the end of the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sleep {
    At(Instant),
    EndOfTrack,
}

/// The sleep timer's fade (axross's 8 s).
const SLEEP_FADE: Duration = Duration::from_secs(8);

/// The details of a track: its path, and what was read of it.
struct Details {
    path: String,
    stream: bool,
    info: std::sync::Arc<std::sync::Mutex<Option<Result<TrackInfo, String>>>>,
}

/// Files and folders on their way into the playlist.
struct Adding {
    rx: crossbeam_channel::Receiver<AddMessage>,
    then: Then,
    found: usize,
}

/// What follows when added files are in.
#[derive(Clone, Copy, PartialEq)]
enum Then {
    Nothing,
    /// The first of them plays, unless something plays by then.
    PlayIfIdle,
    Play,
}

enum AddMessage {
    Found(usize),
    Done(playlist::Collected),
}

/// What the scanner read of a track: its title ("" none) and length (ms,
/// 0 unknown), or why it cannot be read.
type Scanned = Result<Track, String>;

/// What the playlist keeps of a track's tags: its title ("artist – title"),
/// length, artist, album, disc and track number.
fn tags_of(info: &TrackInfo) -> Track {
    let title = match (info.artist.is_empty(), info.title.is_empty()) {
        (_, true) => String::new(),
        (true, false) => info.title.clone(),
        (false, false) => format!("{} – {}", info.artist, info.title),
    };
    Track {
        path: String::new(),
        title,
        duration_ms: info.duration.map(|d| (d * 1000.0) as u64).unwrap_or(0),
        // (the track's own artist - the album artist only where it has none)
        artist: info.artist.clone(),
        album: info.album.clone(),
        disc: info.disc_number.unwrap_or(0).min(100_000) as u32,
        track: info.track_number.unwrap_or(0).min(100_000) as u32,
    }
}

/// `found`'s tags into `track` where it lacks them.
fn fill_tags(track: &mut Track, found: &Track) {
    if track.title.is_empty() {
        track.title = found.title.clone();
    }
    if track.duration_ms == 0 {
        track.duration_ms = found.duration_ms;
    }
    if track.artist.is_empty() {
        track.artist = found.artist.clone();
    }
    if track.album.is_empty() {
        track.album = found.album.clone();
    }
    if track.disc == 0 {
        track.disc = found.disc;
    }
    if track.track == 0 {
        track.track = found.track;
    }
}

/// A thread that reads tracks' tags and lengths: paths in, (path, title,
/// length in ms, or why it cannot be read) out.
struct Scanner {
    tx: crossbeam_channel::Sender<String>,
    rx: crossbeam_channel::Receiver<(String, Scanned)>,
}

impl Scanner {
    fn start(wake: impl Fn() + Send + 'static) -> Scanner {
        let (tx, paths) = crossbeam_channel::unbounded::<String>();
        let (out, rx) = crossbeam_channel::unbounded();
        let _ = std::thread::Builder::new()
            .name("amp-scan".into())
            .spawn(move || {
                let mut sent = 0usize;
                while let Ok(path) = paths.recv() {
                    let result = crate::engine::probe(&path).map(|info| tags_of(&info));
                    if out.send((path, result)).is_err() {
                        return;
                    }
                    sent += 1;
                    // (the window is woken now and then, not for every file)
                    if sent.is_multiple_of(25) || paths.is_empty() {
                        wake();
                    }
                }
            });
        Scanner { tx, rx }
    }

    /// Read what these tracks lack (local files without a title or length).
    fn scan(&self, tracks: &[Track]) {
        for t in tracks {
            let lacking = t.title.is_empty()
                || t.duration_ms == 0
                || (t.album.is_empty() && t.artist.is_empty());
            if !playlist::is_stream(&t.path) && lacking {
                let _ = self.tx.send(t.path.clone());
            }
        }
    }
}

/// A row of the playlist as shown: an album's head, or a track.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Album(AlbumHead),
    Track(usize),
}

/// An album's head: its name, who made it, its tracks and length, the
/// first of them (for its cover), and the key its cover is kept under.
#[derive(Clone, Debug, PartialEq)]
struct AlbumHead {
    album: String,
    artist: String,
    tracks: usize,
    ms: u64,
    first: usize,
    key: String,
}

/// The rows with a head over each album of two tracks or more that follow
/// one another (an album is its name in its folder).
fn with_albums(tracks: &[Track], rows: &[usize]) -> Vec<Entry> {
    let key = |t: &Track| {
        let folder = std::path::Path::new(&t.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        (!t.album.is_empty()).then(|| format!("{folder}\u{1f}{}", t.album))
    };
    let mut out = Vec::with_capacity(rows.len() + 8);
    let mut k = 0;
    while k < rows.len() {
        let first = &tracks[rows[k]];
        let Some(album) = key(first) else {
            out.push(Entry::Track(rows[k]));
            k += 1;
            continue;
        };
        let mut end = k + 1;
        while end < rows.len() && key(&tracks[rows[end]]).as_deref() == Some(album.as_str()) {
            end += 1;
        }
        if end - k >= 2 {
            let artists: std::collections::BTreeSet<&str> = rows[k..end]
                .iter()
                .map(|&i| tracks[i].artist.as_str())
                .filter(|a| !a.is_empty())
                .collect();
            let artist = match artists.len() {
                0 => String::new(),
                1 => artists.iter().next().unwrap().to_string(),
                _ => "Various artists".to_owned(),
            };
            out.push(Entry::Album(AlbumHead {
                album: first.album.clone(),
                artist,
                tracks: end - k,
                ms: rows[k..end].iter().map(|&i| tracks[i].duration_ms).sum(),
                first: rows[k],
                key: album,
            }));
        }
        out.extend(rows[k..end].iter().map(|&i| Entry::Track(i)));
        k = end;
    }
    out
}

/// A thread that reads albums' covers (from a track of each), made small:
/// (key, path) in, (key, picture or none) out.
struct AlbumCovers {
    tx: crossbeam_channel::Sender<(String, String)>,
    rx: crossbeam_channel::Receiver<(String, Option<egui::ColorImage>)>,
}

impl AlbumCovers {
    fn start(wake: impl Fn() + Send + 'static) -> AlbumCovers {
        let (tx, asks) = crossbeam_channel::unbounded::<(String, String)>();
        let (out, rx) = crossbeam_channel::unbounded();
        let _ = std::thread::Builder::new()
            .name("amp-album-covers".into())
            .spawn(move || {
                while let Ok((key, path)) = asks.recv() {
                    let image = crate::engine::probe(&path).ok().and_then(|info| {
                        let art = info.art?;
                        let rgba =
                            image::RgbaImage::from_raw(art.width, art.height, art.rgba.clone())?;
                        let small = image::imageops::thumbnail(&rgba, 64, 64);
                        Some(egui::ColorImage::from_rgba_unmultiplied(
                            [small.width() as usize, small.height() as usize],
                            small.as_raw(),
                        ))
                    });
                    if out.send((key, image)).is_err() {
                        return;
                    }
                    if asks.is_empty() {
                        wake();
                    }
                }
            });
        AlbumCovers { tx, rx }
    }
}

/// Where the app keeps its state and what it connects to (tests use a
/// folder of their own and leave the desktop alone).
#[derive(Clone, Debug)]
pub struct Setup {
    /// axross's configuration folder (`~/.config/axross`).
    pub config_dir: PathBuf,
    /// Offer the player to the desktop (MPRIS on the session D-Bus: media
    /// keys, the desktop's sound menu).
    pub desktop: bool,
    /// The D-Bus for MPRIS (None: the session bus).
    pub desktop_bus: Option<String>,
    /// No sound device: the sound is taken by whoever calls
    /// [`crate::engine::Shared::render`] (tests).
    pub headless_audio: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            config_dir: state::config_dir(),
            desktop: true,
            desktop_bus: None,
            headless_audio: false,
        }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Vec<String>) -> Self {
        Self::with_setup(cc, args, Setup::default())
    }

    /// The app with its state in `setup.config_dir`.
    pub fn with_setup(cc: &eframe::CreationContext<'_>, args: Vec<String>, setup: Setup) -> Self {
        style(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let wake = move || ctx.request_repaint();
        let engine = if setup.headless_audio {
            Engine::start_headless(wake)
        } else {
            Engine::start(wake)
        };
        let state_path = setup.config_dir.join(state::STATE_FILE);
        let audio_path = setup.config_dir.join(state::AUDIO_FILE);
        let saved = state::read_state(&state_path);
        let eq_settings = state::read_eq(&audio_path);
        let amp_path = setup.config_dir.join(state::AMP_FILE);
        let amp = state::read_amp(&amp_path);
        engine.send(Command::Loudness(loudness(&amp)));
        engine.send(Command::Crossfade(amp.crossfade));
        engine.send(Command::FadeOnSkip(amp.fade_on_skip));
        engine.shared.set_volume(volume_curve(saved.volume));
        let output = state::read_output(&audio_path);
        if !output.is_empty() {
            engine.send(Command::Output(Some(output.clone())));
        }
        engine
            .shared
            .muted
            .store(saved.muted, std::sync::atomic::Ordering::Relaxed);
        engine.send(Command::Equalizer {
            gains: eq_settings.gains,
            enabled: eq_settings.enabled,
        });
        let mut playlist = Playlist::default();
        playlist.add_tracks(saved.tracks.clone());
        playlist.set_shuffle(saved.shuffle);
        playlist.repeat = Repeat::from_key(&saved.repeat);
        let meters = crate::meters::Meters::start(engine.shared.clone());
        let mut viz_state = VizState::default();
        // the pixel faces' shaders, where egui draws with wgpu
        // (on a software rasterizer the shaders would take a CPU's time as
        // well: the CPU's own per-pixel drawing does better there;
        // AMP_GPU_SHADERS=1 uses them anyway)
        // (measured: the shaders on the GPU or a CPU thread's drawing,
        // whichever is cheaper here, the faces drawn as small as keeps them
        // cheap - a software rasterizer draws them small - and on a slow one
        // the frame rate lowered; AMP_GPU_SHADERS=1 / 0 decides instead)
        let mut slow = false;
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            viz::gpu::install(render_state);
            let choice = viz::gpu::choose(render_state);
            let gpu = match std::env::var("AMP_GPU_SHADERS").as_deref() {
                Ok("1") => true,
                Ok("0") => false,
                _ => choice.gpu,
            };
            viz_state.set_gpu(gpu);
            if gpu {
                viz_state.gpu.ms_per_pixel = choice.gpu_ms_per_pixel;
            }
            slow = choice.slow_for(gpu);
            if slow {
                // (about 400 x 225: a large window costs no more)
                viz_state.gpu.max_pixels = 90_000.0;
            }
            eprintln!(
                "axross-amp: shader faces on the {} ({:.2e} ms a pixel on the GPU, {:.2e} on the CPU){}",
                if gpu { "GPU" } else { "CPU" },
                choice.gpu_ms_per_pixel,
                choice.cpu_ms_per_pixel,
                slow_note(slow)
            );
        }
        let mut app = App {
            engine,
            playlist,
            info: TrackInfo::default(),
            cover: None,
            sizer: crate::cover::Sizer::default(),
            cover_scheme: None,
            accent: th().accent,
            cover_hue: None,
            theme_shown: ("", Color32::TRANSPARENT, ""),
            design_shown: ("", ""),
            design_preview: None,
            cover_art: None,
            haze: None,
            panel_rects: [egui::Rect::NOTHING; 2],
            controls_rect: egui::Rect::NOTHING,
            reels_turn: 0.0,
            theme_preview: None,
            monitors: if setup.desktop {
                let ctx = cc.egui_ctx.clone();
                crate::display::watch(move || ctx.request_repaint())
            } else {
                Arc::new(Mutex::new(Vec::new()))
            },
            external: None,
            reopen_display: false,
            analyzer: Analyzer::new(),
            meters,
            viz_state,
            tap: Vec::new(),
            mode: viz::known(&saved.visualizer),
            scheme_key: saved.visual_scheme.clone(),
            custom_colours: saved.visual_custom.clone(),
            options: saved.visual_options.clone(),
            volume: saved.volume,
            muted: saved.muted,
            eq: eq_settings,
            eq_open: saved.equalizer_open,
            eq_page: false,
            slow,
            budget_fps: SLOW_FPS,
            cost_from: None,
            frames_drawn: 0,
            stats: std::env::var_os("AMP_STATS").map(|p| (PathBuf::from(p), 0, Instant::now())),
            playlist_open: saved.playlist_open,
            big_visual: false,
            big_list: false,
            intense_asked: false,
            reduced_motion: desktop_reduced_motion(),
            // (watched only while followed: see update_theme)
            desktop_shade: None,
            shade_bus: setup.desktop.then(|| setup.desktop_bus.clone()),
            controls_height: 110.0,
            overview: None,
            pending_click: None,
            generation: 0,
            generations: 0,
            loaded: None,
            opening: false,
            stopped: true,
            failures: 0,
            broken: Default::default(),
            queued: None,
            stream_title: String::new(),
            notice: String::new(),
            window_title: String::new(),
            #[cfg(target_os = "linux")]
            mpris: None,

            art_url: String::new(),
            automatic: false,
            duration: None,
            seek_drag: None,
            seek_asked: None,
            output: String::new(),
            status: String::new(),
            status_seen: (String::new(), Instant::now()),
            url: String::new(),
            url_open: false,
            dialog: FileDialog::new()
                .title("Open")
                .add_file_filter_extensions("Audio and playlists", {
                    let mut e: Vec<&'static str> = playlist::AUDIO_EXTENSIONS.to_vec();
                    e.extend(playlist::PLAYLIST_EXTENSIONS);
                    e
                })
                .default_file_filter("Audio and playlists"),
            picking: Picking::Nothing,
            selected: None,
            chosen: Vec::new(),
            analyzer_frames: 0,
            undo: Vec::new(),
            dragging: None,
            dirty: false,
            eq_dirty: false,
            last_save: Instant::now(),
            last_frame: Instant::now(),
            tab: Tab::Player,
            mixer_page: Default::default(),
            #[cfg(target_os = "linux")]
            bluetooth_page: Default::default(),
            output_choice: output,
            gate: KeyGate::default(),
            wheel: 0.0,
            scroll_to: None,
            adding: Default::default(),
            scanner: {
                let ctx = cc.egui_ctx.clone();
                Scanner::start(move || ctx.request_repaint())
            },
            album_covers: std::collections::HashMap::new(),
            album_loader: {
                let ctx = cc.egui_ctx.clone();
                AlbumCovers::start(move || ctx.request_repaint())
            },
            filter: String::new(),
            focus_filter: false,
            visible: Vec::new(),
            visible_for: (String::new(), u64::MAX),
            ctx: cc.egui_ctx.clone(),
            amp,
            amp_dirty: false,
            saver: crate::saver::Saver::start(
                setup.config_dir.clone(),
                state::read_base(&setup.config_dir.join(state::STATE_FILE)),
            ),
            eq_changed: Instant::now(),
            sleep: None,
            details: None,
            fullscreen: false,
            label_for: "",
            label_until: 0.0,
        };
        #[cfg(target_os = "linux")]
        if setup.desktop {
            let ctx = cc.egui_ctx.clone();
            let shared = app.engine.shared.clone();
            app.mpris = Some(match crate::mpris::Mpris::take_prepared() {
                // (claimed by main before the window)
                Some(mpris) => {
                    mpris.attach(shared, move || ctx.request_repaint());
                    mpris
                }
                None => crate::mpris::Mpris::start(setup.desktop_bus.clone(), shared, move || {
                    ctx.request_repaint()
                }),
            });
        }
        app.scanner.scan(&app.playlist.tracks);
        // (the display view open when it ended: opened there again once
        // the monitors are known)
        app.reopen_display = app.amp.display_open;
        if !args.is_empty() {
            app.add(&args, Then::Play);
        } else if saved.current >= 0 {
            app.play_index(saved.current as usize, saved.position_ms, false);
        }
        app
    }

    /// The engine (tests feed its output tap).
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The visualization shown.
    pub fn mode(&self) -> &'static str {
        self.mode
    }

    /// The tab shown.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// The output device chosen ("" the default).
    pub fn output_choice(&self) -> &str {
        &self.output_choice
    }

    /// What the Mixer tab changed of the player.
    fn apply(&mut self, change: crate::mixer_ui::PlayerChange) {
        use crate::mixer_ui::PlayerChange;
        match change {
            PlayerChange::Output(id) => {
                self.engine
                    .send(Command::Output((!id.is_empty()).then(|| id.clone())));
                self.saver.save(crate::saver::Job::Output(id.clone()));
                self.output_choice = id;
            }
            PlayerChange::Volume(v) => self.set_volume(v),
            PlayerChange::Muted(m) => self.set_muted(m),
            PlayerChange::Equalizer(on) => {
                self.eq.enabled = on;
                let preset = self.eq.preset.clone();
                self.set_eq(self.eq.gains, Some(&preset));
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn bluetooth_tab(&mut self, ui: &mut Ui) {
        let wake = ui.ctx().clone();
        let changes =
            crate::bluetooth_ui::show(ui, &mut self.bluetooth_page, move || wake.request_repaint());
        for change in changes {
            if matches!(change, crate::mixer_ui::PlayerChange::Output(_)) {
                self.mixer_page.devices = None; // (the Mixer tab lists them again)
            }
            self.apply(change);
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn bluetooth_tab(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Bluetooth").strong().size(16.0));
        ui.label(
            RichText::new(
                "Bluetooth devices are connected here through BlueZ, the Linux Bluetooth \
                 service. On this system, pair and connect them in the system's settings; \
                 they then appear as outputs in the Mixer tab.",
            )
            .color(th().muted),
        );
    }

    /// The Bluetooth page (tests).
    #[cfg(target_os = "linux")]
    pub fn bluetooth_page(&self) -> &crate::bluetooth_ui::BluetoothPage {
        &self.bluetooth_page
    }

    /// The equalizer as set.
    pub fn equalizer_settings(&self) -> &EqSettings {
        &self.eq
    }

    /// The message of the status line.
    /// The output the engine plays on (or "no sound: why").
    pub fn output(&self) -> &str {
        &self.output
    }

    /// No output to play on: said above everything, with Retry (the engine
    /// also tries again by itself every few seconds).
    fn no_sound_banner(&mut self, ui: &mut Ui) {
        let Some(why) = self.output.strip_prefix("no sound: ") else {
            return;
        };
        let why = why.to_owned();
        egui::Frame::new()
            .fill(crate::theme::warn_fill(th()))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("⚠ No sound – {why}")).color(th().warn),
                        )
                        .wrap(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Retry").clicked() {
                            self.engine.send(Command::Output(
                                (!self.output_choice.is_empty())
                                    .then(|| self.output_choice.clone()),
                            ));
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// Whether the track asked for last has been opened.
    pub fn is_loaded(&self) -> bool {
        self.loaded == Some(self.generation)
    }

    /// Whether the visualization is shown large.
    /// The shader faces at the window's full resolution even on a slow
    /// renderer (the README's pictures).
    #[doc(hidden)]
    pub fn full_resolution(&mut self) {
        self.viz_state.gpu.max_pixels = 0.0;
        self.viz_state.gpu.ms_per_pixel = 0.0;
        self.viz_state.gpu.scale = 1.0;
    }

    /// How the pictures move: "calm" or "intense" (tests).
    pub fn motion(&self) -> &str {
        &self.amp.motion
    }

    pub fn big_visual(&self) -> bool {
        self.big_visual
    }

    /// Show the visualization `key` (tests, screenshots).
    pub fn set_mode(&mut self, key: &str) {
        self.mode = viz::known(key);
    }

    // -- the player ------------------------------------------------------------------------

    /// Add files, folders, playlists and URLs (found on a thread of their
    /// own: a big folder on a network share does not hold the window).
    fn add(&mut self, args: &[String], then: Then) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let args = args.to_vec();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("amp-add".into())
            .spawn(move || {
                let collected = playlist::collect_with(&args, &mut |n| {
                    let _ = tx.send(AddMessage::Found(n));
                    ctx.request_repaint();
                });
                let _ = tx.send(AddMessage::Done(collected));
                ctx.request_repaint();
            });
        if let Err(e) = spawned {
            self.status = format!("Cannot add: {e}");
            return;
        }
        self.adding.push_back(Adding { rx, then, found: 0 });
    }

    /// What the adding threads found: into the playlist, in their order.
    fn poll_adding(&mut self) {
        while let Some(job) = self.adding.front_mut() {
            let mut done = None;
            loop {
                match job.rx.try_recv() {
                    Ok(AddMessage::Found(n)) => job.found = n,
                    Ok(AddMessage::Done(c)) => {
                        done = Some(c);
                        break;
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        done = Some(playlist::Collected::default());
                        break;
                    }
                }
            }
            let Some(collected) = done else {
                self.status = format!("Adding… {} tracks found", job.found);
                return;
            };
            let then = job.then;
            self.adding.pop_front();
            self.take_added(collected, then);
        }
    }

    fn take_added(&mut self, collected: playlist::Collected, then: Then) {
        let first = self.playlist.tracks.len();
        // a track already in the list is not added again (the one there plays)
        let mut present: std::collections::HashMap<String, usize> = self
            .playlist
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.path.clone(), i))
            .collect();
        let mut first_known = None;
        let mut fresh = Vec::new();
        let mut again = 0;
        for t in collected.tracks {
            match present.get(&t.path) {
                Some(i) => {
                    again += 1;
                    first_known = first_known.or(Some(*i));
                }
                None => {
                    present.insert(t.path.clone(), usize::MAX);
                    fresh.push(t);
                }
            }
        }
        let offered = fresh.len() + again;
        let count = self.playlist.add_tracks(fresh);
        self.scanner.scan(&self.playlist.tracks[first..]);
        let mut status = match (count, collected.skipped.len()) {
            (0, 0) if offered == 0 => "Nothing to add.".to_owned(),
            (0, 0) => String::new(),
            (n, 0) => format!("{n} added"),
            (n, s) => format!("{n} added, {s} skipped (not audio)"),
        };
        if again > 0 {
            if !status.is_empty() {
                status.push_str("; ");
            }
            status.push_str(&format!(
                "{again} already in the playlist (not added again)"
            ));
        }
        if collected.truncated || count + again < offered {
            status.push_str(&format!(
                "; the playlist holds at most {} tracks - the rest was left out",
                crate::state::MAX_TRACKS
            ));
        }
        self.status = status;
        self.dirty = true;
        let play = match then {
            Then::Nothing => false,
            Then::PlayIfIdle => !self.playing(),
            Then::Play => true,
        };
        if play && count > 0 {
            self.play_index(first, 0, true);
        } else if play && let Some(i) = first_known.filter(|i| *i != usize::MAX) {
            self.play_index(i, 0, true);
        }
    }

    /// Keep the playlist as it is, for Ctrl+Z (before a removal or Clear).
    fn remember_for_undo(&mut self) {
        let playing = self
            .playlist
            .current
            .and_then(|i| self.playlist.tracks.get(i))
            .map(|t| t.path.clone());
        self.undo.push((self.playlist.tracks.clone(), playing));
        if self.undo.len() > 20 {
            self.undo.remove(0);
        }
    }

    /// Ctrl+Z: the playlist as it was before the last removal or Clear.
    fn undo(&mut self) {
        let Some((tracks, playing)) = self.undo.pop() else {
            return;
        };
        let current = playing.and_then(|p| tracks.iter().position(|t| t.path == p));
        let n = tracks.len();
        self.playlist.replace(tracks, current);
        self.select(None);
        self.dirty = true;
        self.status = format!("Undone: the playlist has its {n} tracks again.");
    }

    /// Titles and lengths the scanner read: into the tracks that lack them.
    fn poll_scanner(&mut self) {
        let found: std::collections::HashMap<String, Scanned> =
            self.scanner.rx.try_iter().collect();
        if found.is_empty() {
            return;
        }
        for track in &mut self.playlist.tracks {
            match found.get(&track.path) {
                Some(Ok(tags)) => fill_tags(track, tags),
                Some(Err(why)) => {
                    self.broken.insert(track.path.clone(), why.clone());
                }
                None => {}
            }
        }
        self.playlist.version += 1;
        self.dirty = true;
    }

    /// The rows the filter lets through (all without one).
    fn visible_rows(&mut self) -> &[usize] {
        let key = (self.filter.clone(), self.playlist.version);
        if self.visible_for != key {
            let words = playlist::words(&self.filter);
            self.visible = (0..self.playlist.tracks.len())
                .filter(|i| playlist::matches(&self.playlist.tracks[*i], &words))
                .collect();
            self.visible_for = key;
        }
        &self.visible
    }

    fn play_index(&mut self, index: usize, start_ms: u64, play: bool) {
        let Some(track) = self.playlist.tracks.get(index).cloned() else {
            return;
        };
        self.generation = self.new_generation();
        self.automatic = false;
        self.opening = true;
        self.stopped = false;
        self.queued = None;
        self.playlist.play(index);
        self.select(Some(index));
        self.info = TrackInfo::default();
        self.cover = None;
        self.cover_art = None;
        self.haze = None;
        self.viz_state.cover_thumb = None;
        self.sizer.set(None);
        self.cover_scheme = None;
        self.stream_title.clear();
        self.notice.clear();
        self.duration = (track.duration_ms > 0).then(|| track.duration_ms as f64 / 1000.0);
        if playlist::is_stream(&track.path) {
            self.status = format!("Connecting to {}…", host_of(&track.path));
        }
        self.engine.send(Command::Load {
            path: track.path,
            start_ms,
            play,
            generation: self.generation,
        });
        self.dirty = true;
    }

    fn new_generation(&mut self) -> u64 {
        self.generations += 1;
        self.generations
    }

    /// Name the track that follows the current one to the engine (it
    /// decodes it on without a gap), whenever that changes.
    fn sync_queue(&mut self) {
        let want = if self.loaded == Some(self.generation)
            && !self.current_is_stream()
            && self.sleep != Some(Sleep::EndOfTrack)
        {
            self.playlist
                .peek_next()
                .and_then(|i| Some((i, self.playlist.tracks.get(i)?.path.clone())))
                .filter(|(_, p)| !playlist::is_stream(p))
        } else {
            None
        };
        let same = match (&self.queued, &want) {
            (Some((i, p, _)), Some((wi, wp))) => i == wi && p == wp,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.queued = want.map(|(i, p)| (i, p, self.new_generation()));
        self.engine
            .send(Command::Queue(self.queued.as_ref().map(|(_, p, g)| {
                crate::engine::Queued {
                    path: p.clone(),
                    generation: *g,
                }
            })));
    }

    /// The details of the track that plays now.
    /// A track's information as if the engine had just read it (tests).
    #[doc(hidden)]
    pub fn show_info(&mut self, ctx: &egui::Context, info: TrackInfo) {
        self.take_info(ctx, info);
    }

    fn take_info(&mut self, ctx: &egui::Context, info: TrackInfo) {
        self.loaded = Some(self.generation);
        self.analyzer.new_track();
        self.opening = false;
        self.failures = 0;
        self.duration = info.duration.or(self.duration);
        if let Some(i) = self.playlist.current
            && let Some(track) = self.playlist.tracks.get_mut(i)
        {
            self.broken.remove(&track.path);
            fill_tags(track, &tags_of(&info));
            if track.title.is_empty() && !info.station.is_empty() {
                track.title = info.station.clone();
            }
            if let Some(d) = info.duration {
                track.duration_ms = (d * 1000.0) as u64;
            }
            self.playlist.version += 1;
        }
        self.cover = info.art.as_deref().map(|art| cover_texture(ctx, art));
        self.cover_art = info.art.clone();
        self.haze = None;
        self.viz_state.cover_thumb = info.art.as_deref().map(|art| {
            std::sync::Arc::new(viz::Thumb::from_rgba(
                art.width as usize,
                art.height as usize,
                &art.rgba,
            ))
        });
        self.sizer.set(info.art.clone());
        self.cover_scheme = info
            .art
            .as_deref()
            .and_then(|art| viz::cover_scheme(&art.rgba));
        self.cover_hue = info.art.as_deref().and_then(cover_hue);
        self.update_theme(ctx);
        self.art_url.clear();
        #[cfg(target_os = "linux")]
        if self.mpris.is_some() {
            // (written by the writer thread: its file:// URL comes back)
            self.saver.save(crate::saver::Job::Cover(
                info.art.as_ref().map(|a| a.png.clone()),
            ));
        }
        self.info = info;
        // what was said about opening it is over (a note about a track
        // skipped on the way, or about what was added, stays)
        let skipped = self.automatic && self.status.ends_with("skipped.");
        if !skipped
            && (self.status.starts_with("Connecting to ") || self.status.starts_with("Cannot play"))
        {
            self.status.clear();
        }
        self.dirty = true;
    }

    /// Seek in the track playing (past its end: the next one, as at its end).
    fn seek(&mut self, secs: f64) {
        if !(self.info.seekable && self.loaded == Some(self.generation)) {
            return;
        }
        let secs = secs.max(0.0);
        if let Some(duration) = self.duration
            && secs >= duration
        {
            // (as at its end; paused, the next one waits paused)
            let play = self.playing();
            return self.next_as(true, play);
        }
        self.engine.send(Command::Seek(secs));
        self.seek_asked = Some((secs, Instant::now()));
        #[cfg(target_os = "linux")]
        if let Some(m) = &self.mpris {
            m.seeked(secs);
        }
    }

    /// Where the track is, as shown: a seek just asked for until the sound
    /// has caught up with it (or a second has gone by), else the sound's.
    pub fn shown_position(&mut self) -> f64 {
        let heard = self.engine.shared.position();
        match self.seek_asked {
            Some((at, when))
                if (heard - at).abs() > 0.5 && when.elapsed() < Duration::from_secs(1) =>
            {
                at
            }
            _ => {
                self.seek_asked = None;
                heard
            }
        }
    }

    /// Whether the track can be wound (loaded and seekable).
    fn can_seek(&self) -> bool {
        self.info.seekable && self.loaded == Some(self.generation)
    }

    /// Back or on by `secs` (±15 s: the buttons, J and L): from 0:00 at
    /// the least; past the end, on to the next track as at its end (or,
    /// the last, stopped there). Paused, it stays paused.
    pub fn wind(&mut self, secs: f64) {
        if !self.can_seek() {
            return;
        }
        let from = self.shown_position();
        self.seek((from + secs).max(0.0));
    }

    /// Set the sleep timer (None: off).
    pub fn set_sleep(&mut self, sleep: Option<Sleep>) {
        self.sleep = sleep;
        // (a fade under way is taken back)
        self.engine.shared.set_volume(volume_curve(self.volume));
    }

    /// The sleep timer: the fade over its last seconds, then the pause.
    fn sleep_tick(&mut self) {
        let Some(Sleep::At(at)) = self.sleep else {
            return;
        };
        let left = at.saturating_duration_since(Instant::now());
        if left.is_zero() {
            self.engine.send(Command::Pause);
            self.set_sleep(None);
            self.status = "The sleep timer paused the music.".into();
            return;
        }
        let fade = (left.as_secs_f32() / SLEEP_FADE.as_secs_f32()).min(1.0);
        self.engine
            .shared
            .set_volume(volume_curve(self.volume) * fade);
        self.ctx.request_repaint_after(if fade < 1.0 {
            Duration::from_millis(50)
        } else {
            (left - SLEEP_FADE).min(Duration::from_secs(1))
        });
    }

    /// The timer's words beside the controls ("Sleep: 15 min").
    fn sleep_label(&self) -> Option<String> {
        match self.sleep? {
            Sleep::EndOfTrack => Some("Sleep: end of track".into()),
            Sleep::At(at) => {
                let secs = at.saturating_duration_since(Instant::now()).as_secs();
                Some(format!("Sleep: {} min", secs.div_ceil(60).max(1)))
            }
        }
    }

    /// The "⋯" menu: the sleep timer, ReplayGain, the track's details, the mixer.
    fn more_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Sleep timer", |ui| {
            let now = self.sleep;
            if ui.radio(now.is_none(), "Off").clicked() {
                self.set_sleep(None);
                ui.close();
            }
            for minutes in [15u64, 30, 45, 60, 90] {
                if ui.radio(false, format!("{minutes} minutes")).clicked() {
                    self.set_sleep(Some(Sleep::At(
                        Instant::now() + Duration::from_secs(minutes * 60),
                    )));
                    ui.close();
                }
            }
            if ui
                .radio(now == Some(Sleep::EndOfTrack), "At the end of the track")
                .clicked()
            {
                self.set_sleep(Some(Sleep::EndOfTrack));
                ui.close();
            }
        });
        ui.menu_button("Crossfade", |ui| {
            let mut secs = self.amp.crossfade;
            ui.add(
                egui::Slider::new(&mut secs, 0.0..=12.0)
                    .step_by(0.5)
                    .text("seconds")
                    .custom_formatter(|v, _| {
                        if v == 0.0 {
                            "off".into()
                        } else {
                            format!("{v:.1}")
                        }
                    }),
            )
            .accessible_name("Crossfade seconds");
            ui.label(
                RichText::new(
                    "A track fades into the next - never inside an album, which plays gapless.",
                )
                .small()
                .color(th().muted),
            );
            if secs != self.amp.crossfade {
                self.amp.crossfade = secs;
                self.amp_dirty = true;
                self.engine.send(Command::Crossfade(secs));
            }
            if ui
                .checkbox(&mut self.amp.fade_on_skip, "Fade when changing track (1 s)")
                .changed()
            {
                self.amp_dirty = true;
                self.engine.send(Command::FadeOnSkip(self.amp.fade_on_skip));
            }
        });
        ui.menu_button("Loudness target", |ui| {
            for (target, name) in state::LOUDNESS_TARGETS {
                if ui
                    .radio(
                        self.amp.loudness_target == target,
                        format!("{target:.0} LUFS - {name}"),
                    )
                    .clicked()
                {
                    self.amp.loudness_target = target;
                    self.amp_dirty = true;
                    ui.close();
                }
            }
        });
        ui.menu_button("ReplayGain", |ui| {
            let mut amp = self.amp.clone();
            for (key, name) in [
                ("off", "Off"),
                ("track", "Track gain"),
                ("album", "Album gain"),
            ] {
                ui.radio_value(&mut amp.replay_gain, key.to_owned(), name);
            }
            ui.add(
                egui::Slider::new(&mut amp.untagged_db, -15.0..=0.0)
                    .step_by(0.5)
                    .text("Without tags dB"),
            );
            ui.add(
                egui::Slider::new(&mut amp.preamp_db, -15.0..=15.0)
                    .step_by(0.5)
                    .text("Pre-amp dB"),
            );
            ui.label(
                RichText::new("Levels tracks by their ReplayGain tags (a moment later: what is decoded ahead plays as it was).")
                    .small()
                    .color(th().muted),
            );
            if amp != self.amp {
                self.amp = amp;
                self.amp_dirty = true;
                self.engine.send(Command::Loudness(loudness(&self.amp)));
            }
        });
        ui.separator();
        if ui
            .add_enabled(
                self.playlist.current.is_some(),
                egui::Button::new("Track details…"),
            )
            .clicked()
        {
            if let Some(i) = self.playlist.current {
                self.show_details(i);
            }
            ui.close();
        }
        if ui.button("Audio mixer…").clicked() {
            self.tab = Tab::Mixer;
            ui.close();
        }
    }

    /// The details of track `index` (read on a thread for one not playing).
    fn show_details(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else {
            return;
        };
        let path = track.path.clone();
        let stream = playlist::is_stream(&path);
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        if self.playlist.current == Some(index) && self.loaded == Some(self.generation) {
            if let Ok(mut s) = slot.lock() {
                *s = Some(Ok(self.info.clone()));
            }
        } else if !stream {
            let (s, p, ctx) = (slot.clone(), path.clone(), self.ctx.clone());
            let _ = std::thread::Builder::new()
                .name("amp-details".into())
                .spawn(move || {
                    let info = crate::engine::probe(&p);
                    if let Ok(mut s) = s.lock() {
                        *s = Some(info);
                    }
                    ctx.request_repaint();
                });
        }
        self.details = Some(Details {
            path,
            stream,
            info: slot,
        });
    }

    fn details_window(&mut self, ctx: &egui::Context) {
        let Some(details) = &self.details else {
            return;
        };
        let mut open = true;
        let info = details.info.lock().ok().and_then(|i| i.clone());
        let playing_this = self
            .playlist
            .current
            .and_then(|i| self.playlist.tracks.get(i))
            .is_some_and(|t| t.path == details.path);
        // (Escape closes it, as a dialog)
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        // (the card's look: rounded, the panel's colour, room inside)
        let card = egui::Frame::window(&ctx.global_style())
            .corner_radius(CornerRadius::same(12))
            .fill(viz::mix(th().surface, self.accent, 0.08))
            .inner_margin(14.0);
        egui::Window::new("Track details")
            .frame(card)
            .open(&mut open)
            .collapsible(false)
            .default_width(420.0)
            // (over the window's middle, below its tabs)
            .pivot(egui::Align2::CENTER_TOP)
            .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
            .show(ctx, |ui| {
                let mut rows: Vec<(String, String)> =
                    vec![("Location".into(), details.path.clone())];
                match &info {
                    None if details.stream && playing_this => {}
                    None if details.stream => rows.push((
                        "".into(),
                        "A stream: its details come when it plays.".into(),
                    )),
                    None => rows.push(("".into(), "Reading…".into())),
                    Some(Err(why)) => rows.push(("Cannot read it".into(), why.clone())),
                    Some(Ok(i)) => {
                        let mut put = |k: &str, v: String| {
                            if !v.is_empty() {
                                rows.push((k.into(), v));
                            }
                        };
                        put("Title", i.title.clone());
                        put("Artist", i.artist.clone());
                        put("Album", i.album.clone());
                        put("Album artist", i.album_artist.clone());
                        put(
                            "Track",
                            i.track_number.map(|n| n.to_string()).unwrap_or_default(),
                        );
                        put("Date", i.date.clone());
                        put("Genre", i.genre.clone());
                        put("Station", i.station.clone());
                        put("Station's genre", i.station_genre.clone());
                        put("Length", i.duration.map(clock).unwrap_or_default());
                        let mut format = format_line(&i.codec, i.rate, i.channels, i.bits);
                        if let Some(kbps) = i.bitrate {
                            format.push_str(&format!(", {kbps} kbit/s"));
                        }
                        put("Format", format);
                        let gain = |g: Option<f32>, p: Option<f32>| match (g, p) {
                            (Some(g), Some(p)) => format!("{g:+.2} dB, peak {p:.3}"),
                            (Some(g), None) => format!("{g:+.2} dB"),
                            _ => String::new(),
                        };
                        put("ReplayGain (track)", gain(i.track_gain, i.track_peak));
                        put("ReplayGain (album)", gain(i.album_gain, i.album_peak));
                        if !i.tags.is_empty() {
                            rows.push(("".into(), "".into()));
                            for (k, v) in &i.tags {
                                rows.push((k.clone(), v.clone()));
                            }
                        }
                    }
                }
                if details.stream && playing_this && !self.stream_title.is_empty() {
                    rows.insert(1, ("Playing".into(), self.stream_title.clone()));
                }
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        egui::Grid::new("details")
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for (k, v) in rows {
                                    ui.label(RichText::new(k).color(th().muted));
                                    ui.add(egui::Label::new(v).wrap().selectable(true));
                                    ui.end_row();
                                }
                            });
                    });
            });
        if !open {
            self.details = None;
        }
    }

    fn stop(&mut self) {
        self.engine.send(Command::Stop);
        self.stopped = true;
        if self.current_is_stream() {
            // (the engine lets the stream go: Play connects anew)
            self.loaded = None;
            self.opening = false;
            self.stream_title.clear();
        }
    }

    pub fn playing(&self) -> bool {
        self.engine
            .shared
            .playing
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn toggle_play(&mut self) {
        if self.playing() {
            self.engine.send(Command::Pause);
        } else if self.loaded == Some(self.generation) || self.opening {
            self.engine.send(Command::Play);
            self.stopped = false;
        } else if let Some(i) = self
            .playlist
            .current
            .or(if self.playlist.tracks.is_empty() {
                None
            } else {
                Some(0)
            })
        {
            self.play_index(i, 0, true);
        }
    }

    fn next(&mut self, ended: bool) {
        self.next_as(ended, true)
    }

    /// The next track, played or (`play` false: paused, as MPRIS asks of
    /// Next and of a seek past the end while paused) only loaded.
    fn next_as(&mut self, ended: bool, play: bool) {
        match self.playlist.next(ended) {
            Some(i) => {
                self.play_index(i, 0, play);
                self.automatic = ended;
            }
            None => {
                self.stop();
                self.status = "End of the playlist".into();
            }
        }
    }

    fn previous(&mut self) {
        self.previous_as(true)
    }

    /// The track before (or this one from its start), played or only
    /// loaded (`play` false: paused stays paused).
    fn previous_as(&mut self, play: bool) {
        // within the first seconds: the track before; later: this one from
        // its start (a stream cannot go back: the track before)
        let seekable = self.info.seekable && self.loaded == Some(self.generation);
        if seekable && self.engine.shared.position() > 3.0 {
            self.seek(0.0);
        } else if let Some(i) = self.playlist.previous() {
            self.play_index(i, 0, play);
        }
    }

    fn set_volume(&mut self, volume: u32) {
        self.volume = volume.min(100);
        self.engine.shared.set_volume(volume_curve(self.volume));
        self.dirty = true;
    }

    fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.engine
            .shared
            .muted
            .store(muted, std::sync::atomic::Ordering::Relaxed);
        self.dirty = true;
    }

    fn set_eq(&mut self, gains: [f32; 10], preset: Option<&str>) {
        self.eq.gains = eq::clamp(gains);
        self.eq.preset = preset
            .unwrap_or_else(|| eq::preset_of(&self.eq.gains))
            .to_owned();
        self.engine.send(Command::Equalizer {
            gains: self.eq.gains,
            enabled: self.eq.enabled,
        });
        self.eq_dirty = true;
        self.eq_changed = Instant::now();
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                Event::Output(name) => self.output = name,
                Event::Loaded { generation, info } if generation == self.generation => {
                    self.take_info(ctx, info);
                }
                Event::Advanced { generation, info } => {
                    // the queued track is heard: it is the current one now
                    let Some((index, path, queued)) = self.queued.take() else {
                        continue;
                    };
                    if queued != generation {
                        continue;
                    }
                    let index = if self
                        .playlist
                        .tracks
                        .get(index)
                        .is_some_and(|t| t.path == path)
                    {
                        Some(index)
                    } else {
                        self.playlist.tracks.iter().position(|t| t.path == path)
                    };
                    self.generation = generation;
                    self.automatic = true;
                    if let Some(index) = index {
                        let before = self.playlist.current;
                        self.playlist.play(index);
                        if self.selected == before {
                            self.select(Some(index)); // (the selection follows the music)
                        }
                    }
                    self.duration = None;
                    self.stream_title.clear();
                    self.take_info(ctx, info);
                    // the timer said: the end of the track (the next one was
                    // already decoded behind it): it stops where it begins
                    if self.sleep == Some(Sleep::EndOfTrack) {
                        self.set_sleep(None);
                        self.stop();
                        self.status =
                            "The sleep timer stopped the music at the end of the track.".into();
                    }
                }
                Event::Ended { generation } if generation == self.generation => {
                    if self.sleep == Some(Sleep::EndOfTrack) {
                        self.set_sleep(None);
                        self.stop();
                        self.status =
                            "The sleep timer stopped the music at the end of the track.".into();
                    } else {
                        self.next(true);
                    }
                }
                Event::StreamTitle { generation, title } if generation == self.generation => {
                    self.stream_title = title;
                }
                Event::Notice {
                    generation,
                    message,
                } if generation == self.generation => self.notice = message,
                Event::Failed {
                    generation,
                    message,
                } if generation == self.generation => {
                    self.opening = false;
                    self.notice.clear();
                    let track = self
                        .playlist
                        .current
                        .and_then(|i| self.playlist.tracks.get(i));
                    let name = track
                        .map(|t| playlist::default_title(&t.path))
                        .unwrap_or_default();
                    if let Some(t) = track {
                        self.broken.insert(t.path.clone(), message.clone());
                    }
                    self.failures += 1;
                    // playing on its own: the next one - unless every track failed in a row
                    if self.automatic && self.failures < self.playlist.tracks.len() {
                        self.status = format!("Cannot play {name}: {message} – skipped.");
                        self.next(true);
                    } else if self.automatic {
                        self.status = format!(
                            "Cannot play {name}: {message}. No track in the list could be played."
                        );
                    } else {
                        self.status = format!("Cannot play {name}: {message}");
                    }
                }
                _ => {}
            }
        }
    }

    /// Hand what changed to the writer thread: the player's state at most
    /// every few seconds (and every 30 s while playing: the position), the
    /// equalizer a quarter second after its last change (a slider dragged
    /// is one write), this program's settings; `force`: everything, and
    /// wait until it is written (the window closes).
    fn save(&mut self, force: bool) {
        for why in self.saver.errors() {
            self.status = why;
        }
        for (kept, version) in self.saver.adopted() {
            self.adopt(&kept, version);
        }
        if let Some(url) = self.saver.art_url() {
            self.art_url = url;
        }
        let due = self.last_save.elapsed() >= SAVE_EVERY;
        // while playing, the position is kept every 30 s (as axross: a crash
        // or a power cut loses no more)
        if self.playing() && self.last_save.elapsed() >= POSITION_EVERY {
            self.dirty = true;
        }
        if force || (self.dirty && due) {
            self.saver.save(crate::saver::Job::State(
                self.snapshot(),
                self.playlist.version,
            ));
            self.dirty = false;
            self.last_save = Instant::now();
        }
        if self.eq_dirty && (force || self.eq_changed.elapsed() >= EQ_SETTLE) {
            self.saver.save(crate::saver::Job::Eq(self.eq.clone()));
            self.eq_dirty = false;
        }
        if self.amp_dirty && (force || due) {
            self.saver.save(crate::saver::Job::Amp(self.amp.clone()));
            self.amp_dirty = false;
        }
        if self.eq_dirty {
            self.ctx.request_repaint_after(EQ_SETTLE);
        }
        if force {
            self.saver.flush(Duration::from_secs(5));
        }
    }

    /// Take over what another program (axross's player, a second
    /// axross-amp) wrote and the merge kept - else this window would write
    /// its own values back at the next change. The playlist only when it was
    /// not changed here meanwhile (a change here is newer).
    fn adopt(&mut self, kept: &serde_json::Map<String, serde_json::Value>, version: u64) {
        use serde_json::Value;
        if let Some(tracks) = kept.get("tracks")
            && version == self.playlist.version
        {
            let tracks = state::tracks_of(tracks);
            let playing = self
                .playlist
                .current
                .and_then(|i| self.playlist.tracks.get(i))
                .map(|t| t.path.clone());
            let theirs = kept
                .get("current")
                .and_then(Value::as_i64)
                .filter(|c| *c >= 0)
                .map(|c| c as usize);
            // the track playing here stays the current one where it is in theirs
            let current = match &playing {
                Some(p) if self.playing() => tracks.iter().position(|t| t.path == *p),
                _ => theirs,
            };
            self.playlist.replace(tracks, current);
            self.select(None);
            self.queued = None;
            self.scanner.scan(&self.playlist.tracks);
            self.status = "The playlist was changed by another program: taken over.".into();
        }
        if let Some(v) = kept.get("volume").and_then(Value::as_u64) {
            self.set_volume(v.min(100) as u32);
        }
        if let Some(m) = kept.get("muted").and_then(Value::as_bool) {
            self.set_muted(m);
        }
        if let Some(on) = kept.get("shuffle").and_then(Value::as_bool) {
            self.playlist.set_shuffle(on);
        }
        if let Some(r) = kept.get("repeat").and_then(Value::as_str) {
            self.playlist.repeat = Repeat::from_key(r);
        }
        if let Some(v) = kept.get("visualizer").and_then(Value::as_str) {
            self.mode = viz::known(v);
        }
        if let Some(v) = kept.get("visual_scheme").and_then(Value::as_str) {
            self.scheme_key = v.to_owned();
        }
    }

    /// Files the writer thread wrote (tests).
    pub fn files_written(&self) -> usize {
        self.saver.writes()
    }

    /// What would be saved now (also for tests).
    pub fn snapshot(&self) -> PlayerState {
        let current = self.playlist.current.map(|i| i as i64).unwrap_or(-1);
        let position_ms = if self.loaded == Some(self.generation) && !self.current_is_stream() {
            (self.engine.shared.position() * 1000.0) as u64
        } else {
            0
        };
        PlayerState {
            volume: self.volume,
            muted: self.muted,
            shuffle: self.playlist.shuffle,
            repeat: self.playlist.repeat.key().into(),
            tracks: self.playlist.tracks.clone(),
            current,
            position_ms,
            visualizer: self.mode.into(),
            visual_scheme: self.scheme_key.clone(),
            visual_custom: self.custom_colours.clone(),
            visual_options: self.options.clone(),
            equalizer_open: self.eq_open,
            playlist_open: self.playlist_open,
        }
    }

    fn current_is_stream(&self) -> bool {
        self.playlist
            .current
            .and_then(|i| self.playlist.tracks.get(i))
            .is_some_and(|t| playlist::is_stream(&t.path))
    }

    // -- the parts of the window -----------------------------------------------------------

    /// Which of the player's keys the window takes this frame: none on
    /// another tab, while a text is typed (a URL, a file's name) or a file
    /// dialog is open; Space and Enter not while a widget has the keyboard's
    /// focus - they press that widget.
    fn key_gate(&self, ctx: &egui::Context) -> KeyGate {
        let typing = ctx.text_edit_focused()
            || self.url_open
            || matches!(self.dialog.state(), egui_file_dialog::DialogState::Open);
        let any = self.tab == Tab::Player && !typing;
        KeyGate {
            any,
            activation: any && ctx.memory(|m| m.focused().is_none()),
        }
    }

    /// The player's keys (axross's, and a few of its own):
    /// Space play / pause, ←/→ seek 5 s (Shift: 30 s), Ctrl+←/→ previous /
    /// next, ↑/↓ and +/− volume, Home the track's start, M mute, N / P next
    /// / previous, V the next face; in the playlist Ctrl+↑/↓ choose a
    /// track, Alt+↑/↓ move it, Enter plays it, Delete removes it.
    fn keys(&mut self, ui: &Ui) {
        // Ctrl+F: the playlist's filter (also from another text field)
        if self.tab == Tab::Player
            && !matches!(self.dialog.state(), egui_file_dialog::DialogState::Open)
            && ui.input(|i| i.modifiers.command && i.key_pressed(Key::F))
        {
            self.focus_filter = true;
            if !self.playlist_open {
                self.playlist_open = true;
                let id = playlist_header_id();
                let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    true,
                );
                state.set_open(true);
                state.store(ui.ctx());
            }
            return;
        }
        if !self.gate.any {
            return;
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(Key::Z)) {
            self.undo();
            return;
        }
        let pressed = |k: Key| ui.input(|i| i.key_pressed(k));
        let modifiers = ui.input(|i| i.modifiers);
        let pos = self.engine.shared.position();
        if pressed(Key::Space) && self.gate.activation {
            self.toggle_play();
        }
        // (the arrows alone belong to a focused widget - a slider, the
        // knob, a row: it moves itself, or the focus; with Ctrl or Alt
        // they are the window's)
        let free = self.gate.activation;
        let arrows = |m: egui::Modifiers| free || m.command || m.alt;
        for (key, sign) in [(Key::ArrowLeft, -1.0), (Key::ArrowRight, 1.0)] {
            if !pressed(key) || !arrows(modifiers) {
                continue;
            }
            if modifiers.command {
                if sign > 0.0 {
                    self.next(false);
                } else {
                    self.previous();
                }
            } else {
                let step = if modifiers.shift { 30.0 } else { 5.0 };
                self.seek(pos + sign * step);
            }
        }
        // D: the pictures on the display last chosen (or the first other
        // than the window's), or back
        if pressed(Key::D) && !modifiers.any() {
            if self.external.is_some() {
                self.close_display(None);
            } else {
                let monitors = self.monitors();
                let chosen = crate::display::find(
                    &monitors,
                    &self.amp.display_connector,
                    &self.amp.display_name,
                )
                .or_else(|| monitors.iter().find(|m| !m.primary))
                .map(|m| m.connector.clone());
                match chosen {
                    Some(c) => {
                        let sound = self.amp.display_sound;
                        if let Err(why) = self.show_on(&c, sound) {
                            self.notice = why;
                        }
                    }
                    None => self.notice = "No other display is connected".into(),
                }
            }
        }
        // (15 s back and on: J and L, as video players have them - the
        // arrows already seek 5 s, 30 s with Shift)
        if pressed(Key::J) && !modifiers.any() {
            self.wind(-15.0);
        }
        if pressed(Key::L) && !modifiers.any() {
            self.wind(15.0);
        }
        for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
            if !pressed(key) || !arrows(modifiers) {
                continue;
            }
            if modifiers.alt {
                self.move_selected(up);
            } else if modifiers.command {
                self.select_step(up);
            } else {
                self.volume_step(up);
            }
        }
        if pressed(Key::Plus) || pressed(Key::Equals) {
            self.volume_step(true);
        }
        if pressed(Key::Minus) {
            self.volume_step(false);
        }
        if pressed(Key::Home) {
            self.seek(0.0);
        }
        if pressed(Key::F11) {
            self.set_fullscreen(ui.ctx(), !self.fullscreen);
        }
        // (Escape closes the innermost thing only: a dialog open or a field
        // being typed in takes it, not the large picture behind them)
        let inner = self.details.is_some()
            || self.url_open
            || self.intense_asked
            || ui.ctx().memory(|m| m.focused().is_some());
        if pressed(Key::Escape) && !inner {
            if self.fullscreen {
                self.set_fullscreen(ui.ctx(), false);
            } else if self.big_visual {
                self.big_visual = false;
            }
        }
        if pressed(Key::M) {
            self.set_muted(!self.muted);
        }
        if pressed(Key::N) {
            self.next(false);
        }
        if pressed(Key::P) {
            self.previous();
        }
        if pressed(Key::V) {
            self.mode = if modifiers.shift {
                viz::previous(self.mode)
            } else {
                viz::next(self.mode)
            };
            self.dirty = true;
        }
    }

    fn volume_step(&mut self, up: bool) {
        self.set_volume(if up {
            self.volume + 5
        } else {
            self.volume.saturating_sub(5)
        });
    }

    /// The selection one row up or down (shown).
    fn select_step(&mut self, up: bool) {
        let rows = self.visible_rows().to_vec();
        if rows.is_empty() {
            return;
        }
        let at = self
            .selected
            .or(self.playlist.current)
            .and_then(|i| rows.iter().position(|r| *r == i));
        let next = match (at, up) {
            (None, _) => 0,
            (Some(r), true) => r.saturating_sub(1),
            (Some(r), false) => (r + 1).min(rows.len() - 1),
        };
        self.select(Some(rows[next]));
        self.scroll_to = Some(rows[next]);
    }

    /// Choose one row (and only it).
    fn select(&mut self, row: Option<usize>) {
        self.selected = row;
        self.chosen.clear();
    }

    /// The rows chosen: several (Ctrl / Shift-click), else the one selected.
    fn chosen_rows(&self) -> Vec<usize> {
        if self.chosen.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.chosen.clone()
        }
    }

    /// A click on row `i`: alone; Ctrl added / taken out; Shift the rows
    /// from the one chosen before to it (as shown: `rows`).
    fn click_row(&mut self, i: usize, modifiers: egui::Modifiers, rows: &[usize]) {
        if modifiers.command {
            let mut chosen = self.chosen_rows();
            match chosen.iter().position(|r| *r == i) {
                Some(at) => {
                    chosen.remove(at);
                }
                None => chosen.push(i),
            }
            chosen.sort_unstable();
            self.chosen = chosen;
            self.selected = Some(i);
        } else if modifiers.shift
            && let Some(anchor) = self.selected
            && let (Some(a), Some(b)) = (
                rows.iter().position(|r| *r == anchor),
                rows.iter().position(|r| *r == i),
            )
        {
            let mut chosen: Vec<usize> = rows[a.min(b)..=a.max(b)].to_vec();
            chosen.sort_unstable();
            self.chosen = chosen;
        } else {
            self.select(Some(i));
        }
    }

    /// Remove rows; `then_next`: the row that takes the first one's place is
    /// chosen (Delete again goes on down the list), else the choice follows
    /// the rows it was on.
    fn remove_rows(&mut self, gone: &[usize], then_next: bool) {
        self.remember_for_undo();
        let first = gone.iter().copied().min().unwrap_or(0);
        let before = self.chosen_rows();
        self.playlist.remove_many(gone);
        let left = self.playlist.tracks.len();
        if then_next || before.iter().all(|r| gone.contains(r)) {
            self.select((left > 0 && then_next).then(|| first.min(left - 1)));
        } else {
            // (each row chosen moves up by the rows removed above it)
            let shift = |r: usize| r - gone.iter().filter(|g| **g < r).count();
            let kept: Vec<usize> = before
                .iter()
                .filter(|r| !gone.contains(r))
                .map(|r| shift(*r))
                .collect();
            self.selected = self.selected.filter(|s| !gone.contains(s)).map(shift);
            self.chosen = if kept.len() > 1 { kept } else { Vec::new() };
        }
        self.dirty = true;
        self.status = format!(
            "{} removed - Ctrl+Z brings {} back.",
            gone.len(),
            if gone.len() == 1 { "it" } else { "them" }
        );
    }

    /// The selected track one place up or down in the list.
    fn move_selected(&mut self, up: bool) {
        let Some(i) = self.selected.filter(|_| self.filter.trim().is_empty()) else {
            return; // (nothing chosen, or a filtered view: no reordering)
        };
        let to = if up {
            i.checked_sub(1)
        } else {
            (i + 1 < self.playlist.tracks.len()).then_some(i + 1)
        };
        if let Some(to) = to {
            self.playlist.move_track(i, to);
            self.select(Some(to));
            self.scroll_to = Some(to);
            self.dirty = true;
        }
    }

    /// Notches of the mouse wheel over `response` (up: positive).
    fn wheel_steps(&mut self, ui: &Ui, response: &egui::Response) -> i32 {
        if !response.hovered() {
            return 0;
        }
        self.wheel += ui.input(|i| {
            i.raw
                .events
                .iter()
                .map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, .. } => match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / 40.0,
                        egui::MouseWheelUnit::Page => delta.y * 3.0,
                    },
                    _ => 0.0,
                })
                .sum::<f32>()
        });
        let steps = self.wheel.trunc();
        self.wheel -= steps;
        steps as i32
    }

    /// The monitors connected (as last asked).
    pub fn monitors(&self) -> Vec<crate::display::Monitor> {
        self.monitors.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// The monitors connected, as a test says (a display plugged in or
    /// out).
    pub fn set_monitors(&mut self, monitors: Vec<crate::display::Monitor>) {
        if let Ok(mut m) = self.monitors.lock() {
            *m = monitors;
        }
    }

    /// The screen the flash guard's fields are a third of (tests).
    pub fn guard_screen(&self) -> Option<egui::Vec2> {
        self.viz_state.flash.screen_is
    }

    /// The last message said (the status line's notice; tests).
    pub fn notice(&self) -> &str {
        &self.notice
    }

    /// The display the pictures are shown on (its connector), if any.
    pub fn display_shown(&self) -> Option<&str> {
        self.external.as_ref().map(|e| e.monitor.connector.as_str())
    }

    /// Show the pictures on the monitor at `connector` (with the sound
    /// too: its HDMI or DisplayPort output, where it has one - else the
    /// pictures only, and said why). Remembered.
    pub fn show_on(&mut self, connector: &str, with_sound: bool) -> Result<(), String> {
        let monitors = self.monitors();
        let Some(monitor) = monitors.iter().find(|m| m.connector == connector).cloned() else {
            return Err(format!("{connector} is not connected"));
        };
        if self.external.is_some() {
            self.close_display(None);
        }
        let mut external = External {
            monitor: monitor.clone(),
            with_sound: false,
            previous_output: None,
            told: String::new(),
            told_at: f64::NEG_INFINITY,
        };
        let mut said = format!("Showing the pictures on {}", monitor.label());
        if with_sound {
            match self.sound_to(&monitor) {
                Ok(sink) => {
                    external.previous_output = Some(self.output_choice.clone());
                    external.with_sound = true;
                    self.apply(crate::mixer_ui::PlayerChange::Output(sink));
                    said.push_str(", with the sound");
                }
                Err(why) => said = format!("{said} - the sound stays here: {why}"),
            }
        }
        // (its flash guard sized for that screen: a third of it a field)
        self.viz_state.flash.screen_is = Some(vec2(
            monitor.width.max(640) as f32,
            monitor.height.max(480) as f32,
        ));
        self.external = Some(external);
        self.amp.display_connector = monitor.connector.clone();
        self.amp.display_name = monitor.name.clone();
        self.amp.display_sound = with_sound;
        self.amp.display_open = true;
        self.amp_dirty = true;
        self.notice = said;
        Ok(())
    }

    /// Close the display view: the pictures back in the window, the sound
    /// back on the output it was on (`why`: said, as when it was
    /// unplugged).
    pub fn close_display(&mut self, why: Option<String>) {
        let Some(external) = self.external.take() else {
            return;
        };
        if let Some(previous) = external.previous_output {
            self.apply(crate::mixer_ui::PlayerChange::Output(previous));
        }
        self.viz_state.flash.screen_is = None;
        if why.is_none() {
            self.amp.display_open = false;
            self.amp_dirty = true;
        }
        self.notice = why.unwrap_or_else(|| "The pictures are back here".into());
    }

    /// The sound server's output on a monitor (switching its card to the
    /// HDMI / DisplayPort profile where that makes it); its sink's name.
    fn sound_to(&mut self, monitor: &crate::display::Monitor) -> Result<String, String> {
        use crate::display::SoundOn;
        if !monitor.can_play() {
            return Err(format!("{} carries no sound", monitor.connector));
        }
        let snapshot = crate::mixer::refresh()?;
        match crate::display::sound_on(monitor, &snapshot) {
            Some(SoundOn::Sink(name)) => Ok(name),
            Some(SoundOn::Profile { card, profile }) => {
                let (args, what) = crate::mixer::Action::Profile {
                    card,
                    profile: profile.clone(),
                }
                .command()?;
                let (ok, _, err) = crate::mixer::run(&args)?;
                if !ok {
                    return Err(crate::mixer::error_sentence(&what, &err));
                }
                // (its sink comes a moment later)
                for _ in 0..15 {
                    let snapshot = crate::mixer::refresh()?;
                    if let Some(SoundOn::Sink(name)) = crate::display::sound_on(monitor, &snapshot)
                    {
                        return Ok(name);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(format!("no output came for {profile}"))
            }
            None => Err(format!("no sound output for {}", monitor.connector)),
        }
    }

    /// Each frame: the display view's display unplugged (closed, the sound
    /// back, said) or to be opened again (as it was when the program
    /// ended).
    fn follow_displays(&mut self) {
        let monitors = self.monitors();
        if let Some(e) = &self.external
            && !monitors.iter().any(|m| m.connector == e.monitor.connector)
        {
            let what = e.monitor.connector.clone();
            self.close_display(Some(format!(
                "{what} was disconnected: the pictures are back here"
            )));
        }
        if self.reopen_display && !monitors.is_empty() {
            self.reopen_display = false;
            let (connector, name, sound) = (
                self.amp.display_connector.clone(),
                self.amp.display_name.clone(),
                self.amp.display_sound,
            );
            if let Some(m) = crate::display::find(&monitors, &connector, &name) {
                let c = m.connector.clone();
                let _ = self.show_on(&c, sound);
            }
        }
    }

    /// The menu: the monitors to show the pictures on.
    fn display_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(260.0);
        let monitors = self.monitors();
        if monitors.is_empty() {
            ui.label(RichText::new("No other display is known here").color(th().muted));
        }
        let mut sound = self.amp.display_sound;
        if ui
            .checkbox(&mut sound, "With the sound (HDMI, DisplayPort)")
            .changed()
        {
            self.amp.display_sound = sound;
            self.amp_dirty = true;
        }
        for m in &monitors {
            let shown = self.display_shown() == Some(m.connector.as_str());
            let label = if m.primary {
                format!("{} - this window's", m.label())
            } else {
                m.label()
            };
            if ui.radio(shown, label).clicked() && !shown {
                if let Err(why) = self.show_on(&m.connector, sound) {
                    self.notice = why;
                }
                ui.close();
            }
        }
        if self.external.is_some() {
            ui.separator();
            if ui.button("Bring the pictures back (D)").clicked() {
                self.close_display(None);
                ui.close();
            }
        }
    }

    /// The display view: the face over the whole of it, the track's words
    /// for a moment when it changes; Escape closes it, V / Shift+V the
    /// faces, Space plays and pauses.
    fn external_view(&mut self, ctx: &egui::Context) {
        let Some(e) = &self.external else {
            return;
        };
        let monitor = e.monitor.clone();
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("axross-amp - {}", monitor.connector))
            .with_decorations(false)
            .with_fullscreen(true);
        if let Some((x, y)) = monitor.at {
            builder = builder.with_position(egui::pos2(x as f32, y as f32));
        }
        if monitor.width > 0 {
            builder = builder.with_inner_size(vec2(monitor.width as f32, monitor.height as f32));
        }
        let mut close = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("amp-display"),
            builder,
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape)) {
                    close = true;
                }
                if ctx.input(|i| i.key_pressed(Key::V)) {
                    let back = ctx.input(|i| i.modifiers.shift);
                    self.mode = if back {
                        viz::previous(self.mode)
                    } else {
                        viz::next(self.mode)
                    };
                    self.dirty = true;
                }
                if ctx.input(|i| i.key_pressed(Key::Space)) {
                    self.toggle_play();
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(Color32::BLACK))
                    .show(ctx, |ui| {
                        let rect = ui.max_rect();
                        let hover = ui.input(|i| i.pointer.hover_pos());
                        self.draw_face(ui, rect, hover);
                        self.track_words(ui, rect);
                    });
            },
        );
        if close {
            self.close_display(None);
        }
    }

    /// The track's title and artist over the display view's picture, for
    /// four seconds after it changes, fading in and out over one (calm:
    /// words, small, no flash).
    fn track_words(&mut self, ui: &mut Ui, rect: egui::Rect) {
        let now = ui.input(|i| i.time);
        let words = self
            .window_title
            .strip_prefix("axross-amp – ")
            .unwrap_or("")
            .to_owned();
        let Some(e) = self.external.as_mut() else {
            return;
        };
        if words != e.told {
            e.told = words.clone();
            e.told_at = now;
        }
        let age = (now - e.told_at) as f32;
        if words.is_empty() || age > 5.0 {
            return;
        }
        let k = (age.min(5.0 - age)).clamp(0.0, 1.0);
        ui.painter().text(
            rect.left_bottom() + vec2(32.0, -32.0),
            egui::Align2::LEFT_BOTTOM,
            words,
            egui::FontId::proportional((rect.height() / 30.0).clamp(16.0, 48.0)),
            Color32::from_white_alpha((200.0 * k) as u8),
        );
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }

    /// Chooses the window's design (as the picker does; tests, the
    /// gallery).
    pub fn set_design(&mut self, key: &str) {
        self.amp.design = crate::design::find(key).key.to_owned();
        self.amp_dirty = true;
    }

    /// The design the window shows.
    pub fn design_shown(&self) -> &'static str {
        self.design_shown.0
    }

    /// Chooses the window's theme (as the picker does; tests, the gallery).
    pub fn set_theme(&mut self, key: &str) {
        self.amp.theme = crate::theme::find(key).key.to_owned();
        self.amp.follow_system = false;
        self.amp_dirty = true;
    }

    /// Follows the desktop's light or dark (as the picker's box does).
    pub fn follow_desktop(&mut self, on: bool) {
        self.amp.follow_system = on;
        self.amp_dirty = true;
    }

    /// The theme the window shows.
    pub fn theme_shown(&self) -> &'static str {
        self.theme_shown.0
    }

    /// The theme to show: the one the pointer is on in the picker, else
    /// the chosen one (or, following the desktop, its light or dark one),
    /// with the cover's accent made to read in it; set on the window only
    /// when it changes.
    fn update_theme(&mut self, ctx: &egui::Context) {
        // the desktop watched while followed, else let go (its thread ends)
        if self.amp.follow_system {
            if self.desktop_shade.is_none()
                && let Some(bus) = &self.shade_bus
            {
                let wake = ctx.clone();
                self.desktop_shade = Some(crate::desktop_theme::watch(bus.clone(), move || {
                    wake.request_repaint()
                }));
            }
        } else {
            self.desktop_shade = None;
        }
        let chosen = if self.amp.follow_system {
            // (the desktop asked first - on Linux winit knows nothing -
            // then winit's word, else dark)
            let asked = self
                .desktop_shade
                .as_ref()
                .and_then(|s| s.lock().ok().and_then(|s| *s))
                .map(|dark| {
                    if dark {
                        egui::Theme::Dark
                    } else {
                        egui::Theme::Light
                    }
                });
            match asked.or(ctx.system_theme()) {
                Some(egui::Theme::Light) => &self.amp.light_theme,
                _ => &self.amp.dark_theme,
            }
        } else {
            &self.amp.theme
        };
        // (the picker says it again each frame the pointer stays on it)
        let kept = crate::theme::find(chosen);
        let t = self
            .theme_preview
            .take()
            .map(crate::theme::find)
            .unwrap_or(kept);
        crate::theme::set(t.key);
        let accent = match self.cover_hue {
            // (in high contrast always its own yellow: a cover's colour
            // there is one more thing to tell apart)
            Some(c) if self.amp.cover_accent && !t.high_contrast => accent_for(c, t),
            _ => t.accent,
        };
        // the design: as the theme, the one the pointer is on in the
        // picker shown at once - its panels, corners and strokes; its type
        // and room only once chosen (the picker's rows stay under the
        // pointer)
        let kept_design = crate::design::find(&self.amp.design);
        let d = self
            .design_preview
            .take()
            .map(crate::design::find)
            .unwrap_or(kept_design);
        crate::design::set(d.key);
        if self.theme_shown != (t.key, accent, kept.key)
            || self.design_shown != (d.key, kept_design.key)
        {
            self.theme_shown = (t.key, accent, kept.key);
            self.design_shown = (d.key, kept_design.key);
            self.accent = accent;
            // (the chosen theme's density, also while another is shown in
            // the picker: its rows stay under the pointer)
            apply_theme(ctx, t, accent, kept.padding, d, kept_design);
            ctx.request_repaint();
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("🎵 axross-amp")
                    .strong()
                    .color(self.accent)
                    .size(16.0),
            );
            ui.add_space(8.0);
            for (tab, label) in [
                (Tab::Player, "Player"),
                (Tab::Mixer, "Mixer"),
                (Tab::Bluetooth, "Bluetooth"),
            ] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }
        });
        if self.tab != Tab::Player {
            return;
        }
        // the player's actions: a row of their own (they did not fit beside
        // the tabs at the window's default width)
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Files, "Files", vec2(0.0, 0.0))
                .on_hover_text("Add files and playlists")
                .clicked()
            {
                self.picking = Picking::Files;
                self.dialog.pick_multiple();
            }
            if icon_button(ui, Icon::Folder, "Folder", vec2(0.0, 0.0))
                .on_hover_text("Add a folder and everything in it")
                .clicked()
            {
                self.picking = Picking::Folder;
                self.dialog.pick_directory();
            }
            if icon_button(ui, Icon::Radio, "URL", vec2(0.0, 0.0))
                .on_hover_text("Add an internet radio or a file on a web server")
                .clicked()
            {
                self.url_open = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("⚙", |ui| self.visual_menu(ui))
                    .response
                    .on_hover_text("Visualization");
                let mut mode = self.mode;
                // (narrower where the row is short; where even that does not
                // fit beside URL, left out - never over it: the faces are in
                // ⚙'s menu as well)
                let room = ui.available_width() - ui.spacing().item_spacing.x - 28.0;
                if room >= 96.0 {
                    egui::ComboBox::from_id_salt("mode")
                        .selected_text(viz::name(mode))
                        .width(room.min(150.0))
                        .height(560.0)
                        .truncate()
                        .show_ui(ui, |ui| faces_list(ui, &mut mode))
                        .response
                        .accessible_name("Visualization");
                }
                if mode != self.mode {
                    self.mode = mode;
                    self.dirty = true;
                }
            });
        });
    }

    /// The visualization's menu (⚙, and a right click on the picture):
    /// the face, the colours (the custom ones edited here), the options,
    /// full screen, the playlist shown or not - axross's context menu.
    fn visual_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Visualization").strong());
        // the groups side by side: every face at a glance, in the menu itself
        // (a submenu this wide lay over its own menu); scrolled in a low window
        // (the whole menu within the window, else it is moved about and
        // its lower rows miss their clicks: rows close together, the grid
        // scrolled where the window is low)
        let most = (ui.ctx().content_rect().height() - 300.0).max(160.0);
        egui::ScrollArea::vertical()
            .max_height(most)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.spacing_mut().interact_size.y = 18.0;
                ui.horizontal_top(|ui| {
                    for group in [
                        viz::Group::Classic,
                        viz::Group::Analysis,
                        viz::Group::Generative,
                    ] {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(group.name()).strong().color(th().muted));
                            for face in viz::FACES.iter().filter(|f| f.group == group) {
                                if ui.radio(self.mode == face.key, face.name).clicked() {
                                    self.mode = face.key;
                                    self.dirty = true;
                                    ui.close();
                                }
                            }
                            if group == viz::Group::Analysis {
                                ui.add_space(8.0);
                                if ui.radio(self.mode == "off", "Off").clicked() {
                                    self.mode = "off";
                                    self.dirty = true;
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            });
        ui.separator();
        let mut full = self.fullscreen;
        if ui.checkbox(&mut full, "Full screen (F11)").changed() {
            self.set_fullscreen(ui.ctx(), full);
            ui.close();
        }
        let mut list = self.playlist_open;
        if ui.checkbox(&mut list, "Show the playlist").changed() {
            self.show_playlist(ui.ctx(), list);
        }
        // (last: their menus open over nothing of this one but themselves)
        ui.separator();
        ui.horizontal(|ui| {
            ui.menu_button("Show on display", |ui| self.display_menu(ui));
            ui.menu_button("Design", |ui| self.design_menu(ui));
            ui.menu_button("Theme", |ui| self.theme_menu(ui));
            ui.menu_button("Colours", |ui| self.colours_menu(ui));
            ui.menu_button("Options", |ui| self.options_menu(ui));
        });
    }

    /// The window's theme: each with its colours at a glance; the pointer
    /// on one shows it at once, a click keeps it. Following the desktop's
    /// light and dark: a click sets the light or the dark one.
    /// The window's design: the pointer on one shows it at once (its
    /// panels, corners, strokes; its type once chosen), a click keeps it.
    fn design_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(300.0);
        for d in &crate::design::DESIGNS {
            let chosen = self.amp.design == d.key;
            let r = ui.radio(chosen, d.name).on_hover_text(d.character);
            if r.hovered() || r.has_focus() {
                self.design_preview = Some(d.key);
            }
            if r.clicked() {
                self.amp.design = d.key.into();
                self.amp_dirty = true;
            }
        }
        ui.separator();
        // (a theme it goes with: offered, never forced)
        let d = crate::design::current();
        let suggested = crate::theme::find(d.suggests);
        if ui
            .add_enabled(
                self.amp.theme != suggested.key,
                egui::Button::new(format!("{} as the design suggests", suggested.name)),
            )
            .clicked()
        {
            self.amp.theme = suggested.key.into();
            self.amp_dirty = true;
        }
    }

    fn theme_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(300.0);
        let follow = self.amp.follow_system;
        for t in &crate::theme::THEMES {
            let chosen = if follow {
                self.amp.light_theme == t.key || self.amp.dark_theme == t.key
            } else {
                self.amp.theme == t.key
            };
            let r = ui
                .horizontal(|ui| {
                    // its ground, its cards, its text and its accent
                    let (rect, _) = ui.allocate_exact_size(vec2(46.0, 16.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, 4.0, t.ground);
                    p.rect_stroke(
                        rect,
                        4.0,
                        Stroke::new(1.0, th().border),
                        egui::StrokeKind::Inside,
                    );
                    for (k, c) in [t.surface, t.text, t.accent].into_iter().enumerate() {
                        let at = egui::pos2(rect.left() + 10.0 + k as f32 * 13.0, rect.center().y);
                        p.circle_filled(at, 4.5, c);
                    }
                    let shade = if t.dark { "dark" } else { "light" };
                    ui.radio(chosen, t.name)
                        .on_hover_text(format!("{} ({shade})", t.character))
                })
                .inner;
            if r.hovered() {
                self.theme_preview = Some(t.key);
            }
            if r.clicked() {
                if follow {
                    if t.dark {
                        self.amp.dark_theme = t.key.into();
                    } else {
                        self.amp.light_theme = t.key.into();
                    }
                } else {
                    self.amp.theme = t.key.into();
                }
                self.amp_dirty = true;
            }
        }
        ui.separator();
        if ui
            .checkbox(
                &mut self.amp.follow_system,
                "Follow the desktop's light or dark",
            )
            .changed()
        {
            self.amp_dirty = true;
        }
        if self.amp.follow_system {
            let name = |k: &str| crate::theme::find(k).name;
            ui.label(
                RichText::new(format!(
                    "Light: {} · Dark: {}",
                    name(&self.amp.light_theme),
                    name(&self.amp.dark_theme)
                ))
                .small()
                .color(th().muted),
            );
        }
        if ui
            .checkbox(
                &mut self.amp.cover_accent,
                "The accent from the cover (made to read)",
            )
            .changed()
        {
            self.amp_dirty = true;
        }
        // (the faces' colours stay their own: a theme only suggests some)
        // (always there - greyed where its scheme is the faces' already -:
        // the menu keeps its size, whichever theme is shown)
        let faces = th().faces;
        let name = viz::SCHEMES
            .iter()
            .find(|(k, _, _)| *k == faces)
            .map_or(faces, |(_, n, _)| *n);
        if ui
            .add_enabled(
                self.scheme_key != faces,
                egui::Button::new(format!("Faces in {name}, as the theme suggests")),
            )
            .clicked()
        {
            self.scheme_key = faces.to_owned();
            self.dirty = true;
        }
    }

    fn colours_menu(&mut self, ui: &mut Ui) {
        if ui
            .checkbox(
                &mut self.amp.cover_colours,
                "From the cover, when it has one",
            )
            .changed()
        {
            self.amp_dirty = true;
        }
        let named = viz::SCHEMES.iter().map(|(k, n, _)| (*k, *n));
        for (key, name) in named.chain([("theme", "Follow the theme"), ("custom", "Custom")]) {
            if ui.radio(self.scheme_key == key, name).clicked() {
                self.scheme_key = key.to_owned();
                self.dirty = true;
            }
        }
        if self.scheme_key == "custom" {
            ui.horizontal_wrapped(|ui| {
                for (key, name, default) in viz::CUSTOM_COLOURS {
                    let now = self
                        .custom_colours
                        .get(key)
                        .map(String::as_str)
                        .unwrap_or(default);
                    let v = u32::from_str_radix(now.trim_start_matches('#'), 16).unwrap_or(0);
                    let mut rgb = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
                    if ui
                        .color_edit_button_srgb(&mut rgb)
                        .on_hover_text(name)
                        .accessible_name(format!("{name} colour"))
                        .changed()
                    {
                        self.custom_colours.insert(
                            key.to_owned(),
                            format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]),
                        );
                        self.dirty = true;
                    }
                    ui.label(RichText::new(name).small());
                }
            });
            // (a ground of saturated red is shown darker: said, not hidden)
            let ground = self
                .custom_colours
                .get("background")
                .and_then(|v| u32::from_str_radix(v.trim_start_matches('#'), 16).ok())
                .map(|v| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
            if let Some(g) = ground
                && viz::quiet_ground(g) != g
            {
                ui.label(
                    RichText::new(
                        "A saturated red ground is shown darker: on it, what moves flashes red.",
                    )
                    .small()
                    .color(th().muted),
                );
            }
        }
    }

    fn options_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Motion").strong());
        if ui
            .radio(
                self.amp.motion != "intense",
                "Calm - calmer, fewer flashes (the default)",
            )
            .clicked()
        {
            self.amp.motion = "calm".into();
            self.amp_dirty = true;
        }
        let reduce = self.desktop_reduces_motion();
        let intense = ui.add_enabled(
            !reduce,
            egui::RadioButton::new(self.amp.motion == "intense", "Intense…"),
        );
        let intense = if reduce {
            intense.on_disabled_hover_text("The desktop asks for reduced motion")
        } else {
            intense
        };
        if intense.clicked() && self.amp.motion != "intense" {
            // (only after the warning: a window of its own)
            self.intense_asked = true;
            ui.close();
        }
        ui.separator();
        if ui
            .checkbox(
                &mut self.amp.waterfall_hd,
                "Waterfall: smooth, high resolution (else axross's bars)",
            )
            .changed()
        {
            self.amp_dirty = true;
        }
        if ui
            .checkbox(
                &mut self.amp.classic_plus,
                "Starfield and stereo scope: trails and glow (else as axross draws them)",
            )
            .changed()
        {
            self.amp_dirty = true;
        }
        ui.separator();
        let mut o = self.options.clone();
        ui.add(egui::Slider::new(&mut o.bars, 8..=128).text("Bars"));
        ui.add(
            egui::Slider::new(&mut o.smoothing, crate::analyzer::SAFE_SMOOTHING..=95)
                .text("Smoothing %"),
        )
        .on_hover_text("At least 75 %: less smoothing flashes (photosensitivity)");
        // (the pictures take no less smoothing and no faster a fall than
        // these: at their extremes the faces flashed)
        ui.add(
            egui::Slider::new(&mut o.peak_fall, 5..=crate::analyzer::SAFE_PEAK_FALL)
                .text("Peak fall %/s"),
        )
        .on_hover_text("At most 100 %/s: faster falls flash (photosensitivity)");
        ui.add(egui::Slider::new(&mut o.fps, 10..=60).text("Frame rate cap"));
        ui.checkbox(&mut o.hide_cursor, "Hide the pointer in full screen");
        if o != self.options {
            self.options = o.clamped();
            self.dirty = true;
        }
    }

    /// The picture over the whole screen (F11; Escape leaves).
    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        self.big_visual = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    fn show_playlist(&mut self, ctx: &egui::Context, open: bool) {
        self.playlist_open = open;
        self.dirty = true;
        let id = playlist_header_id();
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(ctx, id, open);
        state.set_open(open);
        state.store(ctx);
    }

    fn now_playing(&mut self, ui: &mut Ui) {
        // a card tinted with the cover's colour, drawn as the design draws
        // its panels (where it lay last frame, under what lies on it)
        let tint = card_tint(self.accent);
        let d = crate::design::current();
        let fill = crate::design::card_fill(d, th(), tint);
        if self.panel_rects[0].is_positive() {
            crate::design::panel(ui.painter(), self.panel_rects[0], d, th(), fill, 1);
        }
        let shown = egui::Frame::new()
            .inner_margin(if d.panel == crate::design::Panel::Frame {
                14.0
            } else {
                10.0
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                // (wide: the cover larger, a hero, and what comes next in a
                // column of its own at the right - not 900 px of nothing)
                let wide = ui.available_width() > 900.0;
                let next = if wide {
                    self.playlist.peek_next().and_then(|i| {
                        let track = self.playlist.tracks.get(i)?;
                        let title = if track.title.is_empty() {
                            playlist::default_title(&track.path)
                        } else {
                            track.title.clone()
                        };
                        Some(row_text(track, &title))
                    })
                } else {
                    None
                };
                ui.horizontal(|ui| {
                    // (the cover as large as the window allows: 120..200 px;
                    // wide, a hero as Amberol has it - up to 320 px, lifted
                    // off the card by a soft shadow)
                    let most = if wide { 320.0 } else { 200.0 };
                    let share = if wide { 0.27 } else { 0.22 };
                    let side = (ui.available_width() * share).clamp(120.0, most);
                    let size = vec2(side, side);
                    let px = side * ui.ctx().pixels_per_point();
                    if let Some(texture) = self.sizer.get(px).or(self.cover.as_ref()) {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        if wide {
                            ui.painter().add(
                                egui::epaint::Shadow {
                                    offset: [0, 8],
                                    blur: 28,
                                    spread: 0,
                                    color: Color32::from_black_alpha(110),
                                }
                                .as_shape(rect, CornerRadius::same(10)),
                            );
                        }
                        egui::Image::new(texture)
                            .corner_radius(CornerRadius::same(if wide { 10 } else { 8 }))
                            .paint_at(ui, rect);
                    } else {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            8.0,
                            viz::mix(th().surface, self.accent, 0.3),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "♫",
                            egui::FontId::proportional(36.0),
                            viz::mix(th().muted, th().strong, 0.4),
                        );
                    }
                    ui.vertical(|ui| {
                        let (title, by) = self.now_playing_text();
                        // the type scale: the title bold and large, who in
                        // the accent, the album quieter, the format small
                        // (the design's type: its size, its family; the zine's
                        // title in capitals)
                        let d = crate::design::current();
                        let title_font = if d.mono {
                            egui::FontId::monospace(d.title)
                        } else {
                            bold(d.title)
                        };
                        let collage = d.panel == crate::design::Panel::Paper;
                        let title = if collage {
                            title.to_uppercase()
                        } else if d.mono {
                            format!("> {title}")
                        } else {
                            title
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(title).font(title_font).color(th().strong),
                            )
                            .truncate(),
                        );
                        if !by.is_empty() {
                            ui.add(
                                egui::Label::new({
                                    // (the collage: cut from another page -
                                    // typewriter, slanted)
                                    let by =
                                        RichText::new(by.clone()).size(14.0).color(self.accent);
                                    if collage {
                                        by.monospace().italics()
                                    } else {
                                        by
                                    }
                                })
                                .truncate(),
                            );
                        }
                        // (the album, unless the line above names it)
                        if !self.info.album.is_empty() && !by.contains(&self.info.album) {
                            ui.add(
                                egui::Label::new({
                                    let album = RichText::new(&self.info.album)
                                        .size(13.5)
                                        .color(th().muted);
                                    if collage { album.italics() } else { album }
                                })
                                .truncate(),
                            );
                        }
                        if self.info.rate > 0 {
                            ui.label(
                                RichText::new(card_format(&self.info))
                                    .size(11.5)
                                    .color(th().muted),
                            );
                        }
                        self.card_extras(ui);
                    });
                    if let Some((title, by)) = next {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            ui.set_max_width(260.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Up next").size(11.5).color(th().muted));
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(title).size(15.0).color(th().strong),
                                    )
                                    .truncate(),
                                );
                                if !by.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(by).size(12.5).color(th().muted),
                                        )
                                        .truncate(),
                                    );
                                }
                            });
                        });
                    }
                });
            });
        self.panel_rects[0] = shown.response.rect;
    }

    /// What a design adds to the card: a segment display of the time, a
    /// cassette's reels, VU needles, the position in characters.
    fn card_extras(&mut self, ui: &mut Ui) {
        let d = crate::design::current();
        let x = d.extras;
        if !(x.lcd || x.reels || x.needles || x.ascii) {
            return;
        }
        let pos = self.engine.shared.position();
        let total = self.duration.unwrap_or(0.0);
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        // (the reels turn while it plays, slowly - a quarter turn a
        // second - and stand where the desktop asks for less motion)
        if self.playing() && !self.desktop_reduces_motion() {
            self.reels_turn = (self.reels_turn + dt * 0.25).fract();
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if x.lcd {
                let track = self.playlist.current.map_or(0, |i| i + 1);
                let text = format!("{track:02}  {}", clock(pos));
                let at = ui.cursor().min;
                let rect = crate::design::lcd(ui.painter(), at, &text, 18.0, th());
                let said = format!("Track {track}, {}", clock(pos));
                ui.allocate_rect(rect, Sense::hover()).widget_info(|| {
                    egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, said.clone())
                });
            }
            if x.reels {
                let (rect, _) = ui.allocate_exact_size(vec2(150.0, 46.0), Sense::hover());
                crate::design::reels(ui.painter(), rect, self.reels_turn, th());
            }
            if x.needles {
                let (rect, _) = ui.allocate_exact_size(vec2(160.0, 46.0), Sense::hover());
                crate::design::needles(ui.painter(), rect, self.analyzer.vu, th());
            }
        });
        if x.ascii && total > 0.0 {
            let bar = crate::design::ascii_bar((pos / total) as f32, 24);
            ui.label(
                RichText::new(format!("{bar} {} / {}", clock(pos), clock(total)))
                    .monospace()
                    .color(th().text),
            );
        }
    }

    /// Nothing to play: a place to drop music, the ways to add it, the keys.
    fn empty_state(&mut self, ui: &mut Ui) {
        let zone = ui.available_rect_before_wrap().shrink(6.0);
        let dragging = ui.input(|i| !i.raw.hovered_files.is_empty());
        let painter = ui.painter();
        painter.rect_filled(
            zone,
            16.0,
            viz::mix(
                th().surface,
                self.accent,
                if dragging { 0.18 } else { 0.06 },
            ),
        );
        // a dashed edge round it
        let edge = if dragging {
            self.accent
        } else {
            viz::mix(th().surface, th().muted, 0.6)
        };
        let corners = [
            zone.left_top(),
            zone.right_top(),
            zone.right_bottom(),
            zone.left_bottom(),
            zone.left_top(),
        ];
        for pair in corners.windows(2) {
            painter.extend(egui::Shape::dashed_line(
                &[
                    pair[0] + (pair[1] - pair[0]).normalized() * 16.0,
                    pair[1] - (pair[1] - pair[0]).normalized() * 16.0,
                ],
                Stroke::new(1.5, edge),
                8.0,
                6.0,
            ));
        }
        let builder = egui::UiBuilder::new()
            .max_rect(zone.shrink(20.0))
            .layout(Layout::top_down(Align::Center));
        // (a low window: no note, no keys - the buttons inside the edge)
        let low = zone.height() < 460.0;
        ui.scope_builder(builder, |ui| {
            if low {
                ui.add_space(4.0);
            } else {
                ui.add_space((zone.height() * 0.18 - 40.0).max(8.0));
                ui.label(RichText::new("♫").size(56.0).color(self.accent));
                ui.add_space(4.0);
            }
            ui.label(
                RichText::new("Drop music here")
                    .font(bold(22.0))
                    .color(th().strong),
            );
            ui.label(
                RichText::new("Files, folders and playlists - or open them, or a radio's URL")
                    .size(13.5)
                    .color(th().muted),
            );
            ui.add_space(14.0);
            // (a row, centred, as wide as its buttons; one under another
            // in a narrow window)
            let size = vec2(132.0, 34.0);
            let width = 3.0 * size.x + 2.0 * ui.spacing().item_spacing.x;
            let buttons = |ui: &mut Ui, this: &mut Self| {
                if icon_button(ui, Icon::Files, "Open files…", size).clicked() {
                    this.picking = Picking::Files;
                    this.dialog.pick_multiple();
                }
                if icon_button(ui, Icon::Folder, "Open folder…", size).clicked() {
                    this.picking = Picking::Folder;
                    this.dialog.pick_directory();
                }
                if icon_button(ui, Icon::Radio, "Add a URL…", size).clicked() {
                    this.url_open = true;
                }
            };
            if ui.available_width() >= width {
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - width) / 2.0);
                    buttons(ui, self);
                });
            } else {
                buttons(ui, self);
            }
            if low {
                return;
            }
            ui.add_space(22.0);
            let keys = [
                ("Space", "play / pause"),
                ("← →", "seek (Shift: 30 s)"),
                ("N  P", "next / previous"),
                ("V", "the next picture"),
                ("F11", "the picture full screen"),
                ("Ctrl+F", "find in the playlist"),
            ];
            // (centred: as wide as it was last drawn)
            let id = egui::Id::unique("keys-width");
            let wide: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(240.0);
            let drawn = ui
                .horizontal(|ui| {
                    ui.add_space(((ui.available_width() - wide) / 2.0).max(0.0));
                    egui::Grid::new("keys")
                        .num_columns(2)
                        .spacing(vec2(14.0, 4.0))
                        .show(ui, |ui| {
                            for (key, what) in keys {
                                ui.label(RichText::new(key).monospace().color(self.accent));
                                ui.label(RichText::new(what).size(13.0).color(th().muted));
                                ui.end_row();
                            }
                        })
                        .response
                        .rect
                        .width()
                })
                .inner;
            ui.ctx().data_mut(|d| d.insert_temp(id, drawn));
        });
    }

    /// One line of what plays (the large picture's): its cover and title.
    fn title_line(&mut self, ui: &mut Ui) {
        let px = 28.0 * ui.ctx().pixels_per_point();
        let texture = self.sizer.get(px).or(self.cover.as_ref()).cloned();
        ui.horizontal(|ui| {
            if let Some(texture) = &texture {
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(vec2(28.0, 28.0))
                        .corner_radius(CornerRadius::same(4)),
                );
            }
            let (title, by) = self.now_playing_text();
            let line = if by.is_empty() {
                title
            } else {
                format!("{title} – {by}")
            };
            ui.add(egui::Label::new(RichText::new(line).size(16.0).strong()).truncate());
        });
        ui.add_space(4.0);
    }

    /// What plays now: (the title, who / where from).
    fn now_playing_text(&self) -> (String, String) {
        let track = self
            .playlist
            .current
            .and_then(|i| self.playlist.tracks.get(i));
        let stream = track.is_some_and(|t| playlist::is_stream(&t.path));
        if stream {
            // a radio: what it says it plays, by the station's name
            let station = if !self.info.station.is_empty() {
                self.info.station.clone()
            } else {
                track.map(|t| t.title.clone()).unwrap_or_default()
            };
            let url = track.map(|t| t.path.clone()).unwrap_or_default();
            let station = if station.is_empty() { url } else { station };
            let mut by = vec![];
            if !self.info.station_genre.is_empty() {
                by.push(self.info.station_genre.clone());
            }
            return if self.stream_title.is_empty() {
                (station, by.join(" · "))
            } else {
                by.insert(0, station);
                (self.stream_title.clone(), by.join(" · "))
            };
        }
        let title = if !self.info.title.is_empty() {
            self.info.title.clone()
        } else {
            track
                .map(|t| {
                    if t.title.is_empty() {
                        playlist::default_title(&t.path)
                    } else {
                        t.title.clone()
                    }
                })
                .unwrap_or_else(|| "Nothing playing".into())
        };
        let by = [self.info.artist.as_str(), self.info.album.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ");
        (title, by)
    }

    /// The window's title: the program and what plays (as axross-amp's).
    fn window_title(&mut self, ctx: &egui::Context) {
        let title = if self.playlist.current.is_some() {
            let (title, by) = self.now_playing_text();
            if by.is_empty() {
                format!("axross-amp – {title}")
            } else {
                format!("axross-amp – {title} – {by}")
            }
        } else {
            "axross-amp".to_owned()
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// What the desktop asked (MPRIS), and the state it is told.
    #[cfg(target_os = "linux")]
    fn desktop(&mut self, ctx: &egui::Context) {
        use crate::mpris::Request;
        let Some(requests) = self.mpris.as_ref().map(|m| m.requests()) else {
            return;
        };
        for request in requests {
            match request {
                Request::Play if !self.playing() => self.toggle_play(),
                Request::Play => {}
                Request::Pause => {
                    if self.playing() {
                        self.engine.send(Command::Pause);
                    }
                }
                Request::PlayPause => self.toggle_play(),
                Request::Stop => self.stop(),
                // (paused or stopped, it stays so - MPRIS)
                Request::Next => {
                    let play = self.playing();
                    self.next_as(false, play)
                }
                Request::Previous => {
                    let play = self.playing();
                    self.previous_as(play)
                }
                // (from where the last seek asked, not from what is heard
                // yet: two quick ones add up)
                Request::Seek(us) => {
                    let pos = self.shown_position();
                    self.seek(pos + us as f64 / 1e6);
                }
                Request::SetPosition(id, us) => {
                    if id == self.generation && us >= 0 {
                        self.seek(us as f64 / 1e6);
                    }
                }
                Request::Volume(v) => self.set_volume((v * 100.0).round() as u32),
                Request::Shuffle(on) => {
                    self.playlist.set_shuffle(on);
                    self.dirty = true;
                }
                Request::Loop(mode) => {
                    self.playlist.repeat = match mode.as_str() {
                        "Track" => Repeat::One,
                        "Playlist" => Repeat::All,
                        _ => Repeat::Off,
                    };
                    self.dirty = true;
                }
                Request::OpenUri(uri) => {
                    let target = match uri.strip_prefix("file://") {
                        Some(path) => playlist::percent_decode(path),
                        None => uri,
                    };
                    self.add(&[target], Then::Play);
                }
                Request::Open(items) => self.add(&items, Then::Play),
                Request::Raise => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Request::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        let snapshot = self.mpris_snapshot();
        if let Some(m) = self.mpris.as_mut() {
            m.update(snapshot);
        }
    }

    #[cfg(target_os = "linux")]
    fn mpris_snapshot(&self) -> crate::mpris::Snapshot {
        let loaded = self.loaded == Some(self.generation);
        let status = if self.playing() {
            "Playing"
        } else if loaded && !self.stopped {
            "Paused"
        } else {
            "Stopped"
        };
        let track = self
            .playlist
            .current
            .and_then(|i| self.playlist.tracks.get(i))
            .map(|t| {
                let (title, by) = self.now_playing_text();
                let stream = playlist::is_stream(&t.path);
                crate::mpris::Track {
                    id: self.generation,
                    title,
                    artist: if stream { by } else { self.info.artist.clone() },
                    album: self.info.album.clone(),
                    url: if stream {
                        t.path.clone()
                    } else {
                        file_url(&t.path)
                    },
                    length_us: self.duration.map(|d| (d * 1e6) as i64),
                    art_url: self.art_url.clone(),
                }
            });
        let any = !self.playlist.tracks.is_empty();
        crate::mpris::Snapshot {
            status,
            loop_status: match self.playlist.repeat {
                Repeat::Off => "None",
                Repeat::One => "Track",
                Repeat::All => "Playlist",
            },
            shuffle: self.playlist.shuffle,
            volume: self.volume as f64 / 100.0,
            can_seek: loaded && self.info.seekable && self.duration.is_some(),
            can_next: any,
            can_previous: any,
            track,
        }
    }

    /// How much of the current track's waveform is read (0..1; tests).
    pub fn overview_read(&self) -> f32 {
        self.overview
            .as_ref()
            .and_then(|(_, o, _)| o.lock().ok().map(|o| o.done))
            .unwrap_or(0.0)
    }

    /// Frames a second at most: the option, lowered on a slow renderer.
    /// The pictures' colours: the cover's when wanted and it has some.
    fn scheme(&self) -> viz::Scheme {
        match &self.cover_scheme {
            Some(s) if self.amp.cover_colours && self.scheme_key != "custom" => {
                // (the generative faces each a neighbour of the cover's hues)
                if viz::find(self.mode).is_some_and(|f| f.group == viz::Group::Generative) {
                    viz::turned(s, viz::face_turn(self.mode))
                } else {
                    *s
                }
            }
            _ => viz::scheme(&self.scheme_key, &self.custom_colours),
        }
    }

    fn fps_cap(&self) -> f32 {
        let mut fps = self.options.fps.clamp(10, 60) as f32;
        if self.slow {
            fps = fps.min(SLOW_FPS).min(self.budget_fps);
        }
        // (a calm face on a slow renderer: fewer frames; on a GPU its beat
        // ring and spectrum move at the full rate)
        if self.slow && viz::find(self.mode).is_some_and(|f| f.calm) {
            fps = fps.min(CALM_FPS / 2.0);
        }
        fps
    }

    /// On a slow renderer, while it plays: what a frame has cost the
    /// process lately (CPU seconds), and the frame rate that keeps the
    /// window within its budget.
    fn govern(&mut self) {
        self.frames_drawn += 1;
        if !self.slow || !self.playing() {
            self.cost_from = None;
            return;
        }
        let Some(cpu) = viz::gpu::process_cpu() else {
            return;
        };
        match self.cost_from {
            None => self.cost_from = Some((Instant::now(), cpu, self.frames_drawn)),
            Some((when, before, frames)) if when.elapsed() > Duration::from_secs(2) => {
                let drawn = self.frames_drawn - frames;
                if drawn > 4 {
                    let per_frame = (cpu - before) / drawn as f64;
                    let fits = (SLOW_BUDGET_CORES / per_frame.max(1e-4)) as f32;
                    // (towards it, not at once: a busy moment is no trend)
                    let target = fits.clamp(MIN_SLOW_FPS, SLOW_FPS);
                    self.budget_fps += (target - self.budget_fps) * 0.6;
                }
                self.cost_from = Some((Instant::now(), cpu, self.frames_drawn));
            }
            Some(_) => {}
        }
    }

    /// Paused in a track: the picture holds still (stopped, it falls to rest).
    fn frozen(&self) -> bool {
        !self.playing()
            && self.loaded == Some(self.generation)
            && self.engine.shared.position() > 0.05
    }

    /// What the meters say (tests; the loudness of what was heard).
    pub fn meter_readings(&self) -> crate::meters::Readings {
        self.meters.read()
    }

    /// Frames the analysis ran (tests: none with the picture off).
    pub fn analyzer_frames(&self) -> u64 {
        self.analyzer_frames
    }

    /// The window's title (tests).
    pub fn title(&self) -> &str {
        &self.window_title
    }

    /// Why the track at `path` could not be played, if it could not.
    pub fn broken(&self, path: &str) -> Option<&str> {
        self.broken.get(path).map(String::as_str)
    }

    /// The current track's waveform is being read (a local file of known
    /// length), on a thread of its own; another track's reading stops.
    fn read_overview(&mut self) {
        let Some(path) = self
            .playlist
            .current
            .and_then(|i| self.playlist.tracks.get(i))
            .map(|t| t.path.clone())
            .filter(|p| !p.contains("://") && self.duration.is_some())
        else {
            if let Some((_, _, stop)) = self.overview.take() {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return;
        };
        if self.overview.as_ref().is_some_and(|(p, _, _)| *p == path) {
            return;
        }
        if let Some((_, _, stop)) = self.overview.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let shared = Arc::new(Mutex::new(viz::Overview::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (into, flag, file) = (shared.clone(), stop.clone(), path.clone());
        let _ = std::thread::Builder::new()
            .name("overview".into())
            .spawn(move || {
                let _ = crate::engine::waveform(&file, 1600, &flag, |columns, done| {
                    if let Ok(mut o) = into.lock() {
                        o.columns.clear();
                        o.columns.extend_from_slice(columns);
                        o.done = done;
                    }
                });
            });
        self.overview = Some((path, shared, stop));
    }

    /// The face drawn into `rect` (the window's picture, or the display
    /// view's): the analysis of this frame's sound, then the face.
    fn draw_face(&mut self, ui: &Ui, rect: egui::Rect, hover: Option<egui::Pos2>) {
        let scheme = self.scheme();
        self.viz_state.waterfall_hd = self.amp.waterfall_hd;
        self.viz_state.classic_plus = self.amp.classic_plus;
        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.0, 0.2);
        self.last_frame = Instant::now();
        let written = self.engine.shared.tap_into(&mut self.tap);
        let rate = self
            .engine
            .shared
            .device_rate
            .load(std::sync::atomic::Ordering::Relaxed);
        // (off: no analysis, no frames for it; paused: the picture stands
        // as it was, not emptied)
        let frozen = self.frozen();
        self.analyzer.motion = if self.desktop_reduces_motion() {
            crate::analyzer::guard::Motion::Reduced
        } else if self.amp.motion == "intense" {
            crate::analyzer::guard::Motion::Intense
        } else {
            crate::analyzer::guard::Motion::Calm
        };
        self.analyzer.calm = viz::scheme_calm(&scheme);
        // (the rack's needles follow the sound with the picture off too)
        let needles = crate::design::current().extras.needles;
        if (self.mode != "off" || needles) && !frozen {
            self.analyzer.set_readings(self.meters.read());
            self.analyzer.update(
                &self.tap,
                written,
                rate,
                &self.options,
                dt,
                self.playing(),
                viz::needs_now(self.mode, &self.viz_state),
            );
            self.analyzer_frames += 1;
        }
        if self.mode == "overview" {
            self.read_overview();
        }
        let overview = self
            .overview
            .as_ref()
            .filter(|_| self.mode == "overview")
            .map(|(_, o, _)| o.clone());
        let overview = overview.as_ref().and_then(|o| o.lock().ok());
        let title = self
            .window_title
            .strip_prefix("axross-amp – ")
            .unwrap_or("");
        let frame = viz::Frame {
            painter: ui.painter(),
            rect,
            a: &self.analyzer,
            s: &scheme,
            dt: if frozen { 0.0 } else { dt },
            title,
            overview: overview.as_deref(),
            position: self.engine.shared.position() as f32,
            duration: self.duration.unwrap_or(0.0) as f32,
            hover,
            opacity: 1.0,
            second: false,
            loudness_target: self.amp.loudness_target,
            cover: if self.mode == "cover" {
                let px = viz::cover_side(rect.shrink(4.0)) * ui.ctx().pixels_per_point();
                self.sizer
                    .get(px)
                    .or(self.cover.as_ref())
                    .map(|t| (t.id(), t.size_vec2()))
            } else {
                None
            },
        };
        self.viz_state.gpu.interval = 1.0 / self.fps_cap();
        viz::draw(&frame, self.mode, &mut self.viz_state);
        drop(overview);
    }

    fn visual(&mut self, ui: &mut Ui, height: f32) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        // (a screen reader names the picture by its face)
        let name = viz::name(self.mode);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::accesskit::Role::Image,
                true,
                format!("Visualization: {name}"),
            )
        });
        // (shown on a display of its own: here what is shown where, and a
        // way to bring it back - the face drawn once, there)
        if let Some(e) = &self.external {
            let label = e.monitor.label();
            let said = format!("Showing on {label}");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, said.clone())
            });
            ui.painter().rect_filled(rect, 8.0, th().surface);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Showing on {label}\nD or Escape there brings it back"),
                egui::FontId::proportional(15.0),
                th().muted,
            );
            return;
        }
        let scheme = self.scheme();
        self.draw_face(ui, rect, response.hover_pos());
        crate::design::bezel(ui.painter(), rect, crate::design::current(), th());
        // the face's name, for a moment after it changed (axross's 1.5 s)
        let now = ui.input(|i| i.time);
        if self.label_for != self.mode {
            self.label_for = self.mode;
            self.label_until = now + 1.5;
        }
        if now < self.label_until {
            ui.painter().text(
                rect.center_top() + vec2(0.0, 18.0),
                egui::Align2::CENTER_CENTER,
                viz::name(self.mode),
                egui::FontId::proportional(15.0),
                scheme.peak.gamma_multiply(0.8),
            );
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(self.label_until - now));
        }
        // full screen: the pointer hides when it rests over the picture
        if self.fullscreen && self.options.hide_cursor && response.hovered() {
            let still = ui.input(|i| i.pointer.time_since_last_movement());
            if still > 2.5 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            } else {
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f32(2.5 - still + 0.05));
            }
        }
        let wait = ui.ctx().options(|o| o.input_options.max_double_click_delay);
        if response.double_clicked() {
            self.pending_click = None;
            if self.fullscreen {
                self.set_fullscreen(ui.ctx(), false);
            } else {
                self.big_visual = !self.big_visual;
            }
        } else if response.clicked()
            && self.mode == "overview"
            && let Some(share) = response
                .interact_pointer_pos()
                .and_then(|p| viz::analysis::overview_seek(rect, p))
        {
            // the overview: a click goes there in the track (V: the next face)
            if let Some(total) = self.duration {
                self.seek(share as f64 * total);
            }
        } else if response.clicked() {
            self.pending_click = Some(now);
        }
        if let Some(at) = self.pending_click {
            if now - at > wait {
                self.pending_click = None;
                self.mode = viz::next(self.mode);
                self.dirty = true;
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(wait));
            }
        }
        let response = response.on_hover_text(if self.big_visual {
            format!(
                "{} - click: next face, double-click or Esc: normal size, right click: options",
                viz::name(self.mode)
            )
        } else {
            format!(
                "{} - click: next face, double-click (or F11): larger, right click: options",
                viz::name(self.mode)
            )
        });
        response.context_menu(|ui| self.visual_menu(ui));
    }

    fn position(&mut self, ui: &mut Ui) {
        let shown = self.shown_position();
        let pos = self.seek_drag.unwrap_or(shown);
        let seekable = self.info.seekable && self.duration.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new(clock(pos)).monospace().color(th().muted));
            let total = self.duration.unwrap_or(0.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(if total > 0.0 {
                        clock(total)
                    } else if self.current_is_stream() {
                        "live".into()
                    } else {
                        "--:--".into()
                    })
                    .monospace()
                    .color(th().muted),
                );
                ui.spacing_mut().slider_width = ui.available_width() - 8.0;
                let mut value = pos.min(total.max(0.0));
                let slider = egui::Slider::new(&mut value, 0.0..=total.max(0.001))
                    .show_value(false)
                    .trailing_fill(true);
                let response = ui
                    .add_enabled(seekable, slider)
                    .accessible_name("Position")
                    .on_hover_text("Seek: drag, click, the wheel, ←/→ (Shift: 30 s), J/L (15 s)");
                let notches = self.wheel_steps(ui, &response);
                if notches != 0 {
                    self.seek(self.engine.shared.position() + 5.0 * notches as f64);
                }
                if response.dragged() || response.changed() {
                    self.seek_drag = Some(value);
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    self.seek(value);
                    self.seek_drag = None;
                }
            });
        });
    }

    fn transport(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // (a narrow window: smaller buttons, room for the volume)
            let narrow = ui.available_width() < 520.0;
            // (narrow: closer together, less room round their words - the
            // two 15 s buttons fit with the rest)
            if narrow {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.spacing_mut().button_padding.x = 5.0;
            }
            // (a cassette deck: square keys, taller, side by side)
            let keys = crate::design::current().extras.keys;
            if keys {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.visuals_mut().widgets.inactive.corner_radius = CornerRadius::same(2);
                ui.visuals_mut().widgets.hovered.corner_radius = CornerRadius::same(2);
                ui.visuals_mut().widgets.active.corner_radius = CornerRadius::same(2);
            }
            let big = match (narrow, keys) {
                (true, _) => vec2(36.0, 32.0),
                (false, true) => vec2(48.0, 42.0),
                (false, false) => vec2(46.0, 34.0),
            };
            if ui
                .add(egui::Button::new(RichText::new("⏮").size(18.0)).min_size(big))
                .accessible_name("Previous")
                .on_hover_text("Previous (P)")
                .clicked()
            {
                self.previous();
            }
            let can = self.can_seek();
            let wind = egui::vec2(if narrow { 30.0 } else { 44.0 }, big.y);
            let (back, on) = if narrow {
                ("−15", "+15")
            } else {
                ("−15 s", "+15 s")
            };
            if ui
                .add_enabled(
                    can,
                    egui::Button::new(RichText::new(back).size(if narrow { 11.0 } else { 12.5 }))
                        .min_size(wind),
                )
                .accessible_name("Back 15 s")
                .on_hover_text("Back 15 seconds (J)")
                .on_disabled_hover_text("Nothing to wind back (no track, or a stream)")
                .clicked()
            {
                self.wind(-15.0);
            }
            let play = if self.playing() { "⏸" } else { "▶" };
            let button = if keys {
                // (the play key pressed in while it plays, the accent a
                // band along its top edge as a deck's coloured key)
                egui::Button::new(RichText::new(play).size(20.0))
                    .selected(self.playing())
                    .min_size(if narrow {
                        vec2(48.0, 36.0)
                    } else {
                        vec2(58.0, 42.0)
                    })
            } else {
                egui::Button::new(RichText::new(play).size(22.0).color(ink_on(self.accent)))
                    .fill(self.accent)
                    .min_size(if narrow {
                        vec2(48.0, 36.0)
                    } else {
                        vec2(58.0, 38.0)
                    })
                    .corner_radius(CornerRadius::same(19))
            };
            let response = ui.add(button);
            if keys {
                let r = response.rect;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(r.width() - 6.0, 3.0)),
                    CornerRadius::same(1),
                    self.accent,
                );
            }
            if response
                .accessible_name(if self.playing() { "Pause" } else { "Play" })
                .on_hover_text("Play / pause (Space)")
                .clicked()
            {
                self.toggle_play();
            }
            if ui
                .add(egui::Button::new(RichText::new("⏹").size(18.0)).min_size(big))
                .accessible_name("Stop")
                .on_hover_text("Stop")
                .clicked()
            {
                self.stop();
            }
            if ui
                .add_enabled(
                    can,
                    egui::Button::new(RichText::new(on).size(if narrow { 11.0 } else { 12.5 }))
                        .min_size(wind),
                )
                .accessible_name("Forward 15 s")
                .on_hover_text("Forward 15 seconds (L)")
                .on_disabled_hover_text("Nothing to wind on (no track, or a stream)")
                .clicked()
            {
                self.wind(15.0);
            }
            if ui
                .add(egui::Button::new(RichText::new("⏭").size(18.0)).min_size(big))
                .accessible_name("Next")
                .on_hover_text("Next (N)")
                .clicked()
            {
                self.next(false);
            }
            ui.add_space(if narrow { 2.0 } else { 6.0 });
            let shuffle = self.playlist.shuffle;
            if ui
                .add(
                    egui::Button::new(RichText::new("🔀").size(16.0).color(if shuffle {
                        self.accent
                    } else {
                        th().muted
                    }))
                    .selected(shuffle),
                )
                // (the state in words: not by its colour only)
                .accessible_name(if shuffle {
                    "Shuffle: on"
                } else {
                    "Shuffle: off"
                })
                .on_hover_text(if shuffle {
                    "Shuffle: on"
                } else {
                    "Shuffle: off"
                })
                .clicked()
            {
                self.playlist.set_shuffle(!shuffle);
                self.dirty = true;
            }
            let (icon, tip) = match self.playlist.repeat {
                Repeat::Off => ("🔁", "Repeat: off"),
                Repeat::All => ("🔁", "Repeat: all"),
                Repeat::One => ("🔂", "Repeat: this track"),
            };
            let on = self.playlist.repeat != Repeat::Off;
            if ui
                .add(
                    egui::Button::new(RichText::new(icon).size(16.0).color(if on {
                        self.accent
                    } else {
                        th().muted
                    }))
                    .selected(on),
                )
                .accessible_name(tip)
                .on_hover_text(tip)
                .clicked()
            {
                self.playlist.repeat = self.playlist.repeat.next();
                self.dirty = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut volume = self.volume;
                // (narrow: the slider gives way, never over the buttons)
                ui.spacing_mut().slider_width = (ui.available_width() - 84.0).clamp(36.0, 110.0);
                let response = if crate::design::current().extras.knob {
                    crate::design::knob(ui, &mut volume, "Volume")
                } else {
                    ui.add(
                        egui::Slider::new(&mut volume, 0..=100)
                            .show_value(false)
                            .trailing_fill(true),
                    )
                }
                .accessible_name("Volume")
                .on_hover_text(format!("Volume {volume} % (↑/↓, +/-, the wheel)"));
                if response.changed() {
                    self.set_volume(volume);
                }
                let notches = self.wheel_steps(ui, &response);
                if notches != 0 {
                    self.set_volume((self.volume as i32 + 5 * notches).clamp(0, 100) as u32);
                }
                let icon = if self.muted || self.volume == 0 {
                    "🔇"
                } else {
                    "🔊"
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(icon).size(16.0))
                            .frame(false)
                            .min_size(vec2(24.0, 24.0)),
                    )
                    .accessible_name("Mute")
                    .on_hover_text("Mute (M)")
                    .clicked()
                {
                    self.set_muted(!self.muted);
                }
                // (at least 24 x 24 to hit, as every button)
                egui::containers::menu::MenuButton::from_button(
                    egui::Button::new(RichText::new("⋯").size(16.0)).min_size(vec2(24.0, 24.0)),
                )
                .ui(ui, |ui| self.more_menu(ui))
                .0
                .accessible_name("More")
                .on_hover_text("Sleep timer, ReplayGain, track details, mixer");
                if let Some(label) = self.sleep_label() {
                    ui.label(RichText::new(label).small().color(self.accent));
                }
            });
        });
    }

    /// The lower part's switch: the playlist or the equalizer (and what
    /// the playlist's page offers beside it).
    fn lower_switch(&mut self, ui: &mut Ui) {
        let total: u64 = self.playlist.tracks.iter().map(|t| t.duration_ms).sum();
        ui.horizontal(|ui| {
            let n = self.playlist.tracks.len();
            let list = format!("Playlist  {} · {}", n, length(total as f64 / 1000.0));
            if ui
                .selectable_label(self.playlist_open && !self.eq_page, list)
                .on_hover_text("The playlist")
                .clicked()
            {
                self.eq_page = false;
                if !self.playlist_open {
                    self.show_playlist(ui.ctx(), true);
                }
            }
            let eq = if self.eq.enabled {
                "Equalizer"
            } else {
                "Equalizer (off)"
            };
            if ui
                .selectable_label(self.eq_page, eq)
                .on_hover_text("The ten-band equalizer")
                .clicked()
            {
                self.eq_page = !self.eq_page;
            }
            if !self.eq_page && self.playlist_open {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.playlist_actions(ui);
                });
            }
        });
    }

    /// The equalizer: on or off, a preset, ten bands on a dB scale with
    /// the 0 dB line across them and each band's value.
    fn equalizer_page(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let mut enabled = self.eq.enabled;
            if ui
                .checkbox(&mut enabled, "On")
                .on_hover_text("The equalizer on or off")
                .changed()
            {
                self.eq.enabled = enabled;
                self.set_eq(self.eq.gains, Some(&self.eq.preset.clone()));
            }
            let mut preset = self.eq.preset.clone();
            egui::ComboBox::from_id_salt("preset")
                .selected_text(&preset)
                .show_ui(ui, |ui| {
                    for (name, _) in eq::PRESETS {
                        ui.selectable_value(&mut preset, name.to_owned(), name);
                    }
                })
                .response
                .accessible_name("Equalizer preset");
            if preset != self.eq.preset
                && let Some(gains) = eq::preset(&preset)
            {
                self.set_eq(gains, Some(&preset));
            }
        });
        ui.add_space(4.0);
        let height = (ui.available_height() - 40.0).clamp(90.0, 220.0);
        ui.add_enabled_ui(self.eq.enabled, |ui| {
            ui.horizontal_top(|ui| {
                // (a place for the bands' fill under the sliders, filled
                // once their places are known)
                let fill_slot = ui.painter().add(egui::Shape::Noop);
                // the scale: +12 at the top, 0, -12 at the bottom
                let (scale, _) = ui.allocate_exact_size(vec2(34.0, height), Sense::hover());
                let mut gains = self.eq.gains;
                let mut changed = false;
                let width =
                    ((ui.available_width() - 9.0 * ui.spacing().item_spacing.x) / 10.0).max(26.0);
                let mut rails: Option<egui::Rect> = None;
                let mut bands = [egui::Rect::NOTHING; 10];
                for (i, band) in eq::BANDS.iter().enumerate() {
                    ui.allocate_ui(vec2(width, height + 30.0), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.spacing_mut().slider_width = height;
                            // (egui's rail not drawn: ours is, with the fill
                            // from 0 dB, under the handle)
                            ui.spacing_mut().slider_rail_height = 0.0;
                            let response = ui
                                .add(
                                    egui::Slider::new(
                                        &mut gains[i],
                                        -eq::MAX_GAIN_DB..=eq::MAX_GAIN_DB,
                                    )
                                    .vertical()
                                    .show_value(false)
                                    .trailing_fill(false),
                                )
                                .accessible_name(format!("Equalizer {}", band_name(*band)))
                                .on_hover_text(format!("{} {:+.1} dB", band_name(*band), gains[i]));
                            changed |= response.changed();
                            bands[i] = response.rect;
                            rails = Some(match rails {
                                Some(r) => r.union(response.rect),
                                None => response.rect,
                            });
                            ui.label(RichText::new(band_name(*band)).small().color(th().muted));
                            ui.label(
                                RichText::new(format!("{:+.0}", gains[i]))
                                    .small()
                                    .monospace()
                                    .color(if gains[i].abs() < 0.05 {
                                        th().muted
                                    } else {
                                        th().text
                                    }),
                            );
                        });
                    });
                }
                if let Some(rails) = rails {
                    // (the handle's travel: the rail less half a handle each end)
                    let inset = ui.spacing().interact_size.y / 2.0;
                    let top = rails.top() + inset;
                    let bottom = rails.bottom() - inset;
                    let y_of = |db: f32| {
                        top + (eq::MAX_GAIN_DB - db) / (2.0 * eq::MAX_GAIN_DB) * (bottom - top)
                    };
                    let painter = ui.painter();
                    let zero = y_of(0.0);
                    // each band filled from the 0 dB line to its value (a
                    // flat equalizer shows nothing raised or cut), under the
                    // rails and handles
                    let mut fills = Vec::new();
                    let rail = ui.visuals().widgets.inactive.bg_fill;
                    for (rect, gain) in bands.iter().zip(gains) {
                        let x = rect.center().x;
                        fills.push(egui::Shape::rect_filled(
                            egui::Rect::from_x_y_ranges(x - 3.0..=x + 3.0, top..=bottom),
                            3.0,
                            rail,
                        ));
                        if gain.abs() >= 0.05 {
                            let y = y_of(gain);
                            fills.push(egui::Shape::rect_filled(
                                egui::Rect::from_x_y_ranges(
                                    x - 3.0..=x + 3.0,
                                    zero.min(y)..=zero.max(y),
                                ),
                                3.0,
                                self.accent,
                            ));
                        }
                    }
                    painter.set(fill_slot, egui::Shape::Vec(fills));
                    painter.line_segment(
                        [
                            egui::pos2(rails.left() - 4.0, zero),
                            egui::pos2(rails.right() + 4.0, zero),
                        ],
                        Stroke::new(1.0, th().muted.gamma_multiply(0.6)),
                    );
                    let step = eq::MAX_GAIN_DB / 2.0;
                    for k in -2..=2 {
                        let db = k as f32 * step;
                        painter.text(
                            egui::pos2(scale.right() - 4.0, y_of(db)),
                            egui::Align2::RIGHT_CENTER,
                            if db == 0.0 {
                                "0 dB".to_owned()
                            } else {
                                format!("{db:+.0}")
                            },
                            egui::FontId::proportional(11.0),
                            th().muted,
                        );
                    }
                }
                if changed {
                    self.set_eq(gains, None);
                }
            });
        });
    }

    /// Save, Undo, Clear: beside the playlist's switch.
    fn playlist_actions(&mut self, ui: &mut Ui) {
        if ui.button("Clear").clicked() && !self.playlist.tracks.is_empty() {
            self.remember_for_undo();
            let n = self.playlist.tracks.len();
            self.playlist.clear();
            self.select(None);
            self.dirty = true;
            self.status = format!("{n} tracks removed - Ctrl+Z brings them back.");
        }
        if !self.undo.is_empty()
            && ui
                .button("↶ Undo")
                .on_hover_text("Bring back what was removed (Ctrl+Z)")
                .clicked()
        {
            self.undo();
        }
        if ui
            .button("Save…")
            .on_hover_text("Save as an M3U playlist")
            .clicked()
        {
            self.picking = Picking::SavePlaylist;
            self.dialog.save_file();
        }
    }

    fn playlist_view(&mut self, ui: &mut Ui) {
        // (the playing row's cover, made at its size)
        let px = 30.0 * ui.ctx().pixels_per_point();
        let thumb_texture = self.sizer.get(px).or(self.cover.as_ref()).cloned();
        {
            let row = 40.0;
            ui.horizontal(|ui| {
                let edit = egui::TextEdit::singleline(&mut self.filter)
                    .id_salt("playlist-filter")
                    .hint_text("🔍 Filter (Ctrl+F)")
                    .desired_width((ui.available_width() - 90.0).max(120.0));
                let response = ui.add(edit).accessible_name("Filter the playlist");
                if std::mem::take(&mut self.focus_filter) {
                    response.request_focus();
                }
                // (the field lets go of the focus on Escape itself)
                if (response.has_focus() || response.lost_focus())
                    && ui.input(|i| i.key_pressed(Key::Escape))
                {
                    self.filter.clear();
                    response.surrender_focus();
                }
                ui.menu_button("Sort ▾", |ui| {
                    for (key, name) in playlist::SortKey::ALL {
                        if ui.button(name).clicked() {
                            self.playlist.sort(key);
                            self.select(self.playlist.current);
                            self.scroll_to = self.playlist.current;
                            self.dirty = true;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Sort the playlist");
            });
            let rows = self.visible_rows().to_vec();
            let filtered = !self.filter.trim().is_empty();
            if filtered {
                self.dragging = None; // (no reordering in a filtered view)
                ui.label(
                    RichText::new(format!(
                        "{} of {} shown",
                        rows.len(),
                        self.playlist.tracks.len()
                    ))
                    .small()
                    .color(th().muted),
                );
            }
            // (unfiltered, an album's tracks under a head of their own)
            let entries = if filtered {
                rows.iter().map(|&i| Entry::Track(i)).collect()
            } else {
                with_albums(&self.playlist.tracks, &rows)
            };
            for (key, picture) in self.album_loader.rx.try_iter() {
                let texture = picture.map(|p| {
                    ui.ctx()
                        .load_texture(format!("album {key}"), p, egui::TextureOptions::LINEAR)
                });
                self.album_covers.insert(key, texture);
            }
            let count = entries.len();
            let mut play: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let mut details: Option<usize> = None;
            let mut play_next: Option<usize> = None;
            let mut moved: Option<(usize, usize)> = None;
            let mut clicked: Option<(usize, egui::Modifiers)> = None;
            let height = ui.available_height().max(120.0);
            let mut area = egui::ScrollArea::vertical()
                .id_salt("playlist-rows")
                .auto_shrink([false, false])
                .max_height(height);
            if let Some(target) = self
                .scroll_to
                .take()
                .and_then(|t| entries.iter().position(|e| *e == Entry::Track(t)))
            {
                // (the row chosen by a key comes into view)
                let id = ui.make_persistent_id("playlist-rows");
                let offset = egui::scroll_area::State::load(ui.ctx(), id)
                    .map(|s| s.clamped_offset().y)
                    .unwrap_or(0.0);
                let step = row + ui.spacing().item_spacing.y;
                let top = target as f32 * step;
                let wanted = if top < offset {
                    top
                } else if top + step > offset + height {
                    top + step - height
                } else {
                    offset
                };
                area = area.vertical_scroll_offset(wanted.max(0.0));
            }
            area.show_rows(ui, row, count, |ui, range| {
                for r in range {
                    let i = match &entries[r] {
                        Entry::Album(head) => {
                            if !self.album_covers.contains_key(&head.key) {
                                self.album_covers.insert(head.key.clone(), None);
                                let path = self.playlist.tracks[head.first].path.clone();
                                if !playlist::is_stream(&path) {
                                    let _ = self.album_loader.tx.send((head.key.clone(), path));
                                }
                            }
                            let cover = self.album_covers.get(&head.key).cloned().flatten();
                            album_head(ui, head, row, cover.as_ref(), self.accent);
                            continue;
                        }
                        Entry::Track(i) => *i,
                    };
                    let track = &self.playlist.tracks[i];
                    let current = self.playlist.current == Some(i);
                    let title = if track.title.is_empty() {
                        playlist::default_title(&track.path)
                    } else {
                        track.title.clone()
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(ui.available_width(), row),
                        Sense::click_and_drag(),
                    );
                    let selected = self.selected == Some(i) || self.chosen.contains(&i);
                    // (a screen reader reads the row: its title, whether it is chosen)
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::accesskit::Role::ListItem,
                            true,
                            selected,
                            &title,
                        )
                    });
                    if current || selected || response.hovered() {
                        let fill = if current {
                            crate::theme::playing_row(th(), self.accent)
                        } else if selected {
                            th().selection
                        } else {
                            th().hover
                        };
                        ui.painter().rect_filled(rect, 4.0, fill);
                    }
                    let broken = self.broken.get(&track.path);
                    let colour = if broken.is_some() {
                        th().bad
                    } else if current {
                        th().strong
                    } else {
                        th().text
                    };
                    let painter = ui.painter();
                    // the left: the cover and a playing mark for the track
                    // that plays, else its number
                    let thumb = egui::Rect::from_min_size(
                        rect.left_top() + vec2(6.0, 5.0),
                        vec2(30.0, 30.0),
                    );
                    if current {
                        if let Some(texture) = &thumb_texture {
                            painter.image(
                                texture.id(),
                                thumb,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        } else {
                            painter.rect_filled(thumb, 4.0, self.accent.gamma_multiply(0.5));
                        }
                        // three bars that move with the sound (still: paused)
                        let playing = self.playing();
                        for k in 0..3 {
                            let level = if playing {
                                let n = self.analyzer.bars.len().max(1);
                                let b = self.analyzer.bars[(k * n / 4 + n / 8).min(n - 1)];
                                0.25 + 0.75 * b
                            } else {
                                0.3
                            };
                            let x = thumb.left() + 7.0 + k as f32 * 6.0;
                            let h = 18.0 * level;
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(x, thumb.bottom() - 6.0 - h),
                                    egui::pos2(x + 4.0, thumb.bottom() - 6.0),
                                ),
                                1.0,
                                ink_on(self.accent),
                            );
                        }
                    } else {
                        painter.text(
                            thumb.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{}", i + 1),
                            egui::FontId::monospace(12.0),
                            th().muted,
                        );
                    }
                    if let Some(n) = self.playlist.up_next_place(i) {
                        // (to play next: its place in that queue)
                        painter.text(
                            rect.right_bottom() + vec2(-8.0, -9.0),
                            egui::Align2::RIGHT_CENTER,
                            format!("next {n}"),
                            egui::FontId::proportional(11.0),
                            self.accent,
                        );
                    }
                    let duration = if track.duration_ms > 0 {
                        length(track.duration_ms as f64 / 1000.0)
                    } else if playlist::is_stream(&track.path) {
                        "📻".into()
                    } else {
                        String::new()
                    };
                    // the title, and under it who and from which album (one
                    // line: in the middle of the row, as its number)
                    let (line, by) = row_text(track, &title);
                    let first_y = if by.is_empty() {
                        rect.center().y - rect.top()
                    } else {
                        13.0
                    };
                    painter.text(
                        egui::pos2(rect.right() - 8.0, rect.top() + first_y),
                        egui::Align2::RIGHT_CENTER,
                        duration,
                        egui::FontId::monospace(12.0),
                        th().muted,
                    );
                    let clip = egui::Rect::from_min_max(
                        rect.left_top() + vec2(46.0, 0.0),
                        rect.right_bottom() - vec2(64.0, 0.0),
                    );
                    let clipped = painter.with_clip_rect(clip);
                    clipped.text(
                        clip.left_top() + vec2(0.0, first_y),
                        egui::Align2::LEFT_CENTER,
                        line,
                        egui::FontId::proportional(14.0),
                        colour,
                    );
                    if !by.is_empty() {
                        clipped.text(
                            clip.left_top() + vec2(0.0, 29.0),
                            egui::Align2::LEFT_CENTER,
                            by,
                            egui::FontId::proportional(11.5),
                            th().muted,
                        );
                    }
                    // the path, and why it could not be played (axross's tooltip)
                    let tip = match broken {
                        Some(why) => format!("{}\nCannot play: {why}", track.path),
                        None => track.path.clone(),
                    };
                    let response = response.on_hover_text(tip);
                    if response.clicked() {
                        let modifiers = ui.input(|inp| inp.modifiers);
                        clicked = Some((i, modifiers));
                    }
                    if response.double_clicked() {
                        play = Some(i);
                    }
                    if response.drag_started() {
                        self.dragging = Some(i);
                    }
                    if let (Some(from), true) = (self.dragging, response.hovered())
                        && ui.input(|inp| inp.pointer.any_released())
                        && from != i
                    {
                        moved = Some((from, i));
                    }
                    response.context_menu(|ui| {
                        if ui.button("▶ Play").clicked() {
                            play = Some(i);
                            ui.close();
                        }
                        if ui.button("⏭ Play next").clicked() {
                            play_next = Some(i);
                            ui.close();
                        }
                        if ui.button("✖ Remove").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                        if ui.button("ℹ Details…").clicked() {
                            details = Some(i);
                            ui.close();
                        }
                    });
                }
            });
            if ui.input(|inp| inp.pointer.any_released()) {
                self.dragging = None;
            }
            if let Some((i, modifiers)) = clicked {
                self.click_row(i, modifiers, &rows);
            }
            if let Some((from, to)) = moved {
                // the rows chosen go together (the one dragged among them)
                let picked = self.chosen_rows();
                if picked.len() > 1 && picked.contains(&from) {
                    let placed = self.playlist.move_tracks(&picked, to);
                    self.selected = placed.first().copied();
                    self.chosen = placed;
                } else {
                    self.playlist.move_track(from, to);
                    self.select(Some(to));
                }
                self.dirty = true;
            }
            if let Some(i) = remove {
                let picked = self.chosen_rows();
                let gone = if picked.contains(&i) { picked } else { vec![i] };
                self.remove_rows(&gone, false);
            }
            if let Some(i) = play {
                self.play_index(i, 0, true);
            }
            if let Some(i) = details {
                self.show_details(i);
            }
            if let Some(i) = play_next {
                let picked = self.chosen_rows();
                let rows = if picked.contains(&i) { picked } else { vec![i] };
                self.playlist.play_next(&rows);
                self.status = format!("{} to play next", rows.len());
            }
            if self.gate.any && ui.input(|inp| inp.key_pressed(Key::Delete)) {
                let picked = self.chosen_rows();
                if !picked.is_empty() {
                    self.remove_rows(&picked, true);
                }
            }
            if self.gate.activation
                && ui.input(|inp| inp.key_pressed(Key::Enter))
                && let Some(i) = self.selected
            {
                self.play_index(i, 0, true);
            }
        }
    }

    /// The Player tab's page: what plays, the picture, the controls, the
    /// playlist or the equalizer.
    fn player_page(&mut self, ui: &mut Ui) {
        self.no_sound_banner(ui);
        if self.big_visual {
            // the picture large, what plays above it - and in sight,
            // the way back and the playlist (under the picture when
            // asked for)
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button("Normal size")
                        .on_hover_text("Esc, or a double-click on the picture")
                        .clicked()
                    {
                        if self.fullscreen {
                            self.set_fullscreen(ui.ctx(), false);
                        } else {
                            self.big_visual = false;
                        }
                    }
                    let list = if self.big_list {
                        "Hide the playlist"
                    } else {
                        "Show the playlist"
                    };
                    if ui.button(list).clicked() {
                        self.big_list = !self.big_list;
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        self.title_line(ui)
                    });
                });
            });
            let all = ui.available_height();
            let list = if self.big_list {
                (all * 0.38).max(LEAST_LIST)
            } else {
                0.0
            };
            self.visual(ui, (all - 92.0 - list).max(120.0));
            ui.add_space(6.0);
            self.controls(ui);
            if self.big_list {
                ui.add_space(4.0);
                self.playlist_panel(ui);
            }
            return;
        }
        if self.playlist.tracks.is_empty() && self.adding.is_empty() {
            self.empty_state(ui);
            return;
        }
        // (a low window, or the cover as the picture: what plays
        // in one line)
        if ui.available_height() < 560.0 || self.mode == "cover" {
            self.title_line(ui);
        } else {
            self.now_playing(ui);
            ui.add_space(8.0);
        }
        // the picture large: what the controls and the playlist's
        // rows leave (a short list leaves more), at least 45 % of it
        // (what lies between the picture and the list, as tall as
        // it was last laid out - not a guess)
        let lower = self.playlist_open || self.eq_page;
        let room = ui.available_height() - self.controls_height;
        let picture = if lower {
            let list = if self.eq_page {
                240.0
            } else {
                (self.visible_rows().len() as f32 * 44.0 + 76.0).max(130.0)
            };
            // the picture gives way first: the list keeps at least
            // three rows whatever the window
            // (a low window: the picture down to a strip before the list
            // loses its rows - in a roomier design too)
            let least = (room - LEAST_LIST).clamp(24.0, 80.0);
            (room - list)
                .max(room * 0.45)
                .min(room - LEAST_LIST)
                .clamp(least, 900.0)
        } else {
            (room - 36.0).max(80.0)
        };
        self.visual(ui, picture);
        let top = ui.cursor().top();
        ui.add_space(6.0);
        self.controls(ui);
        ui.add_space(6.0);
        self.lower_switch(ui);
        self.controls_height = ui.cursor().top() - top;
        if self.eq_page {
            self.equalizer_page(ui);
        } else if self.playlist_open {
            self.playlist_panel(ui);
        }
    }

    /// The position and the row of buttons. Zen keeps them quiet - no
    /// fills, no frames, the words and symbols at their full contrast -
    /// until the pointer comes to them or the keys move the focus.
    fn controls(&mut self, ui: &mut Ui) {
        let quiet = crate::design::current().fade_controls
            && !ui.rect_contains_pointer(self.controls_rect.expand(12.0))
            && ui.ctx().memory(|m| m.focused()).is_none();
        let shown = ui.scope(|ui| {
            if quiet {
                let w = &mut ui.visuals_mut().widgets;
                for s in [&mut w.inactive, &mut w.noninteractive] {
                    s.bg_fill = Color32::TRANSPARENT;
                    s.weak_bg_fill = Color32::TRANSPARENT;
                    s.bg_stroke = Stroke::NONE;
                }
            }
            self.position(ui);
            self.transport(ui);
        });
        self.controls_rect = shown.response.rect;
    }

    /// The glass design: the cover's colours as a haze over the whole
    /// page, under the frosted panels (made once a cover, theme and accent).
    fn paint_haze(&mut self, ui: &mut Ui) {
        if crate::design::current().panel != crate::design::Panel::Glass {
            return;
        }
        let Some(art) = self.cover_art.as_ref() else {
            return;
        };
        let t = th();
        let fresh =
            matches!(&self.haze, Some((key, accent, _)) if *key == t.key && *accent == self.accent);
        if !fresh {
            let image = crate::design::haze(
                &art.rgba,
                art.width as usize,
                art.height as usize,
                t,
                self.accent,
            );
            let texture = ui
                .ctx()
                .load_texture("haze", image, egui::TextureOptions::LINEAR);
            self.haze = Some((t.key, self.accent, texture));
        }
        if let Some((_, _, texture)) = &self.haze {
            // (the texels' centres at the edges: no hard border of the
            // outer cells' colour)
            let uv =
                egui::Rect::from_min_max(egui::pos2(0.0625, 0.0625), egui::pos2(0.9375, 0.9375));
            ui.painter()
                .image(texture.id(), ui.max_rect().expand(10.0), uv, Color32::WHITE);
        }
    }

    /// The playlist on the design's panel (where it lay last frame, under
    /// the rows laid over it; the modern card and zen leave it bare).
    fn playlist_panel(&mut self, ui: &mut Ui) {
        let d = crate::design::current();
        if self.panel_rects[1].is_positive() {
            let fill = crate::design::list_fill(d, th());
            crate::design::panel(ui.painter(), self.panel_rects[1], d, th(), fill, 2);
        }
        let margin = crate::design::list_margin(d);
        let shown = egui::Frame::new().inner_margin(margin).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.playlist_view(ui);
        });
        self.panel_rects[1] = if margin > 0.0 {
            shown.response.rect
        } else {
            egui::Rect::NOTHING
        };
    }

    /// Whether the desktop asks for less motion (GNOME's animations off).
    fn desktop_reduces_motion(&self) -> bool {
        self.reduced_motion
            .lock()
            .ok()
            .and_then(|r| *r)
            .unwrap_or(false)
    }

    /// The photosensitivity warning, before the intense motion is used.
    fn intense_window(&mut self, ctx: &egui::Context) {
        if !self.intense_asked {
            return;
        }
        let mut open = true;
        egui::Window::new("Intense motion")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.label(
                    RichText::new("Photosensitivity warning")
                        .font(bold(16.0))
                        .color(th().warn),
                );
                ui.label(
                    "Intense motion makes the pictures faster and livelier, with more \
                     movement on the beat. A small share of people have seizures when they \
                     see flashing light or patterns; some do not know they are at risk. \
                     The pictures are not guaranteed to stay below three flashes a second, nor \
                     free of flickering stripes, for every kind of music - in either motion. \
                     If you or anyone watching has epilepsy or feels unwell, turn the pictures \
                     off.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep calm").clicked() {
                        self.intense_asked = false;
                    }
                    if ui.button("Use intense motion").clicked() {
                        self.amp.motion = "intense".into();
                        self.amp_dirty = true;
                        self.intense_asked = false;
                    }
                });
            });
        if !open {
            self.intense_asked = false;
        }
    }

    fn url_window(&mut self, ctx: &egui::Context) {
        if !self.url_open {
            return;
        }
        let mut open = true;
        let mut add = false;
        egui::Window::new("Add a stream")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("An internet radio or an audio file on a web server (http / https):");
                let edit = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.url)
                            .hint_text("https://…")
                            .desired_width(360.0),
                    )
                    .accessible_name("Stream URL");
                edit.request_focus();
                let valid = playlist::is_stream(self.url.trim());
                if ui
                    .add_enabled(valid, egui::Button::new("Add and play"))
                    .clicked()
                    || (valid && ui.input(|i| i.key_pressed(Key::Enter)))
                {
                    add = true;
                }
            });
        if add {
            let url = self.url.trim().to_owned();
            let first = self.playlist.tracks.len();
            self.playlist.add_tracks(vec![Track {
                path: url,
                title: String::new(),
                duration_ms: 0,
                ..Default::default()
            }]);
            self.play_index(first, 0, true);
            self.url.clear();
            open = false;
        }
        self.url_open = open;
    }

    fn file_dialog(&mut self, ctx: &egui::Context) {
        self.dialog.update(ctx);
        match self.picking {
            Picking::Files => {
                if let Some(paths) = self.dialog.take_picked_multiple() {
                    let args: Vec<String> =
                        paths.iter().map(|p| crate::fspath::to_string(p)).collect();
                    self.add(&args, Then::Nothing);
                    self.picking = Picking::Nothing;
                }
            }
            Picking::Folder => {
                if let Some(path) = self.dialog.take_picked() {
                    self.add(&[crate::fspath::to_string(&path)], Then::Nothing);
                    self.picking = Picking::Nothing;
                }
            }
            Picking::SavePlaylist => {
                if let Some(mut path) = self.dialog.take_picked() {
                    if path.extension().is_none() {
                        path.set_extension("m3u8"); // (UTF-8, as axross saves it)
                    }
                    let text = playlist::to_m3u(&self.playlist.tracks, path.parent());
                    self.status = match playlist::write_atomically(&path, text.as_bytes()) {
                        Ok(()) => format!("Saved {}", path.display()),
                        Err(e) => format!("Cannot save the playlist: {e}"),
                    };
                    self.picking = Picking::Nothing;
                }
            }
            Picking::Nothing => {}
        }
    }
}

impl eframe::App for App {
    /// Everything that keeps the player going - what the engine said, the
    /// desktop's requests, the sleep timer, the next track queued, the state
    /// saved. eframe calls this also while the window is minimised or
    /// covered (and `ui` is not called at all).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        self.poll_adding();
        self.poll_scanner();
        self.sleep_tick();
        // (the window's inner size; where the platform does not say, the area drawn on)
        let size = ctx.input(|i| {
            i.viewport()
                .inner_rect
                .map(|r| r.size())
                .unwrap_or_else(|| i.viewport_rect().size())
        });
        let size = [size.x.round(), size.y.round()];
        if self.amp.window != Some(size) && size[0] >= 200.0 && size[1] >= 200.0 {
            self.amp.window = Some(size);
            self.amp_dirty = true;
        }
        #[cfg(target_os = "linux")]
        self.desktop(ctx);
        self.sync_queue();
        self.window_title(ctx);
        self.save(false);
        // a heartbeat while something goes on: saves, the timer, the desktop
        if self.playing() || self.sleep.is_some() || !self.adding.is_empty() || self.dirty {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.update_theme(&ui.ctx().clone());
        let ctx = ui.ctx().clone();
        // the cover made at the sizes it is shown: what is done comes in
        self.sizer.poll(&ctx);
        if self.sizer.waiting() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        self.gate = self.key_gate(&ctx);
        self.keys(ui);
        self.follow_displays();
        // dropped files and folders
        let dropped: Vec<String> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| crate::fspath::to_string(f.path()))
                .filter(|p| !p.is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.add(&dropped, Then::PlayIfIdle);
        }
        // (a wide window: the bars above and below as wide as the player's
        // column, over and under it - not across the whole screen)
        let wide = ui.available_width() > 1240.0 && self.tab == Tab::Player && !self.big_visual;
        let column = move |ui: &mut Ui, inner: &mut dyn FnMut(&mut Ui)| {
            if wide {
                let room = ui.available_rect_before_wrap();
                let rect = egui::Rect::from_min_size(
                    egui::pos2(room.center().x - 1176.0 / 2.0, room.top()),
                    vec2(1176.0, room.height()),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| inner(ui));
            } else {
                inner(ui);
            }
        };
        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(th().ground).inner_margin(8.0))
            .show(ui, |ui| column(ui, &mut |ui| self.top_bar(ui)));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(th().ground)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| {
                column(ui, &mut |ui| {
                    ui.horizontal(|ui| {
                        let line = if self.notice.is_empty() {
                            self.status.clone()
                        } else {
                            self.notice.clone()
                        };
                        // a message says its piece for a while, then goes
                        if line != self.status_seen.0 {
                            self.status_seen = (line.clone(), Instant::now());
                        }
                        let age = self.status_seen.1.elapsed();
                        let line = if age < STATUS_FOR || !self.adding.is_empty() {
                            ui.ctx()
                                .request_repaint_after(STATUS_FOR.saturating_sub(age));
                            line
                        } else {
                            String::new()
                        };
                        ui.add(
                            egui::Label::new(RichText::new(line).small().color(th().muted))
                                .truncate(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // (said the same way as the banner says it)
                            let output = match self.output.strip_prefix("no sound: ") {
                                Some(why) => format!("No sound – {why}"),
                                None => self.output.clone(),
                            };
                            ui.label(RichText::new(output).small().color(th().muted));
                        });
                    });
                })
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(th().ground).inner_margin(10.0))
            .show(ui, |ui| {
                match self.tab {
                    Tab::Mixer => {
                        let wake = ui.ctx().clone();
                        let changes = crate::mixer_ui::show(
                            ui,
                            &mut self.mixer_page,
                            &self.output_choice,
                            self.volume,
                            self.muted,
                            self.eq.enabled,
                            self.accent,
                            move || wake.request_repaint(),
                        );
                        for change in changes {
                            self.apply(change);
                        }
                        return;
                    }
                    Tab::Bluetooth => {
                        self.bluetooth_tab(ui);
                        return;
                    }
                    Tab::Player => {}
                }
                self.paint_haze(ui);
                // (a wide window: a column of the width it reads best at,
                // in the middle - not a card stretched across 1,900 px)
                let room = ui.available_rect_before_wrap();
                // (zen: a narrow column whatever the window - one thing at
                // a time, much air either side)
                let narrow = crate::design::current().column;
                let width = if narrow > 0.0 { narrow } else { 1180.0 };
                if room.width() > width + 60.0 && !self.big_visual {
                    let column =
                        egui::Rect::from_center_size(room.center(), vec2(width, room.height()));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                        self.player_page(ui)
                    });
                } else {
                    self.player_page(ui);
                }
            });
        self.url_window(&ctx);
        self.intense_window(&ctx);
        self.details_window(&ctx);
        self.file_dialog(&ctx);
        if let Some((path, frames, written)) = self.stats.as_mut() {
            *frames += 1;
            if written.elapsed() > Duration::from_millis(250) {
                *written = Instant::now();
                let _ = std::fs::write(&*path, format!("frames {frames}\n"));
            }
        }
        // the pictures on a display of their own
        if self.external.is_some() {
            self.external_view(&ctx);
        }
        self.govern();
        let fps = self.fps_cap();
        // the picture moves while there is sound, and as long as it falls to
        // rest; off, the clock moves once a second; idle, nothing is drawn
        // until something changes (input, the engine: they wake the window)
        if self.mode == "off" {
            // (a design's reels and needles move at a calm 15 frames a
            // second; else the clock once a second)
            let x = crate::design::current().extras;
            if self.playing() && (x.reels || x.needles) {
                ctx.request_repaint_after(Duration::from_secs_f32(1.0 / 15.0));
            } else if self.playing() {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        } else if self.playing()
            || (!self.frozen() && self.analyzer.bars.iter().any(|b| *b > 0.001))
        {
            ctx.request_repaint_after(Duration::from_secs_f32(1.0 / fps));
        }
    }

    fn on_exit(&mut self) {
        self.save(true);
        #[cfg(target_os = "linux")]
        self.saver.save(crate::saver::Job::Cover(None));
    }
}

impl Drop for App {
    /// The last changes are kept also when the window ends without
    /// `on_exit` (a test harness, a platform that skips it).
    fn drop(&mut self) {
        self.save(true);
    }
}

/// Which keys of the player the window takes (see [`App::key_gate`]).
#[derive(Clone, Copy, Default)]
struct KeyGate {
    any: bool,
    /// Space and Enter (no widget has the keyboard's focus).
    activation: bool,
}

/// The tabs of the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Player,
    Mixer,
    Bluetooth,
}

/// The slider's volume (0-100) as a gain: cubic, as axross's player
/// (`perceptual_to_linear`) - the same number sounds the same in both, and
/// the middle of the slider sounds like the middle.
fn volume_curve(volume: u32) -> f32 {
    (volume.min(100) as f32 / 100.0).powi(3)
}

fn band_name(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{}k", hz / 1000.0)
    } else {
        format!("{hz}")
    }
}

/// A local path as a file:// URL (what is not plain, percent-encoded).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn file_url(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// An album's head in the playlist: its cover, its name and who made it,
/// its tracks and length.
fn album_head(
    ui: &mut Ui,
    head: &AlbumHead,
    row: f32,
    cover: Option<&TextureHandle>,
    accent: Color32,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::hover());
    let title = if head.artist.is_empty() {
        head.album.clone()
    } else {
        format!("{} – {}", head.album, head.artist)
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &title));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let art =
        egui::Rect::from_min_size(rect.left_top() + vec2(4.0, 3.0), vec2(row - 6.0, row - 6.0));
    match cover {
        Some(t) => {
            painter.add(
                egui::epaint::RectShape::filled(art, CornerRadius::same(5), Color32::WHITE)
                    .with_texture(
                        t.id(),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    ),
            );
        }
        None => {
            painter.rect_filled(art, 5.0, viz::mix(th().surface, accent, 0.3));
            painter.text(
                art.center(),
                egui::Align2::CENTER_CENTER,
                "♫",
                egui::FontId::proportional(16.0),
                viz::mix(th().muted, th().strong, 0.4),
            );
        }
    }
    let x = art.right() + 10.0;
    painter.text(
        egui::pos2(x, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        &head.album,
        bold(14.0),
        th().strong,
    );
    painter.text(
        egui::pos2(x, rect.center().y + 9.0),
        egui::Align2::LEFT_CENTER,
        if head.artist.is_empty() {
            ""
        } else {
            head.artist.as_str()
        },
        egui::FontId::proportional(12.0),
        accent,
    );
    let tracks = if head.tracks == 1 {
        "1 track".to_owned()
    } else {
        format!("{} tracks", head.tracks)
    };
    let length = if head.ms > 0 {
        format!(" · {}", length(head.ms as f64 / 1000.0))
    } else {
        String::new()
    };
    painter.text(
        egui::pos2(rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{tracks}{length}"),
        egui::FontId::proportional(12.0),
        th().muted,
    );
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, viz::mix(th().surface, accent, 0.25)),
    );
}

/// The playlist's folding header (opened from a menu and Ctrl+F too).
/// A row's two lines: its title (without the artist the title begins
/// with, when the second line names it) and "artist · album".
fn row_text(track: &crate::state::Track, title: &str) -> (String, String) {
    let mut by = Vec::new();
    if !track.artist.is_empty() {
        by.push(track.artist.as_str());
    }
    if !track.album.is_empty() {
        by.push(track.album.as_str());
    }
    let line = if !track.artist.is_empty() {
        title
            .strip_prefix(&format!("{} – ", track.artist))
            .unwrap_or(title)
            .to_owned()
    } else {
        title.to_owned()
    };
    (line, by.join(" · "))
}

fn playlist_header_id() -> egui::Id {
    egui::Id::NULL.with("amp-playlist")
}

/// ReplayGain's settings as the engine takes them.
fn loudness(amp: &state::AmpSettings) -> crate::engine::Loudness {
    crate::engine::Loudness {
        mode: crate::engine::GainMode::from_key(&amp.replay_gain),
        preamp_db: amp.preamp_db,
        untagged_db: amp.untagged_db,
    }
}

/// The host of an http(s) URL (for "Connecting to …").
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    authority.rsplit('@').next().unwrap_or(authority).to_owned()
}

/// A length (a track's, the playlist's): to the nearest second, the same
/// rule everywhere (a half-second track is 0:01, not 0:00 while the list's
/// total says 0:15).
pub fn length(secs: f64) -> String {
    clock((secs + 0.5).floor())
}

pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The cover (made small on the reading thread) as a texture: an upload.
fn cover_texture(ctx: &egui::Context, art: &crate::engine::Art) -> TextureHandle {
    let size = [art.width as usize, art.height as usize];
    let colour = egui::ColorImage::from_rgba_unmultiplied(size, &art.rgba);
    ctx.load_texture("cover", colour, egui::TextureOptions::LINEAR)
}

/// The look: the theme's (Midnight until the settings are read), its
/// fonts.
fn style(ctx: &egui::Context) {
    apply_theme(
        ctx,
        th(),
        th().accent,
        th().padding,
        crate::design::current(),
        crate::design::current(),
    );
    // small text readable (egui's is 9 px): 11 px on every tab
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(11.0));
    });
    // Hack (the monospace font) as the last fallback of the proportional
    // one: a character the others lack is drawn, not a box.
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional)
        && !family.iter().any(|f| f == "Hack")
    {
        family.push("Hack".to_owned());
    }
    // a bold face for titles, where the system has one (egui brings only a
    // light one); else the titles are in that one too
    let proportional = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut bold = proportional.clone();
    if let Some(data) = BOLD_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert(
            "axross-bold".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        bold.insert(0, "axross-bold".to_owned());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("bold".into()), bold);
    ctx.set_fonts(fonts);
}

/// A theme and an accent on the window: egui's visuals of both its light
/// and dark styles (whichever the theme is, egui is told so), its density.
fn apply_theme(
    ctx: &egui::Context,
    t: &crate::theme::Theme,
    accent: Color32,
    padding: [f32; 2],
    d: &crate::design::Design,
    layout: &crate::design::Design,
) {
    let which = if t.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(which);
    let mut visuals = crate::theme::visuals(t, accent);
    crate::design::restyle(&mut visuals, d, t);
    ctx.set_visuals_of(which, visuals);
    ctx.all_styles_mut(|s| {
        crate::design::retype(s, layout);
        // (the theme's density where it asks more room: high contrast)
        s.spacing.button_padding = s.spacing.button_padding.max(vec2(padding[0], padding[1]));
    });
}

/// Bold system faces looked for (Linux, macOS, Windows), in order.
const BOLD_FONTS: [&str; 6] = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "C:\\Windows\\Fonts\\segoeuib.ttf",
];

/// The bold face at `size` (the type scale's titles).
fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("bold".into()))
}

/// The now-playing card's fill: the window's panel tinted with the accent,
/// a light accent less (its quiet words and the accent must read on it).
pub fn card_tint(accent: Color32) -> Color32 {
    card_tint_in(th(), accent)
}

/// As [`card_tint`], in a theme.
/// The now-playing card's fill in a theme, for an accent (as
/// [`card_tint`]; the designs' tests).
pub fn card_tint_for(t: &crate::theme::Theme, accent: Color32) -> Color32 {
    card_tint_in(t, accent)
}

fn card_tint_in(t: &crate::theme::Theme, accent: Color32) -> Color32 {
    let need = crate::theme::text_contrast(t);
    let mut k = if t.high_contrast { 0.08 } else { 0.14 };
    loop {
        let card = viz::mix(t.surface, accent, k);
        let reads = [t.muted, t.text, accent]
            .iter()
            .all(|c| viz::contrast(*c, card) >= need);
        if k <= 0.02 || reads {
            return card;
        }
        k -= 0.02;
    }
}

/// What is chosen (the tab shown, the playlist's chip, a selected row):
/// filled with the accent, its words light or dark on it - whichever
/// reads (WCAG 1.4.3: 4.5:1), the fill made darker where neither does.
pub fn selection_colours(accent: Color32) -> (Color32, Color32) {
    crate::theme::selection(th(), accent)
}

/// The toolbar's icons, drawn (one set, one stroke: not emoji glyphs of
/// whatever font has them).
#[derive(Clone, Copy)]
enum Icon {
    Files,
    Folder,
    Radio,
}

/// A button with a drawn icon before its text.
fn icon_button(ui: &mut Ui, icon: Icon, text: &str, min: egui::Vec2) -> egui::Response {
    // (room for the icon: the text set off by spaces, the icon drawn there)
    let response = ui
        .add(egui::Button::new(format!("       {text}")).min_size(min))
        .accessible_name(text);
    let colour = ui.style().interact(&response).text_color();
    let stroke = Stroke::new(1.4, colour);
    let painter = ui.painter();
    let left = if min.x > 0.0 {
        // (a wide button: its text centred, the icon before it)
        let galley = painter.layout_no_wrap(
            format!("       {text}"),
            egui::TextStyle::Button.resolve(ui.style()),
            colour,
        );
        response.rect.center().x - galley.size().x / 2.0 + 2.0
    } else {
        response.rect.left() + ui.spacing().button_padding.x
    };
    let c = egui::pos2(left + 7.0, response.rect.center().y);
    let r = egui::Rect::from_center_size(c, vec2(12.0, 14.0));
    match icon {
        Icon::Files => {
            // a sheet with its corner folded
            let fold = 4.0;
            painter.add(egui::Shape::closed_line(
                vec![
                    r.left_top(),
                    egui::pos2(r.right() - fold, r.top()),
                    egui::pos2(r.right(), r.top() + fold),
                    r.right_bottom(),
                    r.left_bottom(),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    egui::pos2(r.right() - fold, r.top()),
                    egui::pos2(r.right() - fold, r.top() + fold),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.right() - fold, r.top() + fold),
                    egui::pos2(r.right(), r.top() + fold),
                ],
                stroke,
            );
        }
        Icon::Folder => {
            let r = egui::Rect::from_center_size(c, vec2(15.0, 11.0));
            painter.add(egui::Shape::closed_line(
                vec![
                    egui::pos2(r.left(), r.top()),
                    egui::pos2(r.left() + 5.0, r.top()),
                    egui::pos2(r.left() + 7.0, r.top() + 2.0),
                    egui::pos2(r.right(), r.top() + 2.0),
                    r.right_bottom(),
                    r.left_bottom(),
                ],
                stroke,
            ));
        }
        Icon::Radio => {
            // a mast sending: a dot and two arcs each side
            painter.circle_filled(c + vec2(0.0, 1.0), 1.8, colour);
            for radius in [4.5f32, 7.5] {
                for side in [-1.0f32, 1.0] {
                    let points: Vec<egui::Pos2> = (0..=6)
                        .map(|i| {
                            let a = (-0.6 + 1.2 * i as f32 / 6.0)
                                + if side < 0.0 {
                                    std::f32::consts::PI
                                } else {
                                    0.0
                                };
                            c + vec2(0.0, 1.0) + vec2(a.cos(), a.sin()) * radius
                        })
                        .collect();
                    painter.add(egui::Shape::line(points, stroke));
                }
            }
        }
    }
    response
}

/// What the start says of a slow renderer (its frame rates from the
/// constants that set them).
fn slow_note(slow: bool) -> String {
    if slow {
        format!(
            "; a slow renderer: at most {SLOW_FPS:.0} frames a second, fewer (down to \
             {MIN_SLOW_FPS:.0}) as a frame costs more"
        )
    } else {
        String::new()
    }
}

/// Whether the desktop asks for reduced motion, found out on a thread;
/// None until known, or where there is no way to ask.
fn desktop_reduced_motion() -> Arc<Mutex<Option<bool>>> {
    let out = Arc::new(Mutex::new(None));
    let found = out.clone();
    let _ = std::thread::Builder::new()
        .name("amp-motion".into())
        .spawn(move || {
            let answer = reduced_motion_asked();
            if let Ok(mut f) = found.lock() {
                *f = answer;
            }
        });
    out
}

/// The desktop's own word on motion: GNOME (animations off), KDE (an
/// animation speed of 0), GTK's settings (animations off), macOS (Reduce
/// motion), Windows (animate windows off). None where none says.
fn reduced_motion_asked() -> Option<bool> {
    let run = |program: &str, args: &[&str]| -> Option<String> {
        std::process::Command::new(program)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    if cfg!(target_os = "macos") {
        return run(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .map(|v| v == "1");
    }
    if cfg!(windows) {
        return run(
            "reg",
            &[
                "query",
                r"HKCU\Control Panel\Desktop\WindowMetrics",
                "/v",
                "MinAnimate",
            ],
        )
        .map(|v| v.split_whitespace().last() == Some("0"));
    }
    let mut asked = None;
    if let Some(v) = run(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    ) {
        asked = Some(v == "false");
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")));
    if let Some(config) = config {
        if let Ok(text) = std::fs::read_to_string(config.join("kdeglobals"))
            && let Some(factor) = setting(&text, "AnimationDurationFactor")
        {
            asked = Some(asked.unwrap_or(false) || factor.parse::<f32>().is_ok_and(|f| f <= 0.0));
        }
        for gtk in ["gtk-3.0/settings.ini", "gtk-4.0/settings.ini"] {
            if let Ok(text) = std::fs::read_to_string(config.join(gtk))
                && let Some(v) = setting(&text, "gtk-enable-animations")
            {
                asked = Some(asked.unwrap_or(false) || v == "false" || v == "0");
            }
        }
    }
    asked
}

/// A `key=value` setting's value in an ini-like text.
fn setting(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_owned())
    })
}

/// The cover's accent: the strongest of its colourful hues (weighed by how
/// colourful and how bright), made readable on the window as text (WCAG AA,
/// 4.5:1); none for a grey cover.
pub fn accent_of(art: &crate::engine::Art) -> Option<Color32> {
    cover_hue(art).map(|c| accent_for(c, th()))
}

/// The cover's strongest colourful hue, as it is (none for a grey cover).
fn cover_hue(art: &crate::engine::Art) -> Option<Color32> {
    Some(viz::hues(&art.rgba)?[0].1.0)
}

/// A colour (the cover's) made the accent of a theme: its text reads on
/// the window, the cards, the controls and the now-playing card it tints
/// (4.5:1; 7:1 in high contrast).
pub fn accent_for(colour: Color32, t: &crate::theme::Theme) -> Color32 {
    let need = crate::theme::text_contrast(t);
    let toward = if t.dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut c = crate::theme::accent_from(colour, t);
    for _ in 0..24 {
        let card = card_tint_in(t, c);
        if viz::contrast(c, t.ground) >= need && viz::contrast(c, card) >= need {
            break;
        }
        c = viz::mix(c, toward, 0.12);
    }
    c
}

/// A track's format in words: its codec, rate, channels and bits (unless
/// the codec's name says them already: "PCM 16-bit").
fn format_line(codec: &str, rate: u32, channels: usize, bits: Option<u32>) -> String {
    let bits = bits
        .filter(|b| !codec.contains(&format!("{b}-bit")) && !codec.contains(&format!("{b} bit")))
        .map(|b| format!(", {b} bit"))
        .unwrap_or_default();
    let s = if channels == 1 { "" } else { "s" };
    format!("{codec}, {rate} Hz, {channels} channel{s}{bits}")
}

/// The card's line of a track's format: its codec, the bits (where the
/// codec has them and does not say them) or the bit rate, the rate in kHz,
/// the channels: "FLAC · 24-bit · 96 kHz · 2 ch", "MP3 · 192 kbps ·
/// 44.1 kHz · 2 ch".
fn card_format(i: &crate::engine::TrackInfo) -> String {
    // (the codec where it is known yet)
    let mut parts: Vec<String> = Some(i.codec.clone())
        .filter(|c| !c.is_empty())
        .into_iter()
        .collect();
    match (i.bits, i.bitrate) {
        (Some(b), _) if !i.codec.contains(&format!("{b}-bit")) => parts.push(format!("{b}-bit")),
        (None, Some(kbps)) => parts.push(format!("{kbps} kbps")),
        _ => {}
    }
    let khz = i.rate as f32 / 1000.0;
    parts.push(if khz.fract() == 0.0 {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    });
    parts.push(format!("{} ch", i.channels));
    parts.join(" · ")
}

/// The theme's light or dark ink, whichever reads better on `fill`.
fn ink_on(fill: Color32) -> Color32 {
    crate::theme::ink_on(fill)
}

/// The faces in their groups, to choose one (the mode box).
fn faces_list(ui: &mut Ui, mode: &mut &'static str) {
    let mut last = None;
    for face in viz::FACES {
        if last != Some(face.group) && face.group != viz::Group::Off {
            ui.label(RichText::new(face.group.name()).small().color(th().muted));
        } else if face.group == viz::Group::Off {
            ui.separator();
        }
        last = Some(face.group);
        ui.selectable_value(mode, face.key, face.name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_albums_tracks_come_under_its_head() {
        let t = |path: &str, album: &str, artist: &str, ms: u64| Track {
            path: path.into(),
            album: album.into(),
            artist: artist.into(),
            duration_ms: ms,
            ..Default::default()
        };
        let tracks = vec![
            t("/m/a/1.flac", "Blue", "Joni", 1000),
            t("/m/a/2.flac", "Blue", "Joni", 2000),
            t("/m/single.mp3", "", "", 0),
            t("/m/b/1.flac", "Blue", "Other", 500),
            t("/m/c/1.flac", "Mix", "X", 0),
            t("/m/c/2.flac", "Mix", "Y", 0),
        ];
        let rows: Vec<usize> = (0..tracks.len()).collect();
        let entries = with_albums(&tracks, &rows);
        let heads: Vec<(String, String, usize, u64)> = entries
            .iter()
            .filter_map(|e| match e {
                Entry::Album(h) => Some((h.album.clone(), h.artist.clone(), h.tracks, h.ms)),
                _ => None,
            })
            .collect();
        // (an album of one track - in another folder - has no head; one of
        // several artists is "Various artists")
        assert_eq!(
            heads,
            vec![
                ("Blue".into(), "Joni".into(), 2, 3000),
                ("Mix".into(), "Various artists".into(), 2, 0),
            ]
        );
        assert_eq!(entries.len(), tracks.len() + 2);
        assert_eq!(entries[1], Entry::Track(0));
        assert_eq!(entries[3], Entry::Track(2));
    }

    #[test]
    fn the_start_tells_the_slow_frame_rate_as_it_is() {
        let note = slow_note(true);
        assert!(
            note.contains(&format!("at most {SLOW_FPS:.0} frames")),
            "{note}"
        );
        assert!(!note.contains("20 frames"));
        assert!(slow_note(false).is_empty());
    }

    #[test]
    fn the_desktops_wish_for_less_motion_is_read() {
        let kde = "[General]\nfoo=1\n[KDE]\nAnimationDurationFactor=0\n";
        assert_eq!(
            setting(kde, "AnimationDurationFactor").as_deref(),
            Some("0")
        );
        let gtk = "[Settings]\ngtk-enable-animations = false\n";
        assert_eq!(
            setting(gtk, "gtk-enable-animations").as_deref(),
            Some("false")
        );
        assert_eq!(setting(gtk, "gtk-theme-name"), None);
    }

    #[test]
    fn chosen_things_read_whatever_the_cover() {
        // covers of every hue, light and dark: the accent made of them, the
        // chosen tab's words on its fill, the accent's words on the window
        let ground = th().ground;
        for k in 0..36 {
            for (l, c) in [(0.45, 0.15), (0.7, 0.18), (0.85, 0.1), (0.3, 0.12)] {
                let colour = viz::from_oklch(l, c, k as f32 * 10.0);
                let rgba: Vec<u8> = (0..64 * 64)
                    .flat_map(|_| [colour.r(), colour.g(), colour.b(), 255])
                    .collect();
                let art = crate::engine::Art {
                    width: 64,
                    height: 64,
                    rgba,
                    png: Vec::new(),
                    large: None,
                };
                let accent = accent_of(&art).unwrap_or(th().accent);
                let (fill, ink) = selection_colours(accent);
                assert!(
                    viz::contrast(ink, fill) >= 4.5,
                    "{colour:?}: chosen words {:.2}:1",
                    viz::contrast(ink, fill)
                );
                assert!(
                    viz::contrast(accent, ground) >= 4.5,
                    "{colour:?}: accent on the window"
                );
                // the card tinted with it: the artist (in the accent) and
                // the quiet words on it
                let card = card_tint(accent);
                assert!(
                    viz::contrast(accent, card) >= 4.5,
                    "{colour:?}: accent on the card {:.2}:1",
                    viz::contrast(accent, card)
                );
                assert!(
                    viz::contrast(th().muted, card) >= 4.5,
                    "{colour:?}: quiet words on the card"
                );
            }
        }
        let (fill, ink) = selection_colours(th().accent);
        assert!(viz::contrast(ink, fill) >= 4.5);
    }

    #[test]
    fn the_cover_accent_reads_in_every_theme() {
        // covers of every hue, light and dark, in every theme: the row that
        // plays (its title, its quiet line), what is chosen and its words,
        // and a slider's fill against its rail and the ground
        let mut failures = Vec::new();
        for t in &crate::theme::THEMES {
            let need = crate::theme::text_contrast(t);
            for k in 0..36 {
                for (l, c) in [
                    (0.45, 0.15),
                    (0.7, 0.18),
                    (0.85, 0.12),
                    (0.3, 0.12),
                    (0.92, 0.08),
                ] {
                    let raw = viz::from_oklch(l, c, k as f32 * 10.0);
                    let accent = if t.high_contrast {
                        t.accent
                    } else {
                        accent_for(raw, t)
                    };
                    let row = crate::theme::playing_row(t, accent);
                    for (what, ink) in [("title", t.strong), ("text", t.text), ("quiet", t.muted)] {
                        if viz::contrast(ink, row) < need {
                            failures.push(format!(
                                "{}: {raw:?}: {what} on the playing row {:.2}",
                                t.name,
                                viz::contrast(ink, row)
                            ));
                        }
                    }
                    let (fill, ink) = crate::theme::selection(t, accent);
                    if viz::contrast(ink, fill) < need {
                        failures.push(format!(
                            "{}: {raw:?}: chosen words {:.2}",
                            t.name,
                            viz::contrast(ink, fill)
                        ));
                    }
                    for (what, g) in [("rail", t.track), ("ground", t.ground)] {
                        if viz::contrast(fill, g) < 3.0 {
                            failures.push(format!(
                                "{}: {raw:?}: slider fill on the {what} {:.2}",
                                t.name,
                                viz::contrast(fill, g)
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures, e.g. {:#?}",
            failures.len(),
            &failures[..failures.len().min(12)]
        );
    }

    #[test]
    fn the_card_says_the_bits_or_the_bit_rate() {
        let info = |codec: &str, rate, bits, bitrate| crate::engine::TrackInfo {
            codec: codec.into(),
            rate,
            channels: 2,
            bits,
            bitrate,
            ..Default::default()
        };
        assert_eq!(
            card_format(&info("FLAC", 96_000, Some(24), Some(2900))),
            "FLAC · 24-bit · 96 kHz · 2 ch"
        );
        assert_eq!(
            card_format(&info("MP3", 44_100, None, Some(192))),
            "MP3 · 192 kbps · 44.1 kHz · 2 ch"
        );
        assert_eq!(
            card_format(&info("PCM 16-bit", 48_000, Some(16), Some(1536))),
            "PCM 16-bit · 48 kHz · 2 ch"
        );
        // (no codec known yet: no separator before nothing)
        assert_eq!(
            card_format(&info("", 48_000, Some(16), None)),
            "16-bit · 48 kHz · 2 ch"
        );
    }

    #[test]
    fn the_format_says_the_bits_once() {
        assert_eq!(
            format_line("PCM 16-bit", 48_000, 2, Some(16)),
            "PCM 16-bit, 48000 Hz, 2 channels"
        );
        assert_eq!(
            format_line("FLAC", 44_100, 1, Some(24)),
            "FLAC, 44100 Hz, 1 channel, 24 bit"
        );
        assert_eq!(
            format_line("MP3", 44_100, 2, None),
            "MP3, 44100 Hz, 2 channels"
        );
    }

    #[test]
    fn the_accent_comes_from_the_cover_and_reads_on_the_window() {
        let art = |rgb: [u8; 3], share: usize| {
            // `share` of 10 pixels in the colour, the rest grey
            let rgba: Vec<u8> = (0..64 * 64)
                .flat_map(|i| {
                    if i % 10 < share {
                        [rgb[0], rgb[1], rgb[2], 255]
                    } else {
                        [90, 90, 90, 255]
                    }
                })
                .collect();
            crate::engine::Art {
                width: 64,
                height: 64,
                rgba,
                png: Vec::new(),
                large: None,
            }
        };
        let ground = th().ground;
        // a dark red cover: a red accent, lightened until it reads (4.5:1)
        let red = accent_of(&art([120, 10, 20], 6)).expect("an accent");
        assert!(red.r() > red.g() && red.r() > red.b(), "{red:?}");
        assert!(viz::contrast(red, ground) >= 4.5);
        // a grey cover: none (the window's own)
        assert_eq!(accent_of(&art([100, 100, 100], 10)), None);
        // the play button's mark reads on any accent
        for c in [red, Color32::from_rgb(250, 240, 120), th().accent] {
            assert!(viz::contrast(ink_on(c), c) >= 3.0, "{c:?}");
        }
    }

    #[test]
    fn file_urls_go_both_ways() {
        let path = "/m/Bob & Alice/01 – Ça va?.flac";
        let url = file_url(path);
        assert_eq!(
            url,
            "file:///m/Bob%20%26%20Alice/01%20%E2%80%93%20%C3%87a%20va%3F.flac"
        );
        assert_eq!(playlist::percent_decode(&url["file://".len()..]), path);
    }

    #[test]
    fn the_volume_sounds_as_loud_as_in_axross() {
        assert_eq!(volume_curve(100), 1.0);
        assert!((volume_curve(50) - 0.125).abs() < 1e-6);
        assert_eq!(volume_curve(0), 0.0);
        assert_eq!(volume_curve(250), 1.0);
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(61.9), "1:01");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(length(0.5), "0:01");
        assert_eq!(length(0.4), "0:00");
        assert_eq!(length(61.6), "1:02");
        assert_eq!(band_name(16000.0), "16k");
        assert_eq!(band_name(62.0), "62");
    }
}
