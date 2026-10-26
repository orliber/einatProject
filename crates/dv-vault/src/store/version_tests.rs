//! D-104: the vault remembers the newest version of the program that opened it, and an older
//! version refuses it without writing anything. Fabricated data only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rusqlite::params;

use super::{aad, sealed_app_version, Secret, Vault, APP_VERSION_SECRET, HEADER_FILE};
use crate::crypto::seal_str;
use crate::db::MAIN_MIGRATIONS;
use crate::header::{VaultHeader, FORMAT};
use crate::password::Argon2Params;
use crate::version::APP_VERSION;
use crate::{peek_backup, VaultError};

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";
const NEWER: &str = "99.0.0";

fn new_vault() -> (tempfile::TempDir, Vault, String) {
    let dir = tempfile::tempdir().unwrap();
    let created = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST).unwrap();
    let kit = created.recovery_key.to_string();
    (dir, created.vault, kit)
}

/// What a newer version leaves behind when it opens the vault: its version in the header
/// (signed) and sealed in the main database.
fn opened_by(vault: &mut Vault, version: &str) {
    record_in_header(vault, version);
    seal_version(vault, version);
}

fn record_in_header(vault: &mut Vault, version: &str) {
    vault.header.app_version = Some(version.to_owned());
    vault.header.sign(&vault.keys.header).unwrap();
    vault.header.write(&vault.dir).unwrap();
}

fn seal_version(vault: &Vault, version: &str) {
    let sealed = seal_str(
        &vault.keys.settings,
        &aad("secrets", "value", APP_VERSION_SECRET, ""),
        version,
    )
    .unwrap();
    vault
        .main
        .execute(
            "INSERT INTO secrets (key, value_enc) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value_enc = excluded.value_enc",
            params![APP_VERSION_SECRET, sealed],
        )
        .unwrap();
}

fn sealed(vault: &Vault) -> Option<String> {
    sealed_app_version(&vault.main, &vault.keys.settings).unwrap()
}

/// Every file of the vault folder, byte for byte (the databases' `-wal`/`-shm` side files
/// aside: SQLite may make them on any read).
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with("-wal") && !n.ends_with("-shm"))
        .map(|n| {
            let bytes = fs::read(dir.join(&n)).unwrap();
            (n, bytes)
        })
        .collect()
}

fn newer(r: Result<Vault, VaultError>) -> String {
    match r {
        Err(VaultError::NewerVersion(v)) => v,
        Err(e) => panic!("not refused as newer: {e:?}"),
        Ok(_) => panic!("an older program opened a vault a newer one opened"),
    }
}

#[test]
fn a_new_vault_records_this_version_in_the_header_and_inside() {
    let (dir, vault, _) = new_vault();
    assert_eq!(vault.app_version(), Some(APP_VERSION));
    assert_eq!(sealed(&vault).as_deref(), Some(APP_VERSION));
    drop(vault);
    assert_eq!(
        Vault::opened_by(dir.path()).unwrap().as_deref(),
        Some(APP_VERSION)
    );
    assert_eq!(Vault::opened_by_newer(dir.path()), None);
    let again = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(again.integrity().header_ok);
}

/// Every vault made before D-104 has no version anywhere: the first opening by this version
/// records it in both places, and the header still checks out.
#[test]
fn a_vault_from_before_records_this_version_when_it_opens() {
    let (dir, mut vault, _) = new_vault();
    vault.header.app_version = None;
    vault.header.format = 1;
    vault.header.sign(&vault.keys.header).unwrap();
    vault.header.write(&vault.dir).unwrap();
    vault
        .main
        .execute("DELETE FROM secrets WHERE key = ?1", [APP_VERSION_SECRET])
        .unwrap();
    drop(vault);
    let header = fs::read_to_string(dir.path().join(HEADER_FILE)).unwrap();
    assert!(!header.contains("app_version"), "as a header from before");
    assert!(header.contains("\"format\": 1"), "as a header from before");
    assert_eq!(Vault::opened_by(dir.path()).unwrap(), None);

    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(vault.integrity().header_ok, "{:?}", vault.integrity());
    assert_eq!(vault.header.format, FORMAT);
    assert_eq!(vault.app_version(), Some(APP_VERSION));
    assert_eq!(sealed(&vault).as_deref(), Some(APP_VERSION));
    drop(vault);
    assert_eq!(
        Vault::opened_by(dir.path()).unwrap().as_deref(),
        Some(APP_VERSION)
    );
}

/// An older version recorded never lowers the newer one, in either place.
#[test]
fn an_older_version_never_replaces_a_newer_one() {
    let (dir, mut vault, _) = new_vault();
    record_in_header(&mut vault, "0.0.1");
    seal_version(&vault, "0.0.1");
    drop(vault);
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert_eq!(vault.app_version(), Some(APP_VERSION));
    assert_eq!(sealed(&vault).as_deref(), Some(APP_VERSION));
}

/// The header says a newer version opened it: refused before the password is even tried (a
/// wrong one is not counted as wrong), by every way in, and nothing in the folder changes.
#[test]
fn a_newer_version_in_the_header_is_refused_before_the_password() {
    let (dir, mut vault, kit) = new_vault();
    opened_by(&mut vault, NEWER);
    drop(vault);
    let before = files(dir.path());

    assert_eq!(Vault::opened_by_newer(dir.path()).as_deref(), Some(NEWER));
    assert_eq!(
        newer(Vault::unlock_with_password(dir.path(), PASSWORD)),
        NEWER
    );
    assert_eq!(
        newer(Vault::unlock_with_password(
            dir.path(),
            "not the password at all"
        )),
        NEWER
    );
    assert_eq!(newer(Vault::unlock_with_recovery(dir.path(), &kit)), NEWER);
    assert!(matches!(
        Vault::verify_password(dir.path(), PASSWORD),
        Err(VaultError::NewerVersion(_))
    ));
    assert_eq!(
        files(dir.path()),
        before,
        "an older program wrote to the vault"
    );
}

/// Someone lowers (or removes) the version in the header while the vault is locked: the
/// password opens the vault, the sealed version still refuses it, and nothing is written.
#[test]
fn a_newer_version_sealed_inside_holds_when_the_header_is_edited() {
    for edited in [Some("0.0.1"), None] {
        let (dir, mut vault, _) = new_vault();
        opened_by(&mut vault, NEWER);
        drop(vault);
        let path = dir.path().join(HEADER_FILE);
        let mut header: VaultHeader = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        header.app_version = edited.map(str::to_owned);
        fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();
        assert_eq!(Vault::opened_by_newer(dir.path()), None);
        let before = files(dir.path());

        assert_eq!(
            newer(Vault::unlock_with_password(dir.path(), PASSWORD)),
            NEWER
        );
        assert_eq!(files(dir.path()), before, "{edited:?}: written to");
    }
}

/// R7 (D-057): a newer version that migrated the databases and then failed to open never
/// recorded itself, so the version before still opens the vault (migrations only add).
#[test]
fn a_newer_version_that_never_opened_the_vault_does_not_shut_out_this_one() {
    let (dir, vault, _) = new_vault();
    let later = i64::try_from(MAIN_MIGRATIONS.len() + 1).unwrap();
    vault
        .main
        .execute_batch("CREATE TABLE from_a_newer_version (x INTEGER);")
        .unwrap();
    vault
        .main
        .execute("INSERT INTO schema_version (version) VALUES (?1)", [later])
        .unwrap();
    drop(vault);
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(vault.integrity().header_ok);
    assert_eq!(vault.app_version(), Some(APP_VERSION));
}

/// A version this program cannot read, in the header, counts as newer (fail closed).
#[test]
fn a_version_it_cannot_read_is_refused() {
    for odd in [
        serde_json::json!("1.0"),
        serde_json::json!("x.y.z"),
        serde_json::json!(9),
        serde_json::json!({"v": 1}),
    ] {
        let (dir, vault, _) = new_vault();
        drop(vault);
        let path = dir.path().join(HEADER_FILE);
        let mut header: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        header["app_version"] = odd.clone();
        fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();
        assert_eq!(
            Vault::opened_by_newer(dir.path()).as_deref(),
            Some(""),
            "{odd}"
        );
        assert_eq!(
            newer(Vault::unlock_with_password(dir.path(), PASSWORD)),
            "",
            "{odd}"
        );
    }
}

/// Even a header in a format this version does not know says "a newer version" when a newer
/// version is named in it, rather than "damaged".
#[test]
fn a_newer_format_named_by_a_newer_version_says_newer() {
    let (dir, vault, _) = new_vault();
    drop(vault);
    let path = dir.path().join(HEADER_FILE);
    let mut header: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    header["format"] = serde_json::json!(FORMAT + 1);
    header["app_version"] = serde_json::json!(NEWER);
    header["slots"] = serde_json::json!({"something": "new"});
    fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();
    assert_eq!(
        newer(Vault::unlock_with_password(dir.path(), PASSWORD)),
        NEWER
    );
}

/// A backup a newer version wrote is refused by an older one: not read, and not restored
/// (nothing is left in the folder it was to go to).
#[test]
fn a_backup_from_a_newer_version_is_refused() {
    let (dir, mut vault, _) = new_vault();
    opened_by(&mut vault, NEWER);
    let out = dir.path().join("b.vaultbak");
    vault.write_backup(&out).unwrap();
    drop(vault);
    let bytes = fs::read(&out).unwrap();
    assert!(matches!(
        peek_backup(&bytes),
        Err(VaultError::NewerVersion(_))
    ));
    let target = tempfile::tempdir().unwrap();
    let restored = Vault::restore_backup(&bytes, target.path(), Secret::Password(PASSWORD));
    assert!(matches!(restored, Err(VaultError::NewerVersion(_))));
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}

/// A backup this version wrote carries its version, and opens again here.
#[test]
fn a_backup_carries_the_version_and_restores_here() {
    let (dir, mut vault, _) = new_vault();
    let out = dir.path().join("b.vaultbak");
    vault.write_backup(&out).unwrap();
    drop(vault);
    let target = tempfile::tempdir().unwrap();
    let (restored, _) = Vault::restore_backup(
        &fs::read(&out).unwrap(),
        target.path(),
        Secret::Password(PASSWORD),
    )
    .unwrap();
    assert_eq!(restored.app_version(), Some(APP_VERSION));
    assert_eq!(sealed(&restored).as_deref(), Some(APP_VERSION));
}

/// Review of #153: a header changed while the vault was locked is never signed again by the
/// program. Not by the log entries of the opening, not by catching up an anchor one entry
/// behind, not by recording the version: it fails its check at every opening.
#[test]
fn a_changed_header_is_never_signed_again() {
    for lagging in [false, true] {
        let (dir, vault, _) = new_vault();
        if lagging {
            // An entry the header's anchor does not name yet (an append cut short).
            crate::audit::append(
                &vault.audit,
                &vault.keys.audit_mac,
                0,
                crate::AuditEvent::Lock,
                None,
                &serde_json::json!({}),
            )
            .unwrap();
        }
        drop(vault);
        let path = dir.path().join(HEADER_FILE);
        let mut header: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        header["created_at"] = serde_json::json!(header["created_at"].as_i64().unwrap() + 1);
        fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();

        for opening in 1..=3 {
            let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
            assert!(
                !vault.integrity().header_ok,
                "lagging {lagging}, opening {opening}: the changed header passed"
            );
            assert!(
                vault.integrity().audit_ok,
                "lagging {lagging}, opening {opening}: {:?}",
                vault.integrity()
            );
        }
        // Nor in a backup of it: restored, it still fails.
        let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
        let out = dir.path().join("b.vaultbak");
        vault.write_backup(&out).unwrap();
        drop(vault);
        let target = tempfile::tempdir().unwrap();
        let (restored, check) = Vault::restore_backup(
            &fs::read(&out).unwrap(),
            target.path(),
            Secret::Password(PASSWORD),
        )
        .unwrap();
        assert!(!restored.integrity().header_ok && !check.integrity_ok);
    }
}

/// The Windows Hello slot an older version signed (D-054) leaves the header, and the header
/// stays signed: only a header changed by someone else is left unsigned.
#[test]
fn a_retired_hello_slot_leaves_a_signed_header_signed() {
    let (dir, mut vault, _) = new_vault();
    vault
        .header
        .slots
        .push(crate::header::KeySlot::WindowsHello {
            credential: "il.diagnosticvault.test".into(),
            challenge: "00".into(),
            wrapped_mk: "00".into(),
        });
    vault.header.sign(&vault.keys.header).unwrap();
    vault.header.write(&vault.dir).unwrap();
    drop(vault);
    let mut vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(vault.integrity().header_ok && vault.has_hello_slot());
    vault.remove_hello_slot().unwrap();
    drop(vault);
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    assert!(vault.integrity().header_ok, "{:?}", vault.integrity());
    assert!(!vault.has_hello_slot());
}
