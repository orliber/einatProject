//! The lines she writes again and again in a list section (K9, K10): her diagnoses and her
//! recommendations. Each line of the section is one item; lines that differ only a little
//! ("מומלץ על ריפוי בעיסוק" and "מומלץ על טיפול בריפוי בעיסוק") are one item, worded the way
//! she wrote it most often. Only lines without a tag or a number are kept, so nothing about a
//! child is ever offered back to her.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use dv_privacy::text::normalize;

use crate::text::{has, FUNCTION_SET};

/// Two lines are the same item when this share of their words is shared (Jaccard).
const SAME: f64 = 0.6;
/// Shortest and longest line kept.
const MIN_WORDS: usize = 2;
const MAX_CHARS: usize = 300;
/// At most this many items.
const MAX_ITEMS: usize = 40;

/// One item of her list, and the reports it appears in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// As she wrote it most often.
    pub text: String,
    /// The indexes of the reports it appears in, in order.
    pub reports: Vec<usize>,
}

/// A line without its list mark ("-", "•", "1.", "א.", or a number the filter hid).
fn unmarked(line: &str) -> &str {
    let line = line
        .trim()
        .trim_start_matches(['-', '•', '*', '·', '–'])
        .trim();
    if let Some(rest) = line
        .strip_prefix("[מספר].")
        .or_else(|| line.strip_prefix("[מספר])"))
    {
        return rest.trim();
    }
    let mut chars = line.char_indices();
    match (chars.next(), chars.next()) {
        (Some((_, c)), Some((i, '.' | ')'))) if c.is_ascii_digit() || ('א'..='י').contains(&c) => {
            line.get(i + 1..).unwrap_or(line).trim()
        }
        _ => line,
    }
}

/// The words a line is compared by: normalized, without small words, and a word of four
/// letters or more without its prefix letter ("בריפוי" and "ריפוי" are one word here).
fn content(line: &str) -> BTreeSet<String> {
    line.split_whitespace()
        .map(|w| normalize(w.trim_matches(|c: char| !c.is_alphanumeric())))
        .filter(|w| !w.is_empty() && !has(&FUNCTION_SET, w))
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some('ו' | 'ב' | 'ל' | 'ה' | 'כ' | 'מ' | 'ש') if w.chars().count() >= 4 => {
                    chars.as_str().to_owned()
                }
                _ => w,
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// One item while it is gathered: its words, and each wording with the reports it is in.
type Group = (BTreeSet<String>, BTreeMap<String, HashSet<usize>>);

/// The lines of each report's section (`reports[d]`: its text), grouped into items that appear
/// in at least `min_reports` reports, the most widespread first.
#[must_use]
pub fn mine_list(reports: &[&str], min_reports: usize) -> Vec<ListItem> {
    let mut items: Vec<Group> = Vec::new();
    for (d, text) in reports.iter().enumerate() {
        for line in text.split('\n').map(unmarked) {
            let line = line.trim_end_matches(['.', ',', ';']).trim();
            if line.split_whitespace().count() < MIN_WORDS
                || line.chars().count() > MAX_CHARS
                || line.contains(['[', ']'])
                || line.chars().any(|c| c.is_ascii_digit())
            {
                continue;
            }
            let words = content(line);
            if words.is_empty() {
                continue;
            }
            let at = items.iter().position(|(w, _)| jaccard(w, &words) >= SAME);
            let entry = match at {
                Some(i) => match items.get_mut(i) {
                    Some(e) => e,
                    None => continue,
                },
                None => {
                    items.push((words, BTreeMap::new()));
                    match items.last_mut() {
                        Some(e) => e,
                        None => continue,
                    }
                }
            };
            entry.1.entry(format!("{line}.")).or_default().insert(d);
        }
    }
    let mut out: Vec<ListItem> = items
        .into_iter()
        .filter_map(|(_, wordings)| {
            let reports: BTreeSet<usize> = wordings.values().flatten().copied().collect();
            let text = wordings
                .iter()
                .max_by(|a, b| a.1.len().cmp(&b.1.len()).then(b.0.cmp(a.0)))
                .map(|(t, _)| t.clone())?;
            (reports.len() >= min_reports).then(|| ListItem {
                text,
                reports: reports.into_iter().collect(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.reports
            .len()
            .cmp(&a.reports.len())
            .then(a.text.cmp(&b.text))
    });
    out.truncate(MAX_ITEMS);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn her_recommendations_are_grouped_across_reports_and_a_childs_line_is_left_out() {
        let reports = [
            "- מומלץ על ריפוי בעיסוק.\n- מומלץ על הדרכת הורים.\n- מומלץ ש[ילד] ימשיך בגן הנוכחי.",
            "1. מומלץ על טיפול בריפוי בעיסוק.\n2. מומלץ על הדרכת הורים.",
            "• מומלץ על ריפוי בעיסוק\n• מומלץ על המשך מעקב התפתחותי בעוד 6 חודשים.",
            "מומלץ על הדרכת הורים.",
        ];
        let items = mine_list(&reports, 2);
        let texts: Vec<(&str, Vec<usize>)> = items
            .iter()
            .map(|i| (i.text.as_str(), i.reports.clone()))
            .collect();
        assert_eq!(
            texts,
            vec![
                ("מומלץ על הדרכת הורים.", vec![0, 1, 3]),
                ("מומלץ על ריפוי בעיסוק.", vec![0, 1, 2]),
            ]
        );
        assert!(mine_list(&reports, 4).is_empty(), "no line is in all four");
    }

    #[test]
    fn list_marks_are_taken_off() {
        assert_eq!(unmarked("- שורה"), "שורה");
        assert_eq!(unmarked("2. שורה"), "שורה");
        assert_eq!(unmarked("ב) שורה"), "שורה");
        assert_eq!(unmarked("[מספר]. שורה"), "שורה");
        assert_eq!(unmarked("שורה רגילה"), "שורה רגילה");
    }
}
