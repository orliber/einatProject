//! R5: a large case (50 materials, 200 paragraphs, invented text) stays quick to open, and
//! what crosses to the screen stays a size the window handles. The budgets are for a debug
//! build on a shared machine; the release build she runs is several times faster.

use std::time::{Duration, Instant};

use super::*;

const MATERIALS: usize = 50;
const PARAGRAPHS: usize = 200;
/// Opening the case (`case_detail`), debug build.
const OPEN_BUDGET: Duration = Duration::from_secs(2);
/// What `case_detail` sends to the screen, as JSON.
const DETAIL_BUDGET_BYTES: usize = 1024 * 1024;

fn material(i: usize) -> String {
    let lines = [
        "בתצפית בגן נראה כי הילד משחק בעיקר לבד בפינת הבנייה.",
        "הגננת מתארת קושי במעברים בין פעילויות ובפרידה בבוקר.",
        "בשיחה עם ההורים עלה כי השינה השתפרה בחודשים האחרונים.",
        "במשחק סמלי הוא בונה סיפור קצר עם התחלה וסוף.",
        "בפעילות מוטורית עדינה נצפה אחיזת עיפרון לא בשלה לגילו.",
    ];
    (0..24)
        .map(|j| lines[(i + j) % lines.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_large_case_opens_within_budget_and_its_detail_stays_small() {
    let (_d, mut core, case) = setup(None);
    for i in 0..MATERIALS {
        core.add_input(
            &case,
            InputKind::Observation,
            &format!("תצפית {i}"),
            &material(i),
        )
        .unwrap();
    }
    let structure = ReportStructure::load_default().unwrap();
    let keys: Vec<String> = structure.sections().map(|s| s.key.clone()).collect();
    for i in 0..PARAGRAPHS {
        let text = format!(
            "פסקה {i}: {}",
            material(i).chars().take(400).collect::<String>()
        );
        core.add_own_paragraph(&case, &keys[i % keys.len()], &text)
            .unwrap();
    }

    let started = Instant::now();
    let detail = core.case_detail(&case).unwrap();
    let open = started.elapsed();
    let bytes = serde_json::to_vec(&detail).unwrap().len();
    let started = Instant::now();
    let cases = core.list_cases().unwrap();
    let list = started.elapsed();
    let _ = std::io::Write::write_fmt(
        &mut std::io::stderr(),
        format_args!(
            "R5: case_detail {open:?}, {} KB; list_cases {list:?}\n",
            bytes / 1024
        ),
    );
    assert_eq!(cases.len(), 1);
    assert_eq!(detail.inputs.len(), MATERIALS + 2);
    assert!(open < OPEN_BUDGET, "opening took {open:?}");
    assert!(bytes < DETAIL_BUDGET_BYTES, "{bytes} bytes to the screen");
}
