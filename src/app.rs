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

use std::path::{Path, PathBuf};
use std::rc::Rc;

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
use crate::search::{SearchItem, Shown, Snapshot, Target, display_path, folder_item};
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
    shown: Vec<Shown>,
    /// The query is a folder path (suggestions instead of search).
    path_mode: bool,
    /// Last folder listed for suggestions (reused while only the partial name changes).
    dir_cache: Option<(PathBuf, Rc<Vec<paths::DirEntry>>)>,
    /// Remembered folders changed since the last save.
    config_dirty: bool,
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
            path_mode: false,
            dir_cache: None,
            config_dirty: false,
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
        self.snapshot.set_history(self.history_items());
        self.dir_cache = None;
        self.palette.reset();
        if !self.snapshot.explorer_windows().is_empty()
            && let Some(w) = &self.worker
        {
            w.request(Request::Scan {
                generation: self.generation,
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
        self.dir_cache = None;
        if self.config_dirty {
            self.save_config();
        }
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

    fn save_config(&mut self) {
        self.config_dirty = false;
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            os::log(&format!("Could not save config: {e}"));
        }
    }

    /// Remembered folders as search items.
    fn history_items(&self) -> Vec<SearchItem> {
        self.config
            .folders
            .iter()
            .map(|f| folder_item(PathBuf::from(f), &self.home))
            .collect()
    }

    // ----- search -----------------------------------------------------------

    fn refresh_results(&mut self) {
        let query = self.palette.query.clone();
        self.palette.status = None;
        self.path_mode = paths::looks_like_path(&query);
        if self.path_mode {
            self.shown = self.folder_suggestions(&query);
            return;
        }
        let limit = self.config.max_results;
        // Remembered folders are searchable, but an empty query lists only
        // what is open right now.
        let pool = if query.trim().is_empty() {
            self.snapshot.open_items()
        } else {
            &self.snapshot.items[..]
        };
        let hits = self.ranker.rank(pool, &query, &self.config.recent, limit);
        self.shown = hits
            .into_iter()
            .map(|h| Shown {
                item: pool[h.index].clone(),
                title_hl: h.title_hl,
                subtitle_hl: h.subtitle_hl,
                completion: None,
            })
            .collect();
    }

    /// Folder input: the subfolders of the typed folder that match the last,
    /// partly typed name, like a wildcard `ls`.
    fn folder_suggestions(&mut self, query: &str) -> Vec<Shown> {
        let completion = match paths::completion(query, &self.home) {
            None => return Vec::new(),
            Some(Err(e)) if e == paths::UNC_NOT_LISTED => {
                // Network paths are not listed or checked while typing: an
                // unreachable server could stall the popup.
                return match paths::resolve(query, &self.home) {
                    Some(Ok(p)) => {
                        let mut item = self.folder_result(p);
                        item.subtitle.push_str(" · checked when opened");
                        vec![Shown::plain(item)]
                    }
                    Some(Err(msg)) => {
                        self.palette.status = Some(Status::Error(msg));
                        Vec::new()
                    }
                    None => Vec::new(),
                };
            }
            Some(Err(msg)) => {
                self.palette.status = Some(Status::Error(msg));
                return Vec::new();
            }
            Some(Ok(c)) => c,
        };
        let entries = match self.list_dir(&completion.dir) {
            Ok(e) => e,
            Err(_) => {
                self.palette.status = Some(Status::Error(format!(
                    "Folder not found: {}",
                    display_path(&completion.dir, &self.home)
                )));
                return Vec::new();
            }
        };

        let limit = self.config.max_results;
        let mut out = Vec::new();
        if completion.partial.is_empty() {
            // Right after a separator: the folder itself first.
            out.push(Shown::plain(self.folder_result(completion.dir.clone())));
        }
        let partial_lower = completion.partial.to_lowercase();
        let mut matches: Vec<(u32, usize, Vec<usize>)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                !e.hidden || (!partial_lower.is_empty() && e.name.to_lowercase().starts_with(&partial_lower))
            })
            .filter_map(|(i, e)| {
                self.ranker
                    .match_name(&e.name, &completion.partial)
                    .map(|(score, hl)| (score, i, hl))
            })
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, i, hl) in matches.into_iter().take(limit.saturating_sub(out.len())) {
            let name = &entries[i].name;
            let item = self.folder_result(completion.dir.join(name));
            // A tab's title is the folder's display name, which is normally the
            // same as the folder name; only then do the highlights line up.
            let title_hl = if item.title == *name { hl } else { Vec::new() };
            out.push(Shown {
                item,
                title_hl,
                subtitle_hl: Vec::new(),
                completion: Some(completion.complete(name)),
            });
        }
        if out.is_empty() {
            self.palette.status = Some(Status::Error(format!(
                "No folder matching “{}” in {}",
                completion.partial,
                display_path(&completion.dir, &self.home)
            )));
        }
        out
    }

    /// Subfolders of `dir`, cached while the user types in the same folder.
    fn list_dir(&mut self, dir: &Path) -> std::io::Result<Rc<Vec<paths::DirEntry>>> {
        if let Some((cached, entries)) = &self.dir_cache
            && cached == dir
        {
            return Ok(entries.clone());
        }
        let entries = Rc::new(paths::list_dirs(dir)?);
        self.dir_cache = Some((dir.to_path_buf(), entries.clone()));
        Ok(entries)
    }

    /// The row for a folder: its open Explorer tab if there is one (Enter
    /// switches to it), otherwise a folder to open.
    fn folder_result(&self, path: PathBuf) -> SearchItem {
        match self.snapshot.tab_for(&path) {
            Some(tab) => tab.clone(),
            None => folder_item(path, &self.home),
        }
    }

    fn activate(&mut self, ctx: &egui::Context, index: usize) {
        let Some(item) = self.shown.get(index).map(|s| s.item.clone()) else {
            return;
        };
        self.activate_item(ctx, item);
    }

    fn activate_item(&mut self, ctx: &egui::Context, item: SearchItem) {
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
                tab_hwnd,
                name,
                nth,
                ..
            } => {
                let explorer = *explorer_hwnd;
                if !win::is_window(explorer) {
                    self.fail(explorer, "That Explorer window has closed.".into());
                    return;
                }
                self.remember(&item);
                // Already the shown tab: just bring its window forward.
                if win::active_explorer_tab(explorer) == Some(*tab_hwnd) {
                    match win::activate(explorer) {
                        Ok(()) => self.hide(ctx),
                        Err(e) => self.fail(explorer, e),
                    }
                    return;
                }
                // A minimized window has no tab strip to click: restore it first.
                if win::is_minimized(explorer) {
                    let _ = win::activate(explorer);
                }
                if let Some(w) = &self.worker {
                    w.request(Request::Select {
                        explorer_hwnd: explorer,
                        tab_hwnd: *tab_hwnd,
                        name: name.clone(),
                        nth: *nth,
                    });
                    self.pending_tab = Some(explorer);
                    self.palette.busy = Some("Switching tab…".into());
                }
            }
            Target::OpenFolder { path } => {
                // Already open in a tab (the scan may have finished after the
                // row was built): switch to it instead of opening another.
                if let Some(tab) = self.snapshot.tab_for(path).cloned() {
                    self.activate_item(ctx, tab);
                    return;
                }
                match actions::open_folder(path) {
                    Ok(()) => {
                        self.remember(&item);
                        self.hide(ctx);
                    }
                    Err(e) => {
                        if !path.is_dir() {
                            self.config.forget_folder(path);
                            self.config_dirty = true;
                            self.snapshot.remove_folder(path);
                            self.refresh_results();
                        }
                        self.palette.status = Some(Status::Error(e));
                    }
                }
            }
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
        if let Some(folder) = item.folder() {
            self.config.push_folder(folder);
        }
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
                if self.visible && self.snapshot.merge_tabs(generation, &tabs, &self.home) {
                    if self.config.note_folders(tabs.iter().filter_map(|t| t.path.as_deref())) {
                        self.config_dirty = true;
                    }
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
                let bring_forward = |()| {
                    if win::is_foreground(explorer_hwnd) {
                        Ok(())
                    } else {
                        win::activate(explorer_hwnd)
                    }
                };
                match result.and_then(bring_forward) {
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

        match self.palette.show(ui, &self.shown, self.path_mode) {
            Some(PaletteAction::Hide) => self.hide(&ctx),
            Some(PaletteAction::QueryChanged | PaletteAction::Completed) => self.refresh_results(),
            Some(PaletteAction::Activate(i)) => self.activate(&ctx, i),
            None => {}
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        styles::BG.to_normalized_gamma_f32()
    }
}
