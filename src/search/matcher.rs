//! Ranking (plan §5.2): prefix and contiguous matches first, then fuzzy
//! matches, with a bonus for recently picked results.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::SearchItem;

const PREFIX_BONUS: u32 = 3_000;
const WORD_PREFIX_BONUS: u32 = 2_000;
const CONTAINS_BONUS: u32 = 1_500;
/// Bonus for the most recent pick; it falls off linearly with age.
const RECENT_BONUS: u32 = 600;

/// Reusable ranker (keeps the matcher's scratch memory between keystrokes).
pub struct Ranker {
    matcher: Matcher,
    buf: Vec<char>,
}

impl Default for Ranker {
    fn default() -> Self {
        Ranker {
            matcher: Matcher::new(Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl Ranker {
    /// Indices into `items`, best first, at most `limit`.
    ///
    /// `recent` holds `recent_key`s, most recent first. With an empty query
    /// every item is returned: recent picks first, then the original order
    /// (window Z-order, which is roughly "most recently used").
    pub fn rank(&mut self, items: &[SearchItem], query: &str, recent: &[String], limit: usize) -> Vec<usize> {
        let query = query.trim();
        let recent_bonus = |item: &SearchItem| -> u32 {
            recent
                .iter()
                .position(|k| *k == item.recent_key)
                .map(|pos| RECENT_BONUS.saturating_sub(pos as u32 * (RECENT_BONUS / 20)))
                .unwrap_or(0)
        };

        let mut scored: Vec<(u32, usize)> = if query.is_empty() {
            items.iter().enumerate().map(|(i, it)| (recent_bonus(it), i)).collect()
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let q = query.to_lowercase();
            items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| {
                    let title = it.title.to_lowercase();
                    let mut score = 0u32;
                    if title.starts_with(&q) {
                        score += PREFIX_BONUS;
                    } else if title
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|w| !w.is_empty() && w.starts_with(&q))
                    {
                        score += WORD_PREFIX_BONUS;
                    } else if title.contains(&q) {
                        score += CONTAINS_BONUS;
                    }
                    let hay = format!("{} {}", it.title, it.subtitle);
                    let fuzzy = pattern.score(Utf32Str::new(&hay, &mut self.buf), &mut self.matcher);
                    match fuzzy {
                        Some(f) => score += f,
                        None if score == 0 => return None,
                        None => {}
                    }
                    Some((score + recent_bonus(it), i))
                })
                .collect()
        };

        // Highest score first; ties keep the original (Z-order) position.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(limit).map(|(_, i)| i).collect()
    }
}
