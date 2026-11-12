//! Main window: note list on the left, the open note on the right.

use crate::config::AppConfig;
use crate::editor::OpenNote;
use crate::extra_search::{ExtraSearch, QmdSettings};
use crate::notes::{self, FolderNotes};
use crate::scheduler::Scheduler;
use crate::tab_bar::{tab_after_keys, Tab, TabKeys};
use crate::theme::{self, CARD_RADIUS, TOUCH};
use crate::editor_view::{
    forget_undo, resolve_note_link, show_conflict_banner, show_editor_header, show_preview,
    show_text_editor, toggled_mode, ViewMode,
};
use crate::history_view::HistoryPanel;
use crate::platform::choose_folder;
use crate::widgets;
use crate::window_life::{self, CloseAction};
use eframe::egui::{self, Id};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

mod backup_tab;
mod sidebar;

const RESCAN_EVERY: Duration = Duration::from_secs(10);
const SEARCH_ID: &str = "note-search";
/// One gutter on every side; the right side shares it with the document's left margin.
const SIDEBAR_MARGIN: egui::Margin = egui::Margin { left: 12, right: 8, top: 12, bottom: 12 };
const DIALOG_TITLE_SIZE: f32 = 20.0;

/// Something waiting on the "save first?" question.
enum Pending {
    Open(PathBuf),
    /// Quit the app after the question.
    Close,
    /// Hide the window into the menu bar after the question.
    Hide,
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
    /// Folders explicitly opened by the user; all start collapsed.
    expanded: HashSet<PathBuf>,
    recent_open: bool,
    /// Which tab the sidebar shows.
    tab: Tab,
    /// Editor, rendered Markdown, or backed-up versions of the open note.
    mode: ViewMode,
    /// The editor was shown last frame; when it comes back it gets the focus back.
    was_editing: bool,
    history: HistoryPanel,
    /// Last time a note was written, so the status line can say a backup is still due.
    last_saved_at: Option<SystemTime>,
    markdown_cache: egui_commonmark::CommonMarkCache,
    /// The editor text was just (re)loaded; old undo steps must not come back.
    clear_undo: bool,
    menus: SystemMenus,
}

/// Tray menu and app menu. Kept alive here so the system keeps showing them.
pub struct SystemMenus {
    pub tray: Option<tray_icon::TrayIcon>,
    pub show_item: tray_icon::menu::MenuItem,
    pub backup_status: crate::tray_status::TrayStatus,
    /// Tray "結束" and the app menu's ⌘Q item; both go through the unsaved-text check.
    pub quit_items: Vec<tray_icon::menu::MenuItem>,
    pub app_menu: Option<tray_icon::menu::Menu>,
    /// Set by the menu handler when Quit was chosen, so the close request ends the app.
    pub quitting: Arc<AtomicBool>,
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
            expanded: HashSet::new(),
            recent_open: true,
            tab: Tab::Notes,
            mode: ViewMode::Edit,
            was_editing: true,
            history: HistoryPanel::default(),
            last_saved_at: None,
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
        let previous = self.library.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(notes::scan_reusing(&folders, &previous));
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
        self.expanded.remove(path);
        self.update_folders(|cfg| cfg.remove_folder(path));
    }

    fn update_folders(&mut self, change: impl FnOnce(&mut AppConfig)) {
        self.update_config(change);
        self.rescan();
    }

    fn update_config(&mut self, change: impl FnOnce(&mut AppConfig)) {
        let saved = {
            let mut cfg = self.config.lock().unwrap();
            change(&mut cfg);
            cfg.save()
        };
        self.folder_error = match saved {
            Ok(()) => String::new(),
            Err(err) => format!("無法儲存設定：{err}"),
        };
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
            self.after_save();
        }
        saved
    }

    fn after_save(&mut self) {
        self.last_saved_at = Some(SystemTime::now());
        self.rescan();
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
            self.after_save();
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
            Pending::Hide => window_life::hide_app(ctx),
        }
    }

    // ---- window events ------------------------------------------------------

    /// The close button hides the window; Quit ends the app.
    /// Either way unsaved text is saved first, or the user is asked.
    fn guard_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) {
            return;
        }
        let is_quitting = self.menus.quitting.swap(false, Ordering::SeqCst);
        let may_close = self.may_close;
        let action = window_life::on_close_request(is_quitting, may_close, || self.save_before_leaving());
        if action != CloseAction::Allow {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        match action {
            CloseAction::Allow => {}
            CloseAction::Hide => window_life::hide_app(ctx),
            // A quit question already on screen stays a quit.
            CloseAction::AskThenHide if matches!(self.pending, Some(Pending::Close)) => {}
            CloseAction::AskThenHide => self.pending = Some(Pending::Hide),
            CloseAction::AskThenQuit => {
                window_life::show_app(ctx);
                self.pending = Some(Pending::Close);
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let command = egui::Modifiers::COMMAND;
        let (save, toggle_preview, tab_keys) = ctx.input_mut(|input| {
            (
                input.consume_key(command, egui::Key::S),
                input.consume_key(command, egui::Key::E),
                TabKeys {
                    find: input.consume_key(command, egui::Key::F),
                    notes: input.consume_key(command, egui::Key::Num1),
                    backup: input.consume_key(command, egui::Key::Num2),
                },
            )
        });
        // Before the focus request below: the search field is only drawn in the notes tab.
        self.tab = tab_after_keys(self.tab, tab_keys);
        if toggle_preview && self.editor.is_some() {
            self.mode = toggled_mode(self.mode);
        }
        let can_save = self
            .editor
            .as_ref()
            .is_some_and(|e| e.is_dirty() && !e.has_conflict);
        if save && can_save {
            self.save();
        }
        if tab_keys.find {
            ctx.memory_mut(|memory| memory.request_focus(Id::new(SEARCH_ID)));
        }
    }

    // ---- editor panel -------------------------------------------------------

    fn show_editor(&mut self, ui: &mut egui::Ui) {
        if !self.open_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.open_error);
        }
        let clear_undo = std::mem::take(&mut self.clear_undo);
        let Some(editor) = self.editor.as_mut() else {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.35).max(24.0));
                ui.heading("選擇一則筆記");
                ui.add_space(8.0);
                widgets::secondary_label(ui, "從左側列表選取，或按 ⌘F 搜尋。");
            });
            return;
        };
        let mut reload = false;
        show_editor_header(ui, editor, &mut self.mode);
        if editor.has_conflict {
            reload = show_conflict_banner(ui);
        }
        let editor_id = Id::new(("note-editor", &editor.path));
        if clear_undo {
            forget_undo(ui.ctx(), editor_id);
        }
        let mut linked = None;
        let mut restored = None;
        if self.mode != ViewMode::History {
            self.history.forget();
        }
        match self.mode {
            ViewMode::Preview => {
                editor.is_composing = false;
                let links = show_preview(ui, &editor.text, &mut self.markdown_cache);
                linked = links.iter().find_map(|url| resolve_note_link(&editor.path, url));
            }
            ViewMode::History => {
                editor.is_composing = false;
                let git = self.config.lock().unwrap().git_path.clone();
                let can_write = !self.scheduler.is_running();
                restored = self.history.show(ui, &editor.path, &git, can_write);
            }
            ViewMode::Edit => {
                show_text_editor(ui, editor, editor_id);
                // After the TextEdit: a click on 「編輯」 this frame would otherwise drop the focus again.
                if !self.was_editing {
                    ui.ctx().memory_mut(|memory| memory.request_focus(editor_id));
                }
            }
        }
        self.was_editing = self.mode == ViewMode::Edit;
        if reload {
            editor.reload();
            self.clear_undo = true;
        }
        if let Some(path) = restored {
            // The copy of an old version is a new unsaved-to-git file; open it to edit.
            self.mode = ViewMode::Edit;
            self.after_save();
            self.request_open(path);
        } else if let Some(path) = linked {
            self.request_open(path);
        }
    }

    fn autosave(&mut self) {
        // A backup run may be reading the folder; a half-written file would land in a commit.
        if self.scheduler.is_running() {
            return;
        }
        if self.editor.as_mut().is_some_and(|e| e.autosave(Instant::now())) {
            self.after_save();
        }
    }

    // ---- first run ----------------------------------------------------------

    fn show_welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 60.0).max(24.0));
            ui.heading("你的筆記，集中在這裡");
            ui.add_space(8.0);
            widgets::secondary_label(ui, "加入 Markdown 資料夾，即可編輯、搜尋與自動備份。");
            ui.add_space(20.0);
            let button =
                widgets::primary_button("選擇筆記資料夾…", 15.0).min_size(egui::vec2(200.0, TOUCH));
            if widgets::add_primary_button(ui, true, button).clicked() {
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
        let frame = egui::Frame::popup(&ctx.style())
            .inner_margin(20.0)
            .corner_radius(CARD_RADIUS);
        let modal = egui::Modal::new(Id::new("unsaved")).frame(frame).show(ctx, |ui| {
            ui.set_width(360.0);
            let heading = egui::RichText::new(format!("「{title}」存不進去")).size(DIALOG_TITLE_SIZE).strong();
            ui.add(egui::Label::new(heading).wrap());
            if !reason.is_empty() {
                ui.label(&reason);
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let retry = widgets::primary_button("再試一次", 15.0);
                if can_retry && widgets::add_primary_button(ui, true, retry).clicked() {
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

impl NoteSyncApp {
    /// Sidebar and document sit on one background; the document is a raised card.
    fn show_panels(&mut self, ctx: &egui::Context) {
        let background = theme::palette(ctx.style().visuals.dark_mode).background;
        egui::SidePanel::left("notes")
            .resizable(true)
            .default_width(280.0)
            // Reserve room for the document; its controls wrap independently of the title.
            .width_range(220.0..=(ctx.screen_rect().width() - 360.0).clamp(220.0, 520.0))
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(background).inner_margin(SIDEBAR_MARGIN))
            .show(ctx, |ui| self.show_sidebar(ui));
        // Left is narrower: with the sidebar's right margin it adds up to one gutter.
        let around_document = egui::Margin { left: 4, right: 12, top: 12, bottom: 12 };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(background).inner_margin(around_document))
            .show(ctx, |ui| {
                theme::card(ui).inner_margin(24.0).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.show_editor(ui);
                });
            });
    }
}

impl eframe::App for NoteSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.menus.backup_status.refresh(self.scheduler.last_report().as_ref());
        self.guard_close(ctx);
        self.handle_shortcuts(ctx);
        self.refresh_editor();
        self.poll_library();
        self.tick_extra_search();

        if self.folders().is_empty() {
            egui::CentralPanel::default().show(ctx, |ui| self.show_welcome(ui));
        } else {
            self.show_panels(ctx);
        }
        self.show_unsaved_dialog(ctx);
        self.autosave();
        self.dirty_editor.store(self.editor_dirty(), Ordering::Relaxed);

        let waiting = self.scan.is_some()
            || self.extra.is_busy()
            || self.scheduler.is_running()
            || self.scheduler.is_pushing()
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
fn is_folder_open(expanded: &HashSet<PathBuf>, folder: &Path, is_searching: bool) -> bool {
    is_searching || expanded.contains(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_folders_open_while_searching() {
        let notes = PathBuf::from("/n");
        let expanded: HashSet<PathBuf> = [notes.clone()].into();
        assert!(is_folder_open(&expanded, &notes, false));
        assert!(!is_folder_open(&HashSet::new(), &notes, false));
        assert!(is_folder_open(&HashSet::new(), &notes, true));
    }

}
