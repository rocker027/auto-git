//! The backup tab of the sidebar: the backup state in full, the two switches,
//! 「立即備份」, and the watched folders. It holds everything the old `⋯` menu did.

use super::sidebar::section_label;
use super::NoteSyncApp;
use crate::controls;
use crate::labels::folder_label;
use crate::status::{StatusLine, Tone};
use crate::theme::{self, GUTTER, ON_ACCENT, TOUCH};
use crate::widgets;
use eframe::egui::{self, Color32, Margin, RichText};
use std::path::Path;

const HEADLINE_SIZE: f32 = 17.0;
/// Room for the shadow of the cards inside the scroll area.
const SHADOW_ROOM: Margin = Margin { left: 2, right: 2, top: 2, bottom: 2 };
const PUSH_HINT: &str = "每次備份後，把版本推送到資料夾已設定的遠端（例如 GitHub）。";
const REMOVE_HINT: &str = "不再備份這個資料夾，檔案不會被刪除";

impl NoteSyncApp {
    pub(super) fn show_backup_tab(&mut self, ui: &mut egui::Ui, line: &StatusLine) {
        egui::ScrollArea::vertical()
            // Its own id: the notes tab's list must not share a scroll position with this one.
            .id_salt("backup-tab")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new().inner_margin(SHADOW_ROOM).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = GUTTER;
                    show_status_card(ui, line);
                    self.show_switches(ui);
                    self.show_backup_now(ui);
                    self.show_folder_error(ui);
                    self.show_folders_card(ui);
                });
            });
    }

    /// Two tiles side by side: automatic backup and automatic push.
    fn show_switches(&mut self, ui: &mut egui::Ui) {
        let is_paused = self.scheduler.is_paused();
        let is_push_on = self.config.lock().unwrap().auto_push;
        let backup_state = if is_paused { "已暫停" } else { "開啟" };
        let push_state = if is_push_on { "開啟" } else { "關閉" };
        let (mut backup_clicked, mut push_clicked) = (false, false);
        ui.columns(2, |columns| {
            backup_clicked =
                controls::toggle_tile(&mut columns[0], "自動備份", backup_state, !is_paused).clicked();
            push_clicked = controls::toggle_tile(&mut columns[1], "自動推送", push_state, is_push_on)
                .on_hover_text(PUSH_HINT)
                .clicked();
        });
        if backup_clicked {
            self.scheduler.set_paused(!is_paused);
        }
        if push_clicked {
            self.set_auto_push(!is_push_on);
        }
    }

    /// Off by default: pushing sends the notes to another machine.
    fn set_auto_push(&mut self, is_on: bool) {
        self.update_config(|cfg| cfg.auto_push = is_on);
        if is_on && !self.scheduler.is_paused() {
            self.scheduler.request_now();
        }
    }

    fn show_backup_now(&mut self, ui: &mut egui::Ui) {
        let can_back_up =
            !self.editor_dirty() && !self.scheduler.is_running() && !self.folders().is_empty();
        let button =
            widgets::primary_button("立即備份", 15.0).min_size(egui::vec2(ui.available_width(), TOUCH));
        if widgets::add_primary_button(ui, can_back_up, button).clicked() {
            self.scheduler.request_now();
        }
    }

    fn show_folders_card(&mut self, ui: &mut egui::Ui) {
        let folders = self.folders();
        let mut removed = None;
        let mut add_clicked = false;
        theme::card(ui).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            section_label(ui, "資料夾");
            for folder in &folders {
                if show_folder_row(ui, &folder_label(folder, &folders), folder) {
                    removed = Some(folder.clone());
                }
            }
            add_clicked = ui.add(widgets::quiet_button(ui, "＋ 加入資料夾")).clicked();
        });
        if let Some(folder) = removed {
            self.remove_folder(&folder);
        }
        if add_clicked {
            self.pick_and_add_folder();
        }
    }
}

/// The backup state with its full explanation, which the notes tab only shows on hover.
fn show_status_card(ui: &mut egui::Ui, line: &StatusLine) {
    let colors = theme::of(ui);
    match line.tone {
        Tone::Good => controls::gradient_card(ui, theme::GRADIENT, |ui| {
            show_status_text(ui, line, ON_ACCENT, ON_ACCENT);
        }),
        Tone::Quiet => show_plain_status(ui, line, colors.surface, colors.text),
        Tone::Problem => {
            let color = ui.visuals().error_fg_color;
            show_plain_status(ui, line, theme::tint(color, ui), color);
        }
    }
}

fn show_plain_status(ui: &mut egui::Ui, line: &StatusLine, fill: Color32, headline: Color32) {
    let body = theme::of(ui).text;
    theme::card(ui).fill(fill).inner_margin(16.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        show_status_text(ui, line, headline, body);
    });
}

fn show_status_text(ui: &mut egui::Ui, line: &StatusLine, headline: Color32, body: Color32) {
    ui.spacing_mut().item_spacing.y = 6.0;
    let text = RichText::new(&line.text).size(HEADLINE_SIZE).color(headline);
    ui.add(egui::Label::new(text).wrap());
    if let Some(detail) = &line.detail {
        ui.add(egui::Label::new(RichText::new(detail).small().color(body)).wrap());
    }
}

/// Folder name on the left, 「移除」 on the right. Returns true when removal was asked for.
fn show_folder_row(ui: &mut egui::Ui, label: &str, folder: &Path) -> bool {
    let error = ui.visuals().error_fg_color;
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let remove = egui::Button::new(RichText::new("移除").color(error)).fill(Color32::TRANSPARENT);
            let is_removed = ui.add(remove).on_hover_text(REMOVE_HINT).clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(label).truncate())
                    .on_hover_text(folder.display().to_string());
            });
            is_removed
        })
        .inner
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_card_shows_the_cause_that_used_to_need_a_hover() {
        let line = StatusLine {
            tone: Tone::Problem,
            text: "⚠ 「notes」推送被拒絕".into(),
            detail: Some("遠端有較新的版本，請先 git pull。".into()),
        };
        let ctx = egui::Context::default();
        let mut card_height = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(196.0);
                let top = ui.cursor().min.y;
                show_status_card(ui, &line);
                card_height = ui.cursor().min.y - top;
            });
        });
        let headline_only = 16.0 * 2.0 + HEADLINE_SIZE;
        assert!(card_height > headline_only + 12.0, "detail adds its own lines: {card_height}");
    }
}
