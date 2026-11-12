//! Pushes a folder's current branch to the upstream it already has.
//! It never forces, pulls, adds a remote or sets an upstream.

use crate::git_ops::run_git;
use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Long enough for a first push of notes with images, short enough that a
/// stalled connection does not hold the next backup for long.
pub const PUSH_TIMEOUT: Duration = Duration::from_secs(120);
/// Marks a push that was stopped at the deadline; the status line looks for it.
/// Git, ssh and curl say "timed out", so this must not.
pub const TIMED_OUT: &str = "push deadline reached";

const POLL_EVERY: Duration = Duration::from_millis(100);
const STDERR_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    Pushed { commits: usize },
    UpToDate,
    /// Nothing to push to: no remote, a detached HEAD, or a branch that only
    /// tracks another local branch.
    NoUpstream,
}

struct Upstream {
    /// Full ref of the local branch, like `refs/heads/main`.
    branch: String,
    remote: String,
    /// Full ref on the remote, like `refs/heads/main`.
    remote_ref: String,
}

/// Pushes only when the current branch is ahead of its upstream, so a push
/// that failed (offline, rejected) is tried again on the next backup.
pub fn auto_push(git: &Path, repo: &Path, timeout: Duration) -> Result<PushOutcome> {
    let Some(upstream) = upstream_of_head(git, repo)? else {
        return Ok(PushOutcome::NoUpstream);
    };
    let commits = commits_ahead(git, repo, &upstream.branch)?;
    if commits == 0 {
        return Ok(PushOutcome::UpToDate);
    }
    push(git, repo, &upstream, timeout)?;
    Ok(PushOutcome::Pushed { commits })
}

fn upstream_of_head(git: &Path, repo: &Path) -> Result<Option<Upstream>> {
    let head = run_git(git, repo, &["symbolic-ref", "-q", "HEAD"])?;
    if !head.status.success() {
        return Ok(None);
    }
    let branch = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let format = "--format=%(upstream:remotename)%09%(upstream:remoteref)";
    let out = run_git(git, repo, &["for-each-ref", format, &branch])?;
    if !out.status.success() {
        bail!(
            "git for-each-ref failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let Some((remote, remote_ref)) = text.lines().next().and_then(|l| l.split_once('\t')) else {
        return Ok(None);
    };
    // "." is this repo itself: the branch only tracks another local branch.
    if remote.is_empty() || remote == "." || remote_ref.is_empty() {
        return Ok(None);
    }
    Ok(Some(Upstream {
        branch,
        remote: remote.to_string(),
        remote_ref: remote_ref.to_string(),
    }))
}

/// Commits on the branch that its upstream does not have, as last seen locally.
/// An upstream that was never fetched (a fresh clone of an empty repo) has no
/// local ref yet, so every commit counts. Any other failure is an error.
fn commits_ahead(git: &Path, repo: &Path, branch: &str) -> Result<usize> {
    // `@{u}` takes the short name; the full ref keeps a tag of the same name out.
    let short = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let upstream = format!("{short}@{{u}}");
    let is_fetched = run_git(git, repo, &["rev-parse", "--verify", "-q", &upstream])?
        .status
        .success();
    let range = if is_fetched {
        format!("{upstream}..{branch}")
    } else {
        branch.to_string()
    };
    let out = run_git(git, repo, &["rev-list", "--count", &range, "--"])?;
    if !out.status.success() {
        bail!(
            "git rev-list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .context("parse git rev-list --count")
}

/// The refspec names the branch on both sides, so the result does not depend
/// on the user's `push.default` and no other branch is ever sent. The two
/// flags keep `push.followTags` and `push.recurseSubmodules` from sending more.
fn push(git: &Path, repo: &Path, upstream: &Upstream, timeout: Duration) -> Result<()> {
    let refspec = format!("{}:{}", upstream.branch, upstream.remote_ref);
    let mut command = Command::new(git);
    command
        .args(["push", "--no-follow-tags", "--no-recurse-submodules", "--"])
        .args([&upstream.remote, &refspec])
        .current_dir(repo)
        // The app has no terminal: a missing credential must fail, not wait.
        .env("GIT_TERMINAL_PROMPT", "0")
        // English messages, so the status line can tell the causes apart.
        .env("LC_ALL", "C");
    let (status, stderr) = run_with_deadline(command, timeout)?;
    if !status.success() {
        bail!("git push failed: {}", stderr.trim());
    }
    Ok(())
}

/// Runs the command to the end or stops it at the deadline.
/// Returns the exit status and what it wrote to stderr.
fn run_with_deadline(mut command: Command, timeout: Duration) -> Result<(ExitStatus, String)> {
    own_process_group(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {command:?}"))?;
    let stderr = read_in_background(child.stderr.take());
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().context("wait for git push")? {
            // A helper process may still hold the pipe; do not wait for it forever.
            let text = stderr.recv_timeout(STDERR_WAIT);
            return Ok((status, text.unwrap_or_else(|_| "(stderr not available)".into())));
        }
        if Instant::now() >= deadline {
            stop_with_helpers(&mut child);
            bail!("git push stopped: {TIMED_OUT} after {} s", timeout.as_secs());
        }
        thread::sleep(POLL_EVERY);
    }
}

/// Git starts ssh or an https helper; a group lets all of them be stopped.
#[cfg(unix)]
fn own_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn own_process_group(_command: &mut Command) {}

fn stop_with_helpers(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        let _ = Command::new("/bin/kill").args(["-KILL", "--", &group]).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_ops::auto_commit;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};

    const SHORT: Duration = Duration::from_secs(30);

    fn git(repo: &Path, args: &[&str]) -> String {
        let out = run_git(Path::new("git"), repo, args).unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn new_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    fn commit_file(repo: &Path, name: &str) {
        fs::write(repo.join(name), name).unwrap();
        auto_commit(Path::new("git"), repo, true).unwrap();
    }

    /// A bare remote and a clone of it that already tracks `origin/main`.
    fn remote_and_clone() -> (TempDir, PathBuf, PathBuf) {
        let dir = tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);
        let notes = dir.path().join("notes");
        new_repo(&notes);
        git(&notes, &["remote", "add", "origin", remote.to_str().unwrap()]);
        commit_file(&notes, "first.md");
        git(&notes, &["push", "-u", "origin", "main"]);
        (dir, remote, notes)
    }

    fn push_now(repo: &Path) -> Result<PushOutcome> {
        auto_push(Path::new("git"), repo, SHORT)
    }

    #[test]
    fn commits_ahead_of_upstream_are_pushed_then_nothing_is_left() {
        let (_dir, remote, notes) = remote_and_clone();
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::UpToDate);
        commit_file(&notes, "a.md");
        commit_file(&notes, "b.md");
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::Pushed { commits: 2 });
        assert_eq!(git(&remote, &["rev-parse", "main"]), git(&notes, &["rev-parse", "HEAD"]));
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::UpToDate);
    }

    #[test]
    fn folder_without_a_remote_is_left_alone() {
        let dir = tempdir().unwrap();
        new_repo(dir.path());
        commit_file(dir.path(), "a.md");
        assert_eq!(push_now(dir.path()).unwrap(), PushOutcome::NoUpstream);
        assert_eq!(git(dir.path(), &["remote"]), "", "no remote was added");
    }

    #[test]
    fn remote_without_an_upstream_is_not_given_one() {
        let (_dir, remote, notes) = remote_and_clone();
        git(&notes, &["branch", "--unset-upstream"]);
        let before = git(&remote, &["rev-parse", "main"]);
        commit_file(&notes, "a.md");
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::NoUpstream);
        assert_eq!(git(&remote, &["rev-parse", "main"]), before);
        let upstream = run_git(Path::new("git"), &notes, &["rev-parse", "@{u}"]).unwrap();
        assert!(!upstream.status.success(), "no upstream was set");
    }

    #[test]
    fn branch_that_tracks_a_local_branch_is_not_pushed() {
        let (_dir, remote, notes) = remote_and_clone();
        let main_before = git(&notes, &["rev-parse", "main"]);
        git(&notes, &["checkout", "-b", "draft", "--track", "main"]);
        commit_file(&notes, "a.md");
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::NoUpstream);
        assert_eq!(git(&notes, &["rev-parse", "main"]), main_before, "local main did not move");
        assert_eq!(git(&remote, &["rev-parse", "main"]), main_before);
    }

    #[test]
    fn tags_stay_local_even_with_follow_tags_set() {
        let (_dir, remote, notes) = remote_and_clone();
        git(&notes, &["config", "push.followTags", "true"]);
        commit_file(&notes, "a.md");
        git(&notes, &["tag", "-a", "v1", "-m", "v1"]);
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::Pushed { commits: 1 });
        assert_eq!(git(&remote, &["rev-parse", "main"]), git(&notes, &["rev-parse", "HEAD"]));
        assert_eq!(git(&remote, &["tag", "--list"]), "", "the tag was not sent");
    }

    #[test]
    fn detached_head_is_not_pushed() {
        let (_dir, remote, notes) = remote_and_clone();
        let before = git(&remote, &["rev-parse", "main"]);
        git(&notes, &["checkout", "--detach"]);
        commit_file(&notes, "a.md");
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::NoUpstream);
        assert_eq!(git(&remote, &["rev-parse", "main"]), before);
    }

    #[test]
    fn newer_remote_rejects_the_push_and_is_never_overwritten() {
        let (dir, remote, notes) = remote_and_clone();
        let other = dir.path().join("other");
        git(dir.path(), &["clone", remote.to_str().unwrap(), other.to_str().unwrap()]);
        git(&other, &["config", "user.email", "test@example.com"]);
        git(&other, &["config", "user.name", "Test"]);
        commit_file(&other, "elsewhere.md");
        git(&other, &["push", "origin", "main"]);
        let remote_head = git(&remote, &["rev-parse", "main"]);

        commit_file(&notes, "here.md");
        let local_head = git(&notes, &["rev-parse", "HEAD"]);
        let err = format!("{:#}", push_now(&notes).unwrap_err());
        assert!(err.contains("rejected"), "{err}");
        assert_eq!(git(&remote, &["rev-parse", "main"]), remote_head, "remote untouched");
        assert_eq!(git(&notes, &["rev-parse", "HEAD"]), local_head, "local commit kept");
        assert!(push_now(&notes).is_err(), "still ahead, so it is tried again");
    }

    #[test]
    fn only_the_current_branch_goes_to_its_own_upstream() {
        let (_dir, remote, notes) = remote_and_clone();
        git(&notes, &["config", "push.default", "matching"]);
        git(&notes, &["branch", "draft"]);
        git(&notes, &["checkout", "-b", "local-name", "--track", "origin/main"]);
        commit_file(&notes, "a.md");
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::Pushed { commits: 1 });
        assert_eq!(git(&remote, &["rev-parse", "main"]), git(&notes, &["rev-parse", "HEAD"]));
        assert_eq!(git(&remote, &["branch", "--list"]), "* main", "no other branch was sent");
    }

    #[test]
    fn fresh_clone_of_an_empty_remote_pushes_its_first_commits() {
        let dir = tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);
        let notes = dir.path().join("notes");
        git(dir.path(), &["clone", remote.to_str().unwrap(), notes.to_str().unwrap()]);
        git(&notes, &["config", "user.email", "test@example.com"]);
        git(&notes, &["config", "user.name", "Test"]);
        git(&notes, &["checkout", "-B", "main"]);
        commit_file(&notes, "a.md");
        assert_eq!(push_now(&notes).unwrap(), PushOutcome::Pushed { commits: 1 });
        assert_eq!(git(&remote, &["rev-parse", "main"]), git(&notes, &["rev-parse", "HEAD"]));
    }

    #[test]
    fn unreachable_remote_fails_and_keeps_the_commit() {
        let (dir, remote, notes) = remote_and_clone();
        fs::rename(&remote, dir.path().join("gone.git")).unwrap();
        commit_file(&notes, "a.md");
        let err = format!("{:#}", push_now(&notes).unwrap_err());
        assert!(err.starts_with("git push failed"), "{err}");
        assert_eq!(git(&notes, &["rev-list", "--count", "@{u}..HEAD"]), "1");
    }

    #[test]
    fn a_stuck_command_is_stopped_at_the_deadline() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let started = Instant::now();
        let err = run_with_deadline(command, Duration::from_millis(300)).unwrap_err();
        assert!(format!("{err:#}").contains(TIMED_OUT), "{err:#}");
        assert!(started.elapsed() < Duration::from_secs(10), "did not wait for the command");
    }

    #[cfg(unix)]
    #[test]
    fn helpers_of_a_stuck_command_are_stopped_too() {
        let dir = tempdir().unwrap();
        let pid_file = dir.path().join("helper.pid");
        let script = format!("sleep 300 & echo $! > '{}'; wait", pid_file.display());
        let mut command = Command::new("sh");
        command.args(["-c", &script]);
        assert!(run_with_deadline(command, Duration::from_millis(500)).is_err());

        let helper = fs::read_to_string(&pid_file).unwrap().trim().to_string();
        let is_alive = || {
            let probe = Command::new("/bin/kill").args(["-0", &helper]).output().unwrap();
            probe.status.success()
        };
        let waited = Instant::now();
        while is_alive() && waited.elapsed() < Duration::from_secs(5) {
            thread::sleep(POLL_EVERY);
        }
        assert!(!is_alive(), "helper {helper} is still running");
    }

    #[test]
    fn stderr_of_a_failed_command_is_kept() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo boom >&2; exit 3"]);
        let (status, stderr) = run_with_deadline(command, SHORT).unwrap();
        assert_eq!(status.code(), Some(3));
        assert_eq!(stderr.trim(), "boom");
    }
}
