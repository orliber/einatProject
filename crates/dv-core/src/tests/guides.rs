//! How each section is written (round 29): the institute's guide goes out with the request,
//! hers replaces it, a case adds its own lines, and a line no paragraph answers comes back as
//! a missing paragraph. Fabricated data only; nothing leaves the machine. The institute's
//! lines are checked against every child's name with the section titles
//! (`score_tables_and_section_titles_are_never_rewritten_silently`).

use super::*;

fn section(core: &mut Core, case: &str, key: &str) -> SectionView {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == key)
        .unwrap()
}

#[test]
fn the_guide_goes_out_numbered_and_an_unanswered_line_is_a_missing_paragraph() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let shown = section(&mut core, &case, "background");
    assert!(shown.guide_is_default);
    assert!(shown.guide.starts_with("רקע משפחתי:"), "{}", shown.guide);
    assert_eq!(shown.mode, dv_domain::SectionMode::Ai);

    let p = core
        .prepare_section(&case, "background", "תנסח את הסעיף")
        .unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [{
            "text": "[ילד] גדל עם הוריו.",
            "source_refs": ["S1"],
            "guide": 1,
        }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(
        sent[0].contains("ההנחיה של הפסיכולוגית לסעיף:"),
        "{}",
        sent[0]
    );
    assert!(sent[0].contains("1. רקע משפחתי:"));
    assert!(sent[0].contains("2. הריון, לידה"));
    assert!(sent[0].contains(r#""guide":{"type":"integer"}"#));
    assert_eq!(r.missing.len(), 1, "{:?}", r.missing);
    assert!(
        r.missing[0].starts_with("חסרה פסקה לפי ההנחיה: הריון, לידה"),
        "{}",
        r.missing[0]
    );
    assert_eq!(r.paragraphs[0].guide, 1);

    // A second turn on the draft is not checked against the guide again.
    let p = core.prepare_section(&case, "background", "לקצר").unwrap();
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "קוצר",
        "paragraphs": [{ "text": "[ילד] גדל עם הוריו.", "source_refs": ["S1"], "guide": 0 }],
        "questions": [], "missing": [], "contradictions": [],
    })));
    let r = core.send_section(&p.approval_id.unwrap()).unwrap();
    assert!(r.missing.is_empty(), "{:?}", r.missing);
}

#[test]
fn her_guide_replaces_the_institutes_and_a_case_adds_its_own_lines_hidden() {
    let fake = FakeTransport::default();
    let (_dir, mut core, case) = setup(Some(fake.clone()));
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0010".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![],
        )
        .unwrap();
    core.set_section_guide("kindergarten", "קודם הגן הנוכחי.\r\nאחר כך הקודמים.  ")
        .unwrap();
    // The child's name in the case's own line: it goes out as a tag like everything else.
    core.set_case_guide(&case, "kindergarten", "להזכיר את המעבר של אלון באמצע השנה.")
        .unwrap();
    let shown = section(&mut core, &case, "kindergarten");
    assert!(!shown.guide_is_default);
    assert_eq!(shown.guide, "קודם הגן הנוכחי.\nאחר כך הקודמים.");
    assert_eq!(shown.case_guide, "להזכיר את המעבר של אלון באמצע השנה.");
    // Another case has her general guide, not this case's lines.
    let there = section(&mut core, &other, "kindergarten");
    assert_eq!(there.guide, shown.guide);
    assert!(there.case_guide.is_empty());

    // Details saved from the screen keep the case's lines.
    let meta = core.vault_ref().unwrap().case_meta(&case).unwrap();
    core.update_case(
        &case,
        CaseMeta {
            guides: None,
            ..meta
        },
    )
    .unwrap();
    assert!(!section(&mut core, &case, "kindergarten")
        .case_guide
        .is_empty());

    let p = core.prepare_section(&case, "kindergarten", "תנסח").unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    *fake.answer.lock().unwrap() = Some(api_json(&json!({
        "reply": "הנה",
        "paragraphs": [], "questions": [], "missing": [], "contradictions": [],
    })));
    core.send_section(&p.approval_id.unwrap()).unwrap();
    let sent = fake.sent.lock().unwrap().clone();
    assert!(!sent[0].contains("אלון"), "{}", sent[0]);
    assert!(sent[0].contains("1. קודם הגן הנוכחי."));
    assert!(
        sent[0].contains("3. להזכיר את המעבר של [ילד] באמצע השנה."),
        "{}",
        sent[0]
    );
    assert!(!sent[0].contains("המסגרות לפי הסדר"));

    // Back to the institute's text, and the case's lines removed.
    core.set_section_guide("kindergarten", "").unwrap();
    core.set_case_guide(&case, "kindergarten", "  ").unwrap();
    let shown = section(&mut core, &case, "kindergarten");
    assert!(shown.guide_is_default);
    assert!(shown.guide.starts_with("המסגרות לפי הסדר"));
    assert!(shown.case_guide.is_empty());
    assert!(core
        .vault_ref()
        .unwrap()
        .case_meta(&case)
        .unwrap()
        .guides
        .is_none());
}

#[test]
fn a_guide_is_only_for_a_known_section_and_within_its_length() {
    let (_dir, mut core, case) = setup(None);
    assert!(matches!(
        core.set_section_guide("nonsense", "x"),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        core.set_case_guide(&case, "letter_parents", "x"),
        Err(CoreError::NotFound(_))
    ));
    assert!(matches!(
        core.set_section_guide("cognitive", &"א".repeat(4_001)),
        Err(CoreError::Refused(_))
    ));
    // A section of the old template is known too: a case from before round 28 is in it.
    core.set_section_guide("referral", "שורה").unwrap();
}
