//! What the institute's report asks for (round 43): the DSM-5 severity she sets goes out with
//! the DSM section in fixed words, and a paragraph with another level gets a note. Fabricated
//! data only; nothing leaves the machine.

use super::*;

#[test]
fn her_dsm_severity_goes_out_as_she_set_it_and_another_level_gets_a_note() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // A level outside 1–3 is refused; nothing set, nothing goes out.
    assert!(core.set_dsm_severity(&case, Some(4), None).is_err());
    core.set_dsm_severity(&case, Some(2), Some(1)).unwrap();
    assert_eq!(
        core.case_detail(&case).unwrap().meta.dsm_severity,
        Some(dv_domain::DsmSeverity::new(Some(2), Some(1)))
    );
    // Editing the case details keeps it.
    let meta = core.case_detail(&case).unwrap().meta;
    core.update_case(
        &case,
        CaseMeta {
            dsm_severity: None,
            ..meta
        },
    )
    .unwrap();
    assert!(core.case_detail(&case).unwrap().meta.dsm_severity.is_some());

    // The DSM section is written from what she approved.
    core.add_own_paragraph(&case, "kindergarten", "אלון מגיב בעוצמה למעברים.")
        .unwrap();
    let p = core.prepare_section(&case, "dsm", "תנסח את הסעיף").unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{
            "text": "[ילד] עומד בקריטריונים, ברמה 3 בתחום התקשורת החברתית.",
            "source_refs": [],
        }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(
        sent[0].contains("תקשורת חברתית ואינטראקציה חברתית: דרגה 2"),
        "{}",
        sent[0]
    );
    assert!(sent[0].contains("מוגבלים וחוזרים: דרגה 1"));
    let dsm = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "dsm")
        .unwrap();
    assert_eq!(
        dsm.paragraphs[0].warnings,
        vec!["בפסקה כתובה דרגה 3, ואת קבעת דרגה 2 ודרגה 1. כדאי לבדוק.".to_owned()]
    );

    // Cleared: the next request carries no level.
    core.set_dsm_severity(&case, None, None).unwrap();
    assert!(core.case_detail(&case).unwrap().meta.dsm_severity.is_none());
}

#[test]
fn the_supervisors_lines_are_printed_under_her_signature() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "referral", "ההורים של אלון פנו בשל קושי במעברים.")
        .unwrap();
    let mut settings = core.report_settings().unwrap();
    assert!(settings.supervisor.is_empty());
    settings.signature = vec!["רותם אלמוג, מתמחה בפסיכולוגיה התפתחותית".into()];
    settings.supervisor = vec![
        "בהדרכת: ד\"ר נועה בדויה".into(),
        "  ".into(),
        "פסיכולוגית התפתחותית מדריכה".into(),
    ];
    core.set_report_settings(&settings).unwrap();
    assert_eq!(core.report_settings().unwrap(), settings);

    let doc = read_docx_text(&core.export_report(&case, None).unwrap());
    let her = doc.find("מתמחה בפסיכולוגיה").unwrap();
    let supervisor = doc.find("נועה בדויה").unwrap();
    assert!(her < supervisor, "{doc}");
    assert!(doc.contains("פסיכולוגית התפתחותית מדריכה"));

    // Settings saved before round 43 have no supervisor, and still load.
    let old = r#"{"title":"דוח","font":"David","confidentiality":"חסוי","signature":["א"]}"#;
    let s: ReportSettings = serde_json::from_str(old).unwrap();
    assert!(s.supervisor.is_empty());
}

#[test]
fn the_letter_is_written_from_the_recommendations_she_marked() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(&case, "recommendations", "המלצה על ריפוי בעיסוק פעם בשבוע.")
        .unwrap();
    core.add_own_paragraph(&case, "recommendations", "המלצה על הדרכת הורים.")
        .unwrap();
    let ids: Vec<String> = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "recommendations")
        .unwrap()
        .paragraphs
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids.len(), 2);
    // Only an approved recommendation of the case can be marked.
    assert!(core
        .set_letter_recommendations(&case, Some(vec!["not-a-paragraph".into()]))
        .is_err());
    core.set_letter_recommendations(&case, Some(vec![ids[1].clone()]))
        .unwrap();

    let p = core.prepare_letter(&case, "school", "").unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{ "text": "מומלצת הדרכת הורים.", "source_refs": [] }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(sent[0].contains("הדרכת הורים"), "{}", sent[0]);
    assert!(!sent[0].contains("ריפוי בעיסוק"), "{}", sent[0]);

    // Back to all of them.
    core.set_letter_recommendations(&case, None).unwrap();
    let p = core.prepare_letter(&case, "school", "").unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{ "text": "מומלץ.", "source_refs": [] }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(sent.last().unwrap().contains("ריפוי בעיסוק"));
}
