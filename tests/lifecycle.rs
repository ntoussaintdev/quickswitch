//! T4 — result lifecycle: stale handles, tabs that disappear, late worker answers.

use std::path::{Path, PathBuf};

use quickswitch::search::{BrowserTabInfo, Snapshot, TabInfo, Target, WindowInfo, folder_item};

fn home() -> PathBuf {
    PathBuf::from(r"C:\Users\alice")
}

fn win(hwnd: isize, title: &str, class: &str) -> WindowInfo {
    WindowInfo {
        hwnd,
        title: title.into(),
        process: "app".into(),
        class_name: class.into(),
    }
}

fn explorer(hwnd: isize, title: &str) -> WindowInfo {
    win(hwnd, title, "CabinetWClass")
}

fn tab(explorer_hwnd: isize, tab_hwnd: isize, path: &str, z: usize) -> TabInfo {
    let path = PathBuf::from(path);
    TabInfo {
        explorer_hwnd,
        tab_hwnd,
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        path: Some(path),
        nth: 0,
        active: z == 0,
        z,
    }
}

fn snapshot() -> Snapshot {
    Snapshot::new(
        7,
        vec![
            win(1, "Editor", "Chrome_WidgetWin_1"),
            explorer(2, "plans and 1 more tab - File Explorer"),
            explorer(3, "Downloads - File Explorer"),
        ],
    )
}

fn titles(s: &Snapshot) -> Vec<&str> {
    s.items.iter().map(|i| i.title.as_str()).collect()
}

#[test]
fn explorer_windows_are_listed_for_the_worker() {
    assert_eq!(snapshot().explorer_windows(), vec![2, 3]);
}

#[test]
fn tabs_replace_their_window_row_shown_tab_first() {
    let mut s = snapshot();
    let tabs = [
        tab(2, 11, r"C:\Users\alice\research", 1),
        tab(2, 10, r"C:\Users\alice\work\plans", 0),
    ];
    assert!(s.merge_tabs(7, &tabs, &home()));
    // Window 3 reported no tabs, so it keeps its window row.
    assert_eq!(
        titles(&s),
        vec!["Editor", "plans", "research", "Downloads - File Explorer"]
    );
    assert!(matches!(
        s.items[2].target,
        Target::ExplorerTab {
            explorer_hwnd: 2,
            tab_hwnd: 11,
            ..
        }
    ));
    assert_eq!(s.items[1].subtitle, "~/work/plans");
    assert!(s.tabs_merged());
}

#[test]
fn duplicate_tab_names_show_their_paths() {
    let mut s = Snapshot::new(1, vec![explorer(2, "A - File Explorer")]);
    s.merge_tabs(
        1,
        &[
            tab(2, 1, r"C:\Users\alice\a\plans", 0),
            tab(2, 2, r"C:\Users\alice\b\plans", 1),
        ],
        &home(),
    );
    assert_eq!(s.items.len(), 2);
    assert_ne!(s.items[0].subtitle, s.items[1].subtitle);
}

#[test]
fn tabs_of_windows_not_in_the_snapshot_are_ignored() {
    let mut s = snapshot();
    s.merge_tabs(7, &[tab(99, 5, r"C:\elsewhere", 0)], &home());
    assert_eq!(s.items.len(), 3);
}

#[test]
fn late_worker_answer_is_ignored() {
    let mut s = snapshot();
    let before = s.items.clone();
    // Answer for an older palette opening.
    assert!(!s.merge_tabs(6, &[tab(2, 10, r"C:\x\plans", 0)], &home()));
    assert_eq!(s.items, before);
    assert!(!s.tabs_merged());
}

#[test]
fn closed_window_is_removed_with_its_tabs() {
    let mut s = snapshot();
    s.set_history(vec![folder_item(PathBuf::from(r"D:\music"), &home())]);
    s.merge_tabs(
        7,
        &[tab(2, 10, r"C:\x\plans", 0), tab(2, 11, r"C:\x\research", 1)],
        &home(),
    );
    s.remove_window(2);
    assert_eq!(titles(&s), vec!["Editor", "Downloads - File Explorer", "music"]);
    assert_eq!(s.open_items().len(), 2);
    assert_eq!(s.explorer_windows(), vec![3]);
}

#[test]
fn history_skips_folders_open_in_a_tab_and_stays_after_open_items() {
    let mut s = snapshot();
    s.set_history(vec![
        folder_item(PathBuf::from(r"C:\Users\alice\work\plans"), &home()),
        folder_item(PathBuf::from(r"D:\music"), &home()),
    ]);
    assert_eq!(s.open_items().len(), 3);
    assert_eq!(s.items.len(), 5);
    // Once the tab scan shows plans is open, its history row goes away.
    s.merge_tabs(7, &[tab(2, 10, r"C:\Users\alice\Work\Plans", 0)], &home());
    assert_eq!(
        titles(&s),
        vec!["Editor", "Plans", "Downloads - File Explorer", "music"]
    );
    assert_eq!(s.open_items().len(), 3);
}

#[test]
fn an_open_folder_is_found_as_its_tab() {
    let mut s = snapshot();
    s.merge_tabs(
        7,
        &[
            tab(2, 10, r"C:\Users\alice\Projects", 1),
            tab(2, 11, r"C:\Users\alice\work", 0),
        ],
        &home(),
    );
    // Case and trailing separators don't matter.
    let found = s
        .tab_for(Path::new(r"c:\users\alice\projects\"))
        .expect("open in a tab");
    assert!(matches!(found.target, Target::ExplorerTab { tab_hwnd: 10, .. }));
    assert!(s.tab_for(Path::new(r"C:\Users\alice\other")).is_none());
}

#[test]
fn missing_remembered_folder_can_be_dropped() {
    let mut s = snapshot();
    s.set_history(vec![folder_item(PathBuf::from(r"D:\gone"), &home())]);
    s.remove_folder(Path::new(r"d:\GONE"));
    assert_eq!(s.items.len(), 3);
}

#[cfg(windows)]
#[test]
fn stale_window_handle_is_an_error_not_a_crash() {
    use quickswitch::os::windows::{activate, active_explorer_tab, is_window};
    // A handle value that is not a live window.
    let stale: isize = 0x7fff_fff0;
    assert!(!is_window(stale));
    assert!(activate(stale).is_err());
    assert!(activate(0).is_err());
    assert_eq!(active_explorer_tab(stale), None);
}

// ----- browser tabs (plan 2026-11-11-n1 step 2) -----------------------------

fn browser_win(hwnd: isize, title: &str, process: &str) -> WindowInfo {
    WindowInfo {
        hwnd,
        title: title.into(),
        process: process.into(),
        class_name: "Chrome_WidgetWin_1".into(),
    }
}

fn btab(window_hwnd: isize, key: u64, title: &str, selected: bool) -> BrowserTabInfo {
    BrowserTabInfo {
        window_hwnd,
        key,
        raw_name: title.into(),
        title: title.into(),
        browser: "Chrome".into(),
        nth: 0,
        selected,
        terminal: false,
    }
}

fn browser_snapshot() -> Snapshot {
    let mut s = Snapshot::new(
        4,
        vec![
            browser_win(1, "Docs - Google Chrome", "chrome"),
            win(2, "Editor", "Chrome_WidgetWin_1"),
            explorer(3, "Downloads - File Explorer"),
        ],
    );
    s.set_history(vec![folder_item(PathBuf::from(r"D:\music"), &home())]);
    s
}

fn listed(s: &Snapshot) -> Vec<&str> {
    s.listed_items().iter().map(|i| i.title.as_str()).collect()
}

#[test]
fn browser_windows_are_listed_for_the_worker_but_electron_apps_are_not() {
    let s = browser_snapshot();
    let b = s.browser_windows();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].hwnd, 1);
    assert_eq!(b[0].spec.name, "Chrome");
}

#[test]
fn shown_browser_tab_is_listed_and_the_others_are_searched_only() {
    let mut s = browser_snapshot();
    let tabs = [
        btab(1, 10, "Mail", false),
        btab(1, 11, "Docs", true),
        btab(1, 12, "News", false),
    ];
    assert!(s.merge_browser_tabs(4, &tabs));
    assert_eq!(listed(&s), vec!["Docs", "Editor", "Downloads - File Explorer"]);
    assert_eq!(
        titles(&s),
        vec!["Docs", "Editor", "Downloads - File Explorer", "Mail", "News", "music"]
    );
    assert_eq!(s.open_items().len(), 5);
    assert!(matches!(
        s.items[0].target,
        Target::BrowserTab {
            window_hwnd: 1,
            key: 11,
            ..
        }
    ));
    assert_eq!(s.items[0].subtitle, "Chrome");
    assert!(s.browser_tabs_merged());
}

#[test]
fn browser_window_without_a_selected_tab_lists_its_first_tab() {
    let mut s = browser_snapshot();
    s.merge_browser_tabs(4, &[btab(1, 10, "Mail", false), btab(1, 11, "Docs", false)]);
    assert_eq!(listed(&s)[0], "Mail");
}

#[test]
fn explorer_and_browser_merges_work_in_either_order() {
    let mut a = browser_snapshot();
    a.merge_tabs(4, &[tab(3, 30, r"C:\Users\alice\Downloads", 0)], &home());
    a.merge_browser_tabs(4, &[btab(1, 10, "Mail", true), btab(1, 11, "Docs", false)]);
    let mut b = browser_snapshot();
    b.merge_browser_tabs(4, &[btab(1, 10, "Mail", true), btab(1, 11, "Docs", false)]);
    b.merge_tabs(4, &[tab(3, 30, r"C:\Users\alice\Downloads", 0)], &home());
    assert_eq!(titles(&a), titles(&b));
    assert_eq!(listed(&a), vec!["Mail", "Editor", "Downloads"]);
    assert_eq!(titles(&a), vec!["Mail", "Editor", "Downloads", "Docs", "music"]);
}

#[test]
fn late_browser_answer_is_ignored() {
    let mut s = browser_snapshot();
    let before = s.items.clone();
    assert!(!s.merge_browser_tabs(3, &[btab(1, 10, "Mail", true)]));
    assert_eq!(s.items, before);
    assert!(!s.browser_tabs_merged());
}

#[test]
fn closed_browser_window_is_removed_with_all_its_tabs() {
    let mut s = browser_snapshot();
    s.merge_browser_tabs(4, &[btab(1, 10, "Mail", true), btab(1, 11, "Docs", false)]);
    s.remove_window(1);
    assert_eq!(titles(&s), vec!["Editor", "Downloads - File Explorer", "music"]);
    assert_eq!(listed(&s).len(), 2);
    assert_eq!(s.open_items().len(), 2);
}

#[test]
fn a_closed_browser_tab_row_can_be_dropped() {
    let mut s = browser_snapshot();
    s.merge_browser_tabs(4, &[btab(1, 10, "Mail", true), btab(1, 11, "Docs", false)]);
    let docs = s.items[3].clone();
    assert_eq!(docs.title, "Docs");
    s.remove_item(&docs);
    assert_eq!(titles(&s), vec!["Mail", "Editor", "Downloads - File Explorer", "music"]);
    assert_eq!(s.open_items().len(), 3);
    // Remembered folders still work after the ranges moved.
    s.remove_folder(Path::new(r"D:\music"));
    assert_eq!(s.items.len(), 3);
}

#[test]
fn browser_recent_key_does_not_contain_the_title() {
    let mut s = browser_snapshot();
    s.merge_browser_tabs(4, &[btab(1, 10, "Secret plans", true)]);
    let key = &s.items[0].recent_key;
    assert!(key.starts_with("web:chrome|"), "{key}");
    assert!(!key.to_lowercase().contains("secret"));
}
