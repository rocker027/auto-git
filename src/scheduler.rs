use crate::config::AppConfig;
use crate::git_ops::{self, CommitOutcome};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

/// What happened to one folder during a backup run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderBackup {
    Done(CommitOutcome),
    Missing,
    Failed(String),
}

/// Result of the latest backup run over every watched folder.
#[derive(Debug, Clone)]
pub struct BackupReport {
    /// A save after this moment may not be in the run's commits.
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub folders: Vec<(PathBuf, FolderBackup)>,
    /// Newest commit over every watched folder, from before or during this run.
    pub last_commit_at: Option<SystemTime>,
}

#[derive(Debug, Default)]
struct SchedulerState {
    report: Option<BackupReport>,
    is_running: bool,
}

/// Background thread that commits every watched folder on an interval,
/// or right away when asked. It never runs while the editor has unsaved text.
pub struct Scheduler {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    run_now: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    state: Arc<Mutex<SchedulerState>>,
}

struct Shared {
    config: Arc<Mutex<AppConfig>>,
    dirty_editor: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    run_now: Arc<AtomicBool>,
    state: Arc<Mutex<SchedulerState>>,
}

impl Scheduler {
    pub fn start(config: Arc<Mutex<AppConfig>>, dirty_editor: Arc<AtomicBool>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let run_now = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(SchedulerState::default()));
        let shared = Shared {
            config,
            dirty_editor,
            stop: stop.clone(),
            pause: pause.clone(),
            run_now: run_now.clone(),
            state: state.clone(),
        };
        let handle = thread::spawn(move || run_loop(shared));
        Self {
            stop,
            pause,
            run_now,
            handle: Some(handle),
            state,
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    /// Ask for a backup on the next loop turn, even while paused.
    pub fn request_now(&self) {
        self.run_now.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.state.lock().map(|s| s.is_running).unwrap_or(false)
    }

    pub fn last_report(&self) -> Option<BackupReport> {
        self.state.lock().ok().and_then(|s| s.report.clone())
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

fn run_loop(shared: Shared) {
    let mut last_run = Instant::now() - Duration::from_secs(3600);
    while !shared.stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(500));
        if shared.dirty_editor.load(Ordering::Relaxed) {
            continue;
        }
        // swap, not load + store: a request that lands mid-run must survive for the next turn.
        let wanted_now = shared.run_now.swap(false, Ordering::Relaxed);
        if shared.pause.load(Ordering::Relaxed) && !wanted_now {
            continue;
        }
        let Some((interval, folders, git, skip)) = read_config(&shared.config) else {
            shared.run_now.fetch_or(wanted_now, Ordering::Relaxed);
            continue;
        };
        if !wanted_now && last_run.elapsed() < interval {
            continue;
        }
        last_run = Instant::now();
        set_running(&shared.state, true);
        let report = back_up_folders(&git, &folders, skip);
        if let Ok(mut st) = shared.state.lock() {
            st.report = Some(report);
            st.is_running = false;
        }
    }
}

fn read_config(config: &Mutex<AppConfig>) -> Option<(Duration, Vec<PathBuf>, PathBuf, bool)> {
    let cfg = config.lock().ok()?;
    Some((
        cfg.check_interval(),
        cfg.folders.clone(),
        cfg.git_path.clone(),
        cfg.skip_if_staged,
    ))
}

fn set_running(state: &Mutex<SchedulerState>, running: bool) {
    if let Ok(mut st) = state.lock() {
        st.is_running = running;
    }
}

pub fn back_up_folders(git: &Path, folders: &[PathBuf], skip_if_staged: bool) -> BackupReport {
    let started_at = SystemTime::now();
    let results: Vec<(PathBuf, FolderBackup)> = folders
        .iter()
        .map(|folder| (folder.clone(), back_up_folder(git, folder, skip_if_staged)))
        .collect();
    let last_commit_at = results
        .iter()
        // A folder inside another repo would report that repo's commits.
        .filter(|(_, result)| {
            matches!(result, FolderBackup::Done(outcome)
                if !matches!(outcome, CommitOutcome::SkippedNotRepoRoot(_)))
        })
        .filter_map(|(folder, _)| git_ops::last_commit_time(git, folder))
        .max();
    BackupReport {
        started_at,
        finished_at: SystemTime::now(),
        folders: results,
        last_commit_at,
    }
}

fn back_up_folder(git: &Path, folder: &Path, skip_if_staged: bool) -> FolderBackup {
    if !folder.exists() {
        return FolderBackup::Missing;
    }
    match git_ops::auto_commit(git, folder, skip_if_staged) {
        Ok(outcome) => FolderBackup::Done(outcome),
        Err(err) => FolderBackup::Failed(format!("{err:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::status::{backup_status, BackupState, Tone};
    use std::fs;
    use tempfile::tempdir;

    fn git(repo: &Path, args: &[&str]) {
        let out = git_ops::run_git(Path::new("git"), repo, args).unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn new_repo(dir: &Path) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    fn status_text(report: &BackupReport) -> (Tone, String) {
        let state = BackupState {
            report: Some(report),
            is_running: false,
            is_paused: false,
            editor_dirty: false,
            last_saved_at: None,
        };
        let line = backup_status(&state, SystemTime::now());
        (line.tone, line.text)
    }

    #[test]
    fn backup_run_to_status_line() {
        let dir = tempdir().unwrap();
        let notes = dir.path().join("notes");
        fs::create_dir(&notes).unwrap();
        new_repo(&notes);
        fs::write(notes.join("a.md"), "# a\n").unwrap();

        let report = back_up_folders(Path::new("git"), std::slice::from_ref(&notes), true);
        assert!(report.last_commit_at.is_some());
        assert!(report.started_at <= report.finished_at);
        assert_eq!(status_text(&report), (Tone::Good, "✓ 已備份 · 剛剛".to_string()));

        let inner = notes.join("inner");
        fs::create_dir(&inner).unwrap();
        fs::write(inner.join("b.md"), "# b\n").unwrap();
        let report = back_up_folders(Path::new("git"), &[inner], true);
        assert_eq!(report.last_commit_at, None, "skipped folders add no version time");
        assert_eq!(
            status_text(&report),
            (Tone::Problem, "⚠ 「inner」在另一個 Git 專案裡，先不備份".to_string())
        );
    }

    #[test]
    fn missing_folder_is_reported() {
        let report = back_up_folders(Path::new("git"), &[PathBuf::from("/no/such/folder")], true);
        assert_eq!(report.folders[0].1, FolderBackup::Missing);
    }
}
