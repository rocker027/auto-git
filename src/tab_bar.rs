//! The tab bar at the bottom of the sidebar, and its two icons.

use crate::theme::{self, CONTROL_RADIUS};
use crate::widgets::paint_focus_ring;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui,
    WidgetInfo, WidgetType,
};

const ITEM_HEIGHT: f32 = 48.0;
const BAR_PADDING: f32 = 4.0;
/// Marks the chosen tab by shape.
const INDICATOR_SIZE: egui::Vec2 = vec2(22.0, 3.0);
const _: () = assert!(ITEM_HEIGHT >= theme::TOUCH);
/// Height of the whole bar, for whoever reserves room for it.
pub const HEIGHT: f32 = ITEM_HEIGHT + BAR_PADDING * 2.0;
const ICON_STROKE: f32 = 1.6;

/// What the sidebar shows. The open note stays on the right in every tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Search, recent notes and folders.
    Notes,
    /// Backup state, its switches and the watched folders.
    Backup,
}

impl Tab {
    pub const ALL: [Self; 2] = [Self::Notes, Self::Backup];

    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => "筆記",
            Self::Backup => "備份",
        }
    }

    fn shortcut(self) -> &'static str {
        match self {
            Self::Notes => "⌘1",
            Self::Backup => "⌘2",
        }
    }
}

/// Keyboard requests that can change the tab this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabKeys {
    /// ⌘F: the search field only exists in the notes tab.
    pub find: bool,
    /// ⌘1
    pub notes: bool,
    /// ⌘2
    pub backup: bool,
}

/// The tab after this frame's shortcuts. ⌘F wins, so the focus it asks for has a field to land on.
pub fn tab_after_keys(current: Tab, keys: TabKeys) -> Tab {
    if keys.find || keys.notes {
        Tab::Notes
    } else if keys.backup {
        Tab::Backup
    } else {
        current
    }
}

/// Draws the bar and returns the tab clicked this frame.
pub fn show(ui: &mut Ui, current: Tab) -> Option<Tab> {
    let mut clicked = None;
    theme::card(ui).inner_margin(BAR_PADDING).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = BAR_PADDING;
        ui.columns(Tab::ALL.len(), |columns| {
            for (column, tab) in columns.iter_mut().zip(Tab::ALL) {
                let hint = format!("{} · {}", tab.label(), tab.shortcut());
                if tab_item(column, tab, tab == current).on_hover_text(hint).clicked() {
                    clicked = Some(tab);
                }
            }
        });
    });
    clicked
}

/// Icon above label. The chosen tab gets a bar above its icon, so the choice does not
/// rest on colour alone, plus a tinted pill and the accent colour.
fn tab_item(ui: &mut Ui, tab: Tab, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ITEM_HEIGHT), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, ui.is_enabled(), selected, tab.label())
    });
    let colors = theme::of(ui);
    let color = if selected { colors.accent_text } else { colors.secondary };
    if selected {
        ui.painter().rect_filled(rect, CONTROL_RADIUS, theme::tint(colors.accent_text, ui));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, CONTROL_RADIUS, colors.hover);
    }
    paint_focus_ring(ui, &response);
    if selected {
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.top() + 4.5), INDICATOR_SIZE);
        ui.painter().rect_filled(bar, INDICATOR_SIZE.y / 2.0, color);
    }
    let icon_center = pos2(rect.center().x, rect.top() + 20.0);
    match tab {
        Tab::Notes => paint_note_icon(ui, icon_center, color),
        Tab::Backup => paint_backup_icon(ui, icon_center, color),
    }
    let label_pos = pos2(rect.center().x, rect.bottom() - 4.0);
    ui.painter()
        .text(label_pos, Align2::CENTER_BOTTOM, tab.label(), FontId::proportional(11.5), color);
    response
}

/// A sheet of paper with two lines of text.
fn paint_note_icon(ui: &Ui, center: Pos2, color: Color32) {
    let stroke = Stroke::new(ICON_STROKE, color);
    let sheet = Rect::from_center_size(center, vec2(14.0, 17.0));
    ui.painter().rect_stroke(sheet, 3.0, stroke, egui::StrokeKind::Middle);
    for offset in [-2.5, 2.5] {
        let y = center.y + offset;
        ui.painter()
            .line_segment([pos2(center.x - 3.5, y), pos2(center.x + 3.5, y)], stroke);
    }
}

/// A clockwise arrow around a circle, as on the app icon.
fn paint_backup_icon(ui: &Ui, center: Pos2, color: Color32) {
    const RADIUS: f32 = 7.5;
    const STEPS: usize = 20;
    let stroke = Stroke::new(ICON_STROKE, color);
    let on_circle = |degrees: f32| {
        let angle = degrees.to_radians();
        center + RADIUS * vec2(angle.cos(), angle.sin())
    };
    // From the upper right, clockwise, leaving a gap for the arrow head.
    let arc: Vec<Pos2> = (0..=STEPS)
        .map(|step| on_circle(-30.0 + 285.0 * step as f32 / STEPS as f32))
        .collect();
    ui.painter().add(Shape::line(arc, stroke));
    let tip = on_circle(-30.0);
    let head = vec![tip + vec2(-5.0, -0.5), tip, tip + vec2(0.5, -5.0)];
    ui.painter().add(Shape::line(head, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sidebar's narrowest inner width: 220 minus its 12-point margins.
    const NARROW: f32 = 196.0;

    fn frame(ctx: &egui::Context, current: Tab, events: Vec<egui::Event>) -> (Option<Tab>, Rect) {
        let mut clicked = None;
        let mut bar = Rect::NOTHING;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(NARROW, 420.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ctx, |ui| {
                    let top = ui.cursor().min;
                    clicked = show(ui, current);
                    bar = Rect::from_min_max(top, pos2(top.x + NARROW, ui.cursor().min.y));
                });
        });
        (clicked, bar)
    }

    fn click(pos: Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    #[test]
    fn clicking_the_right_half_picks_backup_and_the_left_half_notes() {
        let ctx = egui::Context::default();
        let (clicked, bar) = frame(&ctx, Tab::Notes, vec![]);
        assert_eq!(clicked, None);
        assert!(bar.height() >= theme::TOUCH, "{bar:?}");
        let right = pos2(bar.left() + bar.width() * 0.75, bar.top() + 24.0);
        let left = pos2(bar.left() + bar.width() * 0.25, bar.top() + 24.0);
        assert_eq!(frame(&ctx, Tab::Notes, click(right)).0, Some(Tab::Backup));
        assert_eq!(frame(&ctx, Tab::Backup, click(left)).0, Some(Tab::Notes));
    }

    #[test]
    fn find_moves_to_the_notes_tab_where_the_search_field_is() {
        let find = TabKeys { find: true, ..TabKeys::default() };
        assert_eq!(tab_after_keys(Tab::Backup, find), Tab::Notes);
        assert_eq!(tab_after_keys(Tab::Notes, find), Tab::Notes);
        let backup = TabKeys { backup: true, ..TabKeys::default() };
        assert_eq!(tab_after_keys(Tab::Notes, backup), Tab::Backup);
        let notes = TabKeys { notes: true, ..TabKeys::default() };
        assert_eq!(tab_after_keys(Tab::Backup, notes), Tab::Notes);
        assert_eq!(tab_after_keys(Tab::Backup, TabKeys::default()), Tab::Backup);
        let both = TabKeys { find: true, backup: true, ..TabKeys::default() };
        assert_eq!(tab_after_keys(Tab::Notes, both), Tab::Notes, "⌘F needs its field");
    }
}
