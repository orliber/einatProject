//! The password slot bound to this computer (D-076): a copied vault folder cannot be guessed
//! at elsewhere, the kit still opens it anywhere, and a backup still opens with the password
//! alone. The key sealed to Windows is a plain random key here (no Windows in these tests);
//! "another computer" is another key. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::fs;
use std::path::Path;

use dv_domain::CaseMeta;
use dv_vault::crypto::Key32;
use dv_vault::{Argon2Params, NewSlot, Secret, Vault, VaultError};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";
const NEW_PASSWORD: &str = "חתול כתום ישן על הגדר";

fn header_text(dir: &Path) -> String {
    fs::read_to_string(dir.join("vault.header")).unwrap()
}

fn copy_folder(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

fn with_case(vault: &mut Vault) {
    vault
        .create_case(&CaseMeta {
            code: "TEST-0001".into(),
            ..CaseMeta::default()
        })
        .unwrap();
}

#[test]
fn a_bound_vault_opens_with_the_password_only_on_its_computer_and_with_the_kit_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let here = Key32::random().unwrap();
    let created =
        Vault::create_here(dir.path(), PASSWORD, Argon2Params::TEST, Some(&here)).unwrap();
    let kit = created.recovery_key.to_string();
    assert!(created.vault.password_bound());
    created.vault.lock().unwrap();
    assert!(header_text(dir.path()).contains("\"bound\": true"));
    assert!(Vault::password_is_bound(dir.path()).unwrap());

    // Without this computer's key the password is not even tried.
    assert!(matches!(
        Vault::unlock_with_password(dir.path(), PASSWORD),
        Err(VaultError::OtherComputer)
    ));
    // Another computer's key: the password alone is not enough.
    let elsewhere = Key32::random().unwrap();
    assert!(matches!(
        Vault::unlock_with_password_here(dir.path(), PASSWORD, Some(&elsewhere)),
        Err(VaultError::WrongSecret)
    ));
    // This computer's key without the password opens nothing either.
    assert!(matches!(
        Vault::unlock_with_password_here(dir.path(), "סיסמה אחרת לגמרי כאן", Some(&here)),
        Err(VaultError::WrongSecret)
    ));
    Vault::verify_password_here(dir.path(), PASSWORD, Some(&here)).unwrap();
    Vault::unlock_with_password_here(dir.path(), PASSWORD, Some(&here))
        .unwrap()
        .lock()
        .unwrap();
    Vault::unlock_with_recovery(dir.path(), &kit)
        .unwrap()
        .lock()
        .unwrap();
}

#[test]
fn an_unbound_vault_is_bound_at_its_next_opening_and_the_log_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let created = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST).unwrap();
    let before = header_text(dir.path());
    assert!(!before.contains("bound"), "an unbound header is as before");
    created.vault.lock().unwrap();

    let here = Key32::random().unwrap();
    let vault = Vault::unlock_with_password_here(dir.path(), PASSWORD, Some(&here)).unwrap();
    assert!(vault.password_bound());
    assert!(vault.integrity().header_ok);
    let events: Vec<String> = vault
        .audit_entries(5)
        .unwrap()
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert!(events.contains(&"password_bound".to_owned()), "{events:?}");
    vault.lock().unwrap();

    assert_ne!(header_text(dir.path()), before);
    assert!(matches!(
        Vault::unlock_with_password(dir.path(), PASSWORD),
        Err(VaultError::OtherComputer)
    ));
    let again = Vault::unlock_with_password_here(dir.path(), PASSWORD, Some(&here)).unwrap();
    assert!(again.integrity().header_ok);
    again.lock().unwrap();
}

#[test]
fn a_copied_folder_on_another_computer_opens_with_the_kit_and_a_new_password_binds_there() {
    let dir = tempfile::tempdir().unwrap();
    let here = Key32::random().unwrap();
    let created =
        Vault::create_here(dir.path(), PASSWORD, Argon2Params::TEST, Some(&here)).unwrap();
    let kit = created.recovery_key.to_string();
    let mut vault = created.vault;
    with_case(&mut vault);
    vault.lock().unwrap();

    let other = tempfile::tempdir().unwrap();
    copy_folder(dir.path(), other.path());
    let there = Key32::random().unwrap();
    assert!(matches!(
        Vault::unlock_with_password_here(other.path(), PASSWORD, Some(&there)),
        Err(VaultError::WrongSecret)
    ));
    let mut vault = Vault::unlock_with_recovery(other.path(), &kit).unwrap();
    assert_eq!(vault.list_cases().unwrap().len(), 1);
    let slot = NewSlot::password_here(
        &vault.header_copy(),
        Secret::Recovery(&kit),
        NEW_PASSWORD,
        Argon2Params::TEST,
        Some(&there),
    )
    .unwrap();
    vault.apply_slot(slot).unwrap();
    assert!(vault.password_bound());
    vault.lock().unwrap();
    Vault::unlock_with_password_here(other.path(), NEW_PASSWORD, Some(&there))
        .unwrap()
        .lock()
        .unwrap();
    assert!(Vault::unlock_with_password_here(other.path(), NEW_PASSWORD, Some(&here)).is_err());
}

#[test]
fn a_backup_of_a_bound_vault_opens_with_the_password_on_a_new_computer_and_binds_there() {
    let dir = tempfile::tempdir().unwrap();
    let here = Key32::random().unwrap();
    let created =
        Vault::create_here(dir.path(), PASSWORD, Argon2Params::TEST, Some(&here)).unwrap();
    let kit = created.recovery_key.to_string();
    let mut vault = created.vault;
    with_case(&mut vault);
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("b.vaultbak");
    vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    assert!(!dv_vault::peek_backup(&bytes).unwrap().vault_id.is_empty());
    assert!(
        !String::from_utf8_lossy(&bytes).contains("\"bound\""),
        "the backup carries the password slot that opens anywhere"
    );
    // The drill opens it with the password alone, and with the kit.
    dv_vault::drill_backup(dir.path(), &bytes, Secret::Password(PASSWORD)).unwrap();
    dv_vault::drill_backup(dir.path(), &bytes, Secret::Recovery(&kit)).unwrap();
    vault.lock().unwrap();

    let new_computer = tempfile::tempdir().unwrap();
    let target = new_computer.path().join("vault");
    let (mut restored, check) =
        Vault::restore_backup(&bytes, &target, Secret::Password(PASSWORD)).unwrap();
    assert!(check.integrity_ok);
    assert_eq!(check.cases, 1);
    assert!(!restored.password_bound());
    let there = Key32::random().unwrap();
    restored.bind_password(PASSWORD, &there).unwrap();
    assert!(restored.password_bound());
    restored.lock().unwrap();
    Vault::unlock_with_password_here(&target, PASSWORD, Some(&there))
        .unwrap()
        .lock()
        .unwrap();
    assert!(matches!(
        Vault::unlock_with_password_here(&target, PASSWORD, Some(&here)),
        Err(VaultError::WrongSecret)
    ));
}

#[test]
fn after_a_new_password_a_backup_opens_with_the_new_one_and_not_the_old() {
    let dir = tempfile::tempdir().unwrap();
    let here = Key32::random().unwrap();
    let created =
        Vault::create_here(dir.path(), PASSWORD, Argon2Params::TEST, Some(&here)).unwrap();
    let mut vault = created.vault;
    let slot = NewSlot::password_here(
        &vault.header_copy(),
        Secret::PasswordHere {
            password: PASSWORD,
            computer: &here,
        },
        NEW_PASSWORD,
        Argon2Params::TEST,
        Some(&here),
    )
    .unwrap();
    // The bound slot needs this computer's key as the proof too.
    assert!(matches!(
        NewSlot::password(
            &vault.header_copy(),
            Secret::Password(PASSWORD),
            NEW_PASSWORD,
            Argon2Params::TEST
        ),
        Err(VaultError::OtherComputer)
    ));
    vault.apply_slot(slot).unwrap();
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("b.vaultbak");
    vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    assert!(dv_vault::drill_backup(dir.path(), &bytes, Secret::Password(PASSWORD)).is_err());
    dv_vault::drill_backup(dir.path(), &bytes, Secret::Password(NEW_PASSWORD)).unwrap();
}

#[test]
fn the_slot_kept_for_backups_is_never_in_the_header_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let here = Key32::random().unwrap();
    let created =
        Vault::create_here(dir.path(), PASSWORD, Argon2Params::TEST, Some(&here)).unwrap();
    created.vault.lock().unwrap();
    let header = header_text(dir.path());
    // One password slot, the bound one: no slot on disk opens with the password alone.
    assert_eq!(header.matches("\"kind\": \"password\"").count(), 1);
    assert!(header.contains("\"bound\": true"));
}
