//! Round 59 (D-098): her approval of the score tables. Invented scores only.

use super::*;
use crate::InstrumentStatus;
use dv_domain::{ScoreEntry, ScoreSheet};

/// A note no other text in the case has: wherever it shows up, that sheet went along.
const BRIEF_MARK: &str = "הערה-מבחן-שלא-מועבר";

fn sheet(instrument: &str, entries: &[(&str, f64)], note: &str) -> ScoreSheet {
    ScoreSheet {
        instrument: instrument.into(),
        module: String::new(),
        cutoff: None,
        entries: entries
            .iter()
            .map(|(m, v)| ScoreEntry {
                measure: (*m).into(),
                value: *v,
                note: note.into(),
            })
            .collect(),
        notes: String::new(),
    }
}

fn status(core: &mut Core, key: &str) -> InstrumentStatus {
    core.instrument_approvals()
        .unwrap()
        .into_iter()
        .find(|a| a.instrument.key == key)
        .unwrap()
        .status
}

fn tables_line(core: &mut Core) -> bool {
    core.readiness()
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.key == "score_tables")
        .unwrap()
        .done
}

#[test]
fn every_instrument_awaits_her_approval_until_she_answers() {
    let (_dir, mut core, case) = setup(None);
    let all = core.instrument_approvals().unwrap();
    assert_eq!(all.len(), dv_domain::instruments().len());
    assert!(all
        .iter()
        .all(|a| a.status == InstrumentStatus::Awaiting && a.open_cases == 0));
    // The plan's list is there (WPPSI-IV with animal coding among them).
    for key in [
        "wppsi_iv",
        "wisc_v",
        "wppsi_iii",
        "brief_p",
        "vineland_3",
        "mullen",
    ] {
        assert!(all.iter().any(|a| a.instrument.key == key), "{key}");
    }
    assert!(all
        .iter()
        .find(|a| a.instrument.key == "wppsi_iv")
        .unwrap()
        .instrument
        .measures
        .iter()
        .any(|m| m.key == "animal_coding"));
    // Awaiting is still offered in the score table and still works (invented cases).
    assert_eq!(core.score_instruments().unwrap().len(), all.len());
    core.save_scores(&case, None, &sheet("wppsi_iv", &[("fsiq", 104.0)], ""))
        .unwrap();
    let wppsi = core
        .instrument_approvals()
        .unwrap()
        .into_iter()
        .find(|a| a.instrument.key == "wppsi_iv")
        .unwrap();
    assert_eq!(wppsi.open_cases, 1);

    // Only her two answers are stored; nothing else, and no unknown instrument.
    assert!(core
        .set_instrument_status("wppsi_iv", InstrumentStatus::Awaiting)
        .is_err());
    assert!(core
        .set_instrument_status("wppsi_v", InstrumentStatus::Approved)
        .is_err());
    core.set_instrument_status("wppsi_iv", InstrumentStatus::Approved)
        .unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(status(&mut core, "wppsi_iv"), InstrumentStatus::Approved);
    assert_eq!(status(&mut core, "wisc_v"), InstrumentStatus::Awaiting);

    // A value this program did not write reads as "not given", the stricter answer.
    core.vault_mut()
        .unwrap()
        .set_setting("instrument/wisc_v", "yes")
        .unwrap();
    assert_eq!(status(&mut core, "wisc_v"), InstrumentStatus::NotUsed);
}

#[test]
fn readiness_line_waits_for_her_answer_on_every_instrument() {
    let (_dir, mut core, case) = setup(None);
    // Program-checked now; she cannot tick it herself.
    let item = core
        .readiness()
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.key == "score_tables")
        .unwrap();
    assert!(item.checked_by_program);
    assert!(core.confirm_readiness("score_tables", true).is_err());
    // Fail closed: a new vault, with nothing answered and no table anywhere, is not done.
    assert!(!tables_line(&mut core));

    core.save_scores(&case, None, &sheet("wppsi_iv", &[("fsiq", 104.0)], ""))
        .unwrap();
    let keys: Vec<String> = dv_domain::instruments()
        .into_iter()
        .map(|i| i.key)
        .collect();
    let (last, rest) = keys.split_last().unwrap();
    for key in rest {
        // "Not given" is an answer too: those tables never leave the computer.
        let answer = if key == "wppsi_iv" {
            InstrumentStatus::Approved
        } else {
            InstrumentStatus::NotUsed
        };
        core.set_instrument_status(key, answer).unwrap();
    }
    assert!(!tables_line(&mut core), "{last} still awaits her answer");
    core.set_instrument_status(last, InstrumentStatus::Approved)
        .unwrap();
    assert!(tables_line(&mut core));

    // It does not change as cases come and go.
    let other = core.create_sample_case().unwrap();
    assert!(tables_line(&mut core));
    core.delete_case(&other).unwrap();
    assert!(tables_line(&mut core));
}

#[test]
fn an_instrument_she_does_not_give_is_in_no_request_and_not_offered() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.save_scores(
        &case,
        None,
        &sheet("wppsi_iv", &[("fsiq", 104.0), ("vci", 112.0)], ""),
    )
    .unwrap();
    let brief = core
        .save_scores(
            &case,
            None,
            &sheet("brief_p", &[("inhibit", 71.0), ("shift", 64.0)], BRIEF_MARK),
        )
        .unwrap();
    // Before her answer, the BRIEF-P table would go like any other (prepared, not sent).
    let prepared = core.prepare_section(&case, "cognitive", "").unwrap();
    assert!(serde_json::to_string(&prepared)
        .unwrap()
        .contains(BRIEF_MARK));
    let stale = prepared.approval_id.unwrap();

    core.set_instrument_status("brief_p", InstrumentStatus::NotUsed)
        .unwrap();
    // A request prepared before her answer is not sent any more.
    assert!(core.send_section(&stale).is_err());
    assert!(fake.sent.lock().unwrap().is_empty());

    // Not offered in the score table, and not entered or previewed there.
    let offered = core.score_instruments().unwrap();
    assert!(offered.iter().all(|a| a.instrument.key != "brief_p"));
    assert!(offered.iter().any(|a| a.instrument.key == "wppsi_iv"));
    let again = sheet("brief_p", &[("inhibit", 60.0)], "");
    assert!(matches!(
        core.save_scores(&case, None, &again),
        Err(CoreError::Refused(_))
    ));
    assert!(matches!(
        core.save_scores(&case, Some(&brief.id), &again),
        Err(CoreError::Refused(_))
    ));
    assert!(core.preview_scores(&again).is_err());

    // The saved sheet stays in the case as it was stored, and the screen says why it feeds
    // nothing.
    let detail = core.case_detail(&case).unwrap();
    let kept = detail.inputs.iter().find(|i| i.id == brief.id).unwrap();
    assert!(kept.content.contains(BRIEF_MARK));
    assert!(core.score_sheet(&case, &brief.id).unwrap().is_some());
    let route = detail
        .routing
        .iter()
        .find(|r| r.input_id == brief.id)
        .unwrap();
    assert!(route.feeds.is_empty());
    assert!(!route.needs_sorting);
    assert!(route.left_out.as_deref().unwrap().contains("BRIEF-P"));
    assert!(detail
        .routing
        .iter()
        .filter(|r| r.input_id != brief.id)
        .all(|r| r.left_out.is_none()));

    // Every request about the case: each section with material, and the sorting.
    for (_, p) in core.prepare_full_draft(&case).unwrap() {
        if let Some(id) = p.approval_id {
            core.send_section(&id).unwrap();
        }
    }
    let prepared = core.prepare_section(&case, "cognitive", "").unwrap();
    core.send_section(&prepared.approval_id.unwrap()).unwrap();
    if let Ok(p) = core.prepare_sort(&case) {
        if let Some(id) = p.approval_id {
            core.send_sort(&id).unwrap();
        }
    }
    let sent = fake.sent.lock().unwrap().join("\n");
    assert!(sent.contains("ממוצע גבוה"), "WPPSI-IV still goes: {sent}");
    // The sheet's own lines (the fixed section list names BRIEF-P as an example, so the
    // instrument's name alone proves nothing).
    for word in [BRIEF_MARK, "BRIEF-P –", "(INH)", "(SHF)", "עכבה"] {
        assert!(!sent.contains(word), "{word} went out");
    }

    // Not appended to the report either.
    assert_eq!(core.check_export(&case).unwrap().score_tables, 1);

    // Given again: back in.
    core.set_instrument_status("brief_p", InstrumentStatus::Approved)
        .unwrap();
    assert_eq!(core.check_export(&case).unwrap().score_tables, 2);
    let route = core
        .case_detail(&case)
        .unwrap()
        .routing
        .into_iter()
        .find(|r| r.input_id == brief.id)
        .unwrap();
    assert!(route.left_out.is_none() && !route.feeds.is_empty());
}

#[test]
fn a_saved_comparison_follows_her_answer() {
    let fake = FakeTransport::default();
    let (_dir, mut core, before) = setup(Some(fake.clone()));
    core.save_scores(&before, None, &sheet("wppsi_iv", &[("fsiq", 96.0)], ""))
        .unwrap();
    core.save_scores(&before, None, &sheet("brief_p", &[("inhibit", 71.0)], ""))
        .unwrap();
    let after = core.create_follow_up(&before).unwrap();
    let mut meta = core.case_detail(&after).unwrap().meta;
    meta.consent = Some(consent());
    core.update_case(&after, meta).unwrap();
    core.save_scores(&after, None, &sheet("wppsi_iv", &[("fsiq", 104.0)], ""))
        .unwrap();
    core.save_scores(&after, None, &sheet("brief_p", &[("inhibit", 58.0)], ""))
        .unwrap();
    let saved = core.add_comparison_material(&after).unwrap();
    assert!(saved.content.contains("(INH)"), "{}", saved.content);

    // "Not given": the saved comparison is written again without its lines, so no request
    // carries the old numbers.
    core.set_instrument_status("brief_p", InstrumentStatus::NotUsed)
        .unwrap();
    let comparison = |core: &mut Core| {
        core.case_detail(&after)
            .unwrap()
            .inputs
            .into_iter()
            .find(|i| i.title == "השוואה לאבחון הקודם")
    };
    let rewritten = comparison(&mut core).expect("WPPSI-IV is still compared");
    assert!(
        rewritten.content.contains("96 ← 104"),
        "{}",
        rewritten.content
    );
    assert!(
        !rewritten.content.contains("(INH)"),
        "{}",
        rewritten.content
    );
    let prepared = core.prepare_section(&after, "cognitive", "").unwrap();
    core.send_section(&prepared.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().join("\n");
    assert!(sent.contains("96 ← 104"), "{sent}");
    for word in ["(INH)", "71 ← 58"] {
        assert!(!sent.contains(word), "{word} went out");
    }

    // Nothing left to compare: the material is gone, not left with old numbers.
    core.set_instrument_status("wppsi_iv", InstrumentStatus::NotUsed)
        .unwrap();
    assert!(comparison(&mut core).is_none());
}
