//! State of the one note open in the editor, kept in step with the file on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DISK_POLL: Duration = Duration::from_secs(2);
/// Quiet time after the last keystroke before the note is written on its own.
pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

pub struct OpenNote {
    pub path: PathBuf,
    pub text: String,
    /// Text last read from or written to disk.
    saved: String,
    /// The file changed elsewhere while this copy has unsaved edits.
    pub has_conflict: bool,
    pub error: String,
    last_poll: Instant,
    /// Last edit, or last failed autosave; autosave waits AUTOSAVE_DELAY after it.
    edited_at: Option<Instant>,
    /// An input method is composing text (e.g. 注音 candidates still open).
    pub is_composing: bool,
    /// The last unattended save could not read the file (moved, deleted, not UTF-8).
    pub is_unreadable: bool,
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
            edited_at: None,
            is_composing: false,
            is_unreadable: false,
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
                self.is_unreadable = false;
                self.error.clear();
                true
            }
            Err(err) => {
                self.error = format!("無法儲存：{err}");
                false
            }
        }
    }

    pub fn mark_edited(&mut self, at: Instant) {
        self.edited_at = Some(at);
    }

    pub fn is_autosave_due(&self, now: Instant) -> bool {
        self.is_dirty()
            && !self.has_conflict
            && !self.is_composing
            && self
                .edited_at
                .is_some_and(|at| now.saturating_duration_since(at) >= AUTOSAVE_DELAY)
    }

    /// Saves once typing has paused. Returns true when the file was written.
    /// A failed write waits another AUTOSAVE_DELAY instead of retrying every frame.
    pub fn autosave(&mut self, now: Instant) -> bool {
        if !self.is_autosave_due(now) {
            return false;
        }
        let saved = self.save_unattended();
        if !saved {
            self.edited_at = Some(now);
        }
        saved
    }

    /// Before leaving this note: true when nothing is left unsaved.
    pub fn save_if_dirty(&mut self) -> bool {
        !self.is_dirty() || self.save_unattended()
    }

    /// Like `save`, but for writes nobody asked for: when the file cannot be read
    /// (moved, deleted, not UTF-8) its state is unknown, so nothing is written.
    fn save_unattended(&mut self) -> bool {
        match fs::read_to_string(&self.path) {
            Ok(_) => {
                self.is_unreadable = false;
                self.save()
            }
            Err(err) => {
                self.is_unreadable = true;
                self.error = format!("無法確認檔案現況，先不儲存：{err}");
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
                self.is_unreadable = false;
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

    fn edited(path: &Path, text: &str, at: Instant) -> OpenNote {
        let mut note = OpenNote::load(path).unwrap();
        note.text = text.into();
        note.mark_edited(at);
        note
    }

    #[test]
    fn autosave_waits_for_a_pause_in_typing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let start = Instant::now();
        let mut note = edited(&path, "one two", start);
        assert!(!note.autosave(start + Duration::from_millis(500)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        assert!(note.autosave(start + AUTOSAVE_DELAY));
        assert_eq!(fs::read_to_string(&path).unwrap(), "one two");
        assert!(!note.is_dirty());
        assert!(!note.autosave(start + AUTOSAVE_DELAY * 3), "nothing left to save");
    }

    #[test]
    fn autosave_never_overwrites_an_outside_change() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let start = Instant::now();
        let mut note = edited(&path, "mine", start);
        fs::write(&path, "theirs").unwrap();
        assert!(!note.autosave(start + AUTOSAVE_DELAY));
        assert!(note.has_conflict);
        assert!(!note.autosave(start + AUTOSAVE_DELAY * 5));
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
        assert_eq!(note.text, "mine");
    }

    #[test]
    fn autosave_backs_off_after_a_write_error() {
        let dir = tempdir().unwrap();
        let folder = dir.path().join("gone");
        fs::create_dir(&folder).unwrap();
        let path = folder.join("a.md");
        fs::write(&path, "one").unwrap();
        let start = Instant::now();
        let mut note = edited(&path, "two", start);
        fs::remove_dir_all(&folder).unwrap();
        assert!(!note.autosave(start + AUTOSAVE_DELAY));
        assert!(!note.error.is_empty());
        assert!(!note.is_autosave_due(start + AUTOSAVE_DELAY + Duration::from_millis(500)));
        assert!(note.is_autosave_due(start + AUTOSAVE_DELAY * 2));
        assert_eq!(note.text, "two", "text is kept after a failed write");
    }

    #[test]
    fn autosave_waits_while_an_input_method_is_composing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "hello").unwrap();
        let start = Instant::now();
        let mut note = edited(&path, "helloㄓㄨㄥ", start);
        note.is_composing = true;
        assert!(!note.autosave(start + AUTOSAVE_DELAY * 5));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        note.text = "hello中".into();
        note.is_composing = false;
        note.mark_edited(start + AUTOSAVE_DELAY * 5);
        assert!(note.autosave(start + AUTOSAVE_DELAY * 6));
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello中");
    }

    #[test]
    fn unattended_saves_do_not_recreate_a_moved_or_unreadable_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let start = Instant::now();
        let mut moved = edited(&path, "mine", start);
        fs::remove_file(&path).unwrap();
        assert!(!moved.autosave(start + AUTOSAVE_DELAY));
        assert!(!moved.save_if_dirty());
        assert!(moved.is_unreadable);
        assert!(!path.exists(), "the moved note is not brought back");
        assert_eq!(moved.text, "mine");

        fs::write(&path, "one").unwrap();
        let mut garbled = edited(&path, "mine", start);
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(!garbled.autosave(start + AUTOSAVE_DELAY));
        assert_eq!(fs::read(&path).unwrap(), vec![0xff, 0xfe, 0x00]);
    }

    #[test]
    fn save_if_dirty_skips_clean_notes_and_respects_conflicts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "one").unwrap();
        let mut clean = OpenNote::load(&path).unwrap();
        assert!(clean.save_if_dirty());

        let mut dirty = edited(&path, "two", Instant::now());
        assert!(dirty.save_if_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");

        let mut stale = edited(&path, "mine", Instant::now());
        fs::write(&path, "theirs").unwrap();
        assert!(!stale.save_if_dirty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
    }

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
