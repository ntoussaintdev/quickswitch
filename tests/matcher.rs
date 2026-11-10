//! T2 — ranking (plan §9.1).

use quickswitch::search::matcher::Ranker;
use quickswitch::search::{SearchItem, Target};

fn item(hwnd: isize, title: &str, app: &str) -> SearchItem {
    SearchItem {
        title: title.into(),
        subtitle: app.into(),
        target: Target::Window { hwnd },
        recent_key: format!("win:{}|{}", app.to_lowercase(), title.to_lowercase()),
    }
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

fn titles(items: &[SearchItem], order: &[usize]) -> Vec<String> {
    order.iter().map(|&i| items[i].title.clone()).collect()
}

#[test]
fn empty_query_keeps_z_order() {
    let items = items();
    let order = Ranker::default().rank(&items, "", &[], 10);
    assert_eq!(order, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn limit_is_respected() {
    let items = items();
    assert_eq!(Ranker::default().rank(&items, "", &[], 3).len(), 3);
}

#[test]
fn matching_is_case_insensitive() {
    let items = items();
    let mut r = Ranker::default();
    assert_eq!(r.rank(&items, "SPOTIFY", &[], 10), vec![5]);
    assert_eq!(r.rank(&items, "spotify", &[], 10), vec![5]);
}

#[test]
fn prefix_beats_later_substring() {
    let items = items();
    let order = Ranker::default().rank(&items, "proj", &[], 10);
    let t = titles(&items, &order);
    assert_eq!(t[0], "Projects - File Explorer");
    assert!(t.contains(&"projects-archive - File Explorer".to_string()));
}

#[test]
fn word_prefix_beats_scattered_fuzzy() {
    let items = items();
    let order = Ranker::default().rank(&items, "code", &[], 10);
    assert_eq!(items[order[0]].title, "quickswitch - Visual Studio Code");
}

#[test]
fn fuzzy_subsequence_matches() {
    let items = items();
    // "vsc" is not a substring of any title, but is a subsequence of "Visual Studio Code".
    let order = Ranker::default().rank(&items, "vsc", &[], 10);
    assert_eq!(items[order[0]].title, "quickswitch - Visual Studio Code");
    // Small typo-like skips still find the window.
    let order = Ranker::default().rank(&items, "redme", &[], 10);
    assert_eq!(items[order[0]].title, "README.md - Notepad");
}

#[test]
fn non_matches_are_excluded() {
    let items = items();
    assert!(Ranker::default().rank(&items, "zzzz", &[], 10).is_empty());
}

#[test]
fn subtitle_participates_in_fuzzy_match() {
    let items = items();
    let order = Ranker::default().rank(&items, "outlook inbox", &[], 10);
    assert_eq!(order.first(), Some(&0));
}

#[test]
fn duplicate_titles_are_all_listed() {
    let items = vec![item(1, "plans", "File Explorer"), item(2, "plans", "File Explorer")];
    let order = Ranker::default().rank(&items, "plans", &[], 10);
    assert_eq!(order, vec![0, 1]);
}

#[test]
fn recency_reorders_equal_matches_and_empty_query() {
    let items = items();
    let recent = vec![items[4].recent_key.clone()];
    let mut r = Ranker::default();
    // Empty query: the recent pick moves to the top.
    assert_eq!(r.rank(&items, "", &recent, 10)[0], 4);
    // Both Explorer windows match "explorer" equally; recency breaks the tie.
    let order = r.rank(&items, "explorer", &recent, 10);
    assert_eq!(order[0], 4);
    // Recency never lifts a non-match into the results.
    assert!(!r.rank(&items, "spotify", &recent, 10).contains(&4));
}
