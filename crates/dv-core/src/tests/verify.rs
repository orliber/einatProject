//! The check of a draft against its sources.

use super::*;

fn drafted(core: &mut Core, case: &str) {
    let id = core
        .prepare_section(case, "kindergarten", "תנסח פסקה על המעברים")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_section(&id).unwrap();
}

fn proposed_count(core: &mut Core, case: &str) -> usize {
    let detail = core.case_detail(case).unwrap();
    detail
        .sections
        .iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .iter()
        .filter(|p| p.status == DraftStatus::Proposed)
        .count()
}

#[test]
fn the_check_against_the_sources_is_off_until_she_turns_it_on() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    drafted(&mut core, &case);
    assert!(!core.verify_stage_on().unwrap());
    assert!(matches!(
        core.prepare_verify(&case, "kindergarten"),
        Err(CoreError::Refused(_))
    ));
    core.set_verify_stage(true).unwrap();
    assert!(core.prepare_verify(&case, "kindergarten").is_ok());
}

#[test]
fn the_check_goes_through_the_gate_with_tags_only_and_changes_nothing() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &case);
    core.set_verify_stage(true).unwrap();
    let before = proposed_count(&mut core, &case);
    assert!(before > 0);

    let prepared = core.prepare_verify(&case, "kindergarten").unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    let id = prepared.approval_id.clone().expect("cleared by the gate");
    let result = core.send_verify(&id).unwrap();

    assert_eq!(result.checks.len(), before);
    assert_eq!(result.draft_ids.len(), before);
    // The demo check reads for numbers only, so it never says "supported" by itself.
    assert!(result
        .checks
        .iter()
        .all(|c| c.verdict != dv_ai::Verdict::Supported));

    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "the draft and the check are two requests");
    for name in ["אלון", "שירה", "רותם", "אלמוג"] {
        assert!(!sent[1].contains(name), "{name} left the machine");
    }
    assert!(sent[1].contains("[ילד]"));
    assert!(
        sent[1].contains("paragraph=\\\"P1\\\"") && sent[1].contains("id=\\\"S1\\\""),
        "paragraphs and sources go out"
    );

    // Only a verdict: the paragraphs are as they were, still proposed.
    assert_eq!(proposed_count(&mut core, &case), before);
    // The approval is one use only, like every other.
    assert!(matches!(core.send_verify(&id), Err(CoreError::NotFound(_))));
}

#[test]
fn the_check_works_in_demo_mode_without_a_key_or_a_transport() {
    let (_dir, mut core, case) = setup(None);
    drafted(&mut core, &case);
    core.set_verify_stage(true).unwrap();
    let prepared = core.prepare_verify(&case, "kindergarten").unwrap();
    assert!(prepared.demo_mode);
    let result = core.send_verify(&prepared.approval_id.unwrap()).unwrap();
    assert!(result.demo && !result.checks.is_empty());
}

#[test]
fn a_verdict_note_is_shown_with_names_and_a_failed_send_keeps_the_approval() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &case);
    core.set_verify_stage(true).unwrap();
    let id = core
        .prepare_verify(&case, "kindergarten")
        .unwrap()
        .approval_id
        .unwrap();

    // A failed send is not a send.
    let out = core.begin_send(&id, SendKind::Verify).unwrap();
    let failed = core.finish_verify(out, Err(CoreError::Internal("x".into())));
    assert!(failed.is_err());

    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "results": [
        { "paragraph": "P1", "verdict": "contradicted",
          "notes": [{ "note": "[ילד] מתואר אחרת במקור", "source_refs": ["S1", "S9"] }] }
    ]})));
    let result = core.send_verify(&id).unwrap();
    assert_eq!(result.checks[0].verdict, dv_ai::Verdict::Contradicted);
    assert_eq!(result.checks[0].notes[0].note, "אלון מתואר אחרת במקור");
    assert_eq!(
        result.checks[0].notes[0].source_refs,
        vec!["S1".to_owned()],
        "a source that was not sent is dropped"
    );
}

#[test]
fn nothing_to_check_means_nothing_is_sent() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    core.set_verify_stage(true).unwrap();
    // No draft yet.
    assert!(matches!(
        core.prepare_verify(&case, "kindergarten"),
        Err(CoreError::Refused(_))
    ));
    // A section written from the approved ones has no sources to check against.
    assert!(matches!(
        core.prepare_verify(&case, "summary"),
        Err(CoreError::Refused(_))
    ));
}

/// "להחזיר" on another case's name in this case ("שירה" is the first case's kindergarten
/// teacher, here the singing): the next send lets the word out. It used to be hidden again
/// on every send, so the button did nothing (D-050). The first case still hides its name.
#[test]
fn restoring_another_cases_name_holds_on_the_next_send() {
    let (_dir, mut core, case1) = setup(None);
    let case2 = core
        .create_case(
            CaseMeta {
                code: "TEST-0098".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "נועם".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &case2,
        InputKind::Kindergarten,
        "עדכון",
        "נועם אוהב שירה ומוזיקה, ושר כל היום.",
    )
    .unwrap();
    let p = core
        .prepare_section(&case2, "kindergarten", "טיוטה")
        .unwrap();
    let item = p
        .auto_hidden
        .iter()
        .find(|a| a.token == "שירה")
        .unwrap_or_else(|| panic!("hidden first: {}", outgoing(&p)))
        .clone();
    core.restore_auto_hidden(&case2, &item.token, &item.tag)
        .unwrap();
    let p = core
        .prepare_section(&case2, "kindergarten", "טיוטה")
        .unwrap();
    assert!(p.approval_id.is_some(), "{:?}", p.blocked);
    assert!(outgoing(&p).contains("שירה ומוזיקה"), "{}", outgoing(&p));
    let p = core
        .prepare_section(&case1, "kindergarten", "טיוטה")
        .unwrap();
    assert!(p.approval_id.is_some(), "{:?}", p.blocked);
    assert!(!outgoing(&p).contains("שירה"), "{}", outgoing(&p));
}

#[test]
fn a_finished_report_teaches_the_style_once_and_is_forgotten_with_its_case() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(
        &case,
        "referral",
        "ההורים פנו בשל קושי במעברים בגן. נראה כי הקושי בולט בעיקר בבקרים.",
    )
    .unwrap();
    let learned = |core: &mut Core| core.style_overview().unwrap().learned;
    assert_eq!(learned(&mut core).finished_reports, 0);
    core.export_report(&case, None).unwrap();
    let after = learned(&mut core);
    assert_eq!(after.finished_reports, 1);
    assert_eq!(after.from_finals, 1, "one paragraph: {after:?}");
    // Exported again: still one report.
    core.export_report(&case, None).unwrap();
    assert_eq!(learned(&mut core).finished_reports, 1);
    // Erased for good: no longer learned from.
    core.delete_case(&case).unwrap();
    core.purge_case(&case, PASSWORD).unwrap();
    assert_eq!(learned(&mut core).finished_reports, 0);
}

#[test]
fn a_tag_left_by_a_name_brought_back_no_longer_blocks_the_case() {
    let (_dir, mut core, case) = setup(Some(FakeTransport::default()));
    // An approved paragraph written while a word was kept as an institution, whose row is
    // gone since (she pressed "להחזיר" on it in an older version).
    let v = core.vault_mut().unwrap();
    let d = v
        .add_draft(
            &case,
            "kindergarten",
            "[ילד] משתלב היטב ב[מוסד_1] ומשתף פעולה עם הצוות.",
            Author::Ai,
            &[],
        )
        .unwrap();
    v.set_draft_status(&case, &d.id, DraftStatus::Approved)
        .unwrap();
    let prepared = core
        .prepare_section(&case, "recommendations", "המלצות")
        .unwrap();
    assert!(prepared.approval_id.is_some(), "{:?}", prepared.blocked);
    assert!(outgoing(&prepared).contains("משתלב היטב במוסד"));
    let kept = core
        .vault_ref()
        .unwrap()
        .drafts(&case, "kindergarten")
        .unwrap();
    assert_eq!(
        kept[0].text_tagged,
        "[ילד] משתלב היטב במוסד ומשתף פעולה עם הצוות."
    );
    assert_eq!(kept[0].status, DraftStatus::Approved);
}

#[test]
fn a_bug_report_carries_only_crash_lines_in_the_hooks_own_shape() {
    let mut log = String::new();
    for i in 0..7 {
        log.push_str(&format!(
            "17000000{i} · v0.5.1 · thread main · crates/dv-core/src/lib.rs:{i}0 · (message left out)\n"
        ));
    }
    log.push_str("garbage someone wrote\n");
    log.push_str("1700000099 · v0.5.1 · thread main · דוח של נועם.docx:3 · x\n");
    log.push_str("1700000099 · v0.5.1 · thread ילד · a.rs:3 · x\n");
    log.push_str("1700000099 · v0.5.1 · thread main · a.rs:3 · x · y\n");
    let kept = recent_crashes(&log);
    assert_eq!(kept.len(), 5);
    assert!(kept[0].starts_with("170000002 "));
    assert!(kept[4].starts_with("170000006 "));
    assert!(kept
        .iter()
        .all(|l| !l.contains("נועם") && !l.contains("ילד")));
    assert!(recent_crashes("").is_empty());
}

#[test]
fn the_report_is_checked_against_itself_and_the_details() {
    let (_dir, mut core, case) = setup(None);
    core.add_own_paragraph(&case, "kindergarten", "הוא לומד בכיתה א' בבית ספר שכונתי.")
        .unwrap();
    core.add_own_paragraph(
        &case,
        "appearance",
        "הילדה הגיעה בשמחה. היום הוא תלמיד כיתה ב'.",
    )
    .unwrap();
    core.add_own_paragraph(
        &case,
        "recommendations",
        "מומלץ טיפול בתקשורת אצל קלינאית תקשורת.",
    )
    .unwrap();
    let detail = core.case_detail(&case).unwrap();
    let notes = |key: &str| -> Vec<String> {
        detail
            .sections
            .iter()
            .find(|s| s.key == key)
            .unwrap()
            .paragraphs
            .iter()
            .flat_map(|p| p.warnings.clone())
            .collect()
    };
    let seen = notes("appearance");
    assert!(seen.iter().any(|n| n.contains("זה ילד")), "{seen:?}");
    assert!(seen.iter().any(|n| n.contains("כיתה ב'")), "{seen:?}");
    let rec = notes("recommendations");
    assert_eq!(
        rec,
        vec!["המלצה על טיפול בתקשורת, ואין לה בסיס בגוף הדוח".to_owned()]
    );
}

#[test]
fn a_long_material_goes_to_a_section_with_the_passages_that_bear_on_it() {
    let (_dir, mut core, case) = setup(None);
    let mut text = String::new();
    for i in 0..40 {
        text.push_str(&format!(
            "בגן הוא משחק ליד חברים בחצר, והגננת מספרת שבמפגש הבוקר הוא מתקשה לשבת עד הסוף, פעם {i}.\n\n"
        ));
        text.push_str(&format!(
            "ההריון עבר ללא סיבוכים והלידה הייתה בשבוע ארבעים, והמשקל בלידה היה תקין, פעם {i}.\n\n"
        ));
    }
    core.add_input(&case, InputKind::FreeText, "סיכום ארוך", &text)
        .unwrap();
    let id = core
        .case_detail(&case)
        .unwrap()
        .inputs
        .last()
        .unwrap()
        .id
        .clone();
    core.set_input_sections(&case, &id, &["kindergarten".to_owned()])
        .unwrap();
    let prepared = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap();
    let label = prepared
        .parts
        .iter()
        .map(|p| p.label.clone())
        .find(|l| l.contains("סיכום ארוך"))
        .expect("the material goes");
    assert!(label.contains("מתוך 80 קטעים"), "{label}");
    let out = outgoing(&prepared);
    assert!(
        out.contains("במפגש הבוקר") && !out.contains("ההריון"),
        "{out}"
    );
}
