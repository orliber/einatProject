//! The mask book through the core (round 25): a phone or an address in her materials goes out
//! as a numbered tag of the case, the tag keeps its words in the vault, and Claude's answer
//! shows them as she wrote them. Fabricated data only; nothing leaves the machine.

use super::*;

fn kindergarten_paragraphs(core: &mut Core, case: &str) -> Vec<String> {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .unwrap()
        .paragraphs
        .into_iter()
        .map(|p| p.text)
        .collect()
}

#[test]
fn a_phone_and_an_address_go_out_as_tags_and_come_back_as_written() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // Assembled at run time: the repository scanner never sees a phone number here.
    let phone = ["05", "4", "7654321"].concat();
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "פרטים מהגננת",
        &format!("הגננת השאירה טלפון {phone}. המשפחה גרה ברחוב הדקלים 12."),
    )
    .unwrap();
    let p = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    let book = core.vault_ref().unwrap().masks(&case).unwrap();
    let of = |kind: &str| book.iter().find(|e| e.kind == kind).cloned().unwrap();
    let (phone_e, address_e) = (of("טלפון"), of("כתובת"));
    assert_eq!(phone_e.original, phone);
    assert!(address_e.original.contains("הדקלים"), "{address_e:?}");

    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{
            "text": format!("[ילד] גר ב{}, והגננת השאירה את {}.", address_e.tag, phone_e.tag),
            "source_refs": ["S1"],
        }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(
        !sent[0].contains(&phone) && !sent[0].contains("הדקלים"),
        "{}",
        sent[0]
    );
    assert!(sent[0].contains(&phone_e.tag) && sent[0].contains(&address_e.tag));

    // The draft reads as she wrote it: no square brackets.
    let shown = kindergarten_paragraphs(&mut core, &case);
    assert_eq!(
        shown,
        [format!(
            "אלון גר ב{}, והגננת השאירה את {phone}.",
            address_e.original
        )]
    );
    // Kept tagged: the vault's draft holds the tags, not the words.
    let stored = core
        .vault_ref()
        .unwrap()
        .drafts(&case, "kindergarten")
        .unwrap();
    assert!(
        stored[0].text_tagged.contains(&phone_e.tag),
        "{}",
        stored[0].text_tagged
    );

    // A later request of the case gives the same words the same tag, and adds none.
    let again = core
        .prepare_section(&case, "kindergarten", "עוד פסקה")
        .unwrap();
    assert!(again.blocked.is_empty(), "{:?}", again.blocked);
    assert_eq!(core.vault_ref().unwrap().masks(&case).unwrap(), book);
}

#[test]
fn a_consultation_without_a_case_gives_the_numbers_back_too() {
    let fake = FakeTransport::default();
    let (_dir, mut core, _case) = setup(Some(fake.clone()));
    let phone = ["05", "2", "1234567"].concat();
    let p = core
        .prepare_consult(
            None,
            None,
            &format!("ההורה השאיר את המספר {phone}. מה עושים?"),
        )
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    let tag = "[טלפון_1]";
    *fake.answer.lock().unwrap() = Some(api_json(
        &json!({ "answer": format!("אפשר להתקשר ל{tag}.") }),
    ));
    let got = core.send_consult(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(
        !sent[0].contains(&phone) && sent[0].contains(tag),
        "{}",
        sent[0]
    );
    assert!(
        format!("{got:?}").contains(&format!("אפשר להתקשר ל{phone}.")),
        "{got:?}"
    );
}

/// A refusal the fix loop answers no longer stops her. Text the filter never saw (here put in
/// the request by hand, as a template, a bug or a future feature could) holds the child's name
/// and a phone: the gate refuses it, the loop leaves the name's line out and hides the phone in
/// the outgoing copy, the same gate clears that copy, and only the phone is kept in the case's
/// book for the answer (the review of stage 2: nothing of a refused name's line is kept).
#[test]
fn a_refusal_the_loop_can_answer_is_hidden_and_cleared_by_the_same_gate() {
    let (_dir, mut core, case) = setup(None);
    let phone = ["05", "3", "1112223"].concat();
    let body = json!({
        "model": "claude-test",
        "system": "עוזרת לכתיבת דוח.",
        "messages": [{"role": "user", "content": format!("אלון מתקשה.\nטלפון {phone}.")}],
    });
    let data = core.privacy_data(&case).unwrap();
    let mut prepared = Prepared {
        approval_id: None,
        parts: Vec::new(),
        suspects: Vec::new(),
        auto_hidden: Vec::new(),
        hidden: Vec::new(),
        checks: Checks::default(),
        blocked: Vec::new(),
        demo_mode: false,
    };
    let kind = PendingKind::Sort {
        case_id: case.clone(),
        materials: Vec::new(),
        sections: Vec::new(),
    };
    assert!(!core.gate(&data, &body, kind, &mut prepared, false).unwrap());
    assert!(prepared.blocked.is_empty(), "{:?}", prepared.blocked);
    let id = prepared.approval_id.clone().expect("cleared after the fix");
    let sent = String::from_utf8_lossy(core.pending[&id].payload.body()).into_owned();
    assert!(!sent.contains("אלון") && !sent.contains(&phone), "{sent}");
    assert!(sent.contains(dv_privacy::LEFT_OUT), "{sent}");
    let fixed = prepared.parts.last().unwrap();
    assert_eq!(fixed.label, "הוסתר בבדיקה האחרונה לפני השליחה");
    let book = core.vault_ref().unwrap().masks(&case).unwrap();
    assert!(
        book.iter().all(|e| !e.original.contains("אלון")),
        "{book:?}"
    );
    assert!(
        book.iter()
            .any(|e| e.original == phone && sent.contains(&e.tag)),
        "{book:?}"
    );
    let audit = core.vault_ref().unwrap().audit_entries(1000).unwrap();
    let blocked: Vec<_> = audit.iter().filter(|e| e.event == "blocked").collect();
    assert_eq!(blocked.len(), 1, "{audit:?}");
    assert!(
        format!("{:?}", blocked[0]).contains("fixed"),
        "{:?}",
        blocked[0]
    );
}

/// What the loop cannot answer still stops the request, with its reason: a name in a key.
#[test]
fn a_refusal_the_loop_cannot_answer_still_stops_the_request() {
    let (_dir, mut core, case) = setup(None);
    let body = json!({"model": "claude-test", "messages": [{"role": "user", "content": "שלום"}], "אלון": 1});
    let data = core.privacy_data(&case).unwrap();
    let mut prepared = Prepared {
        approval_id: None,
        parts: Vec::new(),
        suspects: Vec::new(),
        auto_hidden: Vec::new(),
        hidden: Vec::new(),
        checks: Checks::default(),
        blocked: Vec::new(),
        demo_mode: false,
    };
    let kind = PendingKind::Sort {
        case_id: case.clone(),
        materials: Vec::new(),
        sections: Vec::new(),
    };
    core.gate(&data, &body, kind, &mut prepared, false).unwrap();
    assert!(prepared.approval_id.is_none());
    assert!(
        prepared.blocked.iter().any(|r| r.code == "identity"),
        "{:?}",
        prepared.blocked
    );
}

/// Too large for one send (round 25): one long material on a single line is one passage, so the
/// passage choice cannot cut it and the request is over the limit; it is shortened and goes,
/// with a part on the request saying what was cut.
#[test]
fn a_request_too_large_is_shortened_not_refused() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let line = "אלון משחק בחצר עם ילדים אחרים ומחכה לתורו בסבלנות ";
    let long = line.repeat(MAX_REQUEST_BYTES / line.len() + 2000);
    core.add_input(&case, InputKind::Kindergarten, "יומן הגן", &long)
        .unwrap();
    let p = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    let note = p
        .parts
        .iter()
        .find(|x| x.label == "קוצר, כי הבקשה הייתה ארוכה מדי")
        .expect("the cut is shown with the request");
    let said: String = note.outgoing.iter().map(|s| s.text.as_str()).collect();
    assert!(said.contains("ההמשך נשאר במחשב"), "{said}");
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].len() <= MAX_REQUEST_BYTES, "{}", sent[0].len());
    assert!(sent[0].contains("הוא נשאר במחשב."));
    assert!(sent[0].contains("הפסיכולוגית מבקשת: תנסח פסקה"));
}

/// Stored texts are settled against the case's tags before each filtering, and the pass is
/// skipped only while nothing was written since the last one (the deep review): a tag that
/// is not the case's, written after a settle, is still settled by the next request.
#[test]
fn a_foreign_tag_written_after_a_settle_is_still_settled() {
    let (_dir, mut core, case) = setup(None);
    let stored = |core: &mut Core| -> Vec<String> {
        core.vault_ref()
            .unwrap()
            .drafts(&case, "kindergarten")
            .unwrap()
            .into_iter()
            .map(|d| d.text_tagged)
            .collect()
    };
    let add = |core: &mut Core, text: &str| {
        core.vault_mut()
            .unwrap()
            .add_draft(&case, "kindergarten", text, Author::User, &[])
            .unwrap();
    };
    add(&mut core, "[ילד] למד ב[מוסד_9].");
    core.privacy_data(&case).unwrap();
    assert_eq!(stored(&mut core), ["[ילד] למד במוסד."]);
    // Nothing written: the next request does not read the texts again, and nothing changes.
    let counts = core.vault_ref().unwrap().change_counts();
    core.privacy_data(&case).unwrap();
    assert_eq!(core.vault_ref().unwrap().change_counts(), counts);
    // A new text with a foreign tag: settled by the next request.
    add(&mut core, "שיחק ב[מוסד_8].");
    core.privacy_data(&case).unwrap();
    assert_eq!(stored(&mut core), ["[ילד] למד במוסד.", "שיחק במוסד."]);
}
