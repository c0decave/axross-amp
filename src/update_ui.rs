//! The Updates window (*Options › Check for updates…*): the check, the
//! download with a plain bar, *Restart now* - the work on threads of its
//! own (src/update.rs), the window calm: no spinner, no blinking, the bar
//! fills and nothing else moves.

use crate::update::{self, Install, Installed, Release, Version, Web};
use egui::{RichText, Ui};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, channel};

#[derive(Clone, Debug)]
pub enum Stage {
    Idle,
    Checking,
    UpToDate(Version),
    Available(Release),
    Downloading(Version),
    Ready(Installed),
    /// Checked, but the program's folder cannot be written: the file there.
    Saved(PathBuf, Version),
    Failed(String),
}

enum Done {
    Checked(Result<Release, String>),
    Downloaded(Result<Result<Installed, (PathBuf, Version)>, String>),
}

pub struct Updater {
    pub open: bool,
    pub stage: Stage,
    install: Install,
    rx: Option<Receiver<Done>>,
    cancel: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    /// The check at the start found something (for the status line).
    notice: Option<String>,
    quiet: bool,
    cleaned: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Updater::new(Install::current())
    }
}

impl Updater {
    pub fn new(install: Install) -> Updater {
        Updater {
            open: false,
            stage: Stage::Idle,
            install,
            rx: None,
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            notice: None,
            quiet: false,
            cleaned: false,
        }
    }

    /// The check at the start, when the setting asks for it.
    pub fn start(&mut self, ctx: &egui::Context, setting: bool) {
        let env = std::env::var(update::NO_CHECK_ENV).ok();
        if update::start_check_allowed(setting, env.as_deref()) && self.install.can_update() {
            self.quiet = true;
            self.check(ctx);
        }
    }

    /// What the start check found, once.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    fn busy(&self) -> bool {
        matches!(self.stage, Stage::Checking | Stage::Downloading(_))
    }

    pub fn check(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        self.stage = Stage::Checking;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Done::Checked(update::latest(&Web::default())));
            ctx.request_repaint();
        });
    }

    fn download(&mut self, ctx: &egui::Context, release: Release) {
        self.stage = Stage::Downloading(release.version);
        self.cancel.store(false, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let (install, cancel, done, total) = (
            self.install.clone(),
            self.cancel.clone(),
            self.done.clone(),
            self.total.clone(),
        );
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut last = 0u64;
            let result = update::download(
                &release,
                &install,
                Version::current(),
                &Web::default(),
                update::TRUSTED_KEYS,
                &update::cache_dir(),
                &mut |d, t| {
                    done.store(d, Ordering::Relaxed);
                    total.store(t, Ordering::Relaxed);
                    // (a repaint per half per cent at most: the bar fills calmly)
                    if t > 0 && (d - last) * 200 >= t {
                        last = d;
                        ctx.request_repaint();
                    }
                },
                &cancel,
            )
            .and_then(|got| {
                if got.in_place {
                    update::install(&got).map(Ok)
                } else {
                    Ok(Err((got.path, got.version)))
                }
            });
            let _ = tx.send(Done::Downloaded(result));
            ctx.request_repaint();
        });
    }

    /// What the threads said.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let Ok(done) = rx.try_recv() else { return };
        self.rx = None;
        let quiet = std::mem::take(&mut self.quiet);
        self.stage = match done {
            Done::Checked(Ok(release)) if release.version > Version::current() => {
                if quiet {
                    self.notice = Some(format!(
                        "axross-amp {} is out: Options › Check for updates…",
                        release.version
                    ));
                }
                Stage::Available(release)
            }
            Done::Checked(Ok(release)) => Stage::UpToDate(release.version),
            Done::Checked(Err(e)) if quiet => {
                eprintln!("axross-amp: the update check failed: {e}");
                Stage::Idle
            }
            Done::Checked(Err(e)) => Stage::Failed(e),
            Done::Downloaded(Ok(Ok(installed))) => Stage::Ready(installed),
            Done::Downloaded(Ok(Err((path, version)))) => Stage::Saved(path, version),
            Done::Downloaded(Err(e)) => Stage::Failed(e),
        };
    }

    /// The menu's entries; true when the setting changed.
    pub fn menu(&mut self, ui: &mut Ui, check_at_start: &mut bool) -> bool {
        if ui.button("Check for updates…").clicked() {
            self.open = true;
            if !self.busy() && self.install.can_update() {
                self.check(ui.ctx());
            }
            ui.close();
        }
        ui.checkbox(check_at_start, "Check for updates at start")
            .on_hover_text("One request to GitHub when the player starts; off by default")
            .changed()
    }

    /// The window.
    pub fn window(&mut self, ctx: &egui::Context, check_at_start: &mut bool) -> bool {
        if !self.cleaned {
            // (this program has started: the one it replaced may go)
            self.cleaned = true;
            if self.install.can_update() {
                update::cleanup_previous(&self.install.target);
            }
        }
        self.poll();
        if !self.open {
            return false;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Updates")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(format!("This is axross-amp {}.", Version::current()));
                if let Some(why) = self.install.why_not() {
                    ui.label(format!("Not updated here: {why}."));
                    ui.hyperlink_to("The releases on GitHub", update::RELEASES_PAGE);
                    return;
                }
                ui.add_space(6.0);
                self.stage_ui(ui);
                ui.add_space(6.0);
                changed = ui
                    .checkbox(check_at_start, "Check for updates at start")
                    .changed();
            });
        if !open {
            self.open = false;
            if matches!(self.stage, Stage::Downloading(_)) {
                self.cancel.store(true, Ordering::Relaxed);
            }
        }
        changed
    }

    fn stage_ui(&mut self, ui: &mut Ui) {
        match self.stage.clone() {
            Stage::Idle | Stage::Failed(_) | Stage::UpToDate(_) => {
                match &self.stage {
                    Stage::UpToDate(v) => {
                        ui.label(format!("The latest release is {v}: nothing to update."));
                    }
                    Stage::Failed(e) => {
                        ui.label(RichText::new(format!("The update failed: {e}")).strong());
                        ui.label("The program is as it was.");
                    }
                    _ => {}
                }
                if ui.button("Check now").clicked() {
                    self.check(ui.ctx());
                }
            }
            Stage::Checking => {
                ui.label("Asking GitHub for the latest release…");
            }
            Stage::Available(release) => {
                ui.label(RichText::new(format!("axross-amp {} is out.", release.version)).strong());
                if !release.notes.is_empty() {
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| ui.label(&release.notes));
                }
                ui.label(
                    "Download and install fetches it, checks its signature and checksum and \
                     puts it in place of this program; it takes effect when the player starts \
                     again.",
                );
                ui.horizontal(|ui| {
                    if ui.button("Download and install").clicked() {
                        self.download(ui.ctx(), release.clone());
                    }
                    ui.hyperlink_to("The release on GitHub", release.page());
                });
            }
            Stage::Downloading(version) => {
                let (done, total) = (
                    self.done.load(Ordering::Relaxed),
                    self.total.load(Ordering::Relaxed),
                );
                ui.label(format!("Downloading axross-amp {version}…"));
                let part = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(part).text(format!(
                    "{} of {} MB",
                    done >> 20,
                    total >> 20
                )));
                if ui.button("Cancel").clicked() {
                    self.cancel.store(true, Ordering::Relaxed);
                }
            }
            Stage::Ready(installed) => {
                ui.label(format!(
                    "axross-amp {} is in place; it runs from the next start on.",
                    installed.version
                ));
                if ui.button("Restart now").clicked() {
                    match update::restart(&installed.path, &installed.sha256) {
                        Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.stage = Stage::Failed(e),
                    }
                }
            }
            Stage::Saved(path, version) => {
                ui.label(format!(
                    "axross-amp {version} is checked, but this program's folder cannot be \
                     written. The new program is {}: put it in place of this one.",
                    path.display()
                ));
            }
        }
    }
}
