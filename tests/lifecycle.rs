//! T4 — result lifecycle: stale handles, tabs that disappear, late worker answers.

use std::path::{Path, PathBuf};

use quickswitch::search::{Snapshot, TabInfo, Target, WindowInfo, folder_item};

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
