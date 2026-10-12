//! D-022: sorting materials into sections.

use super::*;

/// A third material with three passages: pregnancy, milestones, home.
pub(super) fn add_home_intake(core: &mut Core, case: &str) -> String {
    core.add_input(
        case,
        InputKind::Intake,
        "אינטייק שני",
        "ההריון והלידה עברו ללא סיבוכים.\n\nהלך בגיל שנה ואמר מילים ראשונות בגיל שנה וחצי.\n\nבבית אלון אוהב לבנות מגדלים ממגנטים.",
    )
    .unwrap()
    .id
}

/// Claude's sorting for the three materials of the test case (S1 kindergarten talk, S2 first
/// intake, S3 the intake above). The first intake gets nothing: it stays on the table.
pub(super) fn claude_sorting() -> Value {
    api_json(&json!({"sections": [
        {"section": "background", "passages": ["S3P1", "S3P2"]},
        {"section": "parents_view", "passages": ["S3P3"]},
        {"section": "kindergarten", "passages": ["S1P1"]},
        {"section": "summary", "passages": ["S3P1"]},
        {"section": "background", "passages": ["S9P1"]}
    ]}))
}

fn section_request(core: &mut Core, fake: &FakeTransport, case: &str, key: &str) -> String {
    *fake.answer.lock().unwrap() = None;
    let prepared = core.prepare_section(case, key, "טיוטה").unwrap();
    let approval = prepared.approval_id.expect("clears the gate");
    core.send_section(&approval).unwrap();
    fake.sent.lock().unwrap().last().unwrap().clone()
}

#[test]
fn sorting_goes_out_filtered_and_drafts_then_get_only_their_passages() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let home = add_home_intake(&mut core, &case);

    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    assert!(!prepared.demo_mode);
    assert_eq!(prepared.parts.len(), 3, "one review part per material");
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    let result = core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    assert_eq!((result.sorted, result.unchanged, result.links), (2, 1, 3));
    // The derived section and the unknown passage are dropped, never guessed.
    assert_eq!(result.ignored, 2);
    assert!(!result.demo);

    let sent = fake.sent.lock().unwrap()[0].clone();
    assert!(sent.contains("id=\\\"S3P2\\\"") && sent.contains("[ילד]"));
    for name in ["אלון", "שירה", "רותם", "אלמוג"] {
        assert!(!sent.contains(name), "{name} left the machine");
    }

    let detail = core.case_detail(&case).unwrap();
    let r = detail.routing.iter().find(|r| r.input_id == home).unwrap();
    assert!(r.sorted && r.by_ai);
    assert_eq!(r.feeds, ["background", "parents_view"]);
    assert_eq!((r.passages, r.used_passages), (3, 3));
    // The first intake was not placed: it keeps the table.
    let first = &detail.routing[1];
    assert!(!first.sorted && !first.needs_sorting);
    assert_eq!(first.feeds, first.table);

    // Background: the two chosen passages of the new intake, and the whole first intake.
    let background = section_request(&mut core, &fake, &case, "background");
    assert!(background.contains("ההריון") && background.contains("מילים ראשונות"));
    assert!(
        !background.contains("מגדלים"),
        "a passage chosen for another section"
    );
    assert!(
        background.contains("נרדם באיחור"),
        "the unsorted intake still goes whole"
    );
    // Home: only the third passage.
    let home_view = section_request(&mut core, &fake, &case, "parents_view");
    assert!(home_view.contains("מגדלים") && !home_view.contains("ההריון"));
    // Everything sorted: nothing new to send.
    assert!(matches!(
        core.prepare_sort(&case),
        Err(CoreError::Refused(_))
    ));
}

#[test]
fn chosen_passages_are_shown_in_the_review_exactly_as_sent() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();

    let prepared = core.prepare_section(&case, "background", "טיוטה").unwrap();
    let part = prepared
        .parts
        .iter()
        .find(|p| p.label.contains("קטעים שנבחרו לסעיף"))
        .expect("the chosen passages are labelled");
    let shown: String = part.outgoing.iter().map(|s| s.text.as_str()).collect();
    assert!(shown.contains("ההריון") && shown.contains("מילים ראשונות"));
    assert!(!shown.contains("מגדלים"));
}

#[test]
fn einat_decides_last_and_an_edit_forgets_the_old_passages() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let home = add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(claude_sorting());
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();

    core.set_input_sections(&case, &home, &["parents_view".into(), "cognitive".into()])
        .unwrap();
    let r = |core: &mut Core| {
        core.case_detail(&case)
            .unwrap()
            .routing
            .into_iter()
            .find(|r| r.input_id == home)
            .unwrap()
    };
    let now = r(&mut core);
    assert_eq!(now.feeds, ["parents_view", "cognitive"]);
    assert_eq!(
        (now.added.as_slice(), now.removed.as_slice()),
        (
            &["cognitive".to_owned()][..],
            &["background".to_owned()][..]
        )
    );
    // Added by hand: the whole material goes there.
    let cognitive = section_request(&mut core, &fake, &case, "cognitive");
    assert!(cognitive.contains("ההריון") && cognitive.contains("מגדלים"));
    let background = section_request(&mut core, &fake, &case, "background");
    assert!(!background.contains("ההריון"));

    // Only sections written from materials can be chosen.
    assert!(matches!(
        core.set_input_sections(&case, &home, &["summary".into()]),
        Err(CoreError::NotFound(_))
    ));

    // A new text has new passages: the sorting is forgotten, her choices stay.
    core.update_input(&case, &home, "אינטייק שני", "ההורים מתארים ילד סקרן.")
        .unwrap();
    let after = r(&mut core);
    assert!(!after.sorted);
    assert_eq!(after.feeds, ["referral", "parents_view", "cognitive"]);
}

#[test]
fn a_sorting_that_arrives_after_an_edit_is_not_applied() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let home = add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    let out = core
        .begin_send(&prepared.approval_id.unwrap(), SendKind::Sort)
        .unwrap();
    // Einat edits while Claude reads (same number of passages, different text).
    core.update_input(
        &case,
        &home,
        "אינטייק שני",
        "ההריון היה במעקב.\n\nהלך בגיל שנה.\n\nבבית משחק לבד.",
    )
    .unwrap();
    let result = core
        .finish_sort(out, Ok((claude_sorting(), false)))
        .unwrap();
    assert_eq!(result.sorted, 1, "only the kindergarten talk");
    let detail = core.case_detail(&case).unwrap();
    assert!(
        !detail
            .routing
            .iter()
            .find(|r| r.input_id == home)
            .unwrap()
            .sorted
    );
}

#[test]
fn sorting_needs_consent_and_hides_unknown_names() {
    let (_dir, mut core, case) = setup(None);
    core.add_input(
        &case,
        InputKind::SessionNote,
        "מפגש",
        "בפינת הבנייה סיפר שהוא משחק עם יובל.",
    )
    .unwrap();
    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.approval_id.is_some(), "{:?}", prepared.blocked);
    assert!(prepared.auto_hidden.iter().any(|a| a.token == "יובל"));
    assert!(!outgoing(&prepared).contains("יובל"));

    let mut meta = core.case_detail(&case).unwrap().meta;
    meta.consent = None;
    core.update_case(&case, meta).unwrap();
    assert!(matches!(
        core.prepare_sort(&case),
        Err(CoreError::ConsentMissing)
    ));
}

#[test]
fn demo_sorting_runs_locally_and_says_so() {
    let (_dir, mut core, case) = setup(None);
    let home = add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    assert!(prepared.demo_mode);
    let result = core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    assert!(result.demo && result.sorted >= 1);
    let detail = core.case_detail(&case).unwrap();
    let r = detail.routing.iter().find(|r| r.input_id == home).unwrap();
    assert!(r.sorted && !r.by_ai);
    assert!(r.feeds.contains(&"background".to_owned()));
    // The section counts follow the routing.
    let kg = detail
        .sections
        .iter()
        .find(|s| s.key == "kindergarten")
        .unwrap();
    assert!(kg.sortable && kg.source_count >= 1);
    assert!(
        !detail
            .sections
            .iter()
            .find(|s| s.key == "summary")
            .unwrap()
            .sortable
    );
}

#[test]
fn passages_far_apart_are_marked_with_a_gap_the_gate_accepts() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    add_home_intake(&mut core, &case);
    let prepared = core.prepare_sort(&case).unwrap();
    // Passages 1 and 3, not 2: the draft request carries a gap mark between them.
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"sections": [
        {"section": "background", "passages": ["S3P1", "S3P3"]}
    ]})));
    core.send_sort(&prepared.approval_id.unwrap()).unwrap();
    let prepared = core.prepare_section(&case, "background", "טיוטה").unwrap();
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    let background = section_request(&mut core, &fake, &case, "background");
    assert!(background.contains("(…)") && !background.contains("מילים ראשונות"));
}
