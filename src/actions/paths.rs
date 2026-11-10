//! Linux-friendly folder input (plan §5.2, step 11).
//!
//! Path-like input is input that starts with `~`, `./`, a drive prefix such as
//! `C:/`, or a UNC prefix such as `//server/share`. Bare words are never paths.
//!
//! Decisions:
//! - `~/x` expands to the Windows home folder (`%USERPROFILE%\x`).
//! - `./x` resolves relative to the home folder too, never the process's
//!   current directory (which is unpredictable for a tray app).
//! - `~otheruser` and WSL-style `/home/...` are not translated.
//! - `//server/share` is a Windows UNC path, not a WSL path.

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
