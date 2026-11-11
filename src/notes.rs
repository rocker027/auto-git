//! Lists Markdown notes inside the watched folders and filters them for search.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_NOTES_PER_FOLDER: usize = 5000;
const MAX_INDEXED_BYTES: usize = 256 * 1024;
const SKIPPED_DIRS: &[&str] = &["node_modules", "target"];

#[derive(Debug, Clone)]
pub struct Note {
    pub path: PathBuf,
    pub title: String,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// Lower-cased file text used for search.
    haystack: String,
}

impl Note {
    /// A newly created copy can have an older preserved modification time.
    pub fn updated_at(&self) -> Option<SystemTime> {
        self.created.max(self.modified)
    }
}

/// Latest notes across watched folders; overlapping roots do not duplicate a file.
pub fn recent_notes(library: &[FolderNotes]) -> Vec<&Note> {
    let mut seen = HashSet::new();
    let mut notes: Vec<_> = library.iter().flat_map(|group| &group.notes)
        .filter(|note| seen.insert(&note.path)).collect();
    notes.sort_by(|a, b| b.updated_at().cmp(&a.updated_at()).then(a.path.cmp(&b.path)));
    notes.truncate(30);
    notes
}

#[derive(Debug, Clone)]
pub struct FolderNotes {
    pub folder: PathBuf,
    pub exists: bool,
    pub notes: Vec<Note>,
}

pub fn scan(folders: &[PathBuf]) -> Vec<FolderNotes> {
    folders
        .iter()
        .map(|folder| {
            let mut notes = Vec::new();
            let exists = folder.is_dir();
            if exists {
                walk(folder, &mut notes);
            }
            notes.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.title.cmp(&b.title)));
            FolderNotes {
                folder: folder.clone(),
                exists,
                notes,
            }
        })
        .collect()
}

fn walk(dir: &Path, notes: &mut Vec<Note>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if notes.len() >= MAX_NOTES_PER_FOLDER {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(&path, notes);
            }
        } else if kind.is_file() && is_markdown(&path) {
            notes.push(read_note(path));
        }
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
        .unwrap_or(false)
}

fn read_note(path: PathBuf) -> Note {
    let metadata = fs::metadata(&path).ok();
    let modified = metadata.as_ref().and_then(|m| m.modified().ok());
    let created = metadata.as_ref().and_then(|m| m.created().ok());
    let mut text = fs::read(&path).unwrap_or_default();
    text.truncate(MAX_INDEXED_BYTES);
    let title = note_title(&path);
    let haystack = format!("{}\n{}", title, String::from_utf8_lossy(&text)).to_lowercase();
    Note {
        path,
        title,
        modified,
        created,
        haystack,
    }
}

/// File name without the Markdown extension.
pub fn note_title(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    if is_markdown(path) {
        if let Some(stem) = path.file_stem() {
            return stem.to_string_lossy().into_owned();
        }
    }
    name
}

/// Lower-cased words of a search query.
pub fn search_words(query: &str) -> Vec<String> {
    query.to_lowercase().split_whitespace().map(str::to_string).collect()
}

/// Every word must appear in the title or the text.
pub fn matches(note: &Note, words: &[String]) -> bool {
    words.iter().all(|word| note.haystack.contains(word.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::time::{Duration, UNIX_EPOCH};

    fn sample(path: &str, created: Option<u64>, modified: Option<u64>) -> Note {
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
        Note {
            path: PathBuf::from(path), title: note_title(Path::new(path)),
            created: created.map(at), modified: modified.map(at), haystack: String::new(),
        }
    }

    #[test]
    fn recent_notes_merge_roots_deduplicate_and_limit_to_thirty() {
        let mut library: Vec<_> = ["a", "b"].into_iter().enumerate().map(|(root, name)| {
            FolderNotes {
                folder: PathBuf::from(name), exists: true,
                notes: (0..20).map(|i| sample(
                    &format!("{name}/{i}.md"), None, Some((i * 2 + root) as u64),
                )).collect(),
            }
        }).collect();
        library.push(library[1].clone());
        let recent = recent_notes(&library);
        assert_eq!(recent.len(), 30);
        assert_eq!(recent.first().unwrap().path, Path::new("b/19.md"));
        assert_eq!(recent.last().unwrap().path, Path::new("a/5.md"));
        assert_eq!(recent.iter().map(|note| &note.path).collect::<HashSet<_>>().len(), 30);
        assert!(recent.windows(2).all(|pair| pair[0].updated_at() > pair[1].updated_at()));
    }

    #[test]
    fn recent_order_uses_newer_creation_or_modification_with_stable_ties() {
        let mut group = FolderNotes {
            folder: PathBuf::from("notes"), exists: true,
            notes: vec![
                sample("unknown.md", None, None),
                sample("edited.md", Some(1), Some(50)),
                sample("copy.md", Some(60), Some(2)),
                sample("b.md", None, Some(30)),
                sample("a.md", Some(30), None),
            ],
        };
        let paths: Vec<_> = recent_notes(std::slice::from_ref(&group)).iter()
            .map(|note| note.path.to_string_lossy().into_owned()).collect();
        assert_eq!(paths, ["copy.md", "edited.md", "a.md", "b.md", "unknown.md"]);
        group.notes[1].modified = Some(UNIX_EPOCH + Duration::from_secs(70));
        assert_eq!(recent_notes(&[group])[0].path, Path::new("edited.md"));
        assert!(recent_notes(&[]).is_empty());
    }

    #[test]
    fn scan_refreshes_recent_order_after_a_file_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("edited.md");
        fs::write(&path, "first note").unwrap();
        fs::write(dir.path().join("other.md"), "second note").unwrap();
        let later = SystemTime::now() + Duration::from_secs(60);
        fs::File::options().write(true).open(&path).unwrap()
            .set_times(fs::FileTimes::new().set_modified(later)).unwrap();
        let library = scan(&[dir.path().to_path_buf()]);
        let recent = recent_notes(&library);
        assert_eq!(recent[0].path, path);
        assert_eq!(recent[0].modified, Some(later));
        assert_eq!(recent[0].created, fs::metadata(&path).unwrap().created().ok());
    }

    #[test]
    fn scan_finds_markdown_and_skips_hidden_folders() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a.md"), "Hello").unwrap();
        fs::write(root.join("sub/b.markdown"), "World").unwrap();
        fs::write(root.join("c.txt"), "skip").unwrap();
        fs::write(root.join(".git/d.md"), "skip").unwrap();

        let found = scan(&[root.to_path_buf()]);
        assert_eq!(found.len(), 1);
        assert!(found[0].exists);
        let mut titles: Vec<_> = found[0].notes.iter().map(|n| n.title.clone()).collect();
        titles.sort();
        assert_eq!(titles, vec!["a", "b"]);
    }

    #[test]
    fn scan_marks_missing_folder() {
        let found = scan(&[PathBuf::from("/definitely/not/here")]);
        assert!(!found[0].exists);
        assert!(found[0].notes.is_empty());
    }

    #[test]
    fn matches_needs_every_word_in_title_or_text() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("Shopping.md");
        fs::write(&path, "Milk and 雞蛋").unwrap();
        let note = read_note(path);
        assert!(matches(&note, &search_words("shopping")));
        assert!(matches(&note, &search_words("MILK 雞蛋")));
        assert!(matches(&note, &search_words("")));
        assert!(!matches(&note, &search_words("milk bread")));
    }
}
