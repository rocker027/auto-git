//! macOS bits the window needs: the folder picker and a font with CJK glyphs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CJK_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
];

pub fn cjk_font_bytes() -> Option<Vec<u8>> {
    CJK_FONT_CANDIDATES
        .iter()
        .find_map(|path| fs::read(path).ok())
}

/// Keep one immutable copy so egui can borrow it instead of cloning each font.
pub fn shared_cjk_font() -> Option<&'static [u8]> {
    static FONT: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    FONT.get_or_init(cjk_font_bytes).as_deref()
}

pub fn shared_system_ui_font() -> Option<&'static [u8]> {
    static FONT: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    FONT.get_or_init(|| fs::read("/System/Library/Fonts/SFNS.ttf").ok()).as_deref()
}

pub fn shared_system_mono_font() -> Option<&'static [u8]> {
    static FONT: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    FONT.get_or_init(|| fs::read("/System/Library/Fonts/SFNSMono.ttf").ok()).as_deref()
}

/// Reveal the note in Finder, without opening it in another editor.
pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    let path = fs::canonicalize(path).map_err(|err| format!("找不到筆記：{err}"))?;
    let output = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .output()
        .map_err(|err| format!("無法開啟 Finder：{err}"))?;
    if !output.status.success() {
        return Err(format!("無法開啟 Finder：{}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}

/// The system folder picker. Ok(None) when the user cancels.
pub fn choose_folder() -> Result<Option<PathBuf>, String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("POSIX path of (choose folder with prompt \"選擇筆記資料夾\")")
        .output()
        .map_err(|err| format!("無法開啟選擇視窗：{err}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout).map_err(|err| err.to_string())?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(trimmed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_bytes_are_shared_across_requests() {
        if !cfg!(target_os = "macos") { return; }
        let first = shared_cjk_font().unwrap();
        let second = shared_cjk_font().unwrap();
        assert!(std::ptr::eq(first, second));
        let data = eframe::egui::FontData::from_static(first);
        assert!(matches!(data.font, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn cjk_fallback_font_is_on_this_mac() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let bytes = cjk_font_bytes().expect("system CJK font");
        assert!(bytes.len() > 1000);
    }
}
