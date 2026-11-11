//! Web search row: website detection, search URLs, engines, gpt:.

use quickswitch::actions::web::{
    CUSTOM_ENGINE, DEFAULT_ENGINE, ENGINES, Engine, check_custom_url, encode_query, website_url,
};
use quickswitch::search::{Kind, Target, WebMode, parse_qualifier, web_item};

fn url_of(item: &quickswitch::search::SearchItem) -> &str {
    match &item.target {
        Target::Web { url } => url,
        other => panic!("not a web row: {other:?}"),
    }
}

#[test]
fn websites_are_recognised() {
    let v = |s| website_url(s);
    assert_eq!(
        v("https://example.com/a?b=c").as_deref(),
        Some("https://example.com/a?b=c")
    );
    assert_eq!(v("ftp://files.example.org").as_deref(), Some("ftp://files.example.org"));
    assert_eq!(v("www.example.com").as_deref(), Some("https://www.example.com"));
    assert_eq!(v("github.com").as_deref(), Some("https://github.com"));
    assert_eq!(
        v("github.com/ntoussaintdev/quickswitch").as_deref(),
        Some("https://github.com/ntoussaintdev/quickswitch")
    );
    assert_eq!(v("news.bbc.co.uk").as_deref(), Some("https://news.bbc.co.uk"));
    assert_eq!(v("Example.DEV").as_deref(), Some("https://Example.DEV"));
    assert_eq!(v("localhost:3000").as_deref(), Some("http://localhost:3000"));
    assert_eq!(v("localhost/admin").as_deref(), Some("http://localhost/admin"));
    assert_eq!(v("192.168.1.10:8080").as_deref(), Some("http://192.168.1.10:8080"));
}

#[test]
fn file_names_and_plain_text_are_searched() {
    for text in [
        "README.md",
        "app.rs",
        "setup.py",
        "build.sh",
        "node.js",
        "e.g.",
        "rust uia tabitem",
        "github.com is down",
        "localhost",
        "1.5",
        "999.1.1.1:80",
        "example.com:abc",
        "-bad.com",
        "",
        "   ",
    ] {
        assert_eq!(website_url(text), None, "{text:?}");
    }
}

#[test]
fn queries_are_encoded() {
    assert_eq!(encode_query("rust uia"), "rust%20uia");
    assert_eq!(encode_query("a&b=c?#"), "a%26b%3Dc%3F%23");
    assert_eq!(encode_query("café"), "caf%C3%A9");
    assert_eq!(encode_query("x-y_z.~"), "x-y_z.~");
}

#[test]
fn engines_build_search_urls() {
    let google = Engine::from_settings(DEFAULT_ENGINE, "");
    assert_eq!(google.name, "Google");
    assert_eq!(
        google.url_for(" rust uia "),
        "https://www.google.com/search?q=rust%20uia"
    );
    let ddg = Engine::from_settings("duckduckgo", "");
    assert_eq!(ddg.url_for("x"), "https://duckduckgo.com/?q=x");
    // Unknown ids fall back to Google.
    assert_eq!(Engine::from_settings("altavista", "").name, "Google");
    // Every engine has a {} slot and an https address.
    for e in ENGINES {
        assert!(e.url.contains("{}") && e.url.starts_with("https://"), "{}", e.id);
    }
}

#[test]
fn custom_engine_needs_a_valid_address() {
    let custom = Engine::from_settings(CUSTOM_ENGINE, "https://search.example/?q={}");
    assert_eq!(custom.url_for("a b"), "https://search.example/?q=a%20b");
    // Invalid custom address: back to Google.
    assert_eq!(
        Engine::from_settings(CUSTOM_ENGINE, "search.example/?q=").name,
        "Google"
    );
    assert!(check_custom_url("https://x.example/?q={}").is_ok());
    assert!(check_custom_url("ftp://x.example/?q={}").is_err());
    assert!(check_custom_url("https://x.example/?q=").is_err());
    assert!(check_custom_url("https://x.example/?q={} x").is_err());
}

#[test]
fn web_row_searches_or_visits() {
    let google = Engine::from_settings(DEFAULT_ENGINE, "");
    let row = web_item("rust uia", &google, false).unwrap();
    assert_eq!(row.kind(), Kind::Web);
    assert_eq!(row.title, "Search Google for \u{201c}rust uia\u{201d}");
    assert_eq!(url_of(&row), "https://www.google.com/search?q=rust%20uia");
    assert!(row.recent_key.is_empty());

    let row = web_item("github.com/x", &google, false).unwrap();
    assert_eq!(row.title, "Visit github.com/x");
    assert_eq!(url_of(&row), "https://github.com/x");

    assert!(web_item("   ", &google, false).is_none());
}

#[test]
fn gpt_always_asks_chatgpt() {
    let bing = Engine::from_settings("bing", "");
    let row = web_item("explain lifetimes", &bing, true).unwrap();
    assert_eq!(row.title, "Ask ChatGPT \u{201c}explain lifetimes\u{201d}");
    assert_eq!(url_of(&row), "https://chatgpt.com/?q=explain%20lifetimes");
    // Even text that looks like a website is a question.
    let row = web_item("github.com", &bing, true).unwrap();
    assert!(url_of(&row).starts_with("https://chatgpt.com/?q="));
}

#[test]
fn gpt_and_web_filters() {
    let (q, rest) = parse_qualifier("gpt: why is the sky blue").unwrap();
    assert_eq!(q.web, WebMode::Gpt);
    assert!(q.kinds.is_empty());
    assert_eq!(rest, "why is the sky blue");
    assert_eq!(parse_qualifier("chatgpt: x").unwrap().0.web, WebMode::Gpt);
    assert_eq!(parse_qualifier("web: x").unwrap().0.web, WebMode::First);
    assert_eq!(parse_qualifier("browser: x").unwrap().0.web, WebMode::First);
    assert_eq!(parse_qualifier("window: x").unwrap().0.web, WebMode::None);
}
