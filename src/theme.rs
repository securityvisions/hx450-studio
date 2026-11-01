//! Colour palette and egui styling for the studio.

use eframe::egui::{self, Color32, CornerRadius, Stroke};

// Base surfaces
pub const BG: Color32 = Color32::from_rgb(0x0a, 0x0b, 0x0e);
pub const PANEL: Color32 = Color32::from_rgb(0x11, 0x13, 0x17);
pub const CARD: Color32 = Color32::from_rgb(0x15, 0x17, 0x1d);
pub const INPUT: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x13);
pub const HOVER: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x27);
pub const ACTIVE: Color32 = Color32::from_rgb(0x24, 0x28, 0x32);

// Lines
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x25, 0x2d);
pub const BORDER_HI: Color32 = Color32::from_rgb(0x35, 0x3a, 0x46);

// Type
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe8, 0xed);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x90, 0x9e);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5a, 0x60, 0x6d);

// Accents
pub const ACCENT: Color32 = Color32::from_rgb(0xf0, 0xa3, 0x3c);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0x4a, 0x35, 0x18);
pub const CYAN: Color32 = Color32::from_rgb(0x53, 0xb8, 0xd8);
pub const GOOD: Color32 = Color32::from_rgb(0x4c, 0xc3, 0x8a);
pub const BAD: Color32 = Color32::from_rgb(0xe5, 0x53, 0x4b);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = visuals();

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.slider_width = 150.0;
    style.spacing.scroll.bar_width = 6.0;

    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();

    ctx.set_style(style);
}

fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = INPUT;
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.hyperlink_color = CYAN;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };

    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_DIM);

    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_HI);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.widgets.active.bg_fill = ACTIVE;
    v.widgets.active.weak_bg_fill = ACTIVE;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.widgets.open.bg_fill = ACTIVE;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER_HI);
    v
}

