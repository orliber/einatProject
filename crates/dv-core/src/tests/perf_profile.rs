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
}
