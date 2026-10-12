//! What Einat changes in the AI's paragraphs.
//!
//! Her edit is aligned with the AI's wording (longest common subsequence over words). Two kinds
//! of change are learned: a short **replacement** («מפגין» → «מראה») and a short **deletion**
//! of something she does not write («חשוב לציין כי»). Each is counted twice: how often she
//! changed it, and how often it appeared in a paragraph of the AI that she edited or approved
//! (its exposures). A change becomes a suggestion only when the Wilson lower bound of
//! "changed / appeared" is above one half, so a phrase the AI writes fifty times and she
//! changed three times stays a phrase she accepts.
//!
//! The document keeps the field names of the earlier `pairs` list, so what was learned before
//! carries over (`count` is the number of changes; a missing `exposed` counts as `count`).

use std::collections::BTreeMap;

use dv_privacy::text::normalize;
use serde::{Deserialize, Serialize};

use crate::text::{has, words, Word, FUNCTION_SET, GENERIC};

/// Paragraphs longer than this are not aligned (quadratic table).
const MAX_DIFF_WORDS: usize = 400;
/// Phrases kept, the decided ones first, then the most changed.
const MAX_PATTERNS: usize = 500;
/// Fewest changes before a suggestion.
pub const MIN_CHANGES: u32 = 3;
/// The lower bound of "changed / appeared" a suggestion needs.
const MIN_RATE: f64 = 0.5;
/// z for a one-sided 95% bound.
const Z: f64 = 1.645;
/// A suggestion is kept to one section when this share of its changes came from it.
const SECTION_SHARE: f64 = 0.8;
/// How many of her first edits make the "at the start" figure.
const START_EDITS: u32 = 10;
/// Weight of the newest edit in the "now" figure.
const RECENT: f64 = 0.2;

/// One learned change. `to` empty: a deletion.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub from: String,
    pub to: String,
    /// Times she made this change.
    pub count: u32,
    /// Paragraphs of the AI that held `from` and that she edited or approved.
    #[serde(default)]
    pub exposed: u32,
    /// Changes by section key.
    #[serde(default)]
    pub sections: BTreeMap<String, u32>,
    #[serde(default)]
    pub dismissed: bool,
    #[serde(default)]
    pub accepted: bool,
}

impl Pattern {
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}→{}", normalize(&self.from), normalize(&self.to))
    }

    #[must_use]
    pub fn is_deletion(&self) -> bool {
        self.to.is_empty()
    }

    fn exposures(&self) -> u32 {
        self.exposed.max(self.count)
    }

    /// The Wilson lower bound of the share of exposures she changed.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        wilson_lower(self.count, self.exposures())
    }

    /// The section that holds `SECTION_SHARE` of its changes, if any.
    fn section(&self) -> Option<String> {
        let total: u32 = self.sections.values().sum();
        let (key, top) = self.sections.iter().max_by_key(|(_, n)| **n)?;
        (total >= MIN_CHANGES && f64::from(*top) >= SECTION_SHARE * f64::from(total))
            .then(|| key.clone())
    }
}

/// How much of the AI's wording she keeps, at the start and now (a share, 0 to 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    pub edits: u32,
    pub start: f64,
    pub now: f64,
}

/// Everything learned from her edits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EditModel {
    pub pairs: Vec<Pattern>,
    /// Phrases from her reports she said no to (normalized).
    #[serde(default)]
    pub dismissed_phrases: Vec<String>,
    #[serde(default)]
    pub fit: Fit,
}

/// A change that is ready to show her.
#[derive(Debug, Clone, PartialEq)]
pub struct EditSuggestion {
    pub id: String,
    pub from: String,
    /// Empty: she deletes it.
    pub to: String,
    pub count: u32,
    pub exposed: u32,
    /// Lower bound of how often she changes it, 0 to 100.
    pub confidence: u32,
    pub section: Option<String>,
}

/// How many of her usual changes are marked in one paragraph.
pub const MAX_MARKS: usize = 5;

/// A phrase in a paragraph of the AI that she usually changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditMark {
    pub from: String,
    /// Empty: she deletes it.
    pub to: String,
}

/// `phrase` in `text`, not as part of a longer word.
fn contains_words(text: &str, phrase: &str) -> bool {
    let phrase = phrase.trim();
    if phrase.is_empty() {
        return false;
    }
    text.match_indices(phrase).any(|(at, m)| {
        let before = text[..at].chars().next_back();
        let after = text[at + m.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Lower bound of the Wilson score interval for `k` successes in `n` trials.
#[must_use]
pub fn wilson_lower(k: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let n = f64::from(n);
    let p = f64::from(k) / n;
    let z2 = Z * Z;
    let centre = p + z2 / (2.0 * n);
    let margin = Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    ((centre - margin) / (1.0 + z2 / n)).max(0.0)
}

fn plain(run: &[&Word]) -> bool {
    run.iter().all(|w| !w.is_masked())
}

/// A deleted run worth learning: 1–4 words, at least one of them not a small or generic word.
fn deletion_ok(run: &[&Word]) -> bool {
    (1..=4).contains(&run.len())
        && plain(run)
        && run
            .iter()
            .any(|w| !has(&FUNCTION_SET, &w.norm) && !has(&GENERIC, &w.norm))
}

fn replacement_ok(del: &[&Word], ins: &[&Word]) -> bool {
    (1..=3).contains(&del.len()) && (1..=3).contains(&ins.len()) && plain(del) && plain(ins)
}

fn join(run: &[&Word]) -> String {
    run.iter()
        .map(|w| w.raw.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The changes between the AI's paragraph and hers, and the share of the AI's words she kept.
#[must_use]
pub fn changes(before: &str, after: &str) -> (Vec<(String, String)>, Option<f64>) {
    let a = words(before);
    let b = words(after);
    if a.is_empty() || a.len() > MAX_DIFF_WORDS || b.len() > MAX_DIFF_WORDS {
        return (Vec::new(), None);
    }
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i].norm == b[j].norm {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let kept = f64::from(lcs[0][0]) / n as f64;
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut del, mut ins): (Vec<&Word>, Vec<&Word>) = (Vec::new(), Vec::new());
    let mut flush = |del: &mut Vec<&Word>, ins: &mut Vec<&Word>| {
        if ins.is_empty() {
            if deletion_ok(del) {
                out.push((join(del), String::new()));
            }
        } else if replacement_ok(del, ins) && normalize(&join(del)) != normalize(&join(ins)) {
            out.push((join(del), join(ins)));
        }
        del.clear();
        ins.clear();
    };
    while i < n || j < m {
        if i < n && j < m && a[i].norm == b[j].norm {
            flush(&mut del, &mut ins);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            ins.push(&b[j]);
            j += 1;
        } else {
            del.push(&a[i]);
            i += 1;
        }
    }
    flush(&mut del, &mut ins);
    (out, Some(kept))
}

/// `phrase` appears in `text` as whole words (both normalized, space separated).
fn holds(text: &str, phrase: &str) -> bool {
    !phrase.is_empty() && format!(" {text} ").contains(&format!(" {phrase} "))
}

fn normalized_words(text: &str) -> String {
    words(text)
        .into_iter()
        .map(|w| w.norm)
        .collect::<Vec<_>>()
        .join(" ")
}

impl EditModel {
    /// She edited the AI's paragraph in `section` from `before` to `after`.
    pub fn observe_edit(&mut self, section: &str, before: &str, after: &str) {
        let (found, kept) = changes(before, after);
        // Every phrase already known that this paragraph held was exposed once more.
        self.expose(before);
        for (from, to) in found {
            let probe = Pattern {
                from,
                to,
                exposed: 1,
                ..Pattern::default()
            };
            let id = probe.id();
            let p = match self.pairs.iter_mut().position(|p| p.id() == id) {
                Some(i) => &mut self.pairs[i],
                None => {
                    self.pairs.push(probe);
                    let last = self.pairs.len() - 1;
                    &mut self.pairs[last]
                }
            };
            p.count = p.count.saturating_add(1);
            *p.sections.entry(section.to_owned()).or_default() += 1;
        }
        if let Some(k) = kept {
            self.record_fit(k);
        }
        self.trim();
    }

    /// She approved the AI's paragraph as it was: every learned phrase in it was accepted.
    pub fn observe_approval(&mut self, before: &str) {
        self.expose(before);
        self.record_fit(1.0);
    }

    fn expose(&mut self, before: &str) {
        let text = normalized_words(before);
        for p in &mut self.pairs {
            if holds(&text, &normalized_words(&p.from)) {
                p.exposed = p.exposures().saturating_add(1);
            }
        }
    }

    fn record_fit(&mut self, kept: f64) {
        let f = &mut self.fit;
        f.edits = f.edits.saturating_add(1);
        if f.edits <= START_EDITS {
            f.start += (kept - f.start) / f64::from(f.edits);
            f.now = f.start;
        } else {
            f.now += RECENT * (kept - f.now);
        }
    }

    fn trim(&mut self) {
        if self.pairs.len() > MAX_PATTERNS {
            self.pairs.sort_by(|a, b| {
                (b.accepted || b.dismissed, b.count).cmp(&(a.accepted || a.dismissed, a.count))
            });
            self.pairs.truncate(MAX_PATTERNS);
        }
    }

    /// The changes worth suggesting, most certain first.
    #[must_use]
    pub fn suggestions(&self) -> Vec<EditSuggestion> {
        let mut out: Vec<(f64, EditSuggestion)> = self
            .pairs
            .iter()
            .filter(|p| !p.dismissed && !p.accepted && p.count >= MIN_CHANGES)
            .filter(|p| p.confidence() >= MIN_RATE)
            .map(|p| {
                let c = p.confidence();
                (
                    c * f64::from(p.count).ln_1p(),
                    EditSuggestion {
                        id: p.id(),
                        from: p.from.clone(),
                        to: p.to.clone(),
                        count: p.count,
                        exposed: p.exposures(),
                        confidence: percent(c),
                        section: p.section(),
                    },
                )
            })
            .collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.into_iter().map(|(_, s)| s).collect()
    }

    /// Her usual changes that appear in `text` as whole words: what to mark in a paragraph of
    /// the AI, with what she usually writes instead (empty: she deletes it). The same bar as a
    /// suggestion; one she said no to is never marked. At most [`MAX_MARKS`], most certain first.
    #[must_use]
    pub fn marks(&self, text: &str) -> Vec<EditMark> {
        let mut found: Vec<(f64, EditMark)> = self
            .pairs
            .iter()
            .filter(|p| !p.dismissed && p.count >= MIN_CHANGES && p.confidence() >= MIN_RATE)
            .filter(|p| contains_words(text, &p.from))
            .map(|p| {
                (
                    p.confidence(),
                    EditMark {
                        from: p.from.clone(),
                        to: p.to.clone(),
                    },
                )
            })
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        found.dedup_by(|a, b| a.1.from == b.1.from);
        found.into_iter().take(MAX_MARKS).map(|(_, m)| m).collect()
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Pattern> {
        self.pairs.iter_mut().find(|p| p.id() == id)
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn percent(x: f64) -> u32 {
    (x.clamp(0.0, 1.0) * 100.0).round() as u32
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn her_usual_changes_are_marked_only_as_whole_words_and_never_once_dismissed() {
        let mut m = EditModel::default();
        for _ in 0..4 {
            m.observe_edit(
                "observation",
                "הילד מפגין סקרנות רבה במשחק.",
                "הילד מראה סקרנות רבה במשחק.",
            );
        }
        let marks = m.marks("בהמשך הילד מפגין עניין.");
        assert_eq!(
            marks,
            vec![EditMark {
                from: "מפגין".into(),
                to: "מראה".into()
            }]
        );
        assert!(
            m.marks("הילדה מפגינה עניין.").is_empty(),
            "part of a longer word"
        );
        assert!(m.marks("שום דבר כזה.").is_empty());
        let id = m.suggestions()[0].id.clone();
        m.find_mut(&id).unwrap().dismissed = true;
        assert!(m.marks("בהמשך הילד מפגין עניין.").is_empty());
    }

    const AI: &str = "חשוב לציין כי הילד מפגין סקרנות רבה ומשתף פעולה עם הבודקת לאורך המפגש.";
    const HERS: &str = "הילד מראה סקרנות רבה ומשתף פעולה עם הבודקת לאורך המפגש.";

    #[test]
    fn replacements_and_deletions_are_found() {
        let (found, kept) = changes(AI, HERS);
        assert!(
            found.contains(&("חשוב לציין כי".to_owned(), String::new())),
            "{found:?}"
        );
        assert!(
            found.contains(&("מפגין".to_owned(), "מראה".to_owned())),
            "{found:?}"
        );
        let k = kept.unwrap();
        assert!(k > 0.6 && k < 0.8, "{k}");
    }

    #[test]
    fn tags_numbers_and_small_words_are_not_learned() {
        let (found, _) = changes("הילד [ילד] קיבל 12 של נקודות.", "הילד קיבל נקודות.");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn three_changes_out_of_three_become_a_suggestion() {
        let mut m = EditModel::default();
        for _ in 0..3 {
            m.observe_edit("observation", AI, HERS);
        }
        let s = m.suggestions();
        let del = s.iter().find(|s| s.to.is_empty()).unwrap();
        assert_eq!(del.from, "חשוב לציין כי");
        assert_eq!(del.section.as_deref(), Some("observation"));
        assert!(s.iter().any(|s| s.to == "מראה"));
    }

    #[test]
    fn a_phrase_she_mostly_keeps_is_not_suggested() {
        let mut m = EditModel::default();
        for _ in 0..3 {
            m.observe_edit("observation", AI, HERS);
        }
        for _ in 0..20 {
            m.observe_approval(AI);
        }
        assert!(m.suggestions().is_empty(), "{:?}", m.suggestions());
    }

    #[test]
    fn the_earlier_document_still_reads() {
        let old = r#"{"pairs":[{"from":"מפגין","to":"מראה","count":4,"dismissed":false,"accepted":false}]}"#;
        let m: EditModel = serde_json::from_str(old).unwrap();
        let s = m.suggestions();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, "מפגינ→מראה");
        assert_eq!(s[0].section, None);
    }

    #[test]
    fn wilson_is_cautious_with_little_evidence() {
        assert!(wilson_lower(3, 3) > 0.5);
        assert!(wilson_lower(3, 50) < 0.2);
        assert!(wilson_lower(30, 40) > wilson_lower(3, 4));
        assert!(wilson_lower(0, 0).abs() < f64::EPSILON);
    }

    #[test]
    fn fit_tracks_how_much_she_keeps() {
        let mut m = EditModel::default();
        for _ in 0..10 {
            m.observe_edit("observation", AI, "פסקה אחרת לגמרי שכתבתי מחדש.");
        }
        let start = m.fit.start;
        for _ in 0..10 {
            m.observe_approval(AI);
        }
        assert!(m.fit.now > start + 0.5, "{:?}", m.fit);
        assert!((m.fit.start - start).abs() < 1e-9);
    }
}
