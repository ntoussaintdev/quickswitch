//! Browser detection, tab-name cleanup and window-title parsing
//! (plan 2026-11-11-n1 step 1).

use quickswitch::search::browsers::{
    BROWSERS, Engine, browser_of, clean_tab_name, display_name, profile_from_title, title_hash,
};

const CHROME: &str = "Chrome_WidgetWin_1";

#[test]
fn browsers_are_recognised_by_class_and_process() {
    assert_eq!(browser_of(CHROME, "chrome").unwrap().name, "Chrome");
    assert_eq!(browser_of(CHROME, "msedge").unwrap().name, "Edge");
    assert_eq!(browser_of(CHROME, "MSEDGE").unwrap().name, "Edge");
    assert_eq!(browser_of(CHROME, "brave").unwrap().name, "Brave");
    assert_eq!(
        browser_of("MozillaWindowClass", "firefox").unwrap().engine,
        Engine::Gecko
    );
}

#[test]
fn electron_apps_are_not_browsers() {
    for app in ["Code", "slack", "Discord", "ms-teams", "Spotify", "Obsidian"] {
        assert!(browser_of(CHROME, app).is_none(), "{app}");
    }
    // Right process, wrong window (e.g. a Chrome popup with another class).
    assert!(browser_of("Chrome_RenderWidgetHostHWND", "chrome").is_none());
    assert!(browser_of("MozillaDialogClass", "firefox").is_none());
}

#[test]
fn chromium_state_suffixes_are_removed() {
    let c = |s| clean_tab_name(Engine::Chromium, s);
    assert_eq!(c("Inbox (3) - Mail"), "Inbox (3) - Mail");
    assert_eq!(c("Lo-fi beats - YouTube - Audio playing"), "Lo-fi beats - YouTube");
    assert_eq!(c("Docs - Memory usage - 1.2 GB"), "Docs");
    assert_eq!(c("Docs - Memory usage - 120 MB - Audio playing"), "Docs");
    assert_eq!(c("Board - Part of group Work"), "Board");
    assert_eq!(c("Call - Camera or microphone recording"), "Call");
    assert_eq!(c("  Spaces  "), "Spaces");
    // Never cut everything away.
    assert_eq!(c(" - Pinned"), "- Pinned");
}

#[test]
fn gecko_names_are_only_trimmed() {
    assert_eq!(
        clean_tab_name(Engine::Gecko, " Page - Audio playing "),
        "Page - Audio playing"
    );
}

#[test]
fn edge_profile_is_read_from_the_window_title() {
    let edge = browser_of(CHROME, "msedge").unwrap();
    let t = "Pull requests and 2 more pages - Work - Microsoft\u{200b} Edge";
    assert_eq!(profile_from_title(edge, t, "Pull requests").as_deref(), Some("Work"));
    let t = "Pull requests and 1 more page - Personal - Microsoft Edge";
    assert_eq!(
        profile_from_title(edge, t, "Pull requests").as_deref(),
        Some("Personal")
    );
    let t = "Pull requests - Work - Microsoft Edge";
    assert_eq!(profile_from_title(edge, t, "Pull requests").as_deref(), Some("Work"));
    // No profile shown.
    assert_eq!(
        profile_from_title(edge, "Pull requests - Microsoft Edge", "Pull requests"),
        None
    );
    // Title doesn't start with the active tab (it changed meanwhile).
    assert_eq!(
        profile_from_title(edge, "Other - Work - Microsoft Edge", "Pull requests"),
        None
    );
}

#[test]
fn chrome_title_without_profile() {
    let chrome = browser_of(CHROME, "chrome").unwrap();
    assert_eq!(profile_from_title(chrome, "Inbox - Google Chrome", "Inbox"), None);
    assert_eq!(display_name(chrome, None), "Chrome");
    let edge = browser_of(CHROME, "msedge").unwrap();
    assert_eq!(display_name(edge, Some("Work")), "Edge \u{b7} Work");
}

#[test]
fn title_hash_is_fixed() {
    // FNV-1a test vectors: must not change between versions (stored in config).
    assert_eq!(title_hash(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(title_hash("a"), 0xaf63_dc4c_8601_ec8c);
    assert_ne!(title_hash("inbox"), title_hash("inbox "));
}

#[test]
fn table_has_no_duplicates() {
    for (i, a) in BROWSERS.iter().enumerate() {
        for b in &BROWSERS[i + 1..] {
            assert!(!(a.class == b.class && a.process == b.process), "{}", a.name);
        }
    }
}
