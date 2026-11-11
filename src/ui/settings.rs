//! Settings view, opened from the tray menu's "Settings…". Shown in the
//! popup window instead of the palette, with one tab per topic:
//! - Shortcut: the global shortcut (`shortcut::ShortcutEditor`);
//! - Web search: the engine used by the web row, `web:` and Ctrl+Enter.

use eframe::egui::{self, Align, Key, Layout, Modifiers, RichText, Stroke};

use super::palette::Status;
use super::shortcut::{ShortcutAction, ShortcutEditor};
use super::styles::*;
use crate::actions::web::{CUSTOM_ENGINE, ENGINES, check_custom_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Shortcut,
    Web,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    SaveShortcut(String),
    SaveWeb { engine: String, custom_url: String },
    Cancel,
}

pub struct Settings {
    pub tab: SettingsTab,
    pub shortcut: ShortcutEditor,
    pub web: WebEditor,
}

impl Settings {
    pub fn new(hotkey: &str, engine: &str, custom_url: &str) -> Self {
        Settings {
            tab: SettingsTab::Shortcut,
            shortcut: ShortcutEditor::new(hotkey),
            web: WebEditor::new(engine, custom_url),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<SettingsAction> {
        // Ctrl+Tab switches tabs.
        if ui.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Tab)) {
            self.tab = match self.tab {
                SettingsTab::Shortcut => SettingsTab::Web,
                SettingsTab::Web => SettingsTab::Shortcut,
            };
        }

        let rect = ui.max_rect();
        ui.painter()
            .rect(rect, 8.0, BG, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        let inner = rect.shrink2(egui::vec2(20.0, 14.0));
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut content;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").size(SEARCH_FONT));
            ui.add_space(18.0);
            for (tab, label) in [(SettingsTab::Shortcut, "Shortcut"), (SettingsTab::Web, "Web search")] {
                let text = RichText::new(label).size(TITLE_FONT);
                if ui.selectable_label(self.tab == tab, text).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(4.0);
        let y = ui.cursor().top();
        ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, BORDER));
        ui.add_space(10.0);

        match self.tab {
            SettingsTab::Shortcut => self.shortcut.show(ui).map(|a| match a {
                ShortcutAction::Save(text) => SettingsAction::SaveShortcut(text),
                ShortcutAction::Cancel => SettingsAction::Cancel,
            }),
            SettingsTab::Web => self.web.show(ui),
        }
    }
}

/// Web search tab: pick an engine, or enter a custom search address.
pub struct WebEditor {
    engine: String,
    custom_url: String,
    /// Error from the last save attempt.
    pub status: Option<Status>,
}

impl WebEditor {
    pub fn new(engine: &str, custom_url: &str) -> Self {
        let known = engine == CUSTOM_ENGINE || ENGINES.iter().any(|e| e.id == engine);
        WebEditor {
            engine: if known { engine } else { ENGINES[0].id }.to_string(),
            custom_url: custom_url.to_string(),
            status: None,
        }
    }

    fn check(&self) -> Result<(), String> {
        if self.engine == CUSTOM_ENGINE {
            check_custom_url(&self.custom_url)
        } else {
            Ok(())
        }
    }

    fn save(&self) -> SettingsAction {
        SettingsAction::SaveWeb {
            engine: self.engine.clone(),
            custom_url: self.custom_url.trim().to_string(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<SettingsAction> {
        let mut action = None;
        let (enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            return Some(SettingsAction::Cancel);
        }
        let valid = self.check();
        if enter && valid.is_ok() {
            action = Some(self.save());
        }

        ui.label(
            RichText::new("Used by the web row in results, web: and Ctrl+Enter. gpt: always asks ChatGPT.")
                .color(TEXT_WEAK),
        );
        ui.add_space(10.0);

        let before = (self.engine.clone(), self.custom_url.clone());
        egui::Grid::new("web-engines")
            .num_columns(4)
            .spacing([22.0, 8.0])
            .show(ui, |ui| {
                let choices = ENGINES
                    .iter()
                    .map(|e| (e.id, e.name))
                    .chain([(CUSTOM_ENGINE, "Custom…")]);
                for (i, (id, name)) in choices.enumerate() {
                    ui.radio_value(&mut self.engine, id.to_string(), name);
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(8.0);
        if self.engine == CUSTOM_ENGINE {
            ui.add(
                egui::TextEdit::singleline(&mut self.custom_url)
                    .hint_text("https://example.com/search?q={}")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
        }
        if (self.engine.clone(), self.custom_url.clone()) != before {
            self.status = None;
        }

        match (&self.status, &valid) {
            (Some(Status::Error(msg)), _) | (None, Err(msg)) => {
                ui.label(RichText::new(msg).color(ERROR));
            }
            (Some(Status::Info(msg)), _) => {
                ui.label(RichText::new(msg).color(TEXT_WEAK));
            }
            (None, Ok(())) => {}
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Enter save   Esc cancel   Ctrl+Tab next tab")
                        .size(SMALL_FONT)
                        .color(TEXT_FAINT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(valid.is_ok(), egui::Button::new("Save")).clicked() {
                        action = Some(self.save());
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(SettingsAction::Cancel);
                    }
                });
            });
        });

        action
    }
}
