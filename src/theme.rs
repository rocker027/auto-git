//! App-wide look: colour and size tokens, the egui style, and raised surfaces.
//!
//! Layers are told apart by surface colour and a soft shadow, not by borders.

use eframe::egui::{self, vec2, Color32, FontId, Shadow, Stroke, TextStyle, Ui};

/// Cards and windows.
pub const CARD_RADIUS: u8 = 16;
/// Buttons, text fields and banners.
pub const CONTROL_RADIUS: u8 = 12;
/// Rows inside a card.
pub const ROW_RADIUS: u8 = 10;
/// Smallest height of anything that can be clicked.
pub const TOUCH: f32 = 44.0;
/// Space between cards, and between a card and the window edge.
pub const GUTTER: f32 = 12.0;
/// Width of the keyboard focus ring.
pub const FOCUS_RING: f32 = 2.0;

pub const ACCENT: Color32 = Color32::from_rgb(0, 92, 200);
pub const ON_ACCENT: Color32 = Color32::WHITE;
/// Blue to the teal of the app icon; white text stays above 4.5:1 on both ends.
pub const GRADIENT: [Color32; 2] = [Color32::from_rgb(10, 96, 208), Color32::from_rgb(10, 127, 110)];

/// Colours of one appearance. Text colours were checked against every surface they sit on.
pub struct Palette {
    /// Behind the cards.
    pub background: Color32,
    /// Cards, the document and dialogs.
    pub surface: Color32,
    /// Resting fill of text fields, buttons and the segmented track.
    pub control: Color32,
    pub hover: Color32,
    /// The chosen segment, lifted off the track.
    pub raised: Color32,
    pub text: Color32,
    pub secondary: Color32,
    /// Accent used as text or icon colour on a surface.
    pub accent_text: Color32,
    /// Behind selected text in the editor; the text keeps its own colour.
    pub text_selection: Color32,
    pub error: Color32,
    pub warning: Color32,
}

const LIGHT: Palette = Palette {
    background: Color32::from_rgb(242, 242, 247),
    surface: Color32::WHITE,
    control: Color32::from_rgb(228, 228, 234),
    hover: Color32::from_rgb(218, 218, 225),
    raised: Color32::WHITE,
    text: Color32::from_rgb(28, 28, 30),
    secondary: Color32::from_rgb(92, 92, 97),
    accent_text: ACCENT,
    text_selection: Color32::from_rgb(179, 215, 255),
    error: Color32::from_rgb(179, 38, 30),
    warning: Color32::from_rgb(178, 80, 0),
};

const DARK: Palette = Palette {
    background: Color32::from_rgb(21, 21, 23),
    surface: Color32::from_rgb(33, 33, 36),
    control: Color32::from_rgb(46, 46, 51),
    hover: Color32::from_rgb(58, 58, 64),
    raised: Color32::from_rgb(90, 90, 96),
    text: Color32::from_rgb(235, 235, 240),
    secondary: Color32::from_rgb(174, 174, 178),
    accent_text: Color32::from_rgb(100, 180, 255),
    text_selection: Color32::from_rgb(63, 99, 139),
    error: Color32::from_rgb(255, 123, 114),
    warning: Color32::from_rgb(255, 179, 64),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark { &DARK } else { &LIGHT }
}

/// The palette of the appearance this `ui` is drawn in.
pub fn of(ui: &Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

/// A quiet colour wash for the chosen tab, a switched-on tile and banners.
pub fn tint(color: Color32, ui: &Ui) -> Color32 {
    let strength = if ui.visuals().dark_mode { 0.16 } else { 0.12 };
    of(ui).surface.lerp_to_gamma(color, strength)
}

/// Shadows barely show on dark surfaces; there the lighter surface does the lifting.
pub fn soft_shadow(dark: bool) -> Shadow {
    Shadow {
        offset: [0, 1],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 70 } else { 18 }),
    }
}

/// A raised, borderless surface with rounded corners.
pub fn card(ui: &Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(of(ui).surface)
        .corner_radius(CARD_RADIUS)
        .shadow(soft_shadow(ui.visuals().dark_mode))
}

/// Right-click menus are pointer-only, so their rows stay at the denser macOS height.
pub fn compact_menu(ui: &mut Ui) {
    ui.spacing_mut().interact_size.y = 30.0;
}

/// Applies sizes and colours to the light and the dark style.
pub fn apply_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        set_sizes(style);
        let colors = palette(style.visuals.dark_mode);
        set_surfaces(&mut style.visuals, colors);
        set_controls(&mut style.visuals, colors);
    });
}

fn set_sizes(style: &mut egui::Style) {
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(28.0)),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Small, FontId::proportional(12.5)),
        (TextStyle::Monospace, FontId::monospace(14.0)),
    ]
    .into();
    style.spacing.button_padding = vec2(14.0, 8.0);
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.interact_size.y = TOUCH;
    style.spacing.window_margin = egui::Margin::same(20);
    style.spacing.menu_margin = egui::Margin::same(8);
}

fn set_surfaces(visuals: &mut egui::Visuals, colors: &Palette) {
    let shadow = soft_shadow(visuals.dark_mode);
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.window_stroke = Stroke::NONE;
    visuals.window_corner_radius = CARD_RADIUS.into();
    visuals.menu_corner_radius = CONTROL_RADIUS.into();
    visuals.window_shadow = shadow;
    visuals.popup_shadow = shadow;
    visuals.extreme_bg_color = colors.control;
    visuals.faint_bg_color = colors.control;
    visuals.code_bg_color = colors.control;
    visuals.hyperlink_color = colors.accent_text;
    visuals.error_fg_color = colors.error;
    visuals.warn_fg_color = colors.warning;
    // Only text selection uses these; chosen rows and the main button take ACCENT directly.
    visuals.selection.bg_fill = colors.text_selection;
    visuals.selection.stroke = Stroke::new(1.0_f32, colors.accent_text);
}

fn set_controls(visuals: &mut egui::Visuals, colors: &Palette) {
    visuals.widgets.noninteractive.fg_stroke.color = colors.text;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, colors.control);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CONTROL_RADIUS.into();
        widget.bg_stroke = Stroke::NONE;
        widget.fg_stroke = Stroke::new(1.0_f32, colors.text);
        widget.bg_fill = colors.control;
        widget.weak_bg_fill = colors.control;
        // Controls do not grow on hover, so a ring at the edge of a clipped area is never cut off.
        widget.expansion = 0.0;
    }
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, colors.accent_text);
    // egui draws a control with keyboard focus in its `active` style, so this is the focus ring.
    visuals.widgets.active.bg_stroke = Stroke::new(FOCUS_RING, colors.accent_text);
    for widget in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        widget.bg_fill = colors.hover;
        widget.weak_bg_fill = colors.hover;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(text: Color32, surface: Color32) -> f32 {
        let luminance = |color: Color32| {
            let channel = |value: u8| {
                let value = f32::from(value) / 255.0;
                if value <= 0.03928 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
        };
        let (a, b) = (luminance(text), luminance(surface));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_colours_reach_normal_text_contrast_on_their_surfaces() {
        for colors in [&LIGHT, &DARK] {
            let surfaces = [colors.background, colors.surface, colors.control, colors.hover];
            for surface in surfaces {
                assert!(contrast(colors.text, surface) >= 4.5);
                assert!(contrast(colors.secondary, surface) >= 4.5);
            }
            for surface in [colors.background, colors.surface] {
                assert!(contrast(colors.accent_text, surface) >= 4.5);
                assert!(contrast(colors.error, surface) >= 4.5);
            }
            assert!(contrast(colors.text, colors.raised) >= 4.5);
            assert!(contrast(colors.text, colors.text_selection) >= 4.5);
        }
        for end in GRADIENT {
            assert!(contrast(ON_ACCENT, end) >= 4.5);
        }
        assert!(contrast(ON_ACCENT, ACCENT) >= 4.5);
    }

    #[test]
    fn a_focused_control_gets_a_ring_that_shows_on_its_surface() {
        let ctx = egui::Context::default();
        apply_style(&ctx);
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let style = ctx.style_of(theme);
            let colors = palette(style.visuals.dark_mode);
            // egui paints a control that has keyboard focus with `active`.
            let ring = style.visuals.widgets.active.bg_stroke;
            assert!(ring.width >= FOCUS_RING);
            for surface in [colors.background, colors.surface] {
                assert!(contrast(ring.color, surface) >= 3.0);
            }
        }
    }

    #[test]
    fn tinted_surfaces_keep_their_text_readable() {
        for (dark, strength) in [(false, 0.12), (true, 0.16)] {
            let colors = palette(dark);
            let wash = |color: Color32| colors.surface.lerp_to_gamma(color, strength);
            assert!(contrast(colors.accent_text, wash(colors.accent_text)) >= 4.5);
            assert!(contrast(colors.error, wash(colors.error)) >= 4.5);
            assert!(contrast(colors.text, wash(colors.warning)) >= 4.5);
        }
    }
}
