//! Visual contract (plan §2.2): dark, compact, Windows 11-like.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub const BG: Color32 = Color32::from_rgb(0x1f, 0x20, 0x25);
pub const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3c, 0x45);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xf0);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x9a, 0x9f, 0xad);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6c, 0x71, 0x7e);
pub const ACCENT: Color32 = Color32::from_rgb(0x63, 0x66, 0xf1);
pub const ROW_SELECTED: Color32 = Color32::from_rgb(0x34, 0x36, 0x5c);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x34);
/// Matched characters in result rows.
pub const HIGHLIGHT: Color32 = Color32::from_rgb(0xa5, 0xb4, 0xfc);
pub const ERROR: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
pub const BADGE_WINDOW: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const BADGE_TAB: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
pub const BADGE_FOLDER: Color32 = Color32::from_rgb(0x22, 0xc5, 0x5e);

pub const SEARCH_FONT: f32 = 20.0;
pub const TITLE_FONT: f32 = 15.0;
pub const SMALL_FONT: f32 = 12.0;
pub const ROW_HEIGHT: f32 = 30.0;

/// Logical (DPI-independent) popup size.
pub const POPUP_SIZE: (f32, f32) = (680.0, 340.0);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = BG;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
    visuals.selection.stroke = Stroke::new(1.0, TEXT);
    visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(8);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(TITLE_FONT, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(SMALL_FONT, FontFamily::Proportional));
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating = true;
    });
}
