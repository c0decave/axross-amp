//! The Mixer tab: the player's own output and volume, and the system's
//! sound server (crate::mixer) - outputs, inputs, the applications playing
//! and recording, the cards' profiles.

use crate::mixer::{Action, Device, Kind, MAX_PERCENT, Mixer, Status, Stream};
use egui::{Color32, ComboBox, RichText, Ui};

const MUTED: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);

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
                    Color32::from_rgb(0xe3, 0xb3, 0x41)
                } else {
                    MUTED
                }),
        );
    });
    (new_volume, new_mute)
}

fn device_rows(ui: &mut Ui, mixer: &Mixer, devices: &[Device], title: &str) {
    ui.label(RichText::new(title).strong());
    if devices.is_empty() {
        ui.label(RichText::new("none").color(MUTED));
    }
    for d in devices {
        egui::Frame::new()
            .fill(Color32::from_rgb(0x16, 0x1b, 0x22))
            .corner_radius(8)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(icon(&d.bus, &d.form_factor, d.kind));
                    ui.add(egui::Label::new(RichText::new(d.label()).strong()).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if d.is_default {
                            ui.label(
                                RichText::new("default")
                                    .small()
                                    .color(ui.visuals().hyperlink_color),
                            );
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
                let (volume, mute) =
                    volume_row(ui, &which, d.volume_percent(), d.muted, MAX_PERCENT);
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
    }
}

fn stream_rows(ui: &mut Ui, mixer: &Mixer, streams: &[Stream], devices: &[Device], title: &str) {
    ui.label(RichText::new(title).strong());
    if streams.is_empty() {
        ui.label(RichText::new("none").color(MUTED));
    }
    for st in streams {
        egui::Frame::new()
            .fill(Color32::from_rgb(0x16, 0x1b, 0x22))
            .corner_radius(8)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let label = if st.is_own() {
                        "axross-amp (this player)".to_owned()
                    } else {
                        st.label()
                    };
                    ui.label(RichText::new(label).strong());
                    if !st.media.is_empty() && st.media != st.application {
                        ui.add(egui::Label::new(RichText::new(&st.media).color(MUTED)).truncate());
                    }
                    if st.corked {
                        ui.label(RichText::new("paused").small().color(MUTED));
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
    }
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut MixerPage,
    output: &str,
    volume: u32,
    muted: bool,
    eq_on: bool,
    wake: impl Fn() + Send + Sync + 'static,
) -> Vec<PlayerChange> {
    let mut changes = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // -- the player
            ui.label(RichText::new("This player").strong().size(16.0));
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
            ui.separator();

            // -- the system
            ui.horizontal(|ui| {
                ui.label(RichText::new("System").strong().size(16.0));
                ui.checkbox(&mut page.show_monitors, "Show monitor inputs");
            });
            let mixer = page.mixer.get_or_insert_with(|| Mixer::start(wake));
            let view = mixer.view();
            match &view.status {
                Status::Starting => {
                    ui.label(RichText::new("Asking the sound server…").color(MUTED));
                }
                Status::Unavailable(why) => {
                    ui.label(RichText::new(why).color(Color32::from_rgb(0xe3, 0xb3, 0x41)));
                    if ui.button("Try again").clicked() {
                        page.mixer = None;
                    }
                    return;
                }
                Status::Ready => {}
            }
            if !view.error.is_empty() {
                ui.label(RichText::new(&view.error).color(Color32::from_rgb(0xff, 0x7b, 0x72)));
            }
            let s = &view.snapshot;
            device_rows(ui, mixer, &s.sinks, "Outputs");
            let inputs: Vec<Device> = s
                .sources
                .iter()
                .filter(|d| page.show_monitors || !d.is_monitor)
                .cloned()
                .collect();
            device_rows(ui, mixer, &inputs, "Inputs");
            stream_rows(ui, mixer, &s.sink_inputs, &s.sinks, "Playing");
            stream_rows(ui, mixer, &s.source_outputs, &s.sources, "Recording");
            ui.label(RichText::new("Cards").strong());
            for card in &s.cards {
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
                                ui.add_enabled_ui(
                                    p.available || p.name == card.active_profile,
                                    |ui| {
                                        ui.selectable_value(&mut profile, p.name.clone(), text);
                                    },
                                );
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
            }
        });
    changes
}
