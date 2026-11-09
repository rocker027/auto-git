use crate::config::AppConfig;
use crate::git_ops::{self, CommitOutcome};
use crate::qmd_ops::{self, SearchHit};
use crate::scheduler::Scheduler;
use eframe::egui;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct NoteSyncApp {
    config: Arc<Mutex<AppConfig>>,
    scheduler: Scheduler,
    dirty_editor: Arc<AtomicBool>,
    search_query: String,
    search_hits: Vec<SearchHit>,
    search_error: String,
    open_path: Option<PathBuf>,
    editor_text: String,
    disk_fingerprint: u64,
    status: String,
    folder_input: String,
    #[allow(dead_code)]
    _tray: Option<tray_icon::TrayIcon>,
}

impl NoteSyncApp {
    pub fn new(
        config: Arc<Mutex<AppConfig>>,
        dirty_editor: Arc<AtomicBool>,
        scheduler: Scheduler,
        tray: Option<tray_icon::TrayIcon>,
    ) -> Self {
        Self {
            config,
            scheduler,
            dirty_editor,
            search_query: String::new(),
            search_hits: Vec::new(),
            search_error: String::new(),
            open_path: None,
            editor_text: String::new(),
            disk_fingerprint: 0,
            status: "ready".into(),
            folder_input: String::new(),
            _tray: tray,
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

    fn run_search(&mut self) {
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
            &self.search_query,
            limit,
            collection.as_deref(),
            Duration::from_secs(20),
        ) {
            Ok(hits) => {
                self.search_hits = hits;
                self.status = format!("found {} hits", self.search_hits.len());
            }
            Err(e) => {
                self.search_hits.clear();
                self.search_error = e.to_string();
                self.status = "search failed".into();
            }
        }
    }

    fn open_hit(&mut self, path: PathBuf) {
        let roots = self.config.lock().unwrap().folders.clone();
        // Allow open if under a watched folder OR if no folders configured yet
        // (bootstrap); when folders exist, enforce allow-list.
        if !roots.is_empty() && !qmd_ops::path_allowed(&path, &roots) {
            self.status = format!("blocked outside watched folders: {}", path.display());
            return;
        }
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.disk_fingerprint = fingerprint(&text);
                self.editor_text = text;
                self.open_path = Some(path);
                self.mark_dirty(false);
                self.status = "opened".into();
            }
            Err(e) => self.status = format!("open failed: {e}"),
        }
    }

    fn save_open(&mut self) {
        let Some(path) = self.open_path.clone() else {
            self.status = "no file open".into();
            return;
        };
        if let Ok(on_disk) = fs::read_to_string(&path) {
            if fingerprint(&on_disk) != self.disk_fingerprint && on_disk != self.editor_text {
                self.status =
                    "file changed on disk; reload or overwrite manually (save blocked)".into();
                return;
            }
        }
        match fs::write(&path, &self.editor_text) {
            Ok(()) => {
                self.disk_fingerprint = fingerprint(&self.editor_text);
                self.mark_dirty(false);
                self.status = format!("saved {}", path.display());
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    fn commit_now(&mut self) {
        if self.is_dirty() {
            self.status = "unsaved changes — save or discard before commit".into();
            return;
        }
        let (folders, git, skip) = {
            let cfg = self.config.lock().unwrap();
            (cfg.folders.clone(), cfg.git_path.clone(), cfg.skip_if_staged)
        };
        let mut msgs = Vec::new();
        for folder in folders {
            match git_ops::auto_commit(&git, &folder, skip) {
                Ok(CommitOutcome::Committed { message, .. }) => msgs.push(message),
                Ok(other) => msgs.push(format!("{other:?}")),
                Err(e) => msgs.push(e.to_string()),
            }
        }
        self.status = if msgs.is_empty() {
            "no folders".into()
        } else {
            msgs.join(" | ")
        };
    }

    fn add_folder(&mut self) {
        let raw = self.folder_input.trim().to_string();
        if raw.is_empty() {
            return;
        }
        let path = PathBuf::from(&raw);
        if !path.is_dir() {
            self.status = format!("not a directory: {raw}");
            return;
        }
        {
            let mut cfg = self.config.lock().unwrap();
            cfg.add_folder(path);
            if let Err(e) = cfg.save() {
                self.status = format!("save config failed: {e}");
                return;
            }
        }
        self.folder_input.clear();
        self.status = "folder added".into();
    }
}

impl eframe::App for NoteSyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep dirty flag in sync for scheduler.
        self.mark_dirty(self.is_dirty());

        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            let id = event.id;
            // IDs are set when building the menu in main.
            let _ = id;
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("note-sync");
                if ui
                    .button(if self.scheduler.is_paused() {
                        "Resume"
                    } else {
                        "Pause"
                    })
                    .clicked()
                {
                    self.scheduler.set_paused(!self.scheduler.is_paused());
                }
                if ui.button("Commit now").clicked() {
                    self.commit_now();
                }
                if ui.button("Save config").clicked() {
                    match self.config.lock().unwrap().save() {
                        Ok(()) => self.status = "config saved".into(),
                        Err(e) => self.status = e.to_string(),
                    }
                }
            });
            ui.label(format!("status: {}", self.status));
            let sched = self.scheduler.status_message();
            if !sched.is_empty() {
                ui.label(format!("scheduler: {sched}"));
            }
        });

        egui::SidePanel::left("left").resizable(true).show(ctx, |ui| {
            ui.heading("Folders");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.folder_input);
                if ui.button("Add").clicked() {
                    self.add_folder();
                }
            });
            ui.label("Paste absolute folder path, then Add.");
            {
                let mut cfg = self.config.lock().unwrap();
                ui.horizontal(|ui| {
                    ui.label("Check interval (secs)");
                    let mut secs = cfg.check_interval_secs as i32;
                    if ui
                        .add(egui::DragValue::new(&mut secs).range(30..=86400))
                        .changed()
                    {
                        cfg.check_interval_secs = secs as u64;
                    }
                });
                for f in &cfg.folders {
                    ui.label(f.display().to_string());
                }
            }

            ui.separator();
            ui.heading("Search (qmd)");
            ui.horizontal(|ui| {
                let resp = ui.text_edit_singleline(&mut self.search_query);
                if ui.button("Search").clicked()
                    || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    self.run_search();
                }
            });
            if !self.search_error.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.search_error);
            }
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for hit in self.search_hits.clone() {
                    let title = hit
                        .title
                        .clone()
                        .unwrap_or_else(|| hit.file.display().to_string());
                    if ui.link(title).clicked() {
                        self.open_hit(hit.file.clone());
                    }
                    ui.label(hit.file.display().to_string());
                    ui.separator();
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(path) = &self.open_path {
                    ui.label(path.display().to_string());
                    if self.is_dirty() {
                        ui.colored_label(egui::Color32::YELLOW, "unsaved");
                    }
                } else {
                    ui.label("No file open");
                }
                if ui.button("Save").clicked() {
                    self.save_open();
                }
            });
            egui::ScrollArea::both().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.editor_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(40),
                );
            });
        });

        // Periodic UI refresh for scheduler status.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

fn fingerprint(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}
