//! D-109: a section's request is filtered away from the core's lock. Each request is read under
//! the lock, filtered and cleared on its own thread while the core answers other commands, and
//! lands under the lock; nothing begun before a lock, a change of names or a cancel ever lands
//! as an approval. Fabricated data only; nothing leaves the machine.

use std::sync::{Arc, Mutex};

use super::*;

fn shared(core: Core) -> Arc<Mutex<Core>> {
    Arc::new(Mutex::new(core))
}

/// The body of an approval waiting to be sent.
fn payload(core: &Core, id: &str) -> String {
    String::from_utf8_lossy(core.pending.get(id).unwrap().payload.body()).into_owned()
}

fn ready(landed: Landed) -> Prepared {
    match landed {
        Landed::Ready(p) => p,
        Landed::Again(job) => panic!("begun again: {job:?}"),
    }
}

/// A case whose materials hold addresses, so every request of it gives numbered tags.
fn dated_case(core: &mut Core, case: &str) {
    core.add_input(
        case,
        InputKind::Kindergarten,
        "סיכום מהגן",
        "הגננת סיפרה שהוא מגיע מרחוב הדקלים 12, ושחברו מהגן גר ברחוב התאנה 7.",
    )
    .unwrap();
    core.add_input(
        case,
        InputKind::Intake,
        "שיחה עם ההורים",
        "ההורים סיפרו שעברו לרחוב הברושים 4, ושהסבתא גרה ברחוב הזית 9.",
    )
    .unwrap();
}

#[test]
fn a_section_is_filtered_while_the_core_answers() {
    let fake = FakeTransport::default();
    let (_dir, core, case) = setup(Some(fake.clone()));
    let core = shared(core);
    let job = core
        .lock()
        .unwrap()
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let worker = std::thread::spawn(move || job.run());
    {
        // The core answers while the request is filtered: the status, the case, her typing.
        let mut c = core.lock().unwrap();
        assert!(c.status().unlocked);
        assert!(c.case_detail(&case).is_ok());
        // Nothing can be sent yet: the approval exists only once the request lands.
        assert!(c.pending.is_empty());
    }
    let ran = worker.join().unwrap();
    let mut c = core.lock().unwrap();
    let p = ready(c.finish_request(ran).unwrap());
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    let id = p.approval_id.unwrap();
    let body = payload(&c, &id);
    assert!(!body.contains("אלון") && !body.contains("שירה"), "{body}");
    c.send_section(&id).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
}

#[test]
fn filtered_away_from_the_lock_or_holding_it_the_request_is_the_same() {
    let (_dir, mut core, case) = setup(None);
    dated_case(&mut core, &case);
    let here = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap();
    let held = payload(&core, here.approval_id.as_deref().unwrap());
    // The same request again, now through the three steps: the same bytes go out (the
    // section's nonce and the numbered tags are kept), so the same approval.
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = std::thread::spawn(move || job.run()).join().unwrap();
    let away = ready(core.finish_request(ran).unwrap());
    assert_eq!(away.approval_id, here.approval_id);
    assert_eq!(payload(&core, away.approval_id.as_deref().unwrap()), held);
    assert_eq!(away.parts.len(), here.parts.len());
}

#[test]
fn two_sections_filtered_at_once_never_give_one_tag_to_two_addresses() {
    let (_dir, mut core, case) = setup(None);
    dated_case(&mut core, &case);
    let all = core.prepare_full_draft(&case).unwrap();
    let keys: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        keys.contains(&"kindergarten") && keys.len() >= 2,
        "{keys:?}"
    );
    for (key, p) in &all {
        assert!(p.blocked.is_empty(), "{key}: {:?}", p.blocked);
        let body = payload(&core, p.approval_id.as_deref().unwrap());
        assert!(
            !body.contains("הדקלים") && !body.contains("הברושים"),
            "{body}"
        );
    }
    // One tag for each address in the case, whichever section gave it first.
    let book = core.vault_ref().unwrap().masks(&case).unwrap();
    for e in &book {
        let same: Vec<_> = book.iter().filter(|o| o.tag == e.tag).collect();
        assert_eq!(same.len(), 1, "{book:?}");
        let words: Vec<_> = book
            .iter()
            .filter(|o| o.kind == e.kind && o.original == e.original)
            .collect();
        assert_eq!(words.len(), 1, "{book:?}");
    }
    assert!(book.len() >= 4, "{book:?}");
}

#[test]
fn two_requests_begun_apart_land_with_one_book() {
    let (_dir, mut core, case) = setup(None);
    dated_case(&mut core, &case);
    // Two requests of one case, read one after the other with the same book and filtered at
    // the same time on two threads, as two clicks would: each gives the next numbers.
    let first = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let second = core
        .begin_prepare_section(&case, "parents_view", "תנסח פסקה", None)
        .unwrap();
    let a = std::thread::spawn(move || first.run());
    let b = std::thread::spawn(move || second.run());
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    let one = ready(core.finish_request(a).unwrap());
    // The second gave the same numbers to other addresses: begun again, never landed as it
    // was.
    let Landed::Again(job) = core.finish_request(b).unwrap() else {
        panic!("a request whose tags another request took landed");
    };
    let two = ready(core.finish_request(job.run()).unwrap());
    assert!(one.approval_id.is_some() && two.approval_id.is_some());
    let book = core.vault_ref().unwrap().masks(&case).unwrap();
    let mut tags: Vec<&str> = book.iter().map(|e| e.tag.as_str()).collect();
    tags.sort_unstable();
    tags.dedup();
    assert_eq!(tags.len(), book.len(), "{book:?}");
}

#[test]
fn a_lock_in_the_middle_lands_nothing() {
    let (_dir, core, case) = setup(None);
    let core = shared(core);
    let job = core
        .lock()
        .unwrap()
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    // Locked before the worker starts: it stops at its first step, with nothing built.
    core.lock().unwrap().lock();
    let ran = std::thread::spawn(move || job.run()).join().unwrap();
    let mut c = core.lock().unwrap();
    assert!(matches!(c.finish_request(ran), Err(CoreError::Locked)));
    // Opened again: the request of the earlier opening still never lands.
    c.unlock(PASSWORD).unwrap();
    let job = c
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    c.lock();
    c.unlock(PASSWORD).unwrap();
    let ran = job.run();
    assert!(matches!(
        c.finish_request(ran),
        Err(CoreError::Needs(_, ErrorAction::Unlock))
    ));
    assert!(c.pending.is_empty());
}

#[test]
fn a_lock_while_the_worker_runs_stops_it() {
    let (_dir, mut core, case) = setup(None);
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    // The lock sets the job's cancel: the run ends at its next step, built or not, and the
    // result is dropped when it comes back.
    core.lock();
    let ran = job.run();
    assert!(ran.built.is_err());
    core.unlock(PASSWORD).unwrap();
    assert!(core.finish_request(ran).is_err());
    assert!(core.pending.is_empty());
}

#[test]
fn a_request_dropped_on_its_way_or_a_worker_that_panicked_leaves_nothing_to_send() {
    let (_dir, mut core, case) = setup(None);
    dated_case(&mut core, &case);
    // Cancelled: the window closed, and the run was never landed.
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    drop(job.run());
    assert!(core.pending.is_empty());
    assert!(core.vault_ref().unwrap().masks(&case).unwrap().is_empty());

    // A worker that panicked: nothing of it lands, on its own or in a batch.
    let mut job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    job.panics = true;
    assert!(std::thread::spawn(move || job.run()).join().is_err());
    let mut jobs = core.begin_full_draft(&case).unwrap();
    assert!(jobs.len() >= 2);
    if let Some((_, j)) = jobs.last_mut() {
        j.panics = true;
    }
    assert!(matches!(run_requests(jobs), Err(CoreError::Internal(_))));
    assert!(core.pending.is_empty());
    assert!(core.vault_ref().unwrap().masks(&case).unwrap().is_empty());
    // And the next request is prepared as usual.
    let p = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap();
    assert!(p.approval_id.is_some());
}

#[test]
fn a_name_added_in_the_middle_prepares_the_request_again() {
    let (_dir, mut core, case) = setup(None);
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "מהגן",
        "בחצר הוא משחק עם שקד, והם בונים יחד מגדל.",
    )
    .unwrap();
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    // While it was filtered she said "שקד" is a name in the case.
    let mut people: Vec<IdentityInput> = core
        .vault_ref()
        .unwrap()
        .identities(&case)
        .unwrap()
        .into_iter()
        .map(|i| IdentityInput {
            id: Some(i.id),
            role: i.role,
            value: i.value,
            aliases: i.aliases,
        })
        .collect();
    people.push(IdentityInput {
        id: None,
        role: Role::OtherChild,
        value: "שקד".into(),
        aliases: vec![],
    });
    core.set_identities(&case, people).unwrap();
    let Landed::Again(job) = core.finish_request(ran).unwrap() else {
        panic!("a request filtered before the new name landed");
    };
    assert!(core.pending.is_empty());
    let p = ready(core.finish_request(job.run()).unwrap());
    let body = payload(&core, p.approval_id.as_deref().unwrap());
    assert!(!body.contains("שקד"), "{body}");
}

#[test]
fn consent_removed_in_the_middle_lands_nothing() {
    let (_dir, mut core, case) = setup(None);
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    let mut meta = core.vault_ref().unwrap().case_meta(&case).unwrap();
    meta.consent = None;
    core.update_case(&case, meta).unwrap();
    assert!(matches!(
        core.finish_request(ran),
        Err(CoreError::ConsentMissing)
    ));
    assert!(core.pending.is_empty());
}

#[test]
fn a_request_whose_ground_keeps_moving_is_filtered_holding_the_core_and_ends() {
    let (_dir, mut core, case) = setup(None);
    let mut job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let mut again = 0;
    let p = loop {
        let ran = job.run();
        // Every time, something changes while it is filtered.
        core.forget_pending(&case);
        match core.finish_request(ran).unwrap() {
            Landed::Ready(p) => break p,
            Landed::Again(next) => {
                again += 1;
                job = *next;
            }
        }
    };
    assert!(p.approval_id.is_some());
    assert_eq!(again, crate::prepare::MAX_TRIES - 1);
}

#[test]
fn a_decision_on_a_word_in_the_middle_prepares_the_request_again() {
    let (_dir, mut core, case) = setup(None);
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    // A decision that drops no approval (a word marked as a name in the case) still changes
    // what the filter decides by: what was filtered before it never lands.
    core.vault_mut()
        .unwrap()
        .mark_is_name(&case, &normalize("שקדיה"))
        .unwrap();
    assert!(matches!(core.finish_request(ran), Ok(Landed::Again(_))));
    assert!(core.pending.is_empty());
}

#[test]
fn the_check_against_the_sources_is_filtered_away_from_the_lock_too() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let p = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let job = core.begin_prepare_verify(&case, "kindergarten").unwrap();
    let ran = std::thread::spawn(move || job.run()).join().unwrap();
    assert!(core.pending.is_empty());
    let p = ready(core.finish_request(ran).unwrap());
    let id = p.approval_id.unwrap();
    let body = payload(&core, &id);
    assert!(!body.contains("אלון") && !body.contains("שירה"), "{body}");
    core.send_verify(&id).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 2);
    // Locked in the middle: nothing lands.
    let job = core.begin_prepare_verify(&case, "kindergarten").unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.finish_request(job.run()).is_err());
    assert!(core.pending.is_empty());
}

/// Two approved sections, so the whole report can be checked.
fn approved_report(core: &mut Core, case: &str) {
    core.add_own_paragraph(
        case,
        "kindergarten",
        "אלון מתקשה במעברים בין פעילויות בגן, ונעזר בהכנה מראש.",
    )
    .unwrap();
    core.add_own_paragraph(case, "background", "ההריון והלידה עברו ללא סיבוכים.")
        .unwrap();
    core.set_report_check(true).unwrap();
}

/// Each kind of request the core prepares besides a section's, begun under the lock.
fn begin_each(core: &mut Core, case: &str) -> Vec<(&'static str, RequestJob)> {
    vec![
        (
            "consult",
            core.begin_prepare_consult(Some(case), None, "איך לנסח לאלון המלצה על מעברים?")
                .unwrap(),
        ),
        (
            "consult without a case",
            core.begin_prepare_consult(None, None, "מה ההבדל בין WPPSI-IV ל-WISC-V?")
                .unwrap(),
        ),
        ("sort", core.begin_prepare_sort(case).unwrap()),
        (
            "report check",
            core.begin_prepare_report_check(case).unwrap(),
        ),
    ]
}

#[test]
fn a_consultation_a_sorting_and_the_report_check_are_filtered_away_from_the_lock_too() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    super::sort_flows::add_home_intake(&mut core, &case);
    approved_report(&mut core, &case);
    let jobs = begin_each(&mut core, &case);
    let core = shared(core);
    let workers: Vec<_> = jobs
        .into_iter()
        .map(|(what, job)| (what, std::thread::spawn(move || job.run())))
        .collect();
    {
        // The core answers while they are filtered, and nothing can be sent yet.
        let mut c = core.lock().unwrap();
        assert!(c.status().unlocked);
        assert!(c.case_detail(&case).is_ok());
        assert!(c.pending.is_empty());
    }
    let mut c = core.lock().unwrap();
    for (what, worker) in workers {
        let p = ready(c.finish_request(worker.join().unwrap()).unwrap());
        assert!(p.blocked.is_empty(), "{what}: {:?}", p.blocked);
        let id = p.approval_id.expect("cleared by the gate");
        let body = payload(&c, &id);
        assert!(
            !body.contains("אלון") && !body.contains("שירה"),
            "{what}: {body}"
        );
    }
    // Each lands as the approval of its own kind, and goes out as one.
    assert_eq!(c.pending.len(), 4);
    let ids: Vec<(String, SendKind)> = c
        .pending
        .iter()
        .map(|(id, p)| (id.clone(), p.kind.send_kind()))
        .collect();
    for (id, kind) in ids {
        match kind {
            SendKind::Consult => drop(c.send_consult(&id).unwrap()),
            SendKind::ReportCheck => drop(c.send_report_check(&id).unwrap()),
            SendKind::Sort => {}
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(fake.sent.lock().unwrap().len(), 3);
}

#[test]
fn a_lock_in_the_middle_lands_no_consultation_sorting_or_report_check() {
    let (_dir, mut core, case) = setup(None);
    super::sort_flows::add_home_intake(&mut core, &case);
    approved_report(&mut core, &case);
    let jobs = begin_each(&mut core, &case);
    core.lock();
    core.unlock(PASSWORD).unwrap();
    for (what, job) in jobs {
        let ran = job.run();
        assert!(core.finish_request(ran).is_err(), "{what}");
    }
    assert!(core.pending.is_empty());
}

#[test]
fn a_name_added_in_the_middle_prepares_a_consultation_without_a_case_again() {
    let (_dir, mut core, case) = setup(None);
    // A question about no case is still filtered with every case's names.
    let job = core
        .begin_prepare_consult(None, None, "מה כדאי לשאול את שקד בפגישה?")
        .unwrap();
    let ran = job.run();
    let mut people: Vec<IdentityInput> = core
        .vault_ref()
        .unwrap()
        .identities(&case)
        .unwrap()
        .into_iter()
        .map(|i| IdentityInput {
            id: Some(i.id),
            role: i.role,
            value: i.value,
            aliases: i.aliases,
        })
        .collect();
    people.push(IdentityInput {
        id: None,
        role: Role::OtherChild,
        value: "שקד".into(),
        aliases: vec![],
    });
    core.set_identities(&case, people).unwrap();
    let Landed::Again(job) = core.finish_request(ran).unwrap() else {
        panic!("a question filtered before the new name landed");
    };
    let p = ready(core.finish_request(job.run()).unwrap());
    let body = payload(&core, p.approval_id.as_deref().unwrap());
    assert!(!body.contains("שקד"), "{body}");
}

#[test]
fn a_case_moved_to_the_recycle_bin_in_the_middle_lands_nothing_and_drops_what_waits() {
    let (_dir, mut core, case) = setup(None);
    // Prepared before: waiting for her to send it.
    let waiting = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה")
        .unwrap();
    assert!(waiting.approval_id.is_some());
    let job = core
        .begin_prepare_section(&case, "parents_view", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    core.delete_case(&case).unwrap();
    assert!(core.pending.is_empty(), "the waiting request was dropped");
    assert!(core.finish_request(ran).is_err());
    assert!(core.pending.is_empty());
}

#[test]
fn a_material_deleted_or_changed_in_the_middle_never_goes_out() {
    let (_dir, mut core, case) = setup(None);
    let kindergarten = core
        .vault_ref()
        .unwrap()
        .inputs(&case)
        .unwrap()
        .into_iter()
        .find(|i| i.kind == InputKind::Kindergarten)
        .unwrap();
    // Changed while it was filtered: prepared again, with the new text.
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    core.update_input(
        &case,
        &kindergarten.id,
        &kindergarten.title,
        "הגננת סיפרה כי הוא מתקשה במעברים.",
    )
    .unwrap();
    let Landed::Again(job) = core.finish_request(ran).unwrap() else {
        panic!("a request with the old text of a material landed");
    };
    let p = ready(core.finish_request(job.run()).unwrap());
    let body = payload(&core, p.approval_id.as_deref().unwrap());
    assert!(
        body.contains("מתקשה במעברים") && !body.contains("בבכי"),
        "{body}"
    );
    // Deleted after it was prepared: the request waiting with it is dropped.
    core.delete_input(&case, &kindergarten.id).unwrap();
    assert!(core.pending.is_empty());
    core.restore_input(&case, &kindergarten.id).unwrap();
    // Deleted while it was filtered: prepared again, without it.
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    core.delete_input(&case, &kindergarten.id).unwrap();
    let Landed::Again(job) = core.finish_request(ran).unwrap() else {
        panic!("a request with a deleted material landed");
    };
    assert!(core.pending.is_empty());
    match core.finish_request(job.run()) {
        Ok(Landed::Ready(p)) => {
            if let Some(id) = p.approval_id {
                let body = payload(&core, &id);
                assert!(!body.contains("מתקשה במעברים"), "{body}");
            }
        }
        Ok(Landed::Again(_)) => panic!("nothing changed the second time"),
        Err(_) => assert!(core.pending.is_empty()),
    }
}

#[test]
fn a_section_she_takes_to_write_herself_in_the_middle_sends_nothing() {
    let (_dir, mut core, case) = setup(None);
    let job = core
        .begin_prepare_section(&case, "kindergarten", "תנסח פסקה", None)
        .unwrap();
    let ran = job.run();
    core.set_section_mode("kindergarten", dv_domain::SectionMode::Own)
        .unwrap();
    assert!(matches!(
        core.finish_request(ran),
        Err(CoreError::Refused(_))
    ));
    assert!(core.pending.is_empty());
}

#[test]
fn sections_begun_again_together_share_one_book_and_land_in_one_more_round() {
    let (_dir, mut core, case) = setup(None);
    dated_case(&mut core, &case);
    let jobs = core.begin_full_draft(&case).unwrap();
    assert!(jobs.len() >= 2);
    let runs = run_requests(jobs).unwrap();
    // Something changed while they were filtered: every one is begun again.
    core.forget_pending(&case);
    let (ready_now, again) = core.finish_requests(runs).unwrap();
    assert!(ready_now.is_empty());
    assert!(again.len() >= 2);
    // Run side by side once more, they share one book: none gives a tag another one gave.
    let (landed, again) = core.finish_requests(run_requests(again).unwrap()).unwrap();
    assert!(again.is_empty(), "begun again once more: {again:?}");
    assert!(landed.iter().all(|(_, p)| p.approval_id.is_some()));
}

#[test]
fn a_batch_whose_workers_all_panicked_ends_with_an_error_not_a_panic() {
    let (_dir, mut core, case) = setup(None);
    dated_case(&mut core, &case);
    let mut jobs = core.begin_full_draft(&case).unwrap();
    assert!(jobs.len() >= 2);
    for (_, j) in &mut jobs {
        j.panics = true;
    }
    assert!(matches!(run_requests(jobs), Err(CoreError::Internal(_))));
    assert!(core.pending.is_empty());
}
