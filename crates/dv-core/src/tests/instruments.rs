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
fn readiness_line_waits_for_every_instrument_in_an_open_case() {
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
    // No score table in any open case: nothing waits.
    assert!(tables_line(&mut core));

    core.save_scores(&case, None, &sheet("wppsi_iv", &[("fsiq", 104.0)], ""))
        .unwrap();
    let brief = core
        .save_scores(&case, None, &sheet("brief_p", &[("inhibit", 62.0)], ""))
        .unwrap();
    assert!(!tables_line(&mut core), "both await her approval");
    core.set_instrument_status("wppsi_iv", InstrumentStatus::Approved)
        .unwrap();
    assert!(!tables_line(&mut core), "BRIEF-P still awaits");
    // An instrument she does not give never leaves the computer: it does not hold the line.
    core.set_instrument_status("brief_p", InstrumentStatus::NotUsed)
        .unwrap();
    assert!(tables_line(&mut core));
    // Another instrument awaiting approval, but in no open case, does not matter.
    assert_eq!(status(&mut core, "wisc_v"), InstrumentStatus::Awaiting);

    // A case in the bin is not open.
    core.set_instrument_status("brief_p", InstrumentStatus::Approved)
        .unwrap();
    assert!(tables_line(&mut core));
    let other = core.create_sample_case().unwrap();
    core.save_scores(&other, None, &sheet("wisc_v", &[("fsiq", 99.0)], ""))
        .unwrap();
    assert!(!tables_line(&mut core));
    core.delete_case(&other).unwrap();
    assert!(tables_line(&mut core));
    let _ = brief;
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
