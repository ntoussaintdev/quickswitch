//! Web search and website detection for the web row, `web:`, `gpt:` and
//! Ctrl+Enter. Plain string handling, unit tested; opening the URL is
//! `actions::open_url`.

/// A search engine or AI assistant the query can be sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineSpec {
    /// Stored in `config.toml` (`search_engine`).
    pub id: &'static str,
    pub name: &'static str,
    /// Row text before the quoted query: "Search Google for".
    pub action: &'static str,
    /// `{}` is replaced by the encoded query.
    pub url: &'static str,
}

pub const DEFAULT_ENGINE: &str = "google";
/// `search_engine` value that uses `custom_search_url`.
pub const CUSTOM_ENGINE: &str = "custom";

pub const ENGINES: &[EngineSpec] = &[
    EngineSpec {
        id: "google",
        name: "Google",
        action: "Search Google for",
        url: "https://www.google.com/search?q={}",
    },
    EngineSpec {
        id: "bing",
        name: "Bing",
        action: "Search Bing for",
        url: "https://www.bing.com/search?q={}",
    },
    EngineSpec {
        id: "duckduckgo",
        name: "DuckDuckGo",
        action: "Search DuckDuckGo for",
        url: "https://duckduckgo.com/?q={}",
    },
    EngineSpec {
        id: "brave",
        name: "Brave Search",
        action: "Search Brave for",
        url: "https://search.brave.com/search?q={}",
    },
    EngineSpec {
        id: "ecosia",
        name: "Ecosia",
        action: "Search Ecosia for",
        url: "https://www.ecosia.org/search?q={}",
    },
    EngineSpec {
        id: "startpage",
        name: "Startpage",
        action: "Search Startpage for",
        url: "https://www.startpage.com/do/search?q={}",
    },
    EngineSpec {
        id: "kagi",
        name: "Kagi",
        action: "Search Kagi for",
        url: "https://kagi.com/search?q={}",
    },
    EngineSpec {
        id: "yahoo",
        name: "Yahoo",
        action: "Search Yahoo for",
        url: "https://search.yahoo.com/search?p={}",
    },
    EngineSpec {
        id: "perplexity",
        name: "Perplexity",
        action: "Ask Perplexity",
        url: "https://www.perplexity.ai/search?q={}",
    },
    EngineSpec {
        id: "chatgpt",
        name: "ChatGPT",
        action: "Ask ChatGPT",
        url: "https://chatgpt.com/?q={}",
    },
    EngineSpec {
        id: "claude",
        name: "Claude",
        action: "Ask Claude",
        url: "https://claude.ai/new?q={}",
    },
];

/// The engine `gpt:` always uses.
pub fn chatgpt() -> Engine {
    Engine::from_spec(ENGINES.iter().find(|e| e.id == "chatgpt").expect("chatgpt engine"))
}

/// The engine in use, resolved from the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    pub name: String,
    pub action: String,
    pub url: String,
}

impl Engine {
    fn from_spec(s: &EngineSpec) -> Engine {
        Engine {
            name: s.name.into(),
            action: s.action.into(),
            url: s.url.into(),
        }
    }

    /// `id` from the settings; unknown ids and an invalid custom URL fall
    /// back to Google.
    pub fn from_settings(id: &str, custom_url: &str) -> Engine {
        if id == CUSTOM_ENGINE && check_custom_url(custom_url).is_ok() {
            return Engine {
                name: "Custom search".into(),
                action: "Search for".into(),
                url: custom_url.trim().into(),
            };
        }
        let spec = ENGINES.iter().find(|e| e.id == id).unwrap_or(&ENGINES[0]);
        Engine::from_spec(spec)
    }

    /// The URL that searches for `text`.
    pub fn url_for(&self, text: &str) -> String {
        self.url.replace("{}", &encode_query(text.trim()))
    }
}

/// A custom search URL must be http(s) and contain `{}` for the query.
pub fn check_custom_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("The address must start with https:// or http://".into());
    }
    if !u.contains("{}") {
        return Err("Put {} where the search text goes, e.g. https://example.com/?q={}".into());
    }
    if u.chars().any(char::is_whitespace) {
        return Err("The address can't contain spaces.".into());
    }
    Ok(())
}

/// Percent-encode for a URL query value (UTF-8; unreserved characters kept).
pub fn encode_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Top-level domains accepted without a protocol or `www.`. Ones that are
/// also common file extensions are left out on purpose (md, rs, py, sh, pl,
/// ml, zip, mov), so `README.md` or `app.rs` are searched, not visited.
const TLDS: &[&str] = &[
    "com", "org", "net", "edu", "gov", "mil", "int", "io", "dev", "app", "co", "ai", "me", "info", "biz", "xyz",
    "tech", "site", "online", "store", "shop", "blog", "cloud", "page", "news", "wiki", "tv", "fm", "gg", "to", "ly",
    "us", "uk", "ca", "de", "fr", "nl", "eu", "es", "it", "ch", "se", "no", "dk", "fi", "at", "be", "ie", "pt", "cz",
    "jp", "cn", "kr", "in", "br", "mx", "ar", "au", "nz", "za", "ru", "ua", "tr", "il", "sg", "hk", "tw",
];

/// The URL to open when `text` is clearly a website: it has a protocol
/// (`https://`, `ftp://`, ...), starts with `www.`, is `localhost` or an IP
/// address with a port or path, or is a host name ending in a known
/// top-level domain (`github.com/x`). Anything with spaces is not.
pub fn website_url(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some(i) = t.find("://") {
        let scheme = &t[..i];
        let ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
            && t.len() > i + 3;
        return ok.then(|| t.to_string());
    }
    if t.to_ascii_lowercase().starts_with("www.") && t.len() > 4 {
        return Some(format!("https://{t}"));
    }

    let host_end = t.find(['/', '?', '#']).unwrap_or(t.len());
    let host_port = &t[..host_end];
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host_port, None),
    };
    if let Some(p) = port
        && (p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let has_more = port.is_some() || host_end < t.len();
    if host.eq_ignore_ascii_case("localhost") || is_ipv4(host) {
        return has_more.then(|| format!("http://{t}"));
    }

    let labels: Vec<&str> = host.split('.').collect();
    let valid_labels = labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    let tld = labels.last().map(|l| l.to_ascii_lowercase()).unwrap_or_default();
    (valid_labels && TLDS.contains(&tld.as_str())).then(|| format!("https://{t}"))
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u8>().is_ok())
}
