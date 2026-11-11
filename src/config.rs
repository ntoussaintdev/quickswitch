//! Preferences and recency, stored at `%APPDATA%\QuickSwitch\config.toml`.
//!
//! The "Start with Windows" state is deliberately *not* stored here: the
//! registry Run value is the single source of truth (plan §7.1).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Win+K";
pub const MAX_RECENT: usize = 50;
/// Remembered folders (opened from QuickSwitch or seen open in Explorer).
pub const MAX_FOLDERS: usize = 200;
/// A file with this name next to QuickSwitch.exe turns on portable mode:
/// settings and the log are kept beside the exe instead of in `%APPDATA%`.
pub const PORTABLE_MARKER: &str = "QuickSwitch.portable";

/// The exe's folder when running in portable mode.
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join(PORTABLE_MARKER).is_file().then(|| dir.to_path_buf())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Main shortcut, e.g. "Ctrl+Win+K".
    pub hotkey: String,
    /// Optional second shortcut, registered only when set (plan step 5).
    pub fallback_hotkey: Option<String>,
    /// How many results the palette lists (clamped to 1..=20).
    pub max_results: usize,
    /// Recently picked result keys, most recent first.
    pub recent: Vec<String>,
    /// Folders opened from QuickSwitch or seen open in Explorer, most recent
    /// first. Searchable even when no Explorer window shows them.
    pub folders: Vec<String>,
    /// List and switch browser tabs (tray: "Search browser tabs").
    pub browser_tabs: bool,
    /// List and switch Windows Terminal tabs.
    pub terminal_tabs: bool,
    /// Offer a web search row for typed text (tray: "Web search in results").
    pub web_search: bool,
    /// Search engine id (see `actions::web::ENGINES`), or "custom".
    pub search_engine: String,
    /// Used when `search_engine` is "custom"; `{}` marks the query.
    pub custom_search_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: DEFAULT_HOTKEY.to_string(),
            fallback_hotkey: None,
            max_results: 9,
            recent: Vec::new(),
            folders: Vec::new(),
            browser_tabs: true,
            terminal_tabs: true,
            web_search: true,
            search_engine: crate::actions::web::DEFAULT_ENGINE.to_string(),
            custom_search_url: String::new(),
        }
    }
}

impl Config {
    /// `config.toml` next to the exe in portable mode, else under `%APPDATA%`.
    pub fn default_path() -> Option<PathBuf> {
        if let Some(dir) = portable_dir() {
            return Some(dir.join("config.toml"));
        }
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("QuickSwitch").join("config.toml"))
    }

    /// Parse TOML text. Invalid text falls back to defaults with a warning.
    pub fn from_toml(text: &str) -> (Config, Option<String>) {
        match toml::from_str::<Config>(text) {
            Ok(mut c) => {
                c.max_results = c.max_results.clamp(1, 20);
                c.recent.truncate(MAX_RECENT);
                c.folders.truncate(MAX_FOLDERS);
                (c, None)
            }
            Err(e) => (Config::default(), Some(format!("Ignoring invalid config: {e}"))),
        }
    }

    /// Load from `path`. A missing file gives defaults without a warning.
    pub fn load(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
            Err(e) => (Config::default(), Some(format!("Could not read config: {e}"))),
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Write atomically (temp file + rename) so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml())?;
        std::fs::rename(&tmp, path)
    }

    /// Move `key` to the front of the recent list.
    pub fn push_recent(&mut self, key: &str) {
        self.recent.retain(|k| k != key);
        self.recent.insert(0, key.to_string());
        self.recent.truncate(MAX_RECENT);
    }

    /// Move a folder to the front of the remembered folders (it was just used).
    pub fn push_folder(&mut self, path: &Path) {
        let p = path.to_string_lossy().into_owned();
        self.folders.retain(|f| !f.eq_ignore_ascii_case(&p));
        self.folders.insert(0, p);
        self.folders.truncate(MAX_FOLDERS);
    }

    /// Remember folders seen open in Explorer. Already-known folders keep
    /// their place. Returns whether anything changed.
    pub fn note_folders<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) -> bool {
        let mut changed = false;
        for path in paths {
            let p = path.to_string_lossy();
            if !self.folders.iter().any(|f| f.eq_ignore_ascii_case(&p)) {
                self.folders.insert(0, p.into_owned());
                changed = true;
            }
        }
        self.folders.truncate(MAX_FOLDERS);
        changed
    }

    /// Forget a folder (it no longer exists).
    pub fn forget_folder(&mut self, path: &Path) {
        let p = path.to_string_lossy();
        self.folders.retain(|f| !f.eq_ignore_ascii_case(&p));
    }
}
