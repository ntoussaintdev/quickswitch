//! Linux-friendly folder input (plan §5.2, step 11).
//!
//! Path-like input is input that starts with `~`, `./`, an environment
//! variable such as `%APPDATA%`, a drive prefix such as `C:/`, or a UNC prefix
//! such as `//server/share`. Bare words are never paths.
//!
//! Decisions:
//! - `~/x` expands to the Windows home folder (`%USERPROFILE%\x`).
//! - `./x` resolves relative to the home folder too, never the process's
//!   current directory (which is unpredictable for a tray app).
//! - `~otheruser` and WSL-style `/home/...` are not translated.
//! - `//server/share` is a Windows UNC path, not a WSL path.
//! - `%NAME%` is expanded only at the start of the input (case-insensitive, as
//!   Windows does), and must expand to a drive or UNC path. Elsewhere `%` is a
//!   legal folder-name character and is kept literally.

use std::path::{Path, PathBuf};

/// Characters that can never appear in a Windows path component.
const INVALID_CHARS: &[char] = &['<', '>', '"', '|', '?', '*'];

/// Outcome of interpreting path-like input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathCheck {
    /// Resolves to an existing directory.
    Directory(PathBuf),
    /// Well-formed, but nothing exists there.
    NotFound(PathBuf),
    /// Exists, but is a file rather than a folder.
    NotADirectory(PathBuf),
    /// Looks like a path but can't be used (bad characters, `~user`, ...).
    Invalid(String),
}

impl PathCheck {
    /// A short message for the palette status line, or `None` for a valid folder.
    pub fn message(&self) -> Option<String> {
        match self {
            PathCheck::Directory(_) => None,
            PathCheck::NotFound(p) => Some(format!("Folder not found: {}", p.display())),
            PathCheck::NotADirectory(p) => Some(format!("Not a folder: {}", p.display())),
            PathCheck::Invalid(msg) => Some(msg.clone()),
        }
    }
}

/// True when the input should be treated as a folder path instead of a search.
pub fn looks_like_path(input: &str) -> bool {
    let s = strip_quotes(input.trim());
    s.starts_with('~')
        || s == "."
        || s.starts_with("./")
        || s.starts_with(".\\")
        || env_prefix(s).is_some()
        || is_unc(s)
        || drive_prefix_len(s).is_some()
}

/// Resolve path-like input to an absolute, normalized Windows path.
///
/// Returns `None` when the input is not path-like, and `Some(Err(message))`
/// when it is path-like but malformed. Does not touch the filesystem.
pub fn resolve(input: &str, home: &Path) -> Option<Result<PathBuf, String>> {
    let s = strip_quotes(input.trim());
    if !looks_like_path(s) {
        return None;
    }
    Some(resolve_inner(s, home))
}

fn resolve_inner(s: &str, home: &Path) -> Result<PathBuf, String> {
    if let Some((name, rest)) = env_prefix(s) {
        let value = std::env::var(name).map_err(|_| format!("Unknown variable %{name}%"))?;
        let value = value.trim();
        if !(is_unc(value) || drive_prefix_len(value).is_some()) {
            return Err(format!("%{name}% isn't a folder path"));
        }
        return resolve_inner(&format!("{value}{rest}"), home);
    }
    if let Some(rest) = s.strip_prefix('~') {
        if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')) {
            return Err("Only ~/ (your own home folder) is supported".into());
        }
        return join_relative(home, rest);
    }
    if s == "." {
        return Ok(home.to_path_buf());
    }
    if let Some(rest) = s.strip_prefix('.') {
        return join_relative(home, rest);
    }
    if is_unc(s) {
        let body = s[2..].replace('/', "\\");
        let parts: Vec<&str> = body.split('\\').filter(|p| !p.is_empty()).collect();
        check_components(&parts)?;
        let parts = normalize_parts(&parts);
        if parts.len() < 2 {
            return Err("UNC paths need a server and a share: //server/share".into());
        }
        return Ok(PathBuf::from(format!("\\\\{}", parts.join("\\"))));
    }
    if let Some(n) = drive_prefix_len(s) {
        let drive = s[..1].to_ascii_uppercase();
        let rest = &s[n..];
        let parts: Vec<&str> = rest.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        check_components(&parts)?;
        let parts = normalize_parts(&parts);
        return Ok(PathBuf::from(format!("{drive}:\\{}", parts.join("\\"))));
    }
    Err("Not a recognised path".into())
}

/// The existing file that path-like input names, if it names one.
/// Network (UNC) paths are not checked: a slow server could stall typing.
pub fn file(input: &str, home: &Path) -> Option<PathBuf> {
    let path = resolve(input, home)?.ok()?;
    if is_unc(&path.to_string_lossy()) {
        return None;
    }
    std::fs::metadata(&path).ok()?.is_file().then_some(path)
}

/// Resolve and check the filesystem. `None` when the input isn't path-like.
pub fn check(input: &str, home: &Path) -> Option<PathCheck> {
    Some(match resolve(input, home)? {
        Err(msg) => PathCheck::Invalid(msg),
        Ok(path) => match std::fs::metadata(&path) {
            Ok(m) if m.is_dir() => PathCheck::Directory(path),
            Ok(_) => PathCheck::NotADirectory(path),
            Err(_) => PathCheck::NotFound(path),
        },
    })
}

fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].trim()
    } else {
        s
    }
}

/// Split a leading `%NAME%` off the input: `("NAME", rest)`. The rest must be
/// empty or start with a separator, so `%a%b` is not a variable prefix.
fn env_prefix(s: &str) -> Option<(&str, &str)> {
    let body = s.strip_prefix('%')?;
    let end = body.find('%')?;
    let (name, rest) = (&body[..end], &body[end + 1..]);
    let name_ok = !name.is_empty() && !name.contains(['/', '\\', '=']) && !name.chars().any(char::is_whitespace);
    let rest_ok = rest.is_empty() || rest.starts_with(['/', '\\']);
    (name_ok && rest_ok).then_some((name, rest))
}

fn is_unc(s: &str) -> bool {
    s.starts_with("//") || s.starts_with("\\\\")
}

/// Length of a `C:` / `C:/` / `C:\` prefix, if present.
fn drive_prefix_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        match b.get(2) {
            None => Some(2),
            Some(b'/') | Some(b'\\') => Some(3),
            _ => None,
        }
    } else {
        None
    }
}

fn join_relative(base: &Path, rest: &str) -> Result<PathBuf, String> {
    let parts: Vec<&str> = rest.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    check_components(&parts)?;
    let mut out = base.to_path_buf();
    for p in parts {
        match p {
            "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn check_components(parts: &[&str]) -> Result<(), String> {
    for p in parts {
        if p.contains(INVALID_CHARS) || p.contains(':') || p.chars().any(char::is_control) {
            return Err(format!("Invalid character in path segment “{p}”"));
        }
    }
    Ok(())
}

/// Apply `.` and `..` lexically without climbing above the root.
fn normalize_parts<'a>(parts: &[&'a str]) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for p in parts {
        match *p {
            "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Path input split for suggestions: list `dir`, keep names matching `partial`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Existing-or-not folder whose subfolders are suggested.
    pub dir: PathBuf,
    /// Text after the last separator ("" right after a `/`).
    pub partial: String,
    /// Input up to and including the last separator, as typed. A suggestion
    /// completes to `prefix + name + sep`.
    pub prefix: String,
    /// Separator style the user typed (`/` unless they only used `\`).
    pub sep: char,
}

impl Completion {
    /// Query text that Tab completes to for the subfolder `name`.
    pub fn complete(&self, name: &str) -> String {
        format!("{}{}{}", self.prefix, name, self.sep)
    }
}

/// Split path-like input into a folder to list and a partial name.
///
/// `None` when the input is not path-like; `Some(Err)` when it is malformed.
/// Network (UNC) input is never split: listing a slow server on every
/// keystroke could stall typing.
pub fn completion(input: &str, home: &Path) -> Option<Result<Completion, String>> {
    let s = strip_quotes(input.trim());
    if !looks_like_path(s) {
        return None;
    }
    if is_unc(s) {
        return Some(Err(UNC_NOT_LISTED.into()));
    }
    let sep = if s.contains('\\') && !s.contains('/') {
        '\\'
    } else {
        '/'
    };
    // Input that names a folder as a whole: "~", ".", "C:", "..", "~/x/.."
    let last = s.rsplit(['/', '\\']).next().unwrap_or(s);
    let whole = s == "~"
        || s == "."
        || (s.len() == 2 && drive_prefix_len(s) == Some(2))
        || env_prefix(s).is_some_and(|(_, rest)| rest.is_empty())
        || last == "."
        || last == "..";
    let (prefix, partial) = if whole {
        (format!("{s}{sep}"), String::new())
    } else {
        match s.rfind(['/', '\\']) {
            Some(i) => (s[..=i].to_string(), s[i + 1..].to_string()),
            None => {
                return Some(Err(match resolve_inner(s, home) {
                    Err(e) => e,
                    Ok(_) => "Not a recognised path".into(),
                }));
            }
        }
    };
    if partial.contains(INVALID_CHARS) || partial.contains(':') {
        return Some(Err(format!("Invalid character in “{partial}”")));
    }
    let dir = match resolve_inner(&prefix, home) {
        Ok(dir) => dir,
        Err(e) => return Some(Err(e)),
    };
    // A variable can expand to a network path; don't list those either.
    if is_unc(&dir.to_string_lossy()) {
        return Some(Err(UNC_NOT_LISTED.into()));
    }
    Some(Ok(Completion {
        dir,
        partial,
        prefix,
        sep,
    }))
}

/// Marker error from [`completion`] for network paths.
pub const UNC_NOT_LISTED: &str = "unc";

/// A subfolder found while listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    /// Hidden or system folder (shown only when its name is typed).
    pub hidden: bool,
}

/// Subfolders of `dir`, sorted by name (case-insensitive). Capped so a huge
/// folder can't stall the popup.
pub fn list_dirs(dir: &Path) -> std::io::Result<Vec<DirEntry>> {
    const MAX_ENTRIES: usize = 5_000;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten().take(MAX_ENTRIES) {
        let Ok(ft) = entry.file_type() else { continue };
        let is_dir = ft.is_dir() || (ft.is_symlink() && entry.path().is_dir());
        if !is_dir {
            continue;
        }
        let hidden = entry.metadata().map(|m| is_hidden(&m)).unwrap_or(false);
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            hidden,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    Ok(out)
}

#[cfg(windows)]
fn is_hidden(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    m.file_attributes() & (HIDDEN | SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden(_m: &std::fs::Metadata) -> bool {
    false
}
