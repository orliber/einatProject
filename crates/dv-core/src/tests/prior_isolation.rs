//! D-156: a previous report feeds only "אבחונים וטיפולים קודמים". The current assessment is
//! written clean of it: no section of it, and nothing written from the approved sections,
//! gets its text, whatever the table, the sorting or her own choice says.

use super::*;

/// A sentence that exists only in the previous report (fabricated).
const CANARY: &str = "נמצא פער חריג של שלושים נקודות בין המדדים";
/// A sentence that exists only in her approved section about previous assessments.
const PRIOR_SECTION_CANARY: &str = "באבחון קודם תואר קושי בוויסות";

fn add_prior_report(core: &mut Core, case: &str) -> String {
    core.add_input(
        case,
        InputKind::PriorReport,
        "דוח קודם",
        &format!(
            "סיכום אבחון פסיכו-דידקטי שנערך בקהילה.\n\n{CANARY}.\n\nבתקשורת נצפה קשר עין מועט ומשחק משותף קצר."
        ),
    )
    .unwrap()
    .id
}

fn sent_for(core: &mut Core, fake: &FakeTransport, case: &str, key: &str) -> String {
    *fake.answer.lock().unwrap() = None;
    let before = fake.sent.lock().unwrap().len();
    let prepared = core.prepare_section(case, key, "טיוטה").unwrap();
    let approval = prepared.approval_id.expect("clears the gate");
    // The answer does not matter here; what left the machine does.
    let _ = core.send_section(&approval);
    let sent = fake.sent.lock().unwrap();
    assert_eq!(sent.len(), before + 1, "one request for {key}");
    sent.last().unwrap().clone()
}

#[test]
fn a_previous_report_never_reaches_the_current_assessment() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let prior = add_prior_report(&mut core, &case);

    // Claude's sorting places the report's passages in the current assessment, and none in
    // the section about previous assessments.
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"sections": [
        {"section": "cognitive", "passages": ["S3P2"]},
        {"section": "communication", "passages": ["S3P3"]},
        {"section": "background", "passages": ["S3P1"]}
    ]})));
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();

    // The materials screen shows it feeding only its own section.
    let detail = core.case_detail(&case).unwrap();
    let r = detail.routing.iter().find(|r| r.input_id == prior).unwrap();
    assert_eq!(r.feeds, [crate::sorting::PRIOR_SECTION]);
    assert!(r.table.iter().all(|s| s == crate::sorting::PRIOR_SECTION));

    // Her own choice cannot add it to another section either.
    let err = core
        .set_input_sections(
            &case,
            &prior,
            &["prior_assessments".to_owned(), "cognitive".to_owned()],
        )
        .unwrap_err();
    assert!(matches!(err, CoreError::Refused(_)), "{err:?}");

    for key in ["cognitive", "communication", "background"] {
        let sent = sent_for(&mut core, &fake, &case, key);
        assert!(!sent.contains(CANARY), "the previous report reached {key}");
        assert!(
            !sent.contains("קשר עין מועט"),
            "the previous report reached {key}"
        );
    }
    // Its own section still gets it, though the sorting placed none of it there.
    let sent = sent_for(&mut core, &fake, &case, "prior_assessments");
    assert!(
        sent.contains(CANARY),
        "the previous report left its own section"
    );
}

#[test]
fn what_is_written_from_the_approved_sections_leaves_the_previous_ones_out() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(
        &case,
        "prior_assessments",
        &format!("{PRIOR_SECTION_CANARY}."),
    )
    .unwrap();
    core.add_own_paragraph(&case, "kindergarten", "בגן אלון משתתף במפגש הבוקר.")
        .unwrap();

    for key in ["summary", "diagnoses", "recommendations", "dsm"] {
        let sent = sent_for(&mut core, &fake, &case, key);
        assert!(
            sent.contains("מפגש הבוקר"),
            "{key} is written from the approved sections"
        );
        assert!(
            !sent.contains(PRIOR_SECTION_CANARY),
            "the previous assessments reached {key}"
        );
    }
}

#[test]
fn a_consultation_about_the_case_leaves_the_previous_assessments_out() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(
        &case,
        "prior_assessments",
        &format!("{PRIOR_SECTION_CANARY}."),
    )
    .unwrap();
    core.add_own_paragraph(&case, "kindergarten", "בגן אלון משתתף במפגש הבוקר.")
        .unwrap();
    let ask = core
        .prepare_consult(Some(&case), None, "איך לנסח את הסיכום?")
        .unwrap()
        .approval_id
        .unwrap();
    let _ = core.send_consult(&ask);
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(sent.contains("מפגש הבוקר"));
    assert!(!sent.contains(PRIOR_SECTION_CANARY));
}
