//! Global shortcut registration (plan step 5).
//!
//! The manager must be created on the thread running the Win32 message loop.
//! A failed registration is reported, never fatal, and no other shortcut is
//! taken unless the user configured `fallback_hotkey`.

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

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
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
        let mut registered = Vec::new();
        let mut active_label = None;
        for (label, is_primary) in std::iter::once((primary, true)).chain(fallback.map(|f| (f, false))) {
            match parse(label) {
                Ok(hk) => match manager.register(hk) {
                    Ok(()) => {
                        registered.push(hk);
                        active_label.get_or_insert_with(|| label.to_string());
                    }
                    Err(e) => problems.push(format!(
                        "{} shortcut {label} could not be registered (another app may be using it): {e}",
                        if is_primary { "The" } else { "The fallback" }
                    )),
                },
                Err(e) => problems.push(e),
            }
        }

        let ids: Vec<u32> = registered.iter().map(|h| h.id()).collect();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed && ids.contains(&event.id) {
                sender.send(AppCommand::Toggle);
            }
        }));

        (
            Some(Hotkeys {
                manager,
                registered,
                active_label,
            }),
            problems,
        )
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
        let _ = self.manager.unregister_all(&self.registered);
    }
}
