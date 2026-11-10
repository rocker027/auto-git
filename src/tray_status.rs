//! Read-only backup timestamp in the system menu.

use crate::scheduler::BackupReport;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tray_icon::menu::MenuItem;

pub struct TrayStatus {
    pub item: MenuItem,
    checked_at: Option<SystemTime>,
}

impl Default for TrayStatus {
    fn default() -> Self {
        Self {
            item: MenuItem::new("上次備份：讀取中…", false, None),
            checked_at: None,
        }
    }
}

impl TrayStatus {
    /// Only format the timestamp when a new report arrives, including while hidden.
    pub fn refresh(&mut self, report: Option<&BackupReport>) {
        let Some(report) = report else { return };
        if self.checked_at == Some(report.finished_at) {
            return;
        }
        let text = match report.last_commit_at {
            Some(at) => local_timestamp(at).unwrap_or_else(|| "無法顯示時間".into()),
            None if report.folders.iter().any(|(_, result)| {
                !matches!(result, crate::scheduler::FolderBackup::Done(
                    crate::git_ops::CommitOutcome::Committed { .. }
                    | crate::git_ops::CommitOutcome::SkippedNoChanges
                ))
            }) => "無法取得".into(),
            None => "尚無版本".into(),
        };
        self.item.set_text(format!("上次備份：{text}"));
        self.checked_at = Some(report.finished_at);
    }
}

/// macOS date uses the current local timezone. Called once per backup report.
fn local_timestamp(at: SystemTime) -> Option<String> {
    let seconds = at.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let output = Command::new("/bin/date")
        .args(["-r", &seconds.to_string(), "+%Y-%m-%d %H:%M"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}
