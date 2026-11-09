use crate::config::AppConfig;
use crate::git_ops::{self, CommitOutcome};
use crate::qmd_ops::{self, SearchHit};
use crate::scheduler::Scheduler;
use eframe::egui;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CJK_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
];

pub struct NoteSyncApp {
    config: Arc<Mutex<AppConfig>>,
    scheduler: Scheduler,
    dirty_editor: Arc<AtomicBool>,
    search_query: String,
    search_hits: Vec<SearchHit>,
    search_error: String,
    searched: bool,
    open_path: Option<PathBuf>,
    editor_text: String,
    disk_fingerprint: u64,
    disk_conflict: bool,
    just_saved: bool,
    editor_notice: String,
    folder_notice: String,
    backup_notice: String,
    seen_scheduler: String,
    show_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
    _tray: Option<tray_icon::TrayIcon>,
    _show_item: tray_icon::menu::MenuItem,
    _quit_item: tray_icon::menu::MenuItem,
}

impl NoteSyncApp {
    pub fn new(
        config: Arc<Mutex<AppConfig>>,
        dirty_editor: Arc<AtomicBool>,
        scheduler: Scheduler,
        tray: Option<tray_icon::TrayIcon>,
        show_item: tray_icon::menu::MenuItem,
        quit_item: tray_icon::menu::MenuItem,
    ) -> Self {
        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();
        Self {
            config,
            scheduler,
            dirty_editor,
            search_query: String::new(),
            search_hits: Vec::new(),
            search_error: String::new(),
            searched: false,
            open_path: None,
            editor_text: String::new(),
            disk_fingerprint: 0,
            disk_conflict: false,
            just_saved: false,
            editor_notice: String::new(),
            folder_notice: String::new(),
            backup_notice: String::new(),
            seen_scheduler: String::new(),
            show_id,
            quit_id,
            _tray: tray,
            _show_item: show_item,
            _quit_item: quit_item,
        }
    }

    fn mark_dirty(&self, dirty: bool) {
        self.dirty_editor.store(dirty, Ordering::Relaxed);
    }

    fn is_dirty(&self) -> bool {
        if let Some(path) = &self.open_path {
            let on_disk = fs::read_to_string(path).unwrap_or_default();
            return on_disk != self.editor_text;
        }
        false
    }

    fn has_folders(&self) -> bool {
        !self.config.lock().unwrap().folders.is_empty()
    }

    fn run_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            self.search_hits.clear();
            self.search_error.clear();
            self.searched = false;
            return;
        }
        self.search_error.clear();
        let (qmd, limit, collection) = {
            let cfg = self.config.lock().unwrap();
            (
                cfg.qmd_path.clone(),
                cfg.search_limit,
                cfg.qmd_collection.clone(),
            )
        };
        match qmd_ops::search(
            &qmd,
            &query,
            limit,
            collection.as_deref(),
            Duration::from_secs(20),
        ) {
            Ok(hits) => {
                self.search_hits = hits;
                self.searched = true;
            }
            Err(err) => {
                self.search_hits.clear();
                self.search_error = err.to_string();
                self.searched = true;
            }
        }
    }

    fn open_hit(&mut self, path: PathBuf) {
        let roots = self.config.lock().unwrap().folders.clone();
        if !roots.is_empty() && !qmd_ops::path_allowed(&path, &roots) {
            self.editor_notice = "這篇不在已加入的資料夾裡".into();
            return;
        }
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.disk_fingerprint = fingerprint(&text);
                self.editor_text = text;
                self.open_path = Some(path);
                self.disk_conflict = false;
                self.just_saved = false;
                self.editor_notice.clear();
                self.mark_dirty(false);
            }
            Err(err) => self.editor_notice = err.to_string(),
        }
    }

    fn save_open(&mut self) {
        let Some(path) = self.open_path.clone() else {
            return;
        };
        if let Ok(on_disk) = fs::read_to_string(&path) {
            if fingerprint(&on_disk) != self.disk_fingerprint && on_disk != self.editor_text {
                self.disk_conflict = true;
                return;
            }
        }
        match fs::write(&path, &self.editor_text) {
            Ok(()) => {
                self.disk_fingerprint = fingerprint(&self.editor_text);
                self.disk_conflict = false;
                self.just_saved = true;
                self.editor_notice.clear();
                self.mark_dirty(false);
            }
            Err(err) => self.editor_notice = err.to_string(),
        }
    }

    fn reload_open(&mut self) {
        let Some(path) = self.open_path.clone() else {
            return;
        };
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.disk_fingerprint = fingerprint(&text);
                self.editor_text = text;
                self.disk_conflict = false;
                self.just_saved = false;
                self.editor_notice.clear();
                self.mark_dirty(false);
            }
            Err(err) => self.editor_notice = err.to_string(),
        }
    }

    fn commit_now(&mut self) {
        if self.is_dirty() || !self.has_folders() {
            return;
        }
        let (folders, git, skip) = {
            let cfg = self.config.lock().unwrap();
            (
                cfg.folders.clone(),
                cfg.git_path.clone(),
                cfg.skip_if_staged,
            )
        };
        let mut errors = Vec::new();
        let mut committed = false;
        let mut blocked = false;
        for folder in folders {
            match git_ops::auto_commit(&git, &folder, skip) {
                Ok(CommitOutcome::Committed { .. }) => committed = true,
                Ok(CommitOutcome::SkippedNoChanges) => {}
                Ok(CommitOutcome::SkippedStagedPresent | CommitOutcome::SkippedBusyState(_)) => {
                    blocked = true;
                }
                Err(err) => errors.push(err.to_string()),
            }
        }
        self.backup_notice = manual_backup_notice(committed, blocked, &errors);
    }

    fn add_folder(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.folder_notice = "這不是資料夾".into();
            return;
        }
        let saved = {
            let mut cfg = self.config.lock().unwrap();
            cfg.add_folder(path);
            cfg.save()
        };
        self.folder_notice = match saved {
            Ok(()) => String::new(),
            Err(err) => err.to_string(),
        };
    }

    fn remove_saved_folder(&mut self, path: &Path) {
        let saved = {
            let mut cfg = self.config.lock().unwrap();
            cfg.remove_folder(path);
            cfg.save()
        };
        self.folder_notice = match saved {
            Ok(()) => String::new(),
            Err(err) => err.to_string(),
        };
    }

    fn sync_backup_notice(&mut self) {
        let raw = self.scheduler.status_message();
        if raw == self.seen_scheduler {
            return;
        }
        self.seen_scheduler = raw.clone();
        if let Some(line) = backup_line(&raw) {
            self.backup_notice = line;
        }
    }

    fn handle_tray(&self, ctx: &egui::Context) {
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id == self.show_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id == self.quit_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn handle_save_shortcut(&mut self, ctx: &egui::Context) {
        let pressed =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        if pressed && self.open_path.is_some() && !self.disk_conflict {
            self.save_open();
        }
    }

    fn show_search(&mut self, ui: &mut egui::Ui) {
        let mut submit = false;
        ui.horizontal(|ui| {
            let field_width = (ui.available_width() - 56.0).max(40.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text("搜尋筆記")
                    .desired_width(field_width),
            );
            let clicked = ui.button("搜尋").clicked();
            let entered =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            submit = clicked || entered;
        });
        if submit {
            self.run_search();
        }
        if !self.search_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.search_error);
        }
    }

    fn show_hits(&mut self, ui: &mut egui::Ui) {
        if !self.search_error.is_empty() {
            return;
        }
        if self.searched && self.search_hits.is_empty() {
            ui.label("沒有找到");
            return;
        }
        let hits = self.search_hits.clone();
        let mut chosen = None;
        for hit in &hits {
            let selected = self.open_path.as_ref() == Some(&hit.file);
            if ui
                .selectable_label(selected, hit_label(hit, &hits))
                .clicked()
            {
                chosen = Some(hit.file.clone());
            }
        }
        if let Some(path) = chosen {
            self.open_hit(path);
        }
    }

    fn show_folders(&mut self, ui: &mut egui::Ui) {
        let folders = self.config.lock().unwrap().folders.clone();
        let mut remove_path = None;
        for folder in &folders {
            ui.horizontal(|ui| {
                ui.label(folder_label(folder, &folders));
                if ui.button("移除").clicked() {
                    remove_path = Some(folder.clone());
                }
            });
        }
        if ui.button("加入資料夾").clicked() {
            self.pick_and_add_folder();
        }
        if let Some(path) = remove_path {
            self.remove_saved_folder(&path);
        }
        if !self.folder_notice.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.folder_notice);
        }
    }

    fn pick_and_add_folder(&mut self) {
        match choose_folder() {
            Ok(Some(path)) => self.add_folder(path),
            Ok(None) => {}
            Err(message) => self.folder_notice = message,
        }
    }

    fn show_backup(&mut self, ui: &mut egui::Ui) {
        let pause_label = if self.scheduler.is_paused() {
            "繼續備份"
        } else {
            "暫停備份"
        };
        let can_backup = !self.is_dirty() && self.has_folders();
        let mut toggle_pause = false;
        let mut backup_now = false;
        ui.horizontal(|ui| {
            toggle_pause = ui.small_button(pause_label).clicked();
            backup_now = ui
                .add_enabled(can_backup, egui::Button::new("現在備份").small())
                .clicked();
        });
        if toggle_pause {
            self.scheduler.set_paused(!self.scheduler.is_paused());
        }
        if backup_now {
            self.commit_now();
        }
        show_backup_notice(ui, &self.backup_notice);
    }

    fn show_editor(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.open_path.clone() else {
            if !self.editor_notice.is_empty() {
                ui.colored_label(ui.visuals().error_fg_color, &self.editor_notice);
            }
            return;
        };
        self.show_editor_header(ui, &path);
        if !self.editor_notice.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.editor_notice);
        }
        egui::ScrollArea::both().show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.editor_text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(40),
            );
        });
    }

    fn show_editor_header(&mut self, ui: &mut egui::Ui, path: &Path) {
        let conflict = self.disk_conflict;
        let dirty = self.is_dirty();
        let just_saved = self.just_saved;
        let mut reload = false;
        let mut save = false;
        ui.horizontal(|ui| {
            ui.strong(file_name(path));
            show_editor_mark(ui, conflict, dirty, just_saved);
            if conflict {
                reload = ui.button("捨棄修改並重新載入").clicked();
            } else {
                save = ui.button("儲存").clicked();
            }
        });
        if reload {
            self.reload_open();
        }
        if save {
            self.save_open();
        }
    }
}

fn show_editor_mark(ui: &mut egui::Ui, conflict: bool, dirty: bool, just_saved: bool) {
    if conflict {
        ui.colored_label(ui.visuals().error_fg_color, "檔案已被改過");
        return;
    }
    if dirty {
        ui.colored_label(ui.visuals().warn_fg_color, "未儲存");
        return;
    }
    if just_saved {
        ui.label("已儲存");
    }
}

impl eframe::App for NoteSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.is_dirty() {
            self.just_saved = false;
        }
        self.mark_dirty(self.is_dirty());
        self.sync_backup_notice();
        self.handle_save_shortcut(ctx);
        self.handle_tray(ctx);

        egui::SidePanel::left("notes")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                self.show_search(ui);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_hits(ui);
                    if self.searched {
                        ui.separator();
                    }
                    self.show_folders(ui);
                });
                ui.separator();
                self.show_backup(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_editor(ui);
        });

        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

pub fn cjk_font_bytes() -> Option<Vec<u8>> {
    CJK_FONT_CANDIDATES
        .iter()
        .find_map(|path| fs::read(path).ok())
}

fn choose_folder() -> Result<Option<PathBuf>, String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg("POSIX path of (choose folder with prompt \"選擇筆記資料夾\")")
        .output()
        .map_err(|err| err.to_string())?;
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

fn backup_line(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "no folders configured" {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(" | ").collect();
    let failed: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| part.contains(": error") || part.contains(": missing"))
        .collect();
    if !failed.is_empty() {
        return Some(failed.join(" | "));
    }
    if parts.iter().any(|part| part.contains("skipped (")) {
        return Some("這次沒有備份".into());
    }
    if parts.iter().any(|part| part.contains("update ")) {
        return Some("已備份".into());
    }
    None
}

fn manual_backup_notice(committed: bool, blocked: bool, errors: &[String]) -> String {
    if !errors.is_empty() {
        return errors.join(" | ");
    }
    if blocked {
        return "這次沒有備份".into();
    }
    if committed {
        return "已備份".into();
    }
    "沒有新內容".into()
}

fn folder_label(path: &Path, folders: &[PathBuf]) -> String {
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

fn hit_label(hit: &SearchHit, hits: &[SearchHit]) -> String {
    let name = file_name(&hit.file);
    let title = hit
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let Some(title) = title else {
        return name;
    };
    let shared = hits
        .iter()
        .filter(|other| other.title.as_deref().map(str::trim) == Some(title))
        .count()
        > 1;
    if shared {
        format!("{title} · {name}")
    } else {
        title.to_string()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn fingerprint(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn show_backup_notice(ui: &mut egui::Ui, notice: &str) {
    if notice.is_empty() {
        return;
    }
    let failure = notice != "已備份" && notice != "沒有新內容";
    if failure {
        ui.colored_label(ui.visuals().error_fg_color, notice);
    } else {
        ui.label(notice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_backup_lines_stay_hidden() {
        assert_eq!(backup_line(""), None);
        assert_eq!(backup_line("no folders configured"), None);
        assert_eq!(backup_line("/notes: no changes"), None);
    }

    #[test]
    fn blocked_backup_is_not_called_empty() {
        assert_eq!(
            backup_line("/a: no changes | /b: skipped (staged)"),
            Some("這次沒有備份".into())
        );
        assert_eq!(manual_backup_notice(false, true, &[]), "這次沒有備份");
        assert_eq!(manual_backup_notice(true, true, &[]), "這次沒有備份");
        assert_eq!(manual_backup_notice(false, false, &[]), "沒有新內容");
        assert_eq!(manual_backup_notice(true, false, &[]), "已備份");
    }

    #[test]
    fn commit_backup_line_is_short() {
        assert_eq!(
            backup_line("/notes: update 2 files (2)"),
            Some("已備份".into())
        );
    }

    #[test]
    fn failed_backup_line_keeps_the_cause() {
        let raw = "/notes: error git add failed";
        assert_eq!(backup_line(raw), Some(raw.to_string()));
        assert_eq!(backup_line("/gone: missing"), Some("/gone: missing".into()));
    }

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
    }

    #[test]
    fn cjk_fallback_font_is_on_this_mac() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let bytes = cjk_font_bytes().expect("system CJK font");
        assert!(bytes.len() > 1000);
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
