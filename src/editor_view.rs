//! Pieces of the editor panel: header, conflict banner, and editor input state.

use crate::editor::OpenNote;
use crate::notes;
use eframe::egui::{self, Id, RichText};

pub fn show_editor_header(ui: &mut egui::Ui, editor: &OpenNote) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(if editor.is_dirty() { "● 編輯中" } else { "✓ 已儲存" });
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
pub fn track_composing(ui: &egui::Ui, editor: &mut OpenNote) {
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
