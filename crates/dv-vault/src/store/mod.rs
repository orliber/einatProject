//! The unlocked vault. Dropping it closes the databases and wipes every key.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dv_domain::{
    assign_tag, Author, CaseInput, CaseMeta, CaseSummary, ChatMessage, ChatRole, DraftParagraph,
    DraftStatus, Folder, Identity, IdentityInput, InputKind, Role, Transmission,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::audit::{self, AuditEntry, AuditEvent, ChainStatus};
use crate::crypto::{
    hex, hkdf, hmac_sha256, open, open_string, random_array, random_id, seal, seal_str, Key32,
};
use crate::db::{migrate, open_encrypted, AUDIT_MIGRATIONS, IDENTITY_MIGRATIONS, MAIN_MIGRATIONS};
use crate::header::{unwrap_mk, wrap_mk, KeySlot, VaultHeader, FORMAT, HEADER_FILE};
use crate::password::{self, Argon2Params};
use crate::{recovery, VaultError};

mod backup;
pub use backup::{peek_backup, BackupCheck, BackupInfo, BackupPeek, BACKUP_EXTENSION};

/// Raw rows as read from SQLite, before decryption.
type IdentityRow = (String, String, String, Vec<u8>, Vec<u8>);
type InputRow = (String, String, i64, Vec<u8>, Vec<u8>);
type MessageRow = (String, String, bool, i64, Vec<u8>, Vec<u8>);
type TransmissionRow = (String, String, i64, String, String, Vec<u8>);
type SummaryRow = (String, i64, i64, Option<String>, Option<i64>);

const MAIN_DB: &str = "main.db";
const IDENTITY_DB: &str = "identity.db";
const AUDIT_DB: &str = "audit.db";

/// Subkeys derived from the master key, one per purpose.
struct Keys {
    header: Key32,
    audit_mac: Key32,
    index: Key32,
    case_wrap: Key32,
    settings: Key32,
    db_main: Key32,
    db_identity: Key32,
    db_audit: Key32,
    /// Seals `.vaultbak` backups.
    backup: Key32,
}

impl Keys {
    fn derive(mk: &Key32) -> Result<Self, VaultError> {
        let d = |purpose: &str| hkdf(mk.as_bytes(), &format!("dv/{purpose}/v1"));
        Ok(Self {
            header: d("header-mac")?,
            audit_mac: d("audit-mac")?,
            index: d("index")?,
            case_wrap: d("case-wrap")?,
            settings: d("settings")?,
            db_main: d("db/main")?,
            db_identity: d("db/identity")?,
            db_audit: d("db/audit")?,
            backup: d("backup")?,
        })
    }
}

/// What unlocking found about the vault's integrity (shown in the UI, never blocks access).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub header_ok: bool,
    pub audit_ok: bool,
    pub detail: Option<String>,
}

/// The practitioner's own identity: hidden like any other name, in every case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Practitioner {
    pub names: Vec<String>,
}

pub struct Created {
    pub vault: Vault,
    /// Shown once, printed, then forgotten by the program.
    pub recovery_key: Zeroizing<String>,
}

impl std::fmt::Debug for Created {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Created { .. }")
    }
}

/// What opens a vault (or a backup of it).
#[derive(Clone, Copy)]
pub enum Secret<'a> {
    Password(&'a str),
    Recovery(&'a str),
}

impl std::fmt::Debug for Secret<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Secret::Password(_) => "Secret::Password(..)",
            Secret::Recovery(_) => "Secret::Recovery(..)",
        })
    }
}

/// The master key, unwrapped from the header's password or recovery slot.
pub(crate) fn master_key(header: &VaultHeader, secret: Secret<'_>) -> Result<Key32, VaultError> {
    match secret {
        Secret::Password(password) => {
            let (salt, params, wrapped) = header
                .slots
                .iter()
                .find_map(|s| match s {
                    KeySlot::Password {
                        salt,
                        params,
                        wrapped_mk,
                    } => Some((salt.clone(), *params, wrapped_mk.clone())),
                    KeySlot::Recovery { .. } => None,
                })
                .ok_or(VaultError::Corrupt("no password slot".to_owned()))?;
            let salt: [u8; 32] = crate::crypto::unhex(&salt)?
                .try_into()
                .map_err(|_| VaultError::Corrupt("salt length".to_owned()))?;
            let kek = password::derive_kek(password, &salt, params)?;
            unwrap_mk(&kek, "password", &header.vault_id, &wrapped)
        }
        Secret::Recovery(typed) => {
            let wrapped = header
                .slots
                .iter()
                .find_map(|s| match s {
                    KeySlot::Recovery { wrapped_mk } => Some(wrapped_mk.clone()),
                    KeySlot::Password { .. } => None,
                })
                .ok_or(VaultError::Corrupt("no recovery slot".to_owned()))?;
            let kek = recovery::derive_kek(&recovery::parse(typed)?)?;
            unwrap_mk(&kek, "recovery", &header.vault_id, &wrapped)
        }
    }
}

pub struct Vault {
    dir: PathBuf,
    header: VaultHeader,
    keys: Keys,
    main: Connection,
    identity: Connection,
    audit: Connection,
    integrity: IntegrityReport,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Binds a sealed value to exactly where it is stored.
fn aad(table: &str, column: &str, row: &str, case_id: &str) -> Vec<u8> {
    format!("dv/v1|{table}|{column}|{row}|{case_id}").into_bytes()
}

fn to_json<T: Serialize>(value: &T) -> Result<String, VaultError> {
    serde_json::to_string(value).map_err(|e| VaultError::Corrupt(e.to_string()))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, VaultError> {
    serde_json::from_str(text).map_err(|e| VaultError::Corrupt(e.to_string()))
}

fn enum_str<T: Serialize>(value: &T) -> Result<String, VaultError> {
    Ok(to_json(value)?.trim_matches('"').to_owned())
}

fn enum_from<T: DeserializeOwned>(text: &str) -> Result<T, VaultError> {
    from_json(&format!("\"{text}\""))
}

impl Vault {
    #[must_use]
    pub fn exists(dir: &Path) -> bool {
        dir.join(HEADER_FILE).exists()
    }

    /// Create a new vault. Returns it unlocked, with the recovery key to print.
    pub fn create(dir: &Path, password: &str, params: Argon2Params) -> Result<Created, VaultError> {
        password::check_policy(password).map_err(VaultError::Policy)?;
        if Self::exists(dir) {
            return Err(VaultError::AlreadyExists);
        }
        fs::create_dir_all(dir)?;
        let vault_id = random_id()?;
        let mk = Key32::random()?;
        let salt = random_array::<32>()?;
        let kek = password::derive_kek(password, &salt, params)?;
        let recovery_key = Key32::random()?;
        let recovery_kek = recovery::derive_kek(&recovery_key)?;
        let mut header = VaultHeader {
            format: FORMAT,
            vault_id: vault_id.clone(),
            created_at: now(),
            slots: vec![
                KeySlot::Password {
                    salt: hex(&salt),
                    params,
                    wrapped_mk: wrap_mk(&kek, "password", &vault_id, &mk)?,
                },
                KeySlot::Recovery {
                    wrapped_mk: wrap_mk(&recovery_kek, "recovery", &vault_id, &mk)?,
                },
            ],
            audit_anchor: None,
            mac: None,
        };
        let keys = Keys::derive(&mk)?;
        header.sign(&keys.header)?;
        header.write(dir)?;
        let mut vault = Self::open_with(dir, header, &mk)?;
        vault.record(AuditEvent::VaultCreated, None, &serde_json::json!({}))?;
        Ok(Created {
            vault,
            recovery_key: recovery::format(&recovery_key),
        })
    }

    /// Check the password without opening the vault again (before an irreversible action).
    pub fn verify_password(dir: &Path, password: &str) -> Result<(), VaultError> {
        Self::password_mk(dir, password).map(|_| ())
    }

    fn password_mk(dir: &Path, password: &str) -> Result<(VaultHeader, Key32), VaultError> {
        let header = VaultHeader::read(dir)?;
        let mk = master_key(&header, Secret::Password(password))?;
        Ok((header, mk))
    }

    pub fn unlock_with_password(dir: &Path, password: &str) -> Result<Self, VaultError> {
        let (header, mk) = Self::password_mk(dir, password)?;
        let mut vault = Self::open_with(dir, header, &mk)?;
        vault.record(
            AuditEvent::Unlock,
            None,
            &serde_json::json!({"method": "password"}),
        )?;
        Ok(vault)
    }

    pub fn unlock_with_recovery(dir: &Path, typed: &str) -> Result<Self, VaultError> {
        let header = VaultHeader::read(dir)?;
        let mk = master_key(&header, Secret::Recovery(typed))?;
        let mut vault = Self::open_with(dir, header, &mk)?;
        vault.record(
            AuditEvent::Unlock,
            None,
            &serde_json::json!({"method": "recovery"}),
        )?;
        Ok(vault)
    }

    fn open_with(dir: &Path, mut header: VaultHeader, mk: &Key32) -> Result<Self, VaultError> {
        let keys = Keys::derive(mk)?;
        backup::remove_stale_scratch(dir);
        let header_ok = header.verify(&keys.header).is_ok();
        let main = open_encrypted(&dir.join(MAIN_DB), &keys.db_main)?;
        migrate(&main, MAIN_MIGRATIONS)?;
        let identity = open_encrypted(&dir.join(IDENTITY_DB), &keys.db_identity)?;
        migrate(&identity, IDENTITY_MIGRATIONS)?;
        let audit_conn = open_encrypted(&dir.join(AUDIT_DB), &keys.db_audit)?;
        migrate(&audit_conn, AUDIT_MIGRATIONS)?;

        let (audit_ok, detail) =
            match audit::verify(&audit_conn, &keys.audit_mac, header.audit_anchor.as_ref())? {
                ChainStatus::Intact { .. } => (true, None),
                ChainStatus::AnchorLagging { head } => {
                    header.audit_anchor = Some(head);
                    header.sign(&keys.header)?;
                    header.write(dir)?;
                    (true, None)
                }
                ChainStatus::Broken { reason } => (false, Some(reason)),
            };
        let integrity = IntegrityReport {
            header_ok,
            audit_ok,
            detail: detail.or_else(|| (!header_ok).then(|| "vault header was modified".to_owned())),
        };
        let mut vault = Self {
            dir: dir.to_path_buf(),
            header,
            keys,
            main,
            identity,
            audit: audit_conn,
            integrity,
        };
        if !vault.integrity.header_ok || !vault.integrity.audit_ok {
            let reason = vault.integrity.detail.clone().unwrap_or_default();
            vault.record(
                AuditEvent::IntegrityWarning,
                None,
                &serde_json::json!({ "reason": reason }),
            )?;
        }
        Ok(vault)
    }

    #[must_use]
    pub fn integrity(&self) -> &IntegrityReport {
        &self.integrity
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Random, and public (it is in the header): tells a backup of this vault from another's.
    #[must_use]
    pub fn vault_id(&self) -> &str {
        &self.header.vault_id
    }

    /// Lock explicitly (records the event); dropping the vault also wipes the keys.
    pub fn lock(mut self) -> Result<(), VaultError> {
        self.record(AuditEvent::Lock, None, &serde_json::json!({}))
    }

    /// Replace the password slot. Needs a fresh proof of the current password, because the
    /// master key is not kept in memory after unlocking (only purpose-specific subkeys are).
    pub fn rekey_password(
        &mut self,
        current: &str,
        new_password: &str,
        params: Argon2Params,
    ) -> Result<(), VaultError> {
        password::check_policy(new_password).map_err(VaultError::Policy)?;
        let mk = self.unwrap_with_password(current)?;
        let salt = random_array::<32>()?;
        let kek = password::derive_kek(new_password, &salt, params)?;
        let wrapped = wrap_mk(&kek, "password", &self.header.vault_id, &mk)?;
        for slot in &mut self.header.slots {
            if let KeySlot::Password { .. } = slot {
                *slot = KeySlot::Password {
                    salt: hex(&salt),
                    params,
                    wrapped_mk: wrapped.clone(),
                };
            }
        }
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        self.record(AuditEvent::PasswordChanged, None, &serde_json::json!({}))
    }

    /// Issue a new recovery key (the old one stops working). Needs the current password.
    pub fn rotate_recovery_key(
        &mut self,
        current_password: &str,
    ) -> Result<Zeroizing<String>, VaultError> {
        let mk = self.unwrap_with_password(current_password)?;
        let recovery_key = Key32::random()?;
        let kek = recovery::derive_kek(&recovery_key)?;
        let wrapped = wrap_mk(&kek, "recovery", &self.header.vault_id, &mk)?;
        for slot in &mut self.header.slots {
            if let KeySlot::Recovery { .. } = slot {
                *slot = KeySlot::Recovery {
                    wrapped_mk: wrapped.clone(),
                };
            }
        }
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        self.record(AuditEvent::RecoveryKeyRotated, None, &serde_json::json!({}))?;
        Ok(recovery::format(&recovery_key))
    }

    /// True if `typed` opens this vault's recovery slot (used to confirm the printout).
    #[must_use]
    pub fn check_recovery_key(&self, typed: &str) -> bool {
        let Ok(key) = recovery::parse(typed) else {
            return false;
        };
        let Ok(kek) = recovery::derive_kek(&key) else {
            return false;
        };
        self.header.slots.iter().any(|s| match s {
            KeySlot::Recovery { wrapped_mk } => {
                unwrap_mk(&kek, "recovery", &self.header.vault_id, wrapped_mk).is_ok()
            }
            KeySlot::Password { .. } => false,
        })
    }

    fn unwrap_with_password(&self, password: &str) -> Result<Key32, VaultError> {
        for slot in &self.header.slots {
            if let KeySlot::Password {
                salt,
                params,
                wrapped_mk,
            } = slot
            {
                let salt: [u8; 32] = crate::crypto::unhex(salt)?
                    .try_into()
                    .map_err(|_| VaultError::Corrupt("salt length".to_owned()))?;
                let kek = password::derive_kek(password, &salt, *params)?;
                return unwrap_mk(&kek, "password", &self.header.vault_id, wrapped_mk);
            }
        }
        Err(VaultError::Corrupt("no password slot".to_owned()))
    }

    // ---------------------------------------------------------------- audit

    /// Pseudonymous reference to a case for the audit log (no case id or code in the log).
    #[must_use]
    pub fn case_ref(&self, case_id: &str) -> String {
        hex(&hmac_sha256(
            &self.keys.index,
            format!("case|{case_id}").as_bytes(),
        ))[..16]
            .to_owned()
    }

    pub fn record(
        &mut self,
        event: AuditEvent,
        case_id: Option<&str>,
        meta: &serde_json::Value,
    ) -> Result<(), VaultError> {
        let case_ref = case_id.map(|c| self.case_ref(c));
        let anchor = audit::append(
            &self.audit,
            &self.keys.audit_mac,
            now(),
            event,
            case_ref.as_deref(),
            meta,
        )?;
        self.header.audit_anchor = Some(anchor);
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)
    }

    pub fn audit_entries(&self, limit: u32) -> Result<Vec<AuditEntry>, VaultError> {
        audit::recent(&self.audit, limit)
    }

    // ---------------------------------------------------------------- settings

    pub fn setting(&self, key: &str) -> Result<Option<String>, VaultError> {
        Ok(self
            .main
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        self.main.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        self.record(
            AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "key": key }),
        )
    }

    /// Secrets (e.g. the API key) are stored encrypted inside the vault.
    pub fn secret(&self, key: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
        let row: Option<Vec<u8>> = self
            .main
            .query_row("SELECT value_enc FROM secrets WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        row.map(|sealed| {
            open_string(
                &self.keys.settings,
                &aad("secrets", "value", key, ""),
                &sealed,
            )
            .map(Zeroizing::new)
        })
        .transpose()
    }

    pub fn set_secret(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        let sealed = seal_str(
            &self.keys.settings,
            &aad("secrets", "value", key, ""),
            value,
        )?;
        self.main.execute(
            "INSERT INTO secrets (key, value_enc) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value_enc = excluded.value_enc",
            params![key, sealed],
        )?;
        self.record(
            AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "secret": key }),
        )
    }

    pub fn delete_secret(&mut self, key: &str) -> Result<(), VaultError> {
        self.main
            .execute("DELETE FROM secrets WHERE key = ?1", [key])?;
        self.record(
            AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "secret_deleted": key }),
        )
    }

    // ---------------------------------------------------------------- practitioner

    pub fn practitioner(&self) -> Result<Practitioner, VaultError> {
        let row: Option<Vec<u8>> = self
            .identity
            .query_row("SELECT value_enc FROM practitioner WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        match row {
            Some(sealed) => from_json(&open_string(
                &self.keys.settings,
                &aad("practitioner", "value", "1", ""),
                &sealed,
            )?),
            None => Ok(Practitioner::default()),
        }
    }

    pub fn set_practitioner(&mut self, practitioner: &Practitioner) -> Result<(), VaultError> {
        let sealed = seal_str(
            &self.keys.settings,
            &aad("practitioner", "value", "1", ""),
            &to_json(practitioner)?,
        )?;
        self.identity.execute(
            "INSERT INTO practitioner (id, value_enc) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET value_enc = excluded.value_enc",
            [sealed],
        )?;
        self.record(
            AuditEvent::IdentitiesChanged,
            None,
            &serde_json::json!({ "practitioner": true }),
        )
    }

    // ---------------------------------------------------------------- cases

    fn case_key(&self, case_id: &str) -> Result<Key32, VaultError> {
        let wrapped: Vec<u8> = self
            .main
            .query_row(
                "SELECT wrapped_case_key FROM cases WHERE id = ?1",
                [case_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(VaultError::NotFound)?;
        let raw = open(
            &self.keys.case_wrap,
            &aad("cases", "key", case_id, case_id),
            &wrapped,
        )?;
        let bytes: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::Corrupt("case key length".to_owned()))?;
        Ok(Key32::from_bytes(bytes))
    }

    pub fn create_case(&mut self, meta: &CaseMeta) -> Result<String, VaultError> {
        let id = random_id()?;
        let case_key = Key32::random()?;
        let wrapped = seal(
            &self.keys.case_wrap,
            &aad("cases", "key", &id, &id),
            case_key.as_bytes(),
        )?;
        let meta_enc = seal_str(&case_key, &aad("cases", "meta", &id, &id), &to_json(meta)?)?;
        let t = now();
        self.main.execute(
            "INSERT INTO cases (id, created_at, updated_at, wrapped_case_key, meta_enc) VALUES (?1, ?2, ?2, ?3, ?4)",
            params![id, t, wrapped, meta_enc],
        )?;
        self.record(AuditEvent::CaseCreated, Some(&id), &serde_json::json!({}))?;
        Ok(id)
    }

    pub fn case_meta(&self, case_id: &str) -> Result<CaseMeta, VaultError> {
        let key = self.case_key(case_id)?;
        let sealed: Vec<u8> =
            self.main
                .query_row("SELECT meta_enc FROM cases WHERE id = ?1", [case_id], |r| {
                    r.get(0)
                })?;
        from_json(&open_string(
            &key,
            &aad("cases", "meta", case_id, case_id),
            &sealed,
        )?)
    }

    pub fn update_case_meta(&mut self, case_id: &str, meta: &CaseMeta) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        let sealed = seal_str(
            &key,
            &aad("cases", "meta", case_id, case_id),
            &to_json(meta)?,
        )?;
        self.main.execute(
            "UPDATE cases SET meta_enc = ?1, updated_at = ?2 WHERE id = ?3",
            params![sealed, now(), case_id],
        )?;
        Ok(())
    }

    fn touch(&self, case_id: &str) -> Result<(), VaultError> {
        self.main.execute(
            "UPDATE cases SET updated_at = ?1 WHERE id = ?2",
            params![now(), case_id],
        )?;
        Ok(())
    }

    /// Cases at work (not in the recycle bin), most recently changed first.
    pub fn list_cases(&self) -> Result<Vec<CaseSummary>, VaultError> {
        self.summaries(false)
    }

    /// Cases in the recycle bin, most recently deleted first.
    pub fn list_trash(&self) -> Result<Vec<CaseSummary>, VaultError> {
        self.summaries(true)
    }

    fn summaries(&self, trashed: bool) -> Result<Vec<CaseSummary>, VaultError> {
        let sql = if trashed {
            "SELECT id, created_at, updated_at, folder_id, deleted_at FROM cases WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
        } else {
            "SELECT id, created_at, updated_at, folder_id, deleted_at FROM cases WHERE deleted_at IS NULL ORDER BY updated_at DESC"
        };
        let mut stmt = self.main.prepare(sql)?;
        let rows: Vec<SummaryRow> = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, created_at, updated_at, folder_id, deleted_at)| {
                let meta = self.case_meta(&id)?;
                let child_name = self
                    .identities(&id)?
                    .into_iter()
                    .find(|i| i.role == Role::Child)
                    .map(|i| i.value);
                let approved_sections = self.approved_sections(&id)?;
                Ok(CaseSummary {
                    id,
                    meta,
                    child_name,
                    created_at,
                    updated_at,
                    approved_sections,
                    folder_id,
                    deleted_at,
                })
            })
            .collect()
    }

    /// Into the recycle bin: nothing is erased yet, and the case can be restored.
    pub fn trash_case(&mut self, case_id: &str) -> Result<(), VaultError> {
        self.case_key(case_id)?;
        self.main.execute(
            "UPDATE cases SET deleted_at = ?1 WHERE id = ?2",
            params![now(), case_id],
        )?;
        self.record(
            AuditEvent::CaseTrashed,
            Some(case_id),
            &serde_json::json!({}),
        )
    }

    /// Tests elsewhere in the workspace move a deletion back in time.
    #[doc(hidden)]
    pub fn set_deleted_at_for_tests(&mut self, case_id: &str, at: i64) -> Result<(), VaultError> {
        self.main.execute(
            "UPDATE cases SET deleted_at = ?1 WHERE id = ?2",
            params![at, case_id],
        )?;
        Ok(())
    }

    /// Back from the recycle bin. If its folder is gone, it returns to the top level.
    pub fn restore_case(&mut self, case_id: &str) -> Result<(), VaultError> {
        self.case_key(case_id)?;
        self.main.execute(
            "UPDATE cases SET deleted_at = NULL,
                folder_id = (SELECT id FROM folders WHERE id = cases.folder_id)
             WHERE id = ?1",
            [case_id],
        )?;
        self.record(
            AuditEvent::CaseRestored,
            Some(case_id),
            &serde_json::json!({}),
        )
    }

    /// Cases that have been in the recycle bin since before `cutoff` (unix seconds).
    pub fn trashed_before(&self, cutoff: i64) -> Result<Vec<String>, VaultError> {
        let mut stmt = self
            .main
            .prepare("SELECT id FROM cases WHERE deleted_at IS NOT NULL AND deleted_at < ?1")?;
        let ids = stmt
            .query_map([cutoff], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }

    pub fn move_case(&mut self, case_id: &str, folder_id: Option<&str>) -> Result<(), VaultError> {
        self.case_key(case_id)?;
        if let Some(f) = folder_id {
            self.folder_exists(f)?;
        }
        self.main.execute(
            "UPDATE cases SET folder_id = ?1 WHERE id = ?2",
            params![folder_id, case_id],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- folders

    fn folder_exists(&self, id: &str) -> Result<(), VaultError> {
        self.main
            .query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .ok_or(VaultError::NotFound)
    }

    pub fn folders(&self) -> Result<Vec<Folder>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT id, parent_id, created_at, name_enc FROM folders ORDER BY created_at",
        )?;
        let rows: Vec<(String, Option<String>, i64, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, parent_id, created_at, name)| {
                Ok(Folder {
                    name: open_string(
                        &self.keys.settings,
                        &aad("folders", "name", &id, ""),
                        &name,
                    )?,
                    id,
                    parent_id,
                    created_at,
                })
            })
            .collect()
    }

    pub fn create_folder(
        &mut self,
        parent_id: Option<&str>,
        name: &str,
    ) -> Result<Folder, VaultError> {
        if let Some(p) = parent_id {
            self.folder_exists(p)?;
        }
        let id = random_id()?;
        let t = now();
        let sealed = seal_str(&self.keys.settings, &aad("folders", "name", &id, ""), name)?;
        self.main.execute(
            "INSERT INTO folders (id, parent_id, created_at, name_enc) VALUES (?1, ?2, ?3, ?4)",
            params![id, parent_id, t, sealed],
        )?;
        self.record(
            AuditEvent::FoldersChanged,
            None,
            &serde_json::json!({ "created": true }),
        )?;
        Ok(Folder {
            id,
            parent_id: parent_id.map(str::to_owned),
            name: name.to_owned(),
            created_at: t,
        })
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) -> Result<(), VaultError> {
        self.folder_exists(id)?;
        let sealed = seal_str(&self.keys.settings, &aad("folders", "name", id, ""), name)?;
        self.main.execute(
            "UPDATE folders SET name_enc = ?1 WHERE id = ?2",
            params![sealed, id],
        )?;
        Ok(())
    }

    /// Move a folder under another (or to the top level). A folder cannot go inside itself or
    /// inside one of its own subfolders.
    pub fn move_folder(&mut self, id: &str, parent_id: Option<&str>) -> Result<(), VaultError> {
        self.folder_exists(id)?;
        if let Some(target) = parent_id {
            let mut cur = Some(target.to_owned());
            while let Some(c) = cur {
                if c == id {
                    return Err(VaultError::Refused(
                        "a folder cannot go inside itself".to_owned(),
                    ));
                }
                cur = self
                    .main
                    .query_row("SELECT parent_id FROM folders WHERE id = ?1", [&c], |r| {
                        r.get(0)
                    })
                    .optional()?
                    .ok_or(VaultError::NotFound)?;
            }
        }
        self.main.execute(
            "UPDATE folders SET parent_id = ?1 WHERE id = ?2",
            params![parent_id, id],
        )?;
        self.record(
            AuditEvent::FoldersChanged,
            None,
            &serde_json::json!({ "moved": true }),
        )
    }

    /// Delete a folder: its subfolders and cases (also those in the recycle bin) move up to
    /// its parent first. Nothing inside is ever deleted with it.
    pub fn delete_folder(&mut self, id: &str) -> Result<(), VaultError> {
        self.folder_exists(id)?;
        let parent: Option<String> =
            self.main
                .query_row("SELECT parent_id FROM folders WHERE id = ?1", [id], |r| {
                    r.get(0)
                })?;
        let tx = self.main.unchecked_transaction()?;
        tx.execute(
            "UPDATE folders SET parent_id = ?1 WHERE parent_id = ?2",
            params![parent, id],
        )?;
        tx.execute(
            "UPDATE cases SET folder_id = ?1 WHERE folder_id = ?2",
            params![parent, id],
        )?;
        tx.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        tx.commit()?;
        self.record(
            AuditEvent::FoldersChanged,
            None,
            &serde_json::json!({ "deleted": true }),
        )
    }

    /// Crypto-shredding: rows are deleted with `secure_delete` and the case key is gone,
    /// so nothing that belonged to the case can be decrypted again.
    pub fn delete_case(&mut self, case_id: &str) -> Result<(), VaultError> {
        self.case_key(case_id)?;
        let tx = self.main.unchecked_transaction()?;
        for table in ["inputs", "messages", "drafts", "transmissions"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE case_id = ?1"),
                [case_id],
            )?;
        }
        tx.execute("DELETE FROM cases WHERE id = ?1", [case_id])?;
        tx.commit()?;
        self.identity
            .execute("DELETE FROM identities WHERE case_id = ?1", [case_id])?;
        self.identity
            .execute("DELETE FROM decisions WHERE case_id = ?1", [case_id])?;
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.identity
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.record(
            AuditEvent::CaseDeleted,
            Some(case_id),
            &serde_json::json!({}),
        )
    }

    // ---------------------------------------------------------------- identities

    pub fn identities(&self, case_id: &str) -> Result<Vec<Identity>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.identity.prepare(
            "SELECT id, role, tag, value_enc, aliases_enc FROM identities WHERE case_id = ?1 ORDER BY rowid",
        )?;
        let rows: Vec<IdentityRow> = stmt
            .query_map([case_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, role, tag, value, aliases)| {
                Ok(Identity {
                    value: open_string(&key, &aad("identities", "value", &id, case_id), &value)?,
                    aliases: from_json(&open_string(
                        &key,
                        &aad("identities", "aliases", &id, case_id),
                        &aliases,
                    )?)?,
                    role: enum_from(&role)?,
                    case_id: case_id.to_owned(),
                    id,
                    tag,
                })
            })
            .collect()
    }

    /// Identities of every case: the gate scans outgoing text against all of them.
    pub fn all_identities(&self) -> Result<Vec<Identity>, VaultError> {
        let mut stmt = self.main.prepare("SELECT id FROM cases")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let mut all = Vec::new();
        for id in ids {
            all.extend(self.identities(&id)?);
        }
        Ok(all)
    }

    /// Replace the case's identity list. Existing identities keep their tags (tags are stable).
    pub fn set_identities(
        &mut self,
        case_id: &str,
        inputs: &[IdentityInput],
    ) -> Result<Vec<Identity>, VaultError> {
        let key = self.case_key(case_id)?;
        let existing = self.identities(case_id)?;
        let mut used: Vec<String> = existing.iter().map(|i| i.tag.clone()).collect();
        let t = now();
        let tx = self.identity.unchecked_transaction()?;
        let keep: Vec<&str> = inputs.iter().filter_map(|i| i.id.as_deref()).collect();
        for old in &existing {
            if !keep.contains(&old.id.as_str()) {
                tx.execute("DELETE FROM identities WHERE id = ?1", [&old.id])?;
            }
        }
        for input in inputs {
            let value = input.value.trim();
            if value.is_empty() {
                continue;
            }
            let aliases: Vec<String> = input
                .aliases
                .iter()
                .map(|a| a.trim().to_owned())
                .filter(|a| !a.is_empty())
                .collect();
            let previous = input
                .id
                .as_ref()
                .and_then(|id| existing.iter().find(|e| &e.id == id));
            let (id, tag) = match previous {
                Some(p) if p.role == input.role => (p.id.clone(), p.tag.clone()),
                _ => {
                    let tag = assign_tag(input.role, &used);
                    used.push(tag.clone());
                    (random_id()?, tag)
                }
            };
            let value_enc = seal_str(&key, &aad("identities", "value", &id, case_id), value)?;
            let aliases_enc = seal_str(
                &key,
                &aad("identities", "aliases", &id, case_id),
                &to_json(&aliases)?,
            )?;
            tx.execute(
                "INSERT INTO identities (id, case_id, role, tag, created_at, value_enc, aliases_enc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET role = excluded.role, tag = excluded.tag,
                    value_enc = excluded.value_enc, aliases_enc = excluded.aliases_enc",
                params![id, case_id, enum_str(&input.role)?, tag, t, value_enc, aliases_enc],
            )?;
        }
        tx.commit()?;
        self.touch(case_id)?;
        self.record(
            AuditEvent::IdentitiesChanged,
            Some(case_id),
            &serde_json::json!({ "count": inputs.len() }),
        )?;
        self.identities(case_id)
    }

    /// Keyed hash of a normalized token, for the "this is not a name" allow-list.
    #[must_use]
    pub fn token_hmac(&self, token: &str) -> String {
        hex(&hmac_sha256(
            &self.keys.index,
            format!("token|{}", token.trim()).as_bytes(),
        ))
    }

    /// Remember a decision about a token. A later decision for the same case replaces it.
    fn mark_decision(
        &mut self,
        case_id: Option<&str>,
        token: &str,
        decision: &str,
    ) -> Result<(), VaultError> {
        let h = self.token_hmac(token);
        self.identity.execute(
            "INSERT OR REPLACE INTO decisions (id, case_id, token_hmac, decision, tag, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
            params![random_id()?, case_id, h, decision, now()],
        )?;
        Ok(())
    }

    /// "Keep as is": for one case, or for every case when `case_id` is `None`.
    pub fn mark_not_a_name(
        &mut self,
        case_id: Option<&str>,
        token: &str,
    ) -> Result<(), VaultError> {
        self.mark_decision(case_id, token, "not_a_name")
    }

    /// An ordinary word that, in this case, is a prefix plus a declared name
    /// ("שאלון" = ש + אלון). The filter then hides the name inside it.
    pub fn mark_is_name(&mut self, case_id: &str, token: &str) -> Result<(), VaultError> {
        self.mark_decision(Some(case_id), token, "is_name")
    }

    /// Token HMACs the psychologist marked as "not a name" (global and for this case).
    pub fn not_a_name_hmacs(&self, case_id: &str) -> Result<Vec<String>, VaultError> {
        let mut stmt = self.identity.prepare(
            "SELECT token_hmac FROM decisions WHERE decision = 'not_a_name' AND (case_id IS NULL OR case_id = ?1)",
        )?;
        let rows = stmt.query_map([case_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Token HMACs confirmed as "prefix + name" for this case.
    pub fn is_name_hmacs(&self, case_id: &str) -> Result<Vec<String>, VaultError> {
        let mut stmt = self.identity.prepare(
            "SELECT token_hmac FROM decisions WHERE decision = 'is_name' AND case_id = ?1",
        )?;
        let rows = stmt.query_map([case_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ---------------------------------------------------------------- inputs

    pub fn add_input(
        &mut self,
        case_id: &str,
        kind: InputKind,
        title: &str,
        content: &str,
    ) -> Result<CaseInput, VaultError> {
        let key = self.case_key(case_id)?;
        let id = random_id()?;
        let t = now();
        self.main.execute(
            "INSERT INTO inputs (id, case_id, kind, created_at, title_enc, content_enc) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                case_id,
                enum_str(&kind)?,
                t,
                seal_str(&key, &aad("inputs", "title", &id, case_id), title)?,
                seal_str(&key, &aad("inputs", "content", &id, case_id), content)?
            ],
        )?;
        self.touch(case_id)?;
        Ok(CaseInput {
            id,
            case_id: case_id.to_owned(),
            kind,
            title: title.to_owned(),
            content: content.to_owned(),
            created_at: t,
        })
    }

    pub fn update_input(
        &mut self,
        case_id: &str,
        input_id: &str,
        title: &str,
        content: &str,
    ) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        let changed = self.main.execute(
            "UPDATE inputs SET title_enc = ?1, content_enc = ?2 WHERE id = ?3 AND case_id = ?4",
            params![
                seal_str(&key, &aad("inputs", "title", input_id, case_id), title)?,
                seal_str(&key, &aad("inputs", "content", input_id, case_id), content)?,
                input_id,
                case_id
            ],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        self.touch(case_id)
    }

    /// Structured data behind a material (a score sheet as JSON), sealed with the case key.
    pub fn set_input_data(
        &mut self,
        case_id: &str,
        input_id: &str,
        data: &str,
    ) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        let changed = self.main.execute(
            "UPDATE inputs SET data_enc = ?1 WHERE id = ?2 AND case_id = ?3",
            params![
                seal_str(&key, &aad("inputs", "data", input_id, case_id), data)?,
                input_id,
                case_id
            ],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        Ok(())
    }

    pub fn input_data(&self, case_id: &str, input_id: &str) -> Result<Option<String>, VaultError> {
        let key = self.case_key(case_id)?;
        let sealed: Option<Vec<u8>> = self
            .main
            .query_row(
                "SELECT data_enc FROM inputs WHERE id = ?1 AND case_id = ?2",
                params![input_id, case_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(VaultError::NotFound)?;
        sealed
            .map(|b| open_string(&key, &aad("inputs", "data", input_id, case_id), &b))
            .transpose()
    }

    /// Where a material goes in the report (D-022), as JSON, sealed with the case key.
    pub fn set_input_routing(
        &mut self,
        case_id: &str,
        input_id: &str,
        routing: &str,
    ) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        let changed = self.main.execute(
            "UPDATE inputs SET routing_enc = ?1 WHERE id = ?2 AND case_id = ?3",
            params![
                seal_str(&key, &aad("inputs", "routing", input_id, case_id), routing)?,
                input_id,
                case_id
            ],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        Ok(())
    }

    pub fn input_routing(
        &self,
        case_id: &str,
        input_id: &str,
    ) -> Result<Option<String>, VaultError> {
        let key = self.case_key(case_id)?;
        let sealed: Option<Vec<u8>> = self
            .main
            .query_row(
                "SELECT routing_enc FROM inputs WHERE id = ?1 AND case_id = ?2",
                params![input_id, case_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(VaultError::NotFound)?;
        sealed
            .map(|b| open_string(&key, &aad("inputs", "routing", input_id, case_id), &b))
            .transpose()
    }

    pub fn delete_input(&mut self, case_id: &str, input_id: &str) -> Result<(), VaultError> {
        self.main.execute(
            "DELETE FROM inputs WHERE id = ?1 AND case_id = ?2",
            params![input_id, case_id],
        )?;
        self.touch(case_id)
    }

    pub fn inputs(&self, case_id: &str) -> Result<Vec<CaseInput>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.main.prepare(
            "SELECT id, kind, created_at, title_enc, content_enc FROM inputs WHERE case_id = ?1 ORDER BY rowid",
        )?;
        let rows: Vec<InputRow> = stmt
            .query_map([case_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, kind, created_at, title, content)| {
                Ok(CaseInput {
                    title: open_string(&key, &aad("inputs", "title", &id, case_id), &title)?,
                    content: open_string(&key, &aad("inputs", "content", &id, case_id), &content)?,
                    kind: enum_from(&kind)?,
                    case_id: case_id.to_owned(),
                    id,
                    created_at,
                })
            })
            .collect()
    }

    // ---------------------------------------------------------------- chat

    pub fn add_message(
        &mut self,
        case_id: &str,
        section_key: &str,
        role: ChatRole,
        text_tagged: &str,
        hidden: &[String],
        demo: bool,
    ) -> Result<ChatMessage, VaultError> {
        let key = self.case_key(case_id)?;
        let id = random_id()?;
        let t = now();
        self.main.execute(
            "INSERT INTO messages (id, case_id, section_key, role, demo, created_at, text_tagged_enc, hidden_enc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                case_id,
                section_key,
                enum_str(&role)?,
                demo,
                t,
                seal_str(&key, &aad("messages", "text", &id, case_id), text_tagged)?,
                seal_str(&key, &aad("messages", "hidden", &id, case_id), &to_json(&hidden)?)?
            ],
        )?;
        self.touch(case_id)?;
        Ok(ChatMessage {
            id,
            case_id: case_id.to_owned(),
            section_key: section_key.to_owned(),
            role,
            text_tagged: text_tagged.to_owned(),
            hidden: hidden.to_vec(),
            demo,
            created_at: t,
        })
    }

    pub fn messages(
        &self,
        case_id: &str,
        section_key: &str,
    ) -> Result<Vec<ChatMessage>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.main.prepare(
            "SELECT id, role, demo, created_at, text_tagged_enc, hidden_enc FROM messages
             WHERE case_id = ?1 AND section_key = ?2 ORDER BY created_at, rowid",
        )?;
        let rows: Vec<MessageRow> = stmt
            .query_map(params![case_id, section_key], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, role, demo, created_at, text, hidden)| {
                Ok(ChatMessage {
                    text_tagged: open_string(&key, &aad("messages", "text", &id, case_id), &text)?,
                    hidden: from_json(&open_string(
                        &key,
                        &aad("messages", "hidden", &id, case_id),
                        &hidden,
                    )?)?,
                    role: enum_from(&role)?,
                    case_id: case_id.to_owned(),
                    section_key: section_key.to_owned(),
                    id,
                    demo,
                    created_at,
                })
            })
            .collect()
    }

    // ---------------------------------------------------------------- drafts

    pub fn add_draft(
        &mut self,
        case_id: &str,
        section_key: &str,
        text_tagged: &str,
        author: Author,
        source_refs: &[String],
    ) -> Result<DraftParagraph, VaultError> {
        let key = self.case_key(case_id)?;
        let id = random_id()?;
        let t = now();
        let position: u32 = self.main.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM drafts WHERE case_id = ?1 AND section_key = ?2",
            params![case_id, section_key],
            |r| r.get(0),
        )?;
        self.main.execute(
            "INSERT INTO drafts (id, case_id, section_key, position, status, author, created_at, approved_at, text_tagged_enc, source_refs_enc)
             VALUES (?1, ?2, ?3, ?4, 'proposed', ?5, ?6, NULL, ?7, ?8)",
            params![
                id,
                case_id,
                section_key,
                position,
                enum_str(&author)?,
                t,
                seal_str(&key, &aad("drafts", "text", &id, case_id), text_tagged)?,
                seal_str(&key, &aad("drafts", "refs", &id, case_id), &to_json(&source_refs)?)?
            ],
        )?;
        self.touch(case_id)?;
        Ok(DraftParagraph {
            id,
            case_id: case_id.to_owned(),
            section_key: section_key.to_owned(),
            position,
            text_tagged: text_tagged.to_owned(),
            status: DraftStatus::Proposed,
            author,
            source_refs: source_refs.to_vec(),
            created_at: t,
            approved_at: None,
        })
    }

    pub fn drafts(
        &self,
        case_id: &str,
        section_key: &str,
    ) -> Result<Vec<DraftParagraph>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.main.prepare(
            "SELECT id, position, status, author, created_at, approved_at, text_tagged_enc, source_refs_enc
             FROM drafts WHERE case_id = ?1 AND section_key = ?2 ORDER BY position",
        )?;
        type Row = (
            String,
            u32,
            String,
            String,
            i64,
            Option<i64>,
            Vec<u8>,
            Vec<u8>,
        );
        let rows: Vec<Row> = stmt
            .query_map(params![case_id, section_key], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(
                |(id, position, status, author, created_at, approved_at, text, refs)| {
                    Ok(DraftParagraph {
                        text_tagged: open_string(
                            &key,
                            &aad("drafts", "text", &id, case_id),
                            &text,
                        )?,
                        source_refs: from_json(&open_string(
                            &key,
                            &aad("drafts", "refs", &id, case_id),
                            &refs,
                        )?)?,
                        status: enum_from(&status)?,
                        author: enum_from(&author)?,
                        case_id: case_id.to_owned(),
                        section_key: section_key.to_owned(),
                        id,
                        position,
                        created_at,
                        approved_at,
                    })
                },
            )
            .collect()
    }

    pub fn set_draft_status(
        &mut self,
        case_id: &str,
        draft_id: &str,
        status: DraftStatus,
    ) -> Result<(), VaultError> {
        let approved_at = (status == DraftStatus::Approved).then(now);
        let changed = self.main.execute(
            "UPDATE drafts SET status = ?1, approved_at = ?2 WHERE id = ?3 AND case_id = ?4",
            params![enum_str(&status)?, approved_at, draft_id, case_id],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        self.touch(case_id)
    }

    /// Manual edit by the psychologist: the paragraph becomes hers and is approved.
    pub fn edit_draft(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text_tagged: &str,
    ) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        let changed = self.main.execute(
            "UPDATE drafts SET text_tagged_enc = ?1, author = 'user', status = 'approved', approved_at = ?2
             WHERE id = ?3 AND case_id = ?4",
            params![seal_str(&key, &aad("drafts", "text", draft_id, case_id), text_tagged)?, now(), draft_id, case_id],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        self.touch(case_id)
    }

    pub fn approved_sections(&self, case_id: &str) -> Result<Vec<String>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT DISTINCT section_key FROM drafts WHERE case_id = ?1 AND status = 'approved' ORDER BY section_key",
        )?;
        let rows = stmt.query_map([case_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ---------------------------------------------------------------- transmissions

    pub fn add_transmission(
        &mut self,
        case_id: &str,
        section_key: &str,
        payload_sha256: &str,
        model: &str,
        payload_tagged: &str,
    ) -> Result<Transmission, VaultError> {
        let key = self.case_key(case_id)?;
        let id = random_id()?;
        let t = now();
        self.main.execute(
            "INSERT INTO transmissions (id, case_id, section_key, created_at, payload_sha256, model, payload_tagged_enc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                case_id,
                section_key,
                t,
                payload_sha256,
                model,
                seal_str(&key, &aad("transmissions", "payload", &id, case_id), payload_tagged)?
            ],
        )?;
        Ok(Transmission {
            id,
            case_id: case_id.to_owned(),
            section_key: section_key.to_owned(),
            payload_sha256: payload_sha256.to_owned(),
            model: model.to_owned(),
            payload_tagged: payload_tagged.to_owned(),
            created_at: t,
        })
    }

    pub fn transmissions(&self, case_id: &str) -> Result<Vec<Transmission>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.main.prepare(
            "SELECT id, section_key, created_at, payload_sha256, model, payload_tagged_enc FROM transmissions
             WHERE case_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows: Vec<TransmissionRow> = stmt
            .query_map([case_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(
                |(id, section_key, created_at, payload_sha256, model, payload)| {
                    Ok(Transmission {
                        payload_tagged: open_string(
                            &key,
                            &aad("transmissions", "payload", &id, case_id),
                            &payload,
                        )?,
                        case_id: case_id.to_owned(),
                        id,
                        section_key,
                        created_at,
                        payload_sha256,
                        model,
                    })
                },
            )
            .collect()
    }
}
