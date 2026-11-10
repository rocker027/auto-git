//! Small custom widgets and the app-wide look.

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Color32, FontId, Response, Sense, StrokeKind, TextStyle,
    Ui, WidgetInfo, WidgetType,
};

const ROW_HEIGHT: f32 = 32.0;
const ROW_PADDING: f32 = 10.0;
const CORNER: f32 = 6.0;

/// Larger text and roomier controls than the egui defaults, in light and dark mode.
pub fn apply_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(22.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Small, FontId::proportional(12.5)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
        ]
        .into();
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.interact_size.y = 30.0;
    });
}

/// The one action the current screen is about, drawn in the accent color.
pub fn primary_button<'a>(ui: &Ui, text: &str, size: f32) -> egui::Button<'a> {
    let visuals = ui.visuals();
    let label = egui::RichText::new(text)
        .size(size)
        .strong()
        .color(visuals.selection.stroke.color);
    egui::Button::new(label).fill(visuals.selection.bg_fill)
}

/// One clickable list row: title on the left, short meta text on the right.
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
    let visuals = ui.style().interact_selectable(&response, selected);
    let text_color = visuals.text_color();
    let meta_color = if selected { text_color } else { ui.visuals().weak_text_color() };
    let painter = ui.painter();
    let meta_galley =
        painter.layout_no_wrap(meta.to_owned(), TextStyle::Small.resolve(ui.style()), meta_color);
    let title_width = rect.width() - ROW_PADDING * 3.0 - meta_galley.size().x;
    let title_galley = ui.fonts(|fonts| {
        fonts.layout_job(single_line(title, TextStyle::Body.resolve(ui.style()), text_color, title_width))
    });
    let title_pos = pos2(rect.left() + ROW_PADDING, rect.center().y - title_galley.size().y / 2.0);
    let meta_pos = pos2(
        rect.right() - ROW_PADDING - meta_galley.size().x,
        rect.center().y - meta_galley.size().y / 2.0,
    );
    painter.galley(title_pos, title_galley, text_color);
    painter.galley(meta_pos, meta_galley, meta_color);
    response
}

fn paint_row_background(ui: &Ui, response: &Response, selected: bool) {
    let visuals = ui.visuals();
    let fill = if selected {
        Some(visuals.selection.bg_fill)
    } else if response.hovered() {
        Some(visuals.widgets.hovered.weak_bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(response.rect, CORNER, fill);
    }
    if response.has_focus() {
        let stroke = ui.visuals().widgets.hovered.fg_stroke;
        ui.painter()
            .rect_stroke(response.rect.shrink(1.0), CORNER, stroke, StrokeKind::Inside);
    }
}

fn single_line(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(24.0));
    job
}
