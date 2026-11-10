//! Search model: plain result records, the per-open snapshot and ranking.
//!
//! Nothing here holds COM objects or long-lived native handles. A window
//! handle is stored as a plain `isize` and re-validated before use.

pub mod explorer_tabs;
pub mod matcher;

use std::path::PathBuf;

/// What happens when a result is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Window {
        hwnd: isize,
    },
    /// `token` is an ephemeral id owned by the UIA worker. `name` and `index`
    /// let the worker find the tab again if the token went stale.
    ExplorerTab {
        explorer_hwnd: isize,
        token: u64,
        name: String,
        index: usize,
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
    OpenFolder,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Window => "Window",
            Kind::ExplorerTab => "Explorer tab",
            Kind::OpenFolder => "Open folder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchItem {
    pub title: String,
    pub subtitle: String,
    pub target: Target,
    /// Stable-ish identity used for recency ordering (never a handle).
    pub recent_key: String,
}

impl SearchItem {
    pub fn kind(&self) -> Kind {
        match self.target {
            Target::Window { .. } => Kind::Window,
            Target::ExplorerTab { .. } => Kind::ExplorerTab,
            Target::OpenFolder { .. } => Kind::OpenFolder,
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

/// One Explorer tab reported by the UIA worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    pub explorer_hwnd: isize,
    pub token: u64,
    pub name: String,
    pub index: usize,
    pub count: usize,
    pub selected: bool,
}

/// Results for one palette opening: windows first, Explorer tabs merged in
/// when the worker answers.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Incremented on every open; worker answers for older generations are ignored.
    pub generation: u64,
    pub items: Vec<SearchItem>,
    windows: Vec<WindowInfo>,
    tabs_merged: bool,
}

impl Snapshot {
    pub fn new(generation: u64, windows: Vec<WindowInfo>) -> Self {
        let items = windows.iter().map(window_item).collect();
        Snapshot {
            generation,
            items,
            windows,
            tabs_merged: false,
        }
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
    /// replaced by one row per tab. Windows with no reported tabs (for example
    /// minimized ones) keep their window row.
    pub fn merge_tabs(&mut self, generation: u64, tabs: &[TabInfo]) -> bool {
        if generation != self.generation {
            return false;
        }
        let mut new_items = Vec::with_capacity(self.items.len() + tabs.len());
        for item in self.items.drain(..) {
            let hwnd = match item.target {
                Target::Window { hwnd } => hwnd,
                _ => {
                    new_items.push(item);
                    continue;
                }
            };
            let window_tabs: Vec<&TabInfo> = tabs.iter().filter(|t| t.explorer_hwnd == hwnd).collect();
            if window_tabs.is_empty() {
                new_items.push(item);
                continue;
            }
            let context = explorer_context(&item.title);
            for t in window_tabs {
                let mut subtitle = format!("File Explorer · {context} · tab {} of {}", t.index + 1, t.count);
                if t.selected {
                    subtitle.push_str(" · current");
                }
                new_items.push(SearchItem {
                    title: t.name.clone(),
                    subtitle,
                    target: Target::ExplorerTab {
                        explorer_hwnd: t.explorer_hwnd,
                        token: t.token,
                        name: t.name.clone(),
                        index: t.index,
                    },
                    recent_key: format!("tab:{}", t.name.to_lowercase()),
                });
            }
        }
        self.items = new_items;
        self.tabs_merged = true;
        true
    }

    /// Drop every result that belongs to a window that no longer exists.
    pub fn remove_window(&mut self, gone: isize) {
        self.items.retain(|i| match i.target {
            Target::Window { hwnd } => hwnd != gone,
            Target::ExplorerTab { explorer_hwnd, .. } => explorer_hwnd != gone,
            Target::OpenFolder { .. } => true,
        });
        self.windows.retain(|w| w.hwnd != gone);
    }
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
    }
}

/// "plans and 2 more tabs - File Explorer" -> "window “plans and 2 more tabs”"
fn explorer_context(window_title: &str) -> String {
    let t = window_title.strip_suffix(" - File Explorer").unwrap_or(window_title);
    format!("window “{t}”")
}

/// Item for a resolved folder path.
pub fn folder_item(path: PathBuf) -> SearchItem {
    let display = path.display().to_string();
    SearchItem {
        title: display.clone(),
        subtitle: "Open in File Explorer".into(),
        recent_key: format!("dir:{}", display.to_lowercase()),
        target: Target::OpenFolder { path },
    }
}
