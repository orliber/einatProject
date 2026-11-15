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
