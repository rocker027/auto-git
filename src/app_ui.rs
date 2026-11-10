//! Main window: note list on the left, the open note on the right.

use crate::config::AppConfig;
use crate::editor::OpenNote;
use crate::labels::{folder_label, hit_label};
use crate::extra_search::{ExtraSearch, QmdSettings};
use crate::notes::{self, FolderNotes};
use crate::qmd_ops::SearchHit;
use crate::scheduler::Scheduler;
use crate::status::{self, BackupState, Tone};
use crate::editor_view::{
    forget_undo, show_conflict_banner, show_editor_header, show_preview, show_text_editor,
};
use crate::widgets;
use eframe::egui::{self, Id, RichText};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const CJK_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
];
const RESCAN_EVERY: Duration = Duration::from_secs(10);
const SEARCH_ID: &str = "note-search";

/// Something waiting on the "save first?" question.
enum Pending {
    Open(PathBuf),
    Close,
}

enum Choice {
    Save,
    Discard,
    Cancel,
}

pub struct NoteSyncApp {
    config: Arc<Mutex<AppConfig>>,
    scheduler: Scheduler,
    dirty_editor: Arc<AtomicBool>,
    library: Vec<FolderNotes>,
    scan: Option<Receiver<Vec<FolderNotes>>>,
    rescan_queued: bool,
    last_scan: Instant,
    query: String,
    extra: ExtraSearch,
    editor: Option<OpenNote>,
    open_error: String,
    folder_error: String,
    pending: Option<Pending>,
    may_close: bool,
    /// Folders whose notes are hidden in the list.
    collapsed: HashSet<PathBuf>,
    /// Show the open note as rendered Markdown instead of the editor.
    is_preview: bool,
    markdown_cache: egui_commonmark::CommonMarkCache,
    /// The editor text was just (re)loaded; old undo steps must not come back.
    clear_undo: bool,
    menus: SystemMenus,
}

/// Tray menu and app menu. Kept alive here so the system keeps showing them.
pub struct SystemMenus {
    pub tray: Option<tray_icon::TrayIcon>,
    pub show_item: tray_icon::menu::MenuItem,
    /// Tray "結束" and the app menu's ⌘Q item; both go through the unsaved-text check.
    pub quit_items: Vec<tray_icon::menu::MenuItem>,
    pub app_menu: Option<tray_icon::menu::Menu>,
}

impl NoteSyncApp {
    pub fn new(
        config: Arc<Mutex<AppConfig>>,
        dirty_editor: Arc<AtomicBool>,
        scheduler: Scheduler,
        menus: SystemMenus,
    ) -> Self {
        let mut app = Self {
            config,
            scheduler,
            dirty_editor,
            library: Vec::new(),
            scan: None,
            rescan_queued: false,
            last_scan: Instant::now(),
            query: String::new(),
            extra: ExtraSearch::default(),
            editor: None,
            open_error: String::new(),
            folder_error: String::new(),
            pending: None,
            may_close: false,
            clear_undo: false,
            collapsed: HashSet::new(),
            is_preview: false,
            markdown_cache: egui_commonmark::CommonMarkCache::default(),
            menus,
        };
        app.rescan();
        app
    }

    fn folders(&self) -> Vec<PathBuf> {
        self.config.lock().unwrap().folders.clone()
    }

    fn editor_dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(OpenNote::is_dirty)
    }

    // ---- notes library ------------------------------------------------------

    fn rescan(&mut self) {
        if self.scan.is_some() {
            self.rescan_queued = true;
            return;
        }
        let folders = self.folders();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(notes::scan(&folders));
        });
        self.scan = Some(receiver);
        self.last_scan = Instant::now();
    }

    fn poll_library(&mut self) {
        if let Some(receiver) = &self.scan {
            match receiver.try_recv() {
                Ok(library) => {
                    self.library = library;
                    self.scan = None;
                    self.last_scan = Instant::now();
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scan = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.scan.is_none() && (self.rescan_queued || self.last_scan.elapsed() > RESCAN_EVERY) {
            self.rescan_queued = false;
            self.rescan();
        }
    }

    fn is_in_library(&self, path: &Path) -> bool {
        self.library
            .iter()
            .any(|folder| folder.notes.iter().any(|note| note.path == path))
    }

    // ---- folders ------------------------------------------------------------

    fn pick_and_add_folder(&mut self) {
        match choose_folder() {
            Ok(Some(path)) => self.add_folder(path),
            Ok(None) => {}
            Err(message) => self.folder_error = message,
        }
    }

    fn add_folder(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.folder_error = "這不是資料夾".into();
            return;
        }
        self.update_folders(|cfg| cfg.add_folder(path));
        if !self.scheduler.is_paused() {
            self.scheduler.request_now();
        }
    }

    fn remove_folder(&mut self, path: &Path) {
        self.update_folders(|cfg| cfg.remove_folder(path));
    }

    fn update_folders(&mut self, change: impl FnOnce(&mut AppConfig)) {
        let saved = {
            let mut cfg = self.config.lock().unwrap();
            change(&mut cfg);
            cfg.save()
        };
        self.folder_error = match saved {
            Ok(()) => String::new(),
            Err(err) => format!("無法儲存設定：{err}"),
        };
        self.rescan();
    }

    // ---- editor -------------------------------------------------------------

    fn request_open(&mut self, path: PathBuf) {
        let same = self.editor.as_ref().is_some_and(|e| e.path == path);
        if same {
            return;
        }
        if self.save_before_leaving() {
            self.open(&path);
        } else {
            self.pending = Some(Pending::Open(path));
        }
    }

    /// Saves unsaved text before switching away or closing.
    /// False only when the text was not written (conflict, unreadable file, or write error).
    fn save_before_leaving(&mut self) -> bool {
        let Some(editor) = self.editor.as_mut() else {
            return true;
        };
        if !editor.is_dirty() {
            return true;
        }
        // Unattended: refuses when the file cannot be read, so the dialog asks instead.
        let saved = editor.save_if_dirty();
        if saved {
            self.rescan();
        }
        saved
    }

    fn open(&mut self, path: &Path) {
        match OpenNote::load(path) {
            Ok(note) => {
                self.editor = Some(note);
                self.open_error.clear();
                self.clear_undo = true;
            }
            Err(err) => self.open_error = format!("打不開「{}」：{err}", notes::note_title(path)),
        }
    }

    fn save(&mut self) -> bool {
        let saved = self.editor.as_mut().is_some_and(OpenNote::save);
        if saved {
            self.rescan();
        }
        saved
    }

    fn resolve_pending(&mut self, choice: Choice, ctx: &egui::Context) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let go_on = match choice {
            Choice::Cancel => false,
            // "再試一次" is the user's own request, so it keeps the attended save().
            Choice::Save => !self.editor_dirty() || self.save(),
            Choice::Discard => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.reload();
                }
                self.clear_undo = true;
                true
            }
        };
        if !go_on {
            return;
        }
        match pending {
            Pending::Open(path) => self.open(&path),
            Pending::Close => {
                self.may_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    // ---- window events ------------------------------------------------------

    fn handle_tray(&self, ctx: &egui::Context) {
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id == *self.menus.show_item.id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if self.menus.quit_items.iter().any(|item| item.id() == &event.id) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Unsaved text must never vanish with the window.
    fn guard_close(&mut self, ctx: &egui::Context) {
        let asked = ctx.input(|input| input.viewport().close_requested());
        if asked && !self.may_close && !self.save_before_leaving() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Close);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (save, find, toggle_preview) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::F),
                input.consume_key(egui::Modifiers::COMMAND, egui::Key::E),
            )
        });
        if toggle_preview && self.editor.is_some() {
            self.is_preview = !self.is_preview;
        }
        let can_save = self
            .editor
            .as_ref()
            .is_some_and(|e| e.is_dirty() && !e.has_conflict);
        if save && can_save {
            self.save();
        }
        if find {
            ctx.memory_mut(|memory| memory.request_focus(Id::new(SEARCH_ID)));
        }
    }

    // ---- sidebar ------------------------------------------------------------

    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        self.show_status_row(ui);
        ui.add_space(4.0);
        self.show_search_field(ui);
        ui.add_space(4.0);
        if ui.button("＋ 加入資料夾").clicked() {
            self.pick_and_add_folder();
        }
        if !self.folder_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.folder_error);
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.show_note_list(ui));
    }

    fn show_status_row(&mut self, ui: &mut egui::Ui) {
        let report = self.scheduler.last_report();
        let state = BackupState {
            report: report.as_ref(),
            is_running: self.scheduler.is_running(),
            is_paused: self.scheduler.is_paused(),
            editor_dirty: self.editor_dirty(),
        };
        let line = status::backup_status(&state, SystemTime::now());
        let is_problem = line.tone == Tone::Problem;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.show_main_menu(ui);
                let color = match line.tone {
                    Tone::Good => ui.visuals().text_color(),
                    Tone::Quiet => ui.visuals().weak_text_color(),
                    Tone::Problem => ui.visuals().error_fg_color,
                };
                if !is_problem {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&line.text).color(color)).truncate());
                    });
                }
            });
        });
        // Problems get their own wrapped line so the cause is never cut off.
        if is_problem {
            let label = ui.add(egui::Label::new(RichText::new(&line.text).color(ui.visuals().error_fg_color)).wrap());
            if let Some(detail) = &line.detail {
                label.on_hover_text(detail);
            }
        }
    }

    fn show_main_menu(&mut self, ui: &mut egui::Ui) {
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

    fn show_search_field(&mut self, ui: &mut egui::Ui) {
        let field = egui::TextEdit::singleline(&mut self.query)
            .id(Id::new(SEARCH_ID))
            .hint_text("🔍 搜尋")
            .margin(egui::vec2(8.0, 6.0))
            .desired_width(f32::INFINITY);
        let response = ui.add(field);
        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.query.clear();
        }
        self.extra.set_query(&self.query);
    }

    fn show_note_list(&mut self, ui: &mut egui::Ui) {
        let folders = self.folders();
        let query = self.query.trim().to_string();
        let words = notes::search_words(&query);
        let mut chosen = None;
        let mut removed = None;
        let mut toggled = None;
        let mut shown = 0;
        for group in &self.library {
            let matching: Vec<_> = group
                .notes
                .iter()
                .filter(|note| notes::matches(note, &words))
                .collect();
            if !query.is_empty() && matching.is_empty() {
                continue;
            }
            shown += matching.len();
            let is_open = is_folder_open(&self.collapsed, &group.folder, !query.is_empty());
            let header = widgets::folder_header(ui, &folder_label(&group.folder, &folders), is_open);
            if header.clicked() && query.is_empty() {
                toggled = Some(group.folder.clone());
            }
            header.context_menu(|ui| {
                if ui.button("移除這個資料夾").clicked() {
                    removed = Some(group.folder.clone());
                    ui.close_menu();
                }
            });
            if !group.exists {
                ui.colored_label(ui.visuals().error_fg_color, "  找不到這個資料夾");
            }
            if is_open {
                for note in matching {
                    if self.note_row(ui, &note.path, &note.title, note.modified) {
                        chosen = Some(note.path.clone());
                    }
                }
            }
            ui.add_space(if is_open { 10.0 } else { 2.0 });
        }
        if let Some(folder) = toggled {
            if !self.collapsed.remove(&folder) {
                self.collapsed.insert(folder);
            }
        }
        shown += self.show_extra_hits(ui, &mut chosen);
        self.show_list_footer(ui, &query, shown);
        if let Some(path) = chosen {
            self.request_open(path);
        }
        if let Some(folder) = removed {
            self.remove_folder(&folder);
        }
    }

    /// Returns true when clicked.
    fn note_row(
        &self,
        ui: &mut egui::Ui,
        path: &Path,
        title: &str,
        modified: Option<SystemTime>,
    ) -> bool {
        let open = self.editor.as_ref().filter(|e| e.path == path);
        let title = match open {
            Some(editor) if editor.is_dirty() => format!("● {title}"),
            _ => title.to_string(),
        };
        let meta = modified
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .map(status::time_ago)
            .unwrap_or_default();
        let row = widgets::note_row(ui, &title, &meta, open.is_some());
        row.clicked()
    }

    fn show_extra_hits(&self, ui: &mut egui::Ui, chosen: &mut Option<PathBuf>) -> usize {
        let hits: Vec<&SearchHit> = self
            .extra
            .hits()
            .iter()
            .filter(|hit| !self.is_in_library(&hit.file))
            .collect();
        if hits.is_empty() {
            return 0;
        }
        ui.label(RichText::new("其他地方").small().strong().weak());
        let all: Vec<SearchHit> = hits.iter().map(|hit| (*hit).clone()).collect();
        for hit in &hits {
            if self.note_row(ui, &hit.file, &hit_label(hit, &all), None) {
                *chosen = Some(hit.file.clone());
            }
        }
        hits.len()
    }

    fn show_list_footer(&self, ui: &mut egui::Ui, query: &str, shown: usize) {
        if !query.is_empty() && shown == 0 && !self.extra.is_busy() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.weak("沒有符合的筆記"));
        }
        if let Some(failure) = &self.extra.failure {
            ui.weak("其他地方的搜尋失敗").on_hover_text(failure);
        }
    }

    // ---- editor panel -------------------------------------------------------

    fn show_editor(&mut self, ui: &mut egui::Ui) {
        if !self.open_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.open_error);
        }
        let clear_undo = std::mem::take(&mut self.clear_undo);
        let Some(editor) = self.editor.as_mut() else {
            ui.centered_and_justified(|ui| ui.weak("← 選一則筆記"));
            return;
        };
        let mut reload = false;
        show_editor_header(ui, editor, &mut self.is_preview);
        if editor.has_conflict {
            reload = show_conflict_banner(ui);
        }
        let editor_id = Id::new(("note-editor", &editor.path));
        if clear_undo {
            forget_undo(ui.ctx(), editor_id);
        }
        if self.is_preview {
            editor.is_composing = false;
            show_preview(ui, &editor.text, &mut self.markdown_cache);
        } else {
            show_text_editor(ui, editor, editor_id);
        }
        if reload {
            editor.reload();
            self.clear_undo = true;
        }
    }

    fn autosave(&mut self) {

        // A backup run may be reading the folder; a half-written file would land in a commit.
        if self.scheduler.is_running() {
            return;
        }
        if self.editor.as_mut().is_some_and(|e| e.autosave(Instant::now())) {
            self.rescan();
        }
    }

    // ---- first run ----------------------------------------------------------

    fn show_welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 60.0).max(24.0));
            let button =
                widgets::primary_button(ui, "選擇筆記資料夾", 18.0).min_size(egui::vec2(240.0, 52.0));
            if ui.add(button).clicked() {
                self.pick_and_add_folder();
            }
            if !self.folder_error.is_empty() {
                ui.add_space(8.0);
                ui.colored_label(ui.visuals().error_fg_color, &self.folder_error);
            }
        });
    }

    fn show_unsaved_dialog(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        let Some(editor) = self.editor.as_ref() else {
            self.pending = None;
            return;
        };
        // A retry in the background may already have saved it.
        if !editor.is_dirty() {
            self.resolve_pending(Choice::Save, ctx);
            return;
        }
        let title = notes::note_title(&editor.path);
        let can_retry = !editor.has_conflict && !editor.is_unreadable;
        let reason = if editor.has_conflict {
            "這則筆記在別的地方被改過了。".to_string()
        } else if editor.is_unreadable {
            format!("{}\n要存回原位置，請按取消後按 ⌘S。", editor.error)
        } else {
            editor.error.clone()
        };
        let mut choice = None;
        // Only shown when saving failed; normal edits are saved on their own.
        let modal = egui::Modal::new(Id::new("unsaved")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(format!("「{title}」存不進去"));
            if !reason.is_empty() {
                ui.label(&reason);
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if can_retry && ui.add(widgets::primary_button(ui, "再試一次", 15.0)).clicked() {
                    choice = Some(Choice::Save);
                }
                if ui.button("放棄修改").clicked() {
                    choice = Some(Choice::Discard);
                }
                if ui.button("取消").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(Choice::Cancel);
        }
        if let Some(choice) = choice {
            self.resolve_pending(choice, ctx);
        }
    }

    fn refresh_editor(&mut self) {
        if self.editor.as_mut().is_some_and(OpenNote::poll_disk) {
            self.clear_undo = true;
        }
        self.dirty_editor.store(self.editor_dirty(), Ordering::Relaxed);
    }

    fn tick_extra_search(&mut self) {
        let config = self.config.clone();
        self.extra.tick(|| {
            let cfg = config.lock().unwrap();
            QmdSettings {
                qmd: cfg.qmd_path.clone(),
                limit: cfg.search_limit,
                collection: cfg.qmd_collection.clone(),
            }
        });
    }
}

impl eframe::App for NoteSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray(ctx);
        self.guard_close(ctx);
        self.handle_shortcuts(ctx);
        self.refresh_editor();
        self.poll_library();
        self.tick_extra_search();

        if self.folders().is_empty() {
            egui::CentralPanel::default().show(ctx, |ui| self.show_welcome(ui));
        } else {
            egui::SidePanel::left("notes")
                .resizable(true)
                .default_width(300.0)
                .width_range(220.0..=520.0)
                .show(ctx, |ui| self.show_sidebar(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(24.0))
                .show(ctx, |ui| self.show_editor(ui));
        }
        self.show_unsaved_dialog(ctx);
        self.autosave();
        self.dirty_editor.store(self.editor_dirty(), Ordering::Relaxed);

        let waiting = self.scan.is_some()
            || self.extra.is_busy()
            || self.scheduler.is_running()
            || self.editor.as_ref().is_some_and(|e| e.is_dirty() && !e.has_conflict);
        let next = if waiting { 200 } else { 1000 };
        ctx.request_repaint_after(Duration::from_millis(next));
    }

    /// Last chance for Dock "Quit" and logout, which skip the close request.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(editor) = self.editor.as_mut() {
            editor.save_if_dirty();
        }
    }
}

/// Searching always shows matches, even inside folders the user closed.
fn is_folder_open(collapsed: &HashSet<PathBuf>, folder: &Path, is_searching: bool) -> bool {
    is_searching || !collapsed.contains(folder)
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
    fn closed_folders_open_while_searching() {
        let notes = PathBuf::from("/n");
        let closed: HashSet<PathBuf> = [notes.clone()].into();
        assert!(!is_folder_open(&closed, &notes, false));
        assert!(is_folder_open(&closed, &notes, true));
        assert!(is_folder_open(&HashSet::new(), &notes, false));
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
