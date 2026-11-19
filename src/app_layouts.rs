//! The designs' layouts: where the player's parts go in each design, in
//! what sizes, drawn how - the same player (its state, its logic, every
//! control with its name) arranged as a modern card page, a zen column, a
//! hi-fi rack of units, a 90s desktop's main strip with its panels, a
//! cassette deck, a terminal's boxes, a brutalist grid or a collage.
//!
//! Every layout adds its controls in the order they are seen - row by row,
//! left to right - so Tab goes through them as the eye does (a group at a
//! row's right end is placed by its width measured the frame before, not
//! laid out right to left).

use super::*;
use crate::design::{self, KeyLook};
use egui::{Pos2, Rect, Shape, pos2};

/// The transport keys' sizes and words.
#[derive(Clone, Copy)]
pub(super) struct Keys {
    pub look: KeyLook,
    pub size: egui::Vec2,
    pub play: egui::Vec2,
    pub gap: f32,
    /// Shuffle and repeat in the same row, after a gap.
    pub modes: bool,
}

impl App {
    // -- the parts ------------------------------------------------------------------------

    /// The six transport keys (previous, 15 s back, play / pause, stop,
    /// 15 s on, next) and, where `k.modes`, shuffle and repeat, as the
    /// design draws them.
    pub(super) fn transport_keys(&mut self, ui: &mut Ui, k: Keys) {
        ui.spacing_mut().item_spacing.x = k.gap;
        let can = self.can_seek();
        let playing = self.playing();
        if design::key(ui, k.look, "⏮", "prev", "Previous", k.size, false, false)
            .on_hover_text("Previous (P)")
            .clicked()
        {
            self.previous();
        }
        let r = ui
            .add_enabled_ui(can, |ui| {
                design::key(
                    ui,
                    k.look,
                    "↺15",
                    "−15 s",
                    "Back 15 s",
                    k.size,
                    false,
                    false,
                )
            })
            .inner
            .on_hover_text("Back 15 seconds (J)");
        if r.clicked() {
            self.wind(-15.0);
        }
        let (icon, word, name) = if playing {
            ("⏸", "pause", "Pause")
        } else {
            ("▶", "play", "Play")
        };
        if design::key(ui, k.look, icon, word, name, k.play, playing, true)
            .on_hover_text("Play / pause (Space)")
            .clicked()
        {
            self.toggle_play();
        }
        if design::key(ui, k.look, "⏹", "stop", "Stop", k.size, false, false)
            .on_hover_text("Stop")
            .clicked()
        {
            self.stop();
        }
        let r = ui
            .add_enabled_ui(can, |ui| {
                design::key(
                    ui,
                    k.look,
                    "15↻",
                    "+15 s",
                    "Forward 15 s",
                    k.size,
                    false,
                    false,
                )
            })
            .inner
            .on_hover_text("Forward 15 seconds (L)");
        if r.clicked() {
            self.wind(15.0);
        }
        if design::key(ui, k.look, "⏭", "next", "Next", k.size, false, false)
            .on_hover_text("Next (N)")
            .clicked()
        {
            self.next(false);
        }
        if k.modes {
            ui.add_space(k.gap * 2.0);
            self.mode_keys(
                ui,
                k.look,
                egui::vec2(k.size.y.min(40.0), k.size.y.min(40.0)),
            );
        }
    }

    /// Shuffle and repeat (their state in their names, not by colour only).
    pub(super) fn mode_keys(&mut self, ui: &mut Ui, look: KeyLook, size: egui::Vec2) {
        let shuffle = self.playlist.shuffle;
        let name = if shuffle {
            "Shuffle: on"
        } else {
            "Shuffle: off"
        };
        if design::key(ui, look, "🔀", "shuf", name, size, shuffle, false)
            .on_hover_text(name)
            .clicked()
        {
            self.playlist.set_shuffle(!shuffle);
            self.dirty = true;
        }
        let (icon, name) = match self.playlist.repeat {
            Repeat::Off => ("🔁", "Repeat: off"),
            Repeat::All => ("🔁", "Repeat: all"),
            Repeat::One => ("🔂", "Repeat: this track"),
        };
        let on = self.playlist.repeat != Repeat::Off;
        if design::key(ui, look, icon, "rpt", name, size, on, false)
            .on_hover_text(name)
            .clicked()
        {
            self.playlist.repeat = self.playlist.repeat.next();
            self.dirty = true;
        }
    }

    /// The volume (a slider, or a knob of `knob` points), mute and the ⋯
    /// menu, left to right; the sleep timer's words after them.
    pub(super) fn volume_group(&mut self, ui: &mut Ui, knob: Option<f32>, slider: f32) {
        // (left to right as they are seen: the sleep timer's words, ⋯,
        // mute, the volume at the end)
        if let Some(label) = self.sleep_label() {
            ui.label(RichText::new(label).small().color(self.accent));
        }
        egui::containers::menu::MenuButton::from_button(
            egui::Button::new(RichText::new("⋯").size(16.0)).min_size(vec2(24.0, 24.0)),
        )
        .ui(ui, |ui| self.more_menu(ui))
        .0
        .accessible_name("More")
        .on_hover_text("Sleep timer, ReplayGain, track details, mixer");
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
        let mut volume = self.volume;
        let response = match knob {
            Some(side) => design::knob(ui, &mut volume, "Volume", side),
            None => {
                ui.spacing_mut().slider_width = slider;
                ui.add(
                    egui::Slider::new(&mut volume, 0..=100)
                        .show_value(false)
                        .trailing_fill(true),
                )
            }
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
    }

    /// The playlist or the equalizer, under their switch.
    pub(super) fn lower(&mut self, ui: &mut Ui) {
        self.lower_switch(ui);
        if self.eq_page {
            self.equalizer_page(ui);
        } else if self.playlist_open {
            self.playlist_panel(ui);
        }
    }

    /// The cover in `rect` (or, without one, a note on the accent's
    /// shade), its corners `radius`.
    pub(super) fn cover_in(&mut self, ui: &mut Ui, rect: Rect, radius: u8) {
        let px = rect.width().max(rect.height()) * ui.ctx().pixels_per_point();
        if let Some(texture) = self.sizer.get(px).or(self.cover.as_ref()).cloned() {
            egui::Image::new(&texture)
                .corner_radius(CornerRadius::same(radius))
                .paint_at(ui, rect);
        } else {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(radius),
                viz::mix(th().surface, self.accent, 0.3),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "♫",
                egui::FontId::proportional((rect.height() * 0.3).clamp(14.0, 64.0)),
                viz::mix(th().muted, th().strong, 0.4),
            );
        }
    }

    /// The picture in `rect` (as `visual` draws it, in a rect of its own).
    pub(super) fn visual_in(&mut self, ui: &mut Ui, rect: Rect) {
        child(ui, egui::UiBuilder::new().max_rect(rect), |ui| {
            self.visual(ui, rect.height());
        });
    }

    /// The position and the length, as "1:23 / 4:05".
    pub(super) fn times(&mut self) -> String {
        let pos = self.seek_drag.unwrap_or(self.shown_position());
        match self.duration {
            Some(total) if total > 0.0 => format!("{} / {}", clock(pos), clock(total)),
            _ if self.current_is_stream() => format!("{} / live", clock(pos)),
            _ => format!("{} / --:--", clock(pos)),
        }
    }

    // -- the layouts ----------------------------------------------------------------------

    /// The player's page in the design shown.
    pub(super) fn design_page(&mut self, ui: &mut Ui) {
        let key = design::current().key;
        // (wide and low - half a laptop's screen tall: the player at the
        // left, the playlist beside it, not squeezed under it)
        let room = ui.available_rect_before_wrap();
        if room.height() < 440.0 && room.width() >= 800.0 && matches!(key, "cassette" | "brutalist")
        {
            ui.allocate_rect(room, Sense::hover());
            // (the playlist's row of switches as wide as it needs: Save…
            // and Clear in the brutalist's large type)
            let share = if key == "brutalist" { 0.46 } else { 0.56 };
            let left = Rect::from_min_size(room.min, vec2(room.width() * share, room.height()));
            let right = Rect::from_min_max(pos2(left.right() + 12.0, room.top()), room.max);
            self.list_elsewhere = true;
            child(ui, egui::UiBuilder::new().max_rect(left), |ui| match key {
                "cassette" => self.layout_cassette(ui),
                _ => self.layout_brutalist(ui),
            });
            self.list_elsewhere = false;
            child(ui, egui::UiBuilder::new().max_rect(right), |ui| {
                if key == "cassette" {
                    self.cassette_card(ui)
                } else {
                    self.lower(ui)
                }
            });
            return;
        }
        match key {
            "zen" => self.layout_zen(ui),
            "rack" => self.layout_rack(ui),
            "desktop90" => self.layout_desktop90(ui),
            "cassette" => self.layout_cassette(ui),
            "terminal" => self.layout_terminal(ui),
            "brutalist" => self.layout_brutalist(ui),
            "collage" => self.layout_collage(ui),
            _ => self.layout_modern(ui),
        }
    }

    /// Zen: one column, the cover large in the middle and what plays under
    /// it, the picture a quiet band, the keys alone without frames.
    fn layout_zen(&mut self, ui: &mut Ui) {
        let all = ui.available_height();
        // (the cover as large as three rows of the playlist leave it)
        let side = (all - 520.0).min(ui.available_width() * 0.42).min(280.0);
        if side >= 96.0 {
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                self.cover_in(ui, rect, 14);
                ui.add_space(10.0);
                let (title, by) = self.now_playing_text();
                ui.add(
                    egui::Label::new(RichText::new(title).font(bold(26.0)).color(th().strong))
                        .truncate(),
                );
                if !by.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(by).size(15.0).color(th().muted)).truncate(),
                    );
                }
            });
            ui.add_space(8.0);
        } else {
            self.title_line(ui);
        }
        let band = (all * 0.09).clamp(28.0, 110.0);
        self.visual(ui, band);
        ui.add_space(6.0);
        self.position(ui);
        let quiet = KeyLook::Quiet;
        let narrow = ui.available_width() < 520.0;
        let k = Keys {
            look: quiet,
            size: if narrow {
                vec2(34.0, 34.0)
            } else {
                vec2(44.0, 40.0)
            },
            play: if narrow {
                vec2(44.0, 40.0)
            } else {
                vec2(60.0, 52.0)
            },
            gap: if narrow { 2.0 } else { 10.0 },
            modes: false,
        };
        if all < 440.0 {
            // (a low window: the keys, the switches and the volume in one
            // row - the playlist keeps its rows)
            let k = Keys {
                size: vec2(30.0, 32.0),
                play: vec2(36.0, 34.0),
                ..k
            };
            ui.horizontal(|ui| {
                self.transport_keys(ui, k);
                self.mode_keys(ui, quiet, vec2(30.0, 30.0));
                let slider = (ui.available_width() - 70.0).clamp(36.0, 110.0);
                self.volume_group(ui, None, slider);
            });
        } else {
            centred(ui, egui::Id::unique("zen-keys"), |ui| {
                self.transport_keys(ui, k)
            });
            centred(ui, egui::Id::unique("zen-modes"), |ui| {
                self.mode_keys(ui, quiet, vec2(32.0, 32.0));
                ui.add_space(12.0);
                self.volume_group(ui, None, 110.0);
            });
        }
        ui.add_space(6.0);
        self.lower(ui);
    }

    /// The hi-fi rack: units one above the other - the display unit (a
    /// fluorescent readout, the picture as its scope), the amplifier (VU
    /// meters, the volume knob, the switches), the transport (the keys, the
    /// tape's position), the changer (the playlist) - each a brushed panel
    /// with its screws and its name.
    fn layout_rack(&mut self, ui: &mut Ui) {
        let all = ui.available_height();
        let w = ui.available_width();
        let compact = all < 560.0;
        let t = th();
        // the display unit
        let display_h = if all < 440.0 {
            64.0
        } else if compact {
            (all * 0.26).clamp(78.0, 120.0)
        } else {
            (all * 0.27).clamp(130.0, 280.0)
        };
        let (unit, _) = ui.allocate_exact_size(vec2(w, display_h), Sense::hover());
        rack_unit(ui, unit, "DISPLAY", t);
        let inner = unit.shrink2(vec2(26.0, 18.0));
        let split = if w < 520.0 { 0.55 } else { 0.42 };
        let vfd_rect =
            Rect::from_min_size(inner.min, vec2(inner.width() * split - 6.0, inner.height()));
        let scope = Rect::from_min_max(pos2(vfd_rect.right() + 12.0, inner.top()), inner.max);
        let track = self.playlist.current.map_or(0, |i| i + 1);
        let (title, by) = self.now_playing_text();
        let big = (inner.height() * 0.3).clamp(14.0, 34.0);
        let times = self.times();
        let lines: Vec<(String, f32)> = if compact {
            vec![(format!("{track:02}  {times}"), big), (title.clone(), 12.0)]
        } else {
            vec![
                (format!("{track:02}  {times}"), big),
                (title.clone(), 14.0),
                (by.clone(), 12.0),
                (card_format(&self.info), 11.0),
            ]
        };
        let refs: Vec<(&str, f32)> = lines.iter().map(|(s, z)| (s.as_str(), *z)).collect();
        design::vfd(ui.painter(), vfd_rect, &refs, t);
        let readout = ui.interact(vfd_rect, ui.scope_id().with("vfd"), Sense::hover());
        let said = format!("{title} {by}, {times}");
        readout
            .widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, &said));
        self.visual_in(ui, scope);
        ui.add_space(4.0);
        if all < 440.0 {
            // (a low window: one unit for the amplifier and the transport,
            // its keys, switches, knob in one row - the changer keeps rows)
            let (unit, _) = ui.allocate_exact_size(vec2(w, 92.0), Sense::hover());
            rack_unit(ui, unit, "AMP · TRANSPORT", t);
            let inner = unit.shrink2(vec2(14.0, 12.0));
            child(ui, egui::UiBuilder::new().max_rect(inner), |ui| {
                self.position(ui);
                ui.horizontal(|ui| {
                    let kw = ((inner.width() - 170.0) / 6.3).clamp(26.0, 40.0);
                    let k = Keys {
                        look: KeyLook::Metal,
                        size: vec2(kw, 32.0),
                        play: vec2(kw * 1.3, 32.0),
                        gap: 2.0,
                        modes: false,
                    };
                    self.transport_keys(ui, k);
                    self.mode_keys(ui, KeyLook::Metal, vec2(28.0, 30.0));
                    self.volume_group(ui, Some(30.0), 60.0);
                });
            });
            ui.add_space(4.0);
            let rest = ui.available_rect_before_wrap();
            rack_unit(ui, rest, "CHANGER", t);
            child(
                ui,
                egui::UiBuilder::new().max_rect(rest.shrink2(vec2(14.0, 14.0))),
                |ui| self.lower(ui),
            );
            return;
        }
        // the amplifier
        let amp_h = if compact { 72.0 } else { 96.0 };
        let (unit, _) = ui.allocate_exact_size(vec2(w, amp_h), Sense::hover());
        rack_unit(ui, unit, "AMPLIFIER", t);
        let inner = unit.shrink2(vec2(26.0, 14.0));
        let meters = Rect::from_min_size(
            inner.min,
            vec2((inner.width() * 0.34).clamp(110.0, 300.0), inner.height()),
        );
        design::needles(ui.painter(), meters, self.analyzer.vu, t);
        let controls = Rect::from_min_max(pos2(meters.right() + 14.0, inner.top()), inner.max);
        child(
            ui,
            egui::UiBuilder::new()
                .max_rect(controls)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                self.mode_keys(ui, KeyLook::Metal, vec2(36.0, 30.0));
                ui.add_space(8.0);
                let knob = (inner.height() - 4.0).clamp(36.0, 64.0);
                self.volume_group(ui, Some(knob), 80.0);
            },
        );
        // (wide: the maker's plate at the unit's right, its lamp lit while
        // it plays - a still dot, never blinking)
        if inner.width() > 640.0 {
            let fill = design::list_fill(design::find("rack"), t);
            let ink = crate::theme::ink_on_in(t, fill);
            let plate = inner.right_center() - vec2(4.0, 0.0);
            ui.painter().text(
                plate - vec2(0.0, 8.0),
                egui::Align2::RIGHT_CENTER,
                "AXROSS  AMP-1",
                egui::FontId::new(15.0, egui::FontFamily::Name("bold".into())),
                ink,
            );
            ui.painter().text(
                plate + vec2(0.0, 12.0),
                egui::Align2::RIGHT_CENTER,
                "STEREO INTEGRATED AMPLIFIER",
                egui::FontId::proportional(9.0),
                ink,
            );
            let lamp = if self.playing() {
                self.accent
            } else {
                crate::theme::mix(fill, Color32::BLACK, 0.4)
            };
            ui.painter()
                .circle_filled(plate - vec2(150.0, 8.0), 3.5, lamp);
        }
        ui.add_space(4.0);
        // the transport
        let narrow = w < 560.0;
        let keys_h = if compact { 36.0 } else { 52.0 };
        let transport_h = keys_h + 52.0;
        let (unit, _) = ui.allocate_exact_size(vec2(w, transport_h), Sense::hover());
        rack_unit(ui, unit, "TRANSPORT", t);
        let inner = unit.shrink2(vec2(26.0, 12.0));
        child(ui, egui::UiBuilder::new().max_rect(inner), |ui| {
            self.position(ui);
            let kw = if narrow {
                ((inner.width() - 30.0) / 6.0).clamp(30.0, 64.0)
            } else {
                64.0
            };
            let k = Keys {
                look: KeyLook::Metal,
                size: vec2(kw, keys_h),
                play: vec2(kw * 1.2, keys_h),
                gap: 4.0,
                modes: false,
            };
            centred(ui, egui::Id::unique("rack-keys"), |ui| {
                self.transport_keys(ui, k)
            });
        });
        ui.add_space(4.0);
        // the changer: the playlist
        let rest = ui.available_rect_before_wrap();
        rack_unit(ui, rest, "CHANGER", t);
        child(
            ui,
            egui::UiBuilder::new().max_rect(rest.shrink2(vec2(22.0, 16.0))),
            |ui| {
                self.lower(ui);
            },
        );
    }

    /// The 90s desktop: a small main strip - its title bar, the time in
    /// large digits, the picture as a little spectrum, the track's words,
    /// tiny bevelled keys - and the playlist (or the equalizer) as a panel
    /// of its own under it, with its own title bar.
    fn layout_desktop90(&mut self, ui: &mut Ui) {
        let t = th();
        // (wide: the playlist docked at the strip's right, the picture as a
        // window of its own under the strip)
        let docked = ui.available_width() >= 1100.0;
        let width = ui.available_width().min(620.0);
        let x0 = ui.available_rect_before_wrap().left();
        let column = ui.available_rect_before_wrap();
        let all = ui.available_height();
        let compact = all < 520.0;
        // the main strip
        let strip_h = if compact { 112.0 } else { 150.0 };
        let (strip, _) = ui.allocate_exact_size(vec2(width, strip_h + 18.0), Sense::hover());
        let strip = Rect::from_min_size(pos2(x0, strip.top()), strip.size());
        retro_window(ui, strip, "axross-amp", t);
        let body = Rect::from_min_max(strip.min + vec2(6.0, 24.0), strip.max - vec2(6.0, 6.0));
        // the time, large, in its well; the spectrum beside it
        let lcd_w = (body.width() * 0.34).clamp(120.0, 200.0);
        let lcd = Rect::from_min_size(body.min, vec2(lcd_w, body.height() * 0.48));
        sunken(ui.painter(), lcd, t);
        let pos = self.seek_drag.unwrap_or(self.shown_position());
        design::lcd(
            ui.painter(),
            lcd.min + vec2(4.0, 4.0),
            &clock(pos),
            (lcd.height() * 0.5).clamp(14.0, 30.0),
            t,
        );
        // (beside the time: the state and the track's number, small)
        let state = if self.playing() {
            "▶ PLAY"
        } else {
            "‖ PAUSE"
        };
        let track = self.playlist.current.map_or(0, |i| i + 1);
        if lcd.width() >= 170.0 {
            ui.painter().text(
                lcd.right_top() + vec2(-6.0, 6.0),
                egui::Align2::RIGHT_TOP,
                state,
                egui::FontId::monospace(11.0),
                crate::theme::ink_on_in(t, t.raised),
            );
            ui.painter().text(
                lcd.right_bottom() + vec2(-6.0, -6.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("TRACK {track:02}"),
                egui::FontId::monospace(11.0),
                crate::theme::ink_on_in(t, t.raised),
            );
        }
        let spectrum = Rect::from_min_max(
            pos2(lcd.right() + 6.0, body.top()),
            pos2(body.right(), lcd.bottom()),
        );
        if docked {
            // (the picture has its window below: here the format, lit)
            let i = &self.info;
            let khz = i.rate as f32 / 1000.0;
            let lines = [
                (format!("{:>4} kbps", i.bitrate.unwrap_or(0)), 13.0),
                (
                    format!(
                        "{khz:>4.1} kHz  {}",
                        if i.channels >= 2 { "STEREO" } else { "MONO" }
                    ),
                    13.0,
                ),
            ];
            let refs: Vec<(&str, f32)> = lines.iter().map(|(s, z)| (s.as_str(), *z)).collect();
            design::vfd(ui.painter(), spectrum.shrink(2.0), &refs, t);
        } else {
            self.visual_in(ui, spectrum.shrink(2.0));
        }
        sunken(ui.painter(), spectrum, t);
        // the words: what plays, its format
        let words = Rect::from_min_max(
            pos2(body.left(), lcd.bottom() + 4.0),
            pos2(body.right(), lcd.bottom() + 22.0),
        );
        sunken(ui.painter(), words, t);
        let (title, by) = self.now_playing_text();
        let line = if by.is_empty() {
            title
        } else {
            format!("{title} - {by}")
        };
        let line = format!("{line}   ({})", card_format(&self.info));
        let marquee = ui.painter().with_clip_rect(words.shrink(2.0));
        let font = egui::FontId::monospace(12.0);
        let galley = marquee.layout_no_wrap(line.clone(), font, t.text);
        // (a marquee: the words scroll slowly where they do not fit - 24
        // points a second; still where the desktop asks for less motion)
        let overflow = galley.size().x - words.width() + 8.0;
        // (only while it plays: paused or stopped, the words stand and the
        // window is not woken for them)
        let shift = if overflow > 0.0 && self.playing() && !self.desktop_reduces_motion() {
            let period = (overflow / 24.0) as f64 + 4.0;
            let phase = (ui.input(|i| i.time) % period) as f32;
            ui.ctx().request_repaint_after(Duration::from_millis(80));
            (phase * 24.0 - 48.0).clamp(0.0, overflow)
        } else {
            0.0
        };
        marquee.galley(words.left_top() + vec2(4.0 - shift, 3.0), galley, t.text);
        let said = ui.interact(words, ui.scope_id().with("marquee"), Sense::hover());
        said.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, &line));
        // the keys and the volume, in a row; the position under them
        let rows = Rect::from_min_max(pos2(body.left(), words.bottom() + 4.0), body.max);
        child(ui, egui::UiBuilder::new().max_rect(rows), |ui| {
            ui.horizontal(|ui| {
                let k = Keys {
                    look: KeyLook::Bevel,
                    size: vec2(28.0, 24.0),
                    play: vec2(32.0, 24.0),
                    gap: 1.0,
                    modes: false,
                };
                self.transport_keys(ui, k);
                ui.add_space(4.0);
                self.mode_keys(ui, KeyLook::Bevel, vec2(26.0, 24.0));
                ui.add_space(4.0);
                let room = (ui.available_width() - 64.0).clamp(36.0, 100.0);
                self.volume_group(ui, None, room);
            });
            if !compact {
                self.position(ui);
            }
        });
        if compact {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            let row = Rect::from_min_size(pos2(x0, row.top()), vec2(width, 26.0));
            child(ui, egui::UiBuilder::new().max_rect(row), |ui| {
                self.position(ui)
            });
        }
        ui.add_space(4.0);
        // the panel under it - or, docked, the picture's window under it
        // and the playlist (or the equalizer) at the strip's right
        let rest = ui.available_rect_before_wrap();
        let title = if self.eq_page {
            "Equalizer"
        } else {
            "Playlist"
        };
        let panel = if docked {
            let pic = Rect::from_min_max(pos2(x0, rest.top()), pos2(x0 + width, rest.bottom()));
            retro_window(ui, pic, "Visualization", t);
            let inside = Rect::from_min_max(pic.min + vec2(6.0, 24.0), pic.max - vec2(6.0, 6.0));
            self.visual_in(ui, inside);
            sunken(ui.painter(), inside, t);
            Rect::from_min_max(
                pos2(x0 + width + 8.0, column.top()),
                pos2(column.right(), rest.bottom()),
            )
        } else {
            Rect::from_min_max(pos2(x0, rest.top()), pos2(x0 + width, rest.bottom()))
        };
        retro_window(ui, panel, title, t);
        child(
            ui,
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                panel.min + vec2(6.0, 24.0),
                panel.max - vec2(6.0, 4.0),
            )),
            |ui| self.lower(ui),
        );
    }

    /// The cassette deck: the cassette in the middle - its label with what
    /// plays, the picture in its window behind two large reels - the tape
    /// counter beside it, piano keys under it, the playlist on the
    /// cassette's card insert.
    fn layout_cassette(&mut self, ui: &mut Ui) {
        let t = th();
        let all = ui.available_height();
        let w = ui.available_width();
        let compact = all < 560.0;
        // (wide: the deck's right part its counter, its level meters, its
        // switches and volume - as a deck has them beside the cassette)
        let wide = w >= 900.0;
        let low = all < 440.0;
        let deck_h = if low {
            84.0
        } else if compact {
            (all * 0.36).clamp(110.0, 170.0)
        } else {
            (all * 0.42).min(w * 0.34).clamp(200.0, 380.0)
        };
        let (deck, _) = ui.allocate_exact_size(vec2(w, deck_h), Sense::hover());
        // the deck's face
        let face = crate::theme::mix(t.raised, Color32::BLACK, if t.dark { 0.2 } else { 0.05 });
        ui.painter().rect_filled(deck, 6.0, face);
        ui.painter().rect_stroke(
            deck,
            6.0,
            Stroke::new(1.0, t.border),
            egui::StrokeKind::Inside,
        );
        // the cassette: as wide as its proportions allow (100 x 63)
        let counter_w = if wide {
            (w * 0.3).clamp(220.0, 420.0)
        } else if w < 520.0 {
            0.0
        } else {
            120.0
        };
        let room = deck.shrink(10.0);
        let cw = (room.width() - counter_w - 10.0).min(room.height() * 1.59);
        let cassette = Rect::from_min_size(
            pos2(
                room.left() + (room.width() - counter_w - 10.0 - cw) / 2.0,
                room.top(),
            ),
            vec2(cw, room.height()),
        );
        let shell = crate::theme::mix(t.surface, Color32::BLACK, if t.dark { 0.35 } else { 0.1 });
        ui.painter().rect_filled(cassette, 10.0, shell);
        // the label: what plays
        let label = Rect::from_min_size(
            cassette.min + vec2(cassette.width() * 0.06, cassette.height() * 0.07),
            vec2(cassette.width() * 0.88, cassette.height() * 0.3),
        );
        let paper = crate::theme::mix(t.surface, Color32::WHITE, if t.dark { 0.08 } else { 0.6 });
        ui.painter().rect_filled(label, 3.0, paper);
        ui.painter().rect_filled(
            Rect::from_min_size(label.min, vec2(label.width(), 4.0)),
            0.0,
            self.accent,
        );
        let ink = crate::theme::ink_on_in(t, paper);
        let (title, by) = self.now_playing_text();
        let clip = ui.painter().with_clip_rect(label.shrink(4.0));
        let size = (label.height() * 0.36).clamp(11.0, 26.0);
        clip.text(
            label.left_top() + vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            &title,
            bold(size),
            ink,
        );
        clip.text(
            label.left_bottom() + vec2(8.0, -5.0),
            egui::Align2::LEFT_BOTTOM,
            &by,
            egui::FontId::proportional((size * 0.6).max(10.0)),
            ink,
        );
        let said = ui.interact(label, ui.scope_id().with("cassette-label"), Sense::hover());
        let words = format!("{title} {by}");
        said.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, &words));
        // its window: the picture, the reels over it
        let window = Rect::from_min_max(
            pos2(
                cassette.left() + cassette.width() * 0.14,
                label.bottom() + cassette.height() * 0.07,
            ),
            pos2(
                cassette.right() - cassette.width() * 0.14,
                cassette.bottom() - cassette.height() * 0.12,
            ),
        );
        self.visual_in(ui, window);
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if self.playing() && !self.desktop_reduces_motion() {
            self.reels_turn = (self.reels_turn + dt * 0.25).fract();
        }
        let r = (window.height() * 0.5).min(window.width() * 0.17);
        for (i, c) in [
            pos2(window.left() + r * 0.9, window.center().y),
            pos2(window.right() - r * 0.9, window.center().y),
        ]
        .into_iter()
        .enumerate()
        {
            // (the tape wound on: the left reel full at the start, the right one at the end)
            let total = self.duration.unwrap_or(0.0).max(1.0);
            let done = (self.shown_position() / total).clamp(0.0, 1.0) as f32;
            let wound = if i == 0 { 1.0 - done } else { done };
            reel(ui.painter(), c, r, 0.45 + 0.5 * wound, self.reels_turn, t);
        }
        ui.painter().rect_stroke(
            window,
            4.0,
            Stroke::new(2.0, shell),
            egui::StrokeKind::Inside,
        );
        // the counter
        if counter_w > 0.0 {
            let c = Rect::from_min_size(
                pos2(room.right() - counter_w, room.top() + 8.0),
                vec2(counter_w.min(160.0), 36.0),
            );
            let pos = self.shown_position();
            design::counter(ui.painter(), c, pos, t, self.desktop_reduces_motion());
            ui.painter().text(
                c.center_bottom() + vec2(0.0, 6.0),
                egui::Align2::CENTER_TOP,
                "COUNTER",
                egui::FontId::proportional(10.0),
                crate::theme::ink_on_in(t, face),
            );
            let said = ui.interact(c, ui.scope_id().with("counter"), Sense::hover());
            let at = clock(pos);
            said.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::accesskit::Role::Label,
                    true,
                    format!("Counter {at}"),
                )
            });
        }
        if wide {
            // the level meters, the switches and the volume
            let side =
                Rect::from_min_max(pos2(room.right() - counter_w, room.top() + 64.0), room.max);
            let meters = Rect::from_min_size(
                side.min,
                vec2(side.width(), (side.height() - 44.0).clamp(40.0, 90.0)),
            );
            design::needles(ui.painter(), meters, self.analyzer.vu, t);
            let row = Rect::from_min_max(pos2(side.left(), side.bottom() - 36.0), side.max);
            child(
                ui,
                egui::UiBuilder::new()
                    .max_rect(row)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    self.mode_keys(ui, KeyLook::Piano, vec2(36.0, 30.0));
                    ui.add_space(6.0);
                    self.volume_group(ui, None, (ui.available_width() - 70.0).clamp(40.0, 140.0));
                },
            );
        }
        ui.add_space(6.0);
        self.position(ui);
        // the piano keys
        let narrow = w < 560.0;
        let kh = if compact { 44.0 } else { 64.0 };
        let kw = ((w - 40.0) / 6.0).clamp(36.0, 92.0);
        let k = Keys {
            look: KeyLook::Piano,
            size: vec2(if narrow { kw } else { kw.min(84.0) }, kh),
            play: vec2(if narrow { kw } else { kw.min(84.0) * 1.15 }, kh),
            gap: 3.0,
            modes: false,
        };
        if low {
            // (a low window: the keys, the switches and the volume in one row)
            let k = Keys {
                size: vec2(36.0, 36.0),
                play: vec2(38.0, 36.0),
                gap: 2.0,
                ..k
            };
            ui.horizontal(|ui| {
                self.transport_keys(ui, k);
                self.mode_keys(ui, KeyLook::Piano, vec2(28.0, 30.0));
                let slider = (ui.available_width() - 70.0).clamp(30.0, 110.0);
                self.volume_group(ui, None, slider);
            });
        } else {
            centred(ui, egui::Id::unique("cassette-keys"), |ui| {
                self.transport_keys(ui, k)
            });
        }
        if !wide && !low {
            ui.horizontal(|ui| {
                self.mode_keys(ui, KeyLook::Piano, vec2(40.0, 30.0));
                ui.add_space(8.0);
                self.volume_group(ui, None, 110.0);
            });
        }
        ui.add_space(4.0);
        if self.list_elsewhere {
            return;
        }
        self.cassette_card(ui);
    }

    /// The cassette's card insert: the playlist.
    fn cassette_card(&mut self, ui: &mut Ui) {
        let t = th();
        let rest = ui.available_rect_before_wrap();
        let card = design::list_fill(design::current(), t);
        ui.painter().rect_filled(rest, 2.0, card);
        ui.painter().rect_filled(
            Rect::from_min_size(rest.min, vec2(8.0, rest.height())),
            0.0,
            self.accent,
        );
        child(
            ui,
            egui::UiBuilder::new()
                .max_rect(rest.shrink2(vec2(14.0, 6.0)).translate(vec2(4.0, 0.0))),
            |ui| self.lower(ui),
        );
    }

    /// The terminal: boxes of text drawn with lines, their names in their
    /// top edges; everything in the terminal's type; keys as `[ PLAY ]`;
    /// the position and the volume as bars of characters.
    fn layout_terminal(&mut self, ui: &mut Ui) {
        let t = th();
        let all = ui.available_height();
        let compact = all < 520.0;
        let w = ui.available_width();
        // now playing
        let (title, by) = self.now_playing_text();
        let lines = if compact { 2.0 } else { 4.0 };
        let (bx, _) = ui.allocate_exact_size(vec2(w, 22.0 + lines * 17.0), Sense::hover());
        tui_box(ui.painter(), bx, "now playing", t);
        child(
            ui,
            egui::UiBuilder::new().max_rect(bx.shrink2(vec2(12.0, 10.0))),
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("> {title}"))
                            .monospace()
                            .strong()
                            .color(t.strong),
                    )
                    .truncate(),
                );
                if !compact {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("  {by}"))
                                .monospace()
                                .color(self.accent),
                        )
                        .truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("  {}", card_format(&self.info)))
                                .monospace()
                                .color(t.muted),
                        )
                        .truncate(),
                    );
                }
                self.ascii_position(ui);
            },
        );
        // the picture
        let band = if all < 440.0 {
            30.0
        } else if compact {
            (all * 0.22).clamp(48.0, 120.0)
        } else {
            (all * 0.3).clamp(100.0, 420.0)
        };
        let (bx, _) = ui.allocate_exact_size(vec2(w, band + 20.0), Sense::hover());
        tui_box(ui.painter(), bx, crate::viz::name(self.mode), t);
        self.visual_in(ui, bx.shrink2(vec2(8.0, 10.0)));
        // the keys
        // (the box as tall as its keys wrapped in the width there is)
        let top = ui.cursor().min;
        let room = Rect::from_min_size(top + vec2(10.0, 11.0), vec2(w - 20.0, 400.0));
        let used = child(ui, egui::UiBuilder::new().max_rect(room), |ui| {
            ui.horizontal_wrapped(|ui| {
                let k = Keys {
                    look: KeyLook::Bracket,
                    size: vec2(if w < 520.0 { 40.0 } else { 74.0 }, 24.0),
                    play: vec2(if w < 520.0 { 40.0 } else { 84.0 }, 24.0),
                    gap: 2.0,
                    modes: false,
                };
                self.transport_keys(ui, k);
                ui.add_space(6.0);
                self.mode_keys(
                    ui,
                    KeyLook::Bracket,
                    vec2(if w < 520.0 { 40.0 } else { 64.0 }, 24.0),
                );
                ui.add_space(6.0);
                self.ascii_volume(ui);
                ui.add(
                    egui::Button::new(
                        RichText::new(if self.muted { "[MUTED]" } else { "[MUTE]" }).monospace(),
                    )
                    .frame(false)
                    .min_size(vec2(24.0, 24.0)),
                )
                .accessible_name("Mute")
                .on_hover_text("Mute (M)")
                .clicked()
                .then(|| self.set_muted(!self.muted));
                egui::containers::menu::MenuButton::from_button(
                    egui::Button::new(RichText::new("[...]").monospace())
                        .frame(false)
                        .min_size(vec2(24.0, 24.0)),
                )
                .ui(ui, |ui| self.more_menu(ui))
                .0
                .accessible_name("More");
            });
            ui.min_rect()
        });
        let bx = Rect::from_min_size(top, vec2(w, used.height() + 22.0));
        ui.allocate_rect(bx, Sense::hover());
        tui_box(ui.painter(), bx, "keys", t);
        // the playlist
        let rest = ui.available_rect_before_wrap();
        tui_box(
            ui.painter(),
            rest,
            if self.eq_page {
                "equalizer"
            } else {
                "playlist"
            },
            t,
        );
        child(
            ui,
            egui::UiBuilder::new().max_rect(rest.shrink2(vec2(10.0, 10.0))),
            |ui| self.lower(ui),
        );
    }

    /// The brutalist: a hard asymmetric grid - the title huge in the wide
    /// left column, the cover square in the narrow right one, the keys as
    /// heavy blocks with their names, the picture a band across, the
    /// playlist ruled; no corners, no shadows.
    fn layout_brutalist(&mut self, ui: &mut Ui) {
        let t = th();
        let w = ui.available_width();
        let all = ui.available_height();
        let wide = w >= 760.0;
        let compact = all < 560.0;
        let (title, by) = self.now_playing_text();
        let huge = if compact {
            26.0
        } else {
            (w * 0.055).clamp(34.0, 76.0)
        };
        let head_h = if all < 440.0 {
            36.0
        } else if compact {
            64.0
        } else {
            huge * 2.4 + 50.0
        };
        let (head, _) = ui.allocate_exact_size(vec2(w, head_h), Sense::hover());
        let cover_side = if wide {
            (head.height()).min(w * 0.3)
        } else if compact {
            0.0
        } else {
            head.height().min(w * 0.32)
        };
        let words = Rect::from_min_max(
            head.min,
            pos2(
                head.right() - cover_side - if cover_side > 0.0 { 16.0 } else { 0.0 },
                head.bottom(),
            ),
        );
        child(ui, egui::UiBuilder::new().max_rect(words), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(
                egui::Label::new(
                    RichText::new(title.to_uppercase())
                        .font(bold(huge))
                        .color(t.strong),
                )
                .wrap_mode(egui::TextWrapMode::Truncate),
            );
            if !compact {
                ui.add(
                    egui::Label::new(
                        RichText::new(by.to_uppercase())
                            .font(bold(18.0))
                            .color(self.accent),
                    )
                    .truncate(),
                );
                ui.add_space(6.0);
                let times = self.times();
                ui.label(RichText::new(times).font(bold(28.0)).color(t.strong));
            }
        });
        if cover_side > 0.0 {
            let cover = Rect::from_min_size(
                pos2(head.right() - cover_side, head.top()),
                vec2(cover_side, cover_side),
            );
            self.cover_in(ui, cover, 0);
            ui.painter().rect_stroke(
                cover,
                0.0,
                Stroke::new(3.0, t.strong),
                egui::StrokeKind::Inside,
            );
        }
        // the rule
        let (rule, _) =
            ui.allocate_exact_size(vec2(w, if all < 440.0 { 4.0 } else { 8.0 }), Sense::hover());
        ui.painter().rect_filled(
            Rect::from_min_size(rule.min + vec2(0.0, 2.0), vec2(w, 4.0)),
            0.0,
            t.strong,
        );
        // the keys: heavy blocks with their names
        let narrow = w < 560.0;
        let kw = if narrow {
            ((w - 24.0) / 6.0).clamp(30.0, 90.0)
        } else {
            96.0
        };
        let k = Keys {
            look: KeyLook::Slab,
            size: vec2(kw, if compact { 34.0 } else { 52.0 }),
            play: vec2(
                if narrow { kw } else { kw * 1.5 },
                if compact { 34.0 } else { 52.0 },
            ),
            gap: 0.0,
            modes: false,
        };
        let low = all < 440.0;
        if low {
            // (a low window: the switches in the keys' row)
            let k = Keys {
                size: vec2(38.0, 34.0),
                play: vec2(38.0, 34.0),
                ..k
            };
            ui.horizontal(|ui| {
                self.transport_keys(ui, k);
                self.mode_keys(ui, KeyLook::Slab, vec2(38.0, 34.0));
            });
        } else {
            ui.horizontal(|ui| self.transport_keys(ui, k));
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            if !low {
                self.mode_keys(ui, KeyLook::Slab, vec2(44.0, 34.0));
                ui.add_space(12.0);
            }
            self.volume_group(ui, None, (ui.available_width() - 90.0).clamp(40.0, 220.0));
        });
        self.position(ui);
        // the picture: a band across, framed
        let band = if low {
            16.0
        } else if compact {
            (all * 0.16).clamp(36.0, 90.0)
        } else {
            (all * 0.22).clamp(80.0, 300.0)
        };
        let (b, _) = ui.allocate_exact_size(vec2(w, band), Sense::hover());
        self.visual_in(ui, b);
        ui.painter()
            .rect_stroke(b, 0.0, Stroke::new(3.0, t.strong), egui::StrokeKind::Inside);
        ui.add_space(if low { 0.0 } else { 6.0 });
        if low {
            ui.spacing_mut().item_spacing.y = 2.0;
        }
        if self.list_elsewhere {
            return;
        }
        self.lower(ui);
    }

    /// The collage: the cover as a turned photo, the title cut from
    /// letters of other pages, a sticker, the picture on a torn sheet, the
    /// keys on scraps along a strip of tape, the playlist on a ruled page -
    /// sheets turned and overlapping, never over a control.
    fn layout_collage(&mut self, ui: &mut Ui) {
        let t = th();
        let w = ui.available_width();
        let all = ui.available_height();
        let compact = all < 560.0;
        let (title, by) = self.now_playing_text();
        let low = all < 440.0;
        let head_h = if low {
            54.0
        } else if compact {
            76.0
        } else {
            (all * 0.24).clamp(150.0, 260.0)
        };
        let (head, _) = ui.allocate_exact_size(vec2(w, head_h), Sense::hover());
        let p = ui.painter().clone();
        // the photo: the cover in a white frame, turned
        let side = (head_h - 18.0).min(w * 0.3);
        let photo = Rect::from_min_size(head.min + vec2(8.0, 6.0), vec2(side, side));
        let frame = photo.expand2(vec2(6.0, 6.0)).translate(vec2(0.0, 6.0));
        let angle = -0.05;
        p.add(Shape::convex_polygon(
            design::turned(
                Rect::from_min_max(frame.min - vec2(0.0, 6.0), frame.max + vec2(0.0, 8.0)),
                angle,
            ),
            crate::theme::mix(t.surface, Color32::WHITE, 0.92),
            Stroke::new(1.0, crate::theme::mix(t.border, Color32::BLACK, 0.2)),
        ));
        let px = side * ui.ctx().pixels_per_point();
        if let Some(texture) = self.sizer.get(px).or(self.cover.as_ref()).cloned() {
            egui::Image::new(&texture)
                .rotate(angle, egui::Vec2::splat(0.5))
                .paint_at(ui, photo);
        } else {
            p.add(Shape::convex_polygon(
                design::turned(photo, angle),
                viz::mix(t.surface, self.accent, 0.4),
                Stroke::NONE,
            ));
        }
        // the title: words cut out, each its own scrap and type
        let words_rect = Rect::from_min_max(pos2(frame.right() + 22.0, head.top() + 4.0), head.max);
        let fonts = [
            bold(if compact { 22.0 } else { 34.0 }),
            egui::FontId::monospace(if compact { 18.0 } else { 26.0 }),
            egui::FontId::new(
                if compact { 20.0 } else { 30.0 },
                egui::FontFamily::Name("serif".into()),
            ),
            egui::FontId::proportional(if compact { 19.0 } else { 28.0 }),
        ];
        let scraps = [
            crate::theme::mix(t.surface, Color32::WHITE, 0.9),
            self.accent,
            t.strong,
            crate::theme::mix(t.surface, Color32::from_rgb(250, 230, 120), 0.85),
        ];
        let mut at = words_rect.min + vec2(0.0, 4.0);
        let clip = p.with_clip_rect(words_rect);
        // (a short title cut letter by letter, a ransom note's way; a long
        // one word by word - it must still be read)
        let short = title.chars().count() <= 24;
        let pieces: Vec<String> = if short {
            title
                .split_whitespace()
                .flat_map(|w| {
                    w.chars()
                        .map(|c| c.to_uppercase().to_string())
                        .chain(std::iter::once(" ".to_owned()))
                })
                .collect()
        } else {
            title
                .split_whitespace()
                .take(8)
                .map(str::to_uppercase)
                .collect()
        };
        let mut line_h: f32 = 0.0;
        for (i, piece) in pieces.iter().enumerate() {
            if piece == " " {
                at.x += 14.0;
                continue;
            }
            let k = i.wrapping_mul(7).wrapping_add(3);
            let fill = scraps[k % scraps.len()];
            let ink = crate::theme::ink_on_in(t, fill);
            let mut font = fonts[(k / 2) % fonts.len()].clone();
            font.size *= [1.0, 0.82, 1.12, 0.9, 1.05][k % 5];
            let galley = clip.layout_no_wrap(piece.clone(), font, ink);
            let size = galley.size() + vec2(8.0, 4.0);
            if at.x + size.x > words_rect.right() && at.x > words_rect.left() {
                at = pos2(words_rect.left(), at.y + line_h + 6.0);
                line_h = 0.0;
            }
            line_h = line_h.max(size.y);
            let tilt = [-0.07, 0.05, -0.03, 0.08, 0.0, -0.05][k % 6];
            let lift = [0.0, 3.0, -2.0, 1.0][k % 4];
            let rect = Rect::from_min_size(at + vec2(0.0, lift), size);
            clip.add(Shape::convex_polygon(
                design::turned(rect, tilt),
                fill,
                Stroke::NONE,
            ));
            let c = rect.center();
            let half = galley.size() / 2.0;
            let (sn, cs) = tilt.sin_cos();
            let offset = vec2(half.x * cs - half.y * sn, half.x * sn + half.y * cs);
            clip.add(egui::epaint::TextShape::new(c - offset, galley, ink).with_angle(tilt));
            at.x += size.x + 2.0;
        }
        at.y += line_h - 40.0;
        let by_at = pos2(words_rect.left(), (at.y + 44.0).min(head.bottom() - 18.0));
        clip.text(
            by_at,
            egui::Align2::LEFT_TOP,
            &by,
            egui::FontId::monospace(13.0),
            t.text,
        );
        let said = ui.interact(head, ui.scope_id().with("collage-head"), Sense::hover());
        let words = format!("{title} {by}");
        said.widget_info(|| egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, &words));
        // a sticker on the photo's corner
        if !compact {
            design::sticker(
                &p,
                frame.right_top() + vec2(-6.0, 10.0),
                26.0,
                "NOW!",
                Color32::from_rgb(250, 210, 60),
                Color32::from_rgb(30, 20, 10),
                0.25,
            );
        }
        // the picture on a torn sheet
        let band = if low {
            14.0
        } else if compact {
            (all * 0.2).clamp(40.0, 100.0)
        } else {
            (all * 0.26).clamp(90.0, 360.0)
        };
        let (sheet, _) = ui.allocate_exact_size(vec2(w, band + 22.0), Sense::hover());
        design::torn_sheet(
            &p,
            sheet.shrink2(vec2(4.0, 2.0)),
            0.012,
            crate::theme::mix(t.surface, Color32::WHITE, if t.dark { 0.06 } else { 0.7 }),
            t.border,
            3,
        );
        self.visual_in(ui, sheet.shrink2(vec2(18.0, 10.0)));
        // the keys along a strip of tape
        let (strip, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
        p.add(Shape::convex_polygon(
            design::turned(strip.shrink2(vec2(0.0, 6.0)), -0.01),
            Color32::from_rgba_unmultiplied(240, 226, 170, 70),
            Stroke::NONE,
        ));
        child(
            ui,
            egui::UiBuilder::new()
                .max_rect(strip.shrink2(vec2(10.0, 4.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                let narrow = w < 560.0;
                let k = Keys {
                    look: KeyLook::Paper,
                    size: vec2(if narrow { 32.0 } else { 46.0 }, 36.0),
                    play: vec2(if narrow { 38.0 } else { 58.0 }, 40.0),
                    gap: if narrow { 2.0 } else { 6.0 },
                    modes: !narrow || low,
                };
                self.transport_keys(ui, k);
                if low {
                    // (a low window: the volume on the strip too)
                    let slider = (ui.available_width() - 70.0).clamp(30.0, 110.0);
                    self.volume_group(ui, None, slider);
                }
            },
        );
        if low {
        } else if w < 560.0 {
            ui.horizontal(|ui| {
                self.mode_keys(ui, KeyLook::Paper, vec2(34.0, 30.0));
                self.volume_group(ui, None, 90.0);
            });
        } else {
            right_aligned(ui, egui::Id::unique("collage-volume"), |ui| {
                self.volume_group(ui, None, 110.0)
            });
        }
        self.position(ui);
        // the playlist on a ruled page, turned a little
        let rest = ui.available_rect_before_wrap();
        let page = design::list_fill(design::current(), t);
        p.add(Shape::convex_polygon(
            design::turned(rest.shrink(2.0), 0.006),
            page,
            Stroke::new(1.0, t.border),
        ));
        let mut y = rest.top() + 26.0;
        let rule = crate::theme::mix(page, Color32::from_rgb(90, 140, 220), 0.25);
        while y < rest.bottom() - 4.0 {
            p.line_segment(
                [pos2(rest.left() + 6.0, y), pos2(rest.right() - 6.0, y)],
                Stroke::new(1.0, rule),
            );
            y += 22.0;
        }
        p.line_segment(
            [
                pos2(rest.left() + 30.0, rest.top() + 4.0),
                pos2(rest.left() + 30.0, rest.bottom() - 4.0),
            ],
            Stroke::new(
                1.0,
                crate::theme::mix(page, Color32::from_rgb(220, 70, 70), 0.4),
            ),
        );
        child(
            ui,
            egui::UiBuilder::new().max_rect(rest.shrink2(vec2(12.0, 6.0))),
            |ui| self.lower(ui),
        );
    }

    /// The position as a bar of characters, a slider to the keys and the
    /// pointer ("Position").
    fn ascii_position(&mut self, ui: &mut Ui) {
        let total = self.duration.unwrap_or(0.0);
        let pos = self.seek_drag.unwrap_or(self.shown_position());
        let seekable = self.info.seekable && self.duration.is_some();
        let share = if total > 0.0 {
            (pos / total) as f32
        } else {
            0.0
        };
        let times = self.times();
        let font = egui::FontId::monospace(13.0);
        let cell = ui.fonts_mut(|f| f.glyph_width(&font, '#')).max(6.0);
        let chars = (((ui.available_width() - 140.0) / cell) as usize).clamp(8, 80);
        let text = format!("  {} {times}", design::ascii_bar(share, chars));
        let galley = ui.painter().layout_no_wrap(text, font, th().text);
        let (rect, response) = ui.allocate_exact_size(
            vec2(galley.size().x, galley.size().y.max(24.0)),
            Sense::click_and_drag(),
        );
        let bar = Rect::from_min_size(
            rect.min + vec2(cell * 3.0, 0.0),
            vec2(cell * chars as f32, rect.height()),
        );
        ui.painter().galley(
            pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            th().text,
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                bar.expand(1.0),
                0.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Outside,
            );
        }
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::slider(seekable, pos, "Position");
            info.label = Some("Position".into());
            info
        });
        if seekable && total > 0.0 {
            if let Some(p) = response.interact_pointer_pos()
                && (response.clicked() || response.dragged())
            {
                let at = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0) as f64 * total;
                if response.dragged() {
                    self.seek_drag = Some(at);
                } else {
                    self.seek(at);
                }
            }
            if response.drag_stopped()
                && let Some(at) = self.seek_drag.take()
            {
                self.seek(at);
            }
            if response.has_focus() {
                let (l, r) = ui.input(|i| {
                    (
                        i.key_pressed(Key::ArrowLeft),
                        i.key_pressed(Key::ArrowRight),
                    )
                });
                if l {
                    self.seek(pos - 5.0);
                }
                if r {
                    self.seek(pos + 5.0);
                }
            }
        }
        response.on_hover_text("Seek: click or drag the bar, ←/→ (focused), J/L (15 s)");
    }

    /// The volume as a bar of characters, `vol [||||||....]`, a slider.
    fn ascii_volume(&mut self, ui: &mut Ui) {
        let font = egui::FontId::monospace(13.0);
        let n = (self.volume as usize * 10).div_ceil(100);
        let text = format!(
            "vol [{}{}] {:>3}%",
            "|".repeat(n),
            ".".repeat(10 - n),
            self.volume
        );
        let galley = ui.painter().layout_no_wrap(text, font, th().text);
        let (rect, response) =
            ui.allocate_exact_size(vec2(galley.size().x, 24.0), Sense::click_and_drag());
        ui.painter().galley(
            pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            th().text,
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.expand(1.0),
                0.0,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Outside,
            );
        }
        let v = self.volume as f64;
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::slider(true, v, "Volume");
            info.label = Some("Volume".into());
            info
        });
        let cell = rect.width() / 20.0;
        let bar = Rect::from_min_max(
            pos2(rect.left() + cell * 5.0, rect.top()),
            pos2(rect.left() + cell * 15.0, rect.bottom()),
        );
        if let Some(p) = response.interact_pointer_pos()
            && (response.clicked() || response.dragged())
        {
            let v = ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0) * 100.0;
            self.set_volume(v.round() as u32);
        }
        if response.has_focus() {
            let (up, down) = ui.input(|i| {
                (
                    i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowRight),
                    i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowLeft),
                )
            });
            if up {
                self.set_volume((self.volume + 5).min(100));
            }
            if down {
                self.set_volume(self.volume.saturating_sub(5));
            }
        }
        let notches = self.wheel_steps(ui, &response);
        if notches != 0 {
            self.set_volume((self.volume as i32 + 5 * notches).clamp(0, 100) as u32);
        }
        response.on_hover_text("Volume (↑/↓ focused, the wheel)");
    }
}

/// A part laid out in `builder`'s rect, the parent's cursor not moved by
/// it (the layouts allocate their units first, then fill them).
fn child<R>(ui: &mut Ui, builder: egui::UiBuilder, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut c = ui.new_child(builder);
    add(&mut c)
}

/// A group of widgets placed at the row's right end, laid out left to
/// right (Tab goes through it as it is seen): its width measured the frame
/// before (the first frame done again where it changed).
pub(super) fn right_aligned(ui: &mut Ui, id: egui::Id, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let before: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
        // (the space and the group: two items, a gap between them)
        let room = ui.available_width() - ui.spacing().item_spacing.x;
        if before > 0.0 && before < room {
            ui.add_space(room - before);
        }
        let width = ui.scope(add).response.rect.width();
        if (width - before).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(id, width));
            ui.ctx()
                .request_discard("a group's width at the right changed");
        }
    });
}

/// A group of widgets in the middle of its row, left to right.
pub(super) fn centred(ui: &mut Ui, id: egui::Id, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let before: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
        let room = ui.available_width() - ui.spacing().item_spacing.x;
        if before > 0.0 && before < room {
            ui.add_space((room - before) / 2.0);
        }
        let width = ui.scope(add).response.rect.width();
        if (width - before).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(id, width));
            ui.ctx().request_discard("a centred group's width changed");
        }
    });
}

/// A rack unit's panel: brushed metal, screws, its name engraved.
fn rack_unit(ui: &Ui, rect: Rect, name: &str, t: &crate::theme::Theme) {
    let d = design::find("rack");
    let fill = design::list_fill(d, t);
    design::panel(ui.painter(), rect, d, t, fill, name.len() as u32);
    ui.painter().text(
        rect.left_top() + vec2(20.0, 3.0),
        egui::Align2::LEFT_TOP,
        name,
        egui::FontId::new(9.0, egui::FontFamily::Name("bold".into())),
        crate::theme::ink_on_in(t, fill),
    );
}

/// A 90s window: a raised grey frame, a title bar in a gradient with its
/// title.
fn retro_window(ui: &Ui, rect: Rect, title: &str, t: &crate::theme::Theme) {
    let d = design::find("desktop90");
    design::panel(ui.painter(), rect, d, t, t.raised, 0);
    let bar = Rect::from_min_size(rect.min + vec2(3.0, 3.0), vec2(rect.width() - 6.0, 18.0));
    let a = crate::theme::mix(t.accent, Color32::BLACK, 0.45);
    let b = crate::theme::mix(t.accent, Color32::BLACK, 0.1);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(bar.left_top(), a);
    mesh.colored_vertex(bar.right_top(), b);
    mesh.colored_vertex(bar.right_bottom(), b);
    mesh.colored_vertex(bar.left_bottom(), a);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(Shape::mesh(mesh));
    // (the title's ink: whichever of white and black reads on both ends)
    let ink = if crate::viz::contrast(Color32::WHITE, a)
        .min(crate::viz::contrast(Color32::WHITE, b))
        >= crate::viz::contrast(Color32::BLACK, a).min(crate::viz::contrast(Color32::BLACK, b))
    {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    ui.painter().text(
        bar.left_center() + vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::new(11.0, egui::FontFamily::Name("bold".into())),
        ink,
    );
}

/// A sunken well (the 90s readouts).
fn sunken(p: &egui::Painter, rect: Rect, t: &crate::theme::Theme) {
    let dark = crate::theme::mix(t.raised, Color32::BLACK, 0.5);
    let light = crate::theme::mix(t.raised, Color32::WHITE, 0.45);
    let r = rect.shrink(0.5);
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, dark));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, dark));
    p.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, light));
    p.line_segment([r.right_bottom(), r.left_bottom()], Stroke::new(1.0, light));
}

/// A box of the terminal: lines round it, its name in its top edge.
fn tui_box(p: &egui::Painter, rect: Rect, name: &str, t: &crate::theme::Theme) {
    let r = rect.shrink2(vec2(2.0, 6.0));
    let edge = Stroke::new(1.0, crate::theme::mix(t.border, t.text, 0.5));
    p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Middle);
    let font = egui::FontId::monospace(12.0);
    let label = format!("┤ {name} ├");
    let galley = p.layout_no_wrap(label, font, t.accent);
    let at = r.left_top() + vec2(12.0, -galley.size().y / 2.0);
    p.rect_filled(Rect::from_min_size(at, galley.size()), 0.0, t.ground);
    p.galley(at, galley, t.accent);
}

/// A tape reel: the hub with its spokes turned by `turn`, the tape wound
/// round it to `full` (0..1 of its radius).
fn reel(p: &egui::Painter, c: Pos2, r: f32, full: f32, turn: f32, t: &crate::theme::Theme) {
    let tape = crate::theme::mix(t.ground, Color32::from_rgb(70, 45, 30), 0.7);
    let flange = crate::theme::mix(t.surface, Color32::WHITE, if t.dark { 0.1 } else { 0.5 })
        .gamma_multiply(0.85);
    p.circle_filled(c, r, flange);
    p.circle_filled(c, r * full.clamp(0.4, 0.98), tape);
    let hub = r * 0.34;
    p.circle_filled(c, hub, t.surface);
    p.circle_stroke(c, hub, Stroke::new(1.0, t.border));
    for k in 0..3 {
        let a = (turn + k as f32 / 3.0) * std::f32::consts::TAU;
        let d = vec2(a.cos(), a.sin());
        p.line_segment(
            [c + d * hub * 0.35, c + d * hub * 0.95],
            Stroke::new(3.0, t.muted),
        );
    }
    p.circle_stroke(c, r, Stroke::new(1.0, t.border));
}
