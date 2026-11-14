//! The search palette: one text field, a result list and a footer (plan step 7).

use eframe::egui::{
    self, Align, Color32, FontFamily, FontId, Key, Layout, Modifiers, RichText, Sense, Stroke, text::LayoutJob,
};

use super::styles::*;
use crate::search::{Kind, Shown};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    /// Activate the result at this index of the shown list.
    Activate(usize),
    /// Tab on a folder suggestion: the query was replaced by its completion.
    Completed,
    Hide,
    QueryChanged,
    /// Ctrl+Enter: search the web for the typed text, whatever is selected.
    WebSearch,
    /// Shift+Enter: show the file at this index in Explorer (other rows
    /// activate as with Enter).
    Reveal(usize),
}

#[derive(Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
    pub status: Option<Status>,
    /// Shown while Explorer or browser tabs are being read or switched.
    pub busy: Option<String>,
    /// Shown instead of "No matches." when there are no results (set by the
    /// app for type filters).
    pub empty_hint: Option<String>,
    focus_pending: bool,
    scroll_pending: bool,
    /// Move the text cursor to the end on the next frame (after Tab completion).
    cursor_to_end: bool,
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

    /// Draw the palette. `results` is the ranked list currently shown;
    /// `path_mode` is true while the query is a folder path.
    pub fn show(&mut self, ui: &mut egui::Ui, results: &[Shown], path_mode: bool) -> Option<PaletteAction> {
        let mut action = None;
        let n = results.len();
        if n == 0 {
            self.selected = 0;
        } else if self.selected >= n {
            self.selected = n - 1;
        }

        // Keys are consumed before the TextEdit sees them, so arrows don't
        // move the text cursor and Enter/Esc don't drop focus.
        // egui ignores an extra Shift when matching, so the Shift and Ctrl
        // combinations are taken first or plain Tab/Enter would swallow them.
        let (shift_tab, shift_enter, ctrl_enter, up, down, page_up, page_down, enter, escape, tab) =
            ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::SHIFT, Key::Tab),
                    i.consume_key(Modifiers::SHIFT, Key::Enter),
                    i.consume_key(Modifiers::CTRL, Key::Enter),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::PageUp),
                    i.consume_key(Modifiers::NONE, Key::PageDown),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                    i.consume_key(Modifiers::NONE, Key::Escape),
                    i.consume_key(Modifiers::NONE, Key::Tab),
                )
            });
        // In path mode Tab completes the selected folder instead of moving.
        let complete = path_mode && tab;
        let tab = tab && !path_mode;
        if complete && let Some(text) = results.get(self.selected).and_then(|r| r.completion.clone()) {
            self.query = text;
            self.selected = 0;
            self.scroll_pending = true;
            self.cursor_to_end = true;
            self.focus_pending = true;
            // Keep drawing this frame; the app refreshes the results next.
            action = Some(PaletteAction::Completed);
        }
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
        if shift_enter && n > 0 {
            action = Some(PaletteAction::Reveal(self.selected));
        }
        if ctrl_enter && !path_mode && !self.query.trim().is_empty() {
            action = Some(PaletteAction::WebSearch);
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
        let pasted = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))));
        let mut search_id = None;
        ui.horizontal(|ui| {
            magnifier(ui, SEARCH_FONT, TEXT_WEAK);
            let edit = egui::TextEdit::singleline(&mut self.query)
                .id_salt("quickswitch-search")
                .hint_text(RichText::new("Search windows and tabs, or type ~/folder").color(TEXT_FAINT))
                .font(FontId::new(SEARCH_FONT, FontFamily::Proportional))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY);
            let response = ui.add(edit).accessible_name("Search QuickSwitch");
            search_id = Some(response.id);
            if self.cursor_to_end {
                self.cursor_to_end = false;
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                    let end = egui::text::CCursor::new(self.query.chars().count());
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ui.ctx(), response.id);
                }
                ui.ctx().request_repaint();
            }
            if self.focus_pending || !response.has_focus() {
                response.request_focus();
                self.focus_pending = false;
            }
            if response.changed() {
                if pasted && let Some(clean) = tidy_pasted(&self.query) {
                    self.query = clean;
                    self.cursor_to_end = true;
                }
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
        let mut selected_row = None;
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if n == 0 {
                    ui.add_space(8.0);
                    let msg = match &self.empty_hint {
                        Some(hint) => hint.as_str(),
                        None if self.query.trim().is_empty() => "No open windows found.",
                        None => "No matches.",
                    };
                    ui.label(RichText::new(msg).color(TEXT_WEAK));
                }
                for (i, item) in results.iter().enumerate() {
                    let row = result_row(ui, item, i == self.selected);
                    if i == self.selected {
                        selected_row = Some(row.id);
                    }
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

        // Keyboard focus stays in the search field; tell screen readers which
        // row the arrow keys have highlighted so they read it out.
        if let Some(search) = search_id {
            ui.ctx().accesskit_node_builder(search, |node| match selected_row {
                Some(row) => node.set_active_descendant(row.accesskit_id()),
                None => node.clear_active_descendant(),
            });
        }

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
                        ui.spacing_mut().item_spacing.x = 4.0;
                        arrow_keys(ui, SMALL_FONT, TEXT_FAINT);
                        let on_file = results.get(self.selected).is_some_and(|r| r.item.kind() == Kind::File);
                        let hint = if on_file {
                            "navigate   Enter open   Shift+Enter show in folder   Esc close"
                        } else if path_mode {
                            "navigate   Tab complete   Enter open   Esc close"
                        } else {
                            "navigate   Enter select   Ctrl+Enter web   Esc close"
                        };
                        ui.label(RichText::new(hint).size(SMALL_FONT).color(TEXT_FAINT));
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
        Kind::BrowserTab => BADGE_BROWSER,
        Kind::TerminalTab => BADGE_TERMINAL,
        Kind::Web => BADGE_WEB,
        Kind::Folder => BADGE_FOLDER,
        Kind::File => BADGE_FILE,
    }
}

/// Search icon drawn as vector shapes (a ring and a handle), so it needs no
/// font glyph. `size` is the line height it sits in.
fn magnifier(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.8, color);
    let r = size * 0.28;
    let center = rect.center() - egui::vec2(size * 0.08, size * 0.08);
    painter.circle_stroke(center, r, stroke);
    let d = std::f32::consts::FRAC_1_SQRT_2;
    let start = center + egui::vec2(r * d, r * d);
    painter.line_segment([start, start + egui::vec2(size * 0.22, size * 0.22)], stroke);
}

/// Up and down arrow keys hint, drawn as two small arrows.
fn arrow_keys(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size * 1.3, size), Sense::hover());
    let painter = ui.painter();
    let stroke = Stroke::new(1.2, color);
    let h = size * 0.7;
    let head = size * 0.22;
    for (x, up) in [(rect.left() + size * 0.3, true), (rect.left() + size * 0.95, false)] {
        let top = egui::pos2(x, rect.center().y - h / 2.0);
        let bottom = egui::pos2(x, rect.center().y + h / 2.0);
        painter.line_segment([top, bottom], stroke);
        let tip = if up { top } else { bottom };
        let dy = if up { head } else { -head };
        painter.line_segment([tip, tip + egui::vec2(-head, dy)], stroke);
        painter.line_segment([tip, tip + egui::vec2(head, dy)], stroke);
    }
}

/// Append `text` to `job`, drawing the chars at `highlight` (sorted char
/// indices) in the highlight colour.
fn append_highlighted(job: &mut LayoutJob, text: &str, highlight: &[usize], leading: f32, base: egui::TextFormat) {
    let mut hl = base.clone();
    hl.color = HIGHLIGHT;
    let mut leading = leading;
    let mut run = String::new();
    let mut run_hl = false;
    for (i, c) in text.chars().enumerate() {
        let is_hl = highlight.binary_search(&i).is_ok();
        if is_hl != run_hl && !run.is_empty() {
            job.append(&run, leading, if run_hl { hl.clone() } else { base.clone() });
            leading = 0.0;
            run.clear();
        }
        run_hl = is_hl;
        run.push(c);
    }
    if !run.is_empty() {
        job.append(&run, leading, if run_hl { hl } else { base });
    }
}

/// One single-line row: title, dimmed context, and a kind badge on the right.
fn result_row(ui: &mut egui::Ui, shown: &Shown, selected: bool) -> egui::Response {
    let item = &shown.item;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), Sense::click());
    // The row is painted by hand, so name it for screen readers:
    // "Releases · owner/repo, Chrome, Browser tab".
    response.widget_info(|| {
        let mut label = item.title.clone();
        for part in [item.subtitle.as_str(), item.kind().label()] {
            if !part.is_empty() {
                label.push_str(", ");
                label.push_str(part);
            }
        }
        egui::WidgetInfo::selected(egui::accesskit::Role::ListBoxOption, true, selected, label)
    });
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
    append_highlighted(
        &mut job,
        &item.title,
        &shown.title_hl,
        0.0,
        egui::TextFormat::simple(FontId::new(TITLE_FONT, FontFamily::Proportional), TEXT),
    );
    if !item.subtitle.is_empty() {
        append_highlighted(
            &mut job,
            &item.subtitle,
            &shown.subtitle_hl,
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

/// Pasted text, tidied: surrounding spaces (a copied line's newline becomes a
/// space) and one pair of surrounding quotes, as Explorer's "Copy as path"
/// adds, are removed. `None` when there is nothing to change.
pub fn tidy_pasted(query: &str) -> Option<String> {
    let trimmed = query.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .map(str::trim)
        .unwrap_or(trimmed);
    (unquoted != query).then(|| unquoted.to_string())
}
