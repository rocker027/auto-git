//! The sidebar: the tab bar at its bottom, and the notes tab with search,
//! the backup state and the note list.

use super::{is_folder_open, NoteSyncApp, SEARCH_ID};
use crate::controls;
use crate::labels::{folder_label, hit_label};
use crate::notes::{self, FolderNotes};
use crate::qmd_ops::SearchHit;
use crate::status::{self, BackupState, StatusLine, Tone};
use crate::tab_bar::{self, Tab};
use crate::theme::{self, CONTROL_RADIUS};
use crate::widgets;
use eframe::egui::{self, Id, Margin, RichText};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Room around a list card for its shadow, and the gap to the next card.
const LIST_CARD_MARGIN: Margin = Margin { left: 2, right: 2, top: 2, bottom: 10 };
/// Above the tab bar, and beside and below it for its shadow.
const TAB_BAR_GAP: f32 = 8.0;
const TAB_BAR_SHADOW_ROOM: f32 = 2.0;
const CAPTION_ROW_HEIGHT: f32 = 30.0;
/// The left padding holds the magnifier.
const SEARCH_MARGIN: Margin = Margin { left: 34, right: 12, top: 12, bottom: 12 };

/// What the note list is narrowed to this frame.
struct ListFilter {
    folders: Vec<PathBuf>,
    words: Vec<String>,
    is_searching: bool,
}

/// What the user did in the note list this frame; applied after the list is drawn.
#[derive(Default)]
struct ListActions {
    chosen: Option<PathBuf>,
    removed: Option<PathBuf>,
    toggled: Option<PathBuf>,
    add_folder: bool,
    shown: usize,
}

impl NoteSyncApp {
    pub(super) fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        let line = self.status_line();
        let (content, bar) = split_for_tab_bar(ui.available_rect_before_wrap());
        // Content before the bar: the Tab key then walks the sidebar from top to bottom.
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content), |ui| {
            // A very long error above the list must not be drawn over the tab bar.
            ui.set_clip_rect(content.intersect(ui.clip_rect()));
            match self.tab {
                Tab::Notes => self.show_notes_tab(ui, &line),
                Tab::Backup => self.show_backup_tab(ui, &line),
            }
        });
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| {
            if let Some(tab) = tab_bar::show(ui, self.tab) {
                self.tab = tab;
            }
        });
    }

    fn status_line(&self) -> StatusLine {
        let report = self.scheduler.last_report();
        let state = BackupState {
            report: report.as_ref(),
            is_running: self.scheduler.is_running(),
            is_pushing: self.scheduler.is_pushing(),
            is_paused: self.scheduler.is_paused(),
            editor_dirty: self.editor_dirty(),
            last_saved_at: self.last_saved_at,
        };
        status::backup_status(&state, SystemTime::now())
    }

    fn show_notes_tab(&mut self, ui: &mut egui::Ui, line: &StatusLine) {
        self.show_search_field(ui);
        if show_status_link(ui, line) {
            self.tab = Tab::Backup;
        }
        self.show_folder_error(ui);
        egui::ScrollArea::vertical()
            // Its own id: the backup tab must not share a scroll position with this list.
            .id_salt("notes-list")
            .auto_shrink([false, false])
            .show(ui, |ui| self.show_note_list(ui));
    }

    pub(super) fn show_folder_error(&self, ui: &mut egui::Ui) {
        if !self.folder_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.folder_error);
        }
    }

    fn show_search_field(&mut self, ui: &mut egui::Ui) {
        let response = show_search_box(ui, &mut self.query).on_hover_text("搜尋名稱與內容 · ⌘F");
        // egui drops focus on Esc in the same frame, so lost_focus counts too.
        let was_focused = response.has_focus() || response.lost_focus();
        if was_focused && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.query.clear();
        }
        self.extra.set_query(&self.query);
    }

    fn show_note_list(&mut self, ui: &mut egui::Ui) {
        let query = self.query.trim().to_string();
        let filter = ListFilter {
            folders: self.folders(),
            words: notes::search_words(&query),
            is_searching: !query.is_empty(),
        };
        let mut actions = ListActions::default();
        if !filter.is_searching {
            self.show_recent_card(ui, &mut actions);
        }
        for group in &self.library {
            self.show_folder_card(ui, group, &filter, &mut actions);
        }
        actions.shown += self.show_extra_hits(ui, &mut actions.chosen);
        self.show_list_footer(ui, &filter, &mut actions);
        self.apply_list_actions(actions);
    }

    fn show_recent_card(&mut self, ui: &mut egui::Ui, actions: &mut ListActions) {
        let mut is_open = self.recent_open;
        actions.chosen = list_card(ui, |ui| {
            widgets::recent_notes(ui, &self.library, &mut is_open, |ui, note| {
                self.note_row(ui, &note.path, &note.title, note.updated_at())
            })
        });
        self.recent_open = is_open;
    }

    fn show_folder_card(
        &self,
        ui: &mut egui::Ui,
        group: &FolderNotes,
        filter: &ListFilter,
        actions: &mut ListActions,
    ) {
        let matching: Vec<_> = group
            .notes
            .iter()
            .filter(|note| notes::matches(note, &filter.words))
            .collect();
        if filter.is_searching && matching.is_empty() {
            return;
        }
        actions.shown += matching.len();
        let is_open = is_folder_open(&self.expanded, &group.folder, filter.is_searching);
        list_card(ui, |ui| {
            show_folder_header(ui, group, filter, is_open, actions);
            if !is_open {
                return;
            }
            for note in matching {
                if self.note_row(ui, &note.path, &note.title, note.modified) {
                    actions.chosen = Some(note.path.clone());
                }
            }
        });
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
        widgets::note_context_menu(&row, path);
        row.on_hover_text(path.display().to_string()).clicked()
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
        let all: Vec<SearchHit> = hits.iter().map(|hit| (*hit).clone()).collect();
        list_card(ui, |ui| {
            section_label(ui, "其他地方");
            for hit in &hits {
                if self.note_row(ui, &hit.file, &hit_label(hit, &all), None) {
                    *chosen = Some(hit.file.clone());
                }
            }
        });
        hits.len()
    }

    fn show_list_footer(&self, ui: &mut egui::Ui, filter: &ListFilter, actions: &mut ListActions) {
        if filter.is_searching && actions.shown == 0 && !self.extra.is_busy() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("沒有符合的筆記").color(widgets::secondary_color(ui))));
        }
        if let Some(failure) = &self.extra.failure {
            ui.label(RichText::new("其他地方的搜尋失敗").color(widgets::secondary_color(ui)))
                .on_hover_text(failure);
        }
        if !filter.is_searching {
            actions.add_folder = ui.add(widgets::quiet_button(ui, "＋ 加入資料夾")).clicked();
        }
    }

    fn apply_list_actions(&mut self, actions: ListActions) {
        if let Some(folder) = actions.toggled {
            if !self.expanded.remove(&folder) {
                self.expanded.insert(folder);
            }
        }
        if let Some(path) = actions.chosen {
            self.request_open(path);
        }
        if let Some(folder) = actions.removed {
            self.remove_folder(&folder);
        }
        if actions.add_folder {
            self.pick_and_add_folder();
        }
    }
}

/// Folder title with its right-click menu, and a warning when the folder is gone.
fn show_folder_header(
    ui: &mut egui::Ui,
    group: &FolderNotes,
    filter: &ListFilter,
    is_open: bool,
    actions: &mut ListActions,
) {
    let header = widgets::folder_header(ui, &folder_label(&group.folder, &filter.folders), is_open);
    if header.clicked() && !filter.is_searching {
        actions.toggled = Some(group.folder.clone());
    }
    header.context_menu(|ui| {
        theme::compact_menu(ui);
        if ui.button("移除這個資料夾").clicked() {
            actions.removed = Some(group.folder.clone());
            ui.close_menu();
        }
    });
    if !group.exists {
        let error = ui.visuals().error_fg_color;
        caption_row(ui, |ui| ui.colored_label(error, "找不到這個資料夾"));
    }
}

/// The search field with its magnifier and focus ring. Returns the field's response.
fn show_search_box(ui: &mut egui::Ui, query: &mut String) -> egui::Response {
    let hint = RichText::new("搜尋筆記").color(widgets::secondary_color(ui));
    let field = egui::TextEdit::singleline(query)
        .id(Id::new(SEARCH_ID))
        .hint_text(hint)
        .margin(SEARCH_MARGIN)
        // The row height depends on the font, so the touch height is set here, not by the margin.
        .min_size(egui::vec2(0.0, theme::TOUCH))
        .vertical_align(egui::Align::Center)
        .desired_width(f32::INFINITY);
    let response = ui.add(field);
    // egui reports the text area only; the painted field is larger by its margin.
    let field_rect = response.rect + SEARCH_MARGIN;
    controls::paint_search_icon(ui, search_icon_center(field_rect));
    if response.has_focus() {
        let ring = egui::Stroke::new(theme::FOCUS_RING, theme::of(ui).accent_text);
        ui.painter()
            .rect_stroke(field_rect, CONTROL_RADIUS, ring, egui::StrokeKind::Inside);
    }
    response
}

/// In the middle of the field's left padding, clear of the text.
fn search_icon_center(field_rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(field_rect.left() + f32::from(SEARCH_MARGIN.left) / 2.0, field_rect.center().y)
}

/// Splits the sidebar into the tab content and, at its bottom, the tab bar.
fn split_for_tab_bar(sidebar: egui::Rect) -> (egui::Rect, egui::Rect) {
    let bar_top = (sidebar.bottom() - TAB_BAR_SHADOW_ROOM - tab_bar::HEIGHT).max(sidebar.top());
    let mut content = sidebar;
    content.max.y = (bar_top - TAB_BAR_GAP).max(sidebar.top());
    let bar_max = sidebar.max - egui::vec2(TAB_BAR_SHADOW_ROOM, TAB_BAR_SHADOW_ROOM);
    let bar = egui::Rect::from_min_max(egui::pos2(sidebar.left() + TAB_BAR_SHADOW_ROOM, bar_top), bar_max);
    (content, bar)
}

/// The backup state in one row; a click opens the backup tab. Returns true when clicked.
/// A problem is wrapped instead of cut off, so its cause is always readable.
fn show_status_link(ui: &mut egui::Ui, line: &StatusLine) -> bool {
    let hint = line.detail.as_deref().unwrap_or("查看備份");
    let row = if line.tone == Tone::Problem {
        controls::problem_banner(ui, &line.text)
    } else {
        controls::link_row(ui, &line.text, widgets::secondary_color(ui))
    };
    row.on_hover_text(hint).clicked()
}

/// One group of the note list: a card with tightly stacked rows.
fn list_card<R>(ui: &mut egui::Ui, add_rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    theme::card(ui)
        .inner_margin(6.0)
        .outer_margin(LIST_CARD_MARGIN)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            add_rows(ui)
        })
        .inner
}

/// Small heading of a card that has no folder header of its own.
pub(super) fn section_label(ui: &mut egui::Ui, text: &str) {
    let color = widgets::secondary_color(ui);
    caption_row(ui, |ui| ui.label(RichText::new(text).small().color(color)));
}

/// A text-only row inside a card, indented like the rows around it.
/// Lower than a clickable row: nothing here takes a click.
fn caption_row(ui: &mut egui::Ui, add_text: impl FnOnce(&mut egui::Ui) -> egui::Response) {
    let size = egui::vec2(ui.available_width(), CAPTION_ROW_HEIGHT);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add_space(10.0);
        add_text(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sidebar's narrowest inner width.
    const NARROW: f32 = 196.0;

    fn status_link_frame(ctx: &egui::Context, line: &StatusLine, events: Vec<egui::Event>) -> (bool, f32) {
        let mut clicked = false;
        let mut height = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(NARROW, 420.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ctx, |ui| {
                    let top = ui.cursor().min.y;
                    clicked = show_status_link(ui, line);
                    // The panel itself fills the screen, so measure what the row took.
                    height = ui.cursor().min.y - top - ui.spacing().item_spacing.y;
                });
        });
        (clicked, height)
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    #[test]
    fn the_search_icon_sits_in_the_padding_and_never_over_the_text() {
        let ctx = egui::Context::default();
        crate::theme::apply_style(&ctx);
        let mut text_area = egui::Rect::NOTHING;
        let mut panel_left = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(NARROW, 420.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ctx, |ui| {
                    panel_left = ui.cursor().min.x;
                    text_area = show_search_box(ui, &mut String::new()).rect;
                });
        });
        let field = text_area + SEARCH_MARGIN;
        let icon = search_icon_center(field);
        // The magnifier reaches 6 points right of its centre and 6.5 left of it.
        assert!(icon.x + 6.0 < text_area.left(), "icon {icon:?} vs text {text_area:?}");
        assert!(icon.x - 6.5 > field.left());
        assert!((field.left() - panel_left).abs() < 0.5, "the field starts at the panel edge");
        assert!(field.height() >= theme::TOUCH, "{}", field.height());
    }

    #[test]
    fn the_tab_bar_keeps_its_height_and_the_content_gets_the_rest_at_the_smallest_window() {
        // Sidebar inner area at 640×420: 200 wide, 396 high.
        let sidebar = egui::Rect::from_min_size(egui::pos2(12.0, 12.0), egui::vec2(200.0, 396.0));
        let (content, bar) = split_for_tab_bar(sidebar);
        assert_eq!(bar.height(), tab_bar::HEIGHT);
        assert!(content.bottom() < bar.top(), "no overlap");
        assert!(sidebar.contains_rect(content) && sidebar.contains_rect(bar));
        // Search field, status row and at least three note rows still fit above the bar.
        assert!(content.height() >= theme::TOUCH * 2.0 + 52.0 * 3.0, "{}", content.height());
    }

    fn line(tone: Tone, text: &str) -> StatusLine {
        StatusLine { tone, text: text.into(), detail: None }
    }

    #[test]
    fn the_status_row_is_touch_sized_and_reports_a_click() {
        let ctx = egui::Context::default();
        let good = line(Tone::Good, "backed up 3 minutes ago");
        let (clicked, height) = status_link_frame(&ctx, &good, vec![]);
        assert!(!clicked);
        assert!(height >= theme::TOUCH, "{height}");
        let (clicked, _) = status_link_frame(&ctx, &good, click(egui::pos2(NARROW / 2.0, 20.0)));
        assert!(clicked);
    }

    #[test]
    fn a_long_problem_wraps_instead_of_being_cut_off_and_still_takes_the_click() {
        let ctx = egui::Context::default();
        let short = line(Tone::Problem, "push refused");
        let long = line(Tone::Problem, &"push was refused by the remote ".repeat(6));
        let (_, short_height) = status_link_frame(&ctx, &short, vec![]);
        let (_, long_height) = status_link_frame(&ctx, &long, vec![]);
        assert!(short_height >= theme::TOUCH, "{short_height}");
        assert!(long_height > short_height * 2.0, "wrapped onto more lines: {long_height}");
        // On the text itself: the label must not swallow the click.
        let (clicked, _) = status_link_frame(&ctx, &long, click(egui::pos2(40.0, 24.0)));
        assert!(clicked);
    }
}
