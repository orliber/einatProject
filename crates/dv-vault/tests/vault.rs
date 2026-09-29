//! End-to-end tests of the vault's security properties. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use dv_domain::{Author, CaseMeta, ChatRole, DraftStatus, IdentityInput, InputKind, Role};
use dv_vault::{Argon2Params, AuditEvent, Vault, VaultError};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";

fn new_vault() -> (tempfile::TempDir, Vault, String) {
    let dir = tempfile::tempdir().unwrap();
    let created = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST).unwrap();
    let recovery = created.recovery_key.to_string();
    (dir, created.vault, recovery)
}

fn noam(vault: &mut Vault) -> String {
    let case = vault
        .create_case(&CaseMeta {
            code: "TEST-0001".into(),
            ..CaseMeta::default()
        })
        .unwrap();
    vault
        .set_identities(
            &case,
            &[
                IdentityInput {
                    id: None,
                    role: Role::Child,
                    value: "נועם".into(),
                    aliases: vec!["נועמי".into()],
                },
                IdentityInput {
                    id: None,
                    role: Role::Teacher,
                    value: "מיכל".into(),
                    aliases: vec![],
                },
            ],
        )
        .unwrap();
    vault
        .add_input(
            &case,
            InputKind::Kindergarten,
            "שיחה עם הגננת",
            "מיכל סיפרה שנועם מתקשה במעברים",
        )
        .unwrap();
    case
}

/// Bytes of every file in the vault directory.
fn all_bytes(dir: &std::path::Path) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        out.extend(fs::read(entry.path()).unwrap_or_default());
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn nothing_readable_is_written_to_disk() {
    let (dir, mut vault, _) = new_vault();
    noam(&mut vault);
    drop(vault);
    let bytes = all_bytes(dir.path());
    for secret in [
        "נועם",
        "נועמי",
        "מיכל",
        "מתקשה במעברים",
        "TEST-0001",
        PASSWORD,
    ] {
        assert!(
            !contains(&bytes, secret),
            "{secret} found in plaintext on disk"
        );
    }
    assert!(
        !contains(&bytes, "SQLite format 3"),
        "database files must be encrypted"
    );
}

#[test]
fn unlock_with_password_and_with_recovery_key() {
    let (dir, mut vault, recovery) = new_vault();
    let case = noam(&mut vault);
    drop(vault);

    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert_eq!(vault.identities(&case).unwrap()[0].value, "נועם");
    drop(vault);

    let vault = Vault::unlock_with_recovery(dir.path(), &recovery.to_lowercase()).unwrap();
    assert_eq!(
        vault.inputs(&case).unwrap()[0].content,
        "מיכל סיפרה שנועם מתקשה במעברים"
    );
    assert!(vault.integrity().audit_ok && vault.integrity().header_ok);
}

#[test]
fn wrong_secrets_are_rejected() {
    let (dir, vault, _) = new_vault();
    drop(vault);
    assert!(matches!(
        Vault::unlock_with_password(dir.path(), "סיסמה שגויה לגמרי"),
        Err(VaultError::WrongSecret)
    ));
    let other = dv_vault::recovery::format(&dv_vault::crypto::Key32::random().unwrap());
    assert!(matches!(
        Vault::unlock_with_recovery(dir.path(), &other),
        Err(VaultError::WrongSecret)
    ));
}

#[test]
fn weak_passwords_are_refused_at_creation() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Vault::create(dir.path(), "1234", Argon2Params::TEST),
        Err(VaultError::Policy(_))
    ));
}

#[test]
fn tags_are_stable_and_identities_are_per_case() {
    let (_dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let ids = vault.identities(&case).unwrap();
    assert_eq!(ids[0].tag, "[ילד]");
    assert_eq!(ids[1].tag, "[גננת]");

    // Editing the child's spelling keeps the tag; adding a doctor gets a numbered tag.
    let mut edit: Vec<IdentityInput> = ids
        .iter()
        .map(|i| IdentityInput {
            id: Some(i.id.clone()),
            role: i.role,
            value: i.value.clone(),
            aliases: i.aliases.clone(),
        })
        .collect();
    edit[0].value = "נעם".into();
    edit.push(IdentityInput {
        id: None,
        role: Role::Doctor,
        value: "ד\"ר אבנר שטרן".into(),
        aliases: vec![],
    });
    let after = vault.set_identities(&case, &edit).unwrap();
    assert_eq!(after[0].tag, "[ילד]");
    assert_eq!(after[0].value, "נעם");
    assert_eq!(after[2].tag, "[רופא_1]");

    let other = vault.create_case(&CaseMeta::default()).unwrap();
    assert!(vault.identities(&other).unwrap().is_empty());
    assert_eq!(vault.all_identities().unwrap().len(), 3);
}

#[test]
fn deleting_a_case_shreds_everything_that_belonged_to_it() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    vault
        .add_message(
            &case,
            "kindergarten",
            ChatRole::User,
            "[גננת] סיפרה ש[ילד] מתקשה",
            &[],
            false,
        )
        .unwrap();
    vault.delete_case(&case).unwrap();
    assert!(vault.list_cases().unwrap().is_empty());
    assert!(vault.all_identities().unwrap().is_empty());
    assert!(matches!(vault.inputs(&case), Err(VaultError::NotFound)));
    drop(vault);
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(vault.list_cases().unwrap().is_empty());
}

#[test]
fn drafts_and_chat_are_stored_tagged_and_approval_is_tracked() {
    let (_dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let msg = vault
        .add_message(
            &case,
            "kindergarten",
            ChatRole::User,
            "[גננת] סיפרה ש[ילד]…",
            &["שם הילד".into()],
            false,
        )
        .unwrap();
    assert_eq!(vault.messages(&case, "kindergarten").unwrap(), vec![msg]);

    let p = vault
        .add_draft(
            &case,
            "kindergarten",
            "[ילד] מתקשה במעברים.",
            Author::Ai,
            &["input:1".into()],
        )
        .unwrap();
    assert_eq!(p.status, DraftStatus::Proposed);
    vault
        .set_draft_status(&case, &p.id, DraftStatus::Approved)
        .unwrap();
    assert_eq!(
        vault.approved_sections(&case).unwrap(),
        vec!["kindergarten".to_owned()]
    );
    let summary = &vault.list_cases().unwrap()[0];
    assert_eq!(summary.child_name.as_deref(), Some("נועם"));
    assert_eq!(summary.approved_sections, vec!["kindergarten".to_owned()]);
}

#[test]
fn secrets_are_encrypted_and_survive_relock() {
    let (dir, mut vault, _) = new_vault();
    vault
        .set_secret("anthropic_api_key", "sk-ant-test-not-real")
        .unwrap();
    drop(vault);
    assert!(!contains(&all_bytes(dir.path()), "sk-ant-test-not-real"));
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert_eq!(
        vault
            .secret("anthropic_api_key")
            .unwrap()
            .as_deref()
            .map(String::as_str),
        Some("sk-ant-test-not-real")
    );
}

#[test]
fn password_change_and_recovery_rotation() {
    let (dir, mut vault, old_recovery) = new_vault();
    let new_password = "חתול כחול ישן על הספה";
    vault
        .rekey_password(PASSWORD, new_password, Argon2Params::TEST)
        .unwrap();
    assert!(vault
        .rekey_password(
            "סיסמה שגויה לגמרי",
            "עוד סיסמה ארוכה וטובה",
            Argon2Params::TEST
        )
        .is_err());
    let new_recovery = vault.rotate_recovery_key(new_password).unwrap().to_string();
    assert!(vault.check_recovery_key(&new_recovery));
    assert!(!vault.check_recovery_key(&old_recovery));
    drop(vault);
    assert!(Vault::unlock_with_password(dir.path(), PASSWORD).is_err());
    assert!(Vault::unlock_with_password(dir.path(), new_password).is_ok());
    assert!(Vault::unlock_with_recovery(dir.path(), &old_recovery).is_err());
    assert!(Vault::unlock_with_recovery(dir.path(), &new_recovery).is_ok());
}

#[test]
fn audit_log_records_events_and_detects_tampering() {
    let (dir, mut vault, _) = new_vault();
    noam(&mut vault);
    let events: Vec<String> = vault
        .audit_entries(10)
        .unwrap()
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert!(events.contains(&"case_created".to_owned()));
    assert!(events.contains(&"identities_changed".to_owned()));
    for e in vault.audit_entries(50).unwrap() {
        assert!(
            !e.meta.contains("נועם") && !e.meta.contains("TEST-0001"),
            "audit must hold metadata only"
        );
    }
    drop(vault);

    // Roll the header's anchor back: the next unlock must report it.
    let header_path = dir.path().join("vault.header");
    let mut header: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&header_path).unwrap()).unwrap();
    header["audit_anchor"]["seq"] = serde_json::json!(1);
    fs::write(&header_path, serde_json::to_string(&header).unwrap()).unwrap();
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(!vault.integrity().header_ok || !vault.integrity().audit_ok);
    assert_eq!(
        vault.audit_entries(1).unwrap()[0].event,
        AuditEvent::Unlock.as_str()
    );
}

#[test]
fn a_vault_copied_elsewhere_still_needs_the_secret() {
    let (dir, mut vault, _) = new_vault();
    noam(&mut vault);
    drop(vault);
    let copy = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(dir.path()).unwrap().flatten() {
        fs::copy(entry.path(), copy.path().join(entry.file_name())).unwrap();
    }
    assert!(Vault::unlock_with_password(copy.path(), "ניחוש ארוך אבל שגוי").is_err());
    assert!(Vault::unlock_with_password(copy.path(), PASSWORD).is_ok());
}

#[test]
fn structured_data_behind_a_material_is_sealed_and_survives_relock() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let input = vault
        .add_input(
            &case,
            InputKind::TestScores,
            "WPPSI-IV",
            "הבנה מילולית: 112",
        )
        .unwrap();
    assert_eq!(vault.input_data(&case, &input.id).unwrap(), None);
    vault
        .set_input_data(&case, &input.id, r#"{"note":"עבד לאט ובדייקנות"}"#)
        .unwrap();
    assert!(matches!(
        vault.set_input_data(&case, "missing", "{}"),
        Err(VaultError::NotFound)
    ));
    drop(vault);
    assert!(!contains(&all_bytes(dir.path()), "עבד לאט ובדייקנות"));
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert_eq!(
        vault.input_data(&case, &input.id).unwrap().as_deref(),
        Some(r#"{"note":"עבד לאט ובדייקנות"}"#)
    );
}

#[test]
fn where_a_material_goes_is_sealed_and_survives_relock() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let a = vault
        .add_input(&case, InputKind::Intake, "אינטייק", "הלך בגיל שנה.")
        .unwrap();
    let b = vault
        .add_input(&case, InputKind::Kindergarten, "שיחה עם הגננת", "משחק לבד.")
        .unwrap();
    assert_eq!(vault.input_routing(&case, &a.id).unwrap(), None);
    let routing = r#"{"suggestion":null,"added":["סעיף-שנבחר-ביד"],"removed":[]}"#;
    vault.set_input_routing(&case, &a.id, routing).unwrap();
    assert!(matches!(
        vault.set_input_routing(&case, "missing", "{}"),
        Err(VaultError::NotFound)
    ));
    drop(vault);
    assert!(!contains(&all_bytes(dir.path()), "סעיף-שנבחר-ביד"));
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert_eq!(
        vault.input_routing(&case, &a.id).unwrap().as_deref(),
        Some(routing)
    );
    // Each material has its own; nothing leaks to its neighbour.
    assert_eq!(vault.input_routing(&case, &b.id).unwrap(), None);
}
