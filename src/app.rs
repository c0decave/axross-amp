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
    loaded: Option<u64>,
    failures: usize,
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
    /// The output device chosen for the player ("" the default).
    output_choice: String,
    audio_path: PathBuf,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Vec<String>) -> Self {
        style(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let engine = Engine::start(move || ctx.request_repaint());
        let state_path = state::state_path();
        let audio_path = state::audio_path();
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
            loaded: None,
            failures: 0,
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
            output_choice: output,
            audio_path,
        };
        if !args.is_empty() {
            let first = app.playlist.tracks.len();
            app.add(&args);
            if app.playlist.tracks.len() > first {
                app.play_index(first, 0, true);
            }
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

    fn bluetooth_tab(&mut self, ui: &mut Ui) {
        ui.label("Bluetooth");
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

    fn add(&mut self, args: &[String]) {
        let (tracks, skipped) = playlist::collect(args);
        let count = self.playlist.add_tracks(tracks);
        self.status = match (count, skipped.len()) {
            (0, 0) => String::new(),
            (n, 0) => format!("{n} added"),
            (n, s) => format!("{n} added, {s} skipped (not audio)"),
        };
        self.dirty = true;
    }

    fn play_index(&mut self, index: usize, start_ms: u64, play: bool) {
        let Some(track) = self.playlist.tracks.get(index) else {
            return;
        };
        self.generation += 1;
        self.automatic = false;
        self.playlist.current = Some(index);
        self.selected = Some(index);
        self.info = TrackInfo::default();
        self.cover = None;
        self.duration = (track.duration_ms > 0).then(|| track.duration_ms as f64 / 1000.0);
        self.engine.send(Command::Load {
            path: track.path.clone(),
            start_ms,
            play,
            generation: self.generation,
        });
        self.dirty = true;
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
        } else if self.loaded.is_some() && self.loaded == Some(self.generation) {
            self.engine.send(Command::Play);
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
                self.engine.send(Command::Stop);
                self.status = "End of the playlist".into();
            }
        }
    }

    fn previous(&mut self) {
        // within the first seconds: the track before; later: this one from its start
        if self.engine.shared.position() > 3.0 {
            self.engine.send(Command::Seek(0.0));
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
                    self.loaded = Some(generation);
                    self.failures = 0;
                    self.duration = info.duration.or(self.duration);
                    if let Some(i) = self.playlist.current
                        && let Some(track) = self.playlist.tracks.get_mut(i)
                    {
                        if !info.title.is_empty() && track.title.is_empty() {
                            track.title = if info.artist.is_empty() {
                                info.title.clone()
                            } else {
                                format!("{} – {}", info.artist, info.title)
                            };
                        }
                        if let Some(d) = info.duration {
                            track.duration_ms = (d * 1000.0) as u64;
                        }
                    }
                    self.cover = info
                        .cover
                        .as_deref()
                        .and_then(|bytes| cover_texture(ctx, bytes));
                    self.info = info;
                    self.status.clear();
                    self.dirty = true;
                }
                Event::Ended { generation } if generation == self.generation => self.next(true),
                Event::Failed {
                    generation,
                    message,
                } if generation == self.generation => {
                    let name = self
                        .playlist
                        .current
                        .and_then(|i| self.playlist.tracks.get(i))
                        .map(|t| playlist::default_title(&t.path))
                        .unwrap_or_default();
                    self.status = format!("Cannot play {name}: {message}");
                    self.failures += 1;
                    // playing on its own: the next one - unless every track failed in a row
                    if self.automatic && self.failures < self.playlist.tracks.len() {
                        self.next(true);
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

    fn keys(&mut self, ui: &Ui) {
        if ui.ctx().memory(|m| m.focused().is_some()) && self.url_open {
            return; // (typing a URL)
        }
        let (space, left, right, up, down, m, n, p, v) = ui.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::M),
                i.key_pressed(Key::N),
                i.key_pressed(Key::P),
                i.key_pressed(Key::V),
            )
        });
        let pos = self.engine.shared.position();
        if space {
            self.toggle_play();
        }
        if left {
            self.engine.send(Command::Seek((pos - 5.0).max(0.0)));
        }
        if right {
            self.engine.send(Command::Seek(pos + 5.0));
        }
        if up {
            self.set_volume(self.volume + 5);
        }
        if down {
            self.set_volume(self.volume.saturating_sub(5));
        }
        if m {
            self.set_muted(!self.muted);
        }
        if n {
            self.next(false);
        }
        if p {
            self.previous();
        }
        if v {
            self.mode = viz::next(self.mode);
            self.dirty = true;
        }
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
                        let track = self
                            .playlist
                            .current
                            .and_then(|i| self.playlist.tracks.get(i));
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
                        ui.add(
                            egui::Label::new(RichText::new(title).size(18.0).strong()).truncate(),
                        );
                        let by = [self.info.artist.as_str(), self.info.album.as_str()]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" · ");
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
                let response = ui.add_enabled(seekable, slider).accessible_name("Position");
                if response.dragged() || response.changed() {
                    self.seek_drag = Some(value);
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    self.engine.send(Command::Seek(value));
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
                self.engine.send(Command::Stop);
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
                if ui
                    .add(
                        egui::Slider::new(&mut volume, 0..=100)
                            .show_value(false)
                            .trailing_fill(true),
                    )
                    .accessible_name("Volume")
                    .on_hover_text(format!("Volume {volume} % (Up / Down)"))
                    .changed()
                {
                    self.set_volume(volume);
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
            let count = self.playlist.tracks.len();
            let mut play: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let mut moved: Option<(usize, usize)> = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height().max(120.0))
                .show_rows(ui, row, count, |ui, range| {
                    for i in range {
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
                        let colour = if current {
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
            if ui.input(|inp| inp.key_pressed(Key::Delete))
                && let Some(i) = self.selected
            {
                self.playlist.remove(i);
                self.selected = None;
                self.dirty = true;
            }
            if ui.input(|inp| inp.key_pressed(Key::Enter))
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
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.url)
                        .hint_text("https://…")
                        .desired_width(360.0),
                );
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
                    self.add(&args);
                    self.picking = Picking::Nothing;
                }
            }
            Picking::Folder => {
                if let Some(path) = self.dialog.take_picked() {
                    self.add(&[path.to_string_lossy().into_owned()]);
                    self.picking = Picking::Nothing;
                }
            }
            Picking::SavePlaylist => {
                if let Some(mut path) = self.dialog.take_picked() {
                    if path.extension().is_none() {
                        path.set_extension("m3u");
                    }
                    self.status =
                        match std::fs::write(&path, playlist::to_m3u(&self.playlist.tracks)) {
                            Ok(()) => format!("Saved {}", path.display()),
                            Err(e) => format!("Cannot save {}: {e}", path.display()),
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
            let first = self.playlist.tracks.len();
            self.add(&dropped);
            if !self.playing() && self.playlist.tracks.len() > first {
                self.play_index(first, 0, true);
            }
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
                    ui.label(RichText::new(&self.status).small().color(MUTED));
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
    }
}

impl Drop for App {
    /// The last changes are kept also when the window ends without
    /// `on_exit` (a test harness, a platform that skips it).
    fn drop(&mut self) {
        self.save(true);
    }
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
    fn clocks() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(61.9), "1:01");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(band_name(16000.0), "16k");
        assert_eq!(band_name(62.0), "62");
    }
}
