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
    /// The ages of the band, in months (both ends included).
    pub min_age_months: u32,
    pub max_age_months: u32,
}

/// What a sheet's relative places are computed with, recorded with the sheet when it is saved
/// (D-190): her critical values at that moment and the child's age. The stored text and every
/// later use of the sheet (her interpretations, the checks) then agree, whatever she changes
/// in the settings afterwards.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RelativeBasis {
    pub critical: Vec<CriticalValue>,
    /// The child's age in months, when the case has one.
    pub age_months: Option<u32>,
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

fn set(
    key: &str,
    instrument: &str,
    title: &str,
    ages: (u32, u32),
    measures: &[&str],
) -> RelativeSet {
    RelativeSet {
        key: key.to_owned(),
        instrument: instrument.to_owned(),
        title_he: title.to_owned(),
        measures: measures.iter().map(|m| (*m).to_owned()).collect(),
        min_age_months: ages.0,
        max_age_months: ages.1,
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
            (72, 203),
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
            (48, 91),
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
            (30, 47),
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
            (48, 87),
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
            (30, 47),
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
    Publisher {
        title_he: String,
        mean: f64,
        /// WPPSI-III compares its core subtests ("עיקריים"), the others their primary ones.
        core: bool,
    },
    /// The 3-point rule of thumb against the mean of every subtest entered (three or more).
    RuleOfThumb,
    /// Fewer than three subtests: no relative place at all.
    None,
}

/// The relative place of each entry (in order), and how it was found.
pub(crate) fn relative_places(
    instrument: &str,
    entries: &[(&Measure, &ScoreEntry)],
    basis: Option<&RelativeBasis>,
) -> (Method, Vec<Option<Relative>>) {
    // A recorded basis that no longer passes the check is not used at all (fail-closed: the
    // labelled rule of thumb, never a number that might be wrong).
    if let Some(b) = basis.filter(|b| check_critical_values(&b.critical).is_ok()) {
        if let Some(found) = by_publisher(instrument, entries, b) {
            return found;
        }
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

/// The publisher's method, when the child's age band is known, every subtest of its set is
/// entered and every critical value of the set is there.
fn by_publisher(
    instrument: &str,
    entries: &[(&Measure, &ScoreEntry)],
    basis: &RelativeBasis,
) -> Option<(Method, Vec<Option<Relative>>)> {
    let value_of = |key: &str| {
        entries
            .iter()
            .find(|(m, _)| m.key == key)
            .map(|(_, e)| e.value)
    };
    let critical = &basis.critical;
    // The set of the child's age band. Without an age, only an instrument with one set (WISC-V)
    // has one: the subtests entered do not tell the band, and the manual's values differ by it.
    let sets: Vec<RelativeSet> = relative_sets()
        .into_iter()
        .filter(|s| s.instrument == instrument)
        .collect();
    let set = match basis.age_months {
        Some(age) => sets
            .into_iter()
            .find(|s| (s.min_age_months..=s.max_age_months).contains(&age))?,
        None if sets.len() == 1 => sets.into_iter().next()?,
        None => return None,
    };
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
            core: set.instrument == "wppsi_iii",
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
            basis: None,
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

    /// The sheet with a recorded basis: these values, a child of `age` months.
    fn with(
        mut sheet: crate::ScoreSheet,
        critical: Vec<CriticalValue>,
        age: Option<u32>,
    ) -> crate::ScoreSheet {
        sheet.basis = Some(RelativeBasis {
            critical,
            age_months: age,
        });
        sheet
    }

    fn text(sheet: &crate::ScoreSheet) -> String {
        crate::format_sheet(sheet).unwrap()
    }

    fn sheet_of(instrument: &str, entries: &[(&str, f64)]) -> crate::ScoreSheet {
        crate::ScoreSheet {
            instrument: instrument.into(),
            entries: entries
                .iter()
                .map(|(k, v)| ScoreEntry {
                    measure: (*k).into(),
                    value: *v,
                    note: String::new(),
                })
                .collect(),
            ..older_wppsi()
        }
    }

    #[test]
    fn with_every_critical_value_the_publishers_method_is_used() {
        let critical = invented("wppsi_iv_4_7");
        let sheet = with(older_wppsi(), critical.clone(), Some(60));
        let cases = crate::sheet_cases(&sheet);
        let of = |m: &str| -> Vec<String> {
            cases.iter().find(|c| c.measure == m).unwrap().cases.clone()
        };
        let relative = |m: &str| of(m).iter().any(|c| c.ends_with("יחסית"));
        // 13 − 10 = 3 ≥ 2.5; 7 − 10 = −3; 12 − 10 = 2 < 2.5; 8: −2.
        assert!(of("matrix").contains(&"חוזקה יחסית".to_owned()));
        assert!(of("block_design").contains(&"חולשה יחסית".to_owned()));
        assert!(!relative("cancellation") && !relative("bug_search"));
        // An ancillary subtest is not compared, however far it is from the mean.
        assert!(!relative("receptive_vocabulary"));
        let t = text(&sheet);
        let l = line(&t);
        assert!(l.contains("לפי שיטת המדריך"), "{l}");
        assert!(
            l.contains("תתי-המבחנים הראשיים מול הממוצע שלהם (10)"),
            "{l}"
        );
        assert!(l.contains("ברמת מובהקות .05"), "{l}");
        assert!(l.contains("WPPSI-IV, גילאי 4:0–7:7"), "{l}");
        assert!(l.contains("חוזקה יחסית: מטריצות (MR) – ציון 13"), "{l}");
        assert!(l.contains("חולשה יחסית: קוביות (BD) – ציון 7"), "{l}");
        assert!(
            !l.contains("כלל אצבע") && !l.contains("אוצר מילים רצפטיבי"),
            "{l}"
        );
        // A higher critical value: nothing reaches it, and the sheet says so.
        let strict = critical
            .iter()
            .map(|c| CriticalValue {
                value: 3.5,
                ..c.clone()
            })
            .collect();
        let t = text(&with(older_wppsi(), strict, Some(60)));
        assert!(
            line(&t).contains("לא נמצאו חוזקות או חולשות יחסיות מובהקות"),
            "{t}"
        );
    }

    #[test]
    fn a_missing_value_age_or_subtest_keeps_the_labelled_rule_of_thumb() {
        let thumb = |sheet: crate::ScoreSheet| {
            let t = text(&sheet);
            assert!(line(&t).contains("לפי כלל אצבע"), "{t}");
        };
        // No basis recorded (a sheet saved before D-190).
        thumb(older_wppsi());
        // One critical value missing.
        let mut critical = invented("wppsi_iv_4_7");
        critical.pop();
        thumb(with(older_wppsi(), critical, Some(60)));
        // No age: WPPSI-IV has two bands, and the subtests do not tell which.
        thumb(with(older_wppsi(), invented("wppsi_iv_4_7"), None));
        // Values of the other age band do not apply to this child.
        thumb(with(older_wppsi(), invented("wppsi_iv_2_3"), Some(60)));
        // One primary subtest not entered: the manual's values are for the mean of all ten.
        let mut short = older_wppsi();
        short.entries.retain(|e| e.measure != "zoo_locations");
        thumb(with(short, invented("wppsi_iv_4_7"), Some(60)));
        // A recorded value that no longer passes the check is not used.
        let mut bad = invented("wppsi_iv_4_7");
        if let Some(first) = bad.first_mut() {
            first.value = 40.0;
        }
        thumb(with(older_wppsi(), bad, Some(60)));
    }

    #[test]
    fn the_band_comes_from_the_age_not_from_the_subtests_entered() {
        // An older child (5:0) given the six subtests of the young band (RV is ancillary at
        // this age): the young band's values never apply to them.
        let young = [
            "receptive_vocabulary",
            "information",
            "block_design",
            "object_assembly",
            "picture_memory",
            "zoo_locations",
        ];
        let mut sheet = older_wppsi();
        sheet
            .entries
            .retain(|e| young.contains(&e.measure.as_str()));
        let t = text(&with(sheet.clone(), invented("wppsi_iv_2_3"), Some(60)));
        assert!(line(&t).contains("לפי כלל אצבע"), "{t}");
        // A child of 3:0 with the same subtests: the young band.
        let t = text(&with(sheet, invented("wppsi_iv_2_3"), Some(36)));
        assert!(line(&t).contains("WPPSI-IV, גילאי 2:6–3:11"), "{t}");
        // WISC-V has one set: no age is needed.
        let wisc = sheet_of(
            "wisc_v",
            &[
                ("similarities", 10.0),
                ("vocabulary", 10.0),
                ("block_design", 10.0),
                ("visual_puzzles", 10.0),
                ("matrix", 10.0),
                ("figure_weights", 10.0),
                ("digit_span", 10.0),
                ("picture_span", 10.0),
                ("coding", 14.0),
                ("symbol_search", 6.0),
            ],
        );
        let t = text(&with(wisc, invented("wisc_v"), None));
        assert!(line(&t).contains("לפי שיטת המדריך"), "{t}");
    }

    #[test]
    fn wppsi_iii_says_core_subtests() {
        let sheet = sheet_of(
            "wppsi_iii",
            &[
                ("information", 10.0),
                ("vocabulary", 10.0),
                ("word_reasoning", 10.0),
                ("block_design", 10.0),
                ("matrix", 10.0),
                ("picture_concepts", 10.0),
                ("coding", 10.0),
            ],
        );
        let t = text(&with(sheet, invented("wppsi_iii_4_7"), Some(60)));
        assert!(line(&t).contains("תתי-המבחנים העיקריים"), "{t}");
    }
}
