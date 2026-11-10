//! Pieces of the editor panel: header, conflict banner, and editor input state.

use crate::editor::OpenNote;
use crate::notes;
use eframe::egui::{self, Id, RichText};
use std::time::Instant;

pub fn show_editor_header(ui: &mut egui::Ui, editor: &OpenNote, is_preview: &mut bool) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(if editor.is_dirty() { "● 編輯中" } else { "✓ 已儲存" });
            ui.add_space(12.0);
            show_mode_switch(ui, is_preview);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(notes::note_title(&editor.path)).heading()).truncate())
                    .on_hover_text(editor.path.display().to_string());
            });
        });
    });
    if !editor.error.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, &editor.error);
    }
    ui.add_space(8.0);
}

/// Two-segment 編輯｜預覽 switch; ⌘E does the same.
/// Added right to left, so 預覽 comes first.
fn show_mode_switch(ui: &mut egui::Ui, is_preview: &mut bool) {
    ui.spacing_mut().item_spacing.x = 2.0;
    if ui.selectable_label(*is_preview, "預覽").on_hover_text("⌘E").clicked() {
        *is_preview = true;
    }
    if ui.selectable_label(!*is_preview, "編輯").on_hover_text("⌘E").clicked() {
        *is_preview = false;
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

/// Rendered Markdown of the open note. Read only.
pub fn show_preview(ui: &mut egui::Ui, text: &str, cache: &mut egui_commonmark::CommonMarkCache) {
    egui::ScrollArea::vertical()
        .id_salt("note-preview")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui_commonmark::CommonMarkViewer::new().show(ui, cache, text);
        });
}

/// Returns true when the user takes the version from disk.
pub fn show_conflict_banner(ui: &mut egui::Ui) -> bool {
    let mut reload = false;
    egui::Frame::new()
        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.15))
        .corner_radius(6.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
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
