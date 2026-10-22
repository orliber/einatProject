//! Patterns in a child's scores, worked out in code (D-091): not one score, but the shape of
//! the profile. A gap of a standard deviation or more between the indexes of one test, which
//! index is the lowest or the highest, a wide spread that makes the total hard to read, a
//! scatter between the subtests, and the adaptive total against the cognitive one.
//!
//! The same function reads a case's score sheets and a past report's score tags, so a pattern
//! found in a past report (where Einat's reading of it is learned) is the pattern found in a
//! new case (where that reading is used). The keys are fixed here; the model never decides
//! what a pattern is, only describes how she writes about it.
//!
//! Thresholds (`knowledge/instruments.md`, waiting for her approval): 15 standard points (one
//! SD) for a gap, 23 (one and a half SDs, the usual guideline for a total that is hard to
//! interpret) for a wide spread, 6 scaled points (two SDs) for a subtest scatter, and 15
//! points between the adaptive and the cognitive totals.

use std::collections::BTreeMap;

use crate::scores::{instruments, Instrument, Measure, ScoreScale, ScoreSheet, TaggedScore};

/// A gap of this many standard points between two indexes of one test is a pattern.
pub const INDEX_GAP: f64 = 15.0;
/// A spread of this many points between the highest and the lowest index makes the total
/// hard to read.
pub const WIDE_SPREAD: f64 = 23.0;
/// Indexes this close together are a flat profile (three or more of them).
pub const FLAT_SPREAD: f64 = 9.0;
/// A range of this many scaled points between the subtests of one test is a scatter.
pub const SUBTEST_SCATTER: f64 = 6.0;
/// The adaptive total this far from the cognitive total is a gap between them; closer than
/// [`IN_LINE`] they are in line.
pub const ADAPTIVE_GAP: f64 = 15.0;
pub const IN_LINE: f64 = 9.0;

/// The totals of a test: never one of its indexes.
const TOTALS: [&str; 4] = ["fsiq", "gac", "abc", "elc"];
/// Totals of a cognitive test, and of an adaptive one, for the gap between the two.
const COGNITIVE_TOTALS: [(&str, &str); 5] = [
    ("wppsi_iv", "fsiq"),
    ("wisc_v", "fsiq"),
    ("wppsi_iii", "fsiq"),
    ("bayley_4", "cognitive"),
    ("mullen", "elc"),
];
const ADAPTIVE_TOTALS: [(&str, &str); 2] = [("abas_3", "gac"), ("vineland_3", "abc")];

/// One pattern: a fixed key, and its name in words for the request and the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScorePattern {
    pub key: String,
    pub label: String,
}

fn pattern(key: &str, label: &str) -> ScorePattern {
    ScorePattern {
        key: key.to_owned(),
        label: label.to_owned(),
    }
}

/// An index of a test: a standard score that is not a total and not an ancillary index.
fn is_index(m: &Measure) -> bool {
    m.scale == ScoreScale::Standard
        && m.group != "מדדים נוספים"
        && !TOTALS.contains(&m.key.as_str())
}

/// Every pattern key there can be, for a schema (fixed in code, no case data).
#[must_use]
pub fn pattern_keys() -> Vec<String> {
    let mut out: Vec<String> = [
        "index_gap",
        "index_spread_wide",
        "flat_profile",
        "subtest_scatter",
        "adaptive_below_cognitive",
        "cognitive_below_adaptive",
        "adaptive_in_line_with_cognitive",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for i in instruments() {
        for m in i.measures.iter().filter(|m| is_index(m)) {
            out.push(format!("lowest_index:{}", m.key));
            out.push(format!("highest_index:{}", m.key));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The name in words of a pattern key; `None` for a key that is not one.
#[must_use]
pub fn pattern_label(key: &str) -> Option<String> {
    let fixed = match key {
        "index_gap" => Some("פער של סטיית תקן ומעלה בין המדדים"),
        "index_spread_wide" => {
            Some("פיזור גדול בין המדדים (סטיית תקן וחצי ומעלה), שמקשה על פירוש התוצאה הכללית")
        }
        "flat_profile" => Some("פרופיל אחיד: המדדים קרובים זה לזה"),
        "subtest_scatter" => Some("פיזור גדול בין תתי-המבחנים (שתי סטיות תקן ומעלה)"),
        "adaptive_below_cognitive" => Some("התפקוד המסתגל נמוך מהתפקוד הקוגניטיבי"),
        "cognitive_below_adaptive" => Some("התפקוד הקוגניטיבי נמוך מהתפקוד המסתגל"),
        "adaptive_in_line_with_cognitive" => Some("התפקוד המסתגל בהלימה לתפקוד הקוגניטיבי"),
        _ => None,
    };
    if let Some(l) = fixed {
        return Some(l.to_owned());
    }
    let (place, measure) = key.split_once(':')?;
    let word = match place {
        "lowest_index" => "המדד הנמוך ביותר",
        "highest_index" => "המדד הגבוה ביותר",
        _ => return None,
    };
    let name = instruments()
        .into_iter()
        .flat_map(|i| i.measures)
        .find(|m| m.key == measure && is_index(m))?
        .name_he;
    Some(format!("{word}: {name}"))
}

/// `(instrument key, measure key, value)`: one score, wherever it was read from.
type Score<'a> = (&'a str, &'a str, f64);

/// The patterns in a set of scores, in a fixed order, each once. Scores of a measure that is
/// not in the fixed tables are left out; the same measure twice keeps the first.
fn patterns_of(scores: &[Score<'_>]) -> Vec<ScorePattern> {
    let all = instruments();
    let mut by_instrument: BTreeMap<&str, Vec<(&Measure, f64)>> = BTreeMap::new();
    for (inst, measure, value) in scores {
        let Some(m) = all
            .iter()
            .find(|i| i.key == *inst)
            .and_then(|i: &Instrument| i.measures.iter().find(|m| m.key == *measure))
        else {
            continue;
        };
        let list = by_instrument.entry(inst).or_default();
        if !list.iter().any(|(x, _)| x.key == m.key) {
            list.push((m, *value));
        }
    }
    let mut out: Vec<ScorePattern> = Vec::new();
    let mut push = |p: ScorePattern| {
        if !out.iter().any(|o| o.key == p.key) {
            out.push(p);
        }
    };
    for list in by_instrument.values() {
        let indexes: Vec<(&Measure, f64)> =
            list.iter().filter(|(m, _)| is_index(m)).copied().collect();
        if let (Some(hi), Some(lo)) = (
            indexes.iter().max_by(|a, b| a.1.total_cmp(&b.1)),
            indexes.iter().min_by(|a, b| a.1.total_cmp(&b.1)),
        ) {
            let spread = hi.1 - lo.1;
            if spread >= INDEX_GAP {
                push(pattern("index_gap", &label_of("index_gap")));
                let key = format!("lowest_index:{}", lo.0.key);
                push(pattern(&key, &label_of(&key)));
                let key = format!("highest_index:{}", hi.0.key);
                push(pattern(&key, &label_of(&key)));
            }
            if spread >= WIDE_SPREAD {
                push(pattern("index_spread_wide", &label_of("index_spread_wide")));
            }
            if indexes.len() >= 3 && spread <= FLAT_SPREAD {
                push(pattern("flat_profile", &label_of("flat_profile")));
            }
        }
        let subtests: Vec<f64> = list
            .iter()
            .filter(|(m, _)| matches!(m.scale, ScoreScale::Scaled | ScoreScale::VScale))
            .map(|(_, v)| *v)
            .collect();
        if subtests.len() >= 3 {
            let hi = subtests.iter().copied().fold(f64::MIN, f64::max);
            let lo = subtests.iter().copied().fold(f64::MAX, f64::min);
            if hi - lo >= SUBTEST_SCATTER {
                push(pattern("subtest_scatter", &label_of("subtest_scatter")));
            }
        }
    }
    let total = |keys: &[(&str, &str)]| {
        keys.iter().find_map(|(i, m)| {
            by_instrument
                .get(i)?
                .iter()
                .find(|(x, _)| x.key == *m)
                .map(|(_, v)| *v)
        })
    };
    if let (Some(cognitive), Some(adaptive)) = (total(&COGNITIVE_TOTALS), total(&ADAPTIVE_TOTALS)) {
        let key = if cognitive - adaptive >= ADAPTIVE_GAP {
            Some("adaptive_below_cognitive")
        } else if adaptive - cognitive >= ADAPTIVE_GAP {
            Some("cognitive_below_adaptive")
        } else if (cognitive - adaptive).abs() <= IN_LINE {
            Some("adaptive_in_line_with_cognitive")
        } else {
            None
        };
        if let Some(key) = key {
            push(pattern(key, &label_of(key)));
        }
    }
    out
}

fn label_of(key: &str) -> String {
    pattern_label(key).unwrap_or_else(|| key.to_owned())
}

/// The patterns in a case's score sheets (the scores of each sheet, by its instrument).
#[must_use]
pub fn sheet_patterns(sheets: &[ScoreSheet]) -> Vec<ScorePattern> {
    let scores: Vec<Score<'_>> = sheets
        .iter()
        .flat_map(|s| {
            s.entries
                .iter()
                .map(move |e| (s.instrument.as_str(), e.measure.as_str(), e.value))
        })
        .collect();
    patterns_of(&scores)
}

/// The patterns in a past report, from its score tags (the ones with a test or subtest named
/// beside them, [`crate::find_tagged_scores`]).
#[must_use]
pub fn report_patterns(tags: &[TaggedScore]) -> Vec<ScorePattern> {
    let scores: Vec<Score<'_>> = tags
        .iter()
        .map(|t| (t.instrument.as_str(), t.measure.as_str(), t.value))
        .collect();
    patterns_of(&scores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scores::ScoreEntry;

    fn sheet(instrument: &str, entries: &[(&str, f64)]) -> ScoreSheet {
        ScoreSheet {
            instrument: instrument.into(),
            module: String::new(),
            cutoff: None,
            notes: String::new(),
            entries: entries
                .iter()
                .map(|(m, v)| ScoreEntry {
                    measure: (*m).into(),
                    value: *v,
                    note: String::new(),
                })
                .collect(),
        }
    }

    fn keys(p: &[ScorePattern]) -> Vec<&str> {
        p.iter().map(|x| x.key.as_str()).collect()
    }

    #[test]
    fn a_gap_names_the_lowest_and_the_highest_index() {
        let p = sheet_patterns(&[sheet(
            "wppsi_iv",
            &[
                ("fsiq", 98.0),
                ("vci", 110.0),
                ("fri", 100.0),
                ("psi", 82.0),
            ],
        )]);
        assert_eq!(
            keys(&p),
            [
                "index_gap",
                "lowest_index:psi",
                "highest_index:vci",
                "index_spread_wide"
            ]
        );
        assert_eq!(p[1].label, "המדד הנמוך ביותר: מהירות עיבוד");
    }

    #[test]
    fn the_total_and_the_ancillary_indexes_never_make_a_gap() {
        // FSIQ 70 against VCI 95: the total is not an index. GAI 120: ancillary.
        let p = sheet_patterns(&[sheet(
            "wppsi_iv",
            &[
                ("fsiq", 70.0),
                ("vci", 95.0),
                ("fri", 93.0),
                ("wmi", 90.0),
                ("gai", 120.0),
            ],
        )]);
        assert_eq!(keys(&p), ["flat_profile"]);
    }

    #[test]
    fn a_subtest_scatter_needs_three_subtests_and_two_sds() {
        let two = sheet_patterns(&[sheet(
            "wppsi_iv",
            &[("block_design", 4.0), ("matrix", 13.0)],
        )]);
        assert!(two.is_empty());
        let three = sheet_patterns(&[sheet(
            "wppsi_iv",
            &[
                ("block_design", 6.0),
                ("matrix", 12.0),
                ("information", 9.0),
            ],
        )]);
        assert_eq!(keys(&three), ["subtest_scatter"]);
    }

    #[test]
    fn the_adaptive_total_is_read_against_the_cognitive_one_across_sheets() {
        let below = sheet_patterns(&[
            sheet("wppsi_iv", &[("fsiq", 100.0)]),
            sheet("abas_3", &[("gac", 80.0)]),
        ]);
        assert_eq!(keys(&below), ["adaptive_below_cognitive"]);
        let in_line = sheet_patterns(&[
            sheet("bayley_4", &[("cognitive", 90.0)]),
            sheet("vineland_3", &[("abc", 95.0)]),
        ]);
        assert_eq!(keys(&in_line), ["adaptive_in_line_with_cognitive"]);
        let between = sheet_patterns(&[
            sheet("wisc_v", &[("fsiq", 100.0)]),
            sheet("abas_3", &[("gac", 88.0)]),
        ]);
        assert!(between.is_empty(), "12 points: neither a gap nor in line");
    }

    #[test]
    fn a_past_report_gives_the_same_patterns_as_a_sheet() {
        let text = "WPPSI-IV. בהבנה מילולית [תוצאה 110 · ממוצע גבוה], בחשיבה פלואידית [תוצאה 100 · ממוצע] \
                    ובמהירות עיבוד [תוצאה 82 · ממוצע נמוך]. ב-ABAS-3 המדד הכללי (GAC) [תוצאה 80 · ממוצע נמוך].";
        let tags = crate::find_tagged_scores(text);
        assert!(tags.len() >= 3, "{tags:?}");
        let p = report_patterns(&tags);
        assert!(keys(&p).contains(&"lowest_index:psi"), "{p:?}");
    }

    #[test]
    fn every_key_has_a_label_and_the_list_is_fixed() {
        let keys = pattern_keys();
        assert!(keys.contains(&"lowest_index:psi".to_owned()));
        assert!(keys.contains(&"highest_index:language".to_owned()));
        assert!(!keys.contains(&"lowest_index:fsiq".to_owned()));
        for k in &keys {
            assert!(pattern_label(k).is_some(), "{k}");
        }
        assert_eq!(pattern_label("nothing"), None);
        assert_eq!(pattern_label("lowest_index:fsiq"), None);
        assert_eq!(keys, pattern_keys(), "deterministic");
    }
}
