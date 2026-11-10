//! The eframe app: lifecycle and commands in `App::logic`, the palette in
//! `App::ui` (plan §4, §7.3).
//!
//! Hidden-window notes for eframe 0.37 (checked against its source):
//! - eframe shows the root window after its first paint no matter what
//!   `with_visible(false)` said. So the window starts off-screen and
//!   non-activating (`with_active(false)`), paints once, and is then hidden
//!   with `ViewportCommand::Visible(false)`. The user never sees it.
//! - Visibility always goes through `ViewportCommand::Visible` so winit's own
//!   visibility state stays correct. Position and focus use Win32 directly:
//!   egui's `ViewportCommand::Focus` is implemented in winit with a simulated
//!   Alt keypress, which the plan rules out (§8, focus stealing).

use std::path::PathBuf;

use crossbeam_channel::Receiver;
use eframe::egui::{self, ViewportCommand};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::actions::{self, paths};
use crate::command::{AppCommand, CommandSender};
use crate::config::Config;
use crate::os::hotkey::Hotkeys;
use crate::os::instance::{PrimaryInstance, Signal};
use crate::os::tray::Tray;
use crate::os::{self, windows as win};
use crate::search::explorer_tabs::{Request, TabWorker};
use crate::search::matcher::Ranker;
use crate::search::{SearchItem, Snapshot, Target, folder_item};
use crate::startup;
use crate::ui::palette::{Palette, PaletteAction, Status};
use crate::ui::styles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Startup {
    /// Waiting for eframe's mandatory first paint (window is off-screen).
    AwaitFirstPaint,
    /// First paint done; hide on the next logic pass.
    Painted,
    Done,
}

pub struct QuickSwitchApp {
    hwnd: isize,
    rx: Receiver<AppCommand>,
    tray: Option<Tray>,
    hotkeys: Option<Hotkeys>,
    worker: Option<TabWorker>,
    instance: Option<PrimaryInstance>,
    config: Config,
    config_path: Option<PathBuf>,
    home: PathBuf,
    exe: PathBuf,

    ranker: Ranker,
    palette: Palette,
    snapshot: Snapshot,
    /// Ranked results currently on screen.
    shown: Vec<SearchItem>,
    generation: u64,

    startup: Startup,
    visible: bool,
    had_focus: bool,
    /// Re-center once after showing (a monitor/DPI change can shift the window).
    place_again: Option<(windows::Win32::Foundation::RECT, f32)>,
    focus_attempts: u8,
    /// Explorer window we asked the worker to switch a tab in.
    pending_tab: Option<isize>,
    exiting: bool,
}

impl QuickSwitchApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut instance: Option<PrimaryInstance>,
        tx: crossbeam_channel::Sender<AppCommand>,
        rx: Receiver<AppCommand>,
    ) -> Self {
        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        };
        styles::apply(&cc.egui_ctx);
        if hwnd != 0 {
            win::round_corners(hwnd);
        }

        let sender = CommandSender::new(tx, cc.egui_ctx.clone());

        let config_path = Config::default_path();
        let (config, warning) = match &config_path {
            Some(p) => Config::load(p),
            None => (Config::default(), None),
        };
        if let Some(w) = warning {
            os::log(&w);
        }

        if let Some(inst) = instance.as_mut() {
            let s = sender.clone();
            inst.listen(move |signal| {
                s.send(match signal {
                    Signal::Open => AppCommand::Open,
                    Signal::Exit => AppCommand::Exit,
                })
            });
        }

        // Tray and hotkey must be created here, on the event-loop thread.
        let (hotkeys, problems) = Hotkeys::register(sender.clone(), &config.hotkey, config.fallback_hotkey.as_deref());
        let hotkey_label = hotkeys.as_ref().and_then(|h| h.active_label.clone());
        let tray = match Tray::new(
            sender.clone(),
            hotkey_label.as_deref().unwrap_or("no shortcut"),
            startup::is_enabled(),
        ) {
            Ok(t) => Some(t),
            Err(e) => {
                os::warn(format!("QuickSwitch could not create its tray icon: {e}"));
                None
            }
        };
        if !problems.is_empty() {
            let mut msg = problems.join("\n");
            msg.push_str(
                "\n\nQuickSwitch is still running: click its tray icon, or run QuickSwitch.exe again, to open search. \
                 You can set `fallback_hotkey` in config.toml (see the README).",
            );
            if let Some(t) = &tray {
                t.set_tooltip("QuickSwitch (shortcut unavailable; click to open)");
            }
            os::warn(msg);
        }

        let worker = TabWorker::start(sender);

        QuickSwitchApp {
            hwnd,
            rx,
            tray,
            hotkeys,
            worker: Some(worker),
            instance,
            config,
            config_path,
            home: std::env::home_dir().unwrap_or_else(|| PathBuf::from("C:\\")),
            exe: std::env::current_exe().unwrap_or_default(),
            ranker: Ranker::default(),
            palette: Palette::default(),
            snapshot: Snapshot::default(),
            shown: Vec::new(),
            generation: 0,
            startup: Startup::AwaitFirstPaint,
            visible: false,
            had_focus: false,
            place_again: None,
            focus_attempts: 0,
            pending_tab: None,
            exiting: false,
        }
    }

    // ----- lifecycle --------------------------------------------------------

    fn open(&mut self, ctx: &egui::Context) {
        if self.visible {
            self.palette.request_focus();
            win::show_and_focus(self.hwnd);
            return;
        }
        // Pick the monitor before we take the foreground ourselves.
        let (work, scale) = win::active_monitor();

        self.generation += 1;
        self.snapshot = Snapshot::new(self.generation, win::enumerate());
        self.palette.reset();
        let explorers = self.snapshot.explorer_windows();
        if !explorers.is_empty()
            && let Some(w) = &self.worker
        {
            w.request(Request::Scan {
                generation: self.generation,
                windows: explorers,
            });
            self.palette.busy = Some("Reading Explorer tabs…".into());
        }
        self.refresh_results();

        win::place(self.hwnd, work, scale, styles::POPUP_SIZE);
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        self.visible = true;
        self.had_focus = false;
        self.pending_tab = None;
        self.place_again = Some((work, scale));
        self.focus_attempts = 3;
        ctx.request_repaint();
    }

    fn hide(&mut self, ctx: &egui::Context) {
        if self.visible {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        self.visible = false;
        self.pending_tab = None;
        self.palette.busy = None;
        // Results hold window handles; don't keep them around while hidden.
        self.shown.clear();
        self.snapshot = Snapshot::default();
    }

    fn exit(&mut self, ctx: &egui::Context) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        self.hide(ctx);
        self.hotkeys = None; // unregisters the shortcut
        self.tray = None; // removes the icon
        if let Some(mut w) = self.worker.take() {
            w.shutdown();
        }
        if let Some(mut i) = self.instance.take() {
            i.stop();
        }
        self.save_config();
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn toggle_startup(&mut self) {
        let want = !startup::is_enabled();
        if let Err(e) = startup::set_enabled(want, &self.exe) {
            os::warn(format!("Could not change “Start with Windows”: {e}"));
        }
        if let Some(t) = &self.tray {
            t.set_startup_checked(startup::is_enabled());
        }
    }

    fn save_config(&self) {
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            os::log(&format!("Could not save config: {e}"));
        }
    }

    // ----- search -----------------------------------------------------------

    fn refresh_results(&mut self) {
        let query = self.palette.query.clone();
        self.palette.status = None;
        if paths::looks_like_path(&query) {
            self.shown = match paths::resolve(&query, &self.home) {
                Some(Err(msg)) => {
                    self.palette.status = Some(Status::Error(msg));
                    Vec::new()
                }
                // Network paths are checked only on Enter: probing an
                // unreachable server on every keystroke could stall the UI.
                Some(Ok(p)) if p.to_string_lossy().starts_with("\\\\") => {
                    let mut item = folder_item(p);
                    item.subtitle = "Network folder · checked when opened".into();
                    vec![item]
                }
                _ => match paths::check(&query, &self.home) {
                    Some(paths::PathCheck::Directory(p)) => vec![folder_item(p)],
                    Some(other) => {
                        self.palette.status = other.message().map(Status::Error);
                        Vec::new()
                    }
                    None => Vec::new(),
                },
            };
            return;
        }
        let limit = self.config.max_results;
        let order = self
            .ranker
            .rank(&self.snapshot.items, &query, &self.config.recent, limit);
        self.shown = order.into_iter().map(|i| self.snapshot.items[i].clone()).collect();
    }

    fn activate(&mut self, ctx: &egui::Context, index: usize) {
        let Some(item) = self.shown.get(index).cloned() else {
            return;
        };
        match &item.target {
            Target::Window { hwnd } => match win::activate(*hwnd) {
                Ok(()) => {
                    self.remember(&item);
                    self.hide(ctx);
                }
                Err(e) => self.fail(*hwnd, e),
            },
            Target::ExplorerTab {
                explorer_hwnd,
                token,
                name,
                index,
            } => {
                if !win::is_window(*explorer_hwnd) {
                    self.fail(*explorer_hwnd, "That Explorer window has closed.".into());
                    return;
                }
                if let Some(w) = &self.worker {
                    w.request(Request::Select {
                        explorer_hwnd: *explorer_hwnd,
                        token: *token,
                        name: name.clone(),
                        index: *index,
                    });
                    self.pending_tab = Some(*explorer_hwnd);
                    self.palette.busy = Some("Switching tab…".into());
                    self.remember(&item);
                }
            }
            Target::OpenFolder { path } => match actions::open_folder(path) {
                Ok(()) => {
                    self.remember(&item);
                    self.hide(ctx);
                }
                Err(e) => self.palette.status = Some(Status::Error(e)),
            },
        }
    }

    /// Show an error; if the target window is gone, refresh rather than crash.
    fn fail(&mut self, hwnd: isize, msg: String) {
        if !win::is_window(hwnd) {
            self.snapshot.remove_window(hwnd);
            self.refresh_results();
        }
        self.palette.status = Some(Status::Error(msg));
    }

    fn remember(&mut self, item: &SearchItem) {
        self.config.push_recent(&item.recent_key);
        self.save_config();
    }

    fn handle(&mut self, ctx: &egui::Context, cmd: AppCommand) {
        match cmd {
            AppCommand::Toggle => {
                if self.visible {
                    self.hide(ctx);
                } else {
                    self.open(ctx);
                }
            }
            AppCommand::Open => self.open(ctx),
            AppCommand::ToggleStartup => self.toggle_startup(),
            AppCommand::Exit => self.exit(ctx),
            AppCommand::Tabs { generation, tabs } => {
                // Late answers (palette closed or reopened since) are ignored.
                if self.visible && self.snapshot.merge_tabs(generation, &tabs) {
                    if self.pending_tab.is_none() {
                        self.palette.busy = None;
                    }
                    self.refresh_results();
                }
            }
            AppCommand::TabSelected { explorer_hwnd, result } => {
                if self.pending_tab != Some(explorer_hwnd) {
                    return;
                }
                self.pending_tab = None;
                self.palette.busy = None;
                match result.and_then(|()| win::activate(explorer_hwnd)) {
                    Ok(()) => self.hide(ctx),
                    Err(e) => self.fail(explorer_hwnd, e),
                }
            }
        }
    }
}

impl eframe::App for QuickSwitchApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.startup == Startup::Painted {
            // Skip the hide if the user already opened the popup meanwhile.
            if !self.visible {
                ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            }
            self.startup = Startup::Done;
        }

        // Alt+F4 or a system close on the popup hides it; only Exit quits.
        if ctx.input(|i| i.viewport().close_requested()) && !self.exiting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide(ctx);
        }

        while let Ok(cmd) = self.rx.try_recv() {
            self.handle(ctx, cmd);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.startup == Startup::AwaitFirstPaint {
            self.startup = Startup::Painted;
            ctx.request_repaint();
            return;
        }
        if !self.visible || self.exiting {
            return;
        }

        if let Some((work, scale)) = self.place_again.take() {
            win::place(self.hwnd, work, scale, styles::POPUP_SIZE);
        }
        if self.focus_attempts > 0 {
            if win::is_foreground(self.hwnd) {
                self.focus_attempts = 0;
            } else {
                self.focus_attempts -= 1;
                win::show_and_focus(self.hwnd);
                ctx.request_repaint();
            }
        }

        // Hide when focus moves elsewhere (click-away), but only after the
        // popup actually had focus once since it was shown.
        match ctx.input(|i| i.viewport().focused) {
            Some(true) => self.had_focus = true,
            Some(false) if self.had_focus && self.pending_tab.is_none() => {
                self.hide(&ctx);
                return;
            }
            _ => {}
        }

        match self.palette.show(ui, &self.shown) {
            Some(PaletteAction::Hide) => self.hide(&ctx),
            Some(PaletteAction::QueryChanged) => self.refresh_results(),
            Some(PaletteAction::Activate(i)) => self.activate(&ctx, i),
            None => {}
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        styles::BG.to_normalized_gamma_f32()
    }
}
