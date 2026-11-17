//! The files written on a thread of their own: the window never waits for a
//! disk (an fsync, a full disk, a network home folder). What is asked again
//! before it was written is written once, the newest; the player's state is
//! merged with what another program wrote meanwhile
//! ([`crate::state::write_state_merged`]).

use crate::state::{self, AmpSettings, EqSettings, PlayerState};
use crossbeam_channel::{Receiver, Sender};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// What is to be written.
pub enum Job {
    /// The player's state, and the playlist's version it was taken at.
    State(PlayerState, u64),
    Eq(EqSettings),
    Amp(AmpSettings),
    /// The output device chosen (`amp_output` in audio.json).
    Output(String),
    /// The cover shown to the desktop (a PNG; None: none) - written where
    /// MPRIS points to it (Linux).
    Cover(Option<Vec<u8>>),
}

enum Message {
    Job(Box<Job>),
    /// Answered when everything asked before is written.
    Flush(Sender<()>),
}

pub struct Saver {
    tx: Sender<Message>,
    errors: Receiver<String>,
    writes: Arc<AtomicUsize>,
    adopted: Receiver<(Map<String, Value>, u64)>,
    art: Receiver<String>,
}

/// The newest of each kind, waiting to be written.
#[derive(Default)]
struct Pending {
    state: Option<(PlayerState, u64)>,
    eq: Option<EqSettings>,
    amp: Option<AmpSettings>,
    output: Option<String>,
    cover: Option<Option<Vec<u8>>>,
}

impl Saver {
    /// Write into `dir` (axross's configuration folder); `base` is the
    /// player's file as it was read at the start.
    pub fn start(dir: PathBuf, base: Map<String, Value>) -> Saver {
        let (tx, rx) = crossbeam_channel::unbounded::<Message>();
        let (etx, errors) = crossbeam_channel::unbounded();
        let (atx, adopted) = crossbeam_channel::unbounded();
        let (art_tx, art) = crossbeam_channel::unbounded();
        let writes = Arc::new(AtomicUsize::new(0));
        let w = writes.clone();
        let _ = std::thread::Builder::new()
            .name("amp-saver".into())
            .spawn(move || {
                let mut base = Some(base);
                #[cfg(target_os = "linux")]
                let mut covers = crate::mpris::Covers::default();
                while let Ok(first) = rx.recv() {
                    let mut pending = Pending::default();
                    let mut flushes = Vec::new();
                    for message in std::iter::once(first).chain(rx.try_iter()) {
                        match message {
                            Message::Job(job) => match *job {
                                Job::State(s, v) => pending.state = Some((s, v)),
                                Job::Eq(e) => pending.eq = Some(e),
                                Job::Amp(a) => pending.amp = Some(a),
                                Job::Output(o) => pending.output = Some(o),
                                Job::Cover(c) => pending.cover = Some(c),
                            },
                            Message::Flush(ack) => flushes.push(ack),
                        }
                    }
                    let fail = |what: &str, e: std::io::Error| {
                        let _ = etx.send(format!("Cannot save {what}: {e}"));
                    };
                    if let Some((s, version)) = pending.state {
                        w.fetch_add(1, Ordering::Relaxed);
                        match state::write_state_merged(&dir.join(state::STATE_FILE), &s, &mut base)
                        {
                            // another program's changes kept: the window takes them over
                            Ok(kept) if !kept.is_empty() => {
                                let _ = atx.send((kept, version));
                            }
                            Ok(_) => {}
                            Err(e) => fail("the player state", e),
                        }
                    }
                    if let Some(e) = pending.eq {
                        w.fetch_add(1, Ordering::Relaxed);
                        if let Err(err) = state::write_eq(&dir.join(state::AUDIO_FILE), &e) {
                            fail("the equalizer", err);
                        }
                    }
                    if let Some(o) = pending.output {
                        w.fetch_add(1, Ordering::Relaxed);
                        if let Err(err) = state::write_output(&dir.join(state::AUDIO_FILE), &o) {
                            fail("the output", err);
                        }
                    }
                    if let Some(a) = pending.amp {
                        w.fetch_add(1, Ordering::Relaxed);
                        if let Err(err) = state::write_amp(&dir.join(state::AMP_FILE), &a) {
                            fail("the settings", err);
                        }
                    }
                    if let Some(cover) = pending.cover {
                        #[cfg(target_os = "linux")]
                        let _ = art_tx.send(covers.show(cover.as_deref()));
                        #[cfg(not(target_os = "linux"))]
                        let _ = (cover, &art_tx);
                    }
                    for ack in flushes {
                        let _ = ack.send(());
                    }
                }
            });
        Saver {
            tx,
            errors,
            writes,
            adopted,
            art,
        }
    }

    pub fn save(&self, job: Job) {
        let _ = self.tx.send(Message::Job(Box::new(job)));
    }

    /// Wait (at most `limit`) until everything asked is written.
    pub fn flush(&self, limit: Duration) {
        let (ack, done) = crossbeam_channel::bounded(1);
        if self.tx.send(Message::Flush(ack)).is_ok() {
            let _ = done.recv_timeout(limit);
        }
    }

    /// What could not be written since the last look.
    pub fn errors(&self) -> Vec<String> {
        self.errors.try_iter().collect()
    }

    /// What another program wrote and the merge kept (the keys and their
    /// values), with the playlist's version the state was taken at.
    pub fn adopted(&self) -> Vec<(Map<String, Value>, u64)> {
        self.adopted.try_iter().collect()
    }

    /// The cover's file:// URL, when it changed ("" none).
    pub fn art_url(&self) -> Option<String> {
        self.art.try_iter().last()
    }

    /// Files written so far (tests).
    pub fn writes(&self) -> usize {
        self.writes.load(Ordering::Relaxed)
    }
}
