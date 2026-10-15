//! Properties of the score tables: the range of a score comes from code, so the code must be a
//! function in the plain sense: every possible score has exactly one range, a better score never
//! reads worse, the cut-offs sit exactly where the tables say, and a score that cannot exist is
//! refused rather than given the nearest range. Invented scores only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use dv_domain::scores::{classify, percentile, Band, Measure, ScoreScale};
use dv_domain::{format_sheet, instruments, sheet_profiles, sheet_table, ScoreEntry, ScoreSheet};
use proptest::prelude::*;

/// Every measure that has a table, with its instrument key.
fn tabled() -> Vec<(String, Measure)> {
    instruments()
        .into_iter()
        .flat_map(|i| {
            let key = i.key.clone();
            i.measures.into_iter().map(move |m| (key.clone(), m))
        })
        .filter(|(_, m)| !m.bands.is_empty())
        .collect()
}

fn measure(inst: &str, key: &str) -> Measure {
    tabled()
        .into_iter()
        .find(|(i, m)| i == inst && m.key == key)
        .map(|(_, m)| m)
        .unwrap_or_else(|| panic!("{inst}/{key}"))
}

/// The smallest step a score of this scale is written in (CARS-2 items are scored in halves).
fn step(scale: ScoreScale) -> f64 {
    if scale == ScoreScale::Cars {
        0.5
    } else {
        1.0
    }
}

/// Every score that can be written on the measure's scale, from its minimum to its maximum.
fn grid(m: &Measure) -> Vec<f64> {
    let s = step(m.scale);
    let n = ((m.max - m.min) / s).round() as i64;
    (0..=n).map(|k| m.min + s * k as f64).collect()
}

/// True when a higher score means more concern (questionnaires, CARS-2, ADOS-2 comparison);
/// false when a higher score means better functioning (Wechsler, Bayley, ABAS).
fn higher_is_concern(scale: ScoreScale) -> bool {
    !matches!(
        scale,
        ScoreScale::Standard | ScoreScale::Scaled | ScoreScale::VScale | ScoreScale::TAbility
    )
}

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

/// "79" → 79, "<0.1" → 0.05, ">99.9" → 99.95: percentiles as numbers, to compare them.
fn pct_number(p: &str) -> f64 {
    match p {
        "<0.1" => 0.05,
        ">99.9" => 99.95,
        _ => p
            .parse()
            .unwrap_or_else(|_| panic!("not a percentile: {p}")),
    }
}

#[test]
fn every_score_on_every_scale_falls_in_exactly_one_range() {
    for (inst, m) in tabled() {
        for v in grid(&m) {
            let holding: Vec<&Band> = m
                .bands
                .iter()
                .filter(|b| v >= b.min && v <= b.max)
                .collect();
            assert_eq!(
                holding.len(),
                1,
                "{inst}/{}: score {v} is in {} ranges",
                m.key,
                holding.len()
            );
            assert_eq!(classify(&m, v), Some(holding[0]), "{inst}/{} {v}", m.key);
        }
    }
}

#[test]
fn a_better_score_never_reads_worse() {
    for (inst, m) in tabled() {
        let levels: Vec<u8> = grid(&m)
            .into_iter()
            .map(|v| classify(&m, v).map_or(u8::MAX, |b| b.level))
            .collect();
        for w in levels.windows(2) {
            let ok = if higher_is_concern(m.scale) {
                w[0] <= w[1]
            } else {
                w[0] >= w[1]
            };
            assert!(ok, "{inst}/{}: levels {levels:?}", m.key);
        }
        // And the ranges themselves follow the scale: each band once, in one run.
        let mut seen: Vec<&str> = Vec::new();
        for v in grid(&m) {
            let label = classify(&m, v).map(|b| b.label.as_str()).unwrap_or("");
            if seen.last() != Some(&label) {
                assert!(
                    !seen.contains(&label),
                    "{inst}/{}: range «{label}» comes back at {v}",
                    m.key
                );
                seen.push(label);
            }
        }
    }
}

#[test]
fn cut_offs_sit_exactly_where_the_tables_say() {
    let idx = measure("wisc_v", "fsiq");
    for (below, at, low_label, high_label) in [
        (69.0, 70.0, "נמוך מאוד", "גבולי"),
        (79.0, 80.0, "גבולי", "ממוצע נמוך"),
        (89.0, 90.0, "ממוצע נמוך", "ממוצע"),
        (109.0, 110.0, "ממוצע", "ממוצע גבוה"),
        (119.0, 120.0, "ממוצע גבוה", "גבוה"),
        (129.0, 130.0, "גבוה", "גבוה מאוד"),
    ] {
        assert_eq!(
            classify(&idx, below).map(|b| b.label.as_str()),
            Some(low_label)
        );
        assert_eq!(
            classify(&idx, at).map(|b| b.label.as_str()),
            Some(high_label)
        );
    }
    let sub = measure("wisc_v", "digit_span");
    for (below, at, low_label, high_label) in [
        (3.0, 4.0, "נמוך מאוד", "נמוך"),
        (5.0, 6.0, "נמוך", "ממוצע נמוך"),
        (7.0, 8.0, "ממוצע נמוך", "ממוצע"),
        (12.0, 13.0, "ממוצע", "ממוצע גבוה"),
        (15.0, 16.0, "ממוצע גבוה", "גבוה מאוד"),
    ] {
        assert_eq!(
            classify(&sub, below).map(|b| b.label.as_str()),
            Some(low_label)
        );
        assert_eq!(
            classify(&sub, at).map(|b| b.label.as_str()),
            Some(high_label)
        );
    }
    // CARS-2 in half points: 29.5 is the last "minimal", 30 the first "mild-moderate".
    let cars = measure("cars_2", "total");
    assert_eq!(classify(&cars, 29.5).map(|b| b.level), Some(0));
    assert_eq!(classify(&cars, 30.0).map(|b| b.level), Some(2));
    assert_eq!(classify(&cars, 36.5).map(|b| b.level), Some(2));
    assert_eq!(classify(&cars, 37.0).map(|b| b.level), Some(3));
    // Every band of every table holds its own two ends, and the next band starts one step on.
    for (inst, m) in tabled() {
        let mut bands = m.bands.clone();
        bands.sort_by(|a, b| a.min.total_cmp(&b.min));
        for b in &bands {
            assert_eq!(classify(&m, b.min), Some(b), "{inst}/{} {}", m.key, b.min);
            assert_eq!(classify(&m, b.max), Some(b), "{inst}/{} {}", m.key, b.max);
        }
        for w in bands.windows(2) {
            assert!(
                (w[1].min - w[0].max - step(m.scale)).abs() < 1e-9,
                "{inst}/{}: gap or overlap between {} and {}",
                m.key,
                w[0].max,
                w[1].min
            );
        }
        assert_eq!(
            bands.first().map(|b| b.min),
            Some(m.min),
            "{inst}/{}",
            m.key
        );
        assert_eq!(bands.last().map(|b| b.max), Some(m.max), "{inst}/{}", m.key);
    }
}

#[test]
fn a_score_that_cannot_exist_gets_no_range() {
    for (inst, m) in tabled() {
        for v in [
            m.min - step(m.scale),
            m.min - 1e-6,
            m.max + 1e-6,
            m.max + step(m.scale),
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1000.0,
        ] {
            assert_eq!(classify(&m, v), None, "{inst}/{} {v}", m.key);
        }
    }
}

#[test]
fn a_score_that_cannot_exist_is_refused_not_written() {
    for v in [39.0, 161.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let s = sheet("wppsi_iv", &[("vci", v)]);
        assert!(format_sheet(&s).is_err(), "format_sheet accepted {v}");
        assert!(sheet_table(&s).is_err(), "sheet_table accepted {v}");
        assert!(sheet_profiles(&s).is_err(), "sheet_profiles accepted {v}");
    }
    // An unknown instrument or measure is refused, never matched to something close.
    assert!(format_sheet(&sheet("wppsi_v", &[("vci", 100.0)])).is_err());
    assert!(format_sheet(&sheet("wppsi_iv", &[("VCI", 100.0)])).is_err());
    assert!(format_sheet(&sheet("wppsi_iv", &[("vci ", 100.0)])).is_err());
}

#[test]
fn a_score_between_two_ranges_is_not_given_the_nearest_one() {
    // Composite scores are whole numbers; 109.5 is in neither 90–109 nor 110–119.
    let idx = measure("wppsi_iv", "vci");
    assert_eq!(classify(&idx, 109.5), None);
    let text = format_sheet(&sheet("wppsi_iv", &[("vci", 109.5)])).unwrap();
    let line = text.lines().find(|l| l.contains("(VCI)")).unwrap();
    for b in &idx.bands {
        assert!(
            !line.contains(b.label.as_str()),
            "a range was guessed: {line}"
        );
    }
}

#[test]
fn percentiles_rise_with_the_score_and_mirror_around_the_mean() {
    let idx = measure("wppsi_iv", "vci");
    let sub = measure("wppsi_iv", "matrix");
    for m in [&idx, &sub] {
        let p: Vec<f64> = grid(m)
            .iter()
            .map(|v| pct_number(&percentile(m, *v).unwrap()))
            .collect();
        assert!(p.windows(2).all(|w| w[0] <= w[1]), "{}: {p:?}", m.key);
        assert!(p.iter().all(|x| (0.0..=100.0).contains(x)));
    }
    // 100 ± d (and 10 ± d) sit on either side of the 50th percentile, the same distance away.
    for d in 0..=60 {
        let d = f64::from(d);
        let lo = pct_number(&percentile(&idx, 100.0 - d).unwrap());
        let hi = pct_number(&percentile(&idx, 100.0 + d).unwrap());
        assert!((lo + hi - 100.0).abs() <= 0.11, "±{d}: {lo} + {hi}");
    }
    for d in 0..=9 {
        let d = f64::from(d);
        let lo = pct_number(&percentile(&sub, 10.0 - d).unwrap());
        let hi = pct_number(&percentile(&sub, 10.0 + d).unwrap());
        assert!((lo + hi - 100.0).abs() <= 0.11, "±{d}: {lo} + {hi}");
    }
}

#[test]
fn a_standard_and_a_scaled_score_at_the_same_distance_share_a_percentile() {
    // 100 + 5k on the composite scale and 10 + k on the subtest scale are both k/3 SD away.
    let idx = measure("wisc_v", "vci");
    let sub = measure("wisc_v", "coding");
    for k in -9..=9 {
        let k = f64::from(k);
        assert_eq!(
            percentile(&idx, 100.0 + 5.0 * k),
            percentile(&sub, 10.0 + k),
            "k = {k}"
        );
    }
}

#[test]
fn the_average_range_matches_its_percentiles() {
    // The Wechsler "average" range is the middle half of the curve, about the 25th to 75th.
    let idx = measure("wppsi_iv", "fsiq");
    for v in 90..=109 {
        let p = pct_number(&percentile(&idx, f64::from(v)).unwrap());
        assert!((25.0..=75.0).contains(&p), "{v} → {p}");
    }
    // Just outside it, the percentile leaves the middle half too.
    assert!(pct_number(&percentile(&idx, 89.0).unwrap()) < 25.0);
    assert!(pct_number(&percentile(&idx, 110.0).unwrap()) >= 75.0);
}

#[test]
fn only_ability_scores_on_the_normal_curve_have_percentiles() {
    for (inst, m) in tabled() {
        let has = percentile(&m, m.min).is_some();
        let should = matches!(
            m.scale,
            ScoreScale::Standard | ScoreScale::Scaled | ScoreScale::VScale | ScoreScale::TAbility
        );
        assert_eq!(has, should, "{inst}/{}", m.key);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Any real number, not only the grid: no two ranges ever claim it, and the one `classify`
    /// returns (if any) holds it.
    #[test]
    fn no_value_is_claimed_by_two_ranges(i in 0usize..1000, t in 0.0f64..=1.0) {
        let all = tabled();
        let (_, m) = &all[i % all.len()];
        let v = m.min + t * (m.max - m.min);
        let claims = m.bands.iter().filter(|b| v >= b.min && v <= b.max).count();
        prop_assert!(claims <= 1);
        if let Some(b) = classify(m, v) {
            prop_assert!(v >= b.min && v <= b.max + f64::EPSILON);
        }
    }

    /// Two real scores in order never get their levels the wrong way round.
    #[test]
    fn levels_are_monotonic_for_any_pair(i in 0usize..1000, a in 0.0f64..=1.0, b in 0.0f64..=1.0) {
        let all = tabled();
        let (_, m) = &all[i % all.len()];
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (x, y) = (m.min + lo * (m.max - m.min), m.min + hi * (m.max - m.min));
        if let (Some(p), Some(q)) = (classify(m, x), classify(m, y)) {
            if higher_is_concern(m.scale) {
                prop_assert!(p.level <= q.level, "{} {x}→{} {y}→{}", m.key, p.level, q.level);
            } else {
                prop_assert!(p.level >= q.level, "{} {x}→{} {y}→{}", m.key, p.level, q.level);
            }
        }
    }

    /// The percentile is a monotonic function of the score for any real score, and is always
    /// printed in one of the three Wechsler forms.
    #[test]
    fn percentile_is_monotonic_and_well_formed(a in 40.0f64..=160.0, b in 40.0f64..=160.0) {
        let idx = measure("wppsi_iv", "vci");
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (p, q) = (percentile(&idx, lo).unwrap(), percentile(&idx, hi).unwrap());
        prop_assert!(pct_number(&p) <= pct_number(&q), "{lo}→{p} {hi}→{q}");
        for s in [&p, &q] {
            let n = pct_number(s);
            prop_assert!(s == "<0.1" || s == ">99.9" || (0.1..=99.9).contains(&n), "{s}");
        }
    }

    /// The same sheet always gives the same text, table and profile (no hidden state).
    #[test]
    fn sheet_output_is_deterministic(vci in 40u8..=160, psi in 40u8..=160, mr in 1u8..=19) {
        let s = sheet(
            "wisc_v",
            &[("vci", f64::from(vci)), ("psi", f64::from(psi)), ("matrix", f64::from(mr))],
        );
        prop_assert_eq!(format_sheet(&s), format_sheet(&s));
        prop_assert_eq!(sheet_table(&s), sheet_table(&s));
        prop_assert_eq!(sheet_profiles(&s), sheet_profiles(&s));
        // The Word table and the text sent for drafting show the same range for each score.
        let text = format_sheet(&s).unwrap();
        let (_, rows, _) = sheet_table(&s).unwrap();
        for r in rows {
            prop_assert!(text.contains(&format!("{}: ציון {}", r.measure, r.score)), "{text}");
            prop_assert!(text.contains(&r.range), "{} not in {text}", r.range);
        }
    }
}
