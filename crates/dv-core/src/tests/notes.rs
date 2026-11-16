//! D-161: no note gets lost. What an answer said beside its paragraphs, what was found in
//! them when it came, and the verdicts of the check against the sources are kept with the
//! case and shown with it; "approve all" holds back every paragraph that has something to
//! look at. D-193: they, and the notes she left as they are, are kept sealed in the vault and
//! come back after a lock or a restore. D-163: the run sends the wish she kept, and a
//! section's request carries only the recent turns of its conversation. Fabricated data only.

use super::*;

fn section(core: &mut Core, case: &str, key: &str) -> SectionView {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == key)
        .unwrap()
}

fn kinds(p: &ParagraphView) -> Vec<NoteKind> {
    p.notes.iter().map(|n| n.kind).collect()
}

/// A draft of three paragraphs: a clean one, one that cites nothing, and one that holds the
/// child's name as plain words. The answer also asks, misses and finds a contradiction.
fn drafted(core: &mut Core, fake: &FakeTransport, case: &str) -> SectionResult {
    let p = core
        .prepare_section(case, "background", "תנסח את הסעיף")
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [
            {"text": "[ילד] נרדם באיחור ומתעורר בלילה.", "source_refs": ["S1"], "guide": 1},
            {"text": "[ילד] אוהב לשחק בחצר עם חברים.", "source_refs": [], "guide": 1},
            {"text": "אלון מתעורר פעמיים בלילה.", "source_refs": ["S1"], "guide": 1},
        ],
        "questions": ["מתי [ילד] התחיל ללכת?"],
        "missing": ["אין מידע על ההיריון והלידה"],
        "contradictions": ["ההורים והגן מתארים את השינה אחרת"],
    })));
    let result = core.send_section(&p.approval_id.unwrap()).unwrap();
    *fake.answer.lock().unwrap() = None;
    result
}

#[test]
fn what_the_answer_said_is_shown_with_the_case_after_the_result_is_dropped() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let result = drafted(&mut core, &fake, &case);
    // The page drops `result`; the case itself now says the same things.
    let s = section(&mut core, &case, "background");
    assert_eq!(s.notes.questions, ["מתי אלון התחיל ללכת?"]);
    assert_eq!(s.notes.questions, result.questions);
    assert_eq!(s.notes.missing, ["אין מידע על ההיריון והלידה"]);
    assert_eq!(s.notes.contradictions, result.contradictions);
    // The guide's lines no paragraph answers are their own list, as the result worded them.
    for line in &s.notes.unanswered {
        assert!(
            result.missing.iter().any(|m| m.ends_with(line.as_str())),
            "{line}"
        );
    }
    assert_eq!(
        result.missing.len(),
        s.notes.unanswered.len() + s.notes.missing.len()
    );

    let clean = s
        .paragraphs
        .iter()
        .find(|p| p.text.contains("נרדם"))
        .unwrap();
    assert!(clean.notes.is_empty(), "{:?}", clean.notes);
    let uncited = s
        .paragraphs
        .iter()
        .find(|p| p.text.contains("בחצר"))
        .unwrap();
    assert_eq!(kinds(uncited), [NoteKind::NoSource]);
    let named = s
        .paragraphs
        .iter()
        .find(|p| p.text.contains("פעמיים"))
        .unwrap();
    assert!(
        kinds(named).contains(&NoteKind::NameInAnswer),
        "{:?}",
        named.notes
    );
    // `warnings` is what it was: the checks made again on every reading.
    assert!(uncited.warnings.is_empty(), "{:?}", uncited.warnings);
    // Another section has nothing to say.
    let other = section(&mut core, &case, "kindergarten");
    assert_eq!(other.notes, SectionNotes::default());
}

#[test]
fn approve_all_holds_back_what_has_a_note_and_her_own_click_still_approves() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &fake, &case);
    let s = section(&mut core, &case, "background");
    let ids: Vec<String> = s.paragraphs.iter().map(|p| p.id.clone()).collect();
    // Only the clean paragraph: one cites nothing, one holds a name.
    assert_eq!(core.approve_clean_paragraphs(&case, &ids).unwrap(), 1);
    let s = section(&mut core, &case, "background");
    let held: Vec<&ParagraphView> = s
        .paragraphs
        .iter()
        .filter(|p| p.status == DraftStatus::Proposed)
        .collect();
    assert_eq!(held.len(), 2);
    assert!(held
        .iter()
        .all(|p| p.notes.iter().any(|n| n.kind != NoteKind::Polish)));
    // Her explicit approval of one paragraph, and of the section, works as before.
    core.approve_paragraph(&case, &held[0].id).unwrap();
    assert_eq!(core.approve_section(&case, "background").unwrap(), 1);
    let s = section(&mut core, &case, "background");
    assert!(s
        .paragraphs
        .iter()
        .all(|p| p.status == DraftStatus::Approved));
    // An approved paragraph keeps its note, for the finish.
    assert!(s
        .paragraphs
        .iter()
        .any(|p| kinds(p).contains(&NoteKind::NoSource)));
}

#[test]
fn a_verdict_of_the_check_is_kept_with_the_paragraph_and_holds_it_back() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &fake, &case);
    core.set_verify_stage(true).unwrap();
    let id = core
        .prepare_verify(&case, "background")
        .unwrap()
        .approval_id
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "results": [
        { "paragraph": "P1", "verdict": "contradicted",
          "notes": [{ "note": "[ילד] מתואר אחרת במקור", "source_refs": ["S1"] }] },
        { "paragraph": "P2", "verdict": "unsupported", "notes": [] },
        { "paragraph": "P3", "verdict": "supported", "notes": [] },
    ]})));
    let result = core.send_verify(&id).unwrap();
    let s = section(&mut core, &case, "background");
    let of = |draft: &str| s.paragraphs.iter().find(|p| p.id == draft).unwrap();
    let first = of(&result.draft_ids[0]);
    assert_eq!(
        first.notes,
        [ParagraphNote {
            kind: NoteKind::VerifyContradicted,
            text: "אלון מתואר אחרת במקור".to_owned()
        }]
    );
    // No words from the check: one plain line.
    let second = of(&result.draft_ids[1]);
    assert!(second
        .notes
        .iter()
        .any(|n| n.kind == NoteKind::VerifyUnsupported && !n.text.is_empty()));
    // The clean paragraph is now held back by its verdict.
    assert_eq!(
        core.approve_clean_paragraphs(&case, &result.draft_ids)
            .unwrap(),
        0
    );

    // Checked again and supported: the verdict goes.
    let id = core
        .prepare_verify(&case, "background")
        .unwrap()
        .approval_id
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "results": [
        { "paragraph": "P1", "verdict": "supported", "notes": [] },
        { "paragraph": "P2", "verdict": "partial",
          "notes": [{ "note": "החצר לא מוזכרת", "source_refs": [] }] },
        { "paragraph": "P3", "verdict": "supported", "notes": [] },
    ]})));
    let result = core.send_verify(&id).unwrap();
    let s = section(&mut core, &case, "background");
    let of = |draft: &str| s.paragraphs.iter().find(|p| p.id == draft).unwrap();
    assert!(of(&result.draft_ids[0]).notes.is_empty());
    assert!(kinds(of(&result.draft_ids[1])).contains(&NoteKind::VerifyPartial));
    assert!(!kinds(of(&result.draft_ids[1])).contains(&NoteKind::VerifyUnsupported));
    assert_eq!(
        core.approve_clean_paragraphs(&case, &result.draft_ids)
            .unwrap(),
        1
    );
}

#[test]
fn the_demo_check_which_reads_nothing_holds_nothing_back() {
    // Without a key the check only looks for numbers and answers "partial" for the rest.
    let (_dir, mut core, case) = setup(None);
    let id = core
        .prepare_section(&case, "kindergarten", "תנסח פסקה על המעברים")
        .unwrap()
        .approval_id
        .unwrap();
    core.send_section(&id).unwrap();
    core.set_verify_stage(true).unwrap();
    let id = core
        .prepare_verify(&case, "kindergarten")
        .unwrap()
        .approval_id
        .unwrap();
    let result = core.send_verify(&id).unwrap();
    assert!(result.demo);
    assert!(result
        .checks
        .iter()
        .any(|c| c.verdict == dv_ai::Verdict::Partial));
    let s = section(&mut core, &case, "kindergarten");
    assert!(s
        .paragraphs
        .iter()
        .all(|p| !kinds(p).contains(&NoteKind::VerifyPartial)));
}

#[test]
fn notes_die_with_the_wording_they_were_about_and_outlive_the_lock() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &fake, &case);
    let s = section(&mut core, &case, "background");
    let uncited = s
        .paragraphs
        .iter()
        .find(|p| kinds(p).contains(&NoteKind::NoSource))
        .unwrap()
        .clone();
    // She edits the paragraph: it is hers now, and the note about the old wording goes.
    core.edit_paragraph(&case, &uncited.id, "אלון אוהב לשחק בחצר הגן.")
        .unwrap();
    let s = section(&mut core, &case, "background");
    let edited = s.paragraphs.iter().find(|p| p.id == uncited.id).unwrap();
    assert!(!kinds(edited).contains(&NoteKind::NoSource));
    // The paragraph with the name is removed: nothing is kept about it.
    let named = s
        .paragraphs
        .iter()
        .find(|p| kinds(p).contains(&NoteKind::NameInAnswer))
        .unwrap()
        .id
        .clone();
    core.reject_paragraph(&case, &named).unwrap();
    core.case_detail(&case).unwrap();
    assert!(core
        .notes
        .get(&case)
        .unwrap()
        .of_paragraph(&named, "אלון מתעורר פעמיים בלילה.")
        .is_empty());

    // The section is drafted again with nothing to ask: its notes are replaced.
    assert!(!section(&mut core, &case, "background")
        .notes
        .questions
        .is_empty());
    let p = core.prepare_section(&case, "background", "שוב").unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{"text": "[ילד] נרדם באיחור.", "source_refs": [], "guide": 1}],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let s = section(&mut core, &case, "background");
    assert!(s.notes.questions.is_empty() && s.notes.missing.is_empty());
    assert!(s
        .paragraphs
        .iter()
        .any(|p| kinds(p).contains(&NoteKind::NoSource)));

    // The lock wipes the session's copy; the vault's comes back as it was (D-193).
    let before = section(&mut core, &case, "background");
    core.lock();
    assert!(core.notes.get(&case).is_none());
    core.unlock(PASSWORD).unwrap();
    let after = section(&mut core, &case, "background");
    assert_eq!(after, before);
    assert!(after
        .paragraphs
        .iter()
        .any(|p| kinds(p).contains(&NoteKind::NoSource)));
}

#[test]
fn a_rewording_of_one_paragraph_carries_its_notes_to_the_paragraph_it_became() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &fake, &case);
    let s = section(&mut core, &case, "background");
    let clean = s.paragraphs.iter().find(|p| p.notes.is_empty()).unwrap();
    let p = core
        .prepare_section_replacing(&case, "background", "לקצר", Some(&clean.id))
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "קיצרתי",
        "paragraphs": [{"text": "[ילד] נרדם מאוחר.", "source_refs": [], "guide": 1}],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let after = section(&mut core, &case, "background");
    let reworded = after.paragraphs.iter().find(|p| p.id == clean.id).unwrap();
    assert_eq!(reworded.text, "אלון נרדם מאוחר.");
    assert_eq!(kinds(reworded), [NoteKind::NoSource]);
    // The section's own notes stay: a rewording that asks nothing replaces nothing.
    assert_eq!(after.notes, s.notes);
}

#[test]
fn every_warning_is_a_note_with_its_kind_and_the_warnings_are_unchanged() {
    let (_dir, mut core, case) = setup(None);
    // The child in the details is 5:4 and a boy.
    core.add_own_paragraph(&case, "background", "הילד, בן 8:10, הגיע עם אמו.")
        .unwrap();
    core.add_own_paragraph(&case, "background", "הילד הלך הלך לגן בבוקר.")
        .unwrap();
    let s = section(&mut core, &case, "background");
    let aged = s
        .paragraphs
        .iter()
        .find(|p| p.text.contains("8:10"))
        .unwrap();
    assert_eq!(aged.warnings.len(), 1);
    assert_eq!(
        aged.notes
            .iter()
            .filter(|n| n.kind == NoteKind::Age)
            .map(|n| n.text.as_str())
            .collect::<Vec<_>>(),
        aged.warnings.iter().map(String::as_str).collect::<Vec<_>>()
    );
    // Proofreading is a note of its own kind, and the old field still holds it.
    let rough = s
        .paragraphs
        .iter()
        .find(|p| p.text.contains("הלך הלך"))
        .unwrap();
    assert!(rough.warnings.is_empty());
    assert!(!rough.polish.is_empty());
    assert_eq!(
        rough
            .notes
            .iter()
            .filter(|n| n.kind == NoteKind::Polish)
            .map(|n| n.text.clone())
            .collect::<Vec<_>>(),
        rough.polish
    );
    assert!(rough.notes.iter().all(|n| n.kind == NoteKind::Polish));
    // Over IPC the kinds are fixed snake_case words.
    assert_eq!(
        serde_json::to_value(NoteKind::VerifyContradicted).unwrap(),
        json!("verify_contradicted")
    );
    assert_eq!(
        serde_json::to_value(NoteKind::NumberNotInSources).unwrap(),
        json!("number_not_in_sources")
    );
}

/// What the request of `key` says the psychologist asks, as it goes out.
fn asked(prepared: &Prepared) -> String {
    prepared
        .parts
        .iter()
        .find(|p| p.label == "הבקשה שלך")
        .map(|p| p.outgoing.iter().map(|s| s.text.as_str()).collect())
        .unwrap_or_default()
}

#[test]
fn the_whole_report_run_sends_the_wish_she_kept_for_a_section() {
    let (_dir, mut core, case) = setup(None);
    core.keep_draft_brief("kindergarten", "להתחיל מהקשר עם הצוות", true)
        .unwrap();
    let run = core.prepare_full_draft(&case).unwrap();
    assert!(
        run.len() >= 2,
        "{:?}",
        run.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );
    let of = |key: &str| asked(&run.iter().find(|(k, _)| k == key).unwrap().1);
    let with_wish = of("kindergarten");
    assert!(
        with_wish.contains(crate::prepare::WISH_LEAD)
            && with_wish.contains("להתחיל מהקשר עם הצוות"),
        "{with_wish}"
    );
    // A section with no kept wish goes with the fixed request alone.
    let plain = of("background");
    assert!(!plain.contains(crate::prepare::WISH_LEAD), "{plain}");
    assert_eq!(plain, crate::prepare::full_draft_instruction(None));
    assert!(with_wish.starts_with(&plain));
    // The same words a draft she asks for herself sends (the report page's `draftRequest`).
    let own = core
        .prepare_section(
            &case,
            "kindergarten",
            "כתבי טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה. מה שחשוב לי שייכתב בסעיף: להתחיל מהקשר עם הצוות",
        )
        .unwrap();
    assert!(asked(&own).ends_with("מה שחשוב לי שייכתב בסעיף: להתחיל מהקשר עם הצוות"));
    assert!(with_wish.ends_with("מה שחשוב לי שייכתב בסעיף: להתחיל מהקשר עם הצוות"));
    // A wish sent once without keeping it is not the section's.
    core.keep_draft_brief("kindergarten", "משהו אחר", false)
        .unwrap();
    assert_eq!(
        crate::prepare::full_draft_instruction(
            core.draft_briefs()
                .unwrap()
                .sections
                .get("kindergarten")
                .map(String::as_str)
        ),
        plain
    );
}

#[test]
fn only_the_recent_turns_of_a_conversation_are_kept_for_the_next_request() {
    use crate::prepare::{recent_turns, HISTORY_TURNS};
    let turn = |i: usize| {
        (
            if i.is_multiple_of(2) {
                ChatRole::User
            } else {
                ChatRole::Assistant
            },
            format!("תור {i}"),
        )
    };
    // A short conversation goes whole.
    let short: Vec<_> = (0..4).map(turn).collect();
    assert_eq!(recent_turns(short.clone()), short);
    // Twelve turns: the last six, beginning with hers.
    let long: Vec<_> = (0..12).map(turn).collect();
    let kept = recent_turns(long.clone());
    assert_eq!(kept.len(), HISTORY_TURNS);
    assert_eq!(kept, long[6..]);
    // An odd conversation never begins with the model's turn.
    let odd: Vec<_> = (0..11).map(turn).collect();
    let kept = recent_turns(odd.clone());
    assert_eq!(kept, odd[6..]);
    assert_eq!(kept[0].0, ChatRole::User);
}

#[test]
fn a_section_request_after_twelve_turns_sends_six_and_is_smaller() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    // Six exchanges on one section: twelve turns in its conversation.
    for i in 0..6 {
        let p = core
            .prepare_section(
                &case,
                "kindergarten",
                &format!("בקשה מספר {i}: לנסח את הסעיף על המעברים בגן בצורה מפורטת יותר"),
            )
            .unwrap();
        *fake.answer.lock().unwrap() = Some(api_json(&json!({
            "reply": format!("תשובה מספר {i}: ניסחתי את הסעיף מחדש לפי הבקשה, עם דגש על המעברים בין הפעילויות ועל הפרידה בבוקר."),
            "paragraphs": [{"text": "[ילד] מתקשה במעברים בין פעילויות.", "source_refs": ["S1"], "guide": 1}],
            "questions": [], "missing": [], "contradictions": [],
        })));
        core.send_section(&p.approval_id.unwrap()).unwrap();
    }
    let stored = core
        .vault_ref()
        .unwrap()
        .messages(&case, "kindergarten")
        .unwrap();
    assert_eq!(stored.len(), 12, "the vault keeps the whole conversation");

    let p = core
        .prepare_section(&case, "kindergarten", "ועוד פעם")
        .unwrap();
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    let body: Value = serde_json::from_str(&sent).unwrap();
    let messages = body["messages"].as_array().unwrap();
    // Six turns of the conversation and the request itself.
    assert_eq!(messages.len(), crate::prepare::HISTORY_TURNS + 1);
    assert_eq!(messages[0]["role"], "user");
    let text = body["messages"].to_string();
    assert!(text.contains("בקשה מספר 3") && text.contains("תשובה מספר 5"));
    assert!(!text.contains("בקשה מספר 2") && !text.contains("תשובה מספר 2"));

    // The size of the request, and what it was when all twelve turns went out again: the six
    // older turns as they would have been written into `messages`.
    let after = sent.len();
    let dropped: usize = stored[..6]
        .iter()
        .map(|m| {
            let role = if m.role == ChatRole::User {
                "user"
            } else {
                "assistant"
            };
            json!({ "role": role, "content": m.text_tagged })
                .to_string()
                .len()
                + 1
        })
        .sum();
    let before = after + dropped;
    println!("request size with 12 turns of history: before {before} bytes, after {after} bytes");
    assert!(after < before);
}

/// The notes are kept by the stored wording, not by the text with the names put back: when
/// she corrects the spelling of the child's name, or her own name in the settings, what the
/// answer and the check said about the paragraph stays, and "approve all" still holds it back.
#[test]
fn correcting_a_name_keeps_the_notes_that_hold_a_paragraph_back() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &fake, &case);
    core.set_verify_stage(true).unwrap();
    let id = core
        .prepare_verify(&case, "background")
        .unwrap()
        .approval_id
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "results": [
        { "paragraph": "P1", "verdict": "contradicted", "notes": [] },
        { "paragraph": "P2", "verdict": "supported", "notes": [] },
        { "paragraph": "P3", "verdict": "supported", "notes": [] },
    ]})));
    let result = core.send_verify(&id).unwrap();
    *fake.answer.lock().unwrap() = None;

    // The same people, the child's name spelled again, and her own name changed.
    let people: Vec<IdentityInput> = core
        .vault_ref()
        .unwrap()
        .identities(&case)
        .unwrap()
        .into_iter()
        .map(|i| IdentityInput {
            id: Some(i.id),
            role: i.role,
            value: if i.role == Role::Child {
                "אילון".to_owned()
            } else {
                i.value
            },
            aliases: i.aliases,
        })
        .collect();
    core.set_identities(&case, people).unwrap();
    core.set_practitioner(vec!["ד\"ר רותם אלמוג-בדויה".into()])
        .unwrap();

    let s = section(&mut core, &case, "background");
    let of = |draft: &str| s.paragraphs.iter().find(|p| p.id == draft).unwrap();
    let checked = of(&result.draft_ids[0]);
    assert!(checked.text.contains("אילון"), "{}", checked.text);
    assert!(
        kinds(checked).contains(&NoteKind::VerifyContradicted),
        "{:?}",
        checked.notes
    );
    let uncited = s
        .paragraphs
        .iter()
        .find(|p| p.text.contains("בחצר"))
        .unwrap();
    assert!(
        kinds(uncited).contains(&NoteKind::NoSource),
        "{:?}",
        uncited.notes
    );
    let ids: Vec<String> = s.paragraphs.iter().map(|p| p.id.clone()).collect();
    assert_eq!(core.approve_clean_paragraphs(&case, &ids).unwrap(), 0);
    let s = section(&mut core, &case, "background");
    assert!(s
        .paragraphs
        .iter()
        .all(|p| p.status == DraftStatus::Proposed));
}

/// A verdict is about the wording that was sent. When a paragraph is worded again while an
/// older check is still out, that older answer neither clears nor sets the verdict of the new
/// wording: a "supported" about the old wording must not let the new one through approve-all.
#[test]
fn an_older_check_answering_late_says_nothing_about_a_new_wording() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    drafted(&mut core, &fake, &case);
    core.set_verify_stage(true).unwrap();
    let first = section(&mut core, &case, "background")
        .paragraphs
        .into_iter()
        .find(|p| p.text.contains("נרדם"))
        .unwrap()
        .id;
    // The check of the first wording goes out and has not answered yet.
    let older = core
        .prepare_verify(&case, "background")
        .unwrap()
        .approval_id
        .unwrap();

    // Meanwhile the paragraph is worded again, in place.
    let reword = core
        .prepare_section_replacing(&case, "background", "לנסח מחדש", Some(&first))
        .unwrap()
        .approval_id
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [
            {"text": "[ילד] נרדם מאוחר בערב ומתעורר בלילה.", "source_refs": ["S1"], "guide": 1},
        ],
        "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&reword).unwrap();
    // The new wording is checked, and contradicts the sources.
    let newer = core
        .prepare_verify(&case, "background")
        .unwrap()
        .approval_id
        .unwrap();
    let all = |verdict: &str| {
        api_json(&json!({ "results": [
            { "paragraph": "P1", "verdict": verdict, "notes": [] },
            { "paragraph": "P2", "verdict": verdict, "notes": [] },
            { "paragraph": "P3", "verdict": verdict, "notes": [] },
        ]}))
    };
    *fake.answer.lock().unwrap() = Some(all("contradicted"));
    core.send_verify(&newer).unwrap();

    // The check of the old wording answers last: "supported".
    *fake.answer.lock().unwrap() = Some(all("supported"));
    let late = core.send_verify(&older).unwrap();
    *fake.answer.lock().unwrap() = None;
    let at = late.draft_ids.iter().position(|d| *d == first).unwrap();
    assert_eq!(late.checks[at].verdict, dv_ai::Verdict::Unchecked);

    let s = section(&mut core, &case, "background");
    let p = s.paragraphs.iter().find(|p| p.id == first).unwrap();
    assert!(p.text.contains("מאוחר בערב"), "{}", p.text);
    assert!(
        kinds(p).contains(&NoteKind::VerifyContradicted),
        "{:?}",
        p.notes
    );
    assert_eq!(
        core.approve_clean_paragraphs(&case, std::slice::from_ref(&first))
            .unwrap(),
        0
    );
}

// ------------------------------------------------------------ D-193: kept in the vault

/// What the finish page calls a note it shows about the whole report (one it may leave).
const REPORT_NOTE: &str = r#"report:["repetition",["background"],"אותו משפט בשני מקומות."]"#;

/// The drafted case with a verdict of the check against the sources, and two notes she left
/// on the finish page: one about the paragraph that cites nothing, and one about the whole
/// report. Returns that paragraph's id and the page's key for its note.
fn worked_on(core: &mut Core, fake: &FakeTransport, case: &str) -> (String, String) {
    drafted(core, fake, case);
    core.set_verify_stage(true).unwrap();
    let id = core
        .prepare_verify(case, "background")
        .unwrap()
        .approval_id
        .unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({ "results": [
        { "paragraph": "P1", "verdict": "contradicted",
          "notes": [{ "note": "[ילד] מתואר אחרת במקור", "source_refs": ["S1"] }] },
        { "paragraph": "P2", "verdict": "supported", "notes": [] },
        { "paragraph": "P3", "verdict": "supported", "notes": [] },
    ]})));
    core.send_verify(&id).unwrap();
    *fake.answer.lock().unwrap() = None;
    let uncited = section(core, case, "background")
        .paragraphs
        .into_iter()
        .find(|p| kinds(p).contains(&NoteKind::NoSource))
        .unwrap();
    // As the finish page calls it: the paragraph, its words as she sees them, the note.
    let note = uncited
        .notes
        .iter()
        .find(|n| n.kind == NoteKind::NoSource)
        .unwrap();
    let key = json!([uncited.id, uncited.text, "no_source", note.text]).to_string();
    core.set_note_left(case, Some(&uncited.id), &key, true)
        .unwrap();
    core.set_note_left(case, None, REPORT_NOTE, true).unwrap();
    (uncited.id, key)
}

#[test]
fn the_notes_and_the_ones_she_left_are_back_after_the_lock_exactly_as_they_were() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let (_, key) = worked_on(&mut core, &fake, &case);
    let asked = [key.clone(), REPORT_NOTE.to_owned(), "another".to_owned()];
    let before = core.case_detail(&case).unwrap();
    let background = before
        .sections
        .iter()
        .find(|s| s.key == "background")
        .unwrap();
    assert!(!background.notes.questions.is_empty());
    assert!(background
        .paragraphs
        .iter()
        .any(|p| kinds(p).contains(&NoteKind::VerifyContradicted)));
    assert_eq!(
        core.left_notes(&case, &asked).unwrap(),
        [key.clone(), REPORT_NOTE.to_owned()]
    );

    core.lock();
    assert!(
        core.notes.get(&case).is_none(),
        "the session's copy is wiped"
    );
    assert!(matches!(
        core.left_notes(&case, &asked),
        Err(CoreError::Locked)
    ));
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.case_detail(&case).unwrap(), before);
    assert_eq!(
        core.left_notes(&case, &asked).unwrap(),
        [key.clone(), REPORT_NOTE.to_owned()]
    );
    // "Approve all" still holds back every paragraph with something to look at.
    let ids: Vec<String> = background.paragraphs.iter().map(|p| p.id.clone()).collect();
    assert_eq!(core.approve_clean_paragraphs(&case, &ids).unwrap(), 0);
    // Brought back, it stays back after the next lock too.
    core.set_note_left(&case, None, REPORT_NOTE, false).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert_eq!(core.left_notes(&case, &asked).unwrap(), [key]);
}

#[test]
fn an_edit_after_the_lock_still_drops_the_note_and_what_she_left_about_it() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let (uncited, key) = worked_on(&mut core, &fake, &case);
    core.lock();
    core.unlock(PASSWORD).unwrap();
    core.edit_paragraph(&case, &uncited, "אלון אוהב לשחק בחצר הגן.")
        .unwrap();
    let edited = |core: &mut Core| {
        section(core, &case, "background")
            .paragraphs
            .into_iter()
            .find(|p| p.id == uncited)
            .unwrap()
    };
    assert!(!kinds(&edited(&mut core)).contains(&NoteKind::NoSource));
    assert!(core
        .left_notes(&case, std::slice::from_ref(&key))
        .unwrap()
        .is_empty());
    // Gone from the vault too: the next opening does not bring them back.
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(!kinds(&edited(&mut core)).contains(&NoteKind::NoSource));
    assert!(core.left_notes(&case, &[key]).unwrap().is_empty());
    // What she left about the whole report is about no paragraph, and stays.
    assert_eq!(
        core.left_notes(&case, &[REPORT_NOTE.to_owned()]).unwrap(),
        [REPORT_NOTE]
    );
}

#[test]
fn the_notes_and_the_ones_she_left_go_with_the_backup() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let (_, key) = worked_on(&mut core, &fake, &case);
    let before = core.case_detail(&case).unwrap();
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other
        .stage_backup(std::fs::read(&done.path).unwrap(), "b.vaultbak".into())
        .unwrap();
    other.restore_staged_backup(Some(PASSWORD), None).unwrap();
    assert_eq!(other.case_detail(&case).unwrap(), before);
    assert_eq!(
        other
            .left_notes(&case, &[key.clone(), REPORT_NOTE.to_owned()])
            .unwrap(),
        [key, REPORT_NOTE.to_owned()]
    );
}

#[test]
fn erasing_a_case_erases_its_notes_and_so_does_the_bin() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    worked_on(&mut core, &fake, &case);
    let rows = |core: &mut Core| {
        core.vault_ref()
            .unwrap()
            .case_notes_rows_for_tests()
            .unwrap()
    };
    assert_eq!(rows(&mut core), 1);
    // In the recycle bin the case can still come back, and its notes with it.
    core.delete_case(&case).unwrap();
    assert_eq!(rows(&mut core), 1);
    core.purge_case(&case, PASSWORD).unwrap();
    assert_eq!(rows(&mut core), 0);
    assert!(core.notes.get(&case).is_none());

    // The bin's own erase, thirty days on and after a backup.
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0193".into(),
                ..CaseMeta::default()
            },
            vec![],
        )
        .unwrap();
    core.set_note_left(&other, None, REPORT_NOTE, true).unwrap();
    assert_eq!(rows(&mut core), 1);
    core.delete_case(&other).unwrap();
    core.vault_mut()
        .unwrap()
        .set_deleted_at_for_tests(&other, 1)
        .unwrap();
    let drive = tempfile::tempdir().unwrap();
    core.write_backup(&drive.path().join("b")).unwrap();
    core.lock();
    core.unlock(PASSWORD).unwrap();
    assert!(core.list_trash().unwrap().is_empty());
    assert_eq!(rows(&mut core), 0);
}
