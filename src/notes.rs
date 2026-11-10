//! Lists Markdown notes inside the watched folders and filters them for search.

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
    /// Lower-cased file text used for search.
    haystack: String,
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
    let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut text = fs::read(&path).unwrap_or_default();
    text.truncate(MAX_INDEXED_BYTES);
    let title = note_title(&path);
    let haystack = format!("{}\n{}", title, String::from_utf8_lossy(&text)).to_lowercase();
    Note {
        path,
        title,
        modified,
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
