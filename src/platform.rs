//! macOS bits the window needs: the folder picker and a font with CJK glyphs.

use std::fs;
use std::path::PathBuf;

const CJK_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
];

pub fn cjk_font_bytes() -> Option<Vec<u8>> {
    CJK_FONT_CANDIDATES
        .iter()
        .find_map(|path| fs::read(path).ok())
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
    fn cjk_fallback_font_is_on_this_mac() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let bytes = cjk_font_bytes().expect("system CJK font");
        assert!(bytes.len() > 1000);
    }
}
