//! The Mixer tab: the player's own output and volume, and the system's
//! sound server (crate::mixer) - outputs, inputs, the applications playing
//! and recording, the cards' profiles.

use crate::mixer::{Action, Device, Kind, MAX_PERCENT, Mixer, Status, Stream};
use egui::{Color32, ComboBox, RichText, Ui};

/// A section's head: its name and how many there are.
fn section(ui: &mut Ui, title: &str, count: Option<usize>) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(15.0));
        if let Some(n) = count {
            ui.label(
                RichText::new(n.to_string())
                    .small()
                    .color(crate::theme::current().muted),
            );
        }
    });
    ui.add_space(2.0);
}

/// A card of the page: rounded, the panel's colour, room inside.
fn card<R>(ui: &mut Ui, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A pill: a few words on a filled, rounded ground.
fn pill(ui: &mut Ui, text: &str, fill: Color32, ink: Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(ink));
        });
}

/// Cards side by side where the page is wide (two columns from 760 px).
fn grid<T>(ui: &mut Ui, items: &[T], mut add: impl FnMut(&mut Ui, &T)) {
    let columns = if ui.available_width() >= 760.0 { 2 } else { 1 };
    if columns == 1 || items.len() < 2 {
        for item in items {
            add(ui, item);
            ui.add_space(6.0);
        }
        return;
    }
    ui.columns(columns, |cols| {
        for (i, item) in items.iter().enumerate() {
            add(&mut cols[i % columns], item);
            cols[i % columns].add_space(6.0);
        }
    });
}

/// The ink that reads on `fill` (the theme's light or dark).
fn ink_on(fill: Color32) -> Color32 {
    crate::theme::ink_on(fill)
}

/// What a device is, in words: its kind and how it is connected.
fn kind_words(d: &Device) -> String {
    let what = if d.kind == Kind::Sink {
        "Output"
    } else {
        "Input"
    };
    let how = match d.bus.as_str() {
        "bluetooth" => " · Bluetooth",
        "usb" => " · USB",
        "pci" => " · built in",
        _ => "",
    };
    let form = if d.form_factor.is_empty() {
        String::new()
    } else {
        format!(" · {}", d.form_factor)
    };
    format!("{what}{how}{form}")
}

/// What the page keeps between frames.
#[derive(Default)]
pub struct MixerPage {
    pub mixer: Option<Mixer>,
    pub show_monitors: bool,
    /// The player's outputs (id, name), read when the page opens or on ↻.
    pub devices: Option<Vec<(String, String)>>,
}

/// What the page changed of the player.
pub enum PlayerChange {
    Output(String),
    Volume(u32),
    Muted(bool),
    Equalizer(bool),
}

fn icon(bus: &str, form: &str, kind: Kind) -> &'static str {
    match (bus, form) {
        ("bluetooth", _) | (_, "headset" | "headphone" | "hands-free") => "🎧",
        _ if kind == Kind::Source => "🎤",
        _ => "🔊",
    }
}

/// A volume slider (0 to `max` %) with mute: the volume while it is dragged
/// (the mixer sets the newest), the mute when it is clicked.
fn volume_row(
    ui: &mut Ui,
    label: &str,
    percent: f64,
    muted: bool,
    max: f64,
) -> (Option<f64>, Option<bool>) {
    let mut value = percent;
    let mut new_mute = None;
    let mut new_volume = None;
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(if muted { "🔇" } else { "🔊" }).frame(false))
            .accessible_name(format!("Mute {label}"))
            .on_hover_text(if muted { "Unmute" } else { "Mute" })
            .clicked()
        {
            new_mute = Some(!muted);
        }
        ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(80.0);
        let response = ui
            .add(
                egui::Slider::new(&mut value, 0.0..=max)
                    .show_value(false)
                    .trailing_fill(true),
            )
            .accessible_name(format!("Volume of {label}"))
            .on_hover_text(if max > 100.0 {
                format!("{value:.0} % (above 100 %: amplified)")
            } else {
                format!("{value:.0} %")
            });
        if response.changed() || response.drag_stopped() {
            new_volume = Some(value);
        }
        ui.label(
            RichText::new(format!("{value:>3.0} %"))
                .monospace()
                .color(if value > 100.0 {
                    crate::theme::current().warn
                } else {
                    crate::theme::current().muted
                }),
        );
    });
    (new_volume, new_mute)
}

fn device_rows(ui: &mut Ui, mixer: &Mixer, devices: &[Device], title: &str, accent: Color32) {
    section(ui, title, Some(devices.len()));
    if devices.is_empty() {
        ui.label(RichText::new("none").color(crate::theme::current().muted));
    }
    grid(ui, devices, |ui, d| {
        card(ui, crate::theme::current().surface, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon(&d.bus, &d.form_factor, d.kind)).size(22.0));
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(d.label()).strong()).truncate());
                    ui.label(
                        RichText::new(kind_words(d))
                            .small()
                            .color(crate::theme::current().muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if d.is_default {
                        pill(ui, "default", accent, ink_on(accent));
                    } else if ui.small_button("Make default").clicked() {
                        mixer.act(Action::Default {
                            kind: d.kind,
                            name: d.name.clone(),
                        });
                    }
                });
            });
            // (an output and an input often share a name: the kind tells them apart)
            let which = format!(
                "{} {}",
                if d.kind == Kind::Sink {
                    "output"
                } else {
                    "input"
                },
                d.label()
            );
            let (volume, mute) = volume_row(ui, &which, d.volume_percent(), d.muted, MAX_PERCENT);
            if let Some(percent) = volume {
                mixer.act(Action::DeviceVolume {
                    kind: d.kind,
                    name: d.name.clone(),
                    channels: d.channels.clone(),
                    percent,
                });
            }
            if let Some(muted) = mute {
                mixer.act(Action::DeviceMute {
                    kind: d.kind,
                    name: d.name.clone(),
                    muted,
                });
            }
            if d.ports.len() > 1 {
                let mut port = d.active_port.clone();
                let label = d
                    .ports
                    .iter()
                    .find(|p| p.name == port)
                    .map(|p| p.description.clone())
                    .unwrap_or_else(|| port.clone());
                ComboBox::from_id_salt(("port", d.kind.key(), d.index))
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for p in &d.ports {
                            let text = if p.available == Some(false) {
                                format!("{} (unplugged)", p.description)
                            } else {
                                p.description.clone()
                            };
                            ui.selectable_value(&mut port, p.name.clone(), text);
                        }
                    })
                    .response
                    .accessible_name(format!("Port of {which}"));
                if port != d.active_port {
                    mixer.act(Action::Port {
                        kind: d.kind,
                        device: d.name.clone(),
                        port,
                    });
                }
            }
        });
    });
}

fn stream_rows(ui: &mut Ui, mixer: &Mixer, streams: &[Stream], devices: &[Device], title: &str) {
    section(ui, title, Some(streams.len()));
    if streams.is_empty() {
        ui.label(RichText::new("none").color(crate::theme::current().muted));
    }
    grid(ui, streams, |ui, st| {
        card(ui, crate::theme::current().surface, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if st.kind == Kind::SourceOutput {
                        "🎤"
                    } else {
                        "▶"
                    })
                    .size(18.0),
                );
                let label = if st.is_own() {
                    "axross-amp (this player)".to_owned()
                } else {
                    st.label()
                };
                ui.label(RichText::new(label).strong());
                if !st.media.is_empty() && st.media != st.application {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&st.media).color(crate::theme::current().muted),
                        )
                        .truncate(),
                    );
                }
                if st.corked {
                    ui.label(
                        RichText::new("paused")
                            .small()
                            .color(crate::theme::current().muted),
                    );
                }
            });
            if !st.channels.is_empty() {
                let (volume, mute) =
                    volume_row(ui, &st.label(), st.volume_percent(), st.muted, MAX_PERCENT);
                if let Some(percent) = volume {
                    mixer.act(Action::StreamVolume {
                        kind: st.kind,
                        index: st.index,
                        channels: st.channels.clone(),
                        percent,
                    });
                }
                if let Some(muted) = mute {
                    mixer.act(Action::StreamMute {
                        kind: st.kind,
                        index: st.index,
                        muted,
                    });
                }
            }
            let mut target = st.device_index;
            let name_of = |i: Option<u32>| {
                devices
                    .iter()
                    .find(|d| Some(d.index) == i)
                    .map(|d| d.label().to_owned())
                    .unwrap_or_else(|| "?".into())
            };
            ComboBox::from_id_salt(("move", st.kind.key(), st.index))
                .selected_text(name_of(target))
                .show_ui(ui, |ui| {
                    for d in devices
                        .iter()
                        .filter(|d| !d.is_monitor || st.kind == Kind::SourceOutput)
                    {
                        ui.selectable_value(&mut target, Some(d.index), d.label());
                    }
                })
                .response
                .accessible_name(format!("Device of {}", st.label()));
            if target != st.device_index
                && let Some(d) = devices.iter().find(|d| Some(d.index) == target)
            {
                mixer.act(Action::Move {
                    kind: st.kind,
                    index: st.index,
                    device: d.name.clone(),
                });
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut MixerPage,
    output: &str,
    volume: u32,
    muted: bool,
    eq_on: bool,
    accent: Color32,
    wake: impl Fn() + Send + Sync + 'static,
) -> Vec<PlayerChange> {
    let mut changes = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // (a column of a readable width in the middle of a wide window)
            let room = ui.available_width();
            let side = ((room - 980.0) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_max_width(room.min(980.0));
                    page_body(
                        ui,
                        page,
                        output,
                        volume,
                        muted,
                        eq_on,
                        accent,
                        wake,
                        &mut changes,
                    );
                });
            });
        });
    changes
}

#[allow(clippy::too_many_arguments)]
fn page_body(
    ui: &mut Ui,
    page: &mut MixerPage,
    output: &str,
    volume: u32,
    muted: bool,
    eq_on: bool,
    accent: Color32,
    wake: impl Fn() + Send + Sync + 'static,
    changes: &mut Vec<PlayerChange>,
) {
    // -- the player: a card in the cover's colour
    let tint = crate::app::card_tint(accent);
    card(ui, tint, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("🎵").size(22.0));
            ui.label(RichText::new("This player").strong().size(16.0));
        });
        let devices = page
            .devices
            .get_or_insert_with(crate::engine::output_devices)
            .clone();
        let mut rescan = false;
        ui.horizontal(|ui| {
            ui.label("Output");
            let mut chosen = output.to_owned();
            let label = if chosen.is_empty() {
                "System default".to_owned()
            } else {
                devices
                    .iter()
                    .find(|(id, _)| *id == chosen)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| format!("{chosen} (not there)"))
            };
            ComboBox::from_id_salt("player-output")
                .selected_text(label)
                .width(ui.available_width() - 40.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, String::new(), "System default");
                    for (id, name) in devices.iter() {
                        ui.selectable_value(&mut chosen, id.clone(), name);
                    }
                })
                .response
                .accessible_name("Output of this player");
            if chosen != output {
                changes.push(PlayerChange::Output(chosen));
            }
            if ui
                .small_button("↻")
                .on_hover_text("Look for output devices again")
                .clicked()
            {
                rescan = true;
            }
        });
        if rescan {
            page.devices = None;
        }
        // (the player's own volume goes to 100 %: above, the system's)
        let (vol, mute) = volume_row(ui, "this player", volume as f64, muted, 100.0);
        if let Some(v) = vol {
            changes.push(PlayerChange::Volume(v.min(100.0) as u32));
        }
        if let Some(m) = mute {
            changes.push(PlayerChange::Muted(m));
        }
        let mut eq = eq_on;
        if ui.checkbox(&mut eq, "Equalizer").changed() {
            changes.push(PlayerChange::Equalizer(eq));
        }
    });

    // -- the system
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("System").strong().size(18.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut page.show_monitors, "Show monitor inputs");
        });
    });
    let mixer = page.mixer.get_or_insert_with(|| Mixer::start(wake));
    let view = mixer.view();
    match &view.status {
        Status::Starting => {
            ui.label(
                RichText::new("Asking the sound server…").color(crate::theme::current().muted),
            );
        }
        Status::Unavailable(why) => {
            ui.label(RichText::new(why).color(crate::theme::current().warn));
            if ui.button("Try again").clicked() {
                page.mixer = None;
            }
            return;
        }
        Status::Ready => {}
    }
    if !view.error.is_empty() {
        ui.label(RichText::new(&view.error).color(crate::theme::current().bad));
    }
    let s = &view.snapshot;
    device_rows(ui, mixer, &s.sinks, "Outputs", accent);
    let inputs: Vec<Device> = s
        .sources
        .iter()
        .filter(|d| page.show_monitors || !d.is_monitor)
        .cloned()
        .collect();
    device_rows(ui, mixer, &inputs, "Inputs", accent);
    stream_rows(ui, mixer, &s.sink_inputs, &s.sinks, "Playing");
    stream_rows(ui, mixer, &s.source_outputs, &s.sources, "Recording");
    section(ui, "Sound cards", Some(s.cards.len()));
    for sound_card in &s.cards {
        let card_ = sound_card;
        card(ui, crate::theme::current().surface, |ui| {
            let card = card_;
            ui.horizontal(|ui| {
                ui.label(icon(&card.bus, "", Kind::Sink));
                ui.label(&card.description);
                let mut profile = card.active_profile.clone();
                let label = card
                    .profiles
                    .iter()
                    .find(|p| p.name == profile)
                    .map(|p| p.description.clone())
                    .unwrap_or_else(|| profile.clone());
                ComboBox::from_id_salt(("card", card.index))
                    .selected_text(label)
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for p in &card.profiles {
                            let text = if p.available {
                                p.description.clone()
                            } else {
                                format!("{} (not available)", p.description)
                            };
                            ui.add_enabled_ui(p.available || p.name == card.active_profile, |ui| {
                                ui.selectable_value(&mut profile, p.name.clone(), text);
                            });
                        }
                    })
                    .response
                    .accessible_name(format!("Profile of {}", card.description));
                if profile != card.active_profile {
                    mixer.act(Action::Profile {
                        card: card.name.clone(),
                        profile,
                    });
                }
            });
        });
        ui.add_space(6.0);
    }
}
