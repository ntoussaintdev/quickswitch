//! T4 — result lifecycle: stale handles, tabs that disappear, late worker answers.

use quickswitch::search::{Snapshot, TabInfo, Target, WindowInfo};

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

fn tab(explorer_hwnd: isize, token: u64, name: &str, index: usize, count: usize) -> TabInfo {
    TabInfo {
        explorer_hwnd,
        token,
        name: name.into(),
        index,
        count,
        selected: index == 0,
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

#[test]
fn explorer_windows_are_listed_for_the_worker() {
    assert_eq!(snapshot().explorer_windows(), vec![2, 3]);
}

#[test]
fn tabs_replace_their_window_row() {
    let mut s = snapshot();
    assert!(s.merge_tabs(7, &[tab(2, 10, "plans", 0, 2), tab(2, 11, "research", 1, 2)]));
    let titles: Vec<&str> = s.items.iter().map(|i| i.title.as_str()).collect();
    // Window 3 reported no tabs (e.g. minimized), so it keeps its window row.
    assert_eq!(titles, vec!["Editor", "plans", "research", "Downloads - File Explorer"]);
    assert!(matches!(
        s.items[2].target,
        Target::ExplorerTab {
            explorer_hwnd: 2,
            token: 11,
            ..
        }
    ));
    assert!(s.items[1].subtitle.contains("plans and 1 more tab"));
    assert!(s.items[1].subtitle.contains("tab 1 of 2"));
    assert!(s.tabs_merged());
}

#[test]
fn duplicate_tab_names_get_distinct_context() {
    let mut s = Snapshot::new(
        1,
        vec![explorer(2, "A - File Explorer"), explorer(3, "B - File Explorer")],
    );
    s.merge_tabs(1, &[tab(2, 1, "plans", 0, 1), tab(3, 2, "plans", 0, 1)]);
    assert_eq!(s.items.len(), 2);
    assert_ne!(s.items[0].subtitle, s.items[1].subtitle);
}

#[test]
fn late_worker_answer_is_ignored() {
    let mut s = snapshot();
    let before = s.items.clone();
    // Answer for an older palette opening.
    assert!(!s.merge_tabs(6, &[tab(2, 10, "plans", 0, 1)]));
    assert_eq!(s.items, before);
    assert!(!s.tabs_merged());
}

#[test]
fn closed_window_is_removed_with_its_tabs() {
    let mut s = snapshot();
    s.merge_tabs(7, &[tab(2, 10, "plans", 0, 2), tab(2, 11, "research", 1, 2)]);
    s.remove_window(2);
    let titles: Vec<&str> = s.items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, vec!["Editor", "Downloads - File Explorer"]);
    assert_eq!(s.explorer_windows(), vec![3]);
}

#[cfg(windows)]
#[test]
fn stale_window_handle_is_an_error_not_a_crash() {
    use quickswitch::os::windows::{activate, is_window};
    // A handle value that is not a live window.
    let stale: isize = 0x7fff_fff0;
    assert!(!is_window(stale));
    assert!(activate(stale).is_err());
    assert!(activate(0).is_err());
}
