//! A vault made by an earlier version must open in this one, with everything in it.
//!
//! `tests/fixtures/vaults/<version>/` keeps a small vault (fabricated case, fast Argon2) as
//! text, one file per vault file: each half-byte is a letter `a`–`p`, so there are no digits
//! for the personal-data scan to read as an ID or a phone number. Each release adds one:
//! `DV_WRITE_VAULT_FIXTURE=<version> cargo test -p dv-core upgrade::write -- --ignored`.
//! Every kept vault is opened by the current code: unlocked with its password, the case and
//! its materials, names, scores and paragraphs read back, and the log's chain intact. Then
//! she works on it: a section drafted, approved and exported with the names back, a backup
//! written and restored elsewhere, and the vault closed and opened again with the new work in
//! it (F7).

use std::path::{Path, PathBuf};

use dv_domain::{ScoreEntry, ScoreSheet};

use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/vaults")
}

/// What every kept vault holds, so the check is the same for all of them.
const CODE: &str = "FIX-0001";
const CHILD: &str = "גלעד";
const TEACHER: &str = "מאיה";
const OWN: &str = "גלעד שיתף פעולה לאורך כל המפגשים, והתמיד גם במשימות שהיו קשות לו.";

fn fill(core: &mut Core) {
    core.set_practitioner(vec!["ד\"ר רותם אלמוג".into()])
        .unwrap();
    let meta = CaseMeta {
        code: CODE.into(),
        age: Some(Age {
            years: 5,
            months: 2,
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
                    id: None,
                    role: Role::Child,
                    value: CHILD.into(),
                    aliases: vec![],
                },
                IdentityInput {
                    id: None,
                    role: Role::Teacher,
                    value: TEACHER.into(),
                    aliases: vec![],
                },
            ],
        )
        .unwrap();
    core.add_input(
        &case,
        InputKind::Kindergarten,
        "שיחה עם הגננת",
        "מאיה סיפרה כי גלעד מתקשה לחכות לתורו במשחק קבוצתי.",
    )
    .unwrap();
    let sheet = ScoreSheet {
        instrument: "wppsi_iv".into(),
        module: String::new(),
        cutoff: None,
        entries: vec![ScoreEntry {
            measure: "vci".into(),
            value: 104.0,
            note: String::new(),
        }],
        notes: String::new(),
    };
    core.save_scores(&case, None, &sheet).unwrap();
    core.add_own_paragraph(&case, "kindergarten", OWN).unwrap();
}

/// Reads a kept vault back with the current code.
fn check(dir: &Path, version: &str) {
    let mut core = Core::for_tests(dir, None);
    assert!(core.status().vault_exists, "{version}: no vault");
    let status = core
        .unlock(PASSWORD)
        .unwrap_or_else(|e| panic!("{version}: does not unlock: {e:?}"));
    assert!(status.unlocked, "{version}");
    assert!(
        status.integrity_warning.is_none(),
        "{version}: log {:?}",
        status.integrity_warning
    );
    let cases = core.list_cases().unwrap();
    let summary = cases
        .iter()
        .find(|c| c.meta.code == CODE)
        .unwrap_or_else(|| panic!("{version}: the case is gone"));
    let detail = core.case_detail(&summary.id).unwrap();
    let names: Vec<&str> = detail.identities.iter().map(|i| i.value.as_str()).collect();
    assert!(
        names.contains(&CHILD) && names.contains(&TEACHER),
        "{version}: names {names:?}"
    );
    assert_eq!(detail.inputs.len(), 2, "{version}: materials");
    let section = detail
        .sections
        .iter()
        .find(|s| s.key == "kindergarten")
        .unwrap_or_else(|| panic!("{version}: no kindergarten section"));
    assert!(
        section
            .paragraphs
            .iter()
            .any(|p| p.text == OWN && p.status == DraftStatus::Approved),
        "{version}: her paragraph"
    );
    assert_eq!(
        core.check_export(&summary.id).unwrap().score_tables,
        1,
        "{version}: scores"
    );
    // Opened once by the new version, it opens again (migrations ran and were recorded), and
    // the vault now names this version as the newest that opened it (D-104).
    core.lock();
    assert_eq!(
        Vault::opened_by(dir).unwrap().as_deref(),
        Some(dv_vault::version::APP_VERSION),
        "{version}: version recorded"
    );
    let mut again = Core::for_tests(dir, None);
    assert!(
        again.unlock(PASSWORD).unwrap().unlocked,
        "{version}: second open"
    );
}

/// A day's work on a vault this version just opened, saved and opened again.
fn work(dir: &Path, version: &str) {
    let mut core = Core::for_tests(dir, None);
    core.unlock(PASSWORD).unwrap();
    let case = core
        .list_cases()
        .unwrap()
        .into_iter()
        .find(|c| c.meta.code == CODE)
        .unwrap_or_else(|| panic!("{version}: the case is gone"))
        .id;
    core.add_input(
        &case,
        InputKind::Intake,
        "שיחה עם ההורים",
        "ההורים סיפרו שגלעד אוהב לבנות מגדלים מקוביות.",
    )
    .unwrap_or_else(|e| panic!("{version}: new material: {e:?}"));
    let prepared = core
        .prepare_section(&case, "kindergarten", "טיוטה")
        .unwrap_or_else(|e| panic!("{version}: prepare: {e:?}"));
    let approval = prepared
        .approval_id
        .unwrap_or_else(|| panic!("{version}: nothing to send: {:?}", prepared.blocked));
    core.send_section(&approval)
        .unwrap_or_else(|e| panic!("{version}: send: {e:?}"));
    let drafted = core
        .case_detail(&case)
        .unwrap()
        .sections
        .into_iter()
        .find(|s| s.key == "kindergarten")
        .and_then(|s| {
            s.paragraphs
                .into_iter()
                .find(|p| p.status == DraftStatus::Proposed)
        })
        .unwrap_or_else(|| panic!("{version}: no new draft"));
    core.approve_paragraph(&case, &drafted.id).unwrap();
    let doc = super::import_export::read_docx_text(
        &core
            .export_report(&case, None)
            .unwrap_or_else(|e| panic!("{version}: export: {e:?}")),
    );
    assert!(
        doc.contains(OWN),
        "{version}: her paragraph, with the names back"
    );

    let drive = tempfile::tempdir().unwrap();
    let done = core
        .write_backup(&drive.path().join("b"))
        .unwrap_or_else(|e| panic!("{version}: backup: {e:?}"));
    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    other
        .stage_backup(std::fs::read(&done.path).unwrap(), "b.vaultbak".into())
        .unwrap();
    other
        .restore_staged_backup(Some(PASSWORD), None)
        .unwrap_or_else(|e| panic!("{version}: restore: {e:?}"));
    let now = core.case_detail(&case).unwrap();
    assert_eq!(
        other.case_detail(&case).unwrap(),
        now,
        "{version}: restored"
    );

    core.lock();
    let mut again = Core::for_tests(dir, None);
    let status = again.unlock(PASSWORD).unwrap();
    assert!(
        status.integrity_warning.is_none(),
        "{version}: log after work {:?}",
        status.integrity_warning
    );
    assert_eq!(again.case_detail(&case).unwrap(), now, "{version}: saved");
}

fn vault_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files
}

#[test]
fn every_kept_vault_opens_in_this_version() {
    let root = fixtures();
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    assert!(!versions.is_empty(), "no kept vaults in {}", root.display());
    for kept in versions {
        let version = kept.file_name().unwrap().to_string_lossy().into_owned();
        let dir = tempfile::tempdir().unwrap();
        for file in vault_files(&kept) {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            let Some(original) = name.strip_suffix(".txt") else {
                continue;
            };
            let text = std::fs::read_to_string(&file).unwrap();
            let bytes = from_letters(&text);
            std::fs::write(dir.path().join(original), bytes).unwrap();
        }
        check(dir.path(), &version);
        work(dir.path(), &version);
    }
}

/// Writes this version's vault. Run once per release, on purpose.
#[test]
#[ignore = "writes tests/fixtures/vaults/<version>; run with --ignored and DV_WRITE_VAULT_FIXTURE"]
fn write() {
    let version =
        std::env::var("DV_WRITE_VAULT_FIXTURE").expect("DV_WRITE_VAULT_FIXTURE=<version>");
    let dir = tempfile::tempdir().unwrap();
    {
        let mut core = Core::for_tests(dir.path(), None);
        core.create_vault(PASSWORD).unwrap();
        fill(&mut core);
        core.lock();
    }
    check(dir.path(), &version);
    let out = fixtures().join(&version);
    std::fs::create_dir_all(&out).unwrap();
    for file in vault_files(dir.path()) {
        let bytes = std::fs::read(&file).unwrap();
        let text = to_letters(&bytes);
        let lines: Vec<&str> = text
            .as_bytes()
            .chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        let name = format!("{}.txt", file.file_name().unwrap().to_string_lossy());
        std::fs::write(out.join(name), lines.join("\n") + "\n").unwrap();
    }
}

fn to_letters(bytes: &[u8]) -> String {
    let letter = |n: u8| char::from(b'a' + n);
    bytes
        .iter()
        .flat_map(|b| [letter(b >> 4), letter(b & 0x0f)])
        .collect()
}

fn from_letters(text: &str) -> Vec<u8> {
    let nibbles: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| {
            assert!((b'a'..=b'p').contains(&b), "not a kept-vault letter: {b}");
            b - b'a'
        })
        .collect();
    assert!(nibbles.len() % 2 == 0, "odd length");
    nibbles.chunks(2).map(|p| (p[0] << 4) | p[1]).collect()
}
