use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Watched note folder roots (absolute paths).
    pub folders: Vec<PathBuf>,
    /// How often to check each folder for changes.
    pub check_interval_secs: u64,
    /// Path to `git` executable.
    pub git_path: PathBuf,
    /// Path to `qmd` executable.
    pub qmd_path: PathBuf,
    /// Optional qmd collection filter (empty = all).
    pub qmd_collection: Option<String>,
    /// Max search hits.
    pub search_limit: usize,
    /// Skip auto-commit when working tree has staged changes.
    pub skip_if_staged: bool,
    /// Push each folder to its configured upstream after backing it up.
    /// Off unless the user turns it on; a config written before this field
    /// existed must still load, or the folder list would be lost.
    #[serde(default)]
    pub auto_push: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            check_interval_secs: 300,
            git_path: PathBuf::from("git"),
            qmd_path: PathBuf::from("qmd"),
            qmd_collection: None,
            search_limit: 10,
            skip_if_staged: true,
            auto_push: false,
        }
    }
}

impl AppConfig {
    pub fn check_interval(&self) -> Duration {
        Duration::from_secs(self.check_interval_secs.max(30))
    }

    pub fn config_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("app", "NoteSync", "note-sync")
            .context("cannot resolve Application Support directory")?;
        Ok(dirs.config_dir().join("config.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw =
            fs::read_to_string(&path).with_context(|| format!("read config {}", path.display()))?;
        let cfg: Self = serde_json::from_str(&raw).context("parse config.json")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(&path, raw).with_context(|| format!("write config {}", path.display()))?;
        Ok(())
    }

    pub fn add_folder(&mut self, folder: PathBuf) {
        let folder = normalize_path(&folder);
        if !self.folders.iter().any(|f| f == &folder) {
            self.folders.push(folder);
        }
    }

    pub fn remove_folder(&mut self, folder: &Path) {
        self.folders.retain(|existing| existing != folder);
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn default_interval_at_least_30s() {
        let mut cfg = AppConfig::default();
        cfg.check_interval_secs = 5;
        assert_eq!(cfg.check_interval(), Duration::from_secs(30));
    }

    #[test]
    fn config_written_before_auto_push_still_loads() {
        let old = r#"{
            "folders": ["/notes"],
            "check_interval_secs": 300,
            "git_path": "git",
            "qmd_path": "qmd",
            "qmd_collection": null,
            "search_limit": 10,
            "skip_if_staged": true
        }"#;
        let cfg: AppConfig = serde_json::from_str(old).unwrap();
        assert_eq!(cfg.folders, vec![PathBuf::from("/notes")]);
        assert!(!cfg.auto_push, "push stays off until the user turns it on");
        assert!(!AppConfig::default().auto_push);
    }

    #[test]
    fn add_folder_dedupes() {
        let dir = tempdir().unwrap();
        let mut cfg = AppConfig::default();
        cfg.add_folder(dir.path().to_path_buf());
        cfg.add_folder(dir.path().to_path_buf());
        assert_eq!(cfg.folders.len(), 1);
    }

    #[test]
    fn remove_folder_drops_the_chosen_path() {
        let kept = tempdir().unwrap();
        let dropped = tempdir().unwrap();
        let mut cfg = AppConfig::default();
        cfg.add_folder(kept.path().to_path_buf());
        cfg.add_folder(dropped.path().to_path_buf());
        let dropped_stored = cfg.folders[1].clone();
        cfg.remove_folder(&dropped_stored);
        assert_eq!(cfg.folders.len(), 1);
        assert_eq!(cfg.folders[0], fs::canonicalize(kept.path()).unwrap());
    }
}
