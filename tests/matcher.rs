//! T2 — ranking and highlighting (plan §9.1).

use std::path::PathBuf;

use quickswitch::search::matcher::Ranker;
use quickswitch::search::{MatchOn, SearchItem, TabInfo, Target, folder_item, tab_item};

fn item(hwnd: isize, title: &str, app: &str) -> SearchItem {
    SearchItem {
        title: title.into(),
        subtitle: app.into(),
        target: Target::Window { hwnd },
        recent_key: format!("win:{}|{}", app.to_lowercase(), title.to_lowercase()),
        match_on: MatchOn::Text,
    }
}

fn home() -> PathBuf {
    PathBuf::from(r"C:\Users\alice")
}

fn tab(tab_hwnd: isize, path: &str) -> SearchItem {
    let path = PathBuf::from(path);
    tab_item(
        &TabInfo {
            explorer_hwnd: 100,
            tab_hwnd,
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path: Some(path),
            nth: 0,
            active: false,
            z: 0,
        },
        &home(),
    )
}

fn items() -> Vec<SearchItem> {
    vec![
        item(1, "Inbox - Outlook", "OUTLOOK"),
        item(2, "quickswitch - Visual Studio Code", "Code"),
        item(3, "Projects - File Explorer", "File Explorer"),
        item(4, "README.md - Notepad", "Notepad"),
        item(5, "projects-archive - File Explorer", "File Explorer"),
        item(6, "Spotify Premium", "Spotify"),
    ]
}

fn rank(items: &[SearchItem], query: &str, recent: &[String]) -> Vec<usize> {
    Ranker::default()
        .rank(items, query, recent, 10)
        .into_iter()
        .map(|h| h.index)
        .collect()
}

fn titles(items: &[SearchItem], order: &[usize]) -> Vec<String> {
    order.iter().map(|&i| items[i].title.clone()).collect()
}

#[test]
fn empty_query_keeps_z_order() {
    assert_eq!(rank(&items(), "", &[]), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn limit_is_respected() {
    assert_eq!(Ranker::default().rank(&items(), "", &[], 3).len(), 3);
}

#[test]
fn matching_is_case_insensitive() {
    let items = items();
    assert_eq!(rank(&items, "SPOTIFY", &[]), vec![5]);
    assert_eq!(rank(&items, "spotify", &[]), vec![5]);
}

#[test]
fn prefix_beats_later_substring() {
    let items = items();
    let t = titles(&items, &rank(&items, "proj", &[]));
    assert_eq!(t[0], "Projects - File Explorer");
    assert!(t.contains(&"projects-archive - File Explorer".to_string()));
}

#[test]
fn word_prefix_beats_scattered_fuzzy() {
    let items = items();
    assert_eq!(
        items[rank(&items, "code", &[])[0]].title,
        "quickswitch - Visual Studio Code"
    );
}

#[test]
fn fuzzy_matches_within_one_word() {
    let items = items();
    // A skipped letter inside a word still finds the window.
    assert_eq!(items[rank(&items, "redme", &[])[0]].title, "README.md - Notepad");
}

#[test]
fn fuzzy_never_spans_words() {
    // "vsc" is spread over three words: no longer a match.
    assert!(rank(&items(), "vsc", &[]).is_empty());
    // The bug report: "labour" matched an Explorer tab whose description
    // happened to contain l-a (pLAns), b (taB), o (Of), u-r (cURrent).
    let tabs = vec![tab(1, r"C:\Users\alice\Projects\Peace-of-mind\docs\plans")];
    assert!(rank(&tabs, "labour", &[]).is_empty());
}

#[test]
fn non_matches_are_excluded() {
    assert!(rank(&items(), "zzzz", &[]).is_empty());
}

#[test]
fn every_word_must_match_in_any_order() {
    let items = items();
    assert_eq!(rank(&items, "outlook inbox", &[]).first(), Some(&0));
    assert!(rank(&items, "outlook spotify", &[]).is_empty());
}

#[test]
fn duplicate_titles_are_all_listed() {
    let items = vec![item(1, "plans", "File Explorer"), item(2, "plans", "File Explorer")];
    assert_eq!(rank(&items, "plans", &[]), vec![0, 1]);
}

#[test]
fn recency_reorders_equal_matches_and_empty_query() {
    let items = items();
    let recent = vec![items[4].recent_key.clone()];
    // Empty query: the recent pick moves to the top.
    assert_eq!(rank(&items, "", &recent)[0], 4);
    // Both Explorer windows match "explorer" equally; recency breaks the tie.
    assert_eq!(rank(&items, "explorer", &recent)[0], 4);
    // Recency never lifts a non-match into the results.
    assert!(!rank(&items, "spotify", &recent).contains(&4));
}

#[test]
fn tabs_match_on_their_path() {
    let tabs = vec![
        tab(1, r"C:\Users\alice\Projects\Little-Loves\docs\plans"),
        tab(2, r"C:\Users\alice\Projects\Peace-of-mind\docs\plans"),
        tab(3, r"C:\Users\alice\Projects\Labour-monitoring\docs\resources"),
    ];
    // A word anywhere along the path finds the tab.
    assert_eq!(rank(&tabs, "labour", &[]), vec![2]);
    assert_eq!(rank(&tabs, "peace plans", &[]), vec![1]);
    // Typed with a slash, too.
    assert_eq!(rank(&tabs, "peace/plans", &[]), vec![1]);
    // The home folder is shown as ~ and is not searched.
    assert!(rank(&tabs, "alice", &[]).is_empty());
}

#[test]
fn own_folder_name_beats_a_parent_folder() {
    let tabs = vec![
        tab(1, r"C:\Users\alice\plans\archive"),
        tab(2, r"C:\Users\alice\work\plans"),
    ];
    assert_eq!(rank(&tabs, "plans", &[]), vec![1, 0]);
}

#[test]
fn highlights_mark_matched_characters() {
    let tabs = vec![tab(1, r"C:\Users\alice\Projects\Peace-of-mind\docs\plans")];
    let hits = Ranker::default().rank(&tabs, "peace plans", &[], 10);
    let item = &tabs[hits[0].index];
    assert_eq!(item.subtitle, "~/Projects/Peace-of-mind/docs/plans");
    let marked: String = hits[0]
        .subtitle_hl
        .iter()
        .map(|&i| item.subtitle.chars().nth(i).unwrap())
        .collect();
    assert_eq!(marked, "Peaceplans");
    // The title (the tab's own folder) gets the highlights of the last segment.
    assert_eq!(hits[0].title_hl, vec![0, 1, 2, 3, 4]);
}

#[test]
fn window_highlights_are_in_the_title() {
    let items = items();
    let hits = Ranker::default().rank(&items, "spot", &[], 10);
    assert_eq!(hits[0].title_hl, vec![0, 1, 2, 3]);
    assert!(hits[0].subtitle_hl.is_empty());
}

#[test]
fn remembered_folders_are_searchable_by_path() {
    let folders = vec![folder_item(PathBuf::from(r"D:\music\jazz"), &home())];
    assert_eq!(folders[0].subtitle, "D:/music/jazz");
    assert_eq!(folders[0].title, "jazz");
    assert_eq!(rank(&folders, "music", &[]), vec![0]);
}

#[test]
fn folder_names_match_for_suggestions() {
    let mut r = Ranker::default();
    let (exact, _) = r.match_name("Projects", "projects").unwrap();
    let (prefix, hl) = r.match_name("Projects", "projec").unwrap();
    let (fuzzy, _) = r.match_name("Projects", "prjcts").unwrap();
    assert!(exact > prefix && prefix > fuzzy);
    assert_eq!(hl, vec![0, 1, 2, 3, 4, 5]);
    assert!(r.match_name("Projects", "zzz").is_none());
    assert_eq!(r.match_name("Projects", ""), Some((0, Vec::new())));
}
