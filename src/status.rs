//! Turns backup state into one short line for the sidebar.

use crate::git_ops::CommitOutcome;
use crate::scheduler::{BackupReport, FolderBackup};
use std::path::Path;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Quiet,
    Problem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub tone: Tone,
    pub text: String,
    /// Full cause, shown on hover.
    pub detail: Option<String>,
}

pub struct BackupState<'a> {
    pub report: Option<&'a BackupReport>,
    pub is_running: bool,
    pub is_paused: bool,
    pub editor_dirty: bool,
}

pub fn backup_status(state: &BackupState, now: SystemTime) -> StatusLine {
    if state.editor_dirty {
        return line(Tone::Quiet, "✎ 編輯中");
    }
    if state.is_running {
        return line(Tone::Quiet, "↻ 正在備份…");
    }
    if state.is_paused {
        return line(Tone::Quiet, "‖ 自動備份已暫停");
    }
    let Some(report) = state.report else {
        return line(Tone::Quiet, "○ 還沒備份");
    };
    if let Some(problem) = first_problem(report) {
        return problem;
    }
    let ago = time_ago(now.duration_since(report.finished_at).unwrap_or_default());
    line(Tone::Good, &format!("✓ 已備份 · {ago}"))
}

fn line(tone: Tone, text: &str) -> StatusLine {
    StatusLine {
        tone,
        text: text.to_string(),
        detail: None,
    }
}

fn first_problem(report: &BackupReport) -> Option<StatusLine> {
    let problems: Vec<(String, String)> = report
        .folders
        .iter()
        .filter_map(|(folder, result)| folder_problem(folder, result))
        .collect();
    let (text, _) = problems.first()?.clone();
    let detail = problems
        .iter()
        .map(|(_, detail)| detail.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(StatusLine {
        tone: Tone::Problem,
        text: format!("⚠ {text}"),
        detail: Some(detail),
    })
}

/// Short text and full cause for a folder that was not backed up.
fn folder_problem(folder: &Path, result: &FolderBackup) -> Option<(String, String)> {
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string());
    let path = folder.display();
    match result {
        FolderBackup::Done(CommitOutcome::Committed { .. } | CommitOutcome::SkippedNoChanges) => {
            None
        }
        FolderBackup::Done(CommitOutcome::SkippedStagedPresent) => Some((
            format!("「{name}」有手動暫存的修改，先跳過"),
            format!("{path}\n有已 git add 的檔案。請先 commit 或取消暫存。"),
        )),
        FolderBackup::Done(CommitOutcome::SkippedBusyState(state)) => Some((
            format!("「{name}」正在合併，先跳過"),
            format!("{path}\nGit 正在進行 {state}。完成後會自動備份。"),
        )),
        FolderBackup::Missing => Some((
            format!("找不到「{name}」"),
            format!("{path}\n資料夾不見了。可以從選單移除它。"),
        )),
        FolderBackup::Failed(raw) => Some((failure_text(&name, raw), format!("{path}\n{raw}"))),
    }
}

fn failure_text(name: &str, raw: &str) -> String {
    if raw.contains("Please tell me who you are") || raw.contains("user.email") {
        return "請先設定 Git 的名字和 Email".into();
    }
    if raw.starts_with("spawn ") {
        return "找不到 Git，請先安裝".into();
    }
    format!("「{name}」備份失敗")
}

pub fn time_ago(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let (minute, hour, day) = (60, 3600, 86_400);
    match secs {
        s if s < minute => "剛剛".into(),
        s if s < hour => format!("{} 分鐘前", s / minute),
        s if s < day => format!("{} 小時前", s / hour),
        s if s < 7 * day => format!("{} 天前", s / day),
        s if s < 30 * day => format!("{} 週前", s / (7 * day)),
        s if s < 365 * day => format!("{} 個月前", s / (30 * day)),
        s => format!("{} 年前", s / (365 * day)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn report(results: Vec<(&str, FolderBackup)>, at: SystemTime) -> BackupReport {
        BackupReport {
            finished_at: at,
            folders: results
                .into_iter()
                .map(|(p, r)| (PathBuf::from(p), r))
                .collect(),
        }
    }

    fn state(report: Option<&BackupReport>) -> BackupState<'_> {
        BackupState {
            report,
            is_running: false,
            is_paused: false,
            editor_dirty: false,
        }
    }

    #[test]
    fn clean_run_says_backed_up_with_age() {
        let now = SystemTime::now();
        let done = report(
            vec![("/n", FolderBackup::Done(CommitOutcome::SkippedNoChanges))],
            now - Duration::from_secs(180),
        );
        let status = backup_status(&state(Some(&done)), now);
        assert_eq!(status.tone, Tone::Good);
        assert_eq!(status.text, "✓ 已備份 · 3 分鐘前");
    }

    #[test]
    fn unsaved_text_wins_over_everything() {
        let mut s = state(None);
        s.editor_dirty = true;
        s.is_running = true;
        assert_eq!(backup_status(&s, SystemTime::now()).text, "✎ 編輯中");
    }

    #[test]
    fn problems_name_the_folder_and_keep_the_cause() {
        let now = SystemTime::now();
        let failed = report(
            vec![
                ("/a/Notes", FolderBackup::Done(CommitOutcome::SkippedNoChanges)),
                ("/a/Work", FolderBackup::Missing),
                ("/a/Diary", FolderBackup::Failed("git add failed: boom".into())),
            ],
            now,
        );
        let status = backup_status(&state(Some(&failed)), now);
        assert_eq!(status.tone, Tone::Problem);
        assert_eq!(status.text, "⚠ 找不到「Work」");
        let detail = status.detail.unwrap();
        assert!(detail.contains("/a/Work") && detail.contains("boom"));
    }

    #[test]
    fn git_identity_error_gets_a_plain_fix() {
        let raw = "git commit failed: Author identity unknown\n*** Please tell me who you are.";
        assert_eq!(failure_text("Notes", raw), "請先設定 Git 的名字和 Email");
        assert_eq!(failure_text("Notes", "spawn git [\"init\"]"), "找不到 Git，請先安裝");
    }

    #[test]
    fn paused_and_running_are_plain() {
        let mut s = state(None);
        s.is_paused = true;
        assert_eq!(backup_status(&s, SystemTime::now()).text, "‖ 自動備份已暫停");
        s.is_paused = false;
        assert_eq!(backup_status(&s, SystemTime::now()).text, "○ 還沒備份");
        s.is_running = true;
        assert_eq!(backup_status(&s, SystemTime::now()).text, "↻ 正在備份…");
    }

    #[test]
    fn time_ago_steps() {
        assert_eq!(time_ago(Duration::from_secs(5)), "剛剛");
        assert_eq!(time_ago(Duration::from_secs(7200)), "2 小時前");
        assert_eq!(time_ago(Duration::from_secs(3 * 86_400)), "3 天前");
        assert_eq!(time_ago(Duration::from_secs(400 * 86_400)), "1 年前");
    }
}
