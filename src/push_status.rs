//! Words for the push part of a backup report.

use crate::git_push::{PushOutcome, TIMED_OUT};
use crate::scheduler::FolderPush;
use std::path::{Path, PathBuf};

pub fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

/// True when every pushed folder has reached its remote.
pub fn is_all_pushed(pushes: &[(PathBuf, FolderPush)]) -> bool {
    !pushes.is_empty()
        && pushes.iter().all(|(_, push)| {
            matches!(push, FolderPush::Done(PushOutcome::Pushed { .. } | PushOutcome::UpToDate))
        })
}

/// Hover lines that tell where the versions are now; empty while push is off.
pub fn push_note(pushes: &[(PathBuf, FolderPush)]) -> String {
    pushes
        .iter()
        .filter_map(|(folder, push)| {
            let name = folder_name(folder);
            match push {
                FolderPush::Done(PushOutcome::Pushed { .. } | PushOutcome::UpToDate) => {
                    Some(format!("\n「{name}」已推送到遠端"))
                }
                FolderPush::Done(PushOutcome::NoUpstream) => {
                    Some(format!("\n「{name}」沒有設定遠端分支，只備份在這台電腦"))
                }
                FolderPush::Failed(_) => None,
            }
        })
        .collect()
}

/// Short text and full cause for a folder whose push failed.
pub fn push_problem(folder: &Path, push: &FolderPush) -> Option<(String, String)> {
    let FolderPush::Failed(raw) = push else {
        return None;
    };
    let name = folder_name(folder);
    let detail = format!(
        "{}\n版本已經建立在這台電腦，還沒推送到遠端。\n{raw}",
        folder.display()
    );
    Some((push_failure_text(&name, raw), detail))
}

fn push_failure_text(name: &str, raw: &str) -> String {
    let has = |words: &[&str]| words.iter().any(|word| raw.contains(word));
    if has(&["[rejected]", "non-fast-forward", "fetch first"]) {
        return format!("「{name}」的遠端有較新的版本，請先 git pull");
    }
    if has(&[TIMED_OUT]) {
        return format!("「{name}」推送逾時，下次備份再試");
    }
    // Trying again does not help with these; ssh adds a network line to them.
    if has(&[
        "Repository not found",
        "does not appear to be a git repository",
        "Host key verification failed",
    ]) {
        return format!("「{name}」無法存取遠端儲存庫，請檢查網址與權限");
    }
    // Before the network words: an access error also says "unable to access".
    if has(&[
        "Authentication failed",
        "could not read Username",
        "terminal prompts disabled",
        "Permission denied",
        "error: 403",
    ]) {
        return format!("「{name}」無法登入遠端，請檢查 Git 憑證");
    }
    if has(&[
        "Could not resolve host",
        "unable to access",
        "Failed to connect",
        "Could not read from remote repository",
    ]) {
        return format!("「{name}」連不上遠端，下次備份再試");
    }
    format!("「{name}」推送失敗")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pushes(results: Vec<(&str, FolderPush)>) -> Vec<(PathBuf, FolderPush)> {
        results.into_iter().map(|(p, r)| (PathBuf::from(p), r)).collect()
    }

    fn failed(raw: &str) -> FolderPush {
        FolderPush::Failed(raw.to_string())
    }

    #[test]
    fn push_failures_name_the_cause_and_the_fix() {
        let text = |raw: &str| push_failure_text("Notes", raw);
        assert_eq!(
            text("git push failed: ! [rejected] HEAD -> main (fetch first)"),
            "「Notes」的遠端有較新的版本，請先 git pull"
        );
        assert_eq!(
            text("git push stopped: push deadline reached after 120 s"),
            "「Notes」推送逾時，下次備份再試"
        );
        assert_eq!(
            text("git push failed: ssh: connect to host github.com port 22: Operation timed out\nfatal: Could not read from remote repository."),
            "「Notes」連不上遠端，下次備份再試"
        );
        assert_eq!(
            text("git push failed: ERROR: Repository not found.\nfatal: Could not read from remote repository."),
            "「Notes」無法存取遠端儲存庫，請檢查網址與權限"
        );
        assert_eq!(
            text("git push failed: ! [remote rejected] main -> main (protected branch hook declined)"),
            "「Notes」推送失敗"
        );
        assert_eq!(
            text("git push failed: fatal: could not read Username for 'https://github.com': terminal prompts disabled"),
            "「Notes」無法登入遠端，請檢查 Git 憑證"
        );
        assert_eq!(
            text("git push failed: fatal: unable to access 'https://github.com/a/b.git/': The requested URL returned error: 403"),
            "「Notes」無法登入遠端，請檢查 Git 憑證"
        );
        assert_eq!(
            text("git push failed: fatal: unable to access 'https://github.com/a/b.git/': Could not resolve host: github.com"),
            "「Notes」連不上遠端，下次備份再試"
        );
        assert_eq!(text("git push failed: something new"), "「Notes」推送失敗");
    }

    #[test]
    fn push_problem_keeps_the_path_and_says_the_version_is_safe() {
        let (text, detail) = push_problem(Path::new("/a/Notes"), &failed("git push failed: boom")).unwrap();
        assert_eq!(text, "「Notes」推送失敗");
        assert!(detail.contains("/a/Notes") && detail.contains("boom"));
        assert!(detail.contains("版本已經建立在這台電腦"));
        assert_eq!(push_problem(Path::new("/a"), &FolderPush::Done(PushOutcome::UpToDate)), None);
    }

    #[test]
    fn all_pushed_needs_every_folder_on_its_remote() {
        let done = |outcome| FolderPush::Done(outcome);
        assert!(!is_all_pushed(&[]), "push is off");
        assert!(is_all_pushed(&pushes(vec![
            ("/a", done(PushOutcome::Pushed { commits: 2 })),
            ("/b", done(PushOutcome::UpToDate)),
        ])));
        assert!(!is_all_pushed(&pushes(vec![
            ("/a", done(PushOutcome::UpToDate)),
            ("/b", done(PushOutcome::NoUpstream)),
        ])));
        assert!(!is_all_pushed(&pushes(vec![("/a", failed("x"))])));
    }

    #[test]
    fn push_note_tells_which_folders_stay_local() {
        let note = push_note(&pushes(vec![
            ("/a/Wiki", FolderPush::Done(PushOutcome::UpToDate)),
            ("/a/Diary", FolderPush::Done(PushOutcome::NoUpstream)),
        ]));
        assert_eq!(note, "\n「Wiki」已推送到遠端\n「Diary」沒有設定遠端分支，只備份在這台電腦");
        assert_eq!(push_note(&[]), "");
    }
}
