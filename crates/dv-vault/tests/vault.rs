//! End-to-end tests of the vault's security properties. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use dv_domain::{
    Author, CaseMeta, ChatRole, DraftStatus, FoundName, IdentityInput, IdentitySource, InputKind,
    Role,
};
use dv_vault::{Argon2Params, AuditEvent, Secret, Vault, VaultError};

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

/// A name the filter hid on its own becomes an identity of the case with its source and
/// reason, survives a relock, keeps them through an edit of the list, and can be removed.
#[test]
fn found_names_are_kept_with_their_source() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let found = |value: &str, role, source| FoundName {
        value: value.into(),
        role,
        source,
        reason: "אחרי 'הגננת'".into(),
    };
    let added = vault
        .add_found_names(
            &case,
            &[
                found("אסתי", Role::Teacher, IdentitySource::Auto),
                found("נועם", Role::Other, IdentitySource::Auto),
                found("רקס", Role::Other, IdentitySource::Metadata),
                found("אסתי", Role::Other, IdentitySource::Auto),
            ],
        )
        .unwrap();
    // "נועם" is already the child; the second "אסתי" is the same name.
    assert_eq!(added.len(), 2);
    assert_eq!(added[0].tag, "[גננת_2]");
    assert_eq!(added[1].tag, "[אדם_1]");
    vault.lock().unwrap();

    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    let ids = vault.identities(&case).unwrap();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids[0].source, IdentitySource::Manual);
    assert_eq!(ids[2].source, IdentitySource::Auto);
    assert_eq!(ids[2].reason, "אחרי 'הגננת'");
    assert_eq!(ids[3].source, IdentitySource::Metadata);

    // Saving the list keeps every source, also with a changed role (a new tag).
    let mut edit: Vec<IdentityInput> = ids
        .iter()
        .map(|i| IdentityInput {
            id: Some(i.id.clone()),
            role: i.role,
            value: i.value.clone(),
            aliases: i.aliases.clone(),
        })
        .collect();
    edit[3].role = Role::Relative;
    let after = vault.set_identities(&case, &edit).unwrap();
    assert_eq!(after[2].source, IdentitySource::Auto);
    let relative = after.iter().find(|i| i.role == Role::Relative).unwrap();
    assert_eq!(relative.source, IdentitySource::Metadata);
    assert_eq!(relative.tag, "[קרוב_משפחה_1]");

    vault.remove_identity(&case, &after[2].id).unwrap();
    assert!(!vault
        .identities(&case)
        .unwrap()
        .iter()
        .any(|i| i.value == "אסתי"));
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
fn earlier_wordings_of_a_paragraph_are_kept_sealed_and_can_be_brought_back() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let p = vault
        .add_draft(
            &case,
            "kindergarten",
            "[ילד] מתקשה במעברים.",
            Author::Ai,
            &[],
        )
        .unwrap();
    assert!(vault.draft_versions(&case, &p.id).unwrap().is_empty());

    // Claude rewrites its proposal, then she edits it: both earlier wordings are kept.
    assert!(vault
        .rewrite_proposal(&case, &p.id, "[ילד] מתקשה מעט במעברים בין פעילויות.", &[])
        .unwrap());
    vault
        .edit_draft(&case, &p.id, "[ילד] מתקשה במעברים בין פעילויות בגן.")
        .unwrap();
    // Saving the same text again keeps nothing new.
    vault
        .edit_draft(&case, &p.id, "[ילד] מתקשה במעברים בין פעילויות בגן.")
        .unwrap();
    let versions = vault.draft_versions(&case, &p.id).unwrap();
    let texts: Vec<&str> = versions.iter().map(|v| v.text_tagged.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "[ילד] מתקשה מעט במעברים בין פעילויות.",
            "[ילד] מתקשה במעברים."
        ]
    );
    assert!(versions.iter().all(|v| v.author == Author::Ai));
    assert_eq!(
        vault.drafts_with_versions(&case).unwrap(),
        vec![p.id.clone()]
    );

    // A new wording she approves takes the old one's history with it.
    vault
        .propose_rewording(&case, &p.id, "בגן, [ילד] מתקשה במעברים.", &[])
        .unwrap();
    let new_id = vault
        .drafts(&case, "kindergarten")
        .unwrap()
        .into_iter()
        .find(|d| d.replaces.as_deref() == Some(p.id.as_str()))
        .unwrap()
        .id;
    vault
        .set_draft_status(&case, &new_id, DraftStatus::Approved)
        .unwrap();
    let chain = vault.draft_versions(&case, &new_id).unwrap();
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0].id, p.id);
    assert_eq!(chain[0].author, Author::User);

    // Bringing back Claude's first wording: approved, Claude's again, and the current one kept.
    let first = chain.last().unwrap().id.clone();
    vault.restore_draft_version(&case, &new_id, &first).unwrap();
    let now = vault
        .drafts(&case, "kindergarten")
        .unwrap()
        .into_iter()
        .find(|d| d.id == new_id)
        .unwrap();
    assert_eq!(now.text_tagged, "[ילד] מתקשה במעברים.");
    assert_eq!(now.status, DraftStatus::Approved);
    assert_eq!(now.author, Author::Ai);
    assert_eq!(
        vault.draft_versions(&case, &new_id).unwrap()[0].text_tagged,
        "בגן, [ילד] מתקשה במעברים."
    );
    assert!(matches!(
        vault.restore_draft_version(&case, &new_id, "not-a-version"),
        Err(VaultError::NotFound)
    ));

    // Sealed at rest: no wording is readable in the files.
    let bytes = all_bytes(dir.path());
    let needle = "מתקשה מעט במעברים".as_bytes();
    assert!(!bytes.windows(needle.len()).any(|w| w == needle));

    // Erasing the case erases its versions.
    vault.delete_case(&case).unwrap();
    assert!(vault.draft_versions(&case, &new_id).is_err());
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
        .rekey_password(Secret::Password(PASSWORD), new_password, Argon2Params::TEST)
        .unwrap();
    assert!(vault
        .rekey_password(
            Secret::Password("סיסמה שגויה לגמרי"),
            "עוד סיסמה ארוכה וטובה",
            Argon2Params::TEST
        )
        .is_err());
    let new_recovery = vault
        .rotate_recovery_key(Secret::Password(new_password))
        .unwrap()
        .to_string();
    assert!(vault.check_recovery_key(&new_recovery));
    assert!(!vault.check_recovery_key(&old_recovery));
    drop(vault);
    assert!(Vault::unlock_with_password(dir.path(), PASSWORD).is_err());
    assert!(Vault::unlock_with_password(dir.path(), new_password).is_ok());
    assert!(Vault::unlock_with_recovery(dir.path(), &old_recovery).is_err());
    assert!(Vault::unlock_with_recovery(dir.path(), &new_recovery).is_ok());
}

#[test]
fn a_forgotten_password_is_replaced_with_the_recovery_kit() {
    let (dir, vault, recovery) = new_vault();
    drop(vault);
    let mut vault = Vault::unlock_with_recovery(dir.path(), &recovery).unwrap();
    let new_password = "חתול כחול ישן על הספה";
    vault
        .rekey_password(
            Secret::Recovery(&recovery),
            new_password,
            Argon2Params::TEST,
        )
        .unwrap();
    assert!(vault
        .rekey_password(
            Secret::Recovery("AAAA-BBBB"),
            new_password,
            Argon2Params::TEST
        )
        .is_err());
    drop(vault);
    assert!(Vault::unlock_with_password(dir.path(), new_password).is_ok());
    // The kit still works until a new one is issued.
    assert!(Vault::unlock_with_recovery(dir.path(), &recovery).is_ok());
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

#[test]
fn folders_nest_hide_their_names_and_never_take_cases_with_them() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let top = vault.create_folder(None, "משפחת-כהן-תיקייה").unwrap();
    let sub = vault.create_folder(Some(&top.id), "2026").unwrap();
    vault.move_case(&case, Some(&sub.id)).unwrap();
    assert!(matches!(
        vault.create_folder(Some("missing"), "x"),
        Err(VaultError::NotFound)
    ));
    assert!(matches!(
        vault.move_case(&case, Some("missing")),
        Err(VaultError::NotFound)
    ));
    // No folder inside itself, directly or through a subfolder.
    assert!(matches!(
        vault.move_folder(&top.id, Some(&sub.id)),
        Err(VaultError::Refused(_))
    ));
    assert!(matches!(
        vault.move_folder(&top.id, Some(&top.id)),
        Err(VaultError::Refused(_))
    ));
    vault.rename_folder(&sub.id, "שנת 2026").unwrap();

    drop(vault);
    assert!(!contains(&all_bytes(dir.path()), "משפחת-כהן-תיקייה"));
    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    let names: Vec<String> = vault
        .folders()
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["משפחת-כהן-תיקייה", "שנת 2026"]);
    assert_eq!(
        vault.list_cases().unwrap()[0].folder_id.as_deref(),
        Some(sub.id.as_str())
    );

    // Deleting a folder moves what is inside one level up.
    vault.delete_folder(&sub.id).unwrap();
    assert_eq!(
        vault.list_cases().unwrap()[0].folder_id.as_deref(),
        Some(top.id.as_str())
    );
    vault.delete_folder(&top.id).unwrap();
    assert_eq!(vault.list_cases().unwrap()[0].folder_id, None);
    assert!(vault.folders().unwrap().is_empty());
}

#[test]
fn the_recycle_bin_keeps_a_case_until_it_is_restored_or_erased() {
    let (_dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let folder = vault.create_folder(None, "תיקייה").unwrap();
    vault.move_case(&case, Some(&folder.id)).unwrap();
    vault.trash_case(&case).unwrap();
    assert!(vault.list_cases().unwrap().is_empty());
    let trash = vault.list_trash().unwrap();
    assert_eq!(trash.len(), 1);
    assert!(trash[0].deleted_at.is_some());
    // Still readable: nothing is erased while it waits in the bin.
    assert!(!vault.identities(&case).unwrap().is_empty());
    assert!(vault.trashed_before(i64::MAX).unwrap().contains(&case));
    assert!(vault.trashed_before(0).unwrap().is_empty());

    // Its folder was deleted meanwhile: it comes back to the top level.
    vault.delete_folder(&folder.id).unwrap();
    vault.restore_case(&case).unwrap();
    let back = vault.list_cases().unwrap();
    assert_eq!((back.len(), back[0].folder_id.clone()), (1, None));
    assert!(vault.list_trash().unwrap().is_empty());
}

#[test]
fn the_password_can_be_checked_without_opening_the_vault_again() {
    let (dir, _vault, _) = new_vault();
    assert!(Vault::verify_password(dir.path(), PASSWORD).is_ok());
    assert!(matches!(
        Vault::verify_password(dir.path(), "ניחוש ארוך אבל שגוי"),
        Err(VaultError::WrongSecret)
    ));
}

#[test]
fn a_backup_restores_on_a_new_computer_with_the_password_or_the_recovery_kit() {
    let (dir, mut vault, recovery) = new_vault();
    let case = noam(&mut vault);
    let other = vault
        .create_case(&CaseMeta {
            code: "TEST-0002".into(),
            ..CaseMeta::default()
        })
        .unwrap();
    vault.trash_case(&other).unwrap();
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("backup.vaultbak");
    let info = vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    assert_eq!(info.bytes, bytes.len() as u64);
    // Its date and vault can be read before the password is asked for.
    let peek = dv_vault::peek_backup(&bytes).unwrap();
    assert_eq!(
        (peek.created_at, peek.vault_id.as_str()),
        (info.created_at, vault.vault_id())
    );
    // Nothing readable in the backup file, and no scratch left next to the vault.
    for needle in ["נועם", "מיכל", "מתקשה", "TEST-0001", "SQLite format"] {
        assert!(
            !contains(&bytes, needle),
            "{needle} is readable in the backup"
        );
    }
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(leftovers.is_empty());
    assert_eq!(
        vault.audit_entries(1).unwrap()[0].event,
        AuditEvent::BackupWritten.as_str()
    );

    for secret in [Secret::Password(PASSWORD), Secret::Recovery(&recovery)] {
        let target = tempfile::tempdir().unwrap();
        let (restored, check) = Vault::restore_backup(&bytes, target.path(), secret).unwrap();
        assert_eq!(check.cases, 2);
        assert!(check.integrity_ok);
        assert_eq!(check.created_at, info.created_at);
        assert_eq!(restored.list_cases().unwrap()[0].id, case);
        assert_eq!(restored.list_trash().unwrap()[0].id, other);
        let who = restored.identities(&case).unwrap();
        assert!(who.iter().any(|i| i.value == "נועם"));
        assert_eq!(
            restored.audit_entries(1).unwrap()[0].event,
            AuditEvent::Restored.as_str()
        );
        drop(restored);
        // The restored vault is a normal vault from now on.
        let reopened = Vault::unlock_with_password(target.path(), PASSWORD).unwrap();
        assert!(reopened.integrity().header_ok && reopened.integrity().audit_ok);
    }
}

#[test]
fn a_backup_opens_only_with_the_right_secret_and_unchanged() {
    let (_dir, mut vault, _) = new_vault();
    noam(&mut vault);
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("backup.vaultbak");
    vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();

    let target = tempfile::tempdir().unwrap();
    assert!(matches!(
        Vault::restore_backup(
            &bytes,
            target.path(),
            Secret::Password("ניחוש ארוך אבל שגוי")
        ),
        Err(VaultError::WrongSecret)
    ));
    // One changed byte in the sealed part, or a changed backup time: it does not open.
    let mut flipped = bytes.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    assert!(Vault::restore_backup(&flipped, target.path(), Secret::Password(PASSWORD)).is_err());
    let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut retimed = bytes.clone();
    retimed[12 + header_len] ^= 1;
    assert!(Vault::restore_backup(&retimed, target.path(), Secret::Password(PASSWORD)).is_err());
    // A header swapped in from another vault (same password): it does not open either.
    let (_other_dir, mut other, _) = new_vault();
    let other_dest = out.path().join("other.vaultbak");
    other.write_backup(&other_dest).unwrap();
    let other_bytes = fs::read(&other_dest).unwrap();
    let other_len = u32::from_le_bytes(other_bytes[8..12].try_into().unwrap()) as usize;
    let mut swapped = other_bytes[..12 + other_len].to_vec();
    swapped.extend_from_slice(&bytes[12 + header_len..]);
    assert!(Vault::restore_backup(&swapped, target.path(), Secret::Password(PASSWORD)).is_err());
    assert!(
        Vault::restore_backup(b"not a backup", target.path(), Secret::Password(PASSWORD)).is_err()
    );
    // Failed attempts leave nothing behind.
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

#[test]
fn restoring_never_overwrites_a_vault() {
    let (dir, mut vault, _) = new_vault();
    noam(&mut vault);
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("backup.vaultbak");
    vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    assert!(matches!(
        Vault::restore_backup(&bytes, dir.path(), Secret::Password(PASSWORD)),
        Err(VaultError::AlreadyExists)
    ));
}

#[test]
fn the_restore_drill_checks_a_backup_without_touching_the_vault() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("backup.vaultbak");
    vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    // Work goes on after the backup.
    vault.trash_case(&case).unwrap();

    let check = vault
        .check_backup(&bytes, Secret::Password(PASSWORD))
        .unwrap();
    assert_eq!(check.cases, 1);
    assert!(check.integrity_ok);
    assert!(
        vault.list_cases().unwrap().is_empty(),
        "the live vault is untouched"
    );
    assert_eq!(
        vault.audit_entries(1).unwrap()[0].event,
        AuditEvent::BackupChecked.as_str()
    );
    let names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.starts_with('.')), "{names:?}");
    assert!(vault
        .check_backup(&bytes, Secret::Password("ניחוש ארוך אבל שגוי"))
        .is_err());
}

#[test]
fn scratch_folders_left_by_a_crash_are_removed_at_unlock() {
    let (dir, vault, _) = new_vault();
    drop(vault);
    let stale = dir.path().join(".backup-0123");
    fs::create_dir(&stale).unwrap();
    fs::write(stale.join("main.db"), b"encrypted copy").unwrap();
    let _vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(!stale.exists());
}

#[test]
fn the_log_reads_in_pages_and_its_chain_is_checked_on_demand() {
    let (_dir, mut vault, _) = new_vault();
    noam(&mut vault);
    noam(&mut vault);
    let all = vault.audit_entries(1000).unwrap();
    let first = vault.audit_page(None, 3).unwrap();
    let rest = vault.audit_page(Some(first[2].seq), 1000).unwrap();
    assert_eq!(first.len() + rest.len(), all.len());
    assert!(first
        .iter()
        .chain(&rest)
        .map(|e| e.seq)
        .eq(all.iter().map(|e| e.seq)));
    assert!(vault.audit_intact().unwrap());
}

#[test]
fn consultations_are_sealed_and_go_with_their_case() {
    let (dir, mut vault, _) = new_vault();
    let case = noam(&mut vault);
    let general = vault
        .save_consultation(None, None, r#"[{"text":"שאלה כללית על WISC"}]"#)
        .unwrap();
    let about = vault
        .save_consultation(None, Some(&case), r#"[{"text":"נועם מתקשה במעברים"}]"#)
        .unwrap();
    // Saving again replaces the turns; it cannot move a conversation to another case.
    vault
        .save_consultation(Some(&about), Some(&case), r#"[{"text":"נועם, המשך"}]"#)
        .unwrap();
    let all = vault.consultations().unwrap();
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .any(|c| c.id == about && c.turns_json.contains("המשך")));
    drop(vault);
    let bytes = all_bytes(dir.path());
    assert!(!contains(&bytes, "WISC") && !contains(&bytes, "מתקשה"));

    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    vault.trash_case(&case).unwrap();
    assert_eq!(
        vault.consultations().unwrap().len(),
        1,
        "hidden while in the bin"
    );
    vault.restore_case(&case).unwrap();
    vault.delete_case(&case).unwrap();
    let left = vault.consultations().unwrap();
    assert_eq!((left.len(), left[0].id.as_str()), (1, general.as_str()));
    vault.delete_consultation(&general).unwrap();
    assert!(vault.consultations().unwrap().is_empty());
}

#[test]
fn style_sources_and_profiles_are_sealed_versioned_and_erasable() {
    let (dir, mut vault, _) = new_vault();
    let a = vault
        .save_style_source(None, r#"{"text":"[ילד] מגיב היטב לתיווך של המבוגר"}"#)
        .unwrap();
    let b = vault
        .save_style_source(None, r#"{"text":"ניכר קושי בוויסות"}"#)
        .unwrap();
    assert_eq!(vault.style_sources().unwrap().len(), 2);
    vault
        .save_style_source(Some(&a), r#"{"text":"גרסה מעודכנת"}"#)
        .unwrap();
    assert!(vault
        .style_source(&a)
        .unwrap()
        .unwrap()
        .json
        .contains("מעודכנת"));
    assert!(matches!(
        vault.save_style_source(Some("missing"), "{}"),
        Err(VaultError::NotFound)
    ));

    let (_, v1) = vault
        .add_style_profile("active", r#"{"items":["אחד"]}"#)
        .unwrap();
    let (_, v2) = vault
        .add_style_profile("draft", r#"{"items":["טיוטה"]}"#)
        .unwrap();
    let (_, v3) = vault
        .add_style_profile("active", r#"{"items":["שלוש"]}"#)
        .unwrap();
    assert_eq!((v1, v2, v3), (1, 2, 3));
    let profiles = vault.style_profiles().unwrap();
    assert_eq!(profiles[0].version, 3, "newest first");
    assert_eq!(
        profiles.iter().filter(|p| p.status == "active").count(),
        1,
        "approving a version retires the one before"
    );
    assert!(vault.add_style_profile("weird", "{}").is_err());
    vault.delete_style_drafts().unwrap();
    assert!(vault
        .style_profiles()
        .unwrap()
        .iter()
        .all(|p| p.status != "draft"));

    vault
        .save_style_learning(r#"{"pairs":[["מראה","מפגין",3]]}"#)
        .unwrap();
    vault
        .save_style_learning(r#"{"pairs":[["מראה","מפגין",4]]}"#)
        .unwrap();
    assert!(vault.style_learning().unwrap().unwrap().contains('4'));

    vault.delete_style_source(&b).unwrap();
    assert_eq!(vault.style_sources().unwrap().len(), 1);
    assert!(matches!(
        vault.delete_style_source(&b),
        Err(VaultError::NotFound)
    ));
    let events: Vec<String> = vault
        .audit_entries(50)
        .unwrap()
        .into_iter()
        .map(|e| e.event)
        .collect();
    for e in [
        "style_source_added",
        "style_source_deleted",
        "style_profile_approved",
    ] {
        assert!(events.iter().any(|x| x == e), "{e} is in the log");
    }
    vault.reset_style().unwrap();
    assert!(vault.style_profiles().unwrap().is_empty());
    assert!(vault.style_learning().unwrap().is_none());
    vault.lock().unwrap();

    let bytes = all_bytes(dir.path());
    for secret in ["מגיב היטב לתיווך", "גרסה מעודכנת", "מפגין", "שלוש"]
    {
        assert!(
            !contains(&bytes, secret),
            "{secret} found in plaintext on disk"
        );
    }
}
