//! Pieces of the editor panel: header, conflict banner, and editor input state.

use crate::editor::OpenNote;
use crate::notes;
use eframe::egui::{self, Id, RichText};
use std::time::Instant;

/// What the editor panel shows for the open note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Edit,
    Preview,
    /// Backed-up versions, read only.
    History,
}

pub fn show_editor_header(ui: &mut egui::Ui, editor: &OpenNote, mode: &mut ViewMode) {
    ui.add(egui::Label::new(RichText::new(notes::note_title(&editor.path)).heading().strong()).truncate())
        .on_hover_text(editor.path.display().to_string());
    ui.horizontal_wrapped(|ui| {
        show_mode_switch(ui, mode);
        ui.add_space(4.0);
        ui.label(RichText::new(if editor.is_dirty() { "編輯中" } else { "已儲存" }).small().color(crate::widgets::secondary_color(ui)));
    });
    if !editor.error.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, &editor.error);
    }
    ui.add_space(8.0);
}

/// 編輯｜預覽｜歷史 switch; ⌘E flips between 編輯 and 預覽.
/// Kept separate from the title so every mode stays reachable in narrow windows.
fn show_mode_switch(ui: &mut egui::Ui, mode: &mut ViewMode) {
    let segments = [
        (ViewMode::Edit, "編輯", "⌘E"),
        (ViewMode::Preview, "預覽", "⌘E"),
        (ViewMode::History, "歷史", "已備份的版本"),
    ];
    crate::controls::segmented(ui, mode, &segments);
}

/// ⌘E: 編輯 ↔ 預覽; from 歷史 it goes back to 編輯.
pub fn toggled_mode(mode: ViewMode) -> ViewMode {
    match mode {
        ViewMode::Edit => ViewMode::Preview,
        ViewMode::Preview | ViewMode::History => ViewMode::Edit,
    }
}

/// The note text editor; records edits for autosave.
pub fn show_text_editor(ui: &mut egui::Ui, editor: &mut OpenNote, editor_id: Id) {
    track_composing(ui, editor);
    let edited: egui::Response = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut editor.text)
                    .id(editor_id)
                    .lock_focus(true)
                    .frame(false)
                    .font(egui::FontId::proportional(16.0)),
            )
        })
        .inner;
    if edited.changed() {
        editor.mark_edited(Instant::now());
    }
    if !edited.has_focus() {
        editor.is_composing = false;
    }
}

/// Larger notes are not rendered: the viewer lays out the whole text every frame.
const PREVIEW_MAX_BYTES: usize = 100_000;
/// Least height of a line, task box or table row in the preview.
const PREVIEW_ROW_HEIGHT: f32 = 28.0;
/// Largest heading in the preview; the app's own titles are larger.
const PREVIEW_HEADING_SIZE: f32 = 22.0;

/// Rendered Markdown of the open note. Read only.
/// Returns the non-web links clicked this frame; they are not opened here.
pub fn show_preview(
    ui: &mut egui::Ui,
    text: &str,
    cache: &mut egui_commonmark::CommonMarkCache,
) -> Vec<String> {
    if text.len() > PREVIEW_MAX_BYTES {
        crate::widgets::secondary_label(ui, "筆記太大，請用編輯模式");
        return Vec::new();
    }
    ui.style_mut().url_in_tooltip = true;
    let before = ui.ctx().output(|output| output.commands.len());
    egui::ScrollArea::both()
        .id_salt("note-preview")
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            ui.set_width(viewport.width());
            show_markdown(ui, cache, text, viewport.width());
        });
    take_local_links(ui.ctx(), before)
}

/// The rendered note. It is content, not controls: lines, task boxes and table rows
/// are sized by their text here, not by the touch height the rest of the app uses.
fn show_markdown(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    text: &str,
    width: f32,
) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.spacing_mut().interact_size.y = PREVIEW_ROW_HEIGHT;
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(PREVIEW_HEADING_SIZE));
    egui_commonmark::CommonMarkViewer::new()
        .max_image_width(Some(width as usize))
        .show(ui, cache, text);
}

/// Keeps web and mail links for the system; pulls every other link the preview
/// added (from index `from` on) out of egui's output.
fn take_local_links(ctx: &egui::Context, from: usize) -> Vec<String> {
    ctx.output_mut(|output| {
        let mut local = Vec::new();
        let mut index = 0;
        output.commands.retain(|command| {
            let is_mine = index >= from;
            index += 1;
            match command {
                egui::OutputCommand::OpenUrl(open) if is_mine && !is_web_link(&open.url) => {
                    local.push(open.url.clone());
                    false
                }
                _ => true,
            }
        });
        local
    })
}

fn is_web_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// Decodes %XX escapes (UTF-8), leaving malformed ones as they are.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(value)) => {
                out.push(value);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Removes `.` and `..` by the text of the path, without touching the disk,
/// so the result compares equal to the paths in the note list.
fn tidy_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    path.components().fold(std::path::PathBuf::new(), |mut tidy, part| {
        match part {
            Component::ParentDir => {
                tidy.pop();
            }
            Component::CurDir => {}
            other => tidy.push(other),
        }
        tidy
    })
}

/// A relative link to another Markdown note, resolved next to the open note.
/// Anything else (file://, app schemes, missing files) gives None.
pub fn resolve_note_link(note: &std::path::Path, url: &str) -> Option<std::path::PathBuf> {
    if url.contains("://") || url.starts_with('/') || url.contains(':') {
        return None;
    }
    let target = percent_decode(url.split(['#', '?']).next()?);
    let target = target.trim_start_matches('<').trim_end_matches('>');
    if target.is_empty() {
        return None;
    }
    let path = tidy_path(&note.parent()?.join(target));
    let is_markdown = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"));
    (is_markdown && path.is_file()).then_some(path)
}

/// Returns true when the user takes the version from disk.
pub fn show_conflict_banner(ui: &mut egui::Ui) -> bool {
    let mut reload = false;
    egui::Frame::new()
        .fill(crate::theme::tint(ui.visuals().warn_fg_color, ui))
        .corner_radius(crate::theme::CONTROL_RADIUS)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("⚠ 這則筆記在別的地方被改過了");
                reload = ui.button("改用最新版本").clicked();
            });
        });
    ui.add_space(8.0);
    reload
}

/// Follows input-method composition so autosave never writes unpicked candidates.
/// Read before the TextEdit runs, which drops IME events when focus moves.
fn track_composing(ui: &egui::Ui, editor: &mut OpenNote) {
    ui.input(|input| {
        for event in &input.events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
                    editor.is_composing = !text.is_empty();
                }
                egui::Event::Ime(egui::ImeEvent::Commit(_) | egui::ImeEvent::Disabled) => {
                    editor.is_composing = false;
                }
                _ => {}
            }
        }
    });
}

pub fn forget_undo(ctx: &egui::Context, id: Id) {
    if let Some(mut state) = egui::text_edit::TextEditState::load(ctx, id) {
        state.clear_undoer();
        state.store(ctx, id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn note_links_resolve_next_to_the_open_note_only() {
        let dir = tempdir().unwrap();
        let note = dir.path().join("a.md");
        fs::write(&note, "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/other note.md"), "y").unwrap();
        assert_eq!(
            resolve_note_link(&note, "sub/other%20note.md#part"),
            Some(dir.path().join("sub/other note.md"))
        );
        assert_eq!(resolve_note_link(&note, "missing.md"), None);
        assert_eq!(resolve_note_link(&note, "file:///etc/hosts"), None);
        assert_eq!(resolve_note_link(&note, "/etc/hosts"), None);
        assert_eq!(resolve_note_link(&note, "x-apple.systempreferences:com.apple"), None);
        assert_eq!(resolve_note_link(&note, "#heading"), None);
        assert_eq!(resolve_note_link(&note, "../sub/../a.md"), None, "outside the temp dir");
        assert_eq!(
            resolve_note_link(&dir.path().join("sub/other note.md"), "../a.md?x=1"),
            Some(dir.path().join("a.md")),
            "`..` is folded so the path matches the note list"
        );
        fs::write(dir.path().join("中文.md"), "z").unwrap();
        assert_eq!(
            resolve_note_link(&note, "%E4%B8%AD%E6%96%87.md"),
            Some(dir.path().join("中文.md"))
        );
        fs::write(dir.path().join("run.sh"), "echo").unwrap();
        assert_eq!(resolve_note_link(&note, "run.sh"), None);
    }

    /// Height the preview takes for `text` under the app's style.
    fn preview_height(text: &str) -> f32 {
        let ctx = egui::Context::default();
        crate::theme::apply_style(&ctx);
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let mut height = 0.0;
        // Twice: the viewer measures some rows on the first frame and places them on the next.
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().min.y;
                    show_markdown(ui, &mut cache, text, 600.0);
                    height = ui.cursor().min.y - top;
                });
            });
        }
        height
    }

    #[test]
    fn preview_rows_are_sized_by_their_text_not_by_the_touch_height() {
        let touch = crate::theme::TOUCH;
        let tasks = preview_height("- [ ] one\n- [x] two\n- [ ] three\n- [ ] four\n");
        assert!(tasks > 40.0 && tasks < touch * 4.0, "task list: {tasks}");
        let table = preview_height("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |\n");
        assert!(table > 40.0 && table < touch * 4.0, "table of four rows: {table}");
        let lines = preview_height("one\n\ntwo\n\nthree\n\nfour\n");
        assert!(lines > 40.0 && lines < touch * 4.0, "four paragraphs: {lines}");
    }

    #[test]
    fn command_e_leaves_history_for_the_editor() {
        assert_eq!(toggled_mode(ViewMode::Edit), ViewMode::Preview);
        assert_eq!(toggled_mode(ViewMode::Preview), ViewMode::Edit);
        assert_eq!(toggled_mode(ViewMode::History), ViewMode::Edit);
    }

    #[test]
    fn only_web_links_go_to_the_browser() {
        assert!(is_web_link("https://example.com"));
        assert!(is_web_link("http://example.com"));
        assert!(is_web_link("HTTPS://example.com"));
        assert!(is_web_link("mailto:a@example.com"));
        assert!(!is_web_link("file:///etc/hosts"));
        assert!(!is_web_link("other.md"));
    }
}
