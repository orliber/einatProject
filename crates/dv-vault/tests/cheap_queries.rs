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

/// The case list and "every case's identities" are read in a few statements for all cases;
/// what they return is what asking case by case returns, in the same order.
#[test]
fn the_batched_reads_return_what_the_case_by_case_reads_do() {
    use dv_domain::{IdentityInput, Role};
    let (_dir, mut vault) = new_vault();
    let person = |role, value: &str| IdentityInput {
        id: None,
        role,
        value: value.into(),
        aliases: vec![format!("{value} כינוי")],
    };
    let mut ids = Vec::new();
    for i in 0..6 {
        let id = vault
            .create_case(&CaseMeta {
                code: format!("TEST-{i:04}"),
                ..CaseMeta::default()
            })
            .unwrap();
        // The child is not always the first person, and one case has no child at all.
        let mut people = vec![person(Role::Mother, &format!("אמאבדויה{i}"))];
        if i != 3 {
            people.push(person(Role::Child, &format!("ילדבדוי{i}")));
            people.push(person(Role::Child, &format!("ילדשני{i}")));
        }
        vault.set_identities(&id, &people).unwrap();
        ids.push(id);
    }
    vault.trash_case(&ids[5]).unwrap();

    let mut one_by_one = Vec::new();
    for id in &ids {
        one_by_one.extend(vault.identities(id).unwrap());
    }
    let mut all = vault.all_identities().unwrap();
    assert_eq!(all.len(), one_by_one.len());
    // The same cases in whatever order the vault lists them, each case's people in order.
    for id in &ids {
        let of_case: Vec<_> = all.iter().filter(|i| &i.case_id == id).cloned().collect();
        assert_eq!(of_case, vault.identities(id).unwrap());
    }
    all.sort_by(|a, b| a.id.cmp(&b.id));
    one_by_one.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(all, one_by_one);

    let list = vault.list_cases().unwrap();
    assert_eq!(list.len(), 5);
    for c in list.iter().chain(vault.list_trash().unwrap().iter()) {
        let first_child = vault
            .identities(&c.id)
            .unwrap()
            .into_iter()
            .find(|i| i.role == Role::Child)
            .map(|i| i.value);
        assert_eq!(c.child_name, first_child, "{}", c.meta.code);
        assert_eq!(c.meta, vault.case_meta(&c.id).unwrap());
        assert_eq!(c.approved_sections, vault.approved_sections(&c.id).unwrap());
        let (created, updated) = vault.case_times(&c.id).unwrap();
        assert_eq!((c.created_at, c.updated_at), (created, updated));
    }
    assert_eq!(vault.list_trash().unwrap().len(), 1);
    assert!(vault.case_times("no-such-case").is_err());
}
