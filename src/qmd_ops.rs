use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct SearchHit {
    pub file: PathBuf,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
}

pub fn search(
    qmd: &Path,
    query: &str,
    limit: usize,
    collection: Option<&str>,
    timeout: Duration,
) -> Result<Vec<SearchHit>> {
    let mut cmd = Command::new(qmd);
    cmd.arg("search")
        .arg(query)
        .arg("-n")
        .arg(limit.to_string())
        .arg("--format")
        .arg("json")
        .arg("--full-path");
    if let Some(c) = collection {
        if !c.is_empty() {
            cmd.arg("-c").arg(c);
        }
    }

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {}", qmd.display()))?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    out.read_to_string(&mut stdout)?;
                }
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    err.read_to_string(&mut stderr)?;
                }
                if !status.success() {
                    bail!("qmd search failed: {}", stderr.trim());
                }
                let hits: Vec<SearchHit> =
                    serde_json::from_str(&stdout).context("parse qmd search json")?;
                return Ok(hits);
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("qmd search timed out after {}s", timeout.as_secs());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub fn path_allowed(path: &Path, roots: &[PathBuf]) -> bool {
    let Ok(canon) = path.canonicalize() else {
        return false;
    };
    roots.iter().any(|root| {
        let Ok(root) = root.canonicalize() else {
            return false;
        };
        canon.starts_with(&root)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn path_allowed_respects_roots() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("notes");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("a.md");
        std::fs::write(&file, "hi").unwrap();
        assert!(path_allowed(&file, &[dir.path().to_path_buf()]));
        let other = tempdir().unwrap();
        assert!(!path_allowed(&file, &[other.path().to_path_buf()]));
    }
}
