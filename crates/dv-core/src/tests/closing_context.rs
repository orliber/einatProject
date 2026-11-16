//! D-162: the closing sections (DSM, summary, diagnoses, recommendations) are written with the
//! whole picture: the referral first and named as the question, her diagnoses as her decision,
//! the closing sections that are theirs to see, and the key scores worked out on the computer.
//! D-164: each section's request carries the craft of its kind. Fabricated data only; nothing
//! leaves the machine.

use super::*;

/// A sentence of its own in each approved section (fabricated).
const REFERRAL: &str = "ההפניה נעשתה עקב קושי במעברים ובפרידה בבוקר";
const KINDERGARTEN: &str = "בגן משתתף במפגש הבוקר ונעזר בהכנה מראש";
const DIAGNOSES: &str = "עיכוב התפתחותי בתחום השפה";
const DSM: &str = "נצפה קושי בהדדיות חברתית לפי הקריטריון";
const RECOMMENDATIONS: &str = "מומלץ ליווי של קלינאית תקשורת בגן";
const SUMMARY: &str = "ילד סקרן שנעזר בתיווך של מבוגר";
const PRIOR: &str = "באבחון קודם תואר קושי בוויסות";

/// An approved report: two sections of the body, the previous assessments, and every closing
/// section.
fn approved_report(core: &mut Core, case: &str) {
    for (key, text) in [
        ("kindergarten", KINDERGARTEN),
        ("referral", REFERRAL),
        ("prior_assessments", PRIOR),
        ("dsm", DSM),
        ("summary", SUMMARY),
        ("diagnoses", DIAGNOSES),
        ("recommendations", RECOMMENDATIONS),
    ] {
        core.add_own_paragraph(case, key, &format!("{text}."))
            .unwrap();
    }
}

/// What left the machine for a draft of `key`: the text of the request's first message.
fn sent_for(core: &mut Core, fake: &FakeTransport, case: &str, key: &str) -> String {
    *fake.answer.lock().unwrap() = None;
    let prepared = core.prepare_section(case, key, "טיוטה").unwrap();
    let approval = prepared.approval_id.expect("clears the gate");
    // The answer does not matter here; what left the machine does.
    let _ = core.send_section(&approval);
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    let body: Value = serde_json::from_str(&sent).unwrap();
    dv_ai::content_text(&body["messages"][0]["content"])
}

/// `(attributes, text)` of every data block of a request.
fn blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some((_, after)) = rest.split_once("<data nonce=") {
        let Some((head, tail)) = after.split_once(">\n") else {
            break;
        };
        let Some((body, next)) = tail.split_once("\n</data nonce=") else {
            break;
        };
        out.push((head.to_owned(), body.to_owned()));
        rest = next;
    }
    out
}

/// The approved sections a request carries: attributes and text.
fn approved_blocks(text: &str) -> Vec<(String, String)> {
    blocks(text)
        .into_iter()
        .filter(|(head, _)| head.contains(" approved_section=\""))
        .collect()
}

fn wppsi() -> dv_domain::ScoreSheet {
    let entry = |measure: &str, value: f64| dv_domain::ScoreEntry {
        measure: measure.into(),
        value,
        note: String::new(),
    };
    dv_domain::ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![
            entry("fsiq", 104.0),
            entry("vci", 118.0),
            entry("psi", 88.0),
            entry("block_design", 6.0),
        ],
        notes: String::new(),
        basis: None,
    }
}

#[test]
fn the_referral_goes_first_and_is_named_as_the_question() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);

    for key in ["summary", "dsm", "recommendations", "diagnoses"] {
        let sent = sent_for(&mut core, &fake, &case, key);
        let approved = approved_blocks(&sent);
        let (head, body) = approved.first().expect("approved sections went");
        assert!(
            head.ends_with("referral_question=\"true\"") && body.contains(REFERRAL),
            "{key}: the referral is first and marked: {head}"
        );
        assert_eq!(
            approved
                .iter()
                .filter(|(h, _)| h.contains("referral_question"))
                .count(),
            1,
            "{key}"
        );
        // Only the summary is told it must answer it.
        assert_eq!(
            sent.contains(dv_ai::REFERRAL_LINE_SUMMARY),
            key == "summary",
            "{key}"
        );
        assert!(
            key == "summary" || sent.contains(dv_ai::REFERRAL_LINE),
            "{key}"
        );
    }
    // On the review screen too, the referral is the first approved section shown.
    let prepared = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    let first = prepared
        .parts
        .iter()
        .find(|p| p.label.starts_with("סעיף מאושר"))
        .unwrap();
    assert!(first.label.contains("הפניה"), "{}", first.label);

    // A section of the body gets no approved section at all, and no such line.
    let sent = sent_for(&mut core, &fake, &case, "kindergarten");
    assert!(approved_blocks(&sent).is_empty());
    assert!(!sent.contains("referral_question"));
}

#[test]
fn each_closing_section_gets_the_closing_sections_that_are_its_to_see_and_never_itself() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    // A draft still waiting for her in the diagnoses and in the body: never passed on.
    let v = core.vault_mut().unwrap();
    v.add_draft(
        &case,
        "diagnoses",
        "הצעה שלא אושרה לאבחנה.",
        Author::Ai,
        &[],
    )
    .unwrap();
    v.add_draft(
        &case,
        "kindergarten",
        "הצעה שלא אושרה על הגן.",
        Author::Ai,
        &[],
    )
    .unwrap();

    let own = [
        ("dsm", DSM),
        ("summary", SUMMARY),
        ("diagnoses", DIAGNOSES),
        ("recommendations", RECOMMENDATIONS),
    ];
    // Which closing sections each one gets.
    let gets: [(&str, &[&str]); 4] = [
        ("dsm", &[DIAGNOSES]),
        ("summary", &[DSM, DIAGNOSES, RECOMMENDATIONS]),
        ("recommendations", &[DSM, DIAGNOSES]),
        ("diagnoses", &[]),
    ];
    for (key, expected) in gets {
        let sent = sent_for(&mut core, &fake, &case, key);
        let approved = approved_blocks(&sent);
        let all: String = approved
            .iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            all.contains(KINDERGARTEN) && all.contains(REFERRAL),
            "{key}: the body"
        );
        for (other, text) in own {
            assert_eq!(
                all.contains(text),
                expected.contains(&text),
                "{key} and the approved {other}"
            );
        }
        // Her decision is named as hers wherever it goes, and nothing else is.
        for (head, body) in &approved {
            assert_eq!(
                head.contains("psychologist_decision=\"true\""),
                body.contains(DIAGNOSES),
                "{key}: {head}"
            );
        }
        assert_eq!(
            sent.contains(dv_ai::DECISION_LINE),
            expected.contains(&DIAGNOSES),
            "{key}"
        );
        // D-156 holds: the previous assessments reach none of them.
        assert!(
            !sent.contains(PRIOR),
            "the previous assessments reached {key}"
        );
        // A proposal still waiting in another section is never passed on. (The section's own
        // waiting draft goes as its current draft, shown to her as such.)
        assert!(!all.contains("הצעה שלא אושרה"), "{key}");
        assert!(!sent.contains("הצעה שלא אושרה על הגן"), "{key}");
    }
}

#[test]
fn her_diagnoses_alone_are_not_enough_to_write_a_closing_section_from() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(&case, "diagnoses", &format!("{DIAGNOSES}."))
        .unwrap();
    for key in ["summary", "recommendations", "dsm"] {
        assert!(
            matches!(
                core.prepare_section(&case, key, "טיוטה"),
                Err(CoreError::Refused(_))
            ),
            "{key}"
        );
    }
    assert!(fake.sent.lock().unwrap().is_empty());
}

#[test]
fn the_key_scores_go_filtered_and_shown_and_a_score_quoted_from_them_is_a_known_number() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.save_scores(&case, None, &wppsi()).unwrap();

    // The summary, the DSM section and the recommendations get them; the diagnoses and a
    // section of the body do not.
    for (key, with) in [
        ("summary", true),
        ("dsm", true),
        ("recommendations", true),
        ("diagnoses", false),
        ("kindergarten", false),
    ] {
        let sent = sent_for(&mut core, &fake, &case, key);
        let scores: Vec<(String, String)> = blocks(&sent)
            .into_iter()
            .filter(|(head, _)| head.contains(" key_scores=\"true\""))
            .collect();
        assert_eq!(scores.len(), usize::from(with), "{key}");
        assert_eq!(sent.contains(dv_ai::KEY_SCORES_LINE), with, "{key}");
        let Some((_, block)) = scores.first() else {
            continue;
        };
        // The lines of the score material, word for word; no subtest.
        assert!(
            block.contains("- מנת משכל כללית (FSIQ): ציון 104, אחוזון 61 – ממוצע"),
            "{block}"
        );
        assert!(
            block.contains("- הבנה מילולית (VCI): ציון 118, אחוזון 88 – ממוצע גבוה"),
            "{block}"
        );
        assert!(!block.contains("קוביות"), "a subtest: {block}");
        // The patterns of the scores, worked out in code.
        assert!(
            block.contains("דפוסי התוצאות") && block.contains("המדד הנמוך ביותר: מהירות עיבוד"),
            "{block}"
        );
    }

    // Shown on the review screen under its own label, like everything that goes out.
    let prepared = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    let part = prepared
        .parts
        .iter()
        .find(|p| p.label == crate::prepare::KEY_SCORES_LABEL)
        .expect("the key scores are on the review screen");
    let shown: String = part.outgoing.iter().map(|s| s.text.as_str()).collect();
    assert!(shown.contains("ציון 104"), "{shown}");

    // A score quoted from the block is not flagged; a number in nothing that went is.
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [
            {"text": "התפקוד הכללי בטווח הממוצע (104), עם יכולת מילולית גבוהה (118).", "source_refs": [], "guide": 0},
            {"text": "בזיכרון העבודה התקבל ציון 143.", "source_refs": [], "guide": 0},
        ],
        "questions": [], "missing": [], "contradictions": [],
    })));
    let reply = core.send_section(&prepared.approval_id.unwrap()).unwrap();
    let numbers = |w: &[String]| -> Vec<String> {
        w.iter().filter(|x| x.contains("המספר")).cloned().collect()
    };
    assert!(
        numbers(&reply.paragraphs[0].warnings).is_empty(),
        "{:?}",
        reply.paragraphs[0].warnings
    );
    assert!(
        numbers(&reply.paragraphs[1].warnings)
            .iter()
            .any(|w| w.contains("143")),
        "{:?}",
        reply.paragraphs[1].warnings
    );
    // The same after a reload (D-150): the local check knows the key scores too.
    let shown: Vec<ParagraphView> = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "summary")
        .unwrap()
        .paragraphs;
    let quoted = shown.iter().find(|p| p.text.contains("(104)")).unwrap();
    assert!(
        numbers(&quoted.warnings).is_empty(),
        "{:?}",
        quoted.warnings
    );
    let invented = shown.iter().find(|p| p.text.contains("143")).unwrap();
    assert!(
        numbers(&invented.warnings)
            .iter()
            .any(|w| w.contains("143")),
        "{:?}",
        invented.warnings
    );
}

#[test]
fn a_case_with_no_scores_gets_no_key_scores_block() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    let sent = sent_for(&mut core, &fake, &case, "summary");
    assert!(!sent.contains("key_scores") && !sent.contains(dv_ai::KEY_SCORES_LINE));
    let prepared = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    assert!(prepared
        .parts
        .iter()
        .all(|p| p.label != crate::prepare::KEY_SCORES_LABEL));
}

#[test]
fn her_own_section_from_her_points_gets_nothing_of_the_closing_context() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.save_scores(&case, None, &wppsi()).unwrap();
    let p = core
        .prepare_from_points(&case, "appearance", "הגיע עם אמא, נפרד בקלות")
        .unwrap();
    let _ = core.send_section(&p.approval_id.unwrap());
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(sent.contains(dv_ai::POINTS_LINE));
    for gone in [
        "approved_section",
        "key_scores",
        "referral_question",
        "psychologist_decision",
        dv_ai::prompts::CRAFT_HEADING,
        REFERRAL,
        DIAGNOSES,
        "ציון 104",
    ] {
        assert!(!sent.contains(gone), "{gone}");
    }
}

/// D-164: the craft of the section's kind goes with its request, before her guide.
#[test]
fn a_sections_request_carries_the_craft_of_its_kind() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    for key in ["kindergarten", "summary", "recommendations"] {
        let sent = sent_for(&mut core, &fake, &case, key);
        let craft = dv_ai::prompts::section_craft(key).unwrap();
        assert!(
            sent.contains(dv_ai::prompts::CRAFT_HEADING) && sent.contains(craft),
            "{key}"
        );
        if let Some(guide) = sent.find("ההנחיה של הפסיכולוגית לסעיף:") {
            assert!(sent.find(craft).unwrap() < guide, "{key}");
        }
    }
}

/// D-169: the whole-report check tells the model which section is which, in fixed words.
#[test]
fn the_report_check_names_the_role_of_each_section_it_is_about() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    approved_report(&mut core, &case);
    core.set_report_check(true).unwrap();
    let prepared = core.prepare_report_check(&case).unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({"notes": [
        {"kind": "referral_unanswered", "sections": ["R1", "R6"], "note": "הסיכום לא מתייחס לפרידה בבוקר"},
        {"kind": "no_recommendation", "sections": ["R2"], "note": "אין המלצה לקושי במעברים"},
    ]})));
    let result = core
        .send_report_check(&prepared.approval_id.unwrap())
        .unwrap();
    let sent = fake.sent.lock().unwrap().last().unwrap().clone();
    let body: Value = serde_json::from_str(&sent).unwrap();
    let text = dv_ai::content_text(&body["messages"][0]["content"]);
    let role_of = |words: &str| -> Option<String> {
        let (head, _) = blocks(&text).into_iter().find(|(_, b)| b.contains(words))?;
        let (_, rest) = head.split_once(" role=\"")?;
        Some(rest.chars().take_while(|c| *c != '"').collect())
    };
    assert_eq!(role_of(REFERRAL).as_deref(), Some("referral"));
    assert_eq!(role_of(SUMMARY).as_deref(), Some("summary"));
    assert_eq!(role_of(DIAGNOSES).as_deref(), Some("diagnoses"));
    assert_eq!(role_of(RECOMMENDATIONS).as_deref(), Some("recommendations"));
    assert_eq!(role_of(DSM).as_deref(), Some("dsm"));
    assert_eq!(
        role_of(KINDERGARTEN),
        None,
        "a section of the body has none"
    );
    // The new kinds come back as themselves, pointing at the sections by key.
    assert_eq!(
        result.notes[0].kind,
        dv_ai::ReportNoteKind::ReferralUnanswered
    );
    assert_eq!(result.notes[0].sections.first().unwrap(), "referral");
    assert_eq!(
        result.notes[1].kind,
        dv_ai::ReportNoteKind::NoRecommendation
    );
}

/// The child's name (declared in the case by `setup`) planted in every new field that goes out
/// with a closing section: the approved referral (the referral-question block), her approved
/// diagnoses (the decision block), a note on a score (the key scores block). It never leaves
/// the machine; its tag does. 0 leaks is the merge condition.
#[test]
fn a_name_in_the_referral_the_diagnoses_or_a_score_note_never_leaves_with_a_closing_section() {
    const NAME: &str = "אלון";
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    core.add_own_paragraph(&case, "kindergarten", &format!("{KINDERGARTEN}."))
        .unwrap();
    core.add_own_paragraph(
        &case,
        "referral",
        &format!("ההורים של {NAME} פנו עקב קושי במעברים."),
    )
    .unwrap();
    core.add_own_paragraph(&case, "diagnoses", &format!("אצל {NAME} נקבע {DIAGNOSES}."))
        .unwrap();
    core.add_own_paragraph(&case, "dsm", &format!("{DSM}, כפי שנצפה אצל {NAME}."))
        .unwrap();
    core.add_own_paragraph(
        &case,
        "recommendations",
        &format!("{RECOMMENDATIONS} עבור {NAME}."),
    )
    .unwrap();
    let mut sheet = wppsi();
    if let Some(e) = sheet.entries.first_mut() {
        e.note = format!("{NAME} שיתף פעולה לאורך כל המבחן");
    }
    core.save_scores(&case, None, &sheet).unwrap();

    for key in ["summary", "dsm", "recommendations"] {
        let sent = sent_for(&mut core, &fake, &case, key);
        let raw = fake.sent.lock().unwrap().last().unwrap().clone();
        assert!(!raw.contains(NAME), "{key}: the name left the machine");
        let approved = approved_blocks(&sent);
        let referral = approved
            .iter()
            .find(|(h, _)| h.contains("referral_question=\"true\""))
            .expect("the referral block");
        assert!(
            referral.1.contains("[ילד]") && referral.1.contains("פנו עקב קושי"),
            "{key}: {}",
            referral.1
        );
        let decision = approved
            .iter()
            .find(|(h, _)| h.contains("psychologist_decision=\"true\""))
            .expect("the decision block");
        assert!(decision.1.contains("[ילד]"), "{key}: {}", decision.1);
        let (_, scores) = blocks(&sent)
            .into_iter()
            .find(|(h, _)| h.contains(" key_scores=\"true\""))
            .expect("the key scores block");
        assert!(
            scores.contains("[ילד] שיתף פעולה"),
            "{key}: the score note went tagged: {scores}"
        );
        if key == "summary" {
            for (head, body) in &approved {
                if body.contains(DSM) || body.contains(RECOMMENDATIONS) {
                    assert!(body.contains("[ילד]"), "{head}");
                }
            }
        }
        // The craft and the lines that name the blocks are fixed text, never the case's.
        assert!(sent.contains(dv_ai::prompts::CRAFT_HEADING), "{key}");
    }
    // The review screen shows the key scores with the name already hidden.
    let prepared = core.prepare_section(&case, "summary", "טיוטה").unwrap();
    let part = prepared
        .parts
        .iter()
        .find(|p| p.label == crate::prepare::KEY_SCORES_LABEL)
        .unwrap();
    let outgoing: String = part.outgoing.iter().map(|s| s.text.as_str()).collect();
    assert!(!outgoing.contains(NAME), "{outgoing}");

    // The whole-report check: its role attributes go with the sections, the name does not.
    // What the fake answers above proposed is turned down: the check reads approved text only.
    let proposed: Vec<String> = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .flat_map(|s| s.paragraphs)
        .filter(|p| p.status == DraftStatus::Proposed)
        .map(|p| p.id)
        .collect();
    for id in proposed {
        core.reject_paragraph(&case, &id).unwrap();
    }
    core.add_own_paragraph(&case, "summary", &format!("{NAME} {SUMMARY}."))
        .unwrap();
    core.set_report_check(true).unwrap();
    let prepared = core.prepare_report_check(&case).unwrap();
    let _ = core.send_report_check(&prepared.approval_id.unwrap());
    let raw = fake.sent.lock().unwrap().last().unwrap().clone();
    assert!(raw.contains("role=\\\"referral\\\""), "the roles went");
    assert!(raw.contains("role=\\\"summary\\\""));
    assert!(!raw.contains(NAME), "the name left with the report check");
    assert!(raw.contains("[ילד]"));
}
