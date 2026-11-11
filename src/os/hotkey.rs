//! Global shortcut registration (plan step 5).
//!
//! The manager must be created on the thread running the Win32 message loop.
//! A failed registration is reported, never fatal, and no other shortcut is
//! taken unless the user configured `fallback_hotkey`.
//!
//! The main shortcut can be changed while running ([`Hotkeys::replace`]); the
//! settings view picks it from dropdowns ([`Shortcut`]).

use std::sync::{Arc, Mutex};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::command::{AppCommand, CommandSender};

/// Parse "Ctrl+Win+K" style text. "Win"/"Windows" are accepted for the
/// Windows key (global-hotkey calls it "Super").
pub fn parse(text: &str) -> Result<HotKey, String> {
    let normalized: Vec<String> = text
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "win" | "windows" | "meta" => "super".to_string(),
            _ => p.to_string(),
        })
        .collect();
    normalized
        .join("+")
        .parse::<HotKey>()
        .map_err(|e| format!("Invalid shortcut “{text}”: {e}"))
}

/// A modifier key, in the order shortcuts are written ("Ctrl+Alt+Shift+Win").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mod {
    Ctrl,
    Alt,
    Shift,
    Win,
}

impl Mod {
    pub const ALL: [Mod; 4] = [Mod::Ctrl, Mod::Alt, Mod::Shift, Mod::Win];

    pub fn label(self) -> &'static str {
        match self {
            Mod::Ctrl => "Ctrl",
            Mod::Alt => "Alt",
            Mod::Shift => "Shift",
            Mod::Win => "Win",
        }
    }

    fn from_name(name: &str) -> Option<Mod> {
        match name.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Mod::Ctrl),
            "alt" | "option" => Some(Mod::Alt),
            "shift" => Some(Mod::Shift),
            "win" | "windows" | "super" | "meta" => Some(Mod::Win),
            _ => None,
        }
    }
}

/// Keys offered in the settings dropdown, as written in the shortcut text.
pub const KEYS: &[&str] = &[
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
    "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8",
    "F9", "F10", "F11", "F12", "Space", "Insert", "Delete", "Home", "End", "`", "-", "=", "[", "]", ";", "'", ",", ".",
    "/", "\\",
];

/// Up to three modifiers and a key, as picked in the settings view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub mods: [Option<Mod>; 3],
    /// One of [`KEYS`].
    pub key: &'static str,
}

impl Default for Shortcut {
    fn default() -> Self {
        Shortcut {
            mods: [Some(Mod::Ctrl), Some(Mod::Win), None],
            key: "K",
        }
    }
}

impl Shortcut {
    /// Read shortcut text such as "Ctrl+Win+K" back into dropdown choices.
    /// `None` when it uses more than three modifiers or a key not in [`KEYS`].
    pub fn from_text(text: &str) -> Option<Shortcut> {
        let parts: Vec<&str> = text.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
        let (key, mods) = parts.split_last()?;
        if mods.len() > 3 {
            return None;
        }
        let mut out = [None; 3];
        for (slot, name) in out.iter_mut().zip(mods) {
            *slot = Some(Mod::from_name(name)?);
        }
        let key = key_name(key)?;
        Some(Shortcut { mods: out, key })
    }

    /// Shortcut text ("Ctrl+Win+K"), or why this combination can't be used.
    /// Modifiers are written in a fixed order whatever dropdown they are in.
    pub fn to_text(&self) -> Result<String, String> {
        let mut mods: Vec<Mod> = self.mods.iter().flatten().copied().collect();
        mods.sort();
        if mods.is_empty() {
            return Err("Pick at least one modifier key".into());
        }
        if let Some(w) = mods.windows(2).find(|w| w[0] == w[1]) {
            return Err(format!("{} is picked twice", w[0].label()));
        }
        if mods == [Mod::Shift] {
            return Err("Shift alone would get in the way of typing; add Ctrl, Alt or Win".into());
        }
        let mut parts: Vec<&str> = mods.iter().map(|m| m.label()).collect();
        parts.push(self.key);
        let text = parts.join("+");
        parse(&text)?;
        Ok(text)
    }
}

/// The [`KEYS`] entry for a key name ("k", "KeyK", "Digit1", "space").
fn key_name(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    let short = upper
        .strip_prefix("KEY")
        .filter(|r| r.len() == 1)
        .or_else(|| upper.strip_prefix("DIGIT").filter(|r| r.len() == 1))
        .unwrap_or(&upper);
    KEYS.iter().copied().find(|k| k.eq_ignore_ascii_case(short))
}

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    /// IDs of the shortcuts registered now. The press handler reads this
    /// list: global-hotkey accepts a handler only once per process (later
    /// `set_event_handler` calls are ignored), so it can't be replaced when
    /// the shortcut changes.
    ids: Arc<Mutex<Vec<u32>>>,
    primary: Option<HotKey>,
    fallback: Option<HotKey>,
    /// The label of the shortcut that actually works, for the tray menu.
    pub active_label: Option<String>,
}

impl Hotkeys {
    /// Register `primary`, and `fallback` if given. Returns the manager plus
    /// a list of human-readable problems (empty when everything registered).
    pub fn register(sender: CommandSender, primary: &str, fallback: Option<&str>) -> (Option<Hotkeys>, Vec<String>) {
        let mut problems = Vec::new();
        let manager = match GlobalHotKeyManager::new() {
            Ok(m) => m,
            Err(e) => return (None, vec![format!("Global shortcuts are unavailable: {e}")]),
        };
        let ids = Arc::new(Mutex::new(Vec::new()));
        let handler_ids = ids.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let ours = handler_ids.lock().is_ok_and(|ids| ids.contains(&event.id));
            if event.state == HotKeyState::Pressed && ours {
                sender.send(AppCommand::Toggle);
            }
        }));
        let mut hotkeys = Hotkeys {
            manager,
            ids,
            primary: None,
            fallback: None,
            active_label: None,
        };
        for (label, is_primary) in std::iter::once((primary, true)).chain(fallback.map(|f| (f, false))) {
            match parse(label) {
                Ok(hk) => match hotkeys.manager.register(hk) {
                    Ok(()) => {
                        if is_primary {
                            hotkeys.primary = Some(hk);
                        } else {
                            hotkeys.fallback = Some(hk);
                        }
                        hotkeys.active_label.get_or_insert_with(|| label.to_string());
                    }
                    Err(e) => problems.push(format!(
                        "{} shortcut {label} could not be registered (another app may be using it): {e}",
                        if is_primary { "The" } else { "The fallback" }
                    )),
                },
                Err(e) => problems.push(e),
            }
        }

        hotkeys.sync_ids();
        (Some(hotkeys), problems)
    }

    /// Switch the main shortcut to `text`. The new one is registered before
    /// the old one is dropped, so on any error the old shortcut keeps working.
    pub fn replace(&mut self, text: &str) -> Result<(), String> {
        let hk = parse(text)?;
        if Some(hk) == self.primary {
            self.active_label = Some(text.to_string());
            return Ok(());
        }
        if Some(hk) == self.fallback {
            return Err(format!(
                "{text} is already your fallback shortcut (fallback_hotkey in config.toml)"
            ));
        }
        self.manager
            .register(hk)
            .map_err(|_| format!("{text} is already in use by Windows or another app. Pick another."))?;
        if let Some(old) = self.primary.replace(hk) {
            let _ = self.manager.unregister(old);
        }
        self.active_label = Some(text.to_string());
        self.sync_ids();
        Ok(())
    }

    /// Tell the press handler which shortcuts are ours now.
    fn sync_ids(&self) {
        if let Ok(mut ids) = self.ids.lock() {
            *ids = self.registered().map(|h| h.id()).collect();
        }
    }

    fn registered(&self) -> impl Iterator<Item = HotKey> + use<> {
        self.primary.into_iter().chain(self.fallback)
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        // The handler itself can't be removed (see `ids`); make it ignore everything.
        if let Ok(mut ids) = self.ids.lock() {
            ids.clear();
        }
        let registered: Vec<HotKey> = self.registered().collect();
        let _ = self.manager.unregister_all(&registered);
    }
}
