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
    let scores = bm25(docs, query);
    let mut order: Vec<usize> = (0..docs.len()).collect();
    order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
    order.truncate(budget);
    order.sort_unstable();
    order
}

#[cfg(test)]
mod tests {
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
}
