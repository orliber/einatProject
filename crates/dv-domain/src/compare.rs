//! Follow-up assessments (D-029): the same measures before and after, compared by fixed rules.
//! No interpretation beyond the numbers and the tables; the psychologist decides what it means.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::scores::{classify, instruments, Instrument, Measure, ScoreScale, ScoreSheet};

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
        ScoreScale::Scaled => 3.0,
        ScoreScale::TConcern => 10.0,
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

/// Measures entered in both assessments, with the same key and scale (so WPPSI-IV and WISC-V
/// indexes compare, but not a subtest that exists in only one of them), in the order entered.
#[must_use]
pub fn compare(before: &[ScoreSheet], after: &[ScoreSheet]) -> Vec<ComparisonRow> {
    let all = instruments();
    let mut rows = Vec::new();
    for a in after {
        for entry in &a.entries {
            let Some((a_inst, a_m)) = measure_of(&all, a, &entry.measure) else {
                continue;
            };
            let earlier = before.iter().find_map(|b| {
                let (b_inst, b_m) = measure_of(&all, b, &entry.measure)?;
                let e = b.entries.iter().find(|e| e.measure == entry.measure)?;
                (b_m.scale == a_m.scale).then_some((b_inst, b_m, e.value))
            });
            let Some((b_inst, b_m, value)) = earlier else {
                continue;
            };
            if rows
                .iter()
                .any(|r: &ComparisonRow| r.abbr == a_m.abbr && r.measure == a_m.name_he)
            {
                continue;
            }
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
    for r in rows {
        let sign = if r.change > 0.0 { "+" } else { "" };
        let same = r.before_instrument == r.after_instrument;
        out.push_str(&format!(
            "{} ({}): {} ← {} (שינוי {}{}{}); טווח: {} ← {}{}\n",
            r.measure,
            r.abbr,
            num(r.before),
            num(r.after),
            sign,
            num(r.change),
            if r.notable {
                ", שינוי ששווה לבדוק"
            } else {
                ""
            },
            r.before_band,
            r.after_band,
            if same {
                String::new()
            } else {
                format!(" ({} ← {})", r.before_instrument, r.after_instrument)
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
}
