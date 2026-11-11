//! Type filters at the start of the query ("browser: mail").

use quickswitch::search::{Kind, parse_qualifier};

fn kinds(q: &str) -> Option<(Vec<Kind>, String)> {
    parse_qualifier(q).map(|(q, rest)| (q.kinds.to_vec(), rest.to_string()))
}

#[test]
fn full_words_filter_to_their_kind() {
    assert_eq!(kinds("browser:"), Some((vec![Kind::BrowserTab], String::new())));
    assert_eq!(kinds("browser: mail"), Some((vec![Kind::BrowserTab], "mail".into())));
    assert_eq!(kinds("window:code"), Some((vec![Kind::Window], "code".into())));
    assert_eq!(
        kinds("explorer: plans"),
        Some((vec![Kind::ExplorerTab], "plans".into()))
    );
    assert_eq!(kinds("terminal: pwsh"), Some((vec![Kind::TerminalTab], "pwsh".into())));
    assert_eq!(kinds("web: inbox"), Some((vec![Kind::BrowserTab], "inbox".into())));
    assert_eq!(
        kinds("tab: x").unwrap().0,
        vec![Kind::ExplorerTab, Kind::BrowserTab, Kind::TerminalTab]
    );
    assert_eq!(kinds("folder: x").unwrap().0, vec![Kind::Folder, Kind::ExplorerTab]);
}

#[test]
fn case_plurals_prefixes_and_spaces() {
    assert_eq!(kinds("Browser: Mail").unwrap().1, "Mail");
    assert_eq!(kinds("  browser:   mail docs").unwrap().1, "mail docs");
    assert_eq!(kinds("windows:").unwrap().0, vec![Kind::Window]);
    assert_eq!(kinds("tabs:").unwrap().0.len(), 3);
    assert_eq!(kinds("br: x").unwrap().0, vec![Kind::BrowserTab]);
    assert_eq!(kinds("term: x").unwrap().0, vec![Kind::TerminalTab]);
    assert_eq!(kinds("wi: x").unwrap().0, vec![Kind::Window]);
    assert_eq!(kinds("we: x").unwrap().0, vec![Kind::BrowserTab]);
}

#[test]
fn other_text_is_not_a_filter() {
    // Drive letters and paths stay paths.
    assert_eq!(kinds("c:"), None);
    assert_eq!(kinds("f:/music"), None);
    // Unknown words, words with other characters, colons later on.
    assert_eq!(kinds("http://x"), None);
    assert_eq!(kinds("todo: buy milk"), None);
    assert_eq!(kinds("re:invent"), None);
    assert_eq!(kinds("mail browser:"), None);
    assert_eq!(kinds("browser"), None);
    assert_eq!(kinds("x1: y"), None);
}

#[test]
fn every_filter_has_an_empty_message() {
    for word in [
        "window:",
        "explorer:",
        "browser:",
        "web:",
        "terminal:",
        "tab:",
        "folder:",
        "dir:",
        "gpt:",
        "chatgpt:",
    ] {
        let (q, _) = parse_qualifier(word).unwrap();
        assert!(!q.empty.is_empty(), "{word}");
    }
    assert_eq!(
        parse_qualifier("gpt:").unwrap().0.empty,
        "Type a question to ask ChatGPT"
    );
}
