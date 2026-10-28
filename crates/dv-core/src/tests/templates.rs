//! The institute's report for every new case, and each case keeping its own (round 28,
//! D-066). Fabricated data only.

use super::*;

fn keys(core: &mut Core, case: &str) -> Vec<String> {
    core.case_detail(case)
        .unwrap()
        .sections
        .into_iter()
        .map(|s| s.key)
        .collect()
}

#[test]
fn a_new_case_is_in_the_institute_report_and_an_edit_never_changes_it() {
    let (_dir, mut core, case) = setup(None);
    let meta = core.vault_ref().unwrap().case_meta(&case).unwrap();
    assert_eq!(
        meta.template.as_deref(),
        Some(dv_domain::INSTITUTE_TEMPLATE)
    );
    let k = keys(&mut core, &case);
    assert!(k.contains(&"behavior_attention".to_owned()));
    let at = |x: &str| k.iter().position(|y| y == x).unwrap();
    assert!(at("communication") < at("adaptive"));

    // Details saved from the screen (no template, or another one) keep the case's.
    for template in [None, Some("developmental-assessment-v1".to_owned())] {
        core.update_case(
            &case,
            CaseMeta {
                template,
                ..meta.clone()
            },
        )
        .unwrap();
        assert_eq!(
            core.vault_ref()
                .unwrap()
                .case_meta(&case)
                .unwrap()
                .template
                .as_deref(),
            Some(dv_domain::INSTITUTE_TEMPLATE)
        );
    }
    // Asking for a template from outside does not choose it either.
    let other = core
        .create_case(
            CaseMeta {
                code: "TEST-0009".into(),
                template: Some("developmental-assessment-v1".into()),
                ..CaseMeta::default()
            },
            vec![],
        )
        .unwrap();
    assert_eq!(
        core.vault_ref()
            .unwrap()
            .case_meta(&other)
            .unwrap()
            .template
            .as_deref(),
        Some(dv_domain::INSTITUTE_TEMPLATE)
    );
}

#[test]
fn a_case_opened_before_keeps_its_report() {
    let (_dir, mut core, _case) = setup(None);
    // A case as the vault held it before templates were kept with cases.
    let old = core
        .vault_mut()
        .unwrap()
        .create_case(&CaseMeta {
            code: "TEST-0010".into(),
            consent: Some(consent()),
            ..CaseMeta::default()
        })
        .unwrap();
    let k = keys(&mut core, &old);
    assert!(!k.contains(&"behavior_attention".to_owned()));
    assert_eq!(
        k,
        ReportStructure::load_default()
            .unwrap()
            .sections()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>()
    );
    let titles: Vec<String> = core
        .case_detail(&old)
        .unwrap()
        .sections
        .into_iter()
        .map(|s| s.title)
        .collect();
    assert!(titles.contains(&"הופעה והתרשמות".to_owned()));
}

/// The institute's own section (`behavior_attention`) is approved like any other: "✓ לאשר"
/// and "✓ לאשר את כל הטיוטות" looked its paragraphs up in the default report, did not find
/// them, and left them waiting without a word.
#[test]
fn the_institute_sections_paragraphs_are_approved_by_every_button() {
    let (_dir, mut core, case) = setup(None);
    let mut add = |text: &str| {
        core.vault_mut()
            .unwrap()
            .add_draft(
                &case,
                "behavior_attention",
                text,
                dv_domain::Author::Ai,
                &[],
            )
            .unwrap()
            .id
    };
    let one = add("בשאלון ההורים עלו קשיי קשב בבית.");
    let two = add("בגן נצפתה תנועתיות מוגברת במפגש הבוקר.");
    let three = add("המעברים בין פעילויות דורשים תיווך.");
    let status = |core: &mut Core, id: &str| {
        core.case_detail(&case)
            .unwrap()
            .sections
            .into_iter()
            .flat_map(|s| s.paragraphs)
            .find(|p| p.id == id)
            .unwrap()
            .status
    };
    assert_eq!(
        core.approve_paragraphs(&case, std::slice::from_ref(&one))
            .unwrap(),
        1
    );
    assert_eq!(status(&mut core, &one), DraftStatus::Approved);
    assert_eq!(
        core.approve_clean_paragraphs(&case, std::slice::from_ref(&two))
            .unwrap(),
        1
    );
    assert_eq!(status(&mut core, &two), DraftStatus::Approved);
    core.approve_paragraph(&case, &three).unwrap();
    assert_eq!(status(&mut core, &three), DraftStatus::Approved);
}
