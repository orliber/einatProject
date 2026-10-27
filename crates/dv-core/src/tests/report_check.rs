//! The check of the whole approved report (D-097).

use super::*;

/// Two approved body sections and an approved summary, each with words of its own.
fn approved_report(core: &mut Core, case: &str) {
    core.add_own_paragraph(
        case,
        "kindergarten",
        "אלון מתקשה במעברים בין פעילויות בגן, ונעזר בהכנה מראש.",
    )
    .unwrap();
    core.add_own_paragraph(case, "background", "ההריון והלידה עברו ללא סיבוכים.")
        .unwrap();
    core.add_own_paragraph(
        case,
        "summary",
        "אלון ילד סקרן, שמתקשה במעברים ונעזר בהכנה מראש.",
    )
    .unwrap();
}

#[test]
fn the_check_of_the_whole_report_is_off_until_she_turns_it_on() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    assert!(!core.report_check_on().unwrap());
    let setting = core.report_check_setting().unwrap();
    assert!(!setting.on);
    assert!(
        setting.cents_per_check.is_some_and(|c| c > 0),
        "the cost is known before it is turned on: {setting:?}"
    );
    assert!(matches!(
        core.prepare_report_check(&case),
        Err(CoreError::Needs(_, ErrorAction::Settings { .. }))
    ));
    assert!(fake.sent.lock().unwrap().is_empty(), "nothing left");
    core.set_report_check(true).unwrap();
    assert!(core.report_check_setting().unwrap().on);
    assert!(core.prepare_report_check(&case).is_ok());
    core.set_report_check(false).unwrap();
    assert!(core.prepare_report_check(&case).is_err());
}

#[test]
fn only_the_approved_sections_go_out_through_the_gate_and_the_send_is_recorded() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.set_report_check(true).unwrap();
    // A paragraph she rejected is not part of the report.
    let v = core.vault_mut().unwrap();
    let d = v
        .add_draft(
            &case,
            "cognitive",
            "[ילד] פתר את כל המטלות במהירות מסחררת.",
            Author::Ai,
            &[],
        )
        .unwrap();
    v.set_draft_status(&case, &d.id, DraftStatus::Rejected)
        .unwrap();

    let prepared = core.prepare_report_check(&case).unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    let id = prepared.approval_id.clone().expect("cleared by the gate");
    let result = core.send_report_check(&id).unwrap();
    assert!(!result.demo);
    assert_eq!(
        result.checked,
        vec![
            "background".to_owned(),
            "kindergarten".to_owned(),
            "summary".to_owned()
        ],
        "in the order of the report"
    );

    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "one request for the whole report");
    let body = &sent[0];
    // Approved text, tagged: no name, no material, nothing not approved.
    for name in ["אלון", "שירה", "רותם", "אלמוג"] {
        assert!(!body.contains(name), "{name} left the machine");
    }
    assert!(body.contains("[ילד] מתקשה במעברים") && body.contains("ללא סיבוכים"));
    assert!(!body.contains("נפרד בבכי"), "a material is not sent");
    assert!(!body.contains("נרדם באיחור"), "a material is not sent");
    assert!(
        !body.contains("מהירות מסחררת"),
        "a rejected paragraph is not sent"
    );
    assert!(body.contains("closing_section=\\\"R3\\\""), "{body}");
    assert!(body.contains("body_section=\\\"R1\\\""));
    // The schema holds nothing from the case.
    let parsed: Value = serde_json::from_str(body).unwrap();
    let schema = parsed["output_config"]["format"]["schema"].to_string();
    assert!(!schema.contains("ילד") && !schema.contains("R1"));

    // Recorded like every other send, with the exact bytes, in "what left the computer".
    let kept = core.vault_ref().unwrap().transmissions(&case).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].section_key, "report_check");
    assert_eq!(&kept[0].payload_tagged, body);
    let history = core
        .sent_history(Some(&case), None, "", None)
        .unwrap()
        .items;
    assert_eq!(history[0].place, "report_check");
    // The approval is one use only, and only for this kind of request.
    assert!(matches!(
        core.send_report_check(&id),
        Err(CoreError::NotFound(_))
    ));
}

#[test]
fn a_draft_still_waiting_means_nothing_is_sent() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.set_report_check(true).unwrap();
    core.vault_mut()
        .unwrap()
        .add_draft(
            &case,
            "cognitive",
            "[ילד] הציג תפקוד ממוצע.",
            Author::Ai,
            &[],
        )
        .unwrap();
    let Err(CoreError::Refused(why)) = core.prepare_report_check(&case) else {
        panic!("a waiting draft must stop the check");
    };
    assert!(why.contains("מחכות לאישור"), "{why}");
    assert!(fake.sent.lock().unwrap().is_empty());
}

#[test]
fn one_approved_section_has_nothing_to_compare_with() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    core.set_report_check(true).unwrap();
    core.add_own_paragraph(&case, "background", "ההריון והלידה עברו ללא סיבוכים.")
        .unwrap();
    assert!(matches!(
        core.prepare_report_check(&case),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn the_notes_point_at_section_keys_with_names_back_and_change_nothing() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.set_report_check(true).unwrap();
    let before = core.case_detail(&case).unwrap();
    let id = core
        .prepare_report_check(&case)
        .unwrap()
        .approval_id
        .unwrap();

    // A failed send is not a send: the approval stays.
    let out = core.begin_send(&id, SendKind::ReportCheck).unwrap();
    assert!(core
        .finish_report_check(out, Err(CoreError::Internal("x".into())))
        .is_err());

    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "notes": [
        { "kind": "summary_only", "sections": ["R3", "R9"], "note": "בסיכום [ילד] מתואר כסקרן, ואין לזה בסיס בגוף הדוח" },
        { "kind": "repetition", "sections": ["R2", "R3"], "note": "ההכנה מראש חוזרת" }
    ]})));
    let result = core.send_report_check(&id).unwrap();
    assert_eq!(result.notes.len(), 2);
    assert_eq!(result.notes[0].kind, dv_ai::ReportNoteKind::SummaryOnly);
    assert_eq!(result.notes[0].sections, vec!["summary".to_owned()]);
    assert!(result.notes[0].note.contains("אלון"), "names are put back");
    assert_eq!(
        result.notes[1].sections,
        vec!["kindergarten".to_owned(), "summary".to_owned()]
    );
    // Only notes: every paragraph is as it was.
    let after = core.case_detail(&case).unwrap();
    let texts = |d: &CaseDetail| -> Vec<(String, DraftStatus)> {
        d.sections
            .iter()
            .flat_map(|s| s.paragraphs.iter().map(|p| (p.text.clone(), p.status)))
            .collect()
    };
    assert_eq!(texts(&before), texts(&after));
}

#[test]
fn the_check_works_in_demo_mode_without_a_key_or_a_transport() {
    let (_dir, mut core, case) = setup(None);
    approved_report(&mut core, &case);
    core.set_report_check(true).unwrap();
    let prepared = core.prepare_report_check(&case).unwrap();
    assert!(prepared.demo_mode);
    let result = core
        .send_report_check(&prepared.approval_id.unwrap())
        .unwrap();
    assert!(result.demo);
    assert_eq!(
        core.vault_ref().unwrap().transmissions(&case).unwrap()[0].model,
        "demo"
    );
}

/// The approved paragraphs go through the same filter and gate as any request, each one by
/// itself: a first name and an ID number in one of them are hidden before anything is sent,
/// and the name is shown on the card so she can see it was hidden.
#[test]
fn an_approved_paragraph_with_a_name_or_an_id_goes_out_hidden() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.set_report_check(true).unwrap();
    // Assembled at run time: no valid ID number sits in the source.
    let id = (0..10)
        .map(|d| format!("31415926{d}"))
        .find(|s| dv_privacy::patterns::is_valid_israeli_id(s))
        .unwrap();
    // Written straight into the vault, past the tagging her own text goes through: the check
    // must not rely on what was stored.
    let v = core.vault_mut().unwrap();
    let d = v
        .add_draft(
            &case,
            "summary",
            &format!("ולשירן אין קשר לגן. מספר זהות {id}."),
            Author::Ai,
            &[],
        )
        .unwrap();
    v.set_draft_status(&case, &d.id, DraftStatus::Approved)
        .unwrap();

    let prepared = core.prepare_report_check(&case).unwrap();
    assert!(
        prepared.auto_hidden.iter().any(|a| a.token == "שירן"),
        "{:?}",
        prepared.auto_hidden
    );
    assert!(
        prepared.hidden.iter().any(|h| h == "מספר זהות"),
        "{:?}",
        prepared.hidden
    );
    let shown: String = prepared
        .parts
        .iter()
        .flat_map(|p| p.outgoing.iter().map(|s| s.text.as_str()))
        .collect();
    assert!(
        !shown.contains("שירן") && !shown.contains(id.as_str()),
        "{shown}"
    );

    core.send_report_check(&prepared.approval_id.expect("cleared once hidden"))
        .unwrap();
    let sent = fake.sent.lock().unwrap().join("\n");
    assert!(sent.contains("[מספר_זהות_1]"), "{sent}");
    for word in ["שירן", id.as_str()] {
        assert!(!sent.contains(word), "{word} went out");
    }
}
