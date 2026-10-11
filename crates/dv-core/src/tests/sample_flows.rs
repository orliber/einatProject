//! The two sample cases (Tamar, new; Alon, with a follow-up a year later) run through the whole
//! flow as Einat uses it: a new case, every document imported, scores typed in, the materials
//! sorted, a question about the case, every section drafted and approved, and the Word file.
//! Each step must go through without a block she cannot act on, and nothing that names the
//! family may reach the transport. Fabricated data only (`tests/samples/make_samples.py`).

use std::path::{Path, PathBuf};

use dv_domain::{ScoreEntry, ScoreSheet};

use super::*;

/// Makes both sample sets with the generator, the way the zip files Einat gets are made.
fn samples() -> tempfile::TempDir {
    let out = tempfile::tempdir().unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/samples/make_samples.py"
    );
    let python = if cfg!(windows) { "python" } else { "python3" };
    let status = std::process::Command::new(python)
        .arg(script)
        .arg(out.path())
        .stdout(std::process::Stdio::null())
        .status()
        .expect("python runs the sample generator");
    assert!(status.success());
    out
}

/// The PDFs are made with LibreOffice. Without it they are missing; that is allowed on a
/// developer's computer, never in CI (`DV_REQUIRE_SAMPLE_PDF`): the PDF reader is part of the flow.
fn pdf_made(dir: &Path) -> bool {
    let made = files_in(dir).iter().any(|p| {
        p.extension().is_some_and(|e| e == "pdf") && !p.to_string_lossy().contains("סרוק")
    });
    assert!(
        made || std::env::var_os("DV_REQUIRE_SAMPLE_PDF").is_none(),
        "LibreOffice is needed in CI to make the sample PDFs"
    );
    if !made {
        eprintln!("no LibreOffice: the sample PDFs were not made and are not checked");
    }
    made
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        // The scores are typed into the scores screen, not imported.
        .filter(|p| {
            let name = p.to_string_lossy();
            p.is_file() && !name.contains("קרא אותי") && !name.contains("להקלדה")
        })
        .collect();
    files.sort();
    files
}

fn person(role: Role, value: &str) -> IdentityInput {
    IdentityInput {
        id: None,
        role,
        value: value.into(),
        aliases: vec![],
    }
}

fn sheet(instrument: &str, entries: &[(&str, f64)]) -> ScoreSheet {
    ScoreSheet {
        instrument: instrument.into(),
        module: String::new(),
        cutoff: None,
        entries: entries
            .iter()
            .map(|(m, v)| ScoreEntry {
                measure: (*m).into(),
                value: *v,
                note: String::new(),
            })
            .collect(),
        notes: String::new(),
    }
}

/// Imports every document in `dir` the way the materials tab does: the preview, then the
/// material saved with the title and kind it suggested. Returns the previews by file name.
fn import_all(core: &mut Core, case: &str, dir: &Path) -> Vec<(String, ImportPreview)> {
    let mut out = Vec::new();
    for path in files_in(dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        match core.import_document(case, &name, &bytes) {
            Ok(p) => {
                assert!(!p.body.trim().is_empty(), "{name}: an empty body");
                core.add_input(case, p.suggested_kind, &p.title, &p.body)
                    .unwrap_or_else(|e| panic!("{name}: {}", e.to_ui().message));
                out.push((name, p));
            }
            Err(e) => {
                assert!(name.contains("סרוק"), "{name}: {}", e.to_ui().message);
                let message = e.to_ui().message;
                assert!(
                    message.contains("סרוק") || message.contains("תמונה"),
                    "{name}: the refusal says why: {message}"
                );
            }
        }
    }
    out
}

fn preview<'a>(previews: &'a [(String, ImportPreview)], prefix: &str) -> &'a ImportPreview {
    &previews
        .iter()
        .find(|(n, _)| n.starts_with(prefix))
        .unwrap_or_else(|| panic!("{prefix} was not imported"))
        .1
}

fn hid(p: &ImportPreview, token: &str) -> bool {
    p.auto_hidden.iter().any(|a| a.token.contains(token))
}

/// Sends what was prepared; a block or a question here is a block Einat would hit.
fn cleared(step: &str, p: &Prepared) -> String {
    assert!(p.blocked.is_empty(), "{step}: blocked {:?}", p.blocked);
    assert!(p.suspects.is_empty(), "{step}: asks {:?}", p.suspects);
    p.approval_id
        .clone()
        .unwrap_or_else(|| panic!("{step}: no approval"))
}

/// Every section with material is drafted, the draft approved, and the file made.
fn draft_approve_export(core: &mut Core, case: &str) -> ExportCheck {
    let drafts = core.prepare_full_draft(case).unwrap();
    assert!(drafts.len() >= 3, "only {} sections drafted", drafts.len());
    for (key, prepared) in drafts {
        let approval = cleared(&format!("section {key}"), &prepared);
        let result = core.send_section(&approval).unwrap();
        assert!(!result.paragraphs.is_empty(), "{key}: no paragraphs");
        core.approve_section(case, &key).unwrap();
    }
    // The summary is written from the approved sections, as she does last.
    let summary = core.prepare_section(case, "summary", "").unwrap();
    core.send_section(&cleared("summary", &summary)).unwrap();
    core.approve_section(case, "summary").unwrap();

    let check = core.check_export(case).unwrap();
    assert!(
        check.blocking.is_empty(),
        "export blocked: {:?}",
        check.blocking
    );
    let docx = core.export_report(case, None).unwrap();
    let text = read_docx_text(&docx);
    assert!(
        !text.contains('[') || !text.contains("[ילד"),
        "a tag left in the file"
    );
    let locked = core.export_report(case, Some("סיסמה-לקובץ-1")).unwrap();
    assert!(locked.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]));
    check
}

fn assert_nothing_left(sent: &[String], secrets: &[&str]) {
    let sent = sent.join("\n");
    assert!(sent.contains("[ילד"), "the child goes out as a tag");
    for secret in secrets {
        assert!(!sent.contains(secret), "{secret} left the machine");
    }
}

#[test]
fn tamar_new_case_goes_through_the_whole_flow() {
    let out = samples();
    let dir = out.path().join("תמר");
    let fake = FakeTransport::default();
    let (_dir, mut core, _) = setup(Some(fake.clone()));
    fake.sent.lock().unwrap().clear();

    let meta = CaseMeta {
        code: "DEMO-TAMAR".into(),
        age: Some(Age {
            years: 3,
            months: 3,
        }),
        child_gender: Some(GrammaticalGender::Female),
        consent: Some(Consent {
            given_on: "2026-09-01".into(),
            form_version: "v1".into(),
            given_by: "שני ההורים".into(),
        }),
        ..CaseMeta::default()
    };
    let case = core
        .create_case(
            meta,
            vec![
                person(Role::Child, "תמר דמיוני"),
                person(Role::Mother, "מיכל"),
                person(Role::Father, "עידו"),
                person(Role::Brother, "איתי"),
                person(Role::Assistant, "רינה"),
                person(Role::Kindergarten, "מעון הרקפת"),
                person(Role::Town, "קריית הדמיון"),
                person(Role::Slp, "הדס"),
                person(Role::Doctor, "ד\"ר נעמה אלמוגי"),
            ],
        )
        .unwrap();

    let with_pdf = pdf_made(&dir);
    let previews = import_all(&mut core, &case, &dir);
    assert_eq!(
        previews.len(),
        if with_pdf { 6 } else { 5 },
        "every document but the scores is read"
    );
    let intake = preview(&previews, "01");
    assert!(!intake.left_out.is_empty(), "the margins are left out");
    let mut margins = vec![intake, preview(&previews, "03")];
    if with_pdf {
        let pdf = preview(&previews, "04");
        assert_eq!(pdf.format, "pdf");
        assert!(pdf.body.contains("תמר"), "the PDF reads in Hebrew");
        assert!(!pdf.left_out.is_empty(), "the PDF's margins are left out");
        margins.push(pdf);
    }
    for p in margins {
        for line in &p.left_out {
            assert!(
                !p.body.contains(line.trim()),
                "{}: {line} in the body",
                p.file_name
            );
        }
    }
    // Each document lands where the report needs it, under a title she recognises.
    assert_eq!(intake.suggested_kind, InputKind::Intake);
    let mail = preview(&previews, "05");
    assert_eq!(mail.suggested_kind, InputKind::Intake);
    assert_eq!(mail.title, "מייל מההורים אחרי הפגישה");
    assert_eq!(
        preview(&previews, "02").suggested_kind,
        InputKind::Kindergarten
    );
    assert_eq!(
        preview(&previews, "06").suggested_kind,
        InputKind::Observation
    );
    let nursery = preview(&previews, "02");
    assert!(!nursery.body.contains("לא להעביר"), "the comment stays out");
    assert!(
        !nursery.body.contains("ספקטרום"),
        "the deleted wording stays out"
    );
    assert!(
        hid(nursery, "עלמה"),
        "the friend the case does not list is hidden"
    );
    assert!(
        hid(preview(&previews, "05"), "זהבה"),
        "the grandmother is hidden"
    );

    for s in [
        sheet(
            "bayley_4",
            &[
                ("cognitive", 95.0),
                ("language", 74.0),
                ("motor", 98.0),
                ("receptive", 6.0),
                ("expressive", 4.0),
                ("fine_motor", 9.0),
                ("gross_motor", 10.0),
            ],
        ),
        sheet(
            "abas_3",
            &[
                ("gac", 86.0),
                ("conceptual", 78.0),
                ("social", 90.0),
                ("practical", 94.0),
                ("communication", 5.0),
                ("functional_pre_academics", 7.0),
                ("self_direction", 8.0),
                ("leisure", 9.0),
                ("social_skill", 8.0),
                ("home_living", 9.0),
                ("health_safety", 10.0),
                ("self_care", 9.0),
                ("motor", 10.0),
            ],
        ),
        sheet(
            "cbcl_1_5",
            &[
                ("emotionally_reactive", 62.0),
                ("anxious_depressed", 54.0),
                ("somatic", 51.0),
                ("withdrawn", 60.0),
                ("sleep", 50.0),
                ("attention", 58.0),
                ("aggressive", 55.0),
                ("internalizing", 57.0),
                ("externalizing", 54.0),
                ("total", 58.0),
            ],
        ),
        sheet("ados_2", &[("comparison", 3.0)]),
    ] {
        let saved = core.save_scores(&case, None, &s).unwrap();
        assert!(!saved.content.is_empty());
    }

    let sort = core.prepare_sort(&case).unwrap();
    core.send_sort(&cleared("sort", &sort)).unwrap();

    // The consultation from the demo script, written with names on purpose.
    let ask = "תמר שיחקה עם עלמה במסירת כדור ורינה אמרה שזה המשחק הקבוע שלהן. איך לנסח את זה בסעיף התצפית?";
    let consult = core.prepare_consult(Some(&case), None, ask).unwrap();
    let out_text = outgoing(&consult);
    assert!(
        out_text.contains("[ילד]") && !out_text.contains("עלמה") && !out_text.contains("רינה"),
        "{out_text}"
    );
    let answer = core.send_consult(&cleared("consult", &consult)).unwrap();
    assert!(!answer.answer.is_empty());

    let check = draft_approve_export(&mut core, &case);
    assert_eq!(check.score_tables, 4);
    assert!(!check.file_name.contains("תמר"));

    let phone = ["054", "000", "0000"].join("-");
    assert_nothing_left(
        &fake.sent.lock().unwrap(),
        &[
            "תמר",
            "דמיוני",
            "מיכל",
            "עידו",
            "איתי ",
            "רינה",
            "הרקפת",
            "קריית הדמיון",
            "הדס",
            "נעמה",
            "אלמוגי",
            "עלמה",
            "זהבה",
            &phone,
            "לא להעביר",
            "אולי היא על",
        ],
    );
}

#[test]
fn alon_case_and_its_follow_up_go_through_the_whole_flow() {
    let out = samples();
    let fake = FakeTransport::default();
    let (_dir, mut core, _) = setup(Some(fake.clone()));
    fake.sent.lock().unwrap().clear();

    let meta = CaseMeta {
        code: "DEMO-ALON".into(),
        age: Some(Age {
            years: 5,
            months: 8,
        }),
        child_gender: Some(GrammaticalGender::Male),
        consent: Some(consent()),
        ..CaseMeta::default()
    };
    let case = core
        .create_case(
            meta,
            vec![
                IdentityInput {
                    aliases: vec!["בדיוני".into()],
                    ..person(Role::Child, "אלון")
                },
                person(Role::Mother, "שירה"),
                person(Role::Father, "גיא"),
                person(Role::Sister, "נוגה"),
                person(Role::Teacher, "אורית"),
                person(Role::Kindergarten, "גן השקד"),
                person(Role::Town, "גבעת הרימון"),
                person(Role::Slp, "ליאת"),
                person(Role::Therapist, "דפנה"),
                person(Role::Doctor, "ד\"ר יואב רון"),
            ],
        )
        .unwrap();

    let with_pdf = pdf_made(out.path());
    let previews = import_all(&mut core, &case, out.path());
    assert_eq!(previews.len(), if with_pdf { 6 } else { 5 });
    assert!(hid(preview(&previews, "02"), "עומרי"));
    assert_eq!(preview(&previews, "01").suggested_kind, InputKind::Intake);
    assert_eq!(
        preview(&previews, "02").suggested_kind,
        InputKind::Kindergarten
    );
    assert_eq!(preview(&previews, "05").title, "מכתב הפניה מרופא הילדים");
    if with_pdf {
        assert_eq!(preview(&previews, "04").format, "pdf");
    }
    core.save_scores(
        &case,
        None,
        &sheet(
            "wppsi_iv",
            &[
                ("fsiq", 96.0),
                ("vci", 104.0),
                ("vsi", 98.0),
                ("fri", 100.0),
                ("wmi", 85.0),
                ("psi", 82.0),
            ],
        ),
    )
    .unwrap();
    let sort = core.prepare_sort(&case).unwrap();
    core.send_sort(&cleared("sort", &sort)).unwrap();
    draft_approve_export(&mut core, &case);

    // A year later: a follow-up keeps the names and compares the scores.
    // It opens on its details: a new age and a new consent, as the notice asks.
    let follow = core.create_follow_up(&case).unwrap();
    let mut meta = core.case_detail(&follow).unwrap().meta;
    meta.age = Some(Age {
        years: 6,
        months: 9,
    });
    meta.consent = Some(consent());
    core.update_case(&follow, meta).unwrap();
    import_all(&mut core, &follow, &out.path().join("מעקב - שנה אחרי"));
    core.save_scores(
        &follow,
        None,
        &sheet(
            "wisc_v",
            &[
                ("fsiq", 99.0),
                ("vci", 106.0),
                ("vsi", 97.0),
                ("fri", 101.0),
                ("wmi", 88.0),
                ("psi", 86.0),
            ],
        ),
    )
    .unwrap();
    core.add_comparison_material(&follow).unwrap();
    let sort = core.prepare_sort(&follow).unwrap();
    core.send_sort(&cleared("follow-up sort", &sort)).unwrap();
    draft_approve_export(&mut core, &follow);

    let phone = ["050", "000", "0000"].join("-");
    let mail = ["shira.fake", "example.com"].join("@");
    assert_nothing_left(
        &fake.sent.lock().unwrap(),
        &[
            "אלון",
            "שירה",
            "גיא ",
            "נוגה",
            "אורית",
            "השקד",
            "הרימון",
            "ליאת",
            "דפנה",
            "יואב",
            "עומרי",
            "בדיוני",
            "000000026",
            &phone,
            &mail,
            "הזית 4",
            "הפרעת קשב",
        ],
    );
}
