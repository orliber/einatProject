//! Her fixed wording (K4): a sentence that recurs almost word for word in three or more of her
//! reports is how she presents a test or a section ("ה-WPPSI-IV הוא מבחן…"), not something
//! about one child. It is kept as she wrote it, with an empty place where a score was, and
//! offered to her as a template; nothing goes into a request until she adds it.
//!
//! A sentence with any other tag (a name, a place, a date) or a number outside a score is about
//! a child and is never learned.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::edits::percent;
use crate::phrases::ReportParts;
use crate::text::sentences;

/// Fewest reports a sentence must appear in.
pub const MIN_REPORTS: usize = 3;
/// Shortest wording kept, in words (a slot counts as one).
const MIN_WORDS: usize = 6;
/// Longest, in characters.
const MAX_CHARS: usize = 400;
/// At most this many per section.
const PER_SECTION: usize = 6;

/// A sentence she writes the same way in several reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Formula {
    pub section: Option<String>,
    /// As first written, with `{…}` where a score was.
    pub text: String,
    /// Normalized words, space separated (its identity).
    pub norm: String,
    pub reports: u32,
    /// Share of the reports that have the section, 0 to 100.
    pub share: u32,
}

/// `sentence` with each tag replaced by its slot (`slot` names the slot of a tag, `None`: a
/// tag that may not be learned), or `None` when it may not be learned at all.
fn with_slots(sentence: &str, slot: &dyn Fn(&str) -> Option<&'static str>) -> Option<String> {
    let mut out = String::new();
    let mut rest = sentence;
    while let Some(open) = rest.find('[') {
        let close = rest.get(open..)?.find(']')? + open + 1;
        out.push_str(rest.get(..open)?);
        out.push_str(slot(rest.get(open..close)?)?);
        rest = rest.get(close..)?;
    }
    if rest.contains(']') {
        return None;
    }
    out.push_str(rest);
    // A number outside a tag is a detail of the case.
    (!out.chars().any(|c| c.is_ascii_digit())).then_some(out)
}

/// The identity of a wording: its normalized words, slots kept as they are.
fn norm_of(text: &str) -> String {
    text.split_whitespace()
        .map(|w| {
            if w.starts_with('{') {
                w.to_owned()
            } else {
                dv_privacy::text::normalize(
                    w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '{' || c == '}')),
                )
            }
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// (section, norm) → (as first written, the reports it appears in).
type Seen = BTreeMap<(Option<String>, String), (String, HashSet<usize>)>;

/// Her fixed wordings, most widespread first within each section. `slot` names the empty
/// place a tag becomes (`{תוצאה}`), or refuses the sentence (`None`).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn mine_formulas(
    reports: &[ReportParts<'_>],
    slot: &dyn Fn(&str) -> Option<&'static str>,
) -> Vec<Formula> {
    let mut seen: Seen = BTreeMap::new();
    let mut with_section: BTreeMap<Option<String>, HashSet<usize>> = BTreeMap::new();
    for (d, parts) in reports.iter().enumerate() {
        for (section, text) in parts {
            let section = section.map(str::to_owned);
            with_section.entry(section.clone()).or_default().insert(d);
            for s in sentences(text) {
                let Some(text) = with_slots(s, slot) else {
                    continue;
                };
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let norm = norm_of(&text);
                let n = norm.split(' ').count();
                if n < MIN_WORDS
                    || text.chars().count() > MAX_CHARS
                    || norm.split(' ').all(|w| w.starts_with('{'))
                {
                    continue;
                }
                seen.entry((section.clone(), norm))
                    .or_insert_with(|| (format!("{text}."), HashSet::new()))
                    .1
                    .insert(d);
            }
        }
    }
    let mut out: Vec<Formula> = seen
        .into_iter()
        .filter(|(_, (_, docs))| docs.len() >= MIN_REPORTS)
        .map(|((section, norm), (text, docs))| {
            let total = with_section.get(&section).map_or(1, HashSet::len).max(1);
            Formula {
                share: percent(docs.len() as f64 / total as f64),
                reports: u32::try_from(docs.len()).unwrap_or(u32::MAX),
                section,
                text,
                norm,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.section
            .cmp(&b.section)
            .then(b.reports.cmp(&a.reports))
            .then(a.norm.cmp(&b.norm))
    });
    let mut per: BTreeMap<Option<String>, usize> = BTreeMap::new();
    out.retain(|f| {
        let n = per.entry(f.section.clone()).or_default();
        *n += 1;
        *n <= PER_SECTION
    });
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn slot(tag: &str) -> Option<&'static str> {
        if tag.starts_with("[תוצאה") {
            Some("{תוצאה}")
        } else if tag == "[מספר]" {
            Some("{מספר}")
        } else {
            None
        }
    }

    #[test]
    fn a_sentence_in_three_reports_is_a_formula_and_a_childs_sentence_is_not() {
        let children = [
            "[ילד] בנה מגדל גבוה מקוביות והתגאה בו מאוד",
            "[ילד] ציירה בית גדול עם גג אדום ושמש",
            "[ילד] סיפר על הכלב שלו בהתלהבות רבה מאוד",
        ];
        let texts: Vec<String> = children
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "ה-WPPSI-IV הוא מבחן אינטליגנציה לגילאי הגן ובית הספר המוקדם. מנת המשכל הכוללת נמצאה [תוצאה {} · ממוצע] בטווח שנמדד במבחן. {c}. היא סיימה את המשימה ב-{} דקות בערך ובשקט רב.",
                    90 + i,
                    3 + i
                )
            })
            .collect();
        let reports: Vec<ReportParts<'_>> = texts
            .iter()
            .map(|t| vec![(Some("cognitive"), t.as_str())])
            .collect();
        let f = mine_formulas(&reports, &slot);
        let texts: Vec<&str> = f.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "ה-WPPSI-IV הוא מבחן אינטליגנציה לגילאי הגן ובית הספר המוקדם.",
                "מנת המשכל הכוללת נמצאה {תוצאה} בטווח שנמדד במבחן."
            ]
        );
        assert!(f.iter().all(|f| f.reports == 3 && f.share == 100));
        assert!(
            f.iter().all(|f| !f.text.contains('[')),
            "no tag, and the child's sentences are left"
        );
        assert!(mine_formulas(&reports[..2], &slot).is_empty());
    }

    #[test]
    fn a_sentence_with_a_name_tag_or_a_bare_number_is_never_learned() {
        assert_eq!(with_slots("[ילד] הגיע למפגש", &slot), None);
        assert_eq!(with_slots("נבדק בגיל 4 שנים", &slot), None);
        assert_eq!(
            with_slots("הציון [תוצאה 7 · ממוצע] נמוך", &slot).as_deref(),
            Some("הציון {תוצאה} נמוך")
        );
        assert_eq!(with_slots("פתוח [ללא סגירה", &slot), None);
    }
}
