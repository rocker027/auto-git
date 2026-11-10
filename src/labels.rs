//! Short display names for folders and search hits.

use crate::qmd_ops::SearchHit;
use std::path::{Path, PathBuf};

pub fn folder_label(path: &Path, folders: &[PathBuf]) -> String {
    let name = file_name(path);
    let shared = folders
        .iter()
        .filter(|other| other.file_name() == path.file_name())
        .count()
        > 1;
    match path.parent().and_then(Path::file_name) {
        Some(parent) if shared => format!("{}/{}", parent.to_string_lossy(), name),
        _ => name,
    }
}

pub fn hit_label(hit: &SearchHit, hits: &[SearchHit]) -> String {
    let name = file_name(&hit.file);
    let title = hit
        .title
        .as_deref()
        .map(single_line)
        .filter(|title| !title.is_empty());
    let Some(title) = title else {
        return name;
    };
    let shared = hits
        .iter()
        .filter(|other| other.title.as_deref().map(single_line).as_deref() == Some(title.as_str()))
        .count()
        > 1;
    if shared {
        format!("{title} · {name}")
    } else {
        title
    }
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_label_uses_the_name_until_names_collide() {
        let notes = PathBuf::from("/Users/a/Notes");
        let work = PathBuf::from("/Users/a/Work");
        assert_eq!(folder_label(&notes, &[notes.clone(), work]), "Notes");
        let other = PathBuf::from("/Users/b/Notes");
        assert_eq!(folder_label(&notes, &[notes.clone(), other]), "a/Notes");
    }

    #[test]
    fn hit_label_hides_the_path_until_titles_collide() {
        let daily = sample_hit("/notes/daily.md", Some("日記"));
        let other = sample_hit("/notes/other.md", Some("待辦"));
        assert_eq!(hit_label(&daily, &[daily.clone(), other]), "日記");
        let same = sample_hit("/work/daily.md", Some("日記"));
        assert_eq!(hit_label(&daily, &[daily.clone(), same]), "日記 · daily.md");
        let untitled = sample_hit("/notes/loose.md", None);
        assert_eq!(hit_label(&untitled, &[untitled.clone()]), "loose.md");
        let wrapped = sample_hit("/notes/intent.md", Some("Intent — 第 2 批\n；下一行不換行"));
        assert_eq!(
            hit_label(&wrapped, &[wrapped.clone()]),
            "Intent — 第 2 批 ；下一行不換行"
        );
    }

    fn sample_hit(file: &str, title: Option<&str>) -> SearchHit {
        SearchHit {
            file: PathBuf::from(file),
            title: title.map(str::to_string),
            snippet: None,
            score: None,
        }
    }
}
