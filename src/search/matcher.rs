//! Ranking and match highlighting.
//!
//! The query is split into words (on spaces and `/` or `\`). Every word must
//! match, in any order. A word matches one of two ways:
//! - as a contiguous substring of the searched text, or
//! - fuzzily (letters in order, gaps allowed) *within a single word or folder
//!   name*. Fuzzy matches never jump across words, so "labour" can't match
//!   "pLAns ... taB Of ... cURrent".
//!
//! Windows are matched on their title and app name. Explorer tabs and folders
//! are matched on their folder path only, with a bonus for the last folder.

use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::{MatchOn, SearchItem};

const EXACT_SEGMENT: u32 = 3_000;
const SEGMENT_PREFIX: u32 = 2_000;
const SUBSTRING: u32 = 1_200;
/// Bonus when a path word matches the last folder (the tab's own folder), or
/// a title word is the whole part after the title's last `/` ("owner/repo").
const LAST_SEGMENT: u32 = 600;
/// Bonus when a window word matches the title rather than the app name.
const IN_TITLE: u32 = 300;
/// Bonus when the whole query is a prefix of the title.
const TITLE_PREFIX: u32 = 1_000;
/// Bonus for the most recent pick; it falls off linearly with age.
const RECENT_BONUS: u32 = 600;

/// One ranked result: index into the input and highlighted char indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub index: usize,
    pub title_hl: Vec<usize>,
    pub subtitle_hl: Vec<usize>,
}

/// Reusable ranker (keeps the matcher's scratch memory between keystrokes).
pub struct Ranker {
    matcher: Matcher,
    buf: Vec<char>,
    indices: Vec<u32>,
}

impl Default for Ranker {
    fn default() -> Self {
        Ranker {
            matcher: Matcher::new(Config::DEFAULT),
            buf: Vec::new(),
            indices: Vec::new(),
        }
    }
}

/// Split a query into the words that must all match.
pub fn query_words(query: &str) -> Vec<String> {
    query
        .split(|c: char| c.is_whitespace() || c == '/' || c == '\\')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Lower-case one char at a time, so char indices stay aligned.
fn lower_chars(s: &str) -> Vec<char> {
    s.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// Segments of a text as (start, end) char ranges.
fn segments(chars: &[char], path: bool) -> Vec<(usize, usize)> {
    let is_sep = |c: char| {
        if path {
            c == '/' || c == '\\'
        } else {
            c.is_whitespace()
                || matches!(
                    c,
                    '-' | '_' | '.' | '/' | '\\' | '|' | ',' | '(' | ')' | '[' | ']' | ':'
                )
        }
    };
    let mut out = Vec::new();
    let mut start = None;
    for (i, &c) in chars.iter().enumerate() {
        match (is_sep(c), start) {
            (true, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, chars.len()));
    }
    out
}

fn find(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Best match of one word in one field. Returns (score, char indices, segment index).
struct FieldMatch {
    score: u32,
    indices: Vec<usize>,
    segment: usize,
}

impl Ranker {
    fn match_word(&mut self, word: &str, atom: &Atom, text: &str, path: bool) -> Option<FieldMatch> {
        let chars = lower_chars(text);
        let needle = lower_chars(word);
        let segs = segments(&chars, path);
        let seg_of = |pos: usize| segs.iter().position(|&(s, e)| pos >= s && pos < e).unwrap_or(0);

        // Contiguous matches: prefer one that starts a segment.
        let mut best: Option<FieldMatch> = None;
        let mut from = 0;
        while let Some(off) = find(&chars[from..], &needle) {
            let pos = from + off;
            let seg = seg_of(pos);
            let (s, e) = segs.get(seg).copied().unwrap_or((0, chars.len()));
            let score = if pos == s && pos + needle.len() == e {
                EXACT_SEGMENT
            } else if pos == s {
                SEGMENT_PREFIX
            } else {
                SUBSTRING
            };
            if best
                .as_ref()
                .is_none_or(|b| score > b.score || (score == b.score && path && seg > b.segment))
            {
                best = Some(FieldMatch {
                    score,
                    indices: (pos..pos + needle.len()).collect(),
                    segment: seg,
                });
            }
            from = pos + 1;
        }
        if best.is_some() {
            return best;
        }

        // Fuzzy, within one segment at a time.
        for (si, &(s, e)) in segs.iter().enumerate() {
            let seg_text: String = text.chars().skip(s).take(e - s).collect();
            self.indices.clear();
            let hay = Utf32Str::new(&seg_text, &mut self.buf);
            if let Some(score) = atom.indices(hay, &mut self.matcher, &mut self.indices) {
                let score = u32::from(score);
                if best
                    .as_ref()
                    .is_none_or(|b| score > b.score || (score == b.score && path && si > b.segment))
                {
                    best = Some(FieldMatch {
                        score,
                        indices: self.indices.iter().map(|&i| s + i as usize).collect(),
                        segment: si,
                    });
                }
            }
        }
        best
    }

    /// Score one item against the query words. `None` when any word misses.
    fn score_item(&mut self, item: &SearchItem, words: &[(String, Atom)]) -> Option<(u32, Vec<usize>, Vec<usize>)> {
        let mut total = 0u32;
        let mut title_hl = Vec::new();
        let mut subtitle_hl = Vec::new();
        for (word, atom) in words {
            match item.match_on {
                MatchOn::Path => {
                    let m = self.match_word(word, atom, &item.subtitle, true)?;
                    let last = segments(&lower_chars(&item.subtitle), true).len().saturating_sub(1);
                    total += m.score + if m.segment == last { LAST_SEGMENT } else { 0 };
                    subtitle_hl.extend(m.indices);
                }
                MatchOn::Text => {
                    let t = self.match_word(word, atom, &item.title, false);
                    let s = self.match_word(word, atom, &item.subtitle, false);
                    match (t, s) {
                        (Some(t), s) if s.as_ref().is_none_or(|s| t.score + IN_TITLE >= s.score) => {
                            total += t.score + IN_TITLE;
                            if t.score == EXACT_SEGMENT && after_last_slash(&item.title, &t.indices) {
                                total += LAST_SEGMENT;
                            }
                            title_hl.extend(t.indices);
                        }
                        (_, Some(s)) => {
                            total += s.score;
                            subtitle_hl.extend(s.indices);
                        }
                        _ => return None,
                    }
                }
            }
        }
        if item.match_on == MatchOn::Path {
            title_hl = title_highlight(&item.title, &item.subtitle, &subtitle_hl);
        }
        title_hl.sort_unstable();
        title_hl.dedup();
        subtitle_hl.sort_unstable();
        subtitle_hl.dedup();
        Some((total, title_hl, subtitle_hl))
    }

    /// Ranked hits, best first, at most `limit`.
    ///
    /// `recent` holds `recent_key`s, most recent first. With an empty query
    /// every item is returned: recent picks first, then the original order
    /// (window Z-order, which is roughly "most recently used").
    pub fn rank(&mut self, items: &[SearchItem], query: &str, recent: &[String], limit: usize) -> Vec<Hit> {
        let recent_bonus = |item: &SearchItem| -> u32 {
            recent
                .iter()
                .position(|k| *k == item.recent_key)
                .map(|pos| RECENT_BONUS.saturating_sub(pos as u32 * (RECENT_BONUS / 20)))
                .unwrap_or(0)
        };
        let words: Vec<(String, Atom)> = query_words(query)
            .into_iter()
            .map(|w| {
                let atom = Atom::new(&w, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy, false);
                (w, atom)
            })
            .collect();
        let q = query.trim().to_lowercase();

        let mut scored: Vec<(u32, Hit)> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if words.is_empty() {
                scored.push((
                    recent_bonus(item),
                    Hit {
                        index,
                        title_hl: Vec::new(),
                        subtitle_hl: Vec::new(),
                    },
                ));
                continue;
            }
            let Some((mut score, title_hl, subtitle_hl)) = self.score_item(item, &words) else {
                continue;
            };
            if item.title.to_lowercase().starts_with(&q) {
                score += TITLE_PREFIX;
            }
            scored.push((
                score + recent_bonus(item),
                Hit {
                    index,
                    title_hl,
                    subtitle_hl,
                },
            ));
        }
        // Highest score first; ties keep the original (Z-order) position.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.index.cmp(&b.1.index)));
        scored.into_iter().take(limit).map(|(_, h)| h).collect()
    }

    /// Highlight `word` in a single name (folder suggestions). Returns the
    /// score and char indices, or `None` if it doesn't match.
    pub fn match_name(&mut self, name: &str, word: &str) -> Option<(u32, Vec<usize>)> {
        if word.is_empty() {
            return Some((0, Vec::new()));
        }
        let atom = Atom::new(word, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy, false);
        // A name is one segment: treat it as a path with no separators.
        self.match_word(word, &atom, name, true).map(|m| (m.score, m.indices))
    }
}

/// Whether a match starts right after the title's last `/`, as "repo" does in
/// "Releases · owner/repo" or "GitHub - owner/repo: about". That word names
/// the thing, like a folder's last name, so it gets the same bonus.
fn after_last_slash(title: &str, indices: &[usize]) -> bool {
    let Some(&start) = indices.first() else {
        return false;
    };
    let chars: Vec<char> = title.chars().collect();
    start > 0 && chars.get(start - 1) == Some(&'/') && !chars[start..].contains(&'/')
}

/// For path items the title is the last folder name: copy the highlights
/// that fall in the subtitle's last segment over to the title.
fn title_highlight(title: &str, subtitle: &str, subtitle_hl: &[usize]) -> Vec<usize> {
    let chars: Vec<char> = subtitle.chars().collect();
    let Some(&(s, e)) = segments(&chars, true).last() else {
        return Vec::new();
    };
    let last: String = chars[s..e].iter().collect();
    if !last.eq_ignore_ascii_case(title) {
        return Vec::new();
    }
    subtitle_hl
        .iter()
        .filter(|&&i| i >= s && i < e)
        .map(|&i| i - s)
        .collect()
}
