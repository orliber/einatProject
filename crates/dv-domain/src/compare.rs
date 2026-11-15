//! Follow-up assessments (D-029): the same measures before and after, compared by fixed rules.
//! No interpretation beyond the numbers and the tables; the psychologist decides what it means.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::scores::{classify, table, Instrument, Measure, ScoreScale, ScoreSheet};

/// One measure in both assessments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ComparisonRow {
    pub measure: String,
    pub abbr: String,
    pub before_instrument: String,
    pub after_instrument: String,
    pub before: f64,
    pub after: f64,
    pub change: f64,
    pub before_band: String,
    pub after_band: String,
    /// The change is at least the size this scale treats as worth a look (see [`notable`]).
    pub notable: bool,
}

/// A change worth pointing out: 10 points on a standard score (2/3 SD), 3 on a scaled score
/// (1 SD), 10 on a T score (1 SD). Smaller changes are usually within measurement error.
#[must_use]
pub fn notable(scale: ScoreScale, change: f64) -> bool {
    let at_least = match scale {
        ScoreScale::Standard => 10.0,
        ScoreScale::Scaled | ScoreScale::VScale => 3.0,
        ScoreScale::TConcern | ScoreScale::TAbility => 10.0,
        _ => return false,
    };
    change.abs() >= at_least
}

fn measure_of<'a>(
    all: &'a [Instrument],
    sheet: &ScoreSheet,
    key: &str,
) -> Option<(&'a Instrument, &'a Measure)> {
    let inst = all.iter().find(|i| i.key == sheet.instrument)?;
    Some((inst, inst.measures.iter().find(|m| m.key == key)?))
}

/// Tests of one family, whose shared measures (FSIQ, the indexes) mean the same thing.
const WECHSLER: [&str; 3] = ["wppsi_iii", "wppsi_iv", "wisc_v"];

/// Measures entered in both assessments, in the order entered. A measure is paired with the
/// same instrument's earlier score; only when that instrument was not used before, and only
/// inside the Wechsler family, with another test's measure of the same key and scale (so
/// WPPSI-IV and WISC-V indexes compare, but a Bayley motor score never meets a Vineland one).
#[must_use]
pub fn compare(before: &[ScoreSheet], after: &[ScoreSheet]) -> Vec<ComparisonRow> {
    let all = table();
    let mut rows = Vec::new();
    let mut seen: Vec<(&str, &str, &str)> = Vec::new();
    for a in after {
        for entry in &a.entries {
            let Some((a_inst, a_m)) = measure_of(all, a, &entry.measure) else {
                continue;
            };
            let earlier_in = |same_instrument: bool| {
                before.iter().find_map(|b| {
                    let fits = if same_instrument {
                        // The same module too: ADOS-2 modules are scored differently, and a
                        // total of module 1 is not comparable with a total of module 2.
                        b.instrument == a.instrument && b.module == a.module
                    } else {
                        b.instrument != a.instrument
                            && WECHSLER.contains(&a.instrument.as_str())
                            && WECHSLER.contains(&b.instrument.as_str())
                    };
                    if !fits {
                        return None;
                    }
                    let (b_inst, b_m) = measure_of(all, b, &entry.measure)?;
                    let e = b.entries.iter().find(|e| e.measure == entry.measure)?;
                    (b_m.scale == a_m.scale).then_some((b_inst, b_m, e.value))
                })
            };
            let Some((b_inst, b_m, value)) = earlier_in(true).or_else(|| earlier_in(false)) else {
                continue;
            };
            let id = (
                a.instrument.as_str(),
                a.module.as_str(),
                entry.measure.as_str(),
            );
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let change = entry.value - value;
            let band =
                |m: &Measure, v: f64| classify(m, v).map(|b| b.label.clone()).unwrap_or_default();
            rows.push(ComparisonRow {
                measure: a_m.name_he.clone(),
                abbr: a_m.abbr.clone(),
                before_instrument: b_inst.name.clone(),
                after_instrument: a_inst.name.clone(),
                before: value,
                after: entry.value,
                change,
                before_band: band(b_m, value),
                after_band: band(a_m, entry.value),
                notable: notable(a_m.scale, change),
            });
        }
    }
    rows
}

/// More than one test is compared, so every row names its test ("ציון כולל" alone would not
/// say whose total it is).
#[must_use]
pub fn several_instruments(rows: &[ComparisonRow]) -> bool {
    rows.first().is_some_and(|first| {
        rows.iter()
            .any(|r| r.after_instrument != first.after_instrument)
    })
}

fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// The comparison as a material the sections can be written from.
#[must_use]
pub fn comparison_text(rows: &[ComparisonRow]) -> String {
    let mut out = String::from("השוואה לאבחון הקודם (מדדים שנמדדו בשני האבחונים):\n");
    let several = several_instruments(rows);
    for r in rows {
        let sign = if r.change > 0.0 { "+" } else { "" };
        let same = r.before_instrument == r.after_instrument;
        let abbr = if r.abbr.is_empty() {
            String::new()
        } else {
            format!(" ({})", r.abbr)
        };
        let bands = match (r.before_band.is_empty(), r.after_band.is_empty()) {
            (true, true) => String::new(),
            (false, false) => format!("; טווח: {} ← {}", r.before_band, r.after_band),
            (false, true) => format!("; טווח קודם: {}", r.before_band),
            (true, false) => format!("; טווח עכשיו: {}", r.after_band),
        };
        let tests = if !same {
            format!(" ({} ← {})", r.before_instrument, r.after_instrument)
        } else if several {
            format!(" ({})", r.after_instrument)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "{}{abbr}: {} ← {} (שינוי {sign}{}{}){bands}{tests}\n",
            r.measure,
            num(r.before),
            num(r.after),
            num(r.change),
            if r.notable {
                ", שינוי ששווה לבדוק"
            } else {
                ""
            },
        ));
    }
    out.push_str("הפרש קטן מ-10 נקודות בציון תקן (3 בציון מותאם) הוא לרוב בגבולות טעות המדידה. ");
    if rows
        .iter()
        .any(|r| r.before_instrument != r.after_instrument)
    {
        out.push_str("המבחנים שונים בין שני האבחונים, ולכן ההשוואה זהירה.");
    }
    out.trim_end().to_owned()
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
            entries: entries
                .iter()
                .map(|(m, v)| ScoreEntry {
                    measure: (*m).into(),
                    value: *v,
                    note: String::new(),
                })
                .collect(),
            notes: String::new(),
        }
    }

    #[test]
    fn indexes_compare_across_wechsler_tests_and_only_real_changes_are_notable() {
        let before = [sheet(
            "wppsi_iv",
            &[("fsiq", 98.0), ("wmi", 86.0), ("zoo_locations", 8.0)],
        )];
        let after = [sheet(
            "wisc_v",
            &[("fsiq", 101.0), ("wmi", 97.0), ("digit_span", 9.0)],
        )];
        let rows = compare(&before, &after);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(
            (rows[0].abbr.as_str(), rows[0].change, rows[0].notable),
            ("FSIQ", 3.0, false)
        );
        assert_eq!(
            (rows[1].abbr.as_str(), rows[1].change, rows[1].notable),
            ("WMI", 11.0, true)
        );
        let text = comparison_text(&rows);
        assert!(
            text.contains("זיכרון עבודה (WMI): 86 ← 97 (שינוי +11, שינוי ששווה לבדוק)"),
            "{text}"
        );
        assert!(
            text.contains("WPPSI-IV ← WISC-V") && text.contains("ההשוואה זהירה"),
            "{text}"
        );
    }

    #[test]
    fn nothing_in_common_means_no_rows() {
        let before = [sheet("abas_3", &[("gac", 90.0)])];
        let after = [sheet("wisc_v", &[("fsiq", 101.0)])];
        assert!(compare(&before, &after).is_empty());
    }

    #[test]
    fn a_measure_is_paired_with_the_same_test_and_never_with_another_family() {
        let before = [
            sheet("bayley_4", &[("motor", 85.0)]),
            sheet("vineland_3", &[("motor", 95.0)]),
        ];
        let after = [sheet("vineland_3", &[("motor", 90.0)])];
        let rows = compare(&before, &after);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!((rows[0].before, rows[0].after), (95.0, 90.0));
        let rows = compare(
            &[sheet("asrs", &[("total", 70.0)])],
            &[sheet("cbcl_1_5", &[("total", 66.0)])],
        );
        assert!(rows.is_empty(), "{rows:?}");
    }

    #[test]
    fn two_tests_with_a_total_give_two_rows_that_name_their_test() {
        let before = [
            sheet("ados_2", &[("total", 14.0)]),
            sheet("cars_2", &[("total", 38.0)]),
        ];
        let after = [
            sheet("ados_2", &[("total", 12.0)]),
            sheet("cars_2", &[("total", 31.0)]),
        ];
        let rows = compare(&before, &after);
        assert_eq!(rows.len(), 2, "{rows:?}");
        let text = comparison_text(&rows);
        assert!(text.contains("38 ← 31"), "{text}");
        assert!(
            text.contains("(ADOS-2)") && text.contains("(CARS-2)"),
            "{text}"
        );
    }

    #[test]
    fn an_empty_abbreviation_or_range_is_left_out_of_the_text() {
        let rows = compare(
            &[
                sheet("cbcl_1_5", &[("total", 68.0)]),
                sheet("ados_2", &[("total", 14.0)]),
            ],
            &[
                sheet("cbcl_1_5", &[("total", 72.0)]),
                sheet("ados_2", &[("total", 12.0)]),
            ],
        );
        assert_eq!(rows.len(), 2, "{rows:?}");
        let text = comparison_text(&rows);
        assert!(!text.contains("()"), "{text}");
        assert!(
            !text.contains("טווח:  ←") && !text.contains("טווח: ;"),
            "{text}"
        );
        for line in text.lines() {
            assert!(!line.trim_end().ends_with("טווח:"), "{line}");
        }
    }

    #[test]
    fn another_module_of_the_same_test_is_not_compared() {
        let module = |m: &str, total: f64| {
            let mut s = sheet("ados_2", &[("total", total)]);
            s.module = m.into();
            s
        };
        assert!(compare(&[module("1", 14.0)], &[module("2", 12.0)]).is_empty());
        let rows = compare(&[module("2", 14.0)], &[module("2", 12.0)]);
        assert_eq!(rows.len(), 1, "{rows:?}");
        // Two sheets of the same test, one per module, each against its own earlier sheet.
        let rows = compare(
            &[module("1", 16.0), module("2", 14.0)],
            &[module("1", 15.0), module("2", 12.0)],
        );
        assert_eq!(rows.len(), 2, "{rows:?}");
    }
}
