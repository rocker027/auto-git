//! Read-only access to the backed-up versions of one note, plus saving an old
//! version as a new file. Nothing here touches the index, HEAD or the note itself.

use crate::git_ops::run_git;
use anyhow::{anyhow, bail, Context, Result};
use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many versions the history list shows.
pub const MAX_VERSIONS: usize = 50;
/// Numbered names tried before giving up on a free file name.
const MAX_NAME_TRIES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteVersion {
    pub rev: String,
    pub committed_at: SystemTime,
    /// Local time as `YYYY-MM-DD HH:MM`, formatted by git.
    pub when: String,
}

/// Why one version cannot be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionTextError {
    /// Not UTF-8 text.
    NotText,
    Failed(String),
}

/// Why a note has no history to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    /// Not in a repo, repo without commits, or the note was never committed.
    NoVersions,
    /// Something else went wrong; the raw message is kept for the hover text.
    Failed(String),
}

/// Folder and file name of a note; git runs in the folder so paths stay relative to it.
fn split_note(note: &Path) -> Result<(&Path, &OsStr)> {
    let folder = note.parent().ok_or_else(|| anyhow!("no folder for {}", note.display()))?;
    let name = note.file_name().ok_or_else(|| anyhow!("no file name in {}", note.display()))?;
    Ok((folder, name))
}

fn failed(err: anyhow::Error) -> HistoryError {
    HistoryError::Failed(format!("{err:#}"))
}

/// True when `folder` is in a work tree whose HEAD has a commit.
/// Checked first so that only these two cases read as "no versions";
/// every other git failure is reported as it is.
fn has_commits(git: &Path, folder: &Path) -> Result<bool> {
    // English messages, so "not a git repository" can be told apart from other failures
    // such as a repo owned by someone else.
    let inside = std::process::Command::new(git)
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(folder)
        .env("LC_ALL", "C")
        .output()
        .with_context(|| format!("spawn {} rev-parse", git.display()))?;
    if !inside.status.success() {
        let raw = String::from_utf8_lossy(&inside.stderr).trim().to_string();
        if raw.contains("not a git repository") {
            return Ok(false);
        }
        bail!("git rev-parse failed: {raw}");
    }
    if String::from_utf8_lossy(&inside.stdout).trim() != "true" {
        return Ok(false);
    }
    let head = run_git(git, folder, &["rev-parse", "--verify", "-q", "HEAD"])?;
    Ok(head.status.success())
}

/// Newest first, at most MAX_VERSIONS. Renames are not followed, and commits that
/// deleted the note are left out (they have no text to show).
pub fn note_versions(git: &Path, note: &Path) -> Result<Vec<NoteVersion>, HistoryError> {
    let (folder, name) = split_note(note).map_err(failed)?;
    if !has_commits(git, folder).map_err(failed)? {
        return Err(HistoryError::NoVersions);
    }
    let limit = format!("-n{MAX_VERSIONS}");
    let args: Vec<&OsStr> = vec![
        // File names such as `[草稿].md` or `:x.md` must not act as patterns or magic.
        OsStr::new("--literal-pathspecs"),
        OsStr::new("log"),
        OsStr::new(&limit),
        // Explicit kinds: an exclude-only filter (`d`) matches nothing on git 2.50.
        OsStr::new("--diff-filter=ACMRT"),
        // Keep merges where the note was edited (against the first parent), and no patch
        // text, which could otherwise be misread as version lines.
        OsStr::new("--diff-merges=first-parent"),
        OsStr::new("--no-patch"),
        OsStr::new("--date=format-local:%Y-%m-%d %H:%M"),
        OsStr::new("--format=%H%x09%ct%x09%cd"),
        OsStr::new("--"),
        name,
    ];
    let out = run_git(git, folder, &args).map_err(failed)?;
    if !out.status.success() {
        let raw = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(HistoryError::Failed(format!("git log failed: {raw}")));
    }
    let versions: Vec<NoteVersion> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_version_line)
        .collect();
    if versions.is_empty() {
        return Err(HistoryError::NoVersions);
    }
    Ok(versions)
}

fn parse_version_line(line: &str) -> Option<NoteVersion> {
    let mut parts = line.splitn(3, '\t');
    let rev = parts.next()?.to_string();
    let secs: u64 = parts.next()?.parse().ok()?;
    let when = parts.next()?.to_string();
    Some(NoteVersion {
        rev,
        committed_at: UNIX_EPOCH + Duration::from_secs(secs),
        when,
    })
}

/// The note's text as committed in `rev`.
pub fn note_text_at(git: &Path, note: &Path, rev: &str) -> Result<String, VersionTextError> {
    let (folder, name) = split_note(note).map_err(|err| VersionTextError::Failed(format!("{err:#}")))?;
    // `rev:./name` is relative to the working directory, so it works below the repo root too.
    // cat-file only takes an object name, so a name like `[a].md` is never read as a pathspec.
    let mut object = OsString::from(format!("{rev}:./"));
    object.push(name);
    let out = run_git(git, folder, &[OsStr::new("cat-file"), OsStr::new("blob"), object.as_os_str()])
        .map_err(|err| VersionTextError::Failed(format!("{err:#}")))?;
    if !out.status.success() {
        let raw = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(VersionTextError::Failed(format!("git cat-file failed: {raw}")));
    }
    String::from_utf8(out.stdout).map_err(|_| VersionTextError::NotText)
}

/// Writes `text` next to `note` as `<name> (舊版 <when>).md`, adding 2, 3… when
/// the name is taken. Never overwrites: each try uses create_new.
pub fn save_as_new_note(note: &Path, when: &str, text: &str) -> Result<PathBuf> {
    let (folder, _) = split_note(note)?;
    let stem = note
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "筆記".into());
    let label = when.replace(':', "");
    for attempt in 1..=MAX_NAME_TRIES {
        let suffix = if attempt == 1 { String::new() } else { format!(" {attempt}") };
        let path = folder.join(format!("{stem} (舊版 {label}){suffix}.md"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(err) = file.write_all(text.as_bytes()) {
                    // A half-written copy would be picked up by the next backup.
                    let _ = std::fs::remove_file(&path);
                    return Err(err).with_context(|| format!("write {}", path.display()));
                }
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err).with_context(|| format!("create {}", path.display())),
        }
    }
    bail!("找不到可用的檔名，請先整理同名的舊版檔案")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn git(repo: &Path, args: &[&str]) {
        let out = run_git(Path::new("git"), repo, args).unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn new_repo(dir: &Path) {
        git(dir, &["init"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    fn commit_text(repo: &Path, name: &str, text: &str, message: &str) {
        fs::write(repo.join(name), text).unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-m", message]);
    }

    fn index_and_head(repo: &Path) -> (String, String) {
        let run = |args: &[&str]| {
            let out = run_git(Path::new("git"), repo, args).unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        (run(&["rev-parse", "HEAD"]), run(&["status", "--porcelain", "-uall"]))
    }

    #[test]
    fn versions_are_newest_first_and_read_back_exactly() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        new_repo(repo);
        commit_text(repo, "日記.md", "one\n", "update 1 files");
        commit_text(repo, "other.md", "x\n", "update 1 files");
        commit_text(repo, "日記.md", "two\n", "update 1 files");
        fs::write(repo.join("日記.md"), "unsaved draft on disk\n").unwrap();
        let before = index_and_head(repo);

        let note = repo.join("日記.md");
        let versions = note_versions(Path::new("git"), &note).unwrap();
        assert_eq!(versions.len(), 2, "only commits that touched this note");
        assert_eq!(note_text_at(Path::new("git"), &note, &versions[0].rev).unwrap(), "two\n");
        assert_eq!(note_text_at(Path::new("git"), &note, &versions[1].rev).unwrap(), "one\n");
        assert_eq!(versions[0].when.len(), "2026-11-10 15:30".len());

        assert_eq!(index_and_head(repo), before, "reading history changes nothing");
        assert_eq!(fs::read_to_string(&note).unwrap(), "unsaved draft on disk\n");
    }

    #[test]
    fn notes_below_the_repo_root_work_too() {
        let dir = tempdir().unwrap();
        new_repo(dir.path());
        fs::create_dir(dir.path().join("sub")).unwrap();
        commit_text(dir.path(), "sub/a b.md", "deep\n", "c");
        let note = dir.path().join("sub/a b.md");
        let versions = note_versions(Path::new("git"), &note).unwrap();
        assert_eq!(note_text_at(Path::new("git"), &note, &versions[0].rev).unwrap(), "deep\n");
    }

    #[test]
    fn no_versions_outside_a_repo_before_a_commit_or_for_new_notes() {
        let dir = tempdir().unwrap();
        let note = dir.path().join("a.md");
        fs::write(&note, "x").unwrap();
        assert_eq!(note_versions(Path::new("git"), &note), Err(HistoryError::NoVersions));
        new_repo(dir.path());
        assert_eq!(note_versions(Path::new("git"), &note), Err(HistoryError::NoVersions));
        fs::write(dir.path().join("b.md"), "y").unwrap();
        git(dir.path(), &["add", "b.md"]);
        git(dir.path(), &["commit", "-m", "c"]);
        assert_eq!(note_versions(Path::new("git"), &note), Err(HistoryError::NoVersions), "a.md never committed");
    }

    #[test]
    fn a_merge_that_changed_the_note_is_listed() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        new_repo(repo);
        commit_text(repo, "a.md", "base\n", "base");
        git(repo, &["checkout", "-q", "-b", "side"]);
        commit_text(repo, "a.md", "side\n", "side");
        git(repo, &["checkout", "-q", "-"]);
        commit_text(repo, "a.md", "main\n", "main");
        let _ = run_git(Path::new("git"), repo, &["merge", "side"]);
        fs::write(repo.join("a.md"), "resolved by hand\n").unwrap();
        git(repo, &["commit", "-qam", "merge"]);

        let note = repo.join("a.md");
        let versions = note_versions(Path::new("git"), &note).unwrap();
        assert_eq!(note_text_at(Path::new("git"), &note, &versions[0].rev).unwrap(), "resolved by hand\n");
        assert_eq!(versions.len(), 4, "merge, main, side, base");
    }

    #[test]
    fn save_as_new_never_overwrites() {
        let dir = tempdir().unwrap();
        let note = dir.path().join("plan.md");
        fs::write(&note, "current").unwrap();
        let first = save_as_new_note(&note, "2026-11-10 15:30", "old one").unwrap();
        assert_eq!(first, dir.path().join("plan (舊版 2026-11-10 1530).md"));
        let second = save_as_new_note(&note, "2026-11-10 15:30", "old two").unwrap();
        assert_eq!(second, dir.path().join("plan (舊版 2026-11-10 1530) 2.md"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "old one");
        assert_eq!(fs::read_to_string(&second).unwrap(), "old two");
        assert_eq!(fs::read_to_string(&note).unwrap(), "current");
    }

    #[test]
    fn non_text_versions_are_refused() {
        let dir = tempdir().unwrap();
        new_repo(dir.path());
        fs::write(dir.path().join("a.md"), [0xff, 0xfe, 0x00]).unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-m", "c"]);
        let note = dir.path().join("a.md");
        let versions = note_versions(Path::new("git"), &note).unwrap();
        assert_eq!(
            note_text_at(Path::new("git"), &note, &versions[0].rev),
            Err(VersionTextError::NotText)
        );
    }

    #[test]
    fn names_with_pattern_characters_are_taken_literally() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        new_repo(repo);
        commit_text(repo, "todo1.md", "not me\n", "c");
        commit_text(repo, "todo[1].md", "first\n", "c");
        fs::remove_file(repo.join("todo[1].md")).unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-m", "delete"]);
        commit_text(repo, "todo[1].md", "again\n", "c");
        commit_text(repo, ":x.md", "colon\n", "c");

        let note = repo.join("todo[1].md");
        let versions = note_versions(Path::new("git"), &note).unwrap();
        let texts: Vec<String> = versions
            .iter()
            .map(|v| note_text_at(Path::new("git"), &note, &v.rev).unwrap())
            .collect();
        assert_eq!(texts, vec!["again\n", "first\n"], "no todo1.md, no empty deletion entry");

        let colon = repo.join(":x.md");
        let versions = note_versions(Path::new("git"), &colon).unwrap();
        assert_eq!(note_text_at(Path::new("git"), &colon, &versions[0].rev).unwrap(), "colon\n");
    }

    #[test]
    fn other_git_failures_are_not_reported_as_no_versions() {
        let dir = tempdir().unwrap();
        new_repo(dir.path());
        commit_text(dir.path(), "a.md", "x", "c");
        commit_text(dir.path(), "a.md", "y", "c");
        let note = dir.path().join("a.md");
        // A missing object of the older commit: HEAD resolves, but git log fails.
        let first = String::from_utf8(
            run_git(Path::new("git"), dir.path(), &["rev-parse", "HEAD~1"]).unwrap().stdout,
        )
        .unwrap();
        let first = first.trim();
        fs::remove_file(dir.path().join(".git/objects").join(&first[..2]).join(&first[2..])).unwrap();
        assert!(matches!(note_versions(Path::new("git"), &note), Err(HistoryError::Failed(_))));
        let broken = note_text_at(Path::new("git"), &note, "0000000000000000000000000000000000000000");
        assert!(matches!(broken, Err(VersionTextError::Failed(_))));
    }
}
