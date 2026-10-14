//! Which of her profile items fit this request best (BM25, the standard ranking of search
//! engines). A profile can hold dozens of templates and phrases per section; sending them all
//! makes the request long and the model's attention thin. The query is this case's text for the
//! section (already filtered, on her computer); the items closest to it are kept within a
//! budget, and the rest keep their order. Nothing is sent by this module.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::text::{has, words, FUNCTION_SET, GENERIC};

/// BM25 constants (the usual values).
const K1: f64 = 1.2;
const B: f64 = 0.75;

/// The words that carry meaning: no tags, numbers, small or generic words. Each word also
/// counts without its prefix letters, so forms meet.
fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in words(text) {
        if w.is_masked() || has(&FUNCTION_SET, &w.norm) || has(&GENERIC, &w.norm) {
            continue;
        }
        // Up to two prefix letters ("ובאכילה" → "באכילה" → "אכילה").
        let mut rest = w.norm.as_str();
        for _ in 0..2 {
            let mut chars = rest.chars();
            match chars.next() {
                Some('ו' | 'ה' | 'ש' | 'ב' | 'ל' | 'כ' | 'מ')
                    if chars.as_str().chars().count() >= 3 =>
                {
                    rest = chars.as_str();
                    out.push(rest.to_owned());
                }
                _ => break,
            }
        }
        if w.norm.chars().count() >= 2 {
            out.push(w.norm);
        }
    }
    out
}

/// BM25 score of each document against `query`, in the documents' order.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn bm25(docs: &[&str], query: &str) -> Vec<f64> {
    let docs: Vec<Vec<String>> = docs.iter().map(|d| terms(d)).collect();
    let n = docs.len() as f64;
    if docs.is_empty() {
        return Vec::new();
    }
    let avg = (docs.iter().map(Vec::len).sum::<usize>() as f64 / n).max(1.0);
    let mut df: HashMap<&str, f64> = HashMap::new();
    for d in &docs {
        let unique: HashSet<&str> = d.iter().map(String::as_str).collect();
        for t in unique {
            *df.entry(t).or_default() += 1.0;
        }
    }
    // Ordered, so each score is summed in the same order every time: a `HashSet` changes its
    // order from one call to the next, and the float sum with it (in the last bit), which could
    // break a tie between two items differently from run to run.
    let q: BTreeSet<String> = terms(query).into_iter().collect();
    docs.iter()
        .map(|d| {
            let len = d.len() as f64;
            q.iter()
                .filter_map(|t| {
                    let f = d.iter().filter(|w| *w == t).count() as f64;
                    if f == 0.0 {
                        return None;
                    }
                    let df = df.get(t.as_str()).copied().unwrap_or(0.0);
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    Some(idf * f * (K1 + 1.0) / (f + K1 * (1.0 - B + B * len / avg)))
                })
                .sum()
        })
        .collect()
}

/// The `budget` documents that fit `query` best, as indexes in their original order. Ties (and
/// documents that share no word with the query) keep the order they came in, which is already
/// her order of importance.
#[must_use]
pub fn best(docs: &[&str], query: &str, budget: usize) -> Vec<usize> {
    if docs.len() <= budget {
        return (0..docs.len()).collect();
    }
    let mut order: Vec<(usize, f64)> = bm25(docs, query).into_iter().enumerate().collect();
    order.sort_by(|(a, sa), (b, sb)| sb.total_cmp(sa).then(a.cmp(b)));
    order.truncate(budget);
    let mut order: Vec<usize> = order.into_iter().map(|(i, _)| i).collect();
    order.sort_unstable();
    order
}

/// The passages of a long material that bear on `query`, within `words` words (G4): the best
/// first, then kept in their order. A passage that shares no word with the query is left out;
/// when none shares a word, all are kept (nothing to go by).
#[must_use]
pub fn within_words(passages: &[&str], query: &str, words: usize) -> Vec<usize> {
    let scores = bm25(passages, query);
    if scores.iter().all(|s| *s <= 0.0) {
        return (0..passages.len()).collect();
    }
    let mut order: Vec<(usize, f64, &str)> = scores
        .into_iter()
        .zip(passages)
        .enumerate()
        .filter(|(_, (s, _))| *s > 0.0)
        .map(|(i, (s, p))| (i, s, *p))
        .collect();
    order.sort_by(|(a, sa, _), (b, sb, _)| sb.total_cmp(sa).then(a.cmp(b)));
    let mut used = 0;
    let mut out = Vec::new();
    for (i, _, passage) in order {
        let n = passage.split_whitespace().count();
        if !out.is_empty() && used + n > words {
            continue;
        }
        used += n;
        out.push(i);
    }
    out.sort_unstable();
    out
}

/// A paragraph of her reports is shown next to a draft only with at least this many words.
const MIN_SIMILAR_WORDS: usize = 8;
/// A paragraph from the same section scores this much more.
const SAME_SECTION: f64 = 1.5;

/// The paragraph of her reports closest to `query` (ST2): BM25, a paragraph of the same
/// section counted [`SAME_SECTION`] times, and none that shares no word with it or is too
/// short to show. `docs` are (section, paragraph).
#[must_use]
pub fn closest(docs: &[(Option<&str>, &str)], section: &str, query: &str) -> Option<usize> {
    let texts: Vec<&str> = docs.iter().map(|(_, t)| *t).collect();
    let scores = bm25(&texts, query);
    docs.iter()
        .zip(scores)
        .enumerate()
        .filter(|(_, ((_, t), score))| {
            *score > 0.0 && crate::text::words(t).len() >= MIN_SIMILAR_WORDS
        })
        .map(|(i, ((s, _), score))| {
            (
                i,
                if *s == Some(section) {
                    score * SAME_SECTION
                } else {
                    score
                },
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_closest_paragraph_prefers_its_section_and_needs_a_shared_word() {
        let docs = [
            (
                Some("play"),
                "הילד בנה מגדל קוביות גבוה ושמח מאוד כשהמגדל עמד יציב על השולחן.",
            ),
            (
                Some("language"),
                "השפה של הילד עשירה והוא סיפר על המגדל שבנה מקוביות בבית.",
            ),
            (Some("play"), "קצר מדי."),
        ];
        assert_eq!(closest(&docs, "play", "מגדל קוביות"), Some(0));
        assert_eq!(closest(&docs, "language", "סיפר על המגדל"), Some(1));
        assert_eq!(closest(&docs, "play", "שום מילה משותפת"), None);
        assert_eq!(closest(&[], "play", "מגדל"), None);
    }

    use super::*;

    #[test]
    fn the_item_about_the_topic_ranks_first() {
        let docs = [
            "בתחום המוטורי ניכר קושי בגזירה ובציור.",
            "בתחום השפתי הילד משתמש במשפטים מורכבים.",
            "במשחק החברתי הוא יוזם ומשתף.",
        ];
        let s = bm25(&docs, "הגננת מספרת שהוא נמנע מציור ומגזירה בגן");
        assert!(s[0] > s[1] && s[0] > s[2], "{s:?}");
    }

    #[test]
    fn the_budget_keeps_the_best_in_their_order() {
        let docs = ["משחק בובות", "ציור וגזירה", "שפה ודיבור", "ריצה וקפיצה"];
        assert_eq!(best(&docs, "דיבור ומשחק", 2), vec![0, 2]);
        assert_eq!(best(&docs, "", 2), vec![0, 1], "no query: her order");
        assert_eq!(best(&docs, "ציור", 10), vec![0, 1, 2, 3]);
    }

    #[test]
    fn tags_and_numbers_never_match() {
        let s = bm25(&["[ילד] קיבל 12"], "[ילד] 12");
        assert!(s[0].abs() < f64::EPSILON);
    }

    #[test]
    fn a_long_material_keeps_what_bears_on_the_section_within_the_budget() {
        let passages = [
            "בגן הוא משחק עם חברים ומשתף בצעצועים.",
            "ההריון והלידה עברו ללא סיבוכים מיוחדים.",
            "בחצר הגן הוא מעדיף משחק ליד חברים ולא איתם.",
            "הוא אוכל מגוון מצומצם של מאכלים.",
        ];
        assert_eq!(within_words(&passages, "גן חברים משחק", 100), vec![0, 2]);
        let one = within_words(&passages, "גן חברים משחק", 8);
        assert!(
            one == vec![0] || one == vec![2],
            "the best one only: {one:?}"
        );
        assert_eq!(within_words(&passages, "מטוס", 100), vec![0, 1, 2, 3]);
    }
}
