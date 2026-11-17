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

const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
const MUTED: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
const PANEL: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const SAVE_EVERY: Duration = Duration::from_secs(3);
/// How long a message stays in the status line.
const STATUS_FOR: Duration = Duration::from_secs(8);
const POSITION_EVERY: Duration = Duration::from_secs(30);
/// The equalizer is written this long after its last change (axross's 250 ms).
const EQ_SETTLE: Duration = Duration::from_millis(250);
/// A track that could not be played (axross's red).
const BROKEN: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4b);

#[derive(PartialEq)]
enum Picking {
    Nothing,
    Files,
    Folder,
    SavePlaylist,
}

pub struct App {
    engine: Engine,
    playlist: Playlist,
    info: TrackInfo,
    cover: Option<TextureHandle>,
    /// The accent: from the cover (its strongest colour, readable on the
    /// window), else the window's blue.
    accent: Color32,
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
    playlist_open: bool,
    big_visual: bool,
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
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            viz::gpu::install(render_state);
            let forced = std::env::var_os("AMP_GPU_SHADERS").is_some_and(|v| v == "1");
            viz_state.set_gpu(forced || !viz::gpu::is_software(render_state));
        }
        let mut app = App {
            engine,
            playlist,
            info: TrackInfo::default(),
            cover: None,
            accent: ACCENT,
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
            playlist_open: saved.playlist_open,
            big_visual: false,
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
            .color(MUTED),
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
            .fill(Color32::from_rgb(0x3a, 0x2a, 0x10))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("⚠ No sound – {why}"))
                                .color(Color32::from_rgb(0xe3, 0xb3, 0x41)),
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
    fn take_info(&mut self, ctx: &egui::Context, info: TrackInfo) {
        self.loaded = Some(self.generation);
        self.analyzer.new_track();
        self.meters.new_track();
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
        let accent = info.art.as_deref().and_then(accent_of).unwrap_or(ACCENT);
        if accent != self.accent {
            self.accent = accent;
            accent_style(ctx, accent);
        }
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
            return self.next(true);
        }
        self.engine.send(Command::Seek(secs));
        #[cfg(target_os = "linux")]
        if let Some(m) = &self.mpris {
            m.seeked(secs);
        }
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
                    .color(MUTED),
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
        egui::Window::new("Track details")
            .open(&mut open)
            .collapsible(false)
            .default_width(420.0)
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
                        put(
                            "Format",
                            format!(
                                "{}, {} Hz, {} channel{}{}",
                                i.codec,
                                i.rate,
                                i.channels,
                                if i.channels == 1 { "" } else { "s" },
                                i.bits.map(|b| format!(", {b} bit")).unwrap_or_default()
                            ),
                        );
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
                                    ui.label(RichText::new(k).color(MUTED));
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
        match self.playlist.next(ended) {
            Some(i) => {
                self.play_index(i, 0, true);
                self.automatic = ended;
            }
            None => {
                self.stop();
                self.status = "End of the playlist".into();
            }
        }
    }

    fn previous(&mut self) {
        // within the first seconds: the track before; later: this one from
        // its start (a stream cannot go back: the track before)
        let seekable = self.info.seekable && self.loaded == Some(self.generation);
        if seekable && self.engine.shared.position() > 3.0 {
            self.seek(0.0);
        } else if let Some(i) = self.playlist.previous() {
            self.play_index(i, 0, true);
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
        for (key, sign) in [(Key::ArrowLeft, -1.0), (Key::ArrowRight, 1.0)] {
            if !pressed(key) {
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
        for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
            if !pressed(key) {
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
        if pressed(Key::Escape) && self.fullscreen {
            self.set_fullscreen(ui.ctx(), false);
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
            if ui
                .button("➕ Files")
                .on_hover_text("Add files and playlists")
                .clicked()
            {
                self.picking = Picking::Files;
                self.dialog.pick_multiple();
            }
            if ui
                .button("📁 Folder")
                .on_hover_text("Add a folder and everything in it")
                .clicked()
            {
                self.picking = Picking::Folder;
                self.dialog.pick_directory();
            }
            if ui
                .button("📻 URL")
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
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(viz::name(mode))
                    .width(150.0)
                    .height(560.0)
                    .show_ui(ui, |ui| faces_list(ui, &mut mode))
                    .response
                    .accessible_name("Visualization");
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
        ui.menu_button("Visualization", |ui| {
            // the groups side by side: every face at a glance, no menu in a menu
            ui.horizontal_top(|ui| {
                for group in [
                    viz::Group::Classic,
                    viz::Group::Analysis,
                    viz::Group::Generative,
                ] {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(group.name()).strong().color(MUTED));
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
        ui.label(RichText::new("Colours").strong());
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
        }
        ui.separator();
        let mut o = self.options.clone();
        ui.add(egui::Slider::new(&mut o.bars, 8..=128).text("Bars"));
        ui.add(egui::Slider::new(&mut o.smoothing, 0..=95).text("Smoothing %"));
        ui.add(egui::Slider::new(&mut o.peak_fall, 5..=300).text("Peak fall %/s"));
        ui.add(egui::Slider::new(&mut o.fps, 10..=60).text("Frame rate cap"));
        ui.checkbox(&mut o.hide_cursor, "Hide the pointer in full screen");
        if o != self.options {
            self.options = o.clamped();
            self.dirty = true;
        }
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
        // a card tinted with the cover's colour
        let tint = viz::mix(PANEL, self.accent, 0.14);
        egui::Frame::new()
            .fill(tint)
            .corner_radius(CornerRadius::same(12))
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let size = vec2(84.0, 84.0);
                    if let Some(texture) = &self.cover {
                        ui.add(
                            egui::Image::new(texture)
                                .fit_to_exact_size(size)
                                .corner_radius(CornerRadius::same(8)),
                        );
                    } else {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 8.0, viz::mix(PANEL, self.accent, 0.3));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "♫",
                            egui::FontId::proportional(36.0),
                            viz::mix(MUTED, Color32::WHITE, 0.4),
                        );
                    }
                    ui.vertical(|ui| {
                        let (title, by) = self.now_playing_text();
                        ui.add(
                            egui::Label::new(RichText::new(title).size(20.0).strong()).truncate(),
                        );
                        if !by.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(by).size(14.0).color(self.accent))
                                    .truncate(),
                            );
                        }
                        if !self.info.album.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(&self.info.album).color(MUTED))
                                    .truncate(),
                            );
                        }
                        if self.info.rate > 0 {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} Hz · {} ch",
                                    self.info.codec, self.info.rate, self.info.channels
                                ))
                                .small()
                                .color(MUTED),
                            );
                        }
                    });
                });
            });
    }

    /// One line of what plays (the large picture's): its cover and title.
    fn title_line(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some(texture) = &self.cover {
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
                Request::Next => self.next(false),
                Request::Previous => self.previous(),
                Request::Seek(us) => {
                    let pos = self.engine.shared.position();
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
        let scheme = viz::scheme(&self.scheme_key, &self.custom_colours);
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
        if self.mode != "off" && !frozen {
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
            hover: response.hover_pos(),
            opacity: 1.0,
            second: false,
            loudness_target: self.amp.loudness_target,
        };
        viz::draw(&frame, self.mode, &mut self.viz_state);
        drop(overview);
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
        let response = response.on_hover_text(format!(
            "{} - click: next face, double-click: larger, right click: options (V, F11)",
            viz::name(self.mode)
        ));
        response.context_menu(|ui| self.visual_menu(ui));
    }

    fn position(&mut self, ui: &mut Ui) {
        let pos = self
            .seek_drag
            .unwrap_or_else(|| self.engine.shared.position());
        let seekable = self.info.seekable && self.duration.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new(clock(pos)).monospace().color(MUTED));
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
                    .color(MUTED),
                );
                ui.spacing_mut().slider_width = ui.available_width() - 8.0;
                let mut value = pos.min(total.max(0.0));
                let slider = egui::Slider::new(&mut value, 0.0..=total.max(0.001))
                    .show_value(false)
                    .trailing_fill(true);
                let response = ui
                    .add_enabled(seekable, slider)
                    .accessible_name("Position")
                    .on_hover_text("Seek: drag, click, the wheel, ←/→ (Shift: 30 s)");
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
            let big = if narrow {
                vec2(36.0, 32.0)
            } else {
                vec2(46.0, 34.0)
            };
            if ui
                .add(egui::Button::new(RichText::new("⏮").size(18.0)).min_size(big))
                .accessible_name("Previous")
                .on_hover_text("Previous (P)")
                .clicked()
            {
                self.previous();
            }
            let play = if self.playing() { "⏸" } else { "▶" };
            let button =
                egui::Button::new(RichText::new(play).size(22.0).color(ink_on(self.accent)))
                    .fill(self.accent)
                    .min_size(if narrow {
                        vec2(48.0, 36.0)
                    } else {
                        vec2(58.0, 38.0)
                    })
                    .corner_radius(CornerRadius::same(19));
            if ui
                .add(button)
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
                        MUTED
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
                        MUTED
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
                let response = ui
                    .add(
                        egui::Slider::new(&mut volume, 0..=100)
                            .show_value(false)
                            .trailing_fill(true),
                    )
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
                    .add(egui::Button::new(RichText::new(icon).size(16.0)).frame(false))
                    .accessible_name("Mute")
                    .on_hover_text("Mute (M)")
                    .clicked()
                {
                    self.set_muted(!self.muted);
                }
                ui.menu_button(RichText::new("⋯").size(16.0), |ui| self.more_menu(ui))
                    .response
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
            let list = format!("Playlist  {} · {}", n, clock(total as f64 / 1000.0));
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
            if ui
                .small_button("Flat")
                .on_hover_text("Every band to 0 dB")
                .clicked()
            {
                self.set_eq([0.0; 10], Some("Flat"));
            }
        });
        ui.add_space(4.0);
        let height = (ui.available_height() - 40.0).clamp(90.0, 220.0);
        ui.add_enabled_ui(self.eq.enabled, |ui| {
            ui.horizontal_top(|ui| {
                // the scale: +12 at the top, 0, -12 at the bottom
                let (scale, _) = ui.allocate_exact_size(vec2(34.0, height), Sense::hover());
                let mut gains = self.eq.gains;
                let mut changed = false;
                let width =
                    ((ui.available_width() - 9.0 * ui.spacing().item_spacing.x) / 10.0).max(26.0);
                let mut rails: Option<egui::Rect> = None;
                for (i, band) in eq::BANDS.iter().enumerate() {
                    ui.allocate_ui(vec2(width, height + 30.0), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.spacing_mut().slider_width = height;
                            let response = ui
                                .add(
                                    egui::Slider::new(
                                        &mut gains[i],
                                        -eq::MAX_GAIN_DB..=eq::MAX_GAIN_DB,
                                    )
                                    .vertical()
                                    .show_value(false)
                                    .trailing_fill(true),
                                )
                                .accessible_name(format!("Equalizer {}", band_name(*band)))
                                .on_hover_text(format!("{} {:+.1} dB", band_name(*band), gains[i]));
                            changed |= response.changed();
                            rails = Some(match rails {
                                Some(r) => r.union(response.rect),
                                None => response.rect,
                            });
                            ui.label(RichText::new(band_name(*band)).small().color(MUTED));
                            ui.label(
                                RichText::new(format!("{:+.0}", gains[i]))
                                    .small()
                                    .monospace()
                                    .color(if gains[i].abs() < 0.05 {
                                        MUTED
                                    } else {
                                        Color32::from_rgb(0xc9, 0xd1, 0xd9)
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
                    painter.line_segment(
                        [
                            egui::pos2(rails.left() - 4.0, zero),
                            egui::pos2(rails.right() + 4.0, zero),
                        ],
                        Stroke::new(1.0, MUTED.gamma_multiply(0.6)),
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
                            egui::FontId::proportional(10.5),
                            MUTED,
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
        if ui.small_button("Clear").clicked() && !self.playlist.tracks.is_empty() {
            self.remember_for_undo();
            let n = self.playlist.tracks.len();
            self.playlist.clear();
            self.select(None);
            self.dirty = true;
            self.status = format!("{n} tracks removed - Ctrl+Z brings them back.");
        }
        if !self.undo.is_empty()
            && ui
                .small_button("↶ Undo")
                .on_hover_text("Bring back what was removed (Ctrl+Z)")
                .clicked()
        {
            self.undo();
        }
        if ui
            .small_button("Save…")
            .on_hover_text("Save as an M3U playlist")
            .clicked()
        {
            self.picking = Picking::SavePlaylist;
            self.dialog.save_file();
        }
    }

    fn playlist_view(&mut self, ui: &mut Ui) {
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
                    .color(MUTED),
                );
            }
            let count = rows.len();
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
                .and_then(|t| rows.iter().position(|r| *r == t))
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
                for i in range.map(|r| rows[r]) {
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
                            self.accent.gamma_multiply(0.25)
                        } else if selected {
                            Color32::from_rgb(0x24, 0x2c, 0x38)
                        } else {
                            Color32::from_rgb(0x1c, 0x22, 0x2b)
                        };
                        ui.painter().rect_filled(rect, 4.0, fill);
                    }
                    let broken = self.broken.get(&track.path);
                    let colour = if broken.is_some() {
                        BROKEN
                    } else if current {
                        Color32::WHITE
                    } else {
                        Color32::from_rgb(0xc9, 0xd1, 0xd9)
                    };
                    let painter = ui.painter();
                    // the left: the cover and a playing mark for the track
                    // that plays, else its number
                    let thumb = egui::Rect::from_min_size(
                        rect.left_top() + vec2(6.0, 5.0),
                        vec2(30.0, 30.0),
                    );
                    if current {
                        if let Some(texture) = &self.cover {
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
                            MUTED,
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
                        clock(track.duration_ms as f64 / 1000.0)
                    } else if playlist::is_stream(&track.path) {
                        "📻".into()
                    } else {
                        String::new()
                    };
                    painter.text(
                        rect.right_top() + vec2(-8.0, 13.0),
                        egui::Align2::RIGHT_CENTER,
                        duration,
                        egui::FontId::monospace(12.0),
                        MUTED,
                    );
                    // the title, and under it who and from which album
                    let clip = egui::Rect::from_min_max(
                        rect.left_top() + vec2(46.0, 0.0),
                        rect.right_bottom() - vec2(64.0, 0.0),
                    );
                    let clipped = painter.with_clip_rect(clip);
                    let (line, by) = row_text(track, &title);
                    clipped.text(
                        clip.left_top() + vec2(0.0, 13.0),
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
                            MUTED,
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
                    let args: Vec<String> = paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    self.add(&args, Then::Nothing);
                    self.picking = Picking::Nothing;
                }
            }
            Picking::Folder => {
                if let Some(path) = self.dialog.take_picked() {
                    self.add(&[path.to_string_lossy().into_owned()], Then::Nothing);
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
        let ctx = ui.ctx().clone();
        self.gate = self.key_gate(&ctx);
        self.keys(ui);
        // dropped files and folders
        let dropped: Vec<String> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_string_lossy().into_owned())
                .filter(|p| !p.is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.add(&dropped, Then::PlayIfIdle);
        }
        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x0d, 0x11, 0x17))
                    .inner_margin(8.0),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x0d, 0x11, 0x17))
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ui, |ui| {
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
                    ui.add(egui::Label::new(RichText::new(line).small().color(MUTED)).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // (said the same way as the banner says it)
                        let output = match self.output.strip_prefix("no sound: ") {
                            Some(why) => format!("No sound – {why}"),
                            None => self.output.clone(),
                        };
                        ui.label(RichText::new(output).small().color(MUTED));
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x0d, 0x11, 0x17))
                    .inner_margin(10.0),
            )
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
                self.no_sound_banner(ui);
                if self.big_visual {
                    // the picture large, what plays above it
                    self.title_line(ui);
                    let h = ui.available_height() - 92.0;
                    self.visual(ui, h.max(120.0));
                    ui.add_space(6.0);
                    self.position(ui);
                    self.transport(ui);
                    return;
                }
                self.now_playing(ui);
                ui.add_space(8.0);
                // the picture large: half of what the controls leave, the
                // rest for the playlist (all of it when the list is folded)
                let lower = self.playlist_open || self.eq_page;
                let room = ui.available_height() - 100.0;
                let picture = if lower {
                    (room * 0.5).clamp(140.0, 520.0)
                } else {
                    (room - 36.0).max(140.0)
                };
                self.visual(ui, picture);
                ui.add_space(6.0);
                self.position(ui);
                self.transport(ui);
                ui.add_space(6.0);
                self.lower_switch(ui);
                if self.eq_page {
                    self.equalizer_page(ui);
                } else if self.playlist_open {
                    self.playlist_view(ui);
                }
            });
        self.url_window(&ctx);
        self.details_window(&ctx);
        self.file_dialog(&ctx);
        let fps = self.options.fps.clamp(10, 60) as f32;
        // the picture moves while there is sound, and as long as it falls to
        // rest; off, the clock moves once a second; idle, nothing is drawn
        // until something changes (input, the engine: they wake the window)
        if self.mode == "off" {
            if self.playing() {
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

/// The look: dark, rounded, one blue accent.
fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(0x0d, 0x11, 0x17);
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(0x0a, 0x0d, 0x12);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    // (no lines down the side of what folds open)
    visuals.indent_has_left_vline = false;
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x21, 0x26, 0x2d);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x30, 0x36, 0x3d);
    visuals.slider_trailing_fill = true;
    ctx.set_visuals(visuals);
    // Hack (the monospace font) as the last fallback of the proportional
    // one: a character the others lack is drawn, not a box.
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional)
        && !family.iter().any(|f| f == "Hack")
    {
        family.push("Hack".to_owned());
    }
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 4.0);
    });
}

/// The window's colours of selection and links in `accent`.
fn accent_style(ctx: &egui::Context, accent: Color32) {
    let mut visuals = ctx.global_style().visuals.clone();
    visuals.selection.bg_fill = accent.gamma_multiply(0.6);
    visuals.selection.stroke = Stroke::new(1.0, accent);
    visuals.hyperlink_color = accent;
    ctx.set_visuals(visuals);
}

/// The cover's accent: the strongest of its colourful hues (weighed by how
/// colourful and how bright), made readable on the window as text (WCAG AA,
/// 4.5:1); none for a grey cover.
pub fn accent_of(art: &crate::engine::Art) -> Option<Color32> {
    const BUCKETS: usize = 24;
    let mut weight = [0.0f64; BUCKETS];
    let mut sum = [[0.0f64; 3]; BUCKETS];
    let mut all = 0.0f64;
    for px in art.rgba.as_chunks::<4>().0.iter().step_by(3) {
        let (r, g, b) = (
            px[0] as f64 / 255.0,
            px[1] as f64 / 255.0,
            px[2] as f64 / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        all += 1.0;
        let s = if max > 0.0 { (max - min) / max } else { 0.0 };
        if s < 0.25 || max < 0.2 {
            continue;
        }
        let d = max - min;
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
        let k = ((h * BUCKETS as f64) as usize).min(BUCKETS - 1);
        let w = s * max;
        weight[k] += w;
        for (c, v) in sum[k].iter_mut().zip([r, g, b]) {
            *c += v * w;
        }
    }
    let (k, w) = weight
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    // (a few colourful pixels in a grey cover are no accent)
    if *w < all * 0.02 {
        return None;
    }
    let to = |v: f64| (v / w * 255.0).round().clamp(0.0, 255.0) as u8;
    let mut c = Color32::from_rgb(to(sum[k][0]), to(sum[k][1]), to(sum[k][2]));
    let ground = Color32::from_rgb(0x0d, 0x11, 0x17);
    for _ in 0..20 {
        if viz::contrast(c, ground) >= 4.5 {
            break;
        }
        c = viz::mix(c, Color32::WHITE, 0.12);
    }
    Some(c)
}

/// White or near-black, whichever reads better on `fill`.
fn ink_on(fill: Color32) -> Color32 {
    let dark = Color32::from_rgb(0x0d, 0x11, 0x17);
    if viz::contrast(Color32::WHITE, fill) >= viz::contrast(dark, fill) {
        Color32::WHITE
    } else {
        dark
    }
}

/// The faces in their groups, to choose one (the mode box).
fn faces_list(ui: &mut Ui, mode: &mut &'static str) {
    let mut last = None;
    for face in viz::FACES {
        if last != Some(face.group) && face.group != viz::Group::Off {
            ui.label(RichText::new(face.group.name()).small().color(MUTED));
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
            }
        };
        let ground = Color32::from_rgb(0x0d, 0x11, 0x17);
        // a dark red cover: a red accent, lightened until it reads (4.5:1)
        let red = accent_of(&art([120, 10, 20], 6)).expect("an accent");
        assert!(red.r() > red.g() && red.r() > red.b(), "{red:?}");
        assert!(viz::contrast(red, ground) >= 4.5);
        // a grey cover: none (the window's own)
        assert_eq!(accent_of(&art([100, 100, 100], 10)), None);
        // the play button's mark reads on any accent
        for c in [red, Color32::from_rgb(250, 240, 120), ACCENT] {
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
        assert_eq!(band_name(16000.0), "16k");
        assert_eq!(band_name(62.0), "62");
    }
}
