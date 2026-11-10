//! The 歷史 mode of the editor panel: backed-up versions of the open note,
//! a read-only view of one, and ways to get its text back without touching the note.

use crate::history::{self, HistoryError, NoteVersion, VersionTextError};
use crate::status::time_ago;
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const LIST_WIDTH: f32 = 190.0;

/// Loaded once per note and per chosen version; git never runs on a plain repaint,
/// failures included.
#[derive(Default)]
pub struct HistoryPanel {
    note: Option<PathBuf>,
    versions: Option<Result<Vec<NoteVersion>, HistoryError>>,
    selected: Option<usize>,
    /// Text of the selected version, or why it cannot be shown.
    shown: Option<Result<String, VersionTextError>>,
    notice: String,
}

impl HistoryPanel {
    /// Drops what was loaded, so entering 歷史 again reads fresh versions.
    pub fn forget(&mut self) {
        if self.note.is_some() {
            *self = Self::default();
        }
    }

    /// Returns the path of a note saved from an old version, to be opened.
    /// `can_write` is false while a backup runs, so a half-written copy is never committed.
    pub fn show(&mut self, ui: &mut egui::Ui, note: &Path, git: &Path, can_write: bool) -> Option<PathBuf> {
        if self.note.as_deref() != Some(note) {
            self.load(note, git);
        }
        match &self.versions {
            Some(Err(HistoryError::NoVersions)) | None => {
                ui.weak("還沒有備份過的版本。自動備份建立版本後，就會出現在這裡。");
                return None;
            }
            Some(Err(HistoryError::Failed(raw))) => {
                ui.colored_label(ui.visuals().error_fg_color, "讀不到備份紀錄").on_hover_text(raw);
                return None;
            }
            Some(Ok(_)) => {}
        }
        let mut saved = None;
        ui.horizontal_top(|ui| {
            self.show_version_list(ui, note, git);
            ui.separator();
            ui.vertical(|ui| saved = self.show_selected(ui, note, can_write));
        });
        saved
    }

    fn load(&mut self, note: &Path, git: &Path) {
        *self = Self {
            note: Some(note.to_path_buf()),
            versions: Some(history::note_versions(git, note)),
            ..Self::default()
        };
        if matches!(self.versions, Some(Ok(_))) {
            self.select(0, note, git);
        }
    }

    fn select(&mut self, index: usize, note: &Path, git: &Path) {
        let Some(Ok(versions)) = &self.versions else {
            return;
        };
        let Some(version) = versions.get(index) else {
            return;
        };
        let text = history::note_text_at(git, note, &version.rev);
        self.selected = Some(index);
        self.shown = Some(text);
        self.notice.clear();
    }

    fn show_version_list(&mut self, ui: &mut egui::Ui, note: &Path, git: &Path) {
        let Some(Ok(versions)) = &self.versions else {
            return;
        };
        let now = SystemTime::now();
        let mut clicked = None;
        ui.vertical(|ui| {
            ui.set_width(LIST_WIDTH);
            ui.label(RichText::new("只列出已備份的版本，不含改名前的").small().weak());
            egui::ScrollArea::vertical().id_salt("history-list").show(ui, |ui| {
                for (index, version) in versions.iter().enumerate() {
                    let age = time_ago(now.duration_since(version.committed_at).unwrap_or_default());
                    let label = format!("{}\n{age}", version.when);
                    if ui.selectable_label(self.selected == Some(index), label).clicked() {
                        clicked = Some(index);
                    }
                }
            });
        });
        if let Some(index) = clicked.filter(|index| self.selected != Some(*index)) {
            self.select(index, note, git);
        }
    }

    fn show_selected(&mut self, ui: &mut egui::Ui, note: &Path, can_write: bool) -> Option<PathBuf> {
        let text = match &self.shown {
            Some(Ok(text)) => text,
            Some(Err(VersionTextError::NotText)) => {
                ui.colored_label(ui.visuals().error_fg_color, "這個版本不是文字，無法顯示");
                return None;
            }
            Some(Err(VersionTextError::Failed(raw))) => {
                ui.colored_label(ui.visuals().error_fg_color, "這個版本無法顯示").on_hover_text(raw);
                return None;
            }
            None => return None,
        };
        let when = self.selected_when().unwrap_or_default();
        let mut saved = None;
        ui.horizontal(|ui| {
            if ui.button("複製內容").clicked() {
                ui.ctx().copy_text(text.clone());
                self.notice = "已複製到剪貼簿".into();
            }
            let save_button = ui
                .add_enabled(can_write, egui::Button::new("另存成新筆記"))
                .on_hover_text("原本的筆記不會被改動")
                .on_disabled_hover_text("正在備份，請稍候");
            if save_button.clicked() {
                match history::save_as_new_note(note, &when, text) {
                    Ok(path) => saved = Some(path),
                    Err(err) => self.notice = format!("另存失敗：{err:#}"),
                }
            }
            ui.weak(&self.notice);
        });
        show_read_only(ui, text);
        saved
    }

    fn selected_when(&self) -> Option<String> {
        let Some(Ok(versions)) = &self.versions else {
            return None;
        };
        versions.get(self.selected?).map(|version| version.when.clone())
    }
}

/// Selectable but not editable; the note in the editor is never changed from here.
fn show_read_only(ui: &mut egui::Ui, text: &str) {
    let mut view = text;
    egui::ScrollArea::vertical()
        .id_salt("history-text")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut view)
                    .frame(false)
                    .font(egui::FontId::proportional(16.0)),
            );
        });
}
