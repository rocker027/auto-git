//! Custom-painted controls: segmented switch, toggle tile, link row and the gradient card.
//!
//! Each one is at least `TOUCH` high, reports its role and state to assistive
//! technology, and shows keyboard focus.

use crate::theme::{self, CARD_RADIUS, CONTROL_RADIUS, TOUCH};
use crate::widgets::{paint_chevron, paint_focus_ring};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, TextStyle, Ui, WidgetInfo, WidgetType,
};

const SEGMENT_HEIGHT: f32 = 30.0;
const SEGMENT_INSET: f32 = 3.0;
const SEGMENT_RADIUS: f32 = 8.0;
const TILE_HEIGHT: f32 = 92.0;
const TILE_PADDING: f32 = 12.0;
const SWITCH_SIZE: egui::Vec2 = vec2(34.0, 20.0);
const GRADIENT_CORNER_STEPS: usize = 8;

/// One-of-many switch. `options` are (value, label, hover hint).
/// The track is drawn 36 high; each segment still takes clicks over the full `TOUCH` height.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: &mut T, options: &[(T, &str, &str)]) {
    let track = ui.painter().add(Shape::Noop);
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(SEGMENT_INSET);
        for (value, label, hint) in options {
            if segment(ui, label, *current == *value).on_hover_text(*hint).clicked() {
                *current = *value;
            }
        }
        ui.add_space(SEGMENT_INSET);
    });
    let size = vec2(row.response.rect.width(), SEGMENT_HEIGHT + SEGMENT_INSET * 2.0);
    let rect = Rect::from_center_size(row.response.rect.center(), size);
    let fill = theme::of(ui).control;
    ui.painter()
        .set(track, egui::epaint::RectShape::filled(rect, SEGMENT_RADIUS + SEGMENT_INSET, fill));
}

fn segment(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let font = TextStyle::Button.resolve(ui.style());
    let colors = theme::of(ui);
    let color = if selected { colors.text } else { colors.secondary };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 28.0, TOUCH), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, ui.is_enabled(), selected, label)
    });
    let pill = Rect::from_center_size(rect.center(), vec2(rect.width(), SEGMENT_HEIGHT));
    if selected {
        ui.painter().add(theme::soft_shadow(ui.visuals().dark_mode).as_shape(pill, SEGMENT_RADIUS));
        ui.painter().rect_filled(pill, SEGMENT_RADIUS, colors.raised);
    } else if response.hovered() {
        ui.painter().rect_filled(pill, SEGMENT_RADIUS, colors.hover);
    }
    if response.has_focus() {
        let ring = Stroke::new(2.0_f32, colors.accent_text);
        ui.painter().rect_stroke(pill, SEGMENT_RADIUS, ring, StrokeKind::Inside);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// An on/off setting as a tile for a two-column grid. The switch position and
/// `state` text carry the state, so it never depends on colour alone.
/// Everything is stacked on the left edge, so nothing overlaps in the narrowest sidebar.
pub fn toggle_tile(ui: &mut Ui, title: &str, state: &str, is_on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), TILE_HEIGHT), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), is_on, format!("{title}：{state}"))
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let colors = theme::of(ui);
    let fill = if response.hovered() { colors.hover } else { colors.surface };
    ui.painter().add(theme::soft_shadow(ui.visuals().dark_mode).as_shape(rect, CARD_RADIUS));
    ui.painter().rect_filled(rect, CARD_RADIUS, fill);
    paint_focus_ring(ui, &response);
    let inner = rect.shrink(TILE_PADDING);
    paint_switch(ui, Rect::from_min_size(inner.left_top(), SWITCH_SIZE), is_on);
    let title_font = TextStyle::Small.resolve(ui.style());
    let state_font = TextStyle::Body.resolve(ui.style());
    let title_pos = inner.left_top() + vec2(0.0, SWITCH_SIZE.y + 8.0);
    ui.painter().text(title_pos, Align2::LEFT_TOP, title, title_font, colors.secondary);
    ui.painter().text(inner.left_bottom(), Align2::LEFT_BOTTOM, state, state_font, colors.text);
    response
}

/// Knob on the right and accent track when on; knob on the left when off.
fn paint_switch(ui: &Ui, rect: Rect, is_on: bool) {
    let colors = theme::of(ui);
    let track = if is_on { theme::ACCENT } else { colors.control };
    let radius = rect.height() / 2.0;
    let knob_x = if is_on { rect.right() - radius } else { rect.left() + radius };
    ui.painter().rect_filled(rect, radius, track);
    ui.painter()
        .circle_filled(pos2(knob_x, rect.center().y), radius - 2.0, theme::ON_ACCENT);
    if !is_on {
        // The off track is close to the tile colour; the outline keeps the switch shape visible.
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0_f32, colors.secondary), StrokeKind::Inside);
    }
}

/// A full-width row that leads somewhere else: text on the left, chevron on the right.
pub fn link_row(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), TOUCH), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), text));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::ROW_RADIUS, theme::of(ui).hover);
    }
    paint_focus_ring(ui, &response);
    let font = TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 38.0).max(24.0));
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let pos = pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
    paint_chevron(ui, pos2(rect.right() - 14.0, rect.center().y), false, theme::of(ui).secondary);
    response
}

/// A warning that leads somewhere else. The text wraps, so a long cause is never cut off.
pub fn problem_banner(ui: &mut Ui, text: &str) -> Response {
    const PADDING: f32 = 12.0;
    let color = ui.visuals().error_fg_color;
    let frame = egui::Frame::new()
        .fill(theme::tint(color, ui))
        .corner_radius(CONTROL_RADIUS)
        .inner_margin(PADDING)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(TOUCH - PADDING * 2.0);
            let label = egui::RichText::new(text).color(color);
            // Not selectable: the whole banner is one click target.
            ui.add(egui::Label::new(label).wrap().selectable(false));
        });
    let response = ui.interact(frame.response.rect, ui.id().with("problem-banner"), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), text));
    paint_focus_ring(ui, &response);
    response
}

/// Magnifier drawn inside the left padding of the search field.
pub fn paint_search_icon(ui: &Ui, center: Pos2) {
    let stroke = Stroke::new(1.6_f32, theme::of(ui).secondary);
    let lens = center + vec2(-1.5, -1.5);
    ui.painter().circle_stroke(lens, 5.0, stroke);
    ui.painter()
        .line_segment([lens + vec2(3.6, 3.6), lens + vec2(7.5, 7.5)], stroke);
}

/// A card filled with a diagonal gradient. Text inside should use `theme::ON_ACCENT`.
pub fn gradient_card<R>(ui: &mut Ui, colors: [Color32; 2], add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let slot = ui.painter().add(Shape::Noop);
    let frame = egui::Frame::new().inner_margin(16.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add_contents(ui)
    });
    let rect = frame.response.rect;
    ui.painter().set(slot, Shape::mesh(gradient_mesh(rect, f32::from(CARD_RADIUS), colors)));
    frame.inner
}

/// Rounded rectangle as a triangle fan around its centre, coloured along the diagonal.
/// egui has no gradient fill, so the colour is set per vertex.
fn gradient_mesh(rect: Rect, radius: f32, [from, to]: [Color32; 2]) -> Mesh {
    let color_at = |pos: Pos2| {
        let across = (pos.x - rect.left()) / rect.width().max(1.0);
        let down = (pos.y - rect.top()) / rect.height().max(1.0);
        from.lerp_to_gamma(to, ((across + down) / 2.0).clamp(0.0, 1.0))
    };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), color_at(rect.center()));
    for pos in rounded_outline(rect, radius) {
        mesh.colored_vertex(pos, color_at(pos));
    }
    let outline = mesh.vertices.len() as u32 - 1;
    for index in 0..outline {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % outline);
    }
    mesh
}

/// Points along the outline, clockwise from the right edge of the bottom-right corner.
fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let inner = rect.shrink(radius);
    let corners = [
        (inner.right_bottom(), 0.0_f32),
        (inner.left_bottom(), 90.0),
        (inner.left_top(), 180.0),
        (inner.right_top(), 270.0),
    ];
    let mut points = Vec::with_capacity(corners.len() * (GRADIENT_CORNER_STEPS + 1));
    for (center, start) in corners {
        for step in 0..=GRADIENT_CORNER_STEPS {
            let angle = (start + 90.0 * step as f32 / GRADIENT_CORNER_STEPS as f32).to_radians();
            points.push(center + radius * vec2(angle.cos(), angle.sin()));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Pick {
        One,
        Two,
    }

    fn run(ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut Ui)) {
        let mut add = add;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(220.0, 420.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| add(ui));
        });
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
    fn a_segment_takes_clicks_over_the_full_touch_height() {
        let ctx = egui::Context::default();
        let options = [(Pick::One, "編輯", ""), (Pick::Two, "預覽", "")];
        let mut pick = Pick::One;
        let target = std::cell::Cell::new(Pos2::ZERO);
        let show = |ui: &mut Ui, pick: &mut Pick| {
            let font = TextStyle::Button.resolve(ui.style());
            let width = |label: &str| {
                ui.painter().layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE).size().x + 28.0
            };
            // Middle of the second segment, two points below the top of the row:
            // inside the click area but outside the painted pill.
            let across = SEGMENT_INSET + width("編輯") + width("預覽") / 2.0;
            target.set(ui.cursor().min + vec2(across, 2.0));
            segmented(ui, pick, &options);
        };
        run(&ctx, vec![], |ui| show(ui, &mut pick));
        run(&ctx, click(target.get()), |ui| show(ui, &mut pick));
        run(&ctx, vec![], |ui| show(ui, &mut pick));
        assert_eq!(pick, Pick::Two);
    }

    /// The sidebar's narrowest inner width.
    const NARROW: f32 = 196.0;

    /// Real glyph widths need the system CJK font; without it the check would measure fallback boxes.
    fn context_with_cjk_font() -> Option<egui::Context> {
        let bytes = crate::platform::shared_cjk_font()?;
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_static(bytes)));
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)?
            .push("cjk".to_owned());
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        crate::theme::apply_style(&ctx);
        Some(ctx)
    }

    #[test]
    fn tile_text_fits_beside_nothing_and_inside_the_tile_in_the_narrowest_sidebar() {
        let Some(ctx) = context_with_cjk_font() else {
            eprintln!("SKIPPED: no system CJK font, tile text was not measured");
            return;
        };
        let mut tiles = Vec::new();
        let mut widest_text = 0.0_f32;
        let mut text_rows = 0.0_f32;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(NARROW, 420.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
                let size = |text: &str, style: TextStyle| {
                    let font = style.resolve(ui.style());
                    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size()
                };
                for title in ["自動備份", "自動推送"] {
                    widest_text = widest_text.max(size(title, TextStyle::Small).x);
                }
                for state in ["已暫停", "開啟", "關閉"] {
                    widest_text = widest_text.max(size(state, TextStyle::Body).x);
                }
                text_rows = size("自動備份", TextStyle::Small).y + size("已暫停", TextStyle::Body).y;
                ui.columns(2, |columns| {
                    tiles.push(toggle_tile(&mut columns[0], "自動備份", "已暫停", false).rect);
                    tiles.push(toggle_tile(&mut columns[1], "自動推送", "開啟", true).rect);
                });
            });
        });
        assert_eq!(tiles.len(), 2);
        assert!(tiles[0].right() < tiles[1].left(), "tiles sit side by side");
        for tile in &tiles {
            assert!(tile.height() >= TOUCH, "{tile:?}");
            let room = tile.width() - TILE_PADDING * 2.0;
            assert!(widest_text > 30.0, "measured real glyphs: {widest_text}");
            assert!(widest_text <= room && SWITCH_SIZE.x <= room, "text {widest_text} in {room}");
        }
        // Stacked rows: switch, title, state. They must not run into each other.
        let rows = SWITCH_SIZE.y + 8.0 + text_rows;
        assert!(text_rows > 20.0, "measured real rows: {text_rows}");
        assert!(rows <= TILE_HEIGHT - TILE_PADDING * 2.0, "{rows}");
    }

    #[test]
    fn link_rows_are_touch_sized() {
        let ctx = egui::Context::default();
        let mut height = 0.0;
        run(&ctx, vec![], |ui| height = link_row(ui, "backed up", Color32::GRAY).rect.height());
        assert!(height >= TOUCH, "{height}");
    }

    #[test]
    fn the_gradient_runs_from_the_top_left_to_the_bottom_right_inside_its_card() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(196.0, 90.0));
        let [from, to] = theme::GRADIENT;
        let mesh = gradient_mesh(rect, 16.0, [from, to]);
        assert!(mesh.vertices.iter().all(|vertex| rect.expand(0.01).contains(vertex.pos)));
        assert_eq!(mesh.indices.len() % 3, 0);
        assert_eq!(mesh.vertices[0].color, from.lerp_to_gamma(to, 0.5), "centre is the midpoint");
        let nearest = |corner: Pos2| {
            let distance = |pos: Pos2| (pos - corner).length();
            mesh.vertices[1..]
                .iter()
                .min_by(|a, b| distance(a.pos).total_cmp(&distance(b.pos)))
                .map(|vertex| vertex.color)
                .unwrap()
        };
        let close = |a: Color32, b: Color32| {
            a.to_array().iter().zip(b.to_array()).all(|(x, y)| x.abs_diff(y) <= 12)
        };
        assert!(close(nearest(rect.left_top()), from), "{:?}", nearest(rect.left_top()));
        assert!(close(nearest(rect.right_bottom()), to), "{:?}", nearest(rect.right_bottom()));
    }
}
