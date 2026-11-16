//! The name model (D-053) reads every text of a section's request that is hers or the case's,
//! not only the materials: her request, her points and the current draft. Fabricated data
//! only, with a stand-in for the model; nothing leaves the machine.

use super::*;
use crate::name_model::{NameFinder, NerCell, NAME_MODEL_KEY};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A name no rule knows, only the stand-in model.
const ONLY_THE_MODEL: &str = "זרובבלה";

/// Turn the model on, with a stand-in that finds [`ONLY_THE_MODEL`]; returns its read count.
fn model_on(core: &mut Core) -> Arc<AtomicUsize> {
    core.vault_mut()
        .unwrap()
        .set_setting(NAME_MODEL_KEY, "on")
        .unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    core.ner = Some(NerCell::ready(NameFinder::Fixed(
        vec![ONLY_THE_MODEL.to_owned()],
        Arc::clone(&reads),
    )));
    reads
}

fn sent_body(core: &mut Core, fake: &FakeTransport, p: &Prepared) -> String {
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    core.send_section(p.approval_id.as_deref().unwrap())
        .unwrap();
    fake.sent.lock().unwrap().last().unwrap().clone()
}

#[test]
fn the_rules_alone_do_not_know_the_name() {
    let (_dir, mut core, case) = setup(None);
    let out = core
        .preview_filter(&case, &format!("להוסיף ש{ONLY_THE_MODEL} מלווה אותו לגן"))
        .unwrap();
    assert!(out.tagged.contains(ONLY_THE_MODEL), "{}", out.tagged);
}

#[test]
fn the_model_reads_her_request_in_a_section() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let reads = model_on(&mut core);
    let instruction = format!("להוסיף ש{ONLY_THE_MODEL} מלווה אותו לגן");
    let p = core
        .prepare_section(&case, "kindergarten", &instruction)
        .unwrap();
    let body = sent_body(&mut core, &fake, &p);
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");
    assert!(body.contains("מלווה אותו לגן"), "{body}");

    // Asked again, the model does not read the same texts again.
    let before = reads.load(Ordering::SeqCst);
    core.prepare_section(&case, "kindergarten", &instruction)
        .unwrap();
    let again = reads.load(Ordering::SeqCst);
    // Only the paragraphs the first answer added are new.
    let drafts = core
        .vault_ref()
        .unwrap()
        .drafts(&case, "kindergarten")
        .unwrap()
        .len();
    assert!(again - before <= drafts + 2, "{before} -> {again}");
    core.prepare_section(&case, "kindergarten", &instruction)
        .unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), again, "nothing new to read");
}

#[test]
fn the_model_reads_her_points() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    model_on(&mut core);
    let p = core
        .prepare_from_points(
            &case,
            "appearance",
            &format!("הגיע עם {ONLY_THE_MODEL}, נפרד בקלות"),
        )
        .unwrap();
    let body = sent_body(&mut core, &fake, &p);
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");
    assert!(body.contains("נפרד בקלות"), "{body}");
}

#[test]
fn the_model_reads_the_current_draft() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // Her paragraph, written before she turned the model on.
    core.add_own_paragraph(
        &case,
        "kindergarten",
        &format!("בגן הוא משחק בעיקר עם {ONLY_THE_MODEL} ומספר עליה בבית."),
    )
    .unwrap();
    model_on(&mut core);
    let p = core
        .prepare_section(&case, "kindergarten", "לקצר את הפסקה")
        .unwrap();
    let body = sent_body(&mut core, &fake, &p);
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");
    assert!(body.contains("ומספר עליה בבית"), "{body}");
}

/// The model fails: the request is filtered on the rules alone, as before it was turned on.
#[test]
fn a_failing_model_leaves_the_rules() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    model_on(&mut core);
    core.ner = None; // On, with no models folder: never loads.
    let p = core
        .prepare_section(&case, "kindergarten", "להוסיף על אלון שהוא מלווה לגן")
        .unwrap();
    let body = sent_body(&mut core, &fake, &p);
    assert!(!body.contains("אלון"), "{body}");
}

/// The check against the sources reads the proposed paragraphs with the model too.
#[test]
fn the_model_reads_the_paragraphs_the_check_sends() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // A proposed paragraph with a name in it, from before she turned the model on.
    core.vault_mut()
        .unwrap()
        .add_draft(
            &case,
            "kindergarten",
            &format!("[ילד] משחק בגן בעיקר עם {ONLY_THE_MODEL}."),
            dv_domain::Author::Ai,
            &[],
        )
        .unwrap();
    model_on(&mut core);
    core.set_verify_stage(true).unwrap();
    let p = core.prepare_verify(&case, "kindergarten").unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    core.send_verify(p.approval_id.as_deref().unwrap()).unwrap();
    let body = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");
    assert!(body.contains("משחק בגן בעיקר עם"), "{body}");
}

/// A consultation with no case: a name only the model found in her first question is hidden
/// again when that question goes out as the history of her next one.
#[test]
fn the_model_reads_her_earlier_questions_in_a_consultation() {
    let fake = FakeTransport::default();
    let (_dir, mut core, _case) = setup(Some(fake.clone()));
    model_on(&mut core);
    let first = core
        .prepare_consult(
            None,
            None,
            &format!("איך לעבוד עם {ONLY_THE_MODEL} על מעברים בגן?"),
        )
        .unwrap();
    assert!(first.blocked.is_empty(), "{:?}", first.blocked);
    let answer = core
        .send_consult(first.approval_id.as_deref().unwrap())
        .unwrap();
    let body = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");

    let next = core
        .prepare_consult(
            None,
            Some(&answer.conversation_id),
            "ומה לגבי הפרידה בבוקר?",
        )
        .unwrap();
    assert!(next.blocked.is_empty(), "{:?}", next.blocked);
    core.send_consult(next.approval_id.as_deref().unwrap())
        .unwrap();
    let body = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(body.contains("על מעברים בגן"), "{body}");
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");
}

/// A consultation about the case: a name only the model finds, in an approved paragraph she
/// wrote herself, is hidden in the approved sections that go with it.
#[test]
fn the_model_reads_the_approved_paragraphs_a_consultation_sends() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // Her paragraph, written before she turned the model on.
    core.add_own_paragraph(
        &case,
        "kindergarten",
        &format!("בגן הוא משחק בעיקר עם {ONLY_THE_MODEL} ומספר עליה בבית."),
    )
    .unwrap();
    core.add_own_paragraph(&case, "background", "נרדם באיחור ומתעורר בלילה.")
        .unwrap();
    model_on(&mut core);
    let p = core
        .prepare_consult(Some(&case), None, "איך לנסח את ההמלצות על הגן?")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    core.send_consult(p.approval_id.as_deref().unwrap())
        .unwrap();
    let body = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(body.contains("ומספר עליה בבית"), "{body}");
    // Her named paragraph is not the first line of the joined sections: its hints are moved.
    assert!(
        body.find("ומתעורר בלילה").unwrap() < body.find("ומספר עליה בבית").unwrap(),
        "{body}"
    );
    assert!(!body.contains(ONLY_THE_MODEL), "{body}");
}

/// JOBS-4: every text a request has the model read besides the materials (the current
/// draft, the approved sections a closing section is written from, the paragraphs a check
/// sends, her earlier questions…) can be listed without the model and read while the core is
/// free, so beginning the request does not run the model under the core's lock.
#[test]
fn the_texts_a_request_reads_are_read_by_the_model_before_it_begins() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // Her paragraphs, a proposed one, and a conversation, from before she turned it on.
    core.add_own_paragraph(
        &case,
        "kindergarten",
        &format!("בגן הוא משחק בעיקר עם {ONLY_THE_MODEL} ומספר עליה בבית."),
    )
    .unwrap();
    core.add_own_paragraph(&case, "background", "נרדם באיחור ומתעורר בלילה.")
        .unwrap();
    core.vault_mut()
        .unwrap()
        .add_draft(
            &case,
            "kindergarten",
            "[ילד] נפרד בבכי בבוקר.",
            dv_domain::Author::Ai,
            &[],
        )
        .unwrap();
    let asked = core
        .prepare_consult(Some(&case), None, "איך לנסח את הפרידה בבוקר?")
        .unwrap();
    let conversation = core
        .send_consult(asked.approval_id.as_deref().unwrap())
        .unwrap()
        .conversation_id;
    let reads = model_on(&mut core);
    core.set_verify_stage(true).unwrap();
    // The materials are read first, as the shell does before every request.
    let job = core.begin_name_hints(&case, &[]).unwrap().unwrap();
    core.finish_name_hints(job.run());

    type Begin = Box<dyn Fn(&mut Core) -> Result<RequestJob, CoreError>>;
    let c = case.clone();
    let conv = conversation.clone();
    let requests: Vec<(&str, Begin)> = vec![
        (
            "section",
            Box::new(move |k| k.begin_prepare_section(&c, "kindergarten", "לקצר", None)),
        ),
        ("summary", {
            let c = case.clone();
            Box::new(move |k| k.begin_prepare_section(&c, "summary", "לנסח את הסיכום", None))
        }),
        ("check", {
            let c = case.clone();
            Box::new(move |k| k.begin_prepare_verify(&c, "kindergarten"))
        }),
        ("consult", {
            let c = case.clone();
            Box::new(move |k| k.begin_prepare_consult(Some(&c), Some(&conv), "ומה עם המעברים?"))
        }),
    ];
    for (what, begin) in &requests {
        // Listed under the lock without the model.
        let before = reads.load(Ordering::SeqCst);
        let texts = core.request_name_texts(begin);
        assert_eq!(reads.load(Ordering::SeqCst), before, "{what}: listed only");
        assert!(!texts.is_empty(), "{what}");
        // Read as the shell reads them, while the core is free.
        if let Some(job) = core.begin_name_hints(&case, &texts).unwrap() {
            core.finish_name_hints(job.run());
        }
        // Begun now, the request finds everything read: the model does not run under the lock.
        let read_off_lock = reads.load(Ordering::SeqCst);
        let job = begin(&mut core).unwrap();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            read_off_lock,
            "{what}: the model ran under the lock"
        );
        drop(job);
    }
    // Off, nothing is listed and nothing is begun.
    core.vault_mut()
        .unwrap()
        .set_setting(NAME_MODEL_KEY, "")
        .unwrap();
    assert!(core.request_name_texts(&requests[0].1).is_empty());
}
