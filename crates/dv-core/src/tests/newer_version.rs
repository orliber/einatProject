//! D-104: a vault a newer version of the program opened. This version says so on the lock
//! screen, before anything is typed; it never takes it as a wrong password, never asks for the
//! kit, and writes nothing to the vault. Fabricated data only.

use std::collections::BTreeMap;
use std::path::Path;

use super::*;

const NEWER: &str = "99.0.0";

/// A vault, locked, whose header says a newer version opened it last. Returns the kit.
fn opened_by_newer(dir: &Path) -> String {
    let mut core = Core::for_tests(dir, None);
    let kit = core.create_vault(PASSWORD).unwrap().recovery_key;
    core.lock();
    let path = dir.join("vault.header");
    let mut header: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    header["app_version"] = json!(NEWER);
    std::fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();
    kit
}

/// Every file in the vault folder, byte for byte (SQLite's side files aside).
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with("-wal") && !n.ends_with("-shm"))
        .map(|n| {
            let bytes = std::fs::read(dir.join(&n)).unwrap();
            (n, bytes)
        })
        .collect()
}

fn refused_as_newer(r: Result<AppStatus, CoreError>) -> UiError {
    let ui = r
        .expect_err("a vault a newer version opened was opened")
        .to_ui();
    assert_eq!(ui.code, "newer_version", "{ui:?}");
    assert_eq!(ui.action, ErrorAction::Dismiss);
    ui
}

#[test]
fn the_lock_screen_knows_before_anything_is_typed() {
    let dir = tempfile::tempdir().unwrap();
    opened_by_newer(dir.path());
    let mut core = Core::for_tests(dir.path(), None);
    let status = core.status();
    assert!(status.vault_exists && !status.unlocked);
    assert_eq!(status.newer_version.as_deref(), Some(NEWER));
    // Not in the status the UI gets for a vault this version opens.
    let plain = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(plain.path(), None);
    other.create_vault(PASSWORD).unwrap();
    assert_eq!(other.status().newer_version, None);
    other.lock();
    assert_eq!(other.status().newer_version, None);
    let json = serde_json::to_value(other.status()).unwrap();
    assert!(json.get("newer_version").is_none(), "{json}");
}

#[test]
fn neither_the_password_nor_the_kit_is_tried_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let kit = opened_by_newer(dir.path());
    let before = files(dir.path());
    let mut core = Core::for_tests(dir.path(), None);

    let ui = refused_as_newer(core.unlock(PASSWORD));
    assert!(
        ui.message
            .starts_with("הכספת נפתחה בגרסה חדשה יותר (99.0.0). צריך להתקין את הגרסה החדשה."),
        "{}",
        ui.message
    );
    assert!(
        ui.message.contains("לא צריך את ערכת השחזור"),
        "{}",
        ui.message
    );
    // A wrong password is not counted as one: no wait, again and again.
    for _ in 0..4 {
        refused_as_newer(core.unlock("not the password at all"));
    }
    refused_as_newer(core.unlock_with_recovery(&kit));
    refused_as_newer(core.unlock(PASSWORD));
    assert!(core.status().newer_version.is_some());
    assert!(!dir.path().join("unlock.failures").exists());
    assert_eq!(files(dir.path()), before, "this version wrote to the vault");
}

/// Once the newer version is installed (here: the header no longer names a newer one), the
/// same password opens it again, as if nothing had happened.
#[test]
fn the_same_password_opens_it_in_the_version_that_may() {
    let dir = tempfile::tempdir().unwrap();
    opened_by_newer(dir.path());
    let path = dir.path().join("vault.header");
    let mut header: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    header["app_version"] = json!(dv_vault::version::APP_VERSION);
    std::fs::write(&path, serde_json::to_vec_pretty(&header).unwrap()).unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    assert_eq!(core.status().newer_version, None);
    let status = core.unlock(PASSWORD).unwrap();
    assert!(status.unlocked);
    assert!(status.integrity_warning.is_none(), "{status:?}");
}

/// A backup a newer version wrote is refused here, with the same words.
#[test]
fn a_backup_from_a_newer_version_is_not_restored() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = Core::for_tests(dir.path(), None);
    core.create_vault(PASSWORD).unwrap();
    let drive = tempfile::tempdir().unwrap();
    let done = core.write_backup(&drive.path().join("b")).unwrap();
    let bytes = std::fs::read(&done.path).unwrap();
    // The header sits after the 8-byte magic and its 4-byte length; a newer one says so.
    let len = usize::try_from(u32::from_le_bytes(bytes[8..12].try_into().unwrap())).unwrap();
    let mut header: Value = serde_json::from_slice(&bytes[12..12 + len]).unwrap();
    header["app_version"] = json!(NEWER);
    let edited = serde_json::to_vec_pretty(&header).unwrap();
    let mut newer = bytes[..8].to_vec();
    newer.extend_from_slice(&u32::try_from(edited.len()).unwrap().to_le_bytes());
    newer.extend_from_slice(&edited);
    newer.extend_from_slice(&bytes[12 + len..]);

    let fresh = tempfile::tempdir().unwrap();
    let mut other = Core::for_tests(fresh.path(), None);
    let ui = other
        .stage_backup(newer, "b.vaultbak".into())
        .expect_err("a newer backup was staged")
        .to_ui();
    assert_eq!(ui.code, "newer_version", "{ui:?}");
    assert!(!Vault::exists(fresh.path()));
}
