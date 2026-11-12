use crate::config::AppConfig;
use crate::git_ops::{self, CommitOutcome};
use crate::git_push::{self, PushOutcome, PUSH_TIMEOUT};
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

/// What happened when one backed-up folder was pushed to its upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderPush {
    Done(PushOutcome),
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
    /// Empty while auto push is off, and until this run's pushes have ended.
    pub pushes: Vec<(PathBuf, FolderPush)>,
}

#[derive(Debug, Default)]
struct SchedulerState {
    report: Option<BackupReport>,
    is_running: bool,
    is_pushing: bool,
}

/// What one run reads from the config.
struct RunSettings {
    interval: Duration,
    folders: Vec<PathBuf>,
    git: PathBuf,
    skip_if_staged: bool,
    auto_push: bool,
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

    /// True while commits are sent to the remote. The folders are no longer
    /// being read then, so saving a note is safe.
    pub fn is_pushing(&self) -> bool {
        self.state.lock().map(|s| s.is_pushing).unwrap_or(false)
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
        let Some(settings) = read_config(&shared.config) else {
            shared.run_now.fetch_or(wanted_now, Ordering::Relaxed);
            continue;
        };
        if !wanted_now && last_run.elapsed() < settings.interval {
            continue;
        }
        last_run = Instant::now();
        run_backup(&shared.state, &settings, &|folder| is_push_wanted(&shared.config, folder));
    }
}

/// Asked again before each folder: auto push may have been turned off, or the
/// folder removed, while an earlier folder was being pushed.
fn is_push_wanted(config: &Mutex<AppConfig>, folder: &Path) -> bool {
    config
        .lock()
        .map(|cfg| cfg.auto_push && cfg.folders.iter().any(|watched| watched == folder))
        .unwrap_or(false)
}

/// Commits first and publishes that result, then pushes. The push can take
/// long, and a note may be saved again while it runs.
fn run_backup(
    state: &Mutex<SchedulerState>,
    settings: &RunSettings,
    is_push_wanted: &dyn Fn(&Path) -> bool,
) {
    set_running(state, true);
    let report = back_up_folders(&settings.git, &settings.folders, settings.skip_if_staged);
    let to_push = if settings.auto_push { pushable_folders(&report) } else { Vec::new() };
    if let Ok(mut st) = state.lock() {
        st.report = Some(report);
        st.is_running = false;
        st.is_pushing = !to_push.is_empty();
    }
    if to_push.is_empty() {
        return;
    }
    let pushes = push_folders(&settings.git, &to_push, PUSH_TIMEOUT, is_push_wanted);
    if let Ok(mut st) = state.lock() {
        if let Some(report) = st.report.as_mut() {
            report.pushes = pushes;
        }
        st.is_pushing = false;
    }
}

fn read_config(config: &Mutex<AppConfig>) -> Option<RunSettings> {
    let cfg = config.lock().ok()?;
    Some(RunSettings {
        interval: cfg.check_interval(),
        folders: cfg.folders.clone(),
        git: cfg.git_path.clone(),
        skip_if_staged: cfg.skip_if_staged,
        auto_push: cfg.auto_push,
    })
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
        pushes: Vec::new(),
    }
}

/// Folders whose backup ended cleanly. A folder that was skipped or failed is
/// in a state the user is still working on, so nothing is sent from it.
pub fn pushable_folders(report: &BackupReport) -> Vec<PathBuf> {
    report
        .folders
        .iter()
        .filter(|(_, result)| {
            matches!(
                result,
                FolderBackup::Done(CommitOutcome::Committed { .. } | CommitOutcome::SkippedNoChanges)
            )
        })
        .map(|(folder, _)| folder.clone())
        .collect()
}

pub fn push_folders(
    git: &Path,
    folders: &[PathBuf],
    timeout: Duration,
    is_push_wanted: &dyn Fn(&Path) -> bool,
) -> Vec<(PathBuf, FolderPush)> {
    folders
        .iter()
        .filter(|folder| is_push_wanted(folder))
        .map(|folder| {
            let pushed = match git_push::auto_push(git, folder, timeout) {
                Ok(outcome) => FolderPush::Done(outcome),
                Err(err) => FolderPush::Failed(format!("{err:#}")),
            };
            (folder.clone(), pushed)
        })
        .collect()
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
            is_pushing: false,
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

    /// A bare remote and a repo that tracks it, with one pushed commit.
    fn repo_with_remote(dir: &Path) -> (PathBuf, PathBuf) {
        let remote = dir.join("remote.git");
        fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);
        let notes = dir.join("notes");
        fs::create_dir(&notes).unwrap();
        git(&notes, &["init", "-b", "main"]);
        git(&notes, &["config", "user.email", "test@example.com"]);
        git(&notes, &["config", "user.name", "Test"]);
        git(&notes, &["remote", "add", "origin", remote.to_str().unwrap()]);
        fs::write(notes.join("first.md"), "1\n").unwrap();
        git(&notes, &["add", "-A"]);
        git(&notes, &["commit", "-m", "first"]);
        git(&notes, &["push", "-u", "origin", "main"]);
        (remote, notes)
    }

    fn head(repo: &Path, rev: &str) -> String {
        let out = git_ops::run_git(Path::new("git"), repo, &["rev-parse", rev]).unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn settings(folders: Vec<PathBuf>, auto_push: bool) -> RunSettings {
        RunSettings {
            interval: Duration::from_secs(300),
            folders,
            git: PathBuf::from("git"),
            skip_if_staged: true,
            auto_push,
        }
    }

    fn report_after_run(settings: &RunSettings) -> BackupReport {
        let state = Mutex::new(SchedulerState::default());
        run_backup(&state, settings, &|_| true);
        let st = state.lock().unwrap();
        assert!(!st.is_running && !st.is_pushing, "the run has ended");
        st.report.clone().unwrap()
    }

    #[test]
    fn nothing_is_pushed_while_auto_push_is_off() {
        let dir = tempdir().unwrap();
        let (remote, notes) = repo_with_remote(dir.path());
        let remote_head = head(&remote, "main");
        fs::write(notes.join("a.md"), "# a\n").unwrap();

        let report = report_after_run(&settings(vec![notes.clone()], false));
        assert!(matches!(report.folders[0].1, FolderBackup::Done(CommitOutcome::Committed { .. })));
        assert!(report.pushes.is_empty());
        assert_eq!(head(&remote, "main"), remote_head, "the remote did not move");
        assert_ne!(head(&notes, "HEAD"), remote_head, "the commit stays local");
    }

    #[test]
    fn auto_push_sends_the_new_commit_after_the_backup() {
        let dir = tempdir().unwrap();
        let (remote, notes) = repo_with_remote(dir.path());
        fs::write(notes.join("a.md"), "# a\n").unwrap();

        let report = report_after_run(&settings(vec![notes.clone()], true));
        assert_eq!(
            report.pushes,
            vec![(notes.clone(), FolderPush::Done(PushOutcome::Pushed { commits: 1 }))]
        );
        assert_eq!(head(&remote, "main"), head(&notes, "HEAD"));
        assert_eq!(status_text(&report), (Tone::Good, "✓ 已備份並推送 · 剛剛".to_string()));
    }

    #[test]
    fn failed_push_is_a_problem_but_the_commit_is_kept() {
        let dir = tempdir().unwrap();
        let (remote, notes) = repo_with_remote(dir.path());
        fs::rename(&remote, dir.path().join("gone.git")).unwrap();
        fs::write(notes.join("a.md"), "# a\n").unwrap();

        let report = report_after_run(&settings(vec![notes.clone()], true));
        assert!(matches!(report.folders[0].1, FolderBackup::Done(CommitOutcome::Committed { .. })));
        assert!(matches!(report.pushes[0].1, FolderPush::Failed(_)), "{:?}", report.pushes);
        assert_eq!(status_text(&report).0, Tone::Problem);
    }

    #[test]
    fn notes_can_be_saved_while_the_push_runs() {
        let dir = tempdir().unwrap();
        let (remote, notes) = repo_with_remote(dir.path());
        fs::write(notes.join("a.md"), "# a\n").unwrap();
        let state = Mutex::new(SchedulerState::default());
        let seen = Mutex::new(Vec::new());
        // Called in the push phase, right before the folder is pushed.
        run_backup(&state, &settings(vec![notes.clone()], true), &|_| {
            let st = state.lock().unwrap();
            let committed = st.report.as_ref().is_some_and(|r| r.pushes.is_empty());
            seen.lock().unwrap().push((st.is_running, st.is_pushing, committed));
            true
        });
        assert_eq!(*seen.lock().unwrap(), vec![(false, true, true)]);
        assert_eq!(head(&remote, "main"), head(&notes, "HEAD"));
    }

    #[test]
    fn turning_auto_push_off_stops_the_folders_not_yet_pushed() {
        let dir = tempdir().unwrap();
        let (remote, notes) = repo_with_remote(dir.path());
        let remote_head = head(&remote, "main");
        fs::write(notes.join("a.md"), "# a\n").unwrap();
        let state = Mutex::new(SchedulerState::default());
        run_backup(&state, &settings(vec![notes.clone()], true), &|_| false);
        let st = state.lock().unwrap();
        assert!(!st.is_pushing);
        assert!(st.report.as_ref().unwrap().pushes.is_empty());
        assert_eq!(head(&remote, "main"), remote_head, "the remote did not move");
    }

    #[test]
    fn push_follows_the_live_config() {
        let folder = PathBuf::from("/notes");
        let mut cfg = AppConfig::default();
        cfg.folders.push(folder.clone());
        let config = Mutex::new(cfg);
        assert!(!is_push_wanted(&config, &folder), "off by default");
        config.lock().unwrap().auto_push = true;
        assert!(is_push_wanted(&config, &folder));
        assert!(!is_push_wanted(&config, Path::new("/removed")));
    }

    #[test]
    fn only_cleanly_backed_up_folders_are_pushable() {
        let now = SystemTime::now();
        let committed = CommitOutcome::Committed { files: 1, message: "update 1 files".into() };
        let report = BackupReport {
            started_at: now,
            finished_at: now,
            folders: vec![
                (PathBuf::from("/committed"), FolderBackup::Done(committed)),
                (PathBuf::from("/clean"), FolderBackup::Done(CommitOutcome::SkippedNoChanges)),
                (PathBuf::from("/staged"), FolderBackup::Done(CommitOutcome::SkippedStagedPresent)),
                (PathBuf::from("/merging"), FolderBackup::Done(CommitOutcome::SkippedBusyState("MERGE_HEAD".into()))),
                (PathBuf::from("/inner"), FolderBackup::Done(CommitOutcome::SkippedNotRepoRoot(PathBuf::from("/")))),
                (PathBuf::from("/gone"), FolderBackup::Missing),
                (PathBuf::from("/broken"), FolderBackup::Failed("git commit failed".into())),
            ],
            last_commit_at: None,
            pushes: Vec::new(),
        };
        assert_eq!(
            pushable_folders(&report),
            vec![PathBuf::from("/committed"), PathBuf::from("/clean")]
        );
    }

    #[test]
    fn skipped_and_missing_folders_are_never_pushed() {
        let dir = tempdir().unwrap();
        let (remote, notes) = repo_with_remote(dir.path());
        let remote_head = head(&remote, "main");
        fs::write(notes.join("staged.md"), "x\n").unwrap();
        git(&notes, &["add", "staged.md"]);
        let inner = notes.join("inner");
        fs::create_dir(&inner).unwrap();

        let folders = vec![notes, inner, PathBuf::from("/no/such/folder")];
        let report = report_after_run(&settings(folders, true));
        assert_eq!(report.folders[0].1, FolderBackup::Done(CommitOutcome::SkippedStagedPresent));
        assert!(report.pushes.is_empty(), "{:?}", report.pushes);
        assert_eq!(head(&remote, "main"), remote_head);
    }

    #[test]
    fn missing_folder_is_reported() {
        let report = back_up_folders(Path::new("git"), &[PathBuf::from("/no/such/folder")], true);
        assert_eq!(report.folders[0].1, FolderBackup::Missing);
    }
}
