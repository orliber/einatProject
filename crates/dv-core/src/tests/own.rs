//! The sections she writes herself (round 30): nothing is sent for them but her points, on
//! request; the full draft and the sorting leave them out; she decides in settings which
//! sections are hers. Fabricated data only; nothing leaves the machine.

use super::*;

fn section(core: &mut Core, case: &str, key: &str) -> SectionView {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == key)
        .unwrap()
}

#[test]
fn her_sections_send_nothing_but_her_points_when_she_asks() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_input(
        &case,
        InputKind::Observation,
        "תצפית במפגש",
        "אלון הגיע למפגש בשמחה ושיתף פעולה.",
    )
    .unwrap();
    let shown = section(&mut core, &case, "appearance");
    assert_eq!(shown.mode, dv_domain::SectionMode::Own);
    assert_eq!(shown.source_count, 0, "no material is sorted into it");
    assert!(!shown.sortable);

    // Drafting or rewording it from the materials is refused before anything is built.
    assert!(matches!(
        core.prepare_section(&case, "appearance", "תנסח"),
        Err(CoreError::Refused(_))
    ));
    // Her points: only they, the guide and her style go out.
    let p = core
        .prepare_from_points(
            &case,
            "appearance",
            "הגיע עם אמא, נפרד בקלות\nשיתף פעולה 45 דקות",
        )
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{
            "text": "[ילד] הגיע עם אמו ונפרד ממנה בקלות, ושיתף פעולה לאורך 45 דקות ו-3 משימות.",
            "source_refs": [],
            "guide": 1,
        }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(dv_ai::POINTS_LINE));
    assert!(sent[0].contains("נפרד בקלות"));
    assert!(sent[0].contains("ההנחיה של הפסיכולוגית לסעיף:"));
    // No material, not even the observation that would fit.
    assert!(
        !sent[0].contains("בשמחה") && !sent[0].contains("kind="),
        "{}",
        sent[0]
    );
    assert!(!sent[0].contains("אלון"));
    // A number she did not write is pointed out; hers is not.
    let w = &r.paragraphs[0].warnings;
    assert!(
        w.iter()
            .any(|x| x.contains("המספר 3") && x.contains("בנקודות שלך")),
        "{w:?}"
    );
    assert!(!w.iter().any(|x| x.contains("45")), "{w:?}");
    assert_eq!(section(&mut core, &case, "appearance").paragraphs.len(), 1);
    // The guide's other lines have no paragraph yet.
    assert_eq!(r.missing.len(), 3, "{:?}", r.missing);

    // Empty points, and points in a section the AI writes, are refused.
    assert!(matches!(
        core.prepare_from_points(&case, "appearance", "  "),
        Err(CoreError::Refused(_))
    ));
    assert!(matches!(
        core.prepare_from_points(&case, "kindergarten", "נקודה"),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn the_full_draft_and_the_sorting_leave_her_sections_out_and_she_can_change_which() {
    let (_dir, mut core, case) = setup(None);
    core.add_input(
        &case,
        InputKind::Observation,
        "תצפית במפגש",
        "אלון הגיע למפגש בשמחה ושיתף פעולה.",
    )
    .unwrap();
    let keys = |core: &mut Core| -> Vec<String> {
        core.prepare_full_draft(&case)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    };
    let drafted = keys(&mut core);
    assert!(!drafted.contains(&"appearance".to_owned()), "{drafted:?}");
    assert!(!drafted.contains(&"emotional".to_owned()), "{drafted:?}");

    let modes = core.section_modes().unwrap();
    let of = |k: &str| modes.iter().find(|m| m.key == k).unwrap().mode;
    assert_eq!(of("appearance"), dv_domain::SectionMode::Own);
    assert_eq!(of("cognitive"), dv_domain::SectionMode::Ai);
    assert!(
        modes.iter().all(|m| m.key != "summary"),
        "derived sections are not listed"
    );

    // She gives the impression back to the AI, and takes the kindergarten section.
    core.set_section_mode("appearance", dv_domain::SectionMode::Ai)
        .unwrap();
    core.set_section_mode("kindergarten", dv_domain::SectionMode::Own)
        .unwrap();
    assert!(section(&mut core, &case, "appearance").sortable);
    assert!(section(&mut core, &case, "appearance").source_count > 0);
    assert_eq!(
        section(&mut core, &case, "kindergarten").mode,
        dv_domain::SectionMode::Own
    );
    let drafted = keys(&mut core);
    assert!(drafted.contains(&"appearance".to_owned()), "{drafted:?}");
    assert!(!drafted.contains(&"kindergarten".to_owned()), "{drafted:?}");
    assert!(matches!(
        core.set_section_mode("summary", dv_domain::SectionMode::Own),
        Err(CoreError::NotFound(_))
    ));
}

/// The review of stage 3: the check against the sources is refused in her section too, and
/// her approved text of it is what the summary is written from (D-068 §6), filtered like
/// any text: the summary needs the impression she wrote.
#[test]
fn her_section_is_not_checked_and_her_approved_text_feeds_the_summary() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.set_verify_stage(true).unwrap();
    assert!(matches!(
        core.prepare_verify(&case, "appearance"),
        Err(CoreError::Refused(_))
    ));
    core.add_own_paragraph(&case, "appearance", "אלון הגיע בשמחה ונפרד מאמו בקלות.")
        .unwrap();
    let prepared = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    core.send_section(&prepared.approval_id.expect("clears the gate"))
        .unwrap();
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(sent.contains("נפרד מאמו בקלות"), "{sent}");
    assert!(!sent.contains("אלון"), "{sent}");
}
