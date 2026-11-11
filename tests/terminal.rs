//! Windows Terminal tabs: detection, labels for repeated titles, row kind.

use quickswitch::search::browsers::{Engine, browser_of, clean_tab_name, profile_from_title, tab_labels};
use quickswitch::search::{BrowserTabInfo, Kind, browser_tab_item};

const TERMINAL_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";

#[test]
fn windows_terminal_is_recognised_but_old_consoles_are_not() {
    let t = browser_of(TERMINAL_CLASS, "WindowsTerminal").expect("Terminal");
    assert_eq!(t.engine, Engine::Terminal);
    assert_eq!(t.name, "Terminal");
    // conhost windows have no tabs.
    assert!(browser_of("ConsoleWindowClass", "conhost").is_none());
    assert!(browser_of("ConsoleWindowClass", "WindowsTerminal").is_none());
}

#[test]
fn terminal_titles_are_kept_and_have_no_profile() {
    let t = browser_of(TERMINAL_CLASS, "windowsterminal").unwrap();
    assert_eq!(
        clean_tab_name(Engine::Terminal, " C:\\dev - Audio playing "),
        "C:\\dev - Audio playing"
    );
    assert_eq!(profile_from_title(t, "PowerShell", "PowerShell"), None);
}

#[test]
fn repeated_terminal_titles_get_their_tab_number() {
    let t = browser_of(TERMINAL_CLASS, "WindowsTerminal").unwrap();
    let labels = tab_labels(t, "Terminal", &["PowerShell", "Ubuntu", "PowerShell"]);
    assert_eq!(
        labels,
        vec!["Terminal \u{b7} tab 1", "Terminal", "Terminal \u{b7} tab 3"]
    );
    // Browsers never get numbers.
    let chrome = browser_of("Chrome_WidgetWin_1", "chrome").unwrap();
    assert_eq!(
        tab_labels(chrome, "Chrome", &["New Tab", "New Tab"]),
        vec!["Chrome", "Chrome"]
    );
}

#[test]
fn terminal_tab_rows_have_their_own_kind_and_key() {
    let item = browser_tab_item(&BrowserTabInfo {
        window_hwnd: 5,
        key: 1,
        raw_name: "PowerShell".into(),
        title: "PowerShell".into(),
        browser: "Terminal \u{b7} tab 2".into(),
        nth: 1,
        selected: false,
        terminal: true,
    });
    assert_eq!(item.kind(), Kind::TerminalTab);
    assert_eq!(item.kind().label(), "Terminal tab");
    assert!(item.recent_key.starts_with("term:terminal"), "{}", item.recent_key);
}
