//! The Bluetooth tab: the adapter (power, scan), the devices BlueZ knows
//! (kind, state, battery, signal), and Pair, Trust, Connect, Disconnect,
//! Forget and *Use as audio output* - which connects a speaker or headset
//! if needed, waits for its output to appear in the sound server and makes
//! it this player's output. A pairing that asks something (confirm a
//! passkey, type a PIN) asks in a dialog; nothing is accepted without an
//! answer.

use crate::bluetooth::{
    Action, AgentKind, AgentRequest, Answer, Bluetooth, BluetoothView, Device, Op, Status,
    Transport,
};
use crate::mixer_ui::PlayerChange;
use egui::{Color32, ComboBox, Id, Modal, RichText, Ui};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MUTED: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
const GOOD: Color32 = Color32::from_rgb(0x56, 0xd3, 0x64);
const BAD: Color32 = Color32::from_rgb(0xff, 0x7b, 0x72);
/// How long *Use as audio output* waits for the sound server to make the
/// device's output once it is connected.
pub const OUTPUT_WAIT: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct BluetoothPage {
    pub client: Option<Bluetooth>,
    pub selected: Option<String>,
    adapter: Option<String>,
    transport: Option<Transport>,
    filter: String,
    named_only: bool,
    audio_only: bool,
    pub message: String,
    message_bad: bool,
    /// The newest result already told.
    told: u64,
    /// Names of the devices seen (a removed one is still named in its result).
    names: std::collections::BTreeMap<String, String>,
    /// The PIN or passkey being typed, and the question it answers.
    typed: String,
    typed_for: u64,
    /// Codes the user closed (they stay until their pairing ends).
    dismissed: Vec<u64>,
    confirm_forget: Option<String>,
    output: Option<OutputWait>,
}

/// What the search for a device's output found: (id, name) or why not.
type Found = Arc<Mutex<Option<Result<(String, String), String>>>>;

enum Stage {
    Connecting { after: u64 },
    Waiting,
}

struct OutputWait {
    device: String,
    name: String,
    stage: Stage,
    found: Found,
}

/// The outputs the player can use: the sound server's (by their pactl
/// name, as the player's PulseAudio host knows them), then the player's own list.
fn outputs() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if let Ok(snapshot) = crate::mixer::refresh() {
        for sink in snapshot.sinks {
            let id = cpal::DeviceId::new(cpal::HostId::PulseAudio, &sink.name).to_string();
            out.push((id, sink.label().to_owned()));
        }
    }
    out
}

fn wait_for_output(
    address: String,
    names: Vec<String>,
    found: Found,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let _ = std::thread::Builder::new()
        .name("amp-bluetooth-output".into())
        .spawn(move || {
            let end = Instant::now() + OUTPUT_WAIT;
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let result = loop {
                let mut candidates = outputs();
                if crate::bluetooth::find_sink(&candidates, &address, &names).is_none() {
                    candidates.extend(crate::engine::output_devices());
                }
                if let Some(hit) = crate::bluetooth::find_sink(&candidates, &address, &names) {
                    break Ok(hit.clone());
                }
                if Instant::now() >= end {
                    break Err(format!(
                        "No audio output for it appeared within {} s. The device is connected, \
                         but the sound server made no output for it - its Bluetooth support \
                         (PipeWire's or PulseAudio's) may be missing, or the device offers no \
                         audio profile.",
                        OUTPUT_WAIT.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(400));
            };
            if let Ok(mut slot) = found.lock() {
                *slot = Some(result);
            }
            wake();
        });
}

fn state_words(d: &Device) -> String {
    let words: Vec<&str> = [
        ("Connected", d.connected),
        ("Paired", d.paired),
        ("Trusted", d.trusted),
        ("Blocked", d.blocked),
    ]
    .into_iter()
    .filter_map(|(w, on)| on.then_some(w))
    .collect();
    words.join(", ")
}

fn question(q: &AgentRequest) -> String {
    let name = &q.device_name;
    let passkey = q.passkey.map(|p| format!("{p:06}")).unwrap_or_default();
    match q.kind {
        AgentKind::Confirm => {
            format!("Does {name} show the passkey {passkey}? Confirm only if it does.")
        }
        AgentKind::Pin => format!(
            "Enter the PIN for {name} - the one it shows, or the one in its manual \
             (often 0000 or 1234):"
        ),
        AgentKind::Passkey => format!("Enter the 6-digit passkey {name} shows:"),
        AgentKind::DisplayPasskey => match q.entered {
            Some(n) if n > 0 => {
                format!("Type {passkey} on {name} and press Enter there ({n} of 6 typed).")
            }
            _ => format!("Type {passkey} on {name} and press Enter there."),
        },
        AgentKind::DisplayPin => format!("Enter the PIN {} on {name}.", q.pin),
        AgentKind::Authorize => format!("Allow pairing with {name}?"),
        AgentKind::AuthorizeService => format!("Allow {name} to use the service {}?", q.uuid),
    }
}

fn told(op: Op, name: &str, error: &str, timed_out: bool) -> Option<String> {
    if error.is_empty() {
        return match op {
            Op::Pair => Some(format!("Paired with {name}.")),
            Op::Connect => Some(format!("Connected to {name}.")),
            Op::Disconnect => Some(format!("Disconnected from {name}.")),
            Op::Remove => Some(format!("Forgot {name}.")),
            Op::StopScan if timed_out => Some(
                "The scan stopped after 2 minutes (it keeps the radio busy); Scan looks again."
                    .into(),
            ),
            _ => None,
        };
    }
    let what = match op {
        Op::Power => "Could not switch the adapter on or off".to_owned(),
        Op::Scan => "Could not start the scan".to_owned(),
        Op::StopScan => "Could not stop the scan".to_owned(),
        Op::Pair => format!("Could not pair with {name}"),
        Op::CancelPairing => format!("Could not cancel the pairing with {name}"),
        Op::Trust => format!("Could not change the trust of {name}"),
        Op::Connect => format!("Could not connect to {name}"),
        Op::Disconnect => format!("Could not disconnect {name}"),
        Op::Remove => format!("Could not forget {name}"),
    };
    Some(format!("{what}: {error}"))
}

impl BluetoothPage {
    fn say(&mut self, text: String, bad: bool) {
        self.message = text;
        self.message_bad = bad;
    }

    fn name(&self, path: &str) -> String {
        self.names
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or("").to_owned())
    }

    /// Results that came since the last frame, as the message line.
    fn take_results(&mut self, view: &BluetoothView) {
        for f in &view.finished {
            if f.serial <= self.told {
                continue;
            }
            self.told = f.serial;
            // (a canceled pairing says so through the pairing's own result)
            if f.op == Op::CancelPairing && f.error.is_empty() {
                continue;
            }
            if let Some(text) = told(f.op, &self.name(&f.path), &f.error, f.timed_out) {
                self.say(text, !f.error.is_empty());
            }
        }
    }

    /// *Use as audio output*: its stages, one step per frame.
    fn follow_output(
        &mut self,
        view: &BluetoothView,
        wake: &Arc<dyn Fn() + Send + Sync>,
    ) -> Option<PlayerChange> {
        let wait = self.output.as_mut()?;
        let device = view.device(&wait.device).cloned();
        match wait.stage {
            Stage::Connecting { after } => {
                let Some(device) = device else {
                    let text = format!("{} is no longer known to BlueZ.", wait.name);
                    self.output = None;
                    self.say(text, true);
                    return None;
                };
                if device.connected {
                    wait.stage = Stage::Waiting;
                    let text = format!("Waiting for the audio output of {}…", wait.name);
                    wait_for_output(
                        device.address.clone(),
                        vec![device.alias.clone(), device.name.clone()],
                        wait.found.clone(),
                        wake.clone(),
                    );
                    self.say(text, false);
                } else if view.finished.iter().any(|f| {
                    f.serial > after
                        && f.path == wait.device
                        && f.op == Op::Connect
                        && !f.error.is_empty()
                }) {
                    // (the connect's own result said why)
                    self.output = None;
                }
                None
            }
            Stage::Waiting => {
                let result = wait.found.lock().ok()?.take()?;
                let name = wait.name.clone();
                self.output = None;
                match result {
                    Ok((id, output)) => {
                        self.say(
                            format!("{name} is now this player's output ({output})."),
                            false,
                        );
                        Some(PlayerChange::Output(id))
                    }
                    Err(why) => {
                        self.say(format!("{name}: {why}"), true);
                        None
                    }
                }
            }
        }
    }

    fn use_as_output(&mut self, client: &Bluetooth, device: &Device, view: &BluetoothView) {
        let after = view.finished.last().map(|f| f.serial).unwrap_or(0);
        self.output = Some(OutputWait {
            device: device.path.clone(),
            name: device.display_name(),
            stage: Stage::Connecting { after },
            found: Arc::new(Mutex::new(None)),
        });
        if !device.connected {
            self.say(format!("Connecting to {}…", device.display_name()), false);
            client.act(Action::Connect {
                device: device.path.clone(),
            });
        }
    }
}

/// The tab; what it changed of the player.
pub fn show(
    ui: &mut Ui,
    page: &mut BluetoothPage,
    wake: impl Fn() + Send + Sync + 'static,
) -> Vec<PlayerChange> {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    let mut changes = Vec::new();
    if page.client.is_none() {
        let w = wake.clone();
        page.client = Some(Bluetooth::start(move || w()));
    }
    let Some(client) = page.client.take() else {
        return changes;
    };
    let view = client.view();
    for d in &view.devices {
        page.names.insert(d.path.clone(), d.display_name());
    }
    page.take_results(&view);
    if let Some(change) = page.follow_output(&view, &wake) {
        changes.push(change);
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| body(ui, page, &client, &view));
    dialogs(ui, page, &client, &view);
    page.client = Some(client);
    changes
}

fn body(ui: &mut Ui, page: &mut BluetoothPage, client: &Bluetooth, view: &BluetoothView) {
    ui.label(RichText::new("Bluetooth").strong().size(16.0));
    if view.status != Status::Ready {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if view.status == Status::Starting {
                ui.spinner();
            }
            ui.label(RichText::new(view.status.text()).color(MUTED));
        });
        message_line(ui, page);
        return;
    }
    // -- the adapter
    if page
        .adapter
        .as_ref()
        .is_none_or(|a| view.adapter(a).is_none())
    {
        page.adapter = view.adapters.first().map(|a| a.path.clone());
    }
    let Some(adapter) = page.adapter.clone().and_then(|a| view.adapter(&a).cloned()) else {
        return;
    };
    let adapter_busy = view.busy.get(&adapter.path).copied();
    let scanning = view.scans.contains(&adapter.path);
    ui.horizontal_wrapped(|ui| {
        if view.adapters.len() > 1 {
            let mut chosen = adapter.path.clone();
            ComboBox::from_id_salt("bt-adapter")
                .selected_text(adapter.display_name())
                .show_ui(ui, |ui| {
                    for a in &view.adapters {
                        ui.selectable_value(&mut chosen, a.path.clone(), a.display_name());
                    }
                })
                .response
                .accessible_name("Adapter");
            if chosen != adapter.path {
                page.adapter = Some(chosen);
            }
        } else {
            ui.label(RichText::new(adapter.display_name()).color(MUTED))
                .on_hover_text(&adapter.address);
        }
        let mut powered = adapter.powered;
        if ui
            .add_enabled(
                adapter_busy.is_none(),
                egui::Checkbox::new(&mut powered, "Powered"),
            )
            .changed()
        {
            client.act(Action::Power {
                adapter: adapter.path.clone(),
                on: powered,
            });
        }
        let transport = page.transport.get_or_insert(Transport::Auto);
        let label = |t: Transport| match t {
            Transport::Auto => "Classic + BLE",
            Transport::BrEdr => "Classic only",
            Transport::Le => "BLE only",
        };
        ComboBox::from_id_salt("bt-transport")
            .selected_text(label(*transport))
            .show_ui(ui, |ui| {
                for t in [Transport::Auto, Transport::BrEdr, Transport::Le] {
                    ui.selectable_value(transport, t, label(t));
                }
            })
            .response
            .accessible_name("What a scan looks for")
            .on_hover_text(
                "Classic (BR/EDR): speakers, headphones, phones. BLE: earbuds, watches, \
                 sensors, trackers.",
            );
        let transport = *transport;
        let scan_text = if scanning { "Stop scan" } else { "Scan" };
        if ui
            .add_enabled(
                adapter.powered && adapter_busy.is_none(),
                egui::Button::new(scan_text),
            )
            .clicked()
        {
            client.act(if scanning {
                Action::StopScan {
                    adapter: adapter.path.clone(),
                }
            } else {
                Action::Scan {
                    adapter: adapter.path.clone(),
                    transport,
                }
            });
        }
        if scanning || adapter_busy.is_some() {
            ui.spinner();
        }
    });
    if !adapter.powered {
        ui.label(
            RichText::new("The adapter is off: switch it on to scan and connect.").color(MUTED),
        );
    } else if adapter.discovering && !scanning {
        ui.label(
            RichText::new("Another program is scanning; Scan starts this player's own scan.")
                .color(MUTED),
        );
    }
    ui.add_space(4.0);
    // -- the filter
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut page.filter)
                .hint_text("Filter by name, address or type")
                .desired_width(200.0),
        )
        .accessible_name("Filter devices");
        ui.checkbox(&mut page.named_only, "Named only")
            .on_hover_text("Hide devices that did not tell their name (many BLE beacons).");
        ui.checkbox(&mut page.audio_only, "Audio only")
            .on_hover_text("Only speakers, headphones and headsets.");
    });
    let needle = page.filter.trim().to_lowercase();
    let devices: Vec<&Device> = view
        .devices
        .iter()
        .filter(|d| d.adapter == adapter.path)
        .filter(|d| !page.named_only || d.named())
        .filter(|d| !page.audio_only || d.is_audio_output())
        .filter(|d| {
            needle.is_empty()
                || [d.display_name(), d.address.clone(), d.kind().to_owned()]
                    .iter()
                    .any(|s| s.to_lowercase().contains(&needle))
        })
        .collect();
    let mine = view
        .devices
        .iter()
        .filter(|d| d.adapter == adapter.path)
        .count();
    ui.label(
        RichText::new(if devices.len() < mine {
            format!("{} of {mine} devices", devices.len())
        } else if mine == 1 {
            "1 device".to_owned()
        } else {
            format!("{mine} devices")
        })
        .color(MUTED)
        .small(),
    );
    // -- the devices
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(120.0);
        ui.set_width(ui.available_width());
        if devices.is_empty() {
            ui.label(
                RichText::new(if adapter.powered {
                    "No devices - Scan looks for ones nearby (put a new one into pairing mode first)."
                } else {
                    "No devices."
                })
                .color(MUTED),
            );
        }
        for d in &devices {
            device_row(ui, page, view, d);
        }
    });
    // -- what can be done with the one selected
    if let Some(device) = page
        .selected
        .as_ref()
        .and_then(|p| view.device(p))
        .filter(|d| d.adapter == adapter.path)
        .cloned()
    {
        actions(ui, page, client, view, &device, adapter.powered);
    }
    message_line(ui, page);
}

fn device_row(ui: &mut Ui, page: &mut BluetoothPage, view: &BluetoothView, d: &Device) {
    let selected = page.selected.as_deref() == Some(d.path.as_str());
    let mut detail = vec![d.kind().to_owned(), d.address.clone()];
    if let Some(b) = d.battery {
        detail.push(format!("battery {b} %"));
    }
    if let Some(r) = d.rssi {
        detail.push(format!("{r} dBm"));
    }
    let state = state_words(d);
    if !state.is_empty() {
        detail.push(state);
    }
    if let Some(op) = view.busy.get(&d.path) {
        detail.push(format!("{}…", op.as_str()));
    }
    // a row of the list: the name, and below it what it is - left-aligned,
    // the whole row a click target, read out by its name
    let style = ui.style().clone();
    let width = ui.available_width();
    let pad = egui::vec2(8.0, 4.0);
    let name_galley = ui.painter().layout(
        d.display_name(),
        egui::TextStyle::Body.resolve(&style),
        if d.connected {
            GOOD
        } else {
            style.visuals.strong_text_color()
        },
        width - 2.0 * pad.x,
    );
    let detail_galley = ui.painter().layout(
        detail.join(" · "),
        egui::TextStyle::Small.resolve(&style),
        MUTED,
        width - 2.0 * pad.x,
    );
    let height = name_galley.size().y + detail_galley.size().y + 2.0 * pad.y + 1.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let name = d.display_name();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }
        let top = rect.min + pad;
        let name_height = name_galley.size().y;
        ui.painter().galley(top, name_galley, Color32::PLACEHOLDER);
        ui.painter().galley(
            top + egui::vec2(0.0, name_height + 1.0),
            detail_galley,
            Color32::PLACEHOLDER,
        );
    }
    if response.clicked() {
        page.selected = Some(d.path.clone());
    }
}

fn actions(
    ui: &mut Ui,
    page: &mut BluetoothPage,
    client: &Bluetooth,
    view: &BluetoothView,
    d: &Device,
    powered: bool,
) {
    let busy = view.busy.get(&d.path).copied();
    let free = busy.is_none() && powered;
    let waiting = page.output.as_ref().is_some_and(|w| w.device == d.path);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if busy == Some(Op::Pair) {
            if ui.button("Cancel pairing").clicked() {
                client.act(Action::CancelPairing {
                    device: d.path.clone(),
                });
            }
        } else if ui
            .add_enabled(free && !d.paired, egui::Button::new("Pair"))
            .clicked()
        {
            client.act(Action::Pair {
                device: d.path.clone(),
            });
        }
        let trust = if d.trusted { "Untrust" } else { "Trust" };
        if ui
            .add_enabled(busy.is_none(), egui::Button::new(trust))
            .on_hover_text("A trusted device may connect by itself.")
            .clicked()
        {
            client.act(Action::Trust {
                device: d.path.clone(),
                on: !d.trusted,
            });
        }
        if ui
            .add_enabled(free && !d.connected, egui::Button::new("Connect"))
            .clicked()
        {
            client.act(Action::Connect {
                device: d.path.clone(),
            });
        }
        if ui
            .add_enabled(
                busy.is_none() && d.connected,
                egui::Button::new("Disconnect"),
            )
            .clicked()
        {
            client.act(Action::Disconnect {
                device: d.path.clone(),
            });
        }
        if ui
            .add_enabled(busy.is_none(), egui::Button::new("Forget…"))
            .on_hover_text("Remove the device: it has to be paired again to be used.")
            .clicked()
        {
            page.confirm_forget = Some(d.path.clone());
        }
        if d.is_audio_output()
            && ui
                .add_enabled(
                    (free || (d.connected && busy.is_none())) && !waiting,
                    egui::Button::new("🔊 Use as audio output"),
                )
                .on_hover_text(
                    "Connect it if needed and make it this player's output once the sound \
                     server has made one for it.",
                )
                .accessible_name("Use as audio output")
                .clicked()
        {
            page.use_as_output(client, d, view);
        }
        if busy.is_some() || waiting {
            ui.spinner();
        }
    });
}

fn message_line(ui: &mut Ui, page: &BluetoothPage) {
    if !page.message.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(&page.message).color(if page.message_bad { BAD } else { MUTED }));
    }
}

fn dialogs(ui: &mut Ui, page: &mut BluetoothPage, client: &Bluetooth, view: &BluetoothView) {
    // a question of a pairing (the oldest first)
    page.dismissed
        .retain(|t| view.requests.iter().any(|q| q.token == *t));
    if let Some(q) = view
        .requests
        .iter()
        .find(|q| !page.dismissed.contains(&q.token))
        .cloned()
    {
        if page.typed_for != q.token {
            page.typed_for = q.token;
            page.typed.clear();
        }
        let modal = Modal::new(Id::unique(("bt-agent", q.token))).show(ui.ctx(), |ui| {
            ui.set_max_width(380.0);
            ui.label(RichText::new("Bluetooth pairing").strong().size(15.0));
            ui.add_space(6.0);
            ui.label(question(&q));
            let typing = matches!(q.kind, AgentKind::Pin | AgentKind::Passkey);
            let mut enter = false;
            if typing {
                let (limit, what) = if q.kind == AgentKind::Pin {
                    (16, "PIN")
                } else {
                    (6, "Passkey")
                };
                let edit = ui
                    .add(
                        egui::TextEdit::singleline(&mut page.typed)
                            .char_limit(limit)
                            .desired_width(160.0),
                    )
                    .accessible_name(what);
                if q.kind == AgentKind::Passkey {
                    page.typed.retain(|c| c.is_ascii_digit());
                }
                enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                edit.request_focus();
            }
            ui.add_space(8.0);
            let mut decided: Option<Answer> = None;
            ui.horizontal(|ui| {
                if q.needs_answer() {
                    let accept = match q.kind {
                        AgentKind::Confirm => "Confirm",
                        AgentKind::Pin => "Send PIN",
                        AgentKind::Passkey => "Send passkey",
                        _ => "Allow",
                    };
                    let ready = !typing
                        || (!page.typed.trim().is_empty()
                            && (q.kind != AgentKind::Passkey || page.typed.len() <= 6));
                    if ui.add_enabled(ready, egui::Button::new(accept)).clicked()
                        || (enter && ready)
                    {
                        decided = Some(Answer::Accept(page.typed.clone()));
                    }
                    if ui.button("Reject").clicked() {
                        decided = Some(Answer::Reject);
                    }
                } else if ui.button("Close").clicked() {
                    decided = Some(Answer::Reject);
                }
            });
            decided
        });
        let closed = modal.should_close();
        let decided = modal.inner.or(closed.then_some(Answer::Reject));
        if let Some(answer) = decided {
            if q.needs_answer() {
                client.answer(q.token, answer);
            } else {
                page.dismissed.push(q.token);
            }
            page.typed.clear();
        }
    }
    // forget a device?
    if let Some(path) = page.confirm_forget.clone() {
        let name = page.name(&path);
        let modal = Modal::new(Id::unique("bt-forget")).show(ui.ctx(), |ui| {
            ui.set_max_width(360.0);
            ui.label(format!(
                "Forget {name}? It has to be paired again before it can be used."
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Forget").clicked() {
                    return Some(true);
                }
                ui.button("Keep").clicked().then_some(false)
            })
            .inner
        });
        let closed = modal.should_close();
        match modal.inner.or(closed.then_some(false)) {
            Some(true) => {
                client.act(Action::Remove { device: path });
                page.confirm_forget = None;
            }
            Some(false) => page.confirm_forget = None,
            None => {}
        }
    }
}
