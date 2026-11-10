//! Optional `qmd` search, run off the UI thread, for notes outside the watched folders.

use crate::qmd_ops::{self, SearchHit};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

const TYPING_PAUSE: Duration = Duration::from_millis(400);
const QMD_TIMEOUT: Duration = Duration::from_secs(20);

enum Outcome {
    Hits(Vec<SearchHit>),
    Unavailable,
    Failed(String),
}

pub struct QmdSettings {
    pub qmd: PathBuf,
    pub limit: usize,
    pub collection: Option<String>,
}

#[derive(Default)]
pub struct ExtraSearch {
    wanted: String,
    sent: String,
    changed_at: Option<Instant>,
    receiver: Option<Receiver<(String, Outcome)>>,
    hits_query: String,
    hits: Vec<SearchHit>,
    pub failure: Option<String>,
    is_unavailable: bool,
}

impl ExtraSearch {
    pub fn set_query(&mut self, query: &str) {
        let query = query.trim();
        if query != self.wanted {
            self.wanted = query.to_string();
            self.changed_at = Some(Instant::now());
        }
    }

    pub fn is_busy(&self) -> bool {
        self.receiver.is_some() || self.wanted != self.sent
    }

    /// Hits for the current query; empty while typing or when qmd is missing.
    pub fn hits(&self) -> &[SearchHit] {
        if self.hits_query == self.wanted {
            &self.hits
        } else {
            &[]
        }
    }

    pub fn tick(&mut self, settings: impl FnOnce() -> QmdSettings) {
        self.receive();
        let paused = self.changed_at.is_some_and(|at| at.elapsed() >= TYPING_PAUSE);
        if self.is_unavailable {
            self.sent = self.wanted.clone();
            return;
        }
        if self.receiver.is_some() || self.wanted == self.sent || !paused {
            return;
        }
        self.sent = self.wanted.clone();
        if self.sent.is_empty() {
            self.hits.clear();
            self.failure = None;
            return;
        }
        self.receiver = Some(spawn_search(self.sent.clone(), settings()));
    }

    fn receive(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok((query, outcome)) => {
                self.receiver = None;
                self.apply(query, outcome);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.receiver = None,
        }
    }

    fn apply(&mut self, query: String, outcome: Outcome) {
        self.hits_query = query;
        self.failure = None;
        self.hits.clear();
        match outcome {
            Outcome::Hits(hits) => self.hits = hits,
            Outcome::Unavailable => self.is_unavailable = true,
            Outcome::Failed(message) => self.failure = Some(message),
        }
    }
}

fn spawn_search(query: String, settings: QmdSettings) -> Receiver<(String, Outcome)> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let found = qmd_ops::search(
            &settings.qmd,
            &query,
            settings.limit,
            settings.collection.as_deref(),
            QMD_TIMEOUT,
        );
        let outcome = match found {
            Ok(hits) => Outcome::Hits(hits),
            Err(err) if is_not_installed(&err) => Outcome::Unavailable,
            Err(err) => Outcome::Failed(format!("{err:#}")),
        };
        let _ = sender.send((query, outcome));
    });
    receiver
}

fn is_not_installed(err: &anyhow::Error) -> bool {
    err.root_cause()
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
}
