//! Speed checkers: the main actions on a vault of a working size, each against a time budget.
//!
//! The budgets are about ten times what a debug build takes on a four-core machine, so a slow
//! CI runner passes, and an action that went back to reading every case one by one (the case
//! list was 58 ms and opening a case 36 ms before the reads were batched; now 6 and 3) or
//! became several times slower does not. The printed table is the real
//! report: `cargo xtask speed` runs this in a release build and shows it. Nothing here calls
//! a network; every name and text is invented.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

use std::time::{Duration, Instant};

use dv_core::Core;
use dv_domain::{CaseMeta, IdentityInput, InputKind, Role};

const PASSWORD: &str = "סוס ירוק רץ בשדה 2026";
/// A few years of work: more cases than she has today.
const CASES: usize = 300;

fn person(role: Role, value: String) -> IdentityInput {
    IdentityInput {
        id: None,
        role,
        value,
        aliases: Vec::new(),
    }
}

/// A vault with `CASES` invented cases, three people in each, a material and an approved
/// paragraph in every tenth.
fn working_vault(dir: &std::path::Path) -> (Core, Vec<String>) {
    let mut core = Core::for_tests(dir, None);
    core.create_vault(PASSWORD).unwrap();
    let mut ids = Vec::with_capacity(CASES);
    for i in 0..CASES {
        let meta = CaseMeta {
            code: format!("TEST-{i:04}"),
            ..CaseMeta::default()
        };
        let people = vec![
            person(Role::Child, format!("ילדבדוי{i} משפחהבדויה{i}")),
            person(Role::Mother, format!("אמאבדויה{i}")),
            person(Role::Teacher, format!("גננתבדויה{i}")),
        ];
        let id = core.create_case(meta, people).unwrap();
        if i % 10 == 0 {
            core.add_input(
                &id,
                InputKind::Intake,
                "אינטייק בדוי",
                &format!("אמאבדויה{i} סיפרה שהילד משחק יפה בגן, ושהוא אוהב לבנות בקוביות. ")
                    .repeat(20),
            )
            .unwrap();
        }
        ids.push(id);
    }
    (core, ids)
}

/// The fastest of `runs`: what the action costs, without the noise of a busy machine.
fn best_of(runs: usize, mut action: impl FnMut()) -> Duration {
    (0..runs)
        .map(|_| {
            let start = Instant::now();
            action();
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

#[test]
fn the_main_actions_stay_inside_their_time_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let built = Instant::now();
    let (mut core, ids) = working_vault(dir.path());
    let build = built.elapsed();
    let with_material = ids[0].clone();
    let plain = ids[CASES - 1].clone();

    // (action, measured, budget)
    let mut rows: Vec<(&str, Duration, Duration)> = Vec::new();
    rows.push((
        "case list (300 cases)",
        best_of(3, || {
            assert_eq!(core.list_cases().unwrap().len(), CASES);
        }),
        Duration::from_millis(100),
    ));
    rows.push((
        "recycle bin (empty)",
        best_of(3, || {
            assert!(core.list_trash().unwrap().is_empty());
        }),
        Duration::from_millis(50),
    ));
    rows.push((
        "open a case",
        best_of(3, || {
            core.case_detail(&with_material).unwrap();
        }),
        Duration::from_millis(60),
    ));
    let mut n = 0;
    rows.push((
        "save a material (filtered against every case's names)",
        best_of(3, || {
            n += 1;
            core.add_input(
                &plain,
                InputKind::Intake,
                &format!("רישום בדוי {n}"),
                "אמאבדויה299 סיפרה שהילד נרדם מאוחר ומתעורר מוקדם.",
            )
            .unwrap();
        }),
        Duration::from_millis(400),
    ));
    core.lock();
    rows.push((
        "unlock (test key cost) to the case list",
        best_of(2, || {
            core.unlock(PASSWORD).unwrap();
            core.list_cases().unwrap();
            core.lock();
        }),
        Duration::from_millis(400),
    ));

    println!(
        "\nspeed report: {CASES} cases (built in {} ms)",
        build.as_millis()
    );
    println!("{:<58} {:>10} {:>10}", "action", "ms", "budget ms");
    for (name, took, budget) in &rows {
        println!(
            "{name:<58} {:>10.1} {:>10}",
            took.as_secs_f64() * 1000.0,
            budget.as_millis()
        );
    }
    for (name, took, budget) in &rows {
        assert!(
            took <= budget,
            "{name}: {took:?} is over its budget {budget:?}"
        );
    }
}
