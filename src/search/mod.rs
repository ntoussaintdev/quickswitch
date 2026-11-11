//! Search model: plain result records, the per-open snapshot and ranking.
//!
//! Nothing here holds COM objects or long-lived native handles. A window
//! handle is stored as a plain `isize` and re-validated before use.

pub mod explorer_tabs;
pub mod matcher;

use std::path::{Path, PathBuf};

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
    OpenFolder {
        path: PathBuf,
    },
}

/// Kind label shown on each row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Window,
    ExplorerTab,
    Folder,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Window => "Window",
            Kind::ExplorerTab => "Explorer tab",
            Kind::Folder => "Folder",
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
            Target::OpenFolder { .. } => Kind::Folder,
        }
    }

    /// The folder this result stands for, if any.
    pub fn folder(&self) -> Option<&Path> {
        match &self.target {
            Target::ExplorerTab { path, .. } => path.as_deref(),
            Target::OpenFolder { path } => Some(path),
            Target::Window { .. } => None,
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

/// Results for one palette opening: open windows first (Explorer windows
/// replaced by their tabs once the worker answers), then remembered folders.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Incremented on every open; worker answers for older generations are ignored.
    pub generation: u64,
    /// Open windows and tabs, then remembered folders from `open_len` on.
    pub items: Vec<SearchItem>,
    open_len: usize,
    windows: Vec<WindowInfo>,
    tabs_merged: bool,
}

impl Snapshot {
    pub fn new(generation: u64, windows: Vec<WindowInfo>) -> Self {
        let items: Vec<SearchItem> = windows.iter().map(window_item).collect();
        Snapshot {
            generation,
            open_len: items.len(),
            items,
            windows,
            tabs_merged: false,
        }
    }

    /// Open windows and tabs only.
    pub fn open_items(&self) -> &[SearchItem] {
        &self.items[..self.open_len]
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

    pub fn tabs_merged(&self) -> bool {
        self.tabs_merged
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
        let history = self.items.split_off(self.open_len);
        let mut new_items = Vec::with_capacity(self.items.len() + tabs.len());
        for item in self.items.drain(..) {
            let hwnd = match item.target {
                Target::Window { hwnd } => hwnd,
                _ => {
                    new_items.push(item);
                    continue;
                }
            };
            let mut window_tabs: Vec<&TabInfo> = tabs.iter().filter(|t| t.explorer_hwnd == hwnd).collect();
            if window_tabs.is_empty() {
                new_items.push(item);
                continue;
            }
            window_tabs.sort_by_key(|t| (!t.active, t.z));
            new_items.extend(window_tabs.into_iter().map(|t| tab_item(t, home)));
        }
        self.items = new_items;
        self.open_len = self.items.len();
        self.set_history(history);
        self.tabs_merged = true;
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

    /// Drop every result that belongs to a window that no longer exists.
    pub fn remove_window(&mut self, gone: isize) {
        let before = self.items.len();
        let open_len = self.open_len;
        let mut idx = 0;
        let mut removed_open = 0;
        self.items.retain(|i| {
            let keep = match i.target {
                Target::Window { hwnd } => hwnd != gone,
                Target::ExplorerTab { explorer_hwnd, .. } => explorer_hwnd != gone,
                Target::OpenFolder { .. } => true,
            };
            if !keep && idx < open_len {
                removed_open += 1;
            }
            idx += 1;
            keep
        });
        debug_assert!(self.items.len() <= before);
        self.open_len -= removed_open;
        self.windows.retain(|w| w.hwnd != gone);
    }

    /// Drop a remembered folder (e.g. it no longer exists).
    pub fn remove_folder(&mut self, path: &Path) {
        let key = path_key(path);
        let open_len = self.open_len;
        let mut idx = 0;
        self.items.retain(|i| {
            let keep = idx < open_len || i.folder().is_none_or(|p| path_key(p) != key);
            idx += 1;
            keep
        });
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
