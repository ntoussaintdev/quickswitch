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

use crate::actions::web::Engine;
use crate::actions::{self, paths};
use crate::command::{AppCommand, CommandSender};
use crate::config::Config;
use crate::os::hotkey::Hotkeys;
use crate::os::instance::{PrimaryInstance, Signal};
use crate::os::tray::Tray;
use crate::os::{self, windows as win};
use crate::search::browsers::Engine as TabEngine;
use crate::search::matcher::Ranker;
use crate::search::worker::{Request, TabWorker};
use crate::search::{
    Kind, Qualifier, SearchItem, Shown, Snapshot, Target, WebMode, display_path, folder_item, parse_qualifier, web_item,
};
use crate::startup;
use crate::ui::palette::{Palette, PaletteAction, Status};
use crate::ui::settings::{Settings, SettingsAction};
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
    /// The settings view, shown instead of the palette while set.
    settings: Option<Settings>,
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
    /// Explorer or browser window we asked the worker to switch a tab in.
    pending_tab: Option<isize>,
    /// The browser tab row being switched to (dropped if the tab is gone).
    pending_browser: Option<SearchItem>,
    /// Waiting for the worker's Explorer tabs / browser tabs.
    explorer_pending: bool,
    browsers_pending: bool,
    /// Footer text while browser / Terminal tabs are read.
    browsers_label: &'static str,
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
            config.browser_tabs,
            config.web_search,
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
                 Pick another shortcut in \u{201c}Settings\u{2026}\u{201d} in the tray menu.",
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
            settings: None,
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
            pending_browser: None,
            explorer_pending: false,
            browsers_pending: false,
            browsers_label: "Reading browser tabs…",
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
        self.pending_tab = None;
        self.pending_browser = None;
        self.explorer_pending = false;
        self.browsers_pending = false;
        if let Some(w) = &self.worker {
            if !self.snapshot.explorer_windows().is_empty() {
                w.request(Request::Scan {
                    generation: self.generation,
                });
                self.explorer_pending = true;
            }
            let config = &self.config;
            let browsers: Vec<_> = self
                .snapshot
                .browser_windows()
                .into_iter()
                .filter(|b| {
                    if b.spec.engine == TabEngine::Terminal {
                        config.terminal_tabs
                    } else {
                        config.browser_tabs
                    }
                })
                .collect();
            if !browsers.is_empty() {
                self.browsers_label = if browsers.iter().all(|b| b.spec.engine == TabEngine::Terminal) {
                    "Reading Terminal tabs…"
                } else {
                    "Reading browser tabs…"
                };
                w.request(Request::ScanBrowsers {
                    generation: self.generation,
                    windows: browsers,
                });
                self.browsers_pending = true;
            }
        }
        self.update_busy();
        self.refresh_results();

        win::place(self.hwnd, work, scale, styles::POPUP_SIZE);
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        self.visible = true;
        self.had_focus = false;
        self.place_again = Some((work, scale));
        self.focus_attempts = 3;
        ctx.request_repaint();
    }

    fn hide(&mut self, ctx: &egui::Context) {
        if self.visible {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        self.visible = false;
        self.settings = None;
        self.pending_tab = None;
        self.pending_browser = None;
        self.explorer_pending = false;
        self.browsers_pending = false;
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

    /// Tray "Settings…": show the popup with the settings view.
    fn open_settings(&mut self, ctx: &egui::Context) {
        self.open(ctx);
        self.settings = Some(Settings::new(
            &self.config.hotkey,
            &self.config.search_engine,
            &self.config.custom_search_url,
        ));
    }

    /// Use the picked search engine and return to the search.
    fn save_web(&mut self, engine: String, custom_url: String) {
        self.config.search_engine = engine;
        self.config.custom_search_url = custom_url;
        self.save_config();
        self.settings = None;
        self.palette.request_focus();
        self.palette.status = Some(Status::Info(format!("Web search: {}", self.engine().name)));
    }

    /// Switch to the picked shortcut. On failure the old one stays active and
    /// the settings view shows why.
    fn save_shortcut(&mut self, text: String) {
        let result = match self.hotkeys.as_mut() {
            Some(h) => h.replace(&text),
            None => Err("Global shortcuts are unavailable on this system.".into()),
        };
        match result {
            Ok(()) => {
                if let Some(t) = &self.tray {
                    t.set_hotkey_label(&text);
                }
                self.config.hotkey = text.clone();
                self.save_config();
                self.settings = None;
                self.palette.request_focus();
                self.palette.status = Some(Status::Info(format!("Shortcut changed to {text}")));
            }
            Err(e) => {
                if let Some(settings) = self.settings.as_mut() {
                    settings.shortcut.status = Some(Status::Error(e));
                }
            }
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
        self.palette.empty_hint = None;
        self.path_mode = paths::looks_like_path(&query);
        if self.path_mode {
            self.shown = self.folder_suggestions(&query);
            return;
        }
        let limit = self.config.max_results;
        let qualifier = parse_qualifier(&query);

        // A type filter ("browser: mail") searches every result of its kinds,
        // the ones an empty query hides included. Without one, remembered
        // folders and background tabs are searchable, but an empty query
        // lists only open windows and tabs, one row per browser window.
        let (pool, text, web): (Vec<SearchItem>, &str, WebMode) = match qualifier {
            Some((q, rest)) => {
                self.palette.status = Some(Status::Info(match q.web {
                    WebMode::Gpt => "Enter asks ChatGPT in your browser".to_string(),
                    _ => format!("Only {}", q.label),
                }));
                let pool = self
                    .snapshot
                    .items
                    .iter()
                    .filter(|i| q.kinds.contains(&i.kind()))
                    .cloned()
                    .collect();
                (pool, rest, q.web)
            }
            None if query.trim().is_empty() => (self.snapshot.listed_items().to_vec(), "", WebMode::None),
            None => (self.snapshot.items.clone(), query.as_str(), WebMode::None),
        };

        // The web row: first for web:/browser:, the only row for gpt:, and
        // without a filter (when switched on) first only if nothing else
        // matches, otherwise last.
        let engine = self.engine();
        let row = match web {
            WebMode::Gpt => web_item(text, &engine, true),
            WebMode::First => web_item(text, &engine, false),
            WebMode::None if qualifier.is_none() && self.config.web_search => web_item(text, &engine, false),
            WebMode::None => None,
        };
        let room = if row.is_some() {
            limit.saturating_sub(1).max(1)
        } else {
            limit
        };
        let hits = if web == WebMode::Gpt {
            Vec::new()
        } else {
            self.ranker.rank(&pool, text, &self.config.recent, room)
        };
        let mut shown: Vec<Shown> = hits
            .into_iter()
            .map(|h| Shown {
                item: pool[h.index].clone(),
                title_hl: h.title_hl,
                subtitle_hl: h.subtitle_hl,
                completion: None,
            })
            .collect();
        if let Some(row) = row {
            if web != WebMode::None || shown.is_empty() {
                shown.insert(0, Shown::plain(row));
            } else {
                shown.push(Shown::plain(row));
            }
        }
        if shown.is_empty()
            && let Some((q, _)) = qualifier
        {
            self.palette.empty_hint = Some(self.empty_hint(q, text));
        }
        self.shown = shown;
    }

    /// What an empty result list says under a type filter: a prompt when
    /// nothing is typed yet, otherwise that nothing of that kind matches.
    fn empty_hint(&self, q: Qualifier, text: &str) -> String {
        let text = text.trim();
        let mut hint = if text.is_empty() {
            q.empty.to_string()
        } else {
            format!("No {} match \u{201c}{text}\u{201d}", q.label)
        };
        let reading = (self.explorer_pending && q.kinds.contains(&Kind::ExplorerTab))
            || (self.browsers_pending && (q.kinds.contains(&Kind::BrowserTab) || q.kinds.contains(&Kind::TerminalTab)));
        if reading {
            hint.push_str(" (still reading tabs\u{2026})");
        }
        hint
    }

    /// The search engine chosen in Settings.
    fn engine(&self) -> Engine {
        Engine::from_settings(&self.config.search_engine, &self.config.custom_search_url)
    }

    /// Ctrl+Enter: search the web for the typed text (after any filter word;
    /// `gpt:` asks ChatGPT), whatever row is selected.
    fn web_search_now(&mut self, ctx: &egui::Context) {
        let query = self.palette.query.clone();
        let (text, gpt) = match parse_qualifier(&query) {
            Some((q, rest)) => (rest.to_string(), q.web == WebMode::Gpt),
            None => (query.clone(), false),
        };
        if let Some(item) = web_item(&text, &self.engine(), gpt) {
            self.activate_item(ctx, item);
        }
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
                    self.update_busy();
                }
            }
            Target::BrowserTab {
                window_hwnd,
                key,
                name,
                nth,
                ..
            } => {
                let window = *window_hwnd;
                if !win::is_window(window) {
                    self.fail(window, "That window has closed.".into());
                    return;
                }
                self.remember(&item);
                // A minimized window has no tab strip to read: restore it first.
                if win::is_minimized(window) {
                    let _ = win::activate(window);
                }
                if let Some(w) = &self.worker {
                    w.request(Request::SelectBrowserTab {
                        window_hwnd: window,
                        key: *key,
                        name: name.clone(),
                        nth: *nth,
                    });
                    self.pending_tab = Some(window);
                    self.pending_browser = Some(item.clone());
                    self.update_busy();
                }
            }
            Target::Web { url } => match actions::open_url(url) {
                Ok(()) => self.hide(ctx),
                Err(e) => self.palette.status = Some(Status::Error(e)),
            },
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

    /// Footer progress text for whatever the worker is still doing.
    fn update_busy(&mut self) {
        self.palette.busy = if self.pending_tab.is_some() {
            Some("Switching tab…".into())
        } else if self.explorer_pending {
            Some("Reading Explorer tabs…".into())
        } else if self.browsers_pending {
            Some(self.browsers_label.into())
        } else {
            None
        };
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
            AppCommand::OpenSettings => self.open_settings(ctx),
            AppCommand::ToggleWebSearch => {
                self.config.web_search = !self.config.web_search;
                self.save_config();
                if let Some(t) = &self.tray {
                    t.set_web_search_checked(self.config.web_search);
                }
            }
            AppCommand::ToggleStartup => self.toggle_startup(),
            AppCommand::ToggleBrowserTabs => {
                self.config.browser_tabs = !self.config.browser_tabs;
                self.save_config();
                if let Some(t) = &self.tray {
                    t.set_browser_tabs_checked(self.config.browser_tabs);
                }
            }
            AppCommand::Exit => self.exit(ctx),
            AppCommand::Tabs { generation, tabs } => {
                // Late answers (palette closed or reopened since) are ignored.
                if self.visible && self.snapshot.merge_tabs(generation, &tabs, &self.home) {
                    if self.config.note_folders(tabs.iter().filter_map(|t| t.path.as_deref())) {
                        self.config_dirty = true;
                    }
                    self.explorer_pending = false;
                    self.update_busy();
                    self.refresh_results();
                }
            }
            AppCommand::BrowserTabs { generation, tabs } => {
                if self.visible && self.snapshot.merge_browser_tabs(generation, &tabs) {
                    self.browsers_pending = false;
                    self.update_busy();
                    self.refresh_results();
                }
            }
            AppCommand::BrowserTabSelected {
                window_hwnd,
                result,
                gone,
            } => {
                if self.pending_tab != Some(window_hwnd) {
                    return;
                }
                self.pending_tab = None;
                let item = self.pending_browser.take();
                self.update_busy();
                let result = result.and_then(|()| {
                    if win::is_foreground(window_hwnd) {
                        Ok(())
                    } else {
                        win::activate(window_hwnd)
                    }
                });
                match result {
                    Ok(()) => self.hide(ctx),
                    Err(e) => {
                        if gone && let Some(item) = item {
                            self.snapshot.remove_item(&item);
                            self.refresh_results();
                        }
                        self.fail(window_hwnd, e);
                    }
                }
            }
            AppCommand::TabSelected { explorer_hwnd, result } => {
                if self.pending_tab != Some(explorer_hwnd) {
                    return;
                }
                self.pending_tab = None;
                self.update_busy();
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

        if let Some(settings) = self.settings.as_mut() {
            match settings.show(ui) {
                Some(SettingsAction::SaveShortcut(text)) => self.save_shortcut(text),
                Some(SettingsAction::SaveWeb { engine, custom_url }) => self.save_web(engine, custom_url),
                Some(SettingsAction::Cancel) => self.hide(&ctx),
                None => {}
            }
            return;
        }

        match self.palette.show(ui, &self.shown, self.path_mode) {
            Some(PaletteAction::Hide) => self.hide(&ctx),
            Some(PaletteAction::QueryChanged | PaletteAction::Completed) => self.refresh_results(),
            Some(PaletteAction::Activate(i)) => self.activate(&ctx, i),
            Some(PaletteAction::WebSearch) => self.web_search_now(&ctx),
            None => {}
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        styles::BG.to_normalized_gamma_f32()
    }
}
