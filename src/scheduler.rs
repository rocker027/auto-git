use crate::config::AppConfig;
use crate::git_ops::{self, CommitOutcome};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct SchedulerStatus {
    pub last_message: String,
    pub last_tick: Option<Instant>,
}

pub struct Scheduler {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    status: Arc<Mutex<SchedulerStatus>>,
}

impl Scheduler {
    pub fn start(
        config: Arc<Mutex<AppConfig>>,
        dirty_editor: Arc<AtomicBool>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(SchedulerStatus::default()));

        let stop_c = stop.clone();
        let pause_c = pause.clone();
        let status_c = status.clone();
        let dirty_c = dirty_editor.clone();

        let handle = thread::spawn(move || {
            let mut last_run = Instant::now() - Duration::from_secs(3600);
            while !stop_c.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(500));
                if pause_c.load(Ordering::Relaxed) {
                    continue;
                }
                if dirty_c.load(Ordering::Relaxed) {
                    continue;
                }
                let (interval, folders, git, skip) = {
                    let Ok(cfg) = config.lock() else { continue };
                    (
                        cfg.check_interval(),
                        cfg.folders.clone(),
                        cfg.git_path.clone(),
                        cfg.skip_if_staged,
                    )
                };
                if last_run.elapsed() < interval {
                    continue;
                }
                last_run = Instant::now();
                let msg = tick_folders(&git, &folders, skip);
                if let Ok(mut st) = status_c.lock() {
                    st.last_message = msg;
                    st.last_tick = Some(Instant::now());
                }
            }
        });

        Self {
            stop,
            pause,
            handle: Some(handle),
            status,
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    pub fn status_message(&self) -> String {
        self.status
            .lock()
            .map(|s| s.last_message.clone())
            .unwrap_or_default()
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn tick_folders(git: &PathBuf, folders: &[PathBuf], skip_if_staged: bool) -> String {
    if folders.is_empty() {
        return "no folders configured".into();
    }
    let mut parts = Vec::new();
    for folder in folders {
        if !folder.exists() {
            parts.push(format!("{}: missing", folder.display()));
            continue;
        }
        match git_ops::auto_commit(git, folder, skip_if_staged) {
            Ok(CommitOutcome::Committed { files, message }) => {
                parts.push(format!("{}: {message} ({files})", folder.display()));
            }
            Ok(CommitOutcome::SkippedNoChanges) => {
                parts.push(format!("{}: no changes", folder.display()));
            }
            Ok(CommitOutcome::SkippedStagedPresent) => {
                parts.push(format!("{}: skipped (staged)", folder.display()));
            }
            Ok(CommitOutcome::SkippedBusyState(s)) => {
                parts.push(format!("{}: skipped ({s})", folder.display()));
            }
            Err(e) => parts.push(format!("{}: error {e}", folder.display())),
        }
    }
    parts.join(" | ")
}
