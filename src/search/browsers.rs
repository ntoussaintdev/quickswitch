//! Which windows are web browsers (or Windows Terminal, whose tabs are read
//! the same way), and how their tab names and window titles read (plan
//! 2026-11-11-n1 §4.1).
//!
//! Plain data and string handling only, so it is unit tested. Reading the tab
//! strips themselves is in `browser_tabs`.

/// How a browser's tab strip is laid out in UI Automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Chromium,
    Gecko,
    /// Windows Terminal (WinUI tab view).
    Terminal,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BrowserSpec {
    /// Executable name without extension, lowercase.
    pub process: &'static str,
    pub class: &'static str,
    /// Shown in the row's grey text.
    pub name: &'static str,
    pub engine: Engine,
    /// End of the window title, after the active tab's title (and profile).
    pub title_suffix: &'static str,
}

const CHROMIUM: &str = "Chrome_WidgetWin_1";
const GECKO: &str = "MozillaWindowClass";

/// Known browsers. The process check keeps Electron apps (VS Code, Slack,
/// Teams, ...) out: they share Chrome's window class.
pub const BROWSERS: &[BrowserSpec] = &[
    BrowserSpec {
        process: "chrome",
        class: CHROMIUM,
        name: "Chrome",
        engine: Engine::Chromium,
        title_suffix: " - Google Chrome",
    },
    BrowserSpec {
        process: "msedge",
        class: CHROMIUM,
        name: "Edge",
        engine: Engine::Chromium,
        title_suffix: " - Microsoft Edge",
    },
    BrowserSpec {
        process: "brave",
        class: CHROMIUM,
        name: "Brave",
        engine: Engine::Chromium,
        title_suffix: " - Brave",
    },
    BrowserSpec {
        process: "vivaldi",
        class: CHROMIUM,
        name: "Vivaldi",
        engine: Engine::Chromium,
        title_suffix: " - Vivaldi",
    },
    BrowserSpec {
        process: "opera",
        class: CHROMIUM,
        name: "Opera",
        engine: Engine::Chromium,
        title_suffix: " - Opera",
    },
    BrowserSpec {
        process: "firefox",
        class: GECKO,
        name: "Firefox",
        engine: Engine::Gecko,
        title_suffix: " \u{2014} Mozilla Firefox",
    },
    BrowserSpec {
        process: "librewolf",
        class: GECKO,
        name: "LibreWolf",
        engine: Engine::Gecko,
        title_suffix: " \u{2014} LibreWolf",
    },
    BrowserSpec {
        process: "waterfox",
        class: GECKO,
        name: "Waterfox",
        engine: Engine::Gecko,
        title_suffix: " \u{2014} Waterfox",
    },
    BrowserSpec {
        process: "zen",
        class: GECKO,
        name: "Zen",
        engine: Engine::Gecko,
        title_suffix: " \u{2014} Zen Browser",
    },
    BrowserSpec {
        process: "windowsterminal",
        class: "CASCADIA_HOSTING_WINDOW_CLASS",
        name: "Terminal",
        engine: Engine::Terminal,
        title_suffix: "",
    },
];

/// The browser (or Terminal) a top-level window belongs to, if any.
pub fn browser_of(class_name: &str, process: &str) -> Option<&'static BrowserSpec> {
    BROWSERS
        .iter()
        .find(|b| b.class == class_name && b.process.eq_ignore_ascii_case(process))
}

/// A browser window handed to the worker to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserWindow {
    pub hwnd: isize,
    pub spec: &'static BrowserSpec,
    pub title: String,
}

/// State text Chromium appends to a tab's accessible name (" - Audio
/// playing", " - Memory usage - 120 MB", " - Part of group Work", ...).
/// Everything from the first of these on is dropped.
const CHROMIUM_SUFFIXES: &[&str] = &[
    " - Audio playing",
    " - Audio muted",
    " - Muted",
    " - Media recording",
    " - Camera recording",
    " - Microphone recording",
    " - Camera and microphone recording",
    " - Camera or microphone recording",
    " - Desktop content shared",
    " - Tab content shared",
    " - Sharing this tab",
    " - Screen sharing",
    " - Bluetooth device connected",
    " - Connected to a Bluetooth device",
    " - USB device connected",
    " - Connected to a USB device",
    " - Connected to a serial port",
    " - Connected to a HID device",
    " - Virtual reality content",
    " - Picture-in-picture",
    " - Network error",
    " - Crashed",
    " - Pinned",
    " - Memory usage",
    " - Part of group",
    " - Part of unnamed group",
    " - Inactive tab",
    " - Sleeping",
];

/// The tab title as the user sees it on the tab.
pub fn clean_tab_name(engine: Engine, raw: &str) -> String {
    let mut name = raw.trim();
    if engine == Engine::Chromium {
        let cut = CHROMIUM_SUFFIXES.iter().filter_map(|s| name.find(s)).min();
        // Never cut the whole name away (a page titled "- Pinned" stays).
        if let Some(i) = cut.filter(|&i| i > 0) {
            name = name[..i].trim_end();
        }
    }
    name.to_string()
}

/// Edge (and some Chromium browsers) put the profile in the window title:
/// "<tab> and 2 more pages - Work - Microsoft Edge". Returns "Work".
/// `active` is the cleaned title of the tab the window is showing.
pub fn profile_from_title(spec: &BrowserSpec, window_title: &str, active: &str) -> Option<String> {
    if spec.engine != Engine::Chromium || active.is_empty() {
        return None;
    }
    // Edge writes "Microsoft\u{200b} Edge".
    let title = window_title.replace('\u{200b}', "");
    let rest = title.strip_suffix(spec.title_suffix)?;
    let mut rest = rest.strip_prefix(active)?;
    if let Some(after) = rest.strip_prefix(" and ") {
        // " and 2 more pages" / " and 1 more page"
        let i = after.find(" more page")?;
        let tail = &after[i + " more page".len()..];
        rest = tail.strip_prefix('s').unwrap_or(tail);
    }
    let profile = rest.strip_prefix(" - ")?.trim();
    (!profile.is_empty() && profile.chars().count() <= 40).then(|| profile.to_string())
}

/// The row's grey text: the browser, plus the profile when the title shows one.
pub fn display_name(spec: &BrowserSpec, profile: Option<&str>) -> String {
    match profile {
        Some(p) => format!("{} \u{b7} {p}", spec.name),
        None => spec.name.to_string(),
    }
}

/// Grey text for each tab of one window, given their titles in strip order.
/// Terminal tabs often share a title ("PowerShell"), so a repeated title gets
/// its tab number: "Terminal \u{b7} tab 3". Browser tabs all get `shown`.
pub fn tab_labels(spec: &BrowserSpec, shown: &str, titles: &[&str]) -> Vec<String> {
    titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let repeated = titles.iter().filter(|o| *o == t).count() > 1;
            if spec.engine == Engine::Terminal && repeated {
                format!("{shown} \u{b7} tab {}", i + 1)
            } else {
                shown.to_string()
            }
        })
        .collect()
}

/// FNV-1a, 64-bit. Fixed across Rust versions (unlike `DefaultHasher`), so
/// recent keys stored in `config.toml` keep matching after an update.
pub fn title_hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
