//! The search palette: one text field, a result list and a footer (plan step 7).

use eframe::egui::{
    self, Align, Color32, FontFamily, FontId, Key, Layout, Modifiers, RichText, Sense, Stroke, text::LayoutJob,
};

use super::styles::*;
use crate::search::{Kind, SearchItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    /// Activate the result at this index of the shown list.
    Activate(usize),
    Hide,
    QueryChanged,
}

#[derive(Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
    pub status: Option<Status>,
    /// Shown while the UIA worker is still scanning Explorer tabs.
    pub busy: Option<String>,
    focus_pending: bool,
    scroll_pending: bool,
}

impl Palette {
    /// Reset for a fresh opening: empty query, first row, focus the field.
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.status = None;
        self.busy = None;
        self.focus_pending = true;
        self.scroll_pending = true;
    }

    pub fn request_focus(&mut self) {
        self.focus_pending = true;
    }

    /// Draw the palette. `results` is the ranked list currently shown.
    pub fn show(&mut self, ui: &mut egui::Ui, results: &[SearchItem]) -> Option<PaletteAction> {
        let mut action = None;
        let n = results.len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }

        // Keys are consumed before the TextEdit sees them, so arrows don't
        // move the text cursor and Enter/Esc don't drop focus.
        let (up, down, page_up, page_down, enter, escape, tab, shift_tab) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::PageUp),
                i.consume_key(Modifiers::NONE, Key::PageDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
            )
        });
        if n > 0 {
            let before = self.selected;
            if up || shift_tab {
                self.selected = if self.selected == 0 { n - 1 } else { self.selected - 1 };
            }
            if down || tab {
                self.selected = (self.selected + 1) % n;
            }
            if page_up {
                self.selected = self.selected.saturating_sub(5);
            }
            if page_down {
                self.selected = (self.selected + 5).min(n - 1);
            }
            if self.selected != before {
                self.scroll_pending = true;
            }
        }
        if escape {
            return Some(PaletteAction::Hide);
        }
        if enter && n > 0 {
            action = Some(PaletteAction::Activate(self.selected));
        }

        let rect = ui.max_rect();
        ui.painter()
            .rect(rect, 8.0, BG, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);

        let inner = rect.shrink2(egui::vec2(14.0, 10.0));
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut content;

        // Search field.
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔍").size(SEARCH_FONT - 2.0).color(TEXT_WEAK));
            let edit = egui::TextEdit::singleline(&mut self.query)
                .id_salt("quickswitch-search")
                .hint_text(RichText::new("Search windows and Explorer tabs, or type ~/folder").color(TEXT_FAINT))
                .font(FontId::new(SEARCH_FONT, FontFamily::Proportional))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY);
            let response = ui.add(edit);
            if self.focus_pending || !response.has_focus() {
                response.request_focus();
                self.focus_pending = false;
            }
            if response.changed() {
                self.selected = 0;
                self.scroll_pending = true;
                action = Some(PaletteAction::QueryChanged);
            }
        });
        ui.add_space(6.0);
        let sep_y = ui.cursor().top();
        ui.painter().hline(rect.x_range(), sep_y, Stroke::new(1.0, BORDER));
        ui.add_space(6.0);

        // Results.
        let footer_height = 24.0;
        let list_height = (ui.available_height() - footer_height).max(ROW_HEIGHT);
        let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if n == 0 {
                    ui.add_space(8.0);
                    let msg = if self.query.trim().is_empty() {
                        "No open windows found."
                    } else {
                        "No matches."
                    };
                    ui.label(RichText::new(msg).color(TEXT_WEAK));
                }
                for (i, item) in results.iter().enumerate() {
                    let row = result_row(ui, item, i == self.selected);
                    if row.hovered() && pointer_moved {
                        self.selected = i;
                    }
                    if row.clicked() {
                        action = Some(PaletteAction::Activate(i));
                    }
                    if i == self.selected && self.scroll_pending {
                        row.scroll_to_me(None);
                    }
                }
                self.scroll_pending = false;
            });

        // Footer: status (if any) on the left, hints/count on the right.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                match &self.status {
                    Some(Status::Error(msg)) => {
                        ui.label(RichText::new(msg).size(SMALL_FONT).color(ERROR));
                    }
                    Some(Status::Info(msg)) => {
                        ui.label(RichText::new(msg).size(SMALL_FONT).color(TEXT_WEAK));
                    }
                    None => {
                        ui.label(
                            RichText::new("↑ ↓ navigate   Enter select   Esc close")
                                .size(SMALL_FONT)
                                .color(TEXT_FAINT),
                        );
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let count = if n == 1 {
                        "1 result".to_string()
                    } else {
                        format!("{n} results")
                    };
                    ui.label(RichText::new(count).size(SMALL_FONT).color(TEXT_FAINT));
                    if let Some(busy) = &self.busy {
                        ui.label(RichText::new(busy).size(SMALL_FONT).color(TEXT_FAINT));
                    }
                });
            });
        });

        action
    }
}

fn badge_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Window => BADGE_WINDOW,
        Kind::ExplorerTab => BADGE_TAB,
        Kind::OpenFolder => BADGE_FOLDER,
    }
}

/// One single-line row: title, dimmed context, and a kind badge on the right.
fn result_row(ui: &mut egui::Ui, item: &SearchItem, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter_at(rect);
    let fill = if selected {
        ROW_SELECTED
    } else if response.hovered() {
        ROW_HOVER
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 6.0, fill);
    if selected {
        let bar = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(0.0, 6.0),
            egui::vec2(3.0, rect.height() - 12.0),
        );
        painter.rect_filled(bar, 2.0, ACCENT);
    }

    // Kind badge, right-aligned.
    let kind = item.kind();
    let badge_font = FontId::new(SMALL_FONT - 1.0, FontFamily::Proportional);
    let badge_galley = painter.layout_no_wrap(kind.label().to_string(), badge_font, badge_color(kind));
    let badge_size = badge_galley.size() + egui::vec2(12.0, 6.0);
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 8.0 - badge_size.x, rect.center().y - badge_size.y / 2.0),
        badge_size,
    );
    painter.rect_filled(badge_rect, 4.0, badge_color(kind).gamma_multiply(0.18));
    painter.galley(badge_rect.min + egui::vec2(6.0, 3.0), badge_galley, Color32::WHITE);

    // Title + subtitle, elided to the space left of the badge.
    let text_left = rect.left() + 12.0;
    let max_width = (badge_rect.left() - 10.0 - text_left).max(20.0);
    let mut job = LayoutJob::default();
    job.append(
        &item.title,
        0.0,
        egui::TextFormat::simple(FontId::new(TITLE_FONT, FontFamily::Proportional), TEXT),
    );
    if !item.subtitle.is_empty() {
        job.append(
            &item.subtitle,
            10.0,
            egui::TextFormat::simple(FontId::new(SMALL_FONT, FontFamily::Proportional), TEXT_WEAK),
        );
    }
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let y = rect.center().y - galley.size().y / 2.0;
    painter.galley(egui::pos2(text_left, y), galley, TEXT);

    response
}
