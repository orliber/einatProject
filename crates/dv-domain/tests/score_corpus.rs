//! Round 32: how well the scores of old reports are found. Twelve fabricated reports
//! (`tests/fixtures/score_reports`) mark every score `⟦7⟧`; the finder must find at least 95%
//! of them, at the right place and with the right value, and nothing else.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::string_slice
)]

use std::path::{Path, PathBuf};

use dv_domain::find_score_mentions;

fn corpus() -> Vec<(PathBuf, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/score_reports");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let t = std::fs::read_to_string(&p).unwrap();
            (p, t)
        })
        .collect()
}

/// The text without the marks, and each marked score: where it starts, its value, and the
/// measure when the mark names it (`⟦13:matrix⟧`).
pub fn unmark(marked: &str) -> (String, Vec<(usize, f64, Option<String>)>) {
    let mut text = String::with_capacity(marked.len());
    let mut scores = Vec::new();
    let mut rest = marked;
    while let Some(open) = rest.find('⟦') {
        text.push_str(&rest[..open]);
        let after = &rest[open + '⟦'.len_utf8()..];
        let close = after.find('⟧').expect("an open mark is closed");
        let (value, measure) = match after[..close].split_once(':') {
            Some((v, m)) => (v, Some(m.to_owned())),
            None => (&after[..close], None),
        };
        let v = value.parse::<f64>().expect("a mark holds a number");
        scores.push((text.len(), v, measure));
        text.push_str(value);
        rest = &after[close + '⟧'.len_utf8()..];
    }
    text.push_str(rest);
    (text, scores)
}

#[test]
fn the_scores_of_old_reports_are_found_and_nothing_else() {
    let reports = corpus();
    assert_eq!(reports.len(), 12);
    let (mut expected, mut hit) = (0usize, 0usize);
    let mut wrong = Vec::new();
    let mut missed = Vec::new();
    for (path, marked) in &reports {
        let (text, scores) = unmark(marked);
        let found = find_score_mentions(&text);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        expected += scores.len();
        for (at, v, measure) in &scores {
            if found.iter().any(|m| {
                m.start == *at
                    && (m.value - v).abs() < 1e-9
                    && measure.as_ref().is_none_or(|k| *k == m.measure)
            }) {
                hit += 1;
            } else {
                missed.push(format!("{name}: {v}"));
            }
        }
        for m in &found {
            if !scores.iter().any(|(at, _, _)| *at == m.start) {
                wrong.push(format!("{name}: {} ({})", m.value, m.measure));
            }
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let recall = hit as f64 / expected as f64;
    assert!(
        recall >= 0.95,
        "found {hit} of {expected}; missed {missed:?}"
    );
    assert!(wrong.is_empty(), "taken for a score: {wrong:?}");
}

/// The review of stage 4: a date beside a subtest is not a score ("ב 28.1.2019" is not 28.1).
#[test]
fn a_date_beside_a_subtest_is_not_a_score() {
    for text in [
        "בקליטה חזותית ב 28.1.2019 הושג ציון 45.",
        "שאלון CBCL (מילוי 27.6.2018) הראה ציון T של 62.",
        "בקליטה חזותית, בבדיקה מ-3/5/2019, ציון 45.",
    ] {
        let found: Vec<f64> = find_score_mentions(text).iter().map(|m| m.value).collect();
        assert!(
            found.iter().all(|v| (*v - v.round()).abs() < 1e-9
                && ![28.0, 27.0, 2019.0, 2018.0, 3.0, 5.0].contains(v)),
            "{text}: {found:?}"
        );
    }
    let found: Vec<f64> = find_score_mentions("בקליטה חזותית ב 28.1.2019 הושג ציון 45.")
        .iter()
        .map(|m| m.value)
        .collect();
    assert_eq!(found, [45.0]);
}
