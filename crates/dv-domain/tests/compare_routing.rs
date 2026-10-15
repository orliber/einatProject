//! Follow-up comparison and routing are fixed rules: the same input always gives the same
//! answer, nothing in a material is lost when it is cut into passages, and Einat's choice of
//! sections always wins. Invented text and scores only.

#![allow(clippy::string_slice)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use dv_domain::compare::notable;
use dv_domain::scores::{classify, ScoreScale};
use dv_domain::{
    compare, comparison_text, instruments, passage_ranges, passages, Feed, Routing, ScoreEntry,
    ScoreSheet, SectionPassages, Suggestion,
};
use proptest::prelude::*;

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

fn label(instrument: &str, key: &str, v: f64) -> String {
    let inst = instruments()
        .into_iter()
        .find(|i| i.key == instrument)
        .unwrap();
    let m = inst.measures.iter().find(|m| m.key == key).unwrap();
    classify(m, v).map(|b| b.label.clone()).unwrap_or_default()
}

#[test]
fn notable_change_thresholds_are_exact_and_symmetric() {
    for (scale, at) in [
        (ScoreScale::Standard, 10.0),
        (ScoreScale::Scaled, 3.0),
        (ScoreScale::TConcern, 10.0),
    ] {
        assert!(notable(scale, at) && notable(scale, -at), "{scale:?} {at}");
        assert!(!notable(scale, at - 1e-9) && !notable(scale, -(at - 1e-9)));
        assert!(!notable(scale, 0.0));
    }
    // Scales with no agreed measurement error never call a change notable.
    for scale in [
        ScoreScale::RawCutoff,
        ScoreScale::Cars,
        ScoreScale::Comparison,
    ] {
        assert!(
            !notable(scale, 100.0) && !notable(scale, -100.0),
            "{scale:?}"
        );
    }
    assert!(!notable(ScoreScale::Standard, f64::NAN));
}

#[test]
fn a_measure_on_a_different_scale_is_not_compared() {
    // "motor" is a composite (mean 100) in Bayley-4 but a skill area (mean 10) in ABAS-3.
    let before = [sheet("bayley_4", &[("motor", 85.0)])];
    let after = [sheet("abas_3", &[("motor", 8.0)])];
    assert!(compare(&before, &after).is_empty());
    // Unknown instruments and measures are skipped, not matched by a similar name.
    let before = [sheet("wisc_v", &[("fsiq", 90.0)])];
    assert!(compare(&before, &[sheet("wisc_vi", &[("fsiq", 95.0)])]).is_empty());
    assert!(compare(&before, &[sheet("wisc_v", &[("FSIQ", 95.0)])]).is_empty());
}

#[test]
fn a_measure_entered_twice_gives_one_row_from_the_first_sheet_that_has_it() {
    let before = [
        sheet("wppsi_iv", &[("vci", 90.0)]),
        sheet("wisc_v", &[("vci", 70.0)]),
    ];
    let after = [sheet("wisc_v", &[("vci", 104.0), ("vci", 50.0)])];
    let rows = compare(&before, &after);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0].before, rows[0].after), (90.0, 104.0));
    assert_eq!(rows[0].before_instrument, "WPPSI-IV");
}

#[test]
fn the_text_shows_the_sign_of_every_change_and_warns_when_tests_differ() {
    let same = compare(
        &[sheet(
            "wisc_v",
            &[("vci", 100.0), ("psi", 95.0), ("wmi", 90.0)],
        )],
        &[sheet(
            "wisc_v",
            &[("vci", 100.0), ("psi", 85.0), ("wmi", 101.0)],
        )],
    );
    let text = comparison_text(&same);
    assert!(text.contains("(שינוי 0)"), "{text}");
    assert!(text.contains("(שינוי -10, שינוי ששווה לבדוק)"), "{text}");
    assert!(text.contains("(שינוי +11, שינוי ששווה לבדוק)"), "{text}");
    assert!(!text.contains("ההשוואה זהירה"), "{text}");
    assert_eq!(text, comparison_text(&same));
    // No rows: only the heading and the measurement-error sentence.
    let empty = comparison_text(&[]);
    assert!(empty.starts_with("השוואה לאבחון הקודם") && !empty.contains('←'));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// The rows are a pure function of the two sheets, and every figure in them follows from
    /// the numbers entered and the fixed tables.
    #[test]
    fn comparison_is_deterministic_and_follows_the_tables(
        b in prop::collection::vec(40u8..=160, 3),
        a in prop::collection::vec(40u8..=160, 3),
        bs in 1u8..=19,
        as_ in 1u8..=19,
    ) {
        let keys = ["fsiq", "vci", "psi"];
        let mk = |inst: &str, v: &[u8], sub: u8| {
            let mut e: Vec<(&str, f64)> = keys.iter().zip(v).map(|(k, x)| (*k, f64::from(*x))).collect();
            e.push(("similarities", f64::from(sub)));
            sheet(inst, &e)
        };
        let before = [mk("wppsi_iv", &b, bs)];
        let after = [mk("wisc_v", &a, as_)];
        let rows = compare(&before, &after);
        prop_assert_eq!(&rows, &compare(&before, &after));
        prop_assert_eq!(rows.len(), 4);
        for r in &rows {
            prop_assert_eq!(r.change, r.after - r.before);
            let key = match r.abbr.as_str() {
                "FSIQ" => "fsiq", "VCI" => "vci", "PSI" => "psi", "SI" => "similarities",
                other => panic!("unexpected row {other}"),
            };
            prop_assert_eq!(&r.before_band, &label("wppsi_iv", key, r.before));
            prop_assert_eq!(&r.after_band, &label("wisc_v", key, r.after));
            let scale = if key == "similarities" { ScoreScale::Scaled } else { ScoreScale::Standard };
            prop_assert_eq!(r.notable, notable(scale, r.change));
        }
    }

    /// Comparing the other way round negates every change and swaps the ranges.
    #[test]
    fn swapping_the_assessments_negates_the_change(
        b in prop::collection::vec(40u8..=160, 2),
        a in prop::collection::vec(40u8..=160, 2),
    ) {
        let mk = |v: &[u8]| sheet("wisc_v", &[("vci", f64::from(v[0])), ("wmi", f64::from(v[1]))]);
        let forward = compare(&[mk(&b)], &[mk(&a)]);
        let back = compare(&[mk(&a)], &[mk(&b)]);
        prop_assert_eq!(forward.len(), back.len());
        for (f, r) in forward.iter().zip(&back) {
            prop_assert_eq!(&f.abbr, &r.abbr);
            prop_assert_eq!(f.change, -r.change);
            prop_assert_eq!(&f.before_band, &r.after_band);
            prop_assert_eq!(f.notable, r.notable);
        }
    }
}

// ---------------------------------------------------------------- passages

const WORDS: &[&str] = &[
    "הילד",
    "משחק",
    "בגן",
    "עם",
    "חברים",
    "בחצר",
    "ציון",
    "102",
    "WISC-V",
    "5:4",
    "(בקושי)",
    "\"די\"",
    "אמר",
    "שהוא",
    "לא",
    "יודע",
    "המלצות",
    "-",
    "רקע",
];
const ENDS: &[&str] = &["", "", ".", ":", "...", "?", "!", ";", ".)", "׃", ","];

/// A line: blank, white space only, or words with an optional ending, maybe indented.
fn line() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => Just(String::new()),
        1 => prop::sample::select(vec![" ", "\t", "  \t "]).prop_map(str::to_owned),
        6 => (
            prop::sample::select(vec!["", " ", "  ", "\t"]),
            prop::collection::vec(prop::sample::select(WORDS.to_vec()), 1..8),
            prop::sample::select(ENDS.to_vec()),
            prop::sample::select(vec!["", " ", "\t"]),
        )
            .prop_map(|(lead, w, end, trail)| format!("{lead}{}{end}{trail}", w.join(" "))),
    ]
}

fn text() -> impl Strategy<Value = String> {
    (
        prop::collection::vec(line(), 0..40),
        prop::bool::ANY,
        prop::bool::ANY,
    )
        .prop_map(|(lines, crlf, final_newline)| {
            let mut t = lines.join(if crlf { "\r\n" } else { "\n" });
            if final_newline {
                t.push('\n');
            }
            t
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Ranges are in order, inside the text, on character boundaries, trimmed, never longer
    /// than twelve lines, and together they hold every visible character: cutting a material
    /// into passages never drops a word that the sections could need.
    #[test]
    fn passages_cover_every_visible_character_in_order(t in text()) {
        let ranges = passage_ranges(&t);
        prop_assert_eq!(&ranges, &passage_ranges(&t));
        let mut last_end = 0;
        for r in &ranges {
            prop_assert!(r.start < r.end && r.end <= t.len(), "{r:?} in {}", t.len());
            prop_assert!(r.start >= last_end, "overlap at {r:?}");
            let s = t.get(r.clone());
            prop_assert!(s.is_some(), "{r:?} is not on a character boundary");
            let s = s.unwrap();
            prop_assert_eq!(s, s.trim());
            prop_assert!(s.lines().count() <= 12, "{s:?}");
            last_end = r.end;
        }
        for (i, c) in t.char_indices() {
            if !c.is_whitespace() {
                prop_assert!(
                    ranges.iter().any(|r| r.contains(&i)),
                    "{c:?} at {i} is in no passage of {t:?}"
                );
            }
        }
        prop_assert_eq!(passages(&t).len(), ranges.len());
    }

    /// Hiding a name behind a tag (here: any word of letters only) never moves a passage
    /// boundary, so passage numbers chosen on the original still fit the filtered text.
    #[test]
    fn tags_in_place_of_names_never_move_a_boundary(t in text(), pick in 0usize..1000) {
        let candidates: Vec<&str> = t
            .split_whitespace()
            .filter(|w| w.chars().all(|c| ('\u{05d0}'..='\u{05ea}').contains(&c)))
            .collect();
        prop_assume!(!candidates.is_empty());
        let name = candidates[pick % candidates.len()];
        let tagged: String = t
            .split_inclusive(char::is_whitespace)
            .map(|piece| {
                let word = piece.trim_end_matches(char::is_whitespace);
                if word == name {
                    format!("[ילד_גן_2]{}", &piece[word.len()..])
                } else {
                    piece.to_owned()
                }
            })
            .collect();
        let a = passages(&t);
        let b = passages(&tagged);
        prop_assert_eq!(a.len(), b.len(), "{:?} vs {:?}", a, b);
        for (x, y) in a.iter().zip(&b) {
            prop_assert_eq!(x.lines().count(), y.lines().count());
        }
    }
}

// ---------------------------------------------------------------- routing

const SECTIONS: &[&str] = &[
    "background",
    "kindergarten",
    "cognitive",
    "communication",
    "emotional",
    "recommendations",
];

fn sections() -> Vec<String> {
    SECTIONS.iter().map(|s| (*s).to_owned()).collect()
}

fn subset() -> impl Strategy<Value = Vec<String>> {
    prop::sample::subsequence(SECTIONS.to_vec(), 0..=SECTIONS.len())
        .prop_map(|v| v.into_iter().map(str::to_owned).collect())
}

/// A routing as stored: maybe a suggestion (possibly stale, with any passage numbers, even
/// out of range or repeated), and maybe her additions and removals.
fn routing(count: u32) -> impl Strategy<Value = Routing> {
    (
        prop::option::of((
            prop::bool::ANY,
            prop::sample::select(vec![count, count + 1]),
            prop::collection::vec(
                (
                    prop::sample::select(SECTIONS.to_vec()),
                    prop::collection::vec(0u32..count + 3, 0..5),
                ),
                0..6,
            ),
        )),
        subset(),
        subset(),
    )
        .prop_map(|(sugg, added, removed)| Routing {
            suggestion: sugg.map(|(by_ai, passage_count, secs)| Suggestion {
                by_ai,
                passage_count,
                sections: secs
                    .into_iter()
                    .map(|(s, p)| SectionPassages {
                        section: s.to_owned(),
                        passages: p,
                    })
                    .collect(),
            }),
            added,
            removed,
            unplaced: None,
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// What a section gets is well formed whatever is stored: a removed section gets nothing,
    /// an added one gets the whole material, and chosen passages are real, sorted and unique.
    #[test]
    fn feed_is_well_formed_and_removal_always_wins(
        (count, r) in (1u32..8).prop_flat_map(|c| (Just(c), routing(c))),
        table in subset(),
    ) {
        let n = count as usize;
        for s in SECTIONS {
            let f = r.feed(s, &table, n);
            prop_assert_eq!(&f, &r.feed(s, &table, n));
            if r.removed.iter().any(|x| x == s) {
                prop_assert_eq!(f, None);
            } else if r.added.iter().any(|x| x == s) {
                prop_assert_eq!(f, Some(Feed::Whole));
            } else if let Some(Feed::Passages(p)) = f {
                prop_assert!(!p.is_empty());
                prop_assert!(p.windows(2).all(|w| w[0] < w[1]), "{p:?}");
                prop_assert!(p.iter().all(|x| (1..=count).contains(x)), "{p:?}");
            }
        }
        let order = sections();
        let fed = r.feeds(&order, &table, n);
        let expected: Vec<String> = order
            .iter()
            .filter(|s| r.feed(s, &table, n).is_some())
            .cloned()
            .collect();
        prop_assert_eq!(fed, expected);
    }

    /// Einat always wins: after she picks sections, exactly those are fed, in report order,
    /// whatever the table or a (valid) sorting said.
    #[test]
    fn her_choice_is_exactly_what_is_fed(
        count in 1u32..8,
        chosen in subset(),
        table in subset(),
        sugg in prop::option::of(prop::collection::vec(
            (prop::sample::select(SECTIONS.to_vec()), prop::collection::vec(1u32..8, 0..4)),
            0..6,
        )),
        stale in prop::bool::ANY,
    ) {
        let n = count as usize;
        let mut r = Routing {
            suggestion: sugg.map(|secs| Suggestion {
                by_ai: true,
                passage_count: if stale { count + 1 } else { count },
                sections: secs
                    .into_iter()
                    .map(|(s, p)| SectionPassages {
                        section: s.to_owned(),
                        // As parsed from a reply: only passages that exist.
                        passages: p.into_iter().filter(|x| *x <= count).collect(),
                    })
                    .collect(),
            }),
            ..Routing::default()
        };
        r.choose(&chosen, &table, n);
        let order = sections();
        let want: Vec<String> = order.iter().filter(|s| chosen.contains(s)).cloned().collect();
        prop_assert_eq!(r.feeds(&order, &table, n), want);
        // A later edit of the text drops the sorting, and her additions still hold.
        r.text_changed();
        for s in &r.added {
            prop_assert_eq!(r.feed(s, &table, n + 1), Some(Feed::Whole));
        }
        for s in &r.removed {
            prop_assert_eq!(r.feed(s, &table, n + 1), None);
        }
    }
}
