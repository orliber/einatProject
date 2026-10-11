//! The filter measured on whole documents (tests/corpus/README.md).
//!
//! Each corpus file is an invented document with every identifying span annotated, and a
//! few ordinary words that must pass marked `keep`. The test strips the annotations, runs
//! the filter as the case would, and prints per split (dev / test) and per group:
//! recall, ordinary words hidden by mistake (per 1,000 words) and the items the summary
//! card lists per case. Names the filter finds are kept with the case, as dv-core keeps
//! them, so each document is filtered twice and later documents of a case know them.
//! It fails when a number gets worse than the floor recorded below, so every change to the
//! filter shows up as a number. `test/` is sealed: its misses are listed only with
//! `CORPUS_SHOW_TEST=1`, for the final review, never for tuning.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::panic,
    clippy::expect_used
)]

#[path = "support/corpus.rs"]
mod harness;

use std::path::PathBuf;

use harness::{check, load, measure, print, Floor};

// Raised with stage 2 of the filter (D-042, 2026-10-03). Measured on v0.2.0, before it:
// dev name .727 place .604 inst .308, 11.1 false hides per 1,000 words, 20.7 questions per
// case; test name .702 place .644 inst .227, 11.3 per 1,000, 27.2 per case.
// Stage 3 (2026-10-04): no questions; names found are kept with the case. Adjacent hides
// count as one (stage 2 on that count: dev 2.6, test 4.9 per 1,000). Test gains 5 names
// for 4 more false hides (5.3), from names carried to the case's later documents.
const DEV_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 1.0),
        ("name", 0.85),
        ("surname", 1.0),
        ("place", 0.88),
        ("inst", 0.82),
        ("num", 1.0),
        ("date", 1.0),
    ],
    max_false_per_1000: 2.5,
};

const TEST_FLOOR: Floor = Floor {
    min_recall: &[
        ("decl", 0.99),
        ("name", 0.86),
        ("surname", 0.96),
        ("place", 0.73),
        ("inst", 0.68),
        ("num", 1.0),
        ("date", 1.0),
    ],
    max_false_per_1000: 5.3,
};

#[test]
fn corpus_measures_the_filter_on_whole_documents() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let dev = load(&corpus, "dev");
    let test = load(&corpus, "test");
    let show_test = std::env::var_os("CORPUS_SHOW_TEST").is_some();
    let rd = measure(&dev, &|_| Vec::new());
    let rt = measure(&test, &|_| Vec::new());
    print("dev", &rd, true);
    print("test", &rt, show_test);
    let mut problems = check("dev", &rd, &DEV_FLOOR);
    problems.extend(check("test", &rt, &TEST_FLOOR));
    assert!(problems.is_empty(), "{problems:#?}");
}
