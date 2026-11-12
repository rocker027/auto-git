//! Turns backup state into one short line for the sidebar.

use crate::git_ops::CommitOutcome;
use crate::push_status::{folder_name, is_all_pushed, push_note, push_problem};
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
    /// The versions are made; they are on their way to the remote.
    pub is_pushing: bool,
    pub is_paused: bool,
    pub editor_dirty: bool,
    /// Last time the app wrote a note to disk.
    pub last_saved_at: Option<SystemTime>,
}

pub fn backup_status(state: &BackupState, now: SystemTime) -> StatusLine {
    if state.editor_dirty {
        return line(Tone::Quiet, "✎ 編輯中");
    }
    if state.is_running {
        return line(Tone::Quiet, "↻ 正在備份…");
    }
    if state.is_pushing {
        return line(Tone::Quiet, "↑ 正在推送…");
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
    if state.last_saved_at.is_some_and(|saved| saved >= report.started_at) {
        return StatusLine {
            tone: Tone::Quiet,
            text: "◔ 已存檔，等下次備份".into(),
            detail: Some("下次自動備份時會建立版本。要馬上建立，請按「備份」分頁的「立即備份」。".into()),
        };
    }
    backed_up_line(report, now)
}

/// The age shown is of the newest version; when it was last checked is on hover.
fn backed_up_line(report: &BackupReport, now: SystemTime) -> StatusLine {
    let age = |at: SystemTime| time_ago(now.duration_since(at).unwrap_or_default());
    let checked = format!("最近檢查：{}", age(report.finished_at));
    match report.last_commit_at {
        Some(version) => StatusLine {
            tone: Tone::Good,
            text: format!("✓ {} · {}", backed_up_word(report), age(version)),
            detail: Some(format!(
                "最新版本建立於 {}\n{checked}{}",
                age(version),
                push_note(&report.pushes)
            )),
        },
        None => StatusLine {
            tone: Tone::Good,
            text: format!("✓ 已檢查 · {}", age(report.finished_at)),
            detail: Some(format!("還沒有任何版本，沒有需要備份的檔案\n{checked}")),
        },
    }
}

fn backed_up_word(report: &BackupReport) -> &'static str {
    if is_all_pushed(&report.pushes) {
        "已備份並推送"
    } else {
        "已備份"
    }
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
        .chain(report.pushes.iter().filter_map(|(folder, push)| push_problem(folder, push)))
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
    let name = folder_name(folder);
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
        FolderBackup::Done(CommitOutcome::SkippedNotRepoRoot(root)) => Some((
            format!("「{name}」在另一個 Git 專案裡，先不備份"),
            format!(
                "{path}\n它在 {} 這個 Git 專案裡面。為了不把資料夾外的檔案一起備份，只備份本身就是專案最上層的資料夾。\n請改加入 {}，或把筆記搬到獨立的資料夾。",
                root.display(),
                root.display()
            ),
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
            started_at: at,
            finished_at: at,
            folders: results
                .into_iter()
                .map(|(p, r)| (PathBuf::from(p), r))
                .collect(),
            last_commit_at: None,
            pushes: Vec::new(),
        }
    }

    fn state(report: Option<&BackupReport>) -> BackupState<'_> {
        BackupState {
            report,
            is_running: false,
            is_pushing: false,
            is_paused: false,
            editor_dirty: false,
            last_saved_at: None,
        }
    }

    /// The age is of the newest version, not of the last check (changed 2026-11-10).
    #[test]
    fn backed_up_shows_the_age_of_the_newest_version() {
        let now = SystemTime::now();
        let mut done = report(
            vec![("/n", FolderBackup::Done(CommitOutcome::SkippedNoChanges))],
            now - Duration::from_secs(60),
        );
        done.last_commit_at = Some(now - Duration::from_secs(3 * 3600));
        let status = backup_status(&state(Some(&done)), now);
        assert_eq!(status.tone, Tone::Good);
        assert_eq!(status.text, "✓ 已備份 · 3 小時前");
        assert!(status.detail.unwrap().contains("最近檢查：1 分鐘前"));
    }

    #[test]
    fn pushed_folders_change_the_word_and_the_hover_text() {
        use crate::git_push::PushOutcome;
        use crate::scheduler::FolderPush;
        let now = SystemTime::now();
        let mut done = report(
            vec![("/n/Wiki", FolderBackup::Done(CommitOutcome::SkippedNoChanges))],
            now - Duration::from_secs(60),
        );
        done.last_commit_at = Some(now - Duration::from_secs(120));
        done.pushes = vec![(PathBuf::from("/n/Wiki"), FolderPush::Done(PushOutcome::UpToDate))];
        let status = backup_status(&state(Some(&done)), now);
        assert_eq!(status.text, "✓ 已備份並推送 · 2 分鐘前");
        assert!(status.detail.unwrap().contains("「Wiki」已推送到遠端"));

        done.pushes = vec![(PathBuf::from("/n/Wiki"), FolderPush::Done(PushOutcome::NoUpstream))];
        let status = backup_status(&state(Some(&done)), now);
        assert_eq!(status.text, "✓ 已備份 · 2 分鐘前", "nothing reached a remote");
        assert!(status.detail.unwrap().contains("只備份在這台電腦"));
    }

    #[test]
    fn failed_push_is_shown_after_backup_problems() {
        use crate::scheduler::FolderPush;
        let now = SystemTime::now();
        let mut done = report(
            vec![
                ("/n/Wiki", FolderBackup::Done(CommitOutcome::SkippedNoChanges)),
                ("/n/Work", FolderBackup::Missing),
            ],
            now,
        );
        let rejected = "git push failed: ! [rejected] HEAD -> main (fetch first)";
        done.pushes = vec![(PathBuf::from("/n/Wiki"), FolderPush::Failed(rejected.into()))];
        let status = backup_status(&state(Some(&done)), now);
        assert_eq!(status.tone, Tone::Problem);
        assert_eq!(status.text, "⚠ 找不到「Work」");
        assert!(status.detail.unwrap().contains("fetch first"));

        done.folders.pop();
        let status = backup_status(&state(Some(&done)), now);
        assert_eq!(status.text, "⚠ 「Wiki」的遠端有較新的版本，請先 git pull");
    }

    #[test]
    fn pushing_is_shown_until_it_ends() {
        let mut s = state(None);
        s.is_pushing = true;
        assert_eq!(backup_status(&s, SystemTime::now()).text, "↑ 正在推送…");
        s.is_running = true;
        assert_eq!(backup_status(&s, SystemTime::now()).text, "↻ 正在備份…");
    }

    #[test]
    fn no_version_yet_says_checked() {
        let now = SystemTime::now();
        let done = report(
            vec![("/n", FolderBackup::Done(CommitOutcome::SkippedNoChanges))],
            now - Duration::from_secs(180),
        );
        assert_eq!(backup_status(&state(Some(&done)), now).text, "✓ 已檢查 · 3 分鐘前");
    }

    #[test]
    fn a_save_after_the_run_started_waits_for_the_next_backup() {
        let now = SystemTime::now();
        let mut done = report(
            vec![("/n", FolderBackup::Done(CommitOutcome::SkippedNoChanges))],
            now - Duration::from_secs(60),
        );
        done.last_commit_at = Some(now - Duration::from_secs(60));
        let mut s = state(Some(&done));
        s.last_saved_at = Some(now - Duration::from_secs(30));
        let status = backup_status(&s, now);
        assert_eq!(status.tone, Tone::Quiet);
        assert_eq!(status.text, "◔ 已存檔，等下次備份");
        s.last_saved_at = Some(now - Duration::from_secs(90));
        assert_eq!(backup_status(&s, now).text, "✓ 已備份 · 1 分鐘前", "saved before the run");
    }

    #[test]
    fn folder_inside_another_repo_is_a_problem() {
        let now = SystemTime::now();
        let skipped = report(
            vec![(
                "/code/app/notes",
                FolderBackup::Done(CommitOutcome::SkippedNotRepoRoot(PathBuf::from("/code/app"))),
            )],
            now,
        );
        let status = backup_status(&state(Some(&skipped)), now);
        assert_eq!(status.tone, Tone::Problem);
        assert_eq!(status.text, "⚠ 「notes」在另一個 Git 專案裡，先不備份");
        assert!(status.detail.unwrap().contains("/code/app"));
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
