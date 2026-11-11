//! Notification-area icon and menu (plan step 4, §7.1).
//!
//! Must be created on the thread running the native event loop (eframe's
//! main thread). The returned `Tray` must be kept alive; dropping it removes
//! the icon. tray-icon re-adds the icon itself when Explorer restarts
//! (it listens for the `TaskbarCreated` broadcast).

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::command::{AppCommand, CommandSender};

const ICON_32: &[u8] = include_bytes!("../../resources/icon_32.rgba");

pub struct Tray {
    icon: TrayIcon,
    open_item: MenuItem,
    startup_item: CheckMenuItem,
}

pub fn icon_rgba() -> (Vec<u8>, u32, u32) {
    (ICON_32.to_vec(), 32, 32)
}

impl Tray {
    pub fn new(sender: CommandSender, hotkey_label: &str, startup_enabled: bool) -> Result<Tray, String> {
        let open_item = MenuItem::new(open_label(hotkey_label), true, None);
        let shortcut_item = MenuItem::new("Change shortcut…", true, None);
        let startup_item = CheckMenuItem::new("Start with Windows", true, startup_enabled, None);
        let exit_item = MenuItem::new("Exit QuickSwitch", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &open_item,
            &shortcut_item,
            &startup_item,
            &PredefinedMenuItem::separator(),
            &exit_item,
        ])
        .map_err(|e| e.to_string())?;

        let shortcut_id: MenuId = shortcut_item.id().clone();
        let (open_id, startup_id, exit_id): (MenuId, MenuId, MenuId) = (
            open_item.id().clone(),
            startup_item.id().clone(),
            exit_item.id().clone(),
        );
        let menu_sender = sender.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == open_id {
                menu_sender.send(AppCommand::Open);
            } else if event.id == shortcut_id {
                menu_sender.send(AppCommand::EditShortcut);
            } else if event.id == startup_id {
                menu_sender.send(AppCommand::ToggleStartup);
            } else if event.id == exit_id {
                menu_sender.send(AppCommand::Exit);
            }
        }));

        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                sender.send(AppCommand::Open);
            }
        }));

        let (rgba, w, h) = icon_rgba();
        let icon = Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip(tooltip(hotkey_label))
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;

        Ok(Tray {
            icon: tray,
            open_item,
            startup_item,
        })
    }

    /// Show the real registry state, not merely the last click.
    pub fn set_startup_checked(&self, checked: bool) {
        self.startup_item.set_checked(checked);
    }

    /// Show a new shortcut in the menu and tooltip.
    pub fn set_hotkey_label(&self, hotkey_label: &str) {
        self.open_item.set_text(open_label(hotkey_label));
        self.set_tooltip(&tooltip(hotkey_label));
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }
}

fn open_label(hotkey_label: &str) -> String {
    format!("Open search\t{hotkey_label}")
}

fn tooltip(hotkey_label: &str) -> String {
    format!("QuickSwitch ({hotkey_label})")
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Stop delivering events into a queue nobody reads any more.
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
        TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
    }
}
