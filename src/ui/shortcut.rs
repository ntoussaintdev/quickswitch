//! Shortcut settings view: three modifier dropdowns and a key dropdown.
//!
//! Shown in the popup window instead of the palette, opened from the tray
//! menu's "Change shortcut…". Keys can't be recorded by pressing them because
//! Windows handles many Win+key combinations before an app sees them.

use eframe::egui::{self, Align, ComboBox, Key, Layout, Modifiers, RichText, Stroke};

use super::palette::Status;
use super::styles::*;
use crate::os::hotkey::{KEYS, Mod, Shortcut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutAction {
    /// Use this shortcut text (already validated).
    Save(String),
    Cancel,
}

pub struct ShortcutEditor {
    choice: Shortcut,
    /// The shortcut in use now, shown for reference.
    current: String,
    /// Error from the last save attempt (e.g. the combination is taken).
    pub status: Option<Status>,
}

impl ShortcutEditor {
    /// Start from the current shortcut, or the default if the dropdowns can't
    /// show it (e.g. a key that isn't offered).
    pub fn new(current: &str) -> Self {
        let parsed = Shortcut::from_text(current);
        let status = parsed.is_none().then(|| {
            Status::Info(format!(
                "“{current}” can't be shown in the dropdowns; pick a new shortcut"
            ))
        });
        ShortcutEditor {
            choice: parsed.unwrap_or_default(),
            current: current.to_string(),
            status,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<ShortcutAction> {
        let mut action = None;
        let text = self.choice.to_text();

        // Enter/Esc act on the view, unless a dropdown is open (then they
        // pick or close the dropdown).
        if !egui::Popup::is_any_open(ui.ctx()) {
            let (enter, escape) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::Enter),
                    i.consume_key(Modifiers::NONE, Key::Escape),
                )
            });
            if escape {
                return Some(ShortcutAction::Cancel);
            }
            if enter && let Ok(t) = &text {
                action = Some(ShortcutAction::Save(t.clone()));
            }
        }

        let rect = ui.max_rect();
        ui.painter()
            .rect(rect, 8.0, BG, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        let inner = rect.shrink2(egui::vec2(20.0, 16.0));
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut content;

        ui.label(RichText::new("Keyboard shortcut").size(SEARCH_FONT));
        ui.label(RichText::new(format!("Opens QuickSwitch from anywhere. Current: {}", self.current)).color(TEXT_WEAK));
        ui.add_space(18.0);

        let before = self.choice.clone();
        ui.horizontal(|ui| {
            for (i, slot) in self.choice.mods.iter_mut().enumerate() {
                ComboBox::from_id_salt(("shortcut-mod", i))
                    .width(96.0)
                    .selected_text(slot.map_or("(none)", Mod::label))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(slot, None, "(none)");
                        for m in Mod::ALL {
                            ui.selectable_value(slot, Some(m), m.label());
                        }
                    });
                ui.label(RichText::new("+").color(TEXT_WEAK));
            }
            ComboBox::from_id_salt("shortcut-key")
                .width(96.0)
                .height(180.0)
                .selected_text(self.choice.key)
                .show_ui(ui, |ui| {
                    for &k in KEYS {
                        ui.selectable_value(&mut self.choice.key, k, k);
                    }
                });
        });
        if self.choice != before {
            self.status = None;
        }

        ui.add_space(14.0);
        match (&self.status, &text) {
            (Some(Status::Error(msg)), _) | (None, Err(msg)) => {
                ui.label(RichText::new(msg).color(ERROR));
            }
            (Some(Status::Info(msg)), _) => {
                ui.label(RichText::new(msg).color(TEXT_WEAK));
            }
            (None, Ok(t)) if *t == self.current => {
                ui.label(RichText::new("This is the current shortcut.").color(TEXT_WEAK));
            }
            (None, Ok(t)) => {
                ui.label(RichText::new(format!("New shortcut: {t}")).color(TEXT));
            }
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Enter save   Esc cancel")
                        .size(SMALL_FONT)
                        .color(TEXT_FAINT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let save = ui.add_enabled(text.is_ok(), egui::Button::new("Save"));
                    if save.clicked()
                        && let Ok(t) = &text
                    {
                        action = Some(ShortcutAction::Save(t.clone()));
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(ShortcutAction::Cancel);
                    }
                });
            });
        });

        action
    }
}
