//! "מה יצא מהמחשב" (round 27): what the history shows is what left, byte for byte, with
//! what was hidden in it read back from the case. Fabricated data only.

use super::*;

fn sent_section(core: &mut Core, case: &str) {
    let id = core
        .prepare_section(case, "kindergarten", "תנסח פסקה")
        .unwrap()
        .approval_id
        .expect("a cleared request has an approval");
    core.send_section(&id).unwrap();
}

#[test]
fn the_history_shows_what_left_and_what_was_hidden_in_it() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    sent_section(&mut core, &case);
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);

    let all = core.sent_history(None, None, "").unwrap();
    assert_eq!(all.len(), 1);
    let item = &all[0];
    assert_eq!(item.case_code, "TEST-0002");
    assert_eq!(item.place, "kindergarten");
    assert_eq!(item.ai, "Claude");
    assert!(!item.demo);
    // The child and the kindergarten teacher were in the material: both went out as tags.
    let words: Vec<&str> = item.hidden.iter().map(|h| h.word.as_str()).collect();
    assert!(words.contains(&"אלון"), "{:?}", item.hidden);
    assert!(words.contains(&"שירה"), "{:?}", item.hidden);

    let full = core.sent_text(&case, &item.id).unwrap();
    assert_eq!(full.exact, sent[0], "the exact bytes that left");
    assert!(full.hash_ok);
    assert_eq!(full.item, *item);
    assert_eq!(full.parts[0].label, "הוראות קבועות");
    let asked: String = full.parts[1..].iter().map(|p| p.text.as_str()).collect();
    assert!(asked.contains("מתקשה במעברים"));
    for h in &item.hidden {
        assert!(asked.contains(&h.tag), "{} is in the request", h.tag);
        assert!(!full.exact.contains(&h.word), "{} never left", h.word);
    }
}

#[test]
fn a_search_finds_the_text_that_left_and_says_when_the_word_was_hidden() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake));
    sent_section(&mut core, &case);

    let in_text = core.sent_history(Some(&case), None, "מעברים").unwrap();
    assert_eq!(in_text.len(), 1);
    assert!(!in_text[0].hidden_match);

    // The child's name did not leave: found, and marked as hidden there.
    let hidden = core.sent_history(Some(&case), None, "אלון").unwrap();
    assert_eq!(hidden.len(), 1);
    assert!(hidden[0].hidden_match);

    assert!(core
        .sent_history(Some(&case), None, "מילה שלא הייתה")
        .unwrap()
        .is_empty());
    assert!(core
        .sent_history(Some(&case), Some("intake"), "")
        .unwrap()
        .is_empty());
    assert_eq!(
        core.sent_history(Some(&case), Some("kindergarten"), "")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_question_from_inside_a_case_is_kept_with_it_and_one_without_a_case_is_not() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let ask = core
        .prepare_consult(Some(&case), None, "איך מנסחים המלצה על לוח מעברים?")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_consult(&ask).unwrap();
    let general = core
        .prepare_consult(None, None, "איך מנסחים המלצה כללית?")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_consult(&general).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 2);

    let kept = core.sent_history(None, None, "").unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].place, "consult");
    assert_eq!(kept[0].case_id, case);
}

#[test]
fn newest_first_across_cases_each_with_its_own_names() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake));
    sent_section(&mut core, &case);
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0003".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "יובל".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    core.add_input(
        &other,
        InputKind::Kindergarten,
        "שיחה עם הגננת",
        "יובל משחק בחצר ומשתף פעולה עם חברים.",
    )
    .unwrap();
    sent_section(&mut core, &other);

    let all = core.sent_history(None, None, "").unwrap();
    assert_eq!(all.len(), 2);
    assert!(all[0].at >= all[1].at);
    let codes: HashSet<&str> = all.iter().map(|i| i.case_code.as_str()).collect();
    assert_eq!(codes, HashSet::from(["TEST-0002", "TEST-0003"]));
    // Each case's names are its own: the other child's name is not listed under this case.
    let mine = core.sent_history(Some(&case), None, "").unwrap();
    assert!(mine[0].hidden.iter().all(|h| h.word != "יובל"));

    // A wrong id is not found, also through another case.
    assert!(matches!(
        core.sent_text(&case, &all.iter().find(|i| i.case_id == other).unwrap().id),
        Err(CoreError::NotFound(_))
    ));
}
