use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed { files: usize, message: String },
    SkippedNoChanges,
    SkippedStagedPresent,
    SkippedBusyState(String),
}

pub fn run_git(git: &Path, repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new(git)
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| format!("spawn {} {:?}", git.display(), args))
}

pub fn is_git_repo(git: &Path, folder: &Path) -> bool {
    run_git(git, folder, &["rev-parse", "--is-inside-work-tree"])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn ensure_git_repo(git: &Path, folder: &Path) -> Result<()> {
    if is_git_repo(git, folder) {
        return Ok(());
    }
    let out = run_git(git, folder, &["init"])?;
    if !out.status.success() {
        bail!(
            "git init failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Returns porcelain status lines (one per changed path entry).
pub fn status_porcelain(git: &Path, repo: &Path) -> Result<Vec<String>> {
    let out = run_git(git, repo, &["status", "--porcelain", "-uall"])?;
    if !out.status.success() {
        bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect())
}

pub fn has_staged_changes(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            return false;
        }
        let x = bytes[0] as char;
        x != ' ' && x != '?'
    })
}

pub fn is_busy_state(git: &Path, repo: &Path) -> Result<Option<String>> {
    let git_dir_out = run_git(git, repo, &["rev-parse", "--git-dir"])?;
    if !git_dir_out.status.success() {
        return Ok(None);
    }
    let git_dir_raw = String::from_utf8_lossy(&git_dir_out.stdout);
    let git_dir_path = Path::new(git_dir_raw.trim());
    let git_dir = if git_dir_path.is_absolute() {
        git_dir_path.to_path_buf()
    } else {
        repo.join(git_dir_path)
    };
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REBASE_HEAD",
        "rebase-merge",
        "rebase-apply",
    ] {
        if git_dir.join(name).exists() {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

fn count_paths(lines: &[String]) -> usize {
    lines
        .iter()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            // format: XY PATH or XY ORIG -> PATH
            Some(1usize)
        })
        .sum()
}

pub fn auto_commit(
    git: &Path,
    repo: &Path,
    skip_if_staged: bool,
) -> Result<CommitOutcome> {
    ensure_git_repo(git, repo)?;

    if let Some(busy) = is_busy_state(git, repo)? {
        return Ok(CommitOutcome::SkippedBusyState(busy));
    }

    let before = status_porcelain(git, repo)?;
    if before.is_empty() {
        return Ok(CommitOutcome::SkippedNoChanges);
    }
    if skip_if_staged && has_staged_changes(&before) {
        return Ok(CommitOutcome::SkippedStagedPresent);
    }

    let add = run_git(git, repo, &["add", "-A"])?;
    if !add.status.success() {
        bail!(
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr).trim()
        );
    }

    let staged = status_porcelain(git, repo)?;
    let staged_only: Vec<String> = staged
        .into_iter()
        .filter(|l| has_staged_changes(std::slice::from_ref(l)))
        .collect();
    if staged_only.is_empty() {
        return Ok(CommitOutcome::SkippedNoChanges);
    }

    let files = count_paths(&staged_only);
    let message = format!("update {files} files");
    let commit = run_git(git, repo, &["commit", "-m", &message])?;
    if !commit.status.success() {
        bail!(
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr).trim()
        );
    }
    Ok(CommitOutcome::Committed { files, message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn git_bin() -> &'static Path {
        Path::new("git")
    }

    fn configure_identity(repo: &Path) {
        run_git(git_bin(), repo, &["config", "user.email", "test@example.com"]).unwrap();
        run_git(git_bin(), repo, &["config", "user.name", "Test"]).unwrap();
    }

    #[test]
    fn init_and_commit_counts_files() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        ensure_git_repo(git_bin(), repo).unwrap();
        configure_identity(repo);
        fs::write(repo.join("a.md"), "# a\n").unwrap();
        fs::write(repo.join("b.md"), "# b\n").unwrap();
        let out = auto_commit(git_bin(), repo, true).unwrap();
        match out {
            CommitOutcome::Committed { files, message } => {
                assert_eq!(files, 2);
                assert_eq!(message, "update 2 files");
            }
            other => panic!("unexpected {other:?}"),
        }
        let again = auto_commit(git_bin(), repo, true).unwrap();
        assert_eq!(again, CommitOutcome::SkippedNoChanges);
    }

    #[test]
    fn skips_when_staged_present() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        ensure_git_repo(git_bin(), repo).unwrap();
        fs::write(repo.join("a.md"), "1\n").unwrap();
        run_git(git_bin(), repo, &["add", "a.md"]).unwrap();
        fs::write(repo.join("b.md"), "2\n").unwrap();
        let out = auto_commit(git_bin(), repo, true).unwrap();
        assert_eq!(out, CommitOutcome::SkippedStagedPresent);
    }

    #[test]
    fn detects_staged_index() {
        assert!(has_staged_changes(&["M  foo.md".into()]));
        assert!(!has_staged_changes(&[" M foo.md".into()]));
        assert!(!has_staged_changes(&["?? foo.md".into()]));
    }
}
