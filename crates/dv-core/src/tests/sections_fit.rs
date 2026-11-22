//! The second pass on the clinician's list (D-214, D-215): sub-sections with headings written
//! in code, her order of the summary, and the list of tools built from the case. Fabricated
//! data only; nothing leaves the machine.

use super::*;
use dv_domain::{ScoreEntry, ScoreSheet};

fn section(core: &mut Core, case: &str, key: &str) -> SectionView {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == key)
        .unwrap()
}

fn sheet(instrument: &str, module: &str, entries: &[(&str, f64)]) -> ScoreSheet {
    ScoreSheet {
        instrument: instrument.into(),
        module: module.into(),
        cutoff: None,
        entries: entries
            .iter()
            .map(|(m, value)| ScoreEntry {
                measure: (*m).into(),
                value: *value,
                note: String::new(),
            })
            .collect(),
        notes: String::new(),
        basis: None,
    }
}

#[test]
fn a_subsection_gets_its_heading_in_code_and_a_missing_one_is_named() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.save_scores(
        &case,
        None,
        &sheet("ados_2", "2", &[("total", 12.0), ("comparison", 6.0)]),
    )
    .unwrap();
    let shown = section(&mut core, &case, "communication");
    assert_eq!(
        shown.subheadings,
        [
            "אבחון ADOS",
            "שפה ותקשורת",
            "אינטראקציה חברתית הדדית",
            "משחק והתנהגויות סטריאוטיפיות",
            "SRS"
        ]
    );
    let p = core
        .prepare_section(&case, "communication", "תנסח את הסעיף")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    // The model wrote one heading itself, left one out, and answered one line twice.
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [
            { "text": "הועבר ADOS-2 במודול 2.", "source_refs": ["S1"], "guide": 1 },
            { "text": "שפה ותקשורת: [ילד] דיבר במשפטים קצרים.", "source_refs": ["S1"], "guide": 2 },
            { "text": "[ילד] יזם קשר עם המאבחנת.", "source_refs": ["S1"], "guide": 3 },
            { "text": "[ילד] הגיב לחיוך.", "source_refs": ["S1"], "guide": 3 },
        ],
        "questions": [], "missing": [], "contradictions": [],
    })));
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(
        sent[0].contains("2. כותרת משנה: שפה ותקשורת."),
        "{}",
        sent[0]
    );
    assert!(sent[0].contains("הכותרת מתווספת במחשב"));
    let texts: Vec<String> = section(&mut core, &case, "communication")
        .paragraphs
        .into_iter()
        .map(|p| p.text)
        .collect();
    assert_eq!(
        texts,
        [
            "אבחון ADOS\nהועבר ADOS-2 במודול 2.",
            "שפה ותקשורת\nאלון דיבר במשפטים קצרים.",
            "אינטראקציה חברתית הדדית\nאלון יזם קשר עם המאבחנת.",
            "אלון הגיב לחיוך.",
        ]
    );
    assert!(
        r.missing
            .iter()
            .any(|m| m == "חסרה פסקה לפי ההנחיה: תת הסעיף \"SRS\""),
        "{:?}",
        r.missing
    );
    // The file prints a heading as a heading: the export is told which lines they are.
    let her_order = section(&mut core, &case, "summary");
    assert!(
        her_order.guide.starts_with("סיבת ההפניה."),
        "{}",
        her_order.guide
    );
    assert_eq!(her_order.guide_learned_from, 0);
}

#[test]
fn the_tools_section_lists_the_tools_of_the_case_and_never_touches_what_is_hers() {
    let (_dir, mut core, case) = setup(None);
    let before = section(&mut core, &case, "tools").paragraphs.len();
    assert_eq!(before, 0);
    core.save_scores(&case, None, &sheet("wppsi_iv", "", &[("fsiq", 98.0)]))
        .unwrap();
    core.save_scores(&case, None, &sheet("ados_2", "2", &[("total", 12.0)]))
        .unwrap();
    core.save_scores(&case, None, &sheet("srs_2", "", &[("total", 68.0)]))
        .unwrap();
    let list = section(&mut core, &case, "tools").paragraphs;
    assert_eq!(list.len(), 1, "{list:?}");
    let text = &list[0].text;
    assert!(
        text.starts_with("במסגרת האבחון נעשה שימוש בכלים אלו:"),
        "{text}"
    );
    assert!(
        text.ends_with("WPPSI-IV, ADOS-2 (מודול 2), SRS-2."),
        "{text}"
    );
    assert_eq!(list[0].status, DraftStatus::Proposed);
    // Names only: no score and no range.
    assert!(!text.contains("98") && !text.contains("68"), "{text}");

    // Once she approved it, a new sheet changes nothing there.
    core.approve_paragraph(&case, &list[0].id).unwrap();
    core.save_scores(&case, None, &sheet("abas_3", "", &[("gac", 85.0)]))
        .unwrap();
    let after = section(&mut core, &case, "tools").paragraphs;
    assert_eq!(after.len(), 1);
    assert_eq!(&after[0].text, text);
}
