//! Relative strengths and weaknesses among a child's subtests (D-190).
//!
//! The publisher's method: each primary subtest against the mean of the primary subtests
//! (MSS-P), flagged only when the difference reaches the manual's critical value at .05. The
//! critical values are hers to copy from the manuals she owns (they are never invented here);
//! which subtests are primary is the test's published structure. Until every value a sheet
//! needs is entered, the sheet falls back to the labelled 3-point rule of thumb (D-165).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::scores::{Measure, Relative, ScoreEntry, ScoreScale, RELATIVE_POINTS};

/// The subtests the publisher compares with their own mean, for one instrument and age band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RelativeSet {
    /// "wisc_v", "wppsi_iv_4_7".
    pub key: String,
    pub instrument: String,
    /// "WPPSI-IV, גילאי 4:0–7:7".
    pub title_he: String,
    /// The primary (or core) subtests, by measure key.
    pub measures: Vec<String>,
}

/// One critical value she copied from a manual (.05), for one subtest of one set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CriticalValue {
    pub set: String,
    pub measure: String,
    pub value: f64,
}

/// The highest critical value accepted: a scaled-score difference larger than this is not a
/// critical value of any manual, and is more likely a typo.
const MAX_CRITICAL: f64 = 9.0;

fn set(key: &str, instrument: &str, title: &str, measures: &[&str]) -> RelativeSet {
    RelativeSet {
        key: key.to_owned(),
        instrument: instrument.to_owned(),
        title_he: title.to_owned(),
        measures: measures.iter().map(|m| (*m).to_owned()).collect(),
    }
}

/// The comparison sets: the Wechsler scales' primary subtests (WISC-V, WPPSI-IV) and core
/// subtests (WPPSI-III), by age band where the set changes with age. Bayley-4 and Vineland-3
/// have no such comparison here and keep the rule of thumb.
#[must_use]
pub fn relative_sets() -> Vec<RelativeSet> {
    vec![
        set(
            "wisc_v",
            "wisc_v",
            "WISC-V",
            &[
                "similarities",
                "vocabulary",
                "block_design",
                "visual_puzzles",
                "matrix",
                "figure_weights",
                "digit_span",
                "picture_span",
                "coding",
                "symbol_search",
            ],
        ),
        set(
            "wppsi_iv_4_7",
            "wppsi_iv",
            "WPPSI-IV, גילאי 4:0–7:7",
            &[
                "information",
                "similarities",
                "block_design",
                "object_assembly",
                "matrix",
                "picture_concepts",
                "picture_memory",
                "zoo_locations",
                "bug_search",
                "cancellation",
            ],
        ),
        set(
            "wppsi_iv_2_3",
            "wppsi_iv",
            "WPPSI-IV, גילאי 2:6–3:11",
            &[
                "receptive_vocabulary",
                "information",
                "block_design",
                "object_assembly",
                "picture_memory",
                "zoo_locations",
            ],
        ),
        set(
            "wppsi_iii_4_7",
            "wppsi_iii",
            "WPPSI-III, גילאי 4:0–7:3",
            &[
                "information",
                "vocabulary",
                "word_reasoning",
                "block_design",
                "matrix",
                "picture_concepts",
                "coding",
            ],
        ),
        set(
            "wppsi_iii_2_3",
            "wppsi_iii",
            "WPPSI-III, גילאי 2:6–3:11",
            &[
                "receptive_vocabulary",
                "information",
                "block_design",
                "object_assembly",
            ],
        ),
    ]
}

/// Her critical values, checked: a known set and one of its subtests, once each, and a value
/// above 0 and at most [`MAX_CRITICAL`], to two decimals at most.
///
/// # Errors
/// A message in Hebrew naming the first value that is refused.
pub fn check_critical_values(values: &[CriticalValue]) -> Result<(), String> {
    let sets = relative_sets();
    for (i, v) in values.iter().enumerate() {
        let Some(s) = sets.iter().find(|s| s.key == v.set) else {
            return Err("קבוצת השוואה לא מוכרת".into());
        };
        if !s.measures.contains(&v.measure) {
            return Err(format!("{}: תת-מבחן לא מוכר", s.title_he));
        }
        if values
            .iter()
            .take(i)
            .any(|w| w.set == v.set && w.measure == v.measure)
        {
            return Err(format!("{}: תת-מבחן שהוזן פעמיים", s.title_he));
        }
        // Written as "not inside" so that NaN is refused too.
        if !(v.value > 0.0 && v.value <= MAX_CRITICAL)
            || ((v.value * 100.0).round() - v.value * 100.0).abs() > 1e-6
        {
            return Err(format!(
                "{}: ערך קריטי צריך להיות מספר בין 0 ל-{} עם עד שתי ספרות אחרי הנקודה",
                s.title_he, MAX_CRITICAL
            ));
        }
    }
    Ok(())
}

/// How the relative places of a sheet were found.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Method {
    /// The publisher's method, over a set whose subtests and critical values are all there.
    Publisher { title_he: String, mean: f64 },
    /// The 3-point rule of thumb against the mean of every subtest entered (three or more).
    RuleOfThumb,
    /// Fewer than three subtests: no relative place at all.
    None,
}

/// The relative place of each entry (in order), and how it was found.
pub(crate) fn relative_places(
    instrument: &str,
    entries: &[(&Measure, &ScoreEntry)],
    critical: &[CriticalValue],
) -> (Method, Vec<Option<Relative>>) {
    if let Some(found) = by_publisher(instrument, entries, critical) {
        return found;
    }
    let scaled: Vec<f64> = entries
        .iter()
        .filter(|(m, _)| is_subtest(m))
        .map(|(_, e)| e.value)
        .collect();
    if scaled.len() < 3 {
        return (Method::None, vec![None; entries.len()]);
    }
    #[allow(clippy::cast_precision_loss)]
    let mean = scaled.iter().sum::<f64>() / scaled.len() as f64;
    let places = entries
        .iter()
        .map(|(m, e)| {
            if !is_subtest(m) {
                None
            } else if e.value - mean >= RELATIVE_POINTS {
                Some(Relative::Strength)
            } else if mean - e.value >= RELATIVE_POINTS {
                Some(Relative::Weakness)
            } else {
                None
            }
        })
        .collect();
    (Method::RuleOfThumb, places)
}

fn is_subtest(m: &Measure) -> bool {
    matches!(m.scale, ScoreScale::Scaled | ScoreScale::VScale)
}

/// The publisher's method, when the sheet's set is clear, complete and has every critical value.
fn by_publisher(
    instrument: &str,
    entries: &[(&Measure, &ScoreEntry)],
    critical: &[CriticalValue],
) -> Option<(Method, Vec<Option<Relative>>)> {
    let value_of = |key: &str| {
        entries
            .iter()
            .find(|(m, _)| m.key == key)
            .map(|(_, e)| e.value)
    };
    // The set of the sheet's age band: the one that holds the most subtests entered. A tie (only
    // subtests the bands share) says nothing about the band, so the manual's values, which differ
    // by band, are not used.
    let sets: Vec<(RelativeSet, usize)> = relative_sets()
        .into_iter()
        .filter(|s| s.instrument == instrument)
        .map(|s| {
            let n = s.measures.iter().filter(|m| value_of(m).is_some()).count();
            (s, n)
        })
        .collect();
    let best = sets.iter().map(|(_, n)| *n).max()?;
    let mut top = sets.iter().filter(|(_, n)| *n == best);
    let (set, _) = top.next()?;
    if top.next().is_some() {
        return None;
    }
    // Every primary subtest entered: the manual's values are for the mean of all of them.
    let values: Vec<f64> = set
        .measures
        .iter()
        .map(|m| value_of(m))
        .collect::<Option<_>>()?;
    let limits: Vec<f64> = set
        .measures
        .iter()
        .map(|m| {
            critical
                .iter()
                .find(|c| c.set == set.key && c.measure == *m)
                .map(|c| c.value)
        })
        .collect::<Option<_>>()?;
    #[allow(clippy::cast_precision_loss)]
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let places = entries
        .iter()
        .map(|(m, e)| {
            let i = set.measures.iter().position(|k| *k == m.key)?;
            // A hair of tolerance: 13 − 10.3 is not 2.6999… below a critical value of 2.7.
            let limit = limits.get(i)? - 1e-9;
            if e.value - mean >= limit {
                Some(Relative::Strength)
            } else if mean - e.value >= limit {
                Some(Relative::Weakness)
            } else {
                None
            }
        })
        .collect();
    Some((
        Method::Publisher {
            title_he: set.title_he.clone(),
            mean,
        },
        places,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_set_names_subtests_of_its_instrument() {
        let table = crate::instruments();
        for s in relative_sets() {
            let inst = table
                .iter()
                .find(|i| i.key == s.instrument)
                .unwrap_or_else(|| panic!("{}", s.key));
            for m in &s.measures {
                let measure = inst
                    .measures
                    .iter()
                    .find(|x| x.key == *m)
                    .unwrap_or_else(|| panic!("{}: {m}", s.key));
                assert_eq!(measure.scale, ScoreScale::Scaled, "{}: {m}", s.key);
            }
        }
    }

    #[test]
    fn critical_values_are_checked() {
        let v = |set: &str, measure: &str, value: f64| CriticalValue {
            set: set.into(),
            measure: measure.into(),
            value,
        };
        assert!(check_critical_values(&[v("wisc_v", "coding", 3.12)]).is_ok());
        assert!(check_critical_values(&[]).is_ok());
        for bad in [
            vec![v("wisc_vi", "coding", 3.0)],
            vec![v("wisc_v", "cancellation", 3.0)],
            vec![v("wisc_v", "coding", 0.0)],
            vec![v("wisc_v", "coding", -1.0)],
            vec![v("wisc_v", "coding", 9.5)],
            vec![v("wisc_v", "coding", f64::NAN)],
            vec![v("wisc_v", "coding", 3.125)],
            vec![v("wisc_v", "coding", 3.0), v("wisc_v", "coding", 3.0)],
        ] {
            assert!(check_critical_values(&bad).is_err(), "{bad:?}");
        }
    }

    /// A WPPSI-IV sheet of an older child: the ten primary subtests, an index and an ancillary
    /// subtest. Mean of the primary subtests: 100 / 10 = 10.
    fn older_wppsi() -> crate::ScoreSheet {
        let entries = [
            ("information", 10.0),
            ("similarities", 10.0),
            ("block_design", 7.0),
            ("object_assembly", 10.0),
            ("matrix", 13.0),
            ("picture_concepts", 10.0),
            ("picture_memory", 10.0),
            ("zoo_locations", 10.0),
            ("bug_search", 8.0),
            ("cancellation", 12.0),
            ("receptive_vocabulary", 4.0),
            ("fsiq", 100.0),
        ];
        crate::ScoreSheet {
            instrument: "wppsi_iv".into(),
            module: String::new(),
            cutoff: None,
            entries: entries
                .iter()
                .map(|(k, v)| ScoreEntry {
                    measure: (*k).into(),
                    value: *v,
                    note: String::new(),
                })
                .collect(),
            notes: String::new(),
        }
    }

    /// Invented critical values for the test (not a manual's): 2.5 for every subtest of a set.
    fn invented(set: &str) -> Vec<CriticalValue> {
        let s = relative_sets().into_iter().find(|s| s.key == set).unwrap();
        s.measures
            .iter()
            .map(|m| CriticalValue {
                set: set.into(),
                measure: m.clone(),
                value: 2.5,
            })
            .collect()
    }

    fn line(text: &str) -> &str {
        text.lines()
            .find(|l| l.starts_with("חוזקות וחולשות יחסיות"))
            .unwrap_or("")
    }

    #[test]
    fn with_every_critical_value_the_publishers_method_is_used() {
        let sheet = older_wppsi();
        let critical = invented("wppsi_iv_4_7");
        let cases = crate::sheet_cases_with(&sheet, &critical);
        let of = |m: &str| cases.iter().find(|c| c.measure == m).unwrap().cases.clone();
        let has = |m: &str, label: &str| of(m).iter().any(|c| c == label);
        // 13 − 10 = 3 ≥ 2.5; 7 − 10 = −3; 12 − 10 = 2 < 2.5; 8: −2.
        assert!(has("matrix", "חוזקה יחסית"));
        assert!(has("block_design", "חולשה יחסית"));
        assert!(!of("cancellation").iter().any(|c| c.ends_with("יחסית")));
        assert!(!of("bug_search").iter().any(|c| c.ends_with("יחסית")));
        // An ancillary subtest is not compared, however far it is from the mean.
        assert!(!of("receptive_vocabulary")
            .iter()
            .any(|c| c.ends_with("יחסית")));
        let text =
            crate::format_sheet_with(&sheet, crate::SheetHabits::default(), &critical).unwrap();
        let l = line(&text);
        assert!(l.contains("לפי שיטת המדריך"), "{l}");
        assert!(l.contains("הממוצע של תתי-המבחנים הראשיים (10)"), "{l}");
        assert!(l.contains("ברמת מובהקות .05"), "{l}");
        assert!(l.contains("WPPSI-IV, גילאי 4:0–7:7"), "{l}");
        assert!(l.contains("חוזקה יחסית: מטריצות (MR) – ציון 13"), "{l}");
        assert!(l.contains("חולשה יחסית: קוביות (BD) – ציון 7"), "{l}");
        assert!(
            !l.contains("כלל אצבע") && !l.contains("אוצר מילים רצפטיבי"),
            "{l}"
        );
        // A higher critical value: nothing reaches it, and the sheet says so.
        let strict: Vec<CriticalValue> = critical
            .iter()
            .map(|c| CriticalValue {
                value: 3.5,
                ..c.clone()
            })
            .collect();
        let text =
            crate::format_sheet_with(&sheet, crate::SheetHabits::default(), &strict).unwrap();
        assert!(
            line(&text).contains("לא נמצאו חוזקות או חולשות יחסיות מובהקות"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_value_or_subtest_keeps_the_labelled_rule_of_thumb() {
        let sheet = older_wppsi();
        // One critical value missing.
        let mut critical = invented("wppsi_iv_4_7");
        critical.pop();
        let text =
            crate::format_sheet_with(&sheet, crate::SheetHabits::default(), &critical).unwrap();
        assert!(line(&text).contains("לפי כלל אצבע"), "{text}");
        // Values of the other age band do not apply to this sheet.
        let text = crate::format_sheet_with(
            &sheet,
            crate::SheetHabits::default(),
            &invented("wppsi_iv_2_3"),
        )
        .unwrap();
        assert!(line(&text).contains("לפי כלל אצבע"), "{text}");
        // One primary subtest not entered: the manual's values are for the mean of all ten.
        let mut short = older_wppsi();
        short.entries.retain(|e| e.measure != "zoo_locations");
        let text = crate::format_sheet_with(
            &short,
            crate::SheetHabits::default(),
            &invented("wppsi_iv_4_7"),
        )
        .unwrap();
        assert!(line(&text).contains("לפי כלל אצבע"), "{text}");
    }

    #[test]
    fn subtests_both_bands_share_do_not_pick_a_band() {
        // Only subtests of both bands: the band is unknown, so no manual's value is used.
        let shared = [
            "information",
            "block_design",
            "object_assembly",
            "picture_memory",
        ];
        let entries: Vec<(&Measure, ScoreEntry)> = {
            let inst = crate::instruments()
                .into_iter()
                .find(|i| i.key == "wppsi_iv")
                .unwrap();
            let table = Box::leak(Box::new(inst));
            shared
                .iter()
                .map(|k| {
                    let m = table.measures.iter().find(|m| m.key == *k).unwrap();
                    (
                        m,
                        ScoreEntry {
                            measure: (*k).into(),
                            value: 10.0,
                            note: String::new(),
                        },
                    )
                })
                .collect()
        };
        let refs: Vec<(&Measure, &ScoreEntry)> = entries.iter().map(|(m, e)| (*m, e)).collect();
        let mut both = invented("wppsi_iv_4_7");
        both.extend(invented("wppsi_iv_2_3"));
        let (method, _) = relative_places("wppsi_iv", &refs, &both);
        assert_eq!(method, Method::RuleOfThumb);
    }
}
