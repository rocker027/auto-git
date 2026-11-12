//! Rows of the note list: folder headers, note rows and their right-click menu.

use crate::theme::{self, CONTROL_RADIUS, FOCUS_RING, ROW_RADIUS, TOUCH};
use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Color32, FontId, Response, Sense, Stroke, StrokeKind,
    TextStyle, Ui, WidgetInfo, WidgetType,
};

const ROW_HEIGHT: f32 = 52.0;
const ROW_PADDING: f32 = 10.0;

/// Shared by recent notes, folder rows and search results; right-click never opens the note.
pub fn note_context_menu(row: &Response, path: &std::path::Path) {
    row.context_menu(|ui| {
        theme::compact_menu(ui);
        let error_id = egui::Id::new(("note-finder-error", path));
        if ui.button("複製筆記路徑").clicked() {
            ui.ctx().copy_text(path.to_string_lossy().into_owned());
            ui.close_menu();
        }
        if ui.button("在 Finder 中顯示").clicked() {
            match crate::platform::reveal_in_finder(path) {
                Ok(()) => {
                    ui.data_mut(|memory| memory.remove::<String>(error_id));
                    ui.close_menu();
                }
                Err(error) => ui.data_mut(|memory| memory.insert_temp(error_id, error)),
            }
        }
        if let Some(error) = ui.data(|memory| memory.get_temp::<String>(error_id)) {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    });
}

/// A virtual group using the same header and note actions as watched folders.
pub fn recent_notes(
    ui: &mut Ui,
    library: &[crate::notes::FolderNotes],
    is_open: &mut bool,
    mut row: impl FnMut(&mut Ui, &crate::notes::Note) -> bool,
) -> Option<std::path::PathBuf> {
    let mut chosen = None;
    ui.push_id("recent-notes", |ui| {
        if folder_header(ui, "最近更新", *is_open).clicked() {
            *is_open = !*is_open;
        }
        if *is_open {
            let notes = crate::notes::recent_notes(library);
            if notes.is_empty() {
                secondary_label(ui, "尚無筆記");
            }
            for note in notes {
                if row(ui, note) {
                    chosen = Some(note.path.clone());
                }
            }
        }
    });
    chosen
}

/// Secondary information stays readable against the opaque sidebar and document surfaces.
pub fn secondary_color(ui: &Ui) -> Color32 {
    theme::of(ui).secondary
}

/// The one action the current screen is about, drawn in the accent color.
pub fn primary_button<'a>(text: &str, size: f32) -> egui::Button<'a> {
    let label = egui::RichText::new(text).size(size).color(theme::ON_ACCENT);
    egui::Button::new(label).fill(theme::ACCENT)
}

/// Adds a `primary_button`. Its focus ring is drawn inside, in the label colour:
/// the app-wide accent ring would vanish against the accent fill.
pub fn add_primary_button(ui: &mut Ui, is_enabled: bool, button: egui::Button<'_>) -> Response {
    let response = ui.add_enabled(is_enabled, button);
    if response.has_focus() {
        let ring = Stroke::new(FOCUS_RING, theme::ON_ACCENT);
        let inset = 3.0;
        ui.painter().rect_stroke(
            response.rect.shrink(inset),
            f32::from(CONTROL_RADIUS) - inset,
            ring,
            StrokeKind::Inside,
        );
    }
    response
}

/// Secondary text that stays readable; egui's own "weak" text is too faint on these surfaces.
pub fn secondary_label(ui: &mut Ui, text: &str) -> Response {
    ui.label(egui::RichText::new(text).color(secondary_color(ui)))
}

/// A secondary action as accent text. It keeps the button frame, so hover and
/// keyboard focus still draw the ring.
pub fn quiet_button<'a>(ui: &Ui, text: &str) -> egui::Button<'a> {
    let label = egui::RichText::new(text).color(theme::of(ui).accent_text);
    egui::Button::new(label).fill(Color32::TRANSPARENT)
}

/// Folder title in the note list. Click to show or hide its notes.
pub fn folder_header(ui: &mut Ui, label: &str, is_open: bool) -> Response {
    let size = vec2(ui.available_width(), TOUCH);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::CollapsingHeader, ui.is_enabled(), is_open, label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    if response.hovered() {
        ui.painter().rect_filled(rect, ROW_RADIUS, theme::of(ui).hover);
    }
    paint_focus_ring(ui, &response);
    let color = secondary_color(ui);
    paint_chevron(ui, pos2(rect.left() + 14.0, rect.center().y), is_open, color);
    let font = FontId::proportional(13.0);
    let galley = ui.fonts(|fonts| fonts.layout_job(single_line(label, font, color, rect.width() - 38.0)));
    let pos = pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
    response
}

/// Thin chevron: pointing down when open, right when closed.
pub fn paint_chevron(ui: &Ui, center: egui::Pos2, is_open: bool, color: Color32) {
    let r = 4.0;
    let points = if is_open {
        vec![center + vec2(-r, -r * 0.5), center + vec2(0.0, r * 0.5), center + vec2(r, -r * 0.5)]
    } else {
        vec![center + vec2(-r * 0.5, -r), center + vec2(r * 0.5, 0.0), center + vec2(-r * 0.5, r)]
    };
    ui.painter().add(egui::Shape::line(points, Stroke::new(1.6_f32, color)));
}

/// Keyboard focus on a custom-painted row or tile.
pub fn paint_focus_ring(ui: &Ui, response: &Response) {
    if response.has_focus() {
        let stroke = Stroke::new(FOCUS_RING, theme::of(ui).accent_text);
        ui.painter()
            .rect_stroke(response.rect.shrink(1.0), ROW_RADIUS, stroke, StrokeKind::Inside);
    }
}

/// Two-line note row keeps titles readable in a narrow sidebar.
pub fn note_row(ui: &mut Ui, title: &str, meta: &str, selected: bool) -> Response {
    let size = vec2(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, ui.is_enabled(), selected, title)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    paint_row_background(ui, &response, selected);
    let colors = theme::of(ui);
    let text_color = if selected { theme::ON_ACCENT } else { colors.text };
    let meta_color = if selected { theme::ON_ACCENT } else { colors.secondary };
    let painter = ui.painter();
    let meta_galley =
        painter.layout_no_wrap(meta.to_owned(), TextStyle::Small.resolve(ui.style()), meta_color);
    let title_width = rect.width() - ROW_PADDING * 2.0;
    let title_galley = ui.fonts(|fonts| {
        fonts.layout_job(single_line(title, TextStyle::Body.resolve(ui.style()), text_color, title_width))
    });
    let title_pos = pos2(rect.left() + ROW_PADDING, rect.top() + 8.0);
    let meta_pos = pos2(
        rect.left() + ROW_PADDING,
        rect.bottom() - meta_galley.size().y - 8.0,
    );
    painter.galley(title_pos, title_galley, text_color);
    painter.galley(meta_pos, meta_galley, meta_color);
    response
}

fn paint_row_background(ui: &Ui, response: &Response, selected: bool) {
    let fill = if selected {
        Some(theme::ACCENT)
    } else if response.hovered() {
        Some(theme::of(ui).hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(response.rect, ROW_RADIUS, fill);
    }
    if !response.has_focus() {
        return;
    }
    // On the accent fill of the chosen row the ring is the text colour, so it still shows.
    let color = if selected { theme::ON_ACCENT } else { theme::of(ui).accent_text };
    let ring = Stroke::new(FOCUS_RING, color);
    ui.painter()
        .rect_stroke(response.rect.shrink(1.0), ROW_RADIUS, ring, StrokeKind::Inside);
}

fn single_line(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(24.0));
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_header_collapses_and_reopens_without_changing_note_actions() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("test.md");
        std::fs::write(&note, "test").unwrap();
        let library = crate::notes::scan(&[dir.path().to_path_buf()]);
        let ctx = egui::Context::default();
        let mut open = true;
        let mut header = egui::Pos2::ZERO;
        let mut frame = |events: Vec<egui::Event>| {
            let mut rows = 0;
            let mut chosen = None;
            let _ = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(300.0, 300.0))),
                events, ..Default::default()
            }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    header = ui.cursor().min + vec2(20.0, 13.0);
                    chosen = recent_notes(ui, &library, &mut open, |_, _| {
                        rows += 1;
                        true
                    });
                });
            });
            (rows, chosen, open, header)
        };
        let (rows, chosen, _, header) = frame(vec![]);
        assert_eq!(rows, 1);
        assert_eq!(chosen, Some(note.clone()));
        let click = || vec![
            egui::Event::PointerMoved(header),
            egui::Event::PointerButton { pos: header, button: egui::PointerButton::Primary,
                pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos: header, button: egui::PointerButton::Primary,
                pressed: false, modifiers: egui::Modifiers::NONE },
        ];
        let (rows, chosen, open, _) = frame(click());
        assert_eq!((rows, chosen, open), (0, None, false));
        let (rows, chosen, open, _) = frame(click());
        assert_eq!((rows, chosen, open), (1, Some(note), true));
    }
}
