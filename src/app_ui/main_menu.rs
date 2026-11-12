//! The `⋯` menu next to the backup status line.

use super::NoteSyncApp;
use crate::labels::folder_label;
use eframe::egui::{self, RichText};

impl NoteSyncApp {
    pub(super) fn show_main_menu(&mut self, ui: &mut egui::Ui) {
        let can_back_up =
            !self.editor_dirty() && !self.scheduler.is_running() && !self.folders().is_empty();
        ui.menu_button(RichText::new("⋯").size(18.0), |ui| {
            if ui.add_enabled(can_back_up, egui::Button::new("立即備份")).clicked() {
                self.scheduler.request_now();
                ui.close_menu();
            }
            let paused = self.scheduler.is_paused();
            let pause_text = if paused { "繼續自動備份" } else { "暫停自動備份" };
            if ui.button(pause_text).clicked() {
                self.scheduler.set_paused(!paused);
                ui.close_menu();
            }
            self.show_push_toggle(ui);
            ui.separator();
            if ui.button("加入資料夾…").clicked() {
                ui.close_menu();
                self.pick_and_add_folder();
            }
            self.show_remove_menu(ui);
        });
    }

    fn show_remove_menu(&mut self, ui: &mut egui::Ui) {
        let folders = self.folders();
        let mut removed = None;
        ui.add_enabled_ui(!folders.is_empty(), |ui| {
            ui.menu_button("移除資料夾", |ui| {
                for folder in &folders {
                    if ui.button(folder_label(folder, &folders)).clicked() {
                        removed = Some(folder.clone());
                        ui.close_menu();
                    }
                }
            });
        });
        if let Some(folder) = removed {
            self.remove_folder(&folder);
        }
    }

    /// Off by default: pushing sends the notes to another machine.
    fn show_push_toggle(&mut self, ui: &mut egui::Ui) {
        let is_on = self.config.lock().unwrap().auto_push;
        let text = if is_on { "關閉自動推送" } else { "開啟自動推送" };
        let button = ui
            .button(text)
            .on_hover_text("每次備份後，把版本推送到資料夾已設定的遠端（例如 GitHub）。");
        if !button.clicked() {
            return;
        }
        ui.close_menu();
        self.update_config(|cfg| cfg.auto_push = !is_on);
        if !is_on && !self.scheduler.is_paused() {
            self.scheduler.request_now();
        }
    }
}
