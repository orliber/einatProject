//! SEC9: what the screen can send, sent wrong. Every command is a thin wrapper over one of
//! these `Core` calls, so they are called here with another case's ids, ids that do not
//! exist, and strings no screen would send (empty, a path, a NUL, direction marks, a
//! megabyte). Nothing may panic, and nothing done through one case may touch another.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::*;

fn hostile() -> Vec<String> {
    vec![
        String::new(),
        " ".to_owned(),
        "../../../etc/passwd".to_owned(),
        "..\\..\\Windows\\System32".to_owned(),
        "a\0b".to_owned(),
        "\u{202E}\u{200F}\u{FEFF}".to_owned(),
        "' OR 1=1 --".to_owned(),
        "[ילד]".to_owned(),
        "🙂".repeat(1000),
        "א".repeat(1 << 20),
    ]
}

/// A second case with its own material and paragraph: (case, input, draft).
fn other_case(core: &mut Core) -> (String, String, String) {
    let case = core
        .create_case(
            CaseMeta {
                code: "ב".into(),
                consent: Some(consent()),
                ..CaseMeta::default()
            },
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "נבו".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    let input = core
        .add_input(&case, InputKind::Intake, "שיחה", "נבו אוהב לבנות בקוביות.")
        .unwrap()
        .id;
    core.add_own_paragraph(&case, "background", "נבו נולד בלידה רגילה.")
        .unwrap();
    let draft = core
        .case_detail(&case)
        .unwrap()
        .sections
        .iter()
        .flat_map(|s| s.paragraphs.iter())
        .next()
        .unwrap()
        .id
        .clone();
    (case, input, draft)
}

/// Everything the other case holds, as text, to compare before and after.
fn snapshot(core: &mut Core, case: &str) -> String {
    let d = core.case_detail(case).unwrap();
    format!(
        "{:?}{:?}{:?}{:?}",
        d.meta, d.identities, d.inputs, d.sections
    )
}

type Call = Box<dyn Fn(&mut Core, &str, &str) -> bool>;

/// Removing what is not there is not an error (a second click); it only must touch nothing.
const NO_OP_IS_FINE: &[&str] = &["delete_input", "rename_folder", "delete_folder"];

/// Calls that take this case and an id inside a case. Each returns whether it succeeded.
fn calls_with_an_id() -> Vec<(&'static str, Call)> {
    vec![
        (
            "update_input",
            Box::new(|c, case, id| c.update_input(case, id, "x", "y").is_ok()),
        ),
        (
            "delete_input",
            Box::new(|c, case, id| c.delete_input(case, id).is_ok()),
        ),
        (
            "restore_input",
            Box::new(|c, case, id| c.restore_input(case, id).is_ok()),
        ),
        (
            "score_sheet",
            Box::new(|c, case, id| c.score_sheet(case, id).is_ok()),
        ),
        (
            "set_input_sections",
            Box::new(|c, case, id| {
                c.set_input_sections(case, id, &["background".to_owned()])
                    .is_ok()
            }),
        ),
        (
            "approve_paragraph",
            Box::new(|c, case, id| c.approve_paragraph(case, id).is_ok()),
        ),
        (
            "reject_paragraph",
            Box::new(|c, case, id| c.reject_paragraph(case, id).is_ok()),
        ),
        (
            "edit_paragraph",
            Box::new(|c, case, id| c.edit_paragraph(case, id, "שונה").is_ok()),
        ),
        (
            "paragraph_versions",
            Box::new(|c, case, id| c.paragraph_versions(case, id).is_ok()),
        ),
        (
            "restore_paragraph_version",
            Box::new(|c, case, id| c.restore_paragraph_version(case, id, id).is_ok()),
        ),
        (
            "similar_paragraph",
            Box::new(|c, case, id| c.similar_paragraph(case, id).is_ok()),
        ),
        (
            "paragraph_sources",
            Box::new(|c, case, id| c.paragraph_sources(case, id).is_ok()),
        ),
        (
            "add_own_paragraph_at",
            Box::new(|c, case, id| {
                c.add_own_paragraph_at(case, "background", "חדש", Some(Some(id)))
                    .is_ok()
            }),
        ),
        (
            "prepare_section_replacing",
            Box::new(|c, case, id| {
                c.prepare_section_replacing(case, "background", "טיוטה", Some(id))
                    .is_ok()
            }),
        ),
    ]
}

/// Calls that take a case and free text.
fn calls_with_text() -> Vec<(&'static str, Call)> {
    vec![
        (
            "case_detail",
            Box::new(|c, case, _| c.case_detail(case).is_ok()),
        ),
        ("chat", Box::new(|c, case, t| c.chat(case, t).is_ok())),
        (
            "approve_section",
            Box::new(|c, case, t| c.approve_section(case, t).is_ok()),
        ),
        (
            "add_own_paragraph",
            Box::new(|c, case, t| c.add_own_paragraph(case, t, t).is_ok()),
        ),
        (
            "prepare_section",
            Box::new(|c, case, t| c.prepare_section(case, t, t).is_ok()),
        ),
        (
            "prepare_verify",
            Box::new(|c, case, t| c.prepare_verify(case, t).is_ok()),
        ),
        (
            "preview_filter",
            Box::new(|c, case, t| c.preview_filter(case, t).is_ok()),
        ),
        (
            "learn_ocr_misreads",
            Box::new(|c, case, t| c.learn_ocr_misreads(case, t).is_ok()),
        ),
        (
            "restore_auto_hidden",
            Box::new(|c, case, t| c.restore_auto_hidden(case, t, t).is_ok()),
        ),
        (
            "confirm_auto_hidden",
            Box::new(|c, case, t| c.confirm_auto_hidden(case, t, t).is_ok()),
        ),
        (
            "change_role",
            Box::new(|c, case, t| c.change_role(case, t, Role::Other).is_ok()),
        ),
        ("letter", Box::new(|c, case, t| c.letter(case, t).is_ok())),
        (
            "prepare_letter",
            Box::new(|c, case, t| c.prepare_letter(case, t, t).is_ok()),
        ),
        (
            "import_document",
            Box::new(|c, case, t| c.import_document(case, t, t.as_bytes()).is_ok()),
        ),
        (
            "move_case",
            Box::new(|c, case, t| c.move_case(case, Some(t)).is_ok()),
        ),
        (
            "purge_case",
            Box::new(|c, case, t| c.purge_case(case, t).is_ok()),
        ),
        (
            "rename_folder",
            Box::new(|c, _, t| c.rename_folder(t, t).is_ok()),
        ),
        (
            "delete_folder",
            Box::new(|c, _, t| c.delete_folder(t).is_ok()),
        ),
    ]
}

#[test]
fn an_id_from_another_case_is_refused_and_leaves_that_case_alone() {
    let (_dir, mut core, case) = setup(None);
    let (other, input, draft) = other_case(&mut core);
    let before = snapshot(&mut core, &other);
    for (name, call) in calls_with_an_id() {
        for id in [&input, &draft] {
            let ok = call(&mut core, &case, id);
            assert!(
                !ok || NO_OP_IS_FINE.contains(&name),
                "{name} accepted an id of another case"
            );
        }
    }
    assert_eq!(snapshot(&mut core, &other), before);
}

#[test]
fn hostile_strings_never_panic_and_never_reach_another_case() {
    let (_dir, mut core, case) = setup(None);
    let (other, _, _) = other_case(&mut core);
    let before = snapshot(&mut core, &other);
    let mut wrong: Vec<String> = Vec::new();
    for s in hostile() {
        for (name, call) in calls_with_an_id().into_iter().chain(calls_with_text()) {
            // As the case id too: a made-up case is refused like any other.
            for target in [case.as_str(), s.as_str()] {
                let short: String = s.chars().take(20).collect();
                match catch_unwind(AssertUnwindSafe(|| call(&mut core, target, &s))) {
                    Err(_) => wrong.push(format!("{name} panicked on {short:?}")),
                    Ok(true) if target == s && !s.is_empty() && !NO_OP_IS_FINE.contains(&name) => {
                        wrong.push(format!("{name} accepted the case {short:?}"));
                    }
                    Ok(_) => {}
                }
            }
        }
    }
    wrong.dedup();
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert_eq!(snapshot(&mut core, &other), before);
}
