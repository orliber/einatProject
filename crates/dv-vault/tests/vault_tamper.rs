//! What an attacker with the vault folder (or a backup file) can and cannot do: read nothing,
//! open nothing without the secret, and change nothing without it being refused or reported.
//! Fabricated data only.

#![allow(clippy::string_slice)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use dv_domain::{
    Author, CaseMeta, ChatRole, FoundName, IdentityInput, IdentitySource, InputKind, Role,
};
use dv_vault::{peek_backup, Argon2Params, AuditEvent, Practitioner, Secret, Vault, VaultError};
use proptest::prelude::*;

const PASSWORD: &str = "כלב ירוק רץ מהר בגינה";
const OTHER_PASSWORD: &str = "ענן סגול שט מעל הים";

/// A fabricated marker that is written through every path that stores text.
const CANARY: &str = "CANARY-7f3e-קנרית-בדויה";
const CANARY_PARTS: [&str; 3] = ["CANARY-7f3e", "קנרית", "בדויה"];

fn new_vault() -> (tempfile::TempDir, Vault, String) {
    let dir = tempfile::tempdir().unwrap();
    let created = Vault::create(dir.path(), PASSWORD, Argon2Params::TEST).unwrap();
    let recovery = created.recovery_key.to_string();
    (dir, created.vault, recovery)
}

/// Every file in the folder, by name (the vault folder is flat).
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn assert_no_canary(what: &str, bytes: &[u8]) {
    for needle in std::iter::once(CANARY).chain(CANARY_PARTS) {
        assert!(
            !contains(bytes, needle.as_bytes()),
            "{needle} is readable in {what}"
        );
        assert!(
            !contains(bytes, &utf16le(needle)),
            "{needle} (UTF-16) is readable in {what}"
        );
    }
}

fn header_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("vault.header")).unwrap()).unwrap()
}

fn write_header_json(dir: &Path, header: &serde_json::Value) {
    fs::write(
        dir.join("vault.header"),
        serde_json::to_vec_pretty(header).unwrap(),
    )
    .unwrap();
}

fn password_slot(header: &mut serde_json::Value) -> &mut serde_json::Value {
    header["slots"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["kind"] == "password")
        .unwrap()
}

/// The canary goes in through every way the app stores text; none of it may be readable in
/// any file of the folder, while the vault is open (write-ahead logs included), after it is
/// locked, or in a backup.
#[test]
fn a_canary_written_through_every_path_never_reaches_disk_in_the_clear() {
    let (dir, mut vault, _) = new_vault();
    let case = vault
        .create_case(&CaseMeta {
            code: CANARY.into(),
            ..CaseMeta::default()
        })
        .unwrap();
    vault
        .set_identities(
            &case,
            &[IdentityInput {
                id: None,
                role: Role::Child,
                value: CANARY.into(),
                aliases: vec![CANARY.into()],
            }],
        )
        .unwrap();
    vault
        .add_found_names(
            &case,
            &[FoundName {
                value: format!("{CANARY}-found"),
                role: Role::Teacher,
                source: IdentitySource::Auto,
                reason: CANARY.into(),
            }],
        )
        .unwrap();
    let input = vault
        .add_input(&case, InputKind::Kindergarten, CANARY, CANARY)
        .unwrap();
    vault.set_input_data(&case, &input.id, CANARY).unwrap();
    vault.set_input_routing(&case, &input.id, CANARY).unwrap();
    vault
        .add_message(
            &case,
            "background",
            ChatRole::User,
            CANARY,
            &[CANARY.into()],
            false,
        )
        .unwrap();
    let draft = vault
        .add_draft(&case, "background", CANARY, Author::Ai, &[CANARY.into()])
        .unwrap();
    vault
        .edit_draft(&case, &draft.id, &format!("{CANARY} edited"))
        .unwrap();
    vault
        .add_transmission(&case, "background", "00", "model", CANARY)
        .unwrap();
    vault
        .save_consultation(None, Some(&case), &serde_json::json!([CANARY]).to_string())
        .unwrap();
    vault
        .save_consultation(None, None, &serde_json::json!([CANARY]).to_string())
        .unwrap();
    vault.create_folder(None, CANARY).unwrap();
    vault
        .set_practitioner(&Practitioner {
            names: vec![CANARY.into()],
        })
        .unwrap();
    vault.set_secret("api_key", CANARY).unwrap();
    vault.set_setting("canary_setting", CANARY).unwrap();
    vault
        .save_style_source(None, &serde_json::json!({ "text": CANARY }).to_string())
        .unwrap();
    vault
        .add_style_profile("draft", &serde_json::json!({ "text": CANARY }).to_string())
        .unwrap();

    // Everything reads back (the canary really went in)...
    assert_eq!(vault.case_meta(&case).unwrap().code, CANARY);
    assert_eq!(vault.secret("api_key").unwrap().unwrap().as_str(), CANARY);
    // ...and nothing of it is on disk, open or locked.
    for (name, bytes) in snapshot(dir.path()) {
        assert_no_canary(&format!("{name} (vault open)"), &bytes);
    }
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("canary.vaultbak");
    vault.write_backup(&dest).unwrap();
    assert_no_canary("the backup", &fs::read(&dest).unwrap());
    vault.lock().unwrap();
    for (name, bytes) in snapshot(dir.path()) {
        assert_no_canary(&format!("{name} (locked)"), &bytes);
    }
}

/// A wrong guess opens nothing, writes nothing (not even a log entry an attacker could use to
/// grow or probe the files) and never echoes the guess back in the error.
#[test]
fn a_failed_unlock_changes_nothing_on_disk_and_reveals_nothing() {
    let (dir, vault, _) = new_vault();
    vault.lock().unwrap();
    let before = snapshot(dir.path());
    let guess = "ניחוש בדוי ארוך מאוד";
    let wrong_password = Vault::unlock_with_password(dir.path(), guess).unwrap_err();
    let other_kit = dv_vault::recovery::format(&dv_vault::crypto::Key32::random().unwrap());
    let wrong_kit = Vault::unlock_with_recovery(dir.path(), &other_kit).unwrap_err();
    let typo_kit = Vault::unlock_with_recovery(dir.path(), "ZZZZ-not-a-kit").unwrap_err();
    assert!(matches!(wrong_password, VaultError::WrongSecret));
    assert!(matches!(wrong_kit, VaultError::WrongSecret));
    assert!(matches!(typo_kit, VaultError::RecoveryKeyInvalid));
    assert!(Vault::verify_password(dir.path(), guess).is_err());
    for e in [&wrong_password, &wrong_kit, &typo_kit] {
        let shown = format!("{e} {e:?}");
        assert!(
            !shown.contains(guess) && !shown.contains(&other_kit[..9]),
            "{shown}"
        );
    }
    assert_eq!(
        snapshot(dir.path()),
        before,
        "a failed unlock wrote to disk"
    );
}

/// A key slot is bound to its vault (the vault id is in its AAD): a password slot copied from
/// another vault does not open this one, even with the same password, and changing the vault
/// id in the header disables every slot.
#[test]
fn a_key_slot_cannot_be_moved_to_another_vault() {
    let (dir_a, a, _) = new_vault();
    let (dir_b, b, recovery_b) = new_vault();
    a.lock().unwrap();
    b.lock().unwrap();

    let mut header_a = header_json(dir_a.path());
    let original_b = header_json(dir_b.path());
    let mut header_b = original_b.clone();
    *password_slot(&mut header_b) = password_slot(&mut header_a).clone();
    write_header_json(dir_b.path(), &header_b);
    assert!(matches!(
        Vault::unlock_with_password(dir_b.path(), PASSWORD),
        Err(VaultError::WrongSecret)
    ));

    // Vault B with its own slots, renamed to vault A's id: none of them opens.
    let mut renamed = original_b.clone();
    renamed["vault_id"] = header_a["vault_id"].clone();
    write_header_json(dir_b.path(), &renamed);
    assert!(matches!(
        Vault::unlock_with_password(dir_b.path(), PASSWORD),
        Err(VaultError::WrongSecret)
    ));
    assert!(matches!(
        Vault::unlock_with_recovery(dir_b.path(), &recovery_b),
        Err(VaultError::WrongSecret)
    ));
    write_header_json(dir_b.path(), &original_b);
    assert!(Vault::unlock_with_recovery(dir_b.path(), &recovery_b).is_ok());
}

/// Every single-character change to the wrapped master key in the password slot is refused;
/// so is a changed salt or a lowered (or raised) Argon2 cost: an attacker cannot make the
/// password cheaper to guess by editing the header.
#[test]
fn the_password_slot_cannot_be_edited_or_its_cost_lowered() {
    let (dir, vault, _) = new_vault();
    vault.lock().unwrap();
    let original = header_json(dir.path());
    let wrapped = password_slot(&mut original.clone())["wrapped_mk"]
        .as_str()
        .unwrap()
        .to_owned();
    for i in 0..wrapped.len() {
        let mut chars: Vec<char> = wrapped.chars().collect();
        chars[i] = if chars[i] == '0' { '1' } else { '0' };
        let mut header = original.clone();
        password_slot(&mut header)["wrapped_mk"] = chars.into_iter().collect::<String>().into();
        write_header_json(dir.path(), &header);
        assert!(
            Vault::unlock_with_password(dir.path(), PASSWORD).is_err(),
            "opened with wrapped_mk changed at {i}"
        );
    }
    let mut header = original.clone();
    let salt = password_slot(&mut header)["salt"]
        .as_str()
        .unwrap()
        .to_owned();
    let flipped = format!(
        "{}{}",
        if salt.starts_with('0') { '1' } else { '0' },
        &salt[1..]
    );
    password_slot(&mut header)["salt"] = flipped.into();
    write_header_json(dir.path(), &header);
    assert!(matches!(
        Vault::unlock_with_password(dir.path(), PASSWORD),
        Err(VaultError::WrongSecret)
    ));
    for (field, value) in [("m_kib", 8), ("t", 2), ("p", 2)] {
        let mut header = original.clone();
        password_slot(&mut header)["params"][field] = value.into();
        write_header_json(dir.path(), &header);
        assert!(
            matches!(
                Vault::unlock_with_password(dir.path(), PASSWORD),
                Err(VaultError::WrongSecret | VaultError::Crypto(_))
            ),
            "opened with {field} = {value}"
        );
    }
    // Put back, it opens: the refusals above were the edits, not a broken vault.
    write_header_json(dir.path(), &original);
    assert!(Vault::unlock_with_password(dir.path(), PASSWORD).is_ok());
}

/// A header from a newer program version is refused before anything is derived or written.
#[test]
fn a_header_of_an_unknown_format_is_refused_without_writing() {
    let (dir, vault, _) = new_vault();
    vault.lock().unwrap();
    let mut header = header_json(dir.path());
    header["format"] = 3.into();
    write_header_json(dir.path(), &header);
    let before = snapshot(dir.path());
    assert!(matches!(
        Vault::unlock_with_password(dir.path(), PASSWORD),
        Err(VaultError::Corrupt(m)) if m.contains("format")
    ));
    assert_eq!(snapshot(dir.path()), before);
    fs::write(dir.path().join("vault.header"), b"{ not json").unwrap();
    assert!(matches!(
        Vault::unlock_with_password(dir.path(), PASSWORD),
        Err(VaultError::Corrupt(_))
    ));
}

/// Each database has its own key: a database file copied in from another vault, or two of a
/// vault's own files swapped, do not open.
#[test]
fn database_files_cannot_be_swapped_between_vaults_or_roles() {
    let remove_logs = |dir: &Path| {
        for name in ["main.db", "identity.db", "audit.db"] {
            for suffix in ["-wal", "-shm"] {
                let _ = fs::remove_file(dir.join(format!("{name}{suffix}")));
            }
        }
    };
    let (dir_a, a, _) = new_vault();
    let (dir_b, b, _) = new_vault();
    a.lock().unwrap();
    b.lock().unwrap();
    remove_logs(dir_a.path());
    remove_logs(dir_b.path());

    let original_main = fs::read(dir_a.path().join("main.db")).unwrap();
    fs::copy(dir_b.path().join("main.db"), dir_a.path().join("main.db")).unwrap();
    assert!(Vault::unlock_with_password(dir_a.path(), PASSWORD).is_err());

    fs::write(dir_a.path().join("main.db"), &original_main).unwrap();
    fs::copy(
        dir_a.path().join("identity.db"),
        dir_a.path().join("main.db"),
    )
    .unwrap();
    assert!(Vault::unlock_with_password(dir_a.path(), PASSWORD).is_err());

    fs::write(dir_a.path().join("main.db"), &original_main).unwrap();
    assert!(Vault::unlock_with_password(dir_a.path(), PASSWORD).is_ok());
}

/// Locking records why, in one log entry, and the chain stays intact across the lock.
#[test]
fn locking_records_the_reason_and_keeps_the_log_intact() {
    let (dir, vault, _) = new_vault();
    vault.lock_because(Some("sleep")).unwrap();
    let vault = Vault::unlock_with_password(dir.path(), PASSWORD).unwrap();
    let entries = vault.audit_entries(2).unwrap();
    assert_eq!(entries[0].event, AuditEvent::Unlock.as_str());
    assert_eq!(entries[1].event, AuditEvent::Lock.as_str());
    assert!(entries[1].meta.contains("sleep"), "{}", entries[1].meta);
    assert!(vault.integrity().audit_ok && vault.integrity().header_ok);
    assert!(vault.audit_intact().unwrap());
}

// ---------------------------------------------------------------- backups

fn backup_of(vault: &mut Vault) -> (tempfile::TempDir, Vec<u8>) {
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("b.vaultbak");
    vault.write_backup(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    (out, bytes)
}

fn header_end(backup: &[u8]) -> usize {
    12 + u32::from_le_bytes(backup[8..12].try_into().unwrap()) as usize
}

/// A backup with a different header (JSON edited, length field fixed up).
fn with_header(backup: &[u8], edit: impl Fn(&mut serde_json::Value)) -> Vec<u8> {
    let end = header_end(backup);
    let mut header: serde_json::Value = serde_json::from_slice(&backup[12..end]).unwrap();
    edit(&mut header);
    let json = serde_json::to_vec_pretty(&header).unwrap();
    let mut out = backup[..8].to_vec();
    out.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&backup[end..]);
    out
}

/// A backup cut off anywhere (a full disk, an interrupted copy) is refused, and the failed
/// restore leaves nothing in the target folder.
#[test]
fn a_truncated_backup_is_refused_and_leaves_nothing_behind() {
    let (_dir, mut vault, _) = new_vault();
    vault.create_case(&CaseMeta::default()).unwrap();
    let (_out, bytes) = backup_of(&mut vault);
    let end = header_end(&bytes);
    let mut lengths: Vec<usize> = (0..end + 8 + 2).step_by(5).collect();
    lengths.extend((end + 8..bytes.len()).step_by(4093));
    lengths.extend([end, end + 8, end + 8 + 29, bytes.len() - 1]);
    let target = tempfile::tempdir().unwrap();
    for len in lengths {
        let cut = &bytes[..len];
        assert!(
            Vault::restore_backup(cut, target.path(), Secret::Password(PASSWORD)).is_err(),
            "restored from the first {len} bytes"
        );
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0, "at {len}");
    }
}

/// Wrong file version, a header from a newer program, extra bytes at the end, or an absurd
/// header length: all refused, none panics.
#[test]
fn a_backup_of_another_version_or_with_extra_bytes_is_refused() {
    let (_dir, mut vault, _) = new_vault();
    let (_out, bytes) = backup_of(&mut vault);
    let target = tempfile::tempdir().unwrap();
    let restore = |b: &[u8]| Vault::restore_backup(b, target.path(), Secret::Password(PASSWORD));

    let mut version = bytes.clone();
    version[7] = 2;
    assert!(matches!(restore(&version), Err(VaultError::Corrupt(_))));
    assert!(peek_backup(&version).is_err());

    let newer = with_header(&bytes, |h| h["format"] = 3.into());
    assert!(matches!(restore(&newer), Err(VaultError::Corrupt(m)) if m.contains("format")));
    assert!(peek_backup(&newer).is_err());

    let mut longer = bytes.clone();
    longer.push(0);
    assert!(matches!(restore(&longer), Err(VaultError::Integrity(_))));

    let mut huge = bytes.clone();
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(restore(&huge).is_err());
    assert!(peek_backup(&huge).is_err());

    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    // The untouched backup still restores: the refusals were the edits.
    assert!(restore(&bytes).is_ok());
}

/// A backup's key slots can be edited (the header is public), but an edited slot never opens,
/// and an edited unauthenticated field (the log anchor) is never reported as intact.
#[test]
fn an_edited_header_inside_a_backup_is_refused_or_reported() {
    let (_dir, mut vault, _) = new_vault();
    vault.create_case(&CaseMeta::default()).unwrap();
    let (_out, bytes) = backup_of(&mut vault);

    let edited_slot = with_header(&bytes, |h| {
        let slot = password_slot(h);
        let w = slot["wrapped_mk"].as_str().unwrap().to_owned();
        let last = if w.ends_with('0') { '1' } else { '0' };
        slot["wrapped_mk"] = format!("{}{last}", &w[..w.len() - 1]).into();
    });
    let target = tempfile::tempdir().unwrap();
    assert!(matches!(
        Vault::restore_backup(&edited_slot, target.path(), Secret::Password(PASSWORD)),
        Err(VaultError::WrongSecret)
    ));

    let anchor = with_header(&bytes, |h| h["audit_anchor"]["seq"] = 1.into());
    let (_restored, check) =
        Vault::restore_backup(&anchor, target.path(), Secret::Password(PASSWORD)).unwrap();
    assert!(!check.integrity_ok, "edited header reported intact");
}

/// A backup carries the header of the day it was written: after a password change it opens
/// with the old password (and old recovery kit), not the new ones. This is what lets a backup
/// be restored without any new secret; it also means a leaked old password still opens old
/// backups, which is why they should be replaced after a change (STANDARDS §5.6).
#[test]
fn a_backup_opens_with_the_secrets_that_were_current_when_it_was_written() {
    let (_dir, mut vault, old_kit) = new_vault();
    vault.create_case(&CaseMeta::default()).unwrap();
    let (_out, bytes) = backup_of(&mut vault);
    vault
        .rekey_password(
            Secret::Password(PASSWORD),
            OTHER_PASSWORD,
            Argon2Params::TEST,
        )
        .unwrap();
    let new_kit = vault
        .rotate_recovery_key(Secret::Password(OTHER_PASSWORD))
        .unwrap()
        .to_string();

    let try_restore = |secret: Secret<'_>| {
        let target = tempfile::tempdir().unwrap();
        Vault::restore_backup(&bytes, target.path(), secret).map(|(_, check)| check)
    };
    assert!(matches!(
        try_restore(Secret::Password(OTHER_PASSWORD)),
        Err(VaultError::WrongSecret)
    ));
    assert!(matches!(
        try_restore(Secret::Recovery(&new_kit)),
        Err(VaultError::WrongSecret)
    ));
    assert_eq!(try_restore(Secret::Password(PASSWORD)).unwrap().cases, 1);
    assert_eq!(try_restore(Secret::Recovery(&old_kit)).unwrap().cases, 1);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Whatever file is offered as a backup, reading its public part never panics, and only
    /// the real thing restores.
    #[test]
    fn arbitrary_bytes_offered_as_a_backup_never_panic(
        tail in proptest::collection::vec(any::<u8>(), 0..512),
        with_magic in any::<bool>(),
    ) {
        let mut bytes = if with_magic { b"DVBAK\0\0\x01".to_vec() } else { Vec::new() };
        bytes.extend(tail);
        prop_assert!(peek_backup(&bytes).is_err());
        let target = tempfile::tempdir().unwrap();
        prop_assert!(
            Vault::restore_backup(&bytes, target.path(), Secret::Password(PASSWORD)).is_err()
        );
        prop_assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    }
}
