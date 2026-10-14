//! How she opens and closes a section (ST11). The first words of the first sentence of a
//! section, and of its last sentence, are counted across her past reports, in the neutralized
//! text she kept. A beginning that recurs in several reports for the same section becomes a
//! suggestion ("לפתוח את הסעיף ב«…»"); nothing goes into a request until she adds it.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::edits::percent;
use crate::phrases::{candidate_ok, ReportParts};
use crate::text::{sentences, words};

/// Fewest reports a beginning must appear in, and the smallest share of the reports that
/// have the section.
const MIN_REPORTS: usize = 3;
const MIN_SHARE: f64 = 0.3;
/// Beginnings of 2 to 5 words.
const MIN_WORDS: usize = 2;
const MAX_WORDS: usize = 5;
/// A shorter beginning gives way to a longer one seen in this share of its reports.
const SUBSUME: f64 = 0.8;
/// At most this many per section and place.
const PER_PLACE: usize = 2;

/// The start or the end of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    Opening,
    Closing,
}

/// A recurring opening or closing of a section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub place: Place,
    pub section: String,
    /// As first written.
    pub text: String,
    /// Normalized words, space separated.
    pub norm: String,
    pub reports: u32,
    /// Share of the reports that have this section, 0 to 100.
    pub share: u32,
}

/// (place, section, normalized beginning).
type Key = (Place, String, String);
/// (the beginning as first written, the reports it appears in).
type Seen = (String, HashSet<usize>);

/// The beginnings of `sentence` that may be learned: 2 to 5 words, no tag or number.
fn beginnings(sentence: &str) -> Vec<(String, String)> {
    let ws = words(sentence);
    let mut out = Vec::new();
    for n in MIN_WORDS..=MAX_WORDS.min(ws.len()) {
        let Some(head) = ws.get(..n) else {
            break;
        };
        if head.iter().any(crate::text::Word::is_masked) {
            break;
        }
        let norms: Vec<&str> = head.iter().map(|w| w.norm.as_str()).collect();
        if !candidate_ok(&norms) {
            continue;
        }
        let raw: Vec<&str> = head.iter().map(|w| w.raw.as_str()).collect();
        out.push((norms.join(" "), raw.join(" ")));
    }
    out
}

/// Her recurring openings and closings, by section, most widespread first.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn mine_shapes(reports: &[ReportParts<'_>]) -> Vec<Shape> {
    // (place, section, norm) → (text, reports); and how many reports have each section.
    let mut counts: BTreeMap<Key, Seen> = BTreeMap::new();
    let mut with_section: BTreeMap<String, HashSet<usize>> = BTreeMap::new();
    for (d, parts) in reports.iter().enumerate() {
        for (section, text) in parts {
            let Some(section) = section else { continue };
            let s = sentences(text);
            let (Some(first), Some(last)) = (s.first(), s.last()) else {
                continue;
            };
            with_section
                .entry((*section).to_owned())
                .or_default()
                .insert(d);
            let mut places = vec![(Place::Opening, *first)];
            if s.len() >= 2 {
                places.push((Place::Closing, *last));
            }
            for (place, sentence) in places {
                for (norm, raw) in beginnings(sentence) {
                    let e = counts
                        .entry((place, (*section).to_owned(), norm))
                        .or_insert_with(|| (raw, HashSet::new()));
                    e.1.insert(d);
                }
            }
        }
    }
    let kept: Vec<(&Key, &Seen)> = counts
        .iter()
        .filter(|((_, section, _), (_, docs))| {
            let total = with_section.get(section).map_or(0, HashSet::len);
            docs.len() >= MIN_REPORTS && docs.len() as f64 >= MIN_SHARE * total as f64
        })
        .collect();
    let mut out: Vec<Shape> = kept
        .iter()
        .filter(|((place, section, norm), (_, docs))| {
            // Gives way to a longer beginning of the same place that is nearly as widespread.
            !kept.iter().any(|((p, s, other), (_, od))| {
                p == place
                    && s == section
                    && other.len() > norm.len()
                    && other.starts_with(&format!("{norm} "))
                    && od.len() as f64 >= SUBSUME * docs.len() as f64
            })
        })
        .map(|((place, section, norm), (text, docs))| {
            let total = with_section.get(section).map_or(1, HashSet::len).max(1);
            Shape {
                place: *place,
                section: section.clone(),
                text: text.clone(),
                norm: norm.clone(),
                reports: u32::try_from(docs.len()).unwrap_or(u32::MAX),
                share: percent(docs.len() as f64 / total as f64),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        (&a.section, a.place)
            .cmp(&(&b.section, b.place))
            .then(b.reports.cmp(&a.reports))
            .then(a.norm.cmp(&b.norm))
    });
    let mut per: BTreeMap<(String, Place), usize> = BTreeMap::new();
    out.retain(|s| {
        let n = per.entry((s.section.clone(), s.place)).or_default();
        *n += 1;
        *n <= PER_PLACE
    });
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_recurring_opening_and_closing_of_a_section_is_found_and_tags_are_never_learned() {
        let topics = ["בציור", "בפאזל", "במשחק", "בבנייה", "בסיפור"];
        let texts: Vec<String> = topics
            .iter()
            .map(|t| {
                format!(
                    "במהלך המפגש עם [ילד] נראה כי הוא סקרן {t}. הוא בחר לבד. לסיכום התרשמתי מילד נעים {t}."
                )
            })
            .collect();
        let reports: Vec<ReportParts<'_>> = texts
            .iter()
            .map(|t| vec![(Some("observation"), t.as_str())])
            .collect();
        let shapes = mine_shapes(&reports);
        let opening = shapes.iter().find(|s| s.place == Place::Opening).unwrap();
        assert_eq!(opening.text, "במהלך המפגש עם", "stops before the tag");
        assert_eq!((opening.reports, opening.share), (5, 100));
        let closing = shapes.iter().find(|s| s.place == Place::Closing).unwrap();
        assert!(
            closing.text.starts_with("לסיכום התרשמתי מילד"),
            "{}",
            closing.text
        );
        assert!(shapes.iter().all(|s| !s.text.contains('[')));
        assert!(
            mine_shapes(&reports[..2]).is_empty(),
            "two reports are not a habit"
        );
    }
}
