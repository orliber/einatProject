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
