//! State of the one note open in the editor, kept in step with the file on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DISK_POLL: Duration = Duration::from_secs(2);

pub struct OpenNote {
    pub path: PathBuf,
    pub text: String,
    /// Text last read from or written to disk.
    saved: String,
    /// The file changed elsewhere while this copy has unsaved edits.
    pub has_conflict: bool,
    pub error: String,
    last_poll: Instant,
}

impl OpenNote {
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            saved: text.clone(),
            text,
            has_conflict: false,
            error: String::new(),
            last_poll: Instant::now(),
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }

    /// Writes the editor text. Refuses when the file changed elsewhere.
    pub fn save(&mut self) -> bool {
        if let Ok(on_disk) = fs::read_to_string(&self.path) {
            if on_disk != self.saved && on_disk != self.text {
                self.has_conflict = true;
                return false;
            }
        }
        match fs::write(&self.path, &self.text) {
            Ok(()) => {
                self.saved = self.text.clone();
                self.has_conflict = false;
                self.error.clear();
                true
            }
            Err(err) => {
                self.error = format!("無法儲存：{err}");
                false
            }
        }
    }

    /// Drops local edits and takes the file as it is on disk.
    pub fn reload(&mut self) {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                self.saved = text.clone();
                self.text = text;
                self.has_conflict = false;
                self.error.clear();
            }
            Err(err) => self.error = format!("無法讀取：{err}"),
        }
    }

    /// Picks up outside changes: follows them when clean, flags a conflict when dirty.
    /// Returns true when the editor text was replaced with the disk text.
    pub fn poll_disk(&mut self) -> bool {
        if self.last_poll.elapsed() < DISK_POLL {
            return false;
        }
        self.last_poll = Instant::now();
        self.apply_disk_text()
    }

    fn apply_disk_text(&mut self) -> bool {
        let Ok(on_disk) = fs::read_to_string(&self.path) else {
            return false;
        };
        let mut followed = false;
        if on_disk == self.text {
            self.saved = on_disk.clone();
        } else if on_disk != self.saved && !self.is_dirty() {
            self.saved = on_disk.clone();
            self.text = on_disk.clone();
            followed = true;
        }
        // Recomputed every time so the flag clears once the disk agrees again.
        self.has_conflict = self.is_dirty() && on_disk != self.saved;
        followed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_writes_and_clears_dirty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let mut note = OpenNote::load(&path).unwrap();
        note.text.push_str(" two");
        assert!(note.is_dirty());
        assert!(note.save());
        assert!(!note.is_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "one two");
    }

    #[test]
    fn save_refuses_when_file_changed_elsewhere() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let mut note = OpenNote::load(&path).unwrap();
        note.text = "mine".into();
        fs::write(&path, "theirs").unwrap();
        assert!(!note.save());
        assert!(note.has_conflict);
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
        note.reload();
        assert_eq!(note.text, "theirs");
        assert!(!note.has_conflict && !note.is_dirty());
    }

    #[test]
    fn disk_change_follows_clean_note_and_flags_dirty_note() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let mut note = OpenNote::load(&path).unwrap();
        fs::write(&path, "two").unwrap();
        assert!(note.apply_disk_text());
        assert_eq!(note.text, "two");
        assert!(!note.has_conflict);

        note.text = "mine".into();
        fs::write(&path, "three").unwrap();
        note.apply_disk_text();
        assert!(note.has_conflict);
        assert_eq!(note.text, "mine");

        fs::write(&path, "two").unwrap();
        note.apply_disk_text();
        assert!(!note.has_conflict, "disk went back to the saved text");
        assert!(note.is_dirty());
    }

    #[test]
    fn conflict_clears_when_disk_matches_my_text() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let mut note = OpenNote::load(&path).unwrap();
        note.text = "mine".into();
        fs::write(&path, "theirs").unwrap();
        note.apply_disk_text();
        assert!(note.has_conflict);
        fs::write(&path, "mine").unwrap();
        note.apply_disk_text();
        assert!(!note.has_conflict && !note.is_dirty());
    }

    #[test]
    fn clean_note_save_after_outside_change_is_not_a_stuck_conflict() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let mut note = OpenNote::load(&path).unwrap();
        fs::write(&path, "two").unwrap();
        note.save();
        note.apply_disk_text();
        assert_eq!(note.text, "two");
        assert!(!note.has_conflict);
    }
}
