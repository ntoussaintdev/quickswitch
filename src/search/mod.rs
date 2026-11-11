//! Search model: plain result records, the per-open snapshot and ranking.
//!
//! Nothing here holds COM objects or long-lived native handles. A window
//! handle is stored as a plain `isize` and re-validated before use.

pub mod browser_tabs;
pub mod browsers;
pub mod explorer_tabs;
pub mod matcher;
pub mod uia;
pub mod worker;

use std::path::{Path, PathBuf};

use browsers::{BrowserSpec, BrowserWindow};

use crate::actions::web::{self, Engine};

/// What happens when a result is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Window {
        hwnd: isize,
    },
    /// One Explorer tab. `tab_hwnd` is the tab's own `ShellTabWindowClass`
    /// window, which identifies it for as long as it is open. `name` and `nth`
    /// (its rank among same-named tabs of that window) help the worker find
    /// the matching UI Automation tab item.
    ExplorerTab {
        explorer_hwnd: isize,
        tab_hwnd: isize,
        name: String,
        nth: usize,
        path: Option<PathBuf>,
    },
    /// One browser or Windows Terminal tab. `key` names the tab element the
    /// worker kept from its latest scan; `name` (the raw accessible name) and
    /// `nth` (its rank among same-named tabs of that window) find it again if
    /// that element is gone.
    BrowserTab {
        window_hwnd: isize,
        key: u64,
        name: String,
        nth: usize,
        terminal: bool,
    },
    OpenFolder {
        path: PathBuf,
    },
    /// Open this address in the default browser (web search or website).
    Web {
        url: String,
    },
}

/// Kind label shown on each row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Window,
    ExplorerTab,
    BrowserTab,
    TerminalTab,
    Folder,
    Web,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Window => "Window",
            Kind::ExplorerTab => "Explorer tab",
            Kind::BrowserTab => "Browser tab",
            Kind::TerminalTab => "Terminal tab",
            Kind::Folder => "Folder",
            Kind::Web => "Web",
        }
    }
}

/// Which text a query is matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOn {
    /// Words of the title and subtitle (windows).
    Text,
    /// Only the folder path in the subtitle (tabs and folders). The title is
    /// the path's last segment and is highlighted along with it.
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchItem {
    pub title: String,
    pub subtitle: String,
    pub target: Target,
    /// Stable-ish identity used for recency ordering (never a handle).
    pub recent_key: String,
    pub match_on: MatchOn,
}

impl SearchItem {
    pub fn kind(&self) -> Kind {
        match self.target {
            Target::Window { .. } => Kind::Window,
            Target::ExplorerTab { .. } => Kind::ExplorerTab,
            Target::BrowserTab { terminal: false, .. } => Kind::BrowserTab,
            Target::BrowserTab { terminal: true, .. } => Kind::TerminalTab,
            Target::OpenFolder { .. } => Kind::Folder,
            Target::Web { .. } => Kind::Web,
        }
    }

    /// The folder this result stands for, if any.
    pub fn folder(&self) -> Option<&Path> {
        match &self.target {
            Target::ExplorerTab { path, .. } => path.as_deref(),
            Target::OpenFolder { path } => Some(path),
            Target::Window { .. } | Target::BrowserTab { .. } | Target::Web { .. } => None,
        }
    }
}

/// A result on screen: the item plus highlighted character indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub item: SearchItem,
    pub title_hl: Vec<usize>,
    pub subtitle_hl: Vec<usize>,
    /// Query text that Tab completes to (folder suggestions only).
    pub completion: Option<String>,
}

impl Shown {
    pub fn plain(item: SearchItem) -> Shown {
        Shown {
            item,
            title_hl: Vec::new(),
            subtitle_hl: Vec::new(),
            completion: None,
        }
    }
}

/// A top-level window, as captured when the palette opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    /// Executable name without extension, e.g. `Code` or `explorer`.
    pub process: String,
    pub class_name: String,
}

impl WindowInfo {
    pub fn is_explorer(&self) -> bool {
        self.class_name == "CabinetWClass"
    }

    pub fn browser(&self) -> Option<&'static BrowserSpec> {
        browsers::browser_of(&self.class_name, &self.process)
    }
}

/// One Explorer tab reported by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    pub explorer_hwnd: isize,
    pub tab_hwnd: isize,
    /// Display name, as on the tab strip.
    pub name: String,
    /// File-system folder; `None` for virtual locations (Home, This PC, ...).
    pub path: Option<PathBuf>,
    /// Rank among tabs of the same window with the same name.
    pub nth: usize,
    /// The tab currently shown in its window.
    pub active: bool,
    /// Position in the window's tab Z-order (0 = most recently shown).
    pub z: usize,
}

/// One browser or Windows Terminal tab reported by the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTabInfo {
    pub window_hwnd: isize,
    /// Worker's handle for the tab element (valid until its next scan).
    pub key: u64,
    /// Accessible name as the browser reports it.
    pub raw_name: String,
    /// Title as shown on the tab.
    pub title: String,
    /// Grey text: browser name, plus the profile when known ("Edge · Work");
    /// for Terminal, "Terminal" plus the tab number when titles repeat.
    pub browser: String,
    /// Rank among tabs of the same window with the same raw name.
    pub nth: usize,
    /// The tab the window is showing.
    pub selected: bool,
    /// A Windows Terminal tab rather than a browser tab.
    pub terminal: bool,
}

/// Results for one palette opening, in three ranges:
/// 1. listed: open windows, Explorer tabs and each browser window's shown tab
///    (what an empty query lists);
/// 2. searched only: the other browser tabs;
/// 3. remembered folders.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Incremented on every open; worker answers for older generations are ignored.
    pub generation: u64,
    /// All results, ranges as above.
    pub items: Vec<SearchItem>,
    listed_len: usize,
    open_len: usize,
    windows: Vec<WindowInfo>,
    tabs_merged: bool,
    browser_tabs_merged: bool,
}

/// Which range of `Snapshot::items` an item is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    Listed,
    SearchedOnly,
    History,
}

impl Snapshot {
    pub fn new(generation: u64, windows: Vec<WindowInfo>) -> Self {
        let items: Vec<SearchItem> = windows.iter().map(window_item).collect();
        Snapshot {
            generation,
            listed_len: items.len(),
            open_len: items.len(),
            items,
            windows,
            tabs_merged: false,
            browser_tabs_merged: false,
        }
    }

    /// Open windows and tabs, including browser tabs that are searched only.
    pub fn open_items(&self) -> &[SearchItem] {
        &self.items[..self.open_len]
    }

    /// What an empty query lists: open windows and tabs, one row per browser window.
    pub fn listed_items(&self) -> &[SearchItem] {
        &self.items[..self.listed_len]
    }

    /// Replace the remembered folders. Folders that are open in a tab are
    /// left out (the tab row stands for them).
    pub fn set_history(&mut self, folders: Vec<SearchItem>) {
        self.items.truncate(self.open_len);
        let open: Vec<String> = self
            .open_items()
            .iter()
            .filter_map(|i| i.folder().map(path_key))
            .collect();
        self.items.extend(
            folders
                .into_iter()
                .filter(|f| f.folder().is_some_and(|p| !open.contains(&path_key(p)))),
        );
    }

    pub fn explorer_windows(&self) -> Vec<isize> {
        self.windows
            .iter()
            .filter(|w| w.is_explorer())
            .map(|w| w.hwnd)
            .collect()
    }

    /// Browser windows to read tabs from, in window order.
    pub fn browser_windows(&self) -> Vec<BrowserWindow> {
        self.windows
            .iter()
            .filter_map(|w| {
                w.browser().map(|spec| BrowserWindow {
                    hwnd: w.hwnd,
                    spec,
                    title: w.title.clone(),
                })
            })
            .collect()
    }

    pub fn tabs_merged(&self) -> bool {
        self.tabs_merged
    }

    pub fn browser_tabs_merged(&self) -> bool {
        self.browser_tabs_merged
    }

    /// Split into (listed, searched only, history).
    fn take_ranges(&mut self) -> (Vec<SearchItem>, Vec<SearchItem>, Vec<SearchItem>) {
        let history = self.items.split_off(self.open_len);
        let searched = self.items.split_off(self.listed_len);
        let listed = std::mem::take(&mut self.items);
        (listed, searched, history)
    }

    fn put_ranges(&mut self, listed: Vec<SearchItem>, searched: Vec<SearchItem>, history: Vec<SearchItem>) {
        self.listed_len = listed.len();
        self.open_len = listed.len() + searched.len();
        self.items = listed;
        self.items.extend(searched);
        self.items.extend(history);
    }

    /// Merge worker tab results. Returns false (and changes nothing) for a
    /// stale generation, e.g. a late answer after the palette was reopened.
    ///
    /// For every Explorer window that reported tabs, its single window row is
    /// replaced by one row per tab: the shown tab first, then the others from
    /// most to least recently shown. Tabs of windows that are not in the
    /// snapshot (other virtual desktops) are ignored.
    pub fn merge_tabs(&mut self, generation: u64, tabs: &[TabInfo], home: &Path) -> bool {
        if generation != self.generation {
            return false;
        }
        let (listed, searched, history) = self.take_ranges();
        let mut new_listed = Vec::with_capacity(listed.len() + tabs.len());
        for item in listed {
            let hwnd = match item.target {
                Target::Window { hwnd } => hwnd,
                _ => {
                    new_listed.push(item);
                    continue;
                }
            };
            let mut window_tabs: Vec<&TabInfo> = tabs.iter().filter(|t| t.explorer_hwnd == hwnd).collect();
            if window_tabs.is_empty() {
                new_listed.push(item);
                continue;
            }
            window_tabs.sort_by_key(|t| (!t.active, t.z));
            new_listed.extend(window_tabs.into_iter().map(|t| tab_item(t, home)));
        }
        self.put_ranges(new_listed, searched, Vec::new());
        self.set_history(history);
        self.tabs_merged = true;
        true
    }

    /// Merge browser tabs. Each browser window's row becomes its shown tab
    /// (listed); its other tabs are searched only, in tab-strip order.
    /// Windows that reported no tabs keep their window row. Returns false
    /// (and changes nothing) for a stale generation.
    pub fn merge_browser_tabs(&mut self, generation: u64, tabs: &[BrowserTabInfo]) -> bool {
        if generation != self.generation {
            return false;
        }
        let (listed, mut searched, history) = self.take_ranges();
        let mut new_listed = Vec::with_capacity(listed.len());
        for item in listed {
            let hwnd = match item.target {
                Target::Window { hwnd } => hwnd,
                _ => {
                    new_listed.push(item);
                    continue;
                }
            };
            let window_tabs: Vec<&BrowserTabInfo> = tabs.iter().filter(|t| t.window_hwnd == hwnd).collect();
            if window_tabs.is_empty() {
                new_listed.push(item);
                continue;
            }
            let shown = window_tabs.iter().position(|t| t.selected).unwrap_or(0);
            for (i, t) in window_tabs.into_iter().enumerate() {
                if i == shown {
                    new_listed.push(browser_tab_item(t));
                } else {
                    searched.push(browser_tab_item(t));
                }
            }
        }
        self.put_ranges(new_listed, searched, history);
        self.browser_tabs_merged = true;
        true
    }

    /// The open tab showing `path`, preferring a tab that is already shown
    /// and the most recently used window.
    pub fn tab_for(&self, path: &Path) -> Option<&SearchItem> {
        let key = path_key(path);
        self.open_items()
            .iter()
            .find(|i| matches!(i.target, Target::ExplorerTab { .. }) && i.folder().is_some_and(|p| path_key(p) == key))
    }

    /// Keep the items for which `keep` is true, keeping the ranges in step.
    fn retain(&mut self, mut keep: impl FnMut(Range, &SearchItem) -> bool) {
        let (listed, searched, history) = self.take_ranges();
        let listed = listed.into_iter().filter(|i| keep(Range::Listed, i)).collect();
        let searched = searched.into_iter().filter(|i| keep(Range::SearchedOnly, i)).collect();
        let history = history.into_iter().filter(|i| keep(Range::History, i)).collect();
        self.put_ranges(listed, searched, history);
    }

    /// Drop every result that belongs to a window that no longer exists.
    pub fn remove_window(&mut self, gone: isize) {
        self.retain(|_, i| match i.target {
            Target::Window { hwnd } => hwnd != gone,
            Target::ExplorerTab { explorer_hwnd, .. } => explorer_hwnd != gone,
            Target::BrowserTab { window_hwnd, .. } => window_hwnd != gone,
            Target::OpenFolder { .. } | Target::Web { .. } => true,
        });
        self.windows.retain(|w| w.hwnd != gone);
    }

    /// Drop one result (e.g. a browser tab that has closed).
    pub fn remove_item(&mut self, item: &SearchItem) {
        let mut removed = false;
        self.retain(|_, i| {
            if !removed && i == item {
                removed = true;
                return false;
            }
            true
        });
    }

    /// Drop a remembered folder (e.g. it no longer exists).
    pub fn remove_folder(&mut self, path: &Path) {
        let key = path_key(path);
        self.retain(|range, i| range != Range::History || i.folder().is_none_or(|p| path_key(p) != key));
    }
}

/// Case-insensitive identity of a folder path.
pub fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_lowercase()
}

/// `C:\Users\me\Projects\x` -> `~/Projects/x`; other paths keep their drive or
/// server but use forward slashes, matching how folders are typed.
pub fn display_path(path: &Path, home: &Path) -> String {
    let p = path.to_string_lossy();
    let h = home.to_string_lossy();
    let h = h.trim_end_matches(['\\', '/']);
    let lower = p.to_lowercase();
    let shown = if !h.is_empty() && lower.starts_with(&h.to_lowercase()) {
        let rest = &p[h.len()..];
        if rest.is_empty() {
            Some("~".to_string())
        } else if rest.starts_with(['\\', '/']) {
            Some(format!("~{rest}"))
        } else {
            None
        }
    } else {
        None
    };
    shown.unwrap_or_else(|| p.into_owned()).replace('\\', "/")
}

/// Last segment of a path, or the whole path for a root like `C:\`.
pub fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn window_item(w: &WindowInfo) -> SearchItem {
    let app = if w.is_explorer() {
        "File Explorer".to_string()
    } else {
        w.process.clone()
    };
    SearchItem {
        title: w.title.clone(),
        subtitle: app.clone(),
        target: Target::Window { hwnd: w.hwnd },
        recent_key: format!("win:{}|{}", app.to_lowercase(), w.title.to_lowercase()),
        match_on: MatchOn::Text,
    }
}

pub fn tab_item(t: &TabInfo, home: &Path) -> SearchItem {
    let (subtitle, recent_key, match_on) = match &t.path {
        Some(p) => (display_path(p, home), format!("dir:{}", path_key(p)), MatchOn::Path),
        None => (
            "File Explorer".to_string(),
            format!("tab:{}", t.name.to_lowercase()),
            MatchOn::Text,
        ),
    };
    SearchItem {
        title: t.name.clone(),
        subtitle,
        target: Target::ExplorerTab {
            explorer_hwnd: t.explorer_hwnd,
            tab_hwnd: t.tab_hwnd,
            name: t.name.clone(),
            nth: t.nth,
            path: t.path.clone(),
        },
        recent_key,
        match_on,
    }
}

/// Item for one browser or Terminal tab. The recent key holds a hash of the title, not
/// the title, so page titles are not written to `config.toml`.
pub fn browser_tab_item(t: &BrowserTabInfo) -> SearchItem {
    SearchItem {
        title: t.title.clone(),
        subtitle: t.browser.clone(),
        target: Target::BrowserTab {
            window_hwnd: t.window_hwnd,
            key: t.key,
            name: t.raw_name.clone(),
            nth: t.nth,
            terminal: t.terminal,
        },
        recent_key: format!(
            "{}:{}|{:016x}",
            if t.terminal { "term" } else { "web" },
            t.browser.to_lowercase(),
            browsers::title_hash(&t.title.to_lowercase())
        ),
        match_on: MatchOn::Text,
    }
}

/// Item for a folder that is not open in Explorer.
pub fn folder_item(path: PathBuf, home: &Path) -> SearchItem {
    SearchItem {
        title: folder_name(&path),
        subtitle: display_path(&path, home),
        recent_key: format!("dir:{}", path_key(&path)),
        target: Target::OpenFolder { path },
        match_on: MatchOn::Path,
    }
}

/// Where a filter puts the web row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebMode {
    /// No web row.
    None,
    /// Web search (or "Visit") row first, then the matches (`web:`, `browser:`).
    First,
    /// Only an "Ask ChatGPT" row (`gpt:`).
    Gpt,
}

/// A type filter typed at the start of the query, e.g. `browser: mail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Qualifier {
    /// Kinds of result that pass the filter.
    pub kinds: &'static [Kind],
    /// What is shown, e.g. "browser tabs".
    pub label: &'static str,
    pub web: WebMode,
    /// Shown instead of results when nothing is typed after the filter and
    /// there is nothing of its kinds to list.
    pub empty: &'static str,
}

const ALL_TABS: &[Kind] = &[Kind::ExplorerTab, Kind::BrowserTab, Kind::TerminalTab];
const FOLDERS: &[Kind] = &[Kind::Folder, Kind::ExplorerTab];

/// Filter words. Any prefix of two or more letters works when it names one
/// filter (`br:`, `term:`), and so does a plural (`windows:`). One letter is
/// left alone: `c:` is a drive.
const QUALIFIERS: &[(&str, Qualifier)] = &[
    (
        "window",
        Qualifier {
            kinds: &[Kind::Window],
            label: "windows",
            web: WebMode::None,
            empty: "No windows are open",
        },
    ),
    (
        "explorer",
        Qualifier {
            kinds: &[Kind::ExplorerTab],
            label: "Explorer tabs",
            web: WebMode::None,
            empty: "No Explorer tabs are open",
        },
    ),
    (
        "browser",
        Qualifier {
            kinds: &[Kind::BrowserTab],
            label: "browser tabs",
            web: WebMode::First,
            empty: "Type to search the web, or your browser tabs once they're read",
        },
    ),
    (
        "web",
        Qualifier {
            kinds: &[Kind::BrowserTab],
            label: "browser tabs",
            web: WebMode::First,
            empty: "Type to search the web, or your browser tabs once they're read",
        },
    ),
    (
        "terminal",
        Qualifier {
            kinds: &[Kind::TerminalTab],
            label: "Terminal tabs",
            web: WebMode::None,
            empty: "No Windows Terminal tabs are open",
        },
    ),
    (
        "tab",
        Qualifier {
            kinds: ALL_TABS,
            label: "tabs",
            web: WebMode::None,
            empty: "No Explorer, browser or Terminal tabs are open",
        },
    ),
    (
        "folder",
        Qualifier {
            kinds: FOLDERS,
            label: "folders",
            web: WebMode::None,
            empty: "No folders yet: open some in Explorer or type a path like ~/Documents",
        },
    ),
    (
        "dir",
        Qualifier {
            kinds: FOLDERS,
            label: "folders",
            web: WebMode::None,
            empty: "No folders yet: open some in Explorer or type a path like ~/Documents",
        },
    ),
    (
        "gpt",
        Qualifier {
            kinds: &[],
            label: "ChatGPT",
            web: WebMode::Gpt,
            empty: "Type a question to ask ChatGPT",
        },
    ),
    (
        "chatgpt",
        Qualifier {
            kinds: &[],
            label: "ChatGPT",
            web: WebMode::Gpt,
            empty: "Type a question to ask ChatGPT",
        },
    ),
];

/// Split `browser: mail` into the browser-tab filter and `mail`. `None` when
/// the query doesn't start with a known filter word and a colon.
pub fn parse_qualifier(query: &str) -> Option<(Qualifier, &str)> {
    let (word, rest) = query.trim_start().split_once(':')?;
    if word.len() < 2 || !word.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let word = word.to_ascii_lowercase();
    let mut found: Option<Qualifier> = None;
    for (name, q) in QUALIFIERS {
        let plural = word.strip_suffix('s') == Some(name);
        if name.starts_with(&word) || plural {
            match found {
                Some(f) if f != *q => return None, // ambiguous
                _ => found = Some(*q),
            }
        }
    }
    found.map(|q| (q, rest.trim_start()))
}

/// The web row for `text`: "Visit <site>" when the text is clearly a
/// website, otherwise "<engine action> “text”". `None` for empty text.
/// With `gpt`, always "Ask ChatGPT “text”".
pub fn web_item(text: &str, engine: &Engine, gpt: bool) -> Option<SearchItem> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (title, url) = if gpt {
        let chatgpt = web::chatgpt();
        (
            format!("{} \u{201c}{text}\u{201d}", chatgpt.action),
            chatgpt.url_for(text),
        )
    } else if let Some(url) = web::website_url(text) {
        (format!("Visit {text}"), url)
    } else {
        (
            format!("{} \u{201c}{text}\u{201d}", engine.action),
            engine.url_for(text),
        )
    };
    Some(SearchItem {
        title,
        subtitle: "Ctrl+Enter from any row".into(),
        target: Target::Web { url },
        // Never stored: web rows are not remembered.
        recent_key: String::new(),
        match_on: MatchOn::Text,
    })
}
