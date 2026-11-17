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
use std::time::{Duration, Instant};

const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
const MUTED: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
const PANEL: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const SAVE_EVERY: Duration = Duration::from_secs(3);
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
    analyzer: Analyzer,
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
    playlist_open: bool,
    big_visual: bool,
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
    url: String,
    url_open: bool,
    dialog: FileDialog,
    picking: Picking,
    selected: Option<usize>,
    dragging: Option<usize>,
    dirty: bool,
    eq_dirty: bool,
    last_save: Instant,
    last_frame: Instant,
    state_path: PathBuf,
    tab: Tab,
    mixer_page: crate::mixer_ui::MixerPage,
    #[cfg(target_os = "linux")]
    bluetooth_page: crate::bluetooth_ui::BluetoothPage,
    /// The output device chosen for the player ("" the default).
    output_choice: String,
    audio_path: PathBuf,
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
type Scanned = Result<(String, u64), String>;

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
                    let result = crate::engine::probe(&path).map(|info| {
                        let title = match (info.artist.is_empty(), info.title.is_empty()) {
                            (_, true) => String::new(),
                            (true, false) => info.title.clone(),
                            (false, false) => format!("{} – {}", info.artist, info.title),
                        };
                        let ms = info.duration.map(|d| (d * 1000.0) as u64).unwrap_or(0);
                        (title, ms)
                    });
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
            if !playlist::is_stream(&t.path) && (t.title.is_empty() || t.duration_ms == 0) {
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
        let mut app = App {
            engine,
            playlist,
            info: TrackInfo::default(),
            cover: None,
            analyzer: Analyzer::new(),
            viz_state: VizState::default(),
            tap: Vec::new(),
            mode: viz::known(&saved.visualizer),
            scheme_key: saved.visual_scheme.clone(),
            custom_colours: saved.visual_custom.clone(),
            options: saved.visual_options.clone(),
            volume: saved.volume,
            muted: saved.muted,
            eq: eq_settings,
            eq_open: saved.equalizer_open,
            playlist_open: saved.playlist_open,
            big_visual: false,
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
            dragging: None,
            dirty: false,
            eq_dirty: false,
            last_save: Instant::now(),
            last_frame: Instant::now(),
            state_path,
            tab: Tab::Player,
            mixer_page: Default::default(),
            #[cfg(target_os = "linux")]
            bluetooth_page: Default::default(),
            output_choice: output,
            audio_path,
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
        };
        #[cfg(target_os = "linux")]
        if setup.desktop {
            let ctx = cc.egui_ctx.clone();
            app.mpris = Some(crate::mpris::Mpris::start(
                setup.desktop_bus.clone(),
                app.engine.shared.clone(),
                move || ctx.request_repaint(),
            ));
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
                if let Err(e) = state::write_output(&self.audio_path, &id) {
                    self.status = format!("Cannot save the output: {e}");
                }
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
        let offered = collected.tracks.len();
        let count = self.playlist.add_tracks(collected.tracks);
        self.scanner.scan(&self.playlist.tracks[first..]);
        let mut status = match (count, collected.skipped.len()) {
            (0, 0) if offered == 0 => "Nothing to add.".to_owned(),
            (n, 0) => format!("{n} added"),
            (n, s) => format!("{n} added, {s} skipped (not audio)"),
        };
        if collected.truncated || count < offered {
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
        }
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
                Some(Ok((title, ms))) => {
                    if track.title.is_empty() && !title.is_empty() {
                        track.title = title.clone();
                    }
                    if track.duration_ms == 0 {
                        track.duration_ms = *ms;
                    }
                }
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
        self.selected = Some(index);
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
        let want = if self.loaded == Some(self.generation) && !self.current_is_stream() {
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
        self.opening = false;
        self.failures = 0;
        self.duration = info.duration.or(self.duration);
        if let Some(i) = self.playlist.current
            && let Some(track) = self.playlist.tracks.get_mut(i)
        {
            self.broken.remove(&track.path);
            if track.title.is_empty() {
                if !info.title.is_empty() {
                    track.title = if info.artist.is_empty() {
                        info.title.clone()
                    } else {
                        format!("{} – {}", info.artist, info.title)
                    };
                } else if !info.station.is_empty() {
                    track.title = info.station.clone();
                }
            }
            if let Some(d) = info.duration {
                track.duration_ms = (d * 1000.0) as u64;
            }
        }
        self.cover = info
            .cover
            .as_deref()
            .and_then(|bytes| cover_texture(ctx, bytes));
        self.art_url.clear();
        #[cfg(target_os = "linux")]
        if self.mpris.is_some()
            && let Some(bytes) = info.cover.as_deref()
        {
            self.art_url = crate::mpris::write_cover(bytes).unwrap_or_default();
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
        // within the first seconds: the track before; later: this one from its start
        if self.engine.shared.position() > 3.0 {
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
                            self.selected = Some(index); // (the selection follows the music)
                        }
                    }
                    self.duration = None;
                    self.stream_title.clear();
                    self.take_info(ctx, info);
                }
                Event::Ended { generation } if generation == self.generation => self.next(true),
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

    fn save(&mut self, force: bool) {
        if !(force || (self.dirty && self.last_save.elapsed() >= SAVE_EVERY)) && !self.eq_dirty {
            return;
        }
        if self.dirty || force {
            let state = self.snapshot();
            if let Err(e) = state::write_state(&self.state_path, &state) {
                self.status = format!("Cannot save the player state: {e}");
            }
            self.dirty = false;
            self.last_save = Instant::now();
        }
        if self.eq_dirty {
            if let Err(e) = state::write_eq(&self.audio_path, &self.eq) {
                self.status = format!("Cannot save the equalizer: {e}");
            }
            self.eq_dirty = false;
        }
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
                let id = ui.make_persistent_id("playlist");
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
            self.mode = viz::next(self.mode);
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
        self.selected = Some(rows[next]);
        self.scroll_to = Some(rows[next]);
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
            self.selected = Some(to);
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
                    .color(ACCENT)
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
                    .show_ui(ui, |ui| {
                        for (key, name) in viz::MODES {
                            ui.selectable_value(&mut mode, key, name);
                        }
                    })
                    .response
                    .accessible_name("Visualization");
                if mode != self.mode {
                    self.mode = mode;
                    self.dirty = true;
                }
            });
        });
    }

    fn visual_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Colours").strong());
        for (key, name, _) in
            viz::SCHEMES
                .iter()
                .chain(std::iter::once(&("custom", "Custom", ["#000000"; 5])))
        {
            if ui.radio(self.scheme_key == *key, *name).clicked() {
                self.scheme_key = (*key).to_owned();
                self.dirty = true;
            }
        }
        ui.separator();
        let mut o = self.options.clone();
        ui.add(egui::Slider::new(&mut o.bars, 8..=128).text("Bars"));
        ui.add(egui::Slider::new(&mut o.smoothing, 0..=95).text("Smoothing %"));
        ui.add(egui::Slider::new(&mut o.peak_fall, 5..=300).text("Peak fall %/s"));
        ui.add(egui::Slider::new(&mut o.fps, 10..=60).text("Frame rate cap"));
        if o != self.options {
            self.options = o.clamped();
            self.dirty = true;
        }
    }

    fn now_playing(&mut self, ui: &mut Ui) {
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let size = vec2(72.0, 72.0);
                    if let Some(texture) = &self.cover {
                        ui.add(
                            egui::Image::new(texture)
                                .fit_to_exact_size(size)
                                .corner_radius(CornerRadius::same(6)),
                        );
                    } else {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 6.0, Color32::from_rgb(0x22, 0x2a, 0x35));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "♫",
                            egui::FontId::proportional(32.0),
                            MUTED,
                        );
                    }
                    ui.vertical(|ui| {
                        let (title, by) = self.now_playing_text();
                        ui.add(
                            egui::Label::new(RichText::new(title).size(18.0).strong()).truncate(),
                        );
                        if !by.is_empty() {
                            ui.add(egui::Label::new(RichText::new(by).color(MUTED)).truncate());
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

    /// The window's title (tests).
    pub fn title(&self) -> &str {
        &self.window_title
    }

    /// Why the track at `path` could not be played, if it could not.
    pub fn broken(&self, path: &str) -> Option<&str> {
        self.broken.get(path).map(String::as_str)
    }

    fn visual(&mut self, ui: &mut Ui, height: f32) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let scheme = viz::scheme(&self.scheme_key, &self.custom_colours);
        let dt = self.last_frame.elapsed().as_secs_f32().clamp(0.0, 0.2);
        self.last_frame = Instant::now();
        if let Ok(tap) = self.engine.shared.tap.lock() {
            self.tap.clear();
            self.tap.extend(tap.iter().copied());
        }
        let rate = self
            .engine
            .shared
            .device_rate
            .load(std::sync::atomic::Ordering::Relaxed);
        self.analyzer
            .update(&self.tap, rate, &self.options, dt, self.playing());
        viz::draw(
            ui.painter(),
            rect,
            &self.analyzer,
            &scheme,
            self.mode,
            &mut self.viz_state,
            dt,
        );
        let now = ui.input(|i| i.time);
        let wait = ui.ctx().options(|o| o.input_options.max_double_click_delay);
        if response.double_clicked() {
            self.pending_click = None;
            self.big_visual = !self.big_visual;
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
        response.on_hover_text(format!(
            "{} - click: next face, double-click: larger (V)",
            viz::name(self.mode)
        ));
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
            let big = vec2(46.0, 34.0);
            if ui
                .add(egui::Button::new(RichText::new("⏮").size(18.0)).min_size(big))
                .accessible_name("Previous")
                .on_hover_text("Previous (P)")
                .clicked()
            {
                self.previous();
            }
            let play = if self.playing() { "⏸" } else { "▶" };
            let button = egui::Button::new(RichText::new(play).size(22.0).color(Color32::WHITE))
                .fill(ACCENT)
                .min_size(vec2(58.0, 38.0))
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
            ui.add_space(6.0);
            let shuffle = self.playlist.shuffle;
            if ui
                .add(
                    egui::Button::new(RichText::new("🔀").size(16.0).color(if shuffle {
                        ACCENT
                    } else {
                        MUTED
                    }))
                    .selected(shuffle),
                )
                .accessible_name("Shuffle")
                .on_hover_text("Shuffle")
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
                        ACCENT
                    } else {
                        MUTED
                    }))
                    .selected(on),
                )
                .accessible_name("Repeat")
                .on_hover_text(tip)
                .clicked()
            {
                self.playlist.repeat = self.playlist.repeat.next();
                self.dirty = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut volume = self.volume;
                ui.spacing_mut().slider_width = 110.0;
                let response = ui
                    .add(
                        egui::Slider::new(&mut volume, 0..=100)
                            .show_value(false)
                            .trailing_fill(true),
                    )
                    .accessible_name("Volume")
                    .on_hover_text(format!("Volume {volume} % (↑/↓, +/−, the wheel)"));
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
            });
        });
    }

    fn equalizer(&mut self, ui: &mut Ui) {
        let header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id("eq"),
            self.eq_open,
        );
        let open_before = header.is_open();
        let state = header.show_header(ui, |ui| {
            ui.label(RichText::new("Equalizer").strong());
            let mut enabled = self.eq.enabled;
            if ui
                .checkbox(&mut enabled, "")
                .on_hover_text("On / off")
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
        let open = state.is_open();
        state.body(|ui| {
            ui.add_enabled_ui(self.eq.enabled, |ui| {
                ui.horizontal(|ui| {
                    let mut gains = self.eq.gains;
                    let mut changed = false;
                    let width = ((ui.available_width() - 9.0 * ui.spacing().item_spacing.x) / 10.0)
                        .max(28.0);
                    for (i, band) in eq::BANDS.iter().enumerate() {
                        ui.allocate_ui(vec2(width, 150.0), |ui| {
                            ui.vertical_centered(|ui| {
                                ui.spacing_mut().slider_width = 110.0;
                                changed |= ui
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
                                    .on_hover_text(format!(
                                        "{} {:+.1} dB",
                                        band_name(*band),
                                        gains[i]
                                    ))
                                    .changed();
                                ui.label(RichText::new(band_name(*band)).small().color(MUTED));
                            });
                        });
                    }
                    if changed {
                        self.set_eq(gains, None);
                    }
                });
            });
        });
        if open != open_before || open != self.eq_open {
            self.eq_open = open;
            self.dirty = true;
        }
    }

    fn playlist_view(&mut self, ui: &mut Ui) {
        let total: u64 = self.playlist.tracks.iter().map(|t| t.duration_ms).sum();
        let header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id("playlist"),
            self.playlist_open,
        );
        let state = header.show_header(ui, |ui| {
            ui.label(RichText::new("Playlist").strong());
            ui.label(
                RichText::new(format!(
                    "{} {} · {}",
                    self.playlist.tracks.len(),
                    if self.playlist.tracks.len() == 1 {
                        "track"
                    } else {
                        "tracks"
                    },
                    clock(total as f64 / 1000.0)
                ))
                .color(MUTED)
                .small(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.playlist.clear();
                    self.selected = None;
                    self.dirty = true;
                }
                if ui
                    .small_button("Save…")
                    .on_hover_text("Save as an M3U playlist")
                    .clicked()
                {
                    self.picking = Picking::SavePlaylist;
                    self.dialog.save_file();
                }
            });
        });
        let open = state.is_open();
        state.body(|ui| {
            let row = 24.0;
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
                            self.selected = self.playlist.current;
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
            let mut moved: Option<(usize, usize)> = None;
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
                    let selected = self.selected == Some(i);
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
                            ACCENT.gamma_multiply(0.25)
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
                    let font = egui::FontId::proportional(14.0);
                    ui.painter().text(
                        rect.left_center() + vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{:>3}", i + 1),
                        egui::FontId::monospace(12.0),
                        MUTED,
                    );
                    let duration = if track.duration_ms > 0 {
                        clock(track.duration_ms as f64 / 1000.0)
                    } else if playlist::is_stream(&track.path) {
                        "📻".into()
                    } else {
                        String::new()
                    };
                    ui.painter().text(
                        rect.right_center() - vec2(8.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        duration,
                        egui::FontId::monospace(12.0),
                        MUTED,
                    );
                    let clip = egui::Rect::from_min_max(
                        rect.left_top() + vec2(44.0, 0.0),
                        rect.right_bottom() - vec2(64.0, 0.0),
                    );
                    ui.painter().with_clip_rect(clip).text(
                        clip.left_center(),
                        egui::Align2::LEFT_CENTER,
                        title,
                        font,
                        colour,
                    );
                    // the path, and why it could not be played (axross's tooltip)
                    let tip = match broken {
                        Some(why) => format!("{}\nCannot play: {why}", track.path),
                        None => track.path.clone(),
                    };
                    let response = response.on_hover_text(tip);
                    if response.clicked() {
                        self.selected = Some(i);
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
                        if ui.button("✖ Remove").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                }
            });
            if ui.input(|inp| inp.pointer.any_released()) {
                self.dragging = None;
            }
            if let Some((from, to)) = moved {
                self.playlist.move_track(from, to);
                self.selected = Some(to);
                self.dirty = true;
            }
            if let Some(i) = remove {
                self.playlist.remove(i);
                self.dirty = true;
            }
            if let Some(i) = play {
                self.play_index(i, 0, true);
            }
            if self.gate.any
                && ui.input(|inp| inp.key_pressed(Key::Delete))
                && let Some(i) = self.selected
            {
                self.playlist.remove(i);
                self.selected = None;
                self.dirty = true;
            }
            if self.gate.activation
                && ui.input(|inp| inp.key_pressed(Key::Enter))
                && let Some(i) = self.selected
            {
                self.play_index(i, 0, true);
            }
        });
        if open != self.playlist_open {
            self.playlist_open = open;
            self.dirty = true;
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
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events(&ctx);
        self.poll_adding();
        self.poll_scanner();
        #[cfg(target_os = "linux")]
        self.desktop(&ctx);
        self.sync_queue();
        self.window_title(&ctx);
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
                        &self.status
                    } else {
                        &self.notice
                    };
                    ui.add(egui::Label::new(RichText::new(line).small().color(MUTED)).truncate());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&self.output).small().color(MUTED));
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
                if self.big_visual {
                    let h = ui.available_height() - 90.0;
                    self.visual(ui, h.max(120.0));
                    ui.add_space(6.0);
                    self.position(ui);
                    self.transport(ui);
                    return;
                }
                self.now_playing(ui);
                ui.add_space(8.0);
                self.visual(ui, 190.0);
                ui.add_space(6.0);
                self.position(ui);
                self.transport(ui);
                ui.add_space(4.0);
                ui.separator();
                self.equalizer(ui);
                ui.separator();
                self.playlist_view(ui);
            });
        self.url_window(&ctx);
        self.file_dialog(&ctx);
        self.save(false);
        let fps = self.options.fps.clamp(10, 60) as f32;
        if self.playing()
            || self.analyzer.bars.iter().any(|b| *b > 0.001)
            || self.mode == "starfield"
        {
            ctx.request_repaint_after(Duration::from_secs_f32(1.0 / fps));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self) {
        self.save(true);
        #[cfg(target_os = "linux")]
        crate::mpris::remove_covers();
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

/// The slider's volume (0-100) as a gain: a curve, so the middle of the
/// slider sounds like the middle.
fn volume_curve(volume: u32) -> f32 {
    (volume.min(100) as f32 / 100.0).powf(1.6)
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

fn cover_texture(ctx: &egui::Context, bytes: &[u8]) -> Option<TextureHandle> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.thumbnail(256, 256).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let colour = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture("cover", colour, egui::TextureOptions::LINEAR))
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn clocks() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(61.9), "1:01");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(band_name(16000.0), "16k");
        assert_eq!(band_name(62.0), "62");
    }
}
