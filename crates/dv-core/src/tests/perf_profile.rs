//! Where the time goes in what she waits for: opening the vault, opening a case, preparing a
//! request (with the name model on when `DV_NER_MODEL` is set). A measurement, not a gate:
//! `DV_PERF=1 [DV_NER_MODEL=<dir>] cargo test --release -p dv-core perf_profile -- --nocapture`
//! All text is invented.

use std::time::Instant;

use super::*;

const CASES: usize = 12;
const MATERIALS: usize = 8;

fn material(i: usize) -> String {
    let lines = [
        "בתצפית בגן נראה כי אלון משחק בעיקר לבד בפינת הבנייה, ונוגה מנסה להצטרף אליו.",
        "הגננת שירה מתארת קושי במעברים בין פעילויות ובפרידה בבוקר מאמא.",
        "בשיחה עם ההורים עלה כי השינה השתפרה בחודשים האחרונים, אחרי שעברו דירה.",
        "במשחק סמלי הוא בונה סיפור קצר עם התחלה וסוף, ומספר על הודיה מהשכונה.",
        "בפעילות מוטורית עדינה נצפתה אחיזת עיפרון לא בשלה לגילו, והוא מתעייף מהר.",
    ];
    (0..40)
        .map(|j| lines[(i + j) % lines.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

fn took(what: &str, t: Instant) {
    let _ = std::io::Write::write_fmt(
        &mut std::io::stderr(),
        format_args!("PERF {what}: {:?}\n", t.elapsed()),
    );
}

#[cfg(unix)]
#[test]
fn perf_profile() {
    if std::env::var_os("DV_PERF").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(
        &dir.path().join("vault"),
        Some(Box::new(FakeTransport::default()) as Box<dyn Transport>),
    )
    .with_models_dir(dir.path().join("models"));
    core.argon = Some(Argon2Params {
        m_kib: 512 * 1024,
        ..Argon2Params::FLOOR
    });
    core.create_vault(PASSWORD).unwrap();
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into()])
        .unwrap();
    let model = std::env::var_os("DV_NER_MODEL");
    if let Some(m) = &model {
        let f = dir
            .path()
            .join("models")
            .join(dv_egress::name_model::MODEL_FOLDER);
        std::fs::create_dir_all(&f).unwrap();
        for file in dv_egress::name_model::MODEL_FILES {
            std::os::unix::fs::symlink(std::path::Path::new(m).join(file.name), f.join(file.name))
                .unwrap();
        }
        if let Some(exe) = std::env::var_os("DV_NER_WORKER") {
            core.ner_exe = Some(std::path::PathBuf::from(exe));
        }
        core.set_name_model(true).unwrap();
    }
    let mut ids = Vec::new();
    for c in 0..CASES {
        let meta = CaseMeta {
            code: format!("TEST-{c:04}"),
            age: Some(Age {
                years: 5,
                months: 4,
            }),
            child_gender: Some(GrammaticalGender::Male),
            consent: Some(consent()),
            ..CaseMeta::default()
        };
        let case = core
            .create_case(
                meta,
                vec![IdentityInput {
                    id: None,
                    role: Role::Child,
                    value: "אלון".into(),
                    aliases: vec![],
                }],
            )
            .unwrap();
        let t = Instant::now();
        for i in 0..MATERIALS {
            core.add_input(
                &case,
                InputKind::Kindergarten,
                &format!("תצפית {i}"),
                &material(i + c),
            )
            .unwrap();
        }
        if c == 0 {
            took("add 8 materials", t);
        }
        ids.push(case);
    }
    let case = ids[0].clone();
    for round in 0..2 {
        core.lock();
        let t = Instant::now();
        core.unlock(PASSWORD).unwrap();
        took(&format!("unlock #{round}"), t);
        let t = Instant::now();
        let _ = core.status();
        took("status", t);
        let t = Instant::now();
        core.list_cases().unwrap();
        took("list_cases", t);
        let t = Instant::now();
        core.case_detail(&case).unwrap();
        took("case_detail", t);
        for (n, instr) in ["תנסח פסקה על התצפית", "תקצר קצת"].iter().enumerate()
        {
            let t = Instant::now();
            let p = core.prepare_section(&case, "kindergarten", instr).unwrap();
            took(&format!("prepare_section #{n}"), t);
            if n == 1 {
                let t = Instant::now();
                core.send_section(p.approval_id.as_deref().unwrap())
                    .unwrap();
                took("send_section (fake transport)", t);
            }
        }
        let t = Instant::now();
        let p = core
            .prepare_section(&ids[1], "kindergarten", "תנסח")
            .unwrap();
        took("prepare_section other case", t);
        let _ = p;
    }
    full_draft(&mut core, &case);
    long_work(&mut core, &case, dir.path());
}

/// "לכתוב את הריקים" on a case whose materials feed many sections (D-109): how long the
/// sections' requests take, and how long the core is held while they are prepared.
fn full_draft(core: &mut Core, first: &str) {
    let meta = CaseMeta {
        code: "TEST-0900".into(),
        age: Some(Age {
            years: 6,
            months: 2,
        }),
        child_gender: Some(GrammaticalGender::Female),
        consent: Some(consent()),
        ..CaseMeta::default()
    };
    let case = core
        .create_case(
            meta,
            vec![IdentityInput {
                id: None,
                role: Role::Child,
                value: "נוגה".into(),
                aliases: vec![],
            }],
        )
        .unwrap();
    let kinds = [
        InputKind::Intake,
        InputKind::PriorReport,
        InputKind::Professional,
        InputKind::Kindergarten,
        InputKind::Observation,
        InputKind::SessionNote,
    ];
    for (i, kind) in kinds.into_iter().cycle().take(MATERIALS * 2).enumerate() {
        core.add_input(&case, kind, &format!("חומר {i}"), &material(i))
            .unwrap();
    }
    let t = Instant::now();
    let all = core.prepare_full_draft(&case).unwrap();
    took(&format!("prepare_full_draft ({} sections)", all.len()), t);
    // The same, step by step as the app runs it (D-109): what holds the core, and what not.
    let t = Instant::now();
    let jobs = core.begin_full_draft(&case).unwrap();
    took("full draft: begin (locked)", t);
    let t = Instant::now();
    let runs = run_requests(jobs).unwrap();
    took("full draft: filter side by side (free)", t);
    let t = Instant::now();
    let (ready, again) = core.finish_requests(runs).unwrap();
    took(
        &format!(
            "full draft: finish (locked, {} ready, {} again)",
            ready.len(),
            again.len()
        ),
        t,
    );
    let t = Instant::now();
    let job = core
        .begin_prepare_section(first, "kindergarten", "תנסח", None)
        .unwrap();
    took("prepare_section: begin (locked)", t);
    let t = Instant::now();
    let ran = job.run();
    took("prepare_section: filter (free)", t);
    let t = Instant::now();
    core.finish_request(ran).unwrap();
    took("prepare_section: finish (locked)", t);
}

/// The work that may hold the core for long (F2): each must leave the lock or stay short.
fn long_work(core: &mut Core, case: &str, dir: &std::path::Path) {
    let page = material(3);
    let report = (0..12).map(|_| page.as_str()).collect::<Vec<_>>().join(" ");
    let bytes = super::import_export::docx(&report, "מרכז בדוי", "יעל בדויה");
    let t = Instant::now();
    let job = core
        .begin_import_document(case, "דוח קודם.docx", bytes.clone())
        .unwrap();
    let read = job.run();
    took("import_document: read (free)", t);
    let t = Instant::now();
    core.finish_import_document(case, read).unwrap();
    took("import_document: preview (locked, 12 pages)", t);
    for n in 0..8 {
        let read = core
            .begin_import_style_source(&format!("דוח {n}.docx"), bytes.clone())
            .unwrap()
            .run();
        let t = Instant::now();
        let p = core.finish_import_style_source(read).unwrap();
        let all: Vec<u32> = (0..u32::try_from(p.parts.len()).unwrap()).collect();
        if n == 0 {
            took("import_style_source: preview (locked)", t);
        }
        let t = Instant::now();
        core.save_style_source(&p.token, &all, None).unwrap();
        if n == 7 {
            took("save_style_source (8th report)", t);
        }
    }
    let backups = dir.join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    // Each job: the time under the lock (begin + finish) and the time away from it (run).
    let t = Instant::now();
    let task = core.begin_backup(&backups.join("b.vaultbak")).unwrap();
    took("backup: begin (locked)", t);
    let t = Instant::now();
    let run = task.run();
    took("backup: write and read back (free)", t);
    let t = Instant::now();
    let done = core.finish_backup(run).unwrap();
    took(
        &format!("backup: finish (locked, {} KB)", done.bytes / 1024),
        t,
    );
    let staged = std::fs::read(&done.path).unwrap();
    core.stage_backup(staged, "b.vaultbak".into()).unwrap();
    let t = Instant::now();
    let job = core.begin_check_backup(PASSWORD).unwrap();
    let t2 = Instant::now();
    let drilled = job.run();
    took("drill: run (free)", t2);
    core.finish_check_backup(drilled).unwrap();
    took("drill: all", t);
    let t = Instant::now();
    let job = core
        .begin_change_password(PASSWORD, false, "סיסמה חדשה וארוכה מאוד")
        .unwrap();
    took("password: begin (locked)", t);
    let t = Instant::now();
    let made = job.run();
    took("password: run (free)", t);
    let t = Instant::now();
    core.finish_change_password(made).unwrap();
    took("password: finish (locked)", t);
}
