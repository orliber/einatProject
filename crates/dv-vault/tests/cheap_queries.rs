//! The two cheap queries the core's timers and the export check rely on. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dv_domain::CaseMeta;
use dv_vault::{Argon2Params, AuditEvent, Vault};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

fn new_vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let created = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST).unwrap();
    (dir, created.vault)
}

fn a_case(vault: &mut Vault) -> String {
    vault
        .create_case(&CaseMeta {
            code: "TEST-0001".into(),
            ..CaseMeta::default()
        })
        .unwrap()
}

#[test]
fn the_backup_timer_asks_cheaply_whether_there_is_a_case_at_all() {
    let (_dir, mut vault) = new_vault();
    assert!(!vault.has_cases().unwrap());
    let case = a_case(&mut vault);
    assert!(vault.has_cases().unwrap());
    // A case in the recycle bin still counts: it is in the vault, and in a backup.
    vault.trash_case(&case).unwrap();
    assert!(vault.has_cases().unwrap());
}

#[test]
fn an_export_is_found_by_its_fingerprint_without_reading_the_whole_log() {
    let (_dir, mut vault) = new_vault();
    let case = a_case(&mut vault);
    vault
        .record(
            AuditEvent::Export,
            Some(&case),
            &serde_json::json!({ "sha256": "abc123", "pages": 2 }),
        )
        .unwrap();
    vault
        .record(
            AuditEvent::Export,
            Some(&case),
            &serde_json::json!({ "sha256": "def456", "pages": 3 }),
        )
        .unwrap();
    let found = vault
        .audit_find("export", "\"sha256\":\"abc123\"")
        .unwrap()
        .unwrap();
    assert_eq!(found.event, "export");
    assert!(found.meta.contains("abc123"));
    assert!(vault
        .audit_find("export", "\"sha256\":\"zzz\"")
        .unwrap()
        .is_none());
    assert!(vault
        .audit_find("unlock", "\"sha256\":\"abc123\"")
        .unwrap()
        .is_none());
}

#[test]
fn state_the_program_keeps_for_itself_is_written_without_a_log_entry() {
    let (_dir, mut vault) = new_vault();
    let before = vault.audit_entries(100).unwrap().len();
    vault.set_state("usage/2026-10", "{}").unwrap();
    assert_eq!(
        vault.setting("usage/2026-10").unwrap().as_deref(),
        Some("{}")
    );
    assert_eq!(vault.audit_entries(100).unwrap().len(), before);
    // A setting she chose is logged, as before.
    vault.set_setting("backup_auto", "1").unwrap();
    let after = vault.audit_entries(100).unwrap();
    assert_eq!(after.len(), before + 1);
    assert_eq!(after[0].event, "settings_changed");
}
