use anyhow::{bail, Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed { files: usize, message: String },
    SkippedNoChanges,
    SkippedStagedPresent,
    SkippedBusyState(String),
    /// The folder sits inside a bigger repo (its root is given); committing would
    /// take in files outside the folder.
    SkippedNotRepoRoot(PathBuf),
}

pub fn run_git<S: AsRef<OsStr>>(git: &Path, repo: &Path, args: &[S]) -> Result<std::process::Output> {
    Command::new(git)
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| {
            let shown: Vec<_> = args.iter().map(|a| a.as_ref().to_string_lossy()).collect();
            format!("spawn {} {:?}", git.display(), shown)
        })
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

/// The repo root when it is not `folder` itself; None when `folder` is the root.
/// Compares file identity, not path text, so symlinks, `/var` vs `/private/var`
/// and Unicode forms of the same directory still count as the root.
/// Any failure is an error, so the caller never commits on a guess.
pub fn outer_repo_root(git: &Path, folder: &Path) -> Result<Option<PathBuf>> {
    let out = run_git(git, folder, &["rev-parse", "--show-toplevel"])?;
    if !out.status.success() {
        bail!(
            "git rev-parse --show-toplevel failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    // Only the trailing newline is git's; a folder name may itself end in spaces.
    let raw = out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout);
    let root = path_from_bytes(raw);
    if is_same_dir(&root, folder)? {
        Ok(None)
    } else {
        Ok(Some(root))
    }
}

#[cfg(unix)]
fn path_from_bytes(raw: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(raw))
}

#[cfg(not(unix))]
fn path_from_bytes(raw: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(raw).into_owned())
}

#[cfg(unix)]
fn is_same_dir(a: &Path, b: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let meta_a = std::fs::metadata(a).with_context(|| format!("stat {}", a.display()))?;
    let meta_b = std::fs::metadata(b).with_context(|| format!("stat {}", b.display()))?;
    Ok(meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino())
}

#[cfg(not(unix))]
fn is_same_dir(a: &Path, b: &Path) -> Result<bool> {
    Ok(std::fs::canonicalize(a)? == std::fs::canonicalize(b)?)
}

/// Time of the newest commit, or None when the repo has no commits yet.
pub fn last_commit_time(git: &Path, repo: &Path) -> Option<std::time::SystemTime> {
    let out = run_git(git, repo, &["log", "-1", "--format=%ct"]).ok()?;
    if !out.status.success() {
        return None;
    }
    let secs: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
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

pub fn auto_commit(git: &Path, repo: &Path, skip_if_staged: bool) -> Result<CommitOutcome> {
    ensure_git_repo(git, repo)?;
    // Before anything is staged: `git add -A` would cover the whole outer repo.
    if let Some(root) = outer_repo_root(git, repo)? {
        return Ok(CommitOutcome::SkippedNotRepoRoot(root));
    }

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
        run_git(
            git_bin(),
            repo,
            &["config", "user.email", "test@example.com"],
        )
        .unwrap();
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

    fn head_of(repo: &Path) -> String {
        let out = run_git(git_bin(), repo, &["rev-parse", "HEAD"]).unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn subfolder_of_a_repo_is_never_committed() {
        let dir = tempdir().unwrap();
        let outer = dir.path();
        ensure_git_repo(git_bin(), outer).unwrap();
        configure_identity(outer);
        fs::write(outer.join("code.rs"), "fn main() {}\n").unwrap();
        run_git(git_bin(), outer, &["add", "-A"]).unwrap();
        run_git(git_bin(), outer, &["commit", "-m", "base"]).unwrap();
        let head = head_of(outer);

        let notes = outer.join("notes");
        fs::create_dir(&notes).unwrap();
        fs::write(notes.join("a.md"), "# a\n").unwrap();
        fs::write(outer.join("code.rs"), "changed outside the notes\n").unwrap();

        match auto_commit(git_bin(), &notes, true).unwrap() {
            CommitOutcome::SkippedNotRepoRoot(root) => {
                assert!(is_same_dir(&root, outer).unwrap());
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(head_of(outer), head, "no new commit");
        let status = status_porcelain(git_bin(), outer).unwrap();
        assert!(!has_staged_changes(&status), "nothing staged: {status:?}");
        assert!(!notes.join(".git").exists(), "no nested repo created");
        assert_eq!(fs::read_to_string(outer.join("code.rs")).unwrap(), "changed outside the notes\n");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_the_root_still_counts_as_the_root() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        ensure_git_repo(git_bin(), &real).unwrap();
        configure_identity(&real);
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        fs::write(real.join("a.md"), "x\n").unwrap();
        let out = auto_commit(git_bin(), &link, true).unwrap();
        assert!(matches!(out, CommitOutcome::Committed { files: 1, .. }), "{out:?}");
        assert!(last_commit_time(git_bin(), &real).is_some());
    }

    #[test]
    fn unknown_root_fails_closed() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        ensure_git_repo(git_bin(), repo).unwrap();
        configure_identity(repo);
        fs::write(repo.join("a.md"), "x\n").unwrap();
        // Inside .git there is no work tree, so the root cannot be found.
        assert!(outer_repo_root(git_bin(), &repo.join(".git")).is_err());
        assert!(auto_commit(git_bin(), &repo.join(".git"), true).is_err());
        assert!(run_git(git_bin(), repo, &["rev-parse", "--verify", "-q", "HEAD"]).unwrap().stdout.is_empty());
    }

    #[test]
    fn root_whose_name_ends_in_a_space_is_still_the_root() {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("筆記 ");
        fs::create_dir(&repo).unwrap();
        ensure_git_repo(git_bin(), &repo).unwrap();
        configure_identity(&repo);
        fs::write(repo.join("a.md"), "x\n").unwrap();
        assert!(matches!(auto_commit(git_bin(), &repo, true).unwrap(), CommitOutcome::Committed { .. }));
    }

    #[test]
    fn empty_repo_has_no_last_commit_time() {
        let dir = tempdir().unwrap();
        ensure_git_repo(git_bin(), dir.path()).unwrap();
        assert_eq!(last_commit_time(git_bin(), dir.path()), None);
    }

    #[test]
    fn detects_staged_index() {
        assert!(has_staged_changes(&["M  foo.md".into()]));
        assert!(!has_staged_changes(&[" M foo.md".into()]));
        assert!(!has_staged_changes(&["?? foo.md".into()]));
    }
}
