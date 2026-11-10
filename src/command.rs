//! Messages into the app's command queue.
//!
//! Tray, hotkey, second-launch and UIA-worker callbacks run outside the egui
//! frame. They push one of these and call `egui::Context::request_repaint`,
//! which wakes `App::logic` even while the popup is hidden.

use eframe::egui;

use crate::search::TabInfo;

#[derive(Debug)]
pub enum AppCommand {
    /// Hotkey: open if hidden, hide if shown.
    Toggle,
    /// Tray click / menu / second launch: open (or refocus).
    Open,
    /// Tray menu "Start with Windows" was clicked.
    ToggleStartup,
    /// Tray menu "Exit".
    Exit,
    /// UIA worker finished scanning Explorer tabs.
    Tabs { generation: u64, tabs: Vec<TabInfo> },
    /// UIA worker finished selecting a tab.
    TabSelected {
        explorer_hwnd: isize,
        result: Result<(), String>,
    },
}

/// Cloneable sender that also wakes the egui loop.
#[derive(Clone)]
pub struct CommandSender {
    tx: crossbeam_channel::Sender<AppCommand>,
    ctx: egui::Context,
}

impl CommandSender {
    pub fn new(tx: crossbeam_channel::Sender<AppCommand>, ctx: egui::Context) -> Self {
        CommandSender { tx, ctx }
    }

    pub fn send(&self, cmd: AppCommand) {
        if self.tx.send(cmd).is_ok() {
            self.ctx.request_repaint();
        }
    }
}
