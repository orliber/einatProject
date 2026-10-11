//! Expressions that recur across Einat's past reports, found on her computer without any AI.
//!
//! Runs of 2 to 4 words inside a sentence are counted in the neutralized text she kept. A run
//! is a candidate when it appears in several **different** reports (so it is how she writes,
//! not what one child did), holds no tag or number, and is not made only of small or generic
//! words. Candidates are ranked by how many reports hold them and how particular they are to a
//! section (TF-IDF of the section against the others); a run inside a longer candidate seen in
//! almost as many reports gives way to it.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::edits::percent;
use crate::text::{has, sentences, words, FUNCTION_SET, GENERIC};
use crate::GENERAL;

/// Fewest reports a phrase must appear in.
const MIN_REPORTS: usize = 3;
/// … and the smallest share of her reports.
const MIN_SHARE: f64 = 0.3;
/// A phrase belongs to a section when this share of its uses is there.
const SECTION_SHARE: f64 = 0.6;
/// A shorter phrase gives way to a longer one seen in this share of its reports.
const SUBSUME: f64 = 0.8;
const PER_SECTION: usize = 8;
const TOTAL: usize = 30;

/// One past report: `(section key or None, text)` for each part she kept.
pub type ReportParts<'a> = Vec<(Option<&'a str>, &'a str)>;

/// A phrase she uses, as she writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhraseCandidate {
    /// As first written.
    pub text: String,
    /// Normalized words, space separated (its identity).
    pub norm: String,
    pub section: Option<String>,
    /// In how many of her reports it appears.
    pub reports: u32,
    /// Share of her reports, 0 to 100.
    pub share: u32,
}

#[derive(Default)]
struct Count {
    text: String,
    docs: HashSet<usize>,
    by_section: HashMap<String, u32>,
}

fn candidate_ok(norms: &[&str]) -> bool {
    norms
        .iter()
        .any(|w| !has(&FUNCTION_SET, w) && !has(&GENERIC, w))
        && !norms
            .first()
            .is_some_and(|w| has(&FUNCTION_SET, w) && *w != "כי")
}

/// The phrases of her past reports worth offering, best first.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn mine_phrases(reports: &[ReportParts<'_>]) -> Vec<PhraseCandidate> {
    let docs = reports.len();
    let need = MIN_REPORTS.max((MIN_SHARE * docs as f64).ceil() as usize);
    if docs < MIN_REPORTS {
        return Vec::new();
    }
    let mut counts: HashMap<String, Count> = HashMap::new();
    let mut section_totals: HashMap<String, u32> = HashMap::new();
    for (d, parts) in reports.iter().enumerate() {
        for (section, text) in parts {
            let key = section.unwrap_or(GENERAL).to_owned();
            for s in sentences(text) {
                let ws = words(s);
                *section_totals.entry(key.clone()).or_default() += ws.len() as u32;
                for n in 2..=4 {
                    for win in ws.windows(n) {
                        if win.iter().any(crate::text::Word::is_masked) {
                            continue;
                        }
                        let norms: Vec<&str> = win.iter().map(|w| w.norm.as_str()).collect();
                        if !candidate_ok(&norms) {
                            continue;
                        }
                        let c = counts.entry(norms.join(" ")).or_default();
                        if c.text.is_empty() {
                            c.text = win
                                .iter()
                                .map(|w| w.raw.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                        }
                        c.docs.insert(d);
                        *c.by_section.entry(key.clone()).or_default() += 1;
                    }
                }
            }
        }
    }
    let kept: HashMap<&String, &Count> = counts
        .iter()
        .filter(|(_, c)| c.docs.len() >= need)
        .collect();
    let mut out: Vec<(f64, PhraseCandidate)> = Vec::new();
    for (norm, c) in &kept {
        // Gives way to a longer phrase that holds it and is nearly as widespread.
        let longer = kept.iter().any(|(other, oc)| {
            other.len() > norm.len()
                && format!(" {other} ").contains(&format!(" {norm} "))
                && oc.docs.len() as f64 >= SUBSUME * c.docs.len() as f64
        });
        if longer {
            continue;
        }
        let uses: u32 = c.by_section.values().sum();
        let (top_key, top) = c
            .by_section
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map_or((GENERAL.to_owned(), 0), |(k, n)| (k.clone(), *n));
        let section = (f64::from(top) >= SECTION_SHARE * f64::from(uses) && top_key != GENERAL)
            .then_some(top_key.clone());
        // TF-IDF of the section: uses per thousand words here, against how many sections use it.
        let words_here = f64::from(*section_totals.get(&top_key).unwrap_or(&1)).max(1.0);
        let tf = f64::from(top) * 1_000.0 / words_here;
        let idf = ((section_totals.len() as f64 + 1.0) / (c.by_section.len() as f64)).ln() + 1.0;
        let df = c.docs.len() as f64 / docs as f64;
        let n_words = norm.split(' ').count() as f64;
        out.push((
            df * tf.ln_1p() * idf * n_words.sqrt(),
            PhraseCandidate {
                text: c.text.clone(),
                norm: (*norm).clone(),
                section,
                reports: u32::try_from(c.docs.len()).unwrap_or(u32::MAX),
                share: percent(df),
            },
        ));
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.norm.cmp(&b.1.norm)));
    // Greedy: a phrase that shares two words in a row with a better one says nothing new
    // (overlapping windows of one recurring sentence).
    let bigrams = |n: &str| -> HashSet<String> {
        let w: Vec<&str> = n.split(' ').collect();
        w.windows(2).map(|p| p.join(" ")).collect()
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut per: BTreeMap<Option<String>, usize> = BTreeMap::new();
    let mut picked = Vec::new();
    for (_, p) in out {
        let b = bigrams(&p.norm);
        if b.iter().any(|x| seen.contains(x)) {
            continue;
        }
        let n = per.entry(p.section.clone()).or_default();
        if *n >= PER_SECTION {
            continue;
        }
        *n += 1;
        seen.extend(b);
        picked.push(p);
        if picked.len() == TOTAL {
            break;
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// Invented reports: her habit «ניכר כי» and «בהתאם לגילו» in observation, «מומלץ להמשיך
    /// במעקב» in recommendations, with content that differs from report to report.
    fn reports() -> Vec<Vec<(Option<&'static str>, String)>> {
        let topics = ["בציור", "בפאזל", "במשחק הדמיון", "בבניית מגדל", "בסיפור"];
        topics
            .iter()
            .enumerate()
            .map(|(i, t)| {
                vec![
                    (
                        Some("observation"),
                        format!("ניכר כי [ילד] פועל בהתאם לגילו {t}. הוא בחר צבע {i} ושיחק לבד."),
                    ),
                    (
                        Some("recommendations"),
                        format!("מומלץ להמשיך במעקב התפתחותי. הדרכה {t} לפי הצורך."),
                    ),
                ]
            })
            .collect()
    }

    fn mine() -> Vec<PhraseCandidate> {
        let r = reports();
        let parts: Vec<ReportParts<'_>> = r
            .iter()
            .map(|p| p.iter().map(|(s, t)| (*s, t.as_str())).collect())
            .collect();
        mine_phrases(&parts)
    }

    #[test]
    fn her_recurring_phrases_are_found_by_section() {
        let found = mine();
        let get = |t: &str| found.iter().find(|p| p.text == t);
        let a = get("פועל בהתאם לגילו");
        assert!(a.is_some(), "{found:?}");
        let a = a.unwrap();
        assert_eq!(a.section.as_deref(), Some("observation"));
        assert_eq!(a.reports, 5);
        let b = get("מומלץ להמשיך במעקב התפתחותי").unwrap();
        assert_eq!(b.section.as_deref(), Some("recommendations"));
    }

    #[test]
    fn shorter_runs_give_way_to_the_longer_phrase() {
        let found = mine();
        assert!(!found.iter().any(|p| p.text == "להמשיך במעקב"), "{found:?}");
    }

    #[test]
    fn overlapping_windows_of_one_sentence_are_offered_once() {
        let r: Vec<String> = (0..4)
            .map(|i| format!("בשיח עימי ניכר כי הוא מגיב היטב לתיווך של המבוגר. נושא {i}."))
            .collect();
        let parts: Vec<ReportParts<'_>> = r.iter().map(|t| vec![(None, t.as_str())]).collect();
        let found = mine_phrases(&parts);
        assert!(found.len() <= 3, "{found:?}");
    }

    #[test]
    fn what_one_report_says_is_not_a_phrase() {
        let found = mine();
        assert!(
            !found.iter().any(|p| p.text.contains("בבניית")),
            "{found:?}"
        );
        assert!(!found.iter().any(|p| p.text.contains("[ילד]")));
        assert!(!found
            .iter()
            .any(|p| p.text.chars().any(|c| c.is_ascii_digit())));
    }

    #[test]
    fn too_few_reports_say_nothing() {
        let r = reports();
        let parts: Vec<ReportParts<'_>> = r[..2]
            .iter()
            .map(|p| p.iter().map(|(s, t)| (*s, t.as_str())).collect())
            .collect();
        assert!(mine_phrases(&parts).is_empty());
    }
}
