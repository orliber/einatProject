//! The unlocked vault. Dropping it closes the databases and wipes every key.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dv_domain::{
    assign_tag, Author, CaseInput, CaseMeta, CaseSummary, ChatMessage, ChatRole, DraftParagraph,
    DraftStatus, Folder, FoundName, Identity, IdentityInput, IdentitySource, InputKind, MaskEntry,
    Role, Transmission,
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
use crate::hello;
use crate::password::{self, Argon2Params};
use crate::{recovery, version, VaultError};

mod backup;
mod slots;
mod style;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod version_tests;
pub use backup::{
    drill_backup, peek_backup, BackupCheck, BackupInfo, BackupJob, BackupPeek, BackupWritten,
    WrittenBackup, BACKUP_EXTENSION,
};
pub use slots::NewSlot;
pub use style::{StoredStyleProfile, StoredStyleSource};

/// Raw rows as read from SQLite, before decryption.
type IdentityRow = (String, String, String, Vec<u8>, Vec<u8>, String, String);
type InputRow = (String, String, i64, Vec<u8>, Vec<u8>);

/// A material just deleted, kept (sealed, in memory only) so the deletion can be undone.
pub struct RemovedInput {
    pub id: String,
    pub case_id: String,
    kind: String,
    created_at: i64,
    title_enc: Vec<u8>,
    content_enc: Vec<u8>,
    data_enc: Option<Vec<u8>>,
    routing_enc: Option<Vec<u8>>,
}

impl std::fmt::Debug for RemovedInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemovedInput")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
type MessageRow = (String, String, bool, i64, Vec<u8>, Vec<u8>);
type TransmissionRow = (String, String, i64, String, String, Vec<u8>);
type SummaryRow = (String, i64, i64, Option<String>, Option<i64>);
/// author, created_at, approved_at, sealed text, replaces: one link of a paragraph's history.
type ChainRow = (String, i64, Option<i64>, Vec<u8>, Option<String>);

/// How far back a paragraph's history is followed through the wordings it replaced.
const MAX_VERSION_CHAIN: usize = 50;

/// An earlier wording of a paragraph (D-046), tagged like the draft itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftVersion {
    /// A saved version's id, or the id of the approved paragraph a new wording replaced.
    pub id: String,
    pub saved_at: i64,
    pub author: Author,
    pub text_tagged: String,
}

const MAIN_DB: &str = "main.db";
const IDENTITY_DB: &str = "identity.db";
const AUDIT_DB: &str = "audit.db";

fn token_hmac(key: &Key32, token: &str) -> String {
    hex(&hmac_sha256(
        key,
        format!("token|{}", token.trim()).as_bytes(),
    ))
}

/// [`Vault::token_hmac`] away from the open vault.
pub struct TokenHasher(Key32);

impl TokenHasher {
    #[must_use]
    pub fn token_hmac(&self, token: &str) -> String {
        token_hmac(&self.0, token)
    }
}

impl std::fmt::Debug for TokenHasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenHasher")
    }
}

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

/// A saved consultation (D-027). The turns are the caller's own JSON, sealed at rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConsultation {
    pub id: String,
    pub case_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub turns_json: String,
}

/// What opens a vault (or a backup of it).
#[derive(Clone, Copy)]
pub enum Secret<'a> {
    /// The password, where its slot is not bound to a computer (or to open a backup).
    Password(&'a str),
    /// The password on this computer, with the key sealed here to her Windows account
    /// (D-076). Opens a bound slot, and an unbound one as [`Secret::Password`] does.
    PasswordHere {
        password: &'a str,
        computer: &'a Key32,
    },
    Recovery(&'a str),
    /// The key kept in her Google Drive (fetched after she signed in) and the key sealed to
    /// her Windows account on this computer (D-041). Both are needed.
    Google {
        drive: &'a Key32,
        computer: &'a Key32,
    },
}

impl std::fmt::Debug for Secret<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Secret::Password(_) => "Secret::Password(..)",
            Secret::PasswordHere { .. } => "Secret::PasswordHere(..)",
            Secret::Recovery(_) => "Secret::Recovery(..)",
            Secret::Google { .. } => "Secret::Google(..)",
        })
    }
}

/// The password slot: its salt, its cost, its wrapped key, and whether it is bound.
fn password_slot(
    header: &VaultHeader,
) -> Result<([u8; 32], Argon2Params, String, bool), VaultError> {
    let (salt, params, wrapped, bound) = header
        .slots
        .iter()
        .find_map(|s| match s {
            KeySlot::Password {
                salt,
                params,
                wrapped_mk,
                bound,
            } => Some((salt.clone(), *params, wrapped_mk.clone(), *bound)),
            KeySlot::Recovery { .. } | KeySlot::WindowsHello { .. } | KeySlot::Google { .. } => {
                None
            }
        })
        .ok_or(VaultError::Corrupt("no password slot".to_owned()))?;
    let salt: [u8; 32] = crate::crypto::unhex(&salt)?
        .try_into()
        .map_err(|_| VaultError::Corrupt("salt length".to_owned()))?;
    Ok((salt, params, wrapped, bound))
}

/// The master key from the password slot, and the stretched password (to bind the slot
/// without stretching again). A bound slot without this computer's key is refused before
/// any slow work.
fn password_master_key(
    header: &VaultHeader,
    password: &str,
    computer: Option<&Key32>,
) -> Result<(Key32, Zeroizing<[u8; 32]>), VaultError> {
    let (salt, params, wrapped, bound) = password_slot(header)?;
    let computer = match (bound, computer) {
        (true, None) => return Err(VaultError::OtherComputer),
        (true, Some(c)) => Some(c),
        (false, _) => None,
    };
    let stretched = password::stretch(password, &salt, params)?;
    let mk = match computer {
        Some(c) => unwrap_mk(
            &password::bound_kek_of(&stretched, c)?,
            "password-bound",
            &header.vault_id,
            &wrapped,
        )?,
        None => unwrap_mk(
            &password::kek_of(&stretched)?,
            "password",
            &header.vault_id,
            &wrapped,
        )?,
    };
    Ok((mk, stretched))
}

/// Where the unbound password slot is kept for the backups, sealed inside the vault (D-076).
const BACKUP_SLOT_SECRET: &str = "password_slot_for_backups";

/// Where the newest version that opened the vault is kept, sealed inside it (D-104).
const APP_VERSION_SECRET: &str = "app_version";

/// The version sealed in the main database (D-104), read before the vault is built and before
/// any migration. `None` for a vault no version recorded itself in yet (one from before the
/// table of secrets has no table at all).
fn sealed_app_version(main: &Connection, key: &Key32) -> Result<Option<String>, VaultError> {
    let tables: i64 = main.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'secrets'",
        [],
        |r| r.get(0),
    )?;
    if tables == 0 {
        return Ok(None);
    }
    let row: Option<Vec<u8>> = main
        .query_row(
            "SELECT value_enc FROM secrets WHERE key = ?1",
            [APP_VERSION_SECRET],
            |r| r.get(0),
        )
        .optional()?;
    row.map(|sealed| {
        open_string(
            key,
            &aad("secrets", "value", APP_VERSION_SECRET, ""),
            &sealed,
        )
    })
    .transpose()
}

/// A password slot bound to this computer, from the stretched password.
pub(crate) fn bound_slot(
    stretched: &[u8; 32],
    salt: &[u8; 32],
    params: Argon2Params,
    computer: &Key32,
    vault_id: &str,
    mk: &Key32,
) -> Result<KeySlot, VaultError> {
    Ok(KeySlot::Password {
        salt: hex(salt),
        params,
        wrapped_mk: wrap_mk(
            &password::bound_kek_of(stretched, computer)?,
            "password-bound",
            vault_id,
            mk,
        )?,
        bound: true,
    })
}

/// The master key, unwrapped from the header's password, recovery or Google slot.
pub(crate) fn master_key(header: &VaultHeader, secret: Secret<'_>) -> Result<Key32, VaultError> {
    match secret {
        Secret::Password(password) => password_master_key(header, password, None).map(|(mk, _)| mk),
        Secret::PasswordHere { password, computer } => {
            password_master_key(header, password, Some(computer)).map(|(mk, _)| mk)
        }
        Secret::Recovery(typed) => {
            let wrapped = header
                .slots
                .iter()
                .find_map(|s| match s {
                    KeySlot::Recovery { wrapped_mk } => Some(wrapped_mk.clone()),
                    KeySlot::Password { .. }
                    | KeySlot::WindowsHello { .. }
                    | KeySlot::Google { .. } => None,
                })
                .ok_or(VaultError::Corrupt("no recovery slot".to_owned()))?;
            let kek = recovery::derive_kek(&recovery::parse(typed)?)?;
            unwrap_mk(&kek, "recovery", &header.vault_id, &wrapped)
        }
        Secret::Google { drive, computer } => {
            // No Google slot (turned off, or an older backup): the key opens nothing.
            let wrapped = header
                .slots
                .iter()
                .find_map(|s| match s {
                    KeySlot::Google { wrapped_mk, .. } => Some(wrapped_mk.clone()),
                    KeySlot::Password { .. }
                    | KeySlot::Recovery { .. }
                    | KeySlot::WindowsHello { .. } => None,
                })
                .ok_or(VaultError::WrongSecret)?;
            let kek = recovery::derive_google_kek(drive, computer)?;
            unwrap_mk(&kek, "google", &header.vault_id, &wrapped)
        }
    }
}

/// The Windows Hello slot's key-pair name, challenge and wrapped master key.
fn hello_slot_of(header: &VaultHeader) -> Option<(String, String, String)> {
    header.slots.iter().find_map(|s| match s {
        KeySlot::WindowsHello {
            credential,
            challenge,
            wrapped_mk,
        } => Some((credential.clone(), challenge.clone(), wrapped_mk.clone())),
        KeySlot::Password { .. } | KeySlot::Recovery { .. } | KeySlot::Google { .. } => None,
    })
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
        Self::create_here(dir, password, params, None)
    }

    /// Create a new vault, its password slot bound to this computer when `computer` is the
    /// key sealed here to her Windows account (D-076).
    pub fn create_here(
        dir: &Path,
        password: &str,
        params: Argon2Params,
        computer: Option<&Key32>,
    ) -> Result<Created, VaultError> {
        password::check_policy(password).map_err(VaultError::Policy)?;
        if Self::exists(dir) {
            return Err(VaultError::AlreadyExists);
        }
        fs::create_dir_all(dir)?;
        let vault_id = random_id()?;
        let mk = Key32::random()?;
        let salt = random_array::<32>()?;
        let stretched = password::stretch(password, &salt, params)?;
        let unbound = KeySlot::Password {
            salt: hex(&salt),
            params,
            wrapped_mk: wrap_mk(&password::kek_of(&stretched)?, "password", &vault_id, &mk)?,
            bound: false,
        };
        let live = match computer {
            Some(c) => bound_slot(&stretched, &salt, params, c, &vault_id, &mk)?,
            None => unbound.clone(),
        };
        let recovery_key = Key32::random()?;
        let recovery_kek = recovery::derive_kek(&recovery_key)?;
        let mut header = VaultHeader {
            format: FORMAT,
            vault_id: vault_id.clone(),
            created_at: now(),
            slots: vec![
                live,
                KeySlot::Recovery {
                    wrapped_mk: wrap_mk(&recovery_kek, "recovery", &vault_id, &mk)?,
                },
            ],
            audit_anchor: None,
            app_version: None,
            mac: None,
        };
        let keys = Keys::derive(&mk)?;
        header.sign(&keys.header)?;
        header.write(dir)?;
        let mut vault = Self::open_with(dir, header, &mk)?;
        if computer.is_some() {
            vault.keep_backup_slot(&unbound)?;
        }
        vault.record(AuditEvent::VaultCreated, None, &serde_json::json!({}))?;
        Ok(Created {
            vault,
            recovery_key: recovery::format(&recovery_key),
        })
    }

    /// Check the password without opening the vault again (before an irreversible action).
    pub fn verify_password(dir: &Path, password: &str) -> Result<(), VaultError> {
        Self::verify_password_here(dir, password, None)
    }

    /// Check the password on this computer (`computer`: the key sealed here, D-076).
    pub fn verify_password_here(
        dir: &Path,
        password: &str,
        computer: Option<&Key32>,
    ) -> Result<(), VaultError> {
        let header = VaultHeader::read(dir)?;
        password_master_key(&header, password, computer).map(|_| ())
    }

    /// True when the password slot is bound to a computer (read while the vault is locked, to
    /// know whether this computer's key is needed). Unauthenticated until the vault opens.
    pub fn password_is_bound(dir: &Path) -> Result<bool, VaultError> {
        let header = VaultHeader::read(dir)?;
        password_slot(&header).map(|(_, _, _, bound)| bound)
    }

    pub fn unlock_with_password(dir: &Path, password: &str) -> Result<Self, VaultError> {
        Self::unlock_with_password_here(dir, password, None)
    }

    /// Open with the password on this computer. With `computer` (the key sealed here to her
    /// Windows account) a slot not yet bound is bound now, from the same stretched password,
    /// so the change costs no second derivation (D-076). A failed binding leaves the vault
    /// open with the slot as it was, and is tried again at the next opening.
    pub fn unlock_with_password_here(
        dir: &Path,
        password: &str,
        computer: Option<&Key32>,
    ) -> Result<Self, VaultError> {
        let header = VaultHeader::read(dir)?;
        let (mk, stretched) = password_master_key(&header, password, computer)?;
        let mut vault = Self::open_with(dir, header, &mk)?;
        vault.record(
            AuditEvent::Unlock,
            None,
            &serde_json::json!({"method": "password"}),
        )?;
        if let Some(c) = computer {
            if vault.password_bound() {
                // The slot the backups carry is made again when it is missing or stale (a
                // new password whose header write failed): never a vault without backups.
                if vault.backup_header().is_err() {
                    let _ = vault.keep_unbound(&stretched, &mk);
                }
            } else {
                if let Err(e) = vault.bind_with(&stretched, c, &mk) {
                    let _ = vault.record(
                        AuditEvent::IntegrityWarning,
                        None,
                        &serde_json::json!({ "reason": format!("binding to this computer: {e}") }),
                    );
                }
            }
        }
        Ok(vault)
    }

    /// The password slot is bound to a computer.
    #[must_use]
    pub fn password_bound(&self) -> bool {
        password_slot(&self.header).is_ok_and(|(_, _, _, bound)| bound)
    }

    /// Bind the password slot to this computer, with the password (after a restore, where the
    /// slot came unbound from the backup). Stretches the password once more; does nothing
    /// when the slot is bound already.
    pub fn bind_password(&mut self, password: &str, computer: &Key32) -> Result<(), VaultError> {
        if self.password_bound() {
            return Ok(());
        }
        let (mk, stretched) = password_master_key(&self.header, password, None)?;
        self.bind_with(&stretched, computer, &mk)
    }

    /// Swap the unbound password slot for one bound to this computer. The unbound one is kept
    /// first, sealed inside the vault, for the backups (they open with the password alone on
    /// any computer, as they always did).
    fn bind_with(
        &mut self,
        stretched: &[u8; 32],
        computer: &Key32,
        mk: &Key32,
    ) -> Result<(), VaultError> {
        let unbound = self
            .header
            .slots
            .iter()
            .find(|s| matches!(s, KeySlot::Password { bound: false, .. }))
            .cloned()
            .ok_or(VaultError::Corrupt("no unbound password slot".to_owned()))?;
        let (salt, params, _, _) = password_slot(&self.header)?;
        let bound = bound_slot(
            stretched,
            &salt,
            params,
            computer,
            &self.header.vault_id,
            mk,
        )?;
        self.keep_backup_slot(&unbound)?;
        for slot in &mut self.header.slots {
            if matches!(slot, KeySlot::Password { .. }) {
                *slot = bound.clone();
            }
        }
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        self.record(AuditEvent::PasswordBound, None, &serde_json::json!({}))
    }

    /// Make the unbound password slot again from the stretched password, and keep it.
    fn keep_unbound(&mut self, stretched: &[u8; 32], mk: &Key32) -> Result<(), VaultError> {
        let (salt, params, _, _) = password_slot(&self.header)?;
        let unbound = KeySlot::Password {
            salt: hex(&salt),
            params,
            wrapped_mk: wrap_mk(
                &password::kek_of(stretched)?,
                "password",
                &self.header.vault_id,
                mk,
            )?,
            bound: false,
        };
        self.keep_backup_slot(&unbound)
    }

    /// Keep the unbound password slot sealed inside the vault, for the backups. Not logged on
    /// its own: the password's own event (created, bound, changed) is.
    pub(crate) fn keep_backup_slot(&mut self, unbound: &KeySlot) -> Result<(), VaultError> {
        let key = BACKUP_SLOT_SECRET;
        let sealed = seal_str(
            &self.keys.settings,
            &aad("secrets", "value", key, ""),
            &to_json(unbound)?,
        )?;
        self.main.execute(
            "INSERT INTO secrets (key, value_enc) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value_enc = excluded.value_enc",
            params![key, sealed],
        )?;
        Ok(())
    }

    /// The password slot is no longer bound: the backups carry the live one.
    pub(crate) fn drop_backup_slot(&mut self) -> Result<(), VaultError> {
        self.main
            .execute("DELETE FROM secrets WHERE key = ?1", [BACKUP_SLOT_SECRET])?;
        Ok(())
    }

    /// The header a backup carries: this one, with the unbound password slot in place of the
    /// bound one, signed again. Refused when the kept slot is missing or not the one the live
    /// slot was made with (fail closed: a backup that the password could not open is never
    /// written).
    pub(crate) fn backup_header(&self) -> Result<Vec<u8>, VaultError> {
        if !self.password_bound() {
            return Ok(fs::read(self.dir.join(HEADER_FILE))?);
        }
        let kept: KeySlot = from_json(&self.secret(BACKUP_SLOT_SECRET)?.ok_or(
            VaultError::Integrity("the password slot for backups is missing"),
        )?)?;
        let (salt, params, _, _) = password_slot(&self.header)?;
        let fits = matches!(
            &kept,
            KeySlot::Password { salt: s, params: p, bound: false, .. }
                if crate::crypto::unhex(s).ok().as_deref() == Some(salt.as_slice()) && *p == params
        );
        if !fits {
            return Err(VaultError::Integrity(
                "the password slot for backups does not match the password",
            ));
        }
        let mut header = self.header.clone();
        for slot in &mut header.slots {
            if matches!(slot, KeySlot::Password { .. }) {
                *slot = kept.clone();
            }
        }
        header.sign(&self.keys.header)?;
        serde_json::to_vec_pretty(&header).map_err(|e| VaultError::Corrupt(e.to_string()))
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

    /// Forgot the password: open with the key from her Google Drive and the key sealed to
    /// this computer (D-041).
    pub fn unlock_with_google(
        dir: &Path,
        drive: &Key32,
        computer: &Key32,
    ) -> Result<Self, VaultError> {
        let header = VaultHeader::read(dir)?;
        let mk = master_key(&header, Secret::Google { drive, computer })?;
        let mut vault = Self::open_with(dir, header, &mk)?;
        vault.record(
            AuditEvent::Unlock,
            None,
            &serde_json::json!({"method": "google"}),
        )?;
        Ok(vault)
    }

    #[must_use]
    pub fn has_hello_slot(&self) -> bool {
        hello_slot_of(&self.header).is_some()
    }

    /// Windows Hello was taken out of the program (D-054). A vault that had it turned on
    /// keeps opening with the password; the first time it opens, the old slot leaves the
    /// header and the key pair is removed from Windows. Only ever takes a way in away.
    pub fn remove_hello_slot(&mut self) -> Result<(), VaultError> {
        let Some((credential, _, _)) = hello_slot_of(&self.header) else {
            return Ok(());
        };
        self.header
            .slots
            .retain(|s| !matches!(s, KeySlot::WindowsHello { .. }));
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        hello::delete_key_pair(&credential);
        self.record(AuditEvent::WindowsHelloOff, None, &serde_json::json!({}))
    }

    /// The vault's id and the tag of the Google account that can open it, read from the
    /// header while the vault is still locked. `None` when sign-in with Google is off.
    /// Unauthenticated until the vault opens: it only decides what the lock screen offers.
    pub fn google_slot(dir: &Path) -> Result<(String, Option<String>), VaultError> {
        let header = VaultHeader::read(dir)?;
        let tag = header.slots.iter().find_map(|s| match s {
            KeySlot::Google { account, .. } => Some(account.clone()),
            KeySlot::Password { .. } | KeySlot::Recovery { .. } | KeySlot::WindowsHello { .. } => {
                None
            }
        });
        Ok((header.vault_id, tag))
    }

    #[must_use]
    pub fn has_google_slot(&self) -> bool {
        self.header
            .slots
            .iter()
            .any(|s| matches!(s, KeySlot::Google { .. }))
    }

    /// Turn on sign-in with Google (or move it to another account): the master key wrapped
    /// under the key just put in her Drive together with the key sealed to this computer.
    /// Needs a fresh proof, like a new password.
    pub fn set_google_slot(
        &mut self,
        current: Secret<'_>,
        drive: &Key32,
        computer: &Key32,
        account_tag: &str,
    ) -> Result<(), VaultError> {
        let mk = master_key(&self.header, current)?;
        let kek = recovery::derive_google_kek(drive, computer)?;
        let slot = KeySlot::Google {
            account: account_tag.to_owned(),
            wrapped_mk: wrap_mk(&kek, "google", &self.header.vault_id, &mk)?,
        };
        self.header
            .slots
            .retain(|s| !matches!(s, KeySlot::Google { .. }));
        self.header.slots.push(slot);
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        self.record(AuditEvent::GoogleRecoveryOn, None, &serde_json::json!({}))
    }

    /// Turn sign-in with Google off. Needs a fresh proof. Backups written before still carry
    /// the slot, so the Drive key should be deleted too (the core does that).
    pub fn remove_google_slot(&mut self, current: Secret<'_>) -> Result<(), VaultError> {
        master_key(&self.header, current)?;
        if !self.has_google_slot() {
            return Ok(());
        }
        self.header
            .slots
            .retain(|s| !matches!(s, KeySlot::Google { .. }));
        self.header.sign(&self.keys.header)?;
        self.header.write(&self.dir)?;
        self.record(AuditEvent::GoogleRecoveryOff, None, &serde_json::json!({}))
    }

    fn open_with(dir: &Path, mut header: VaultHeader, mk: &Key32) -> Result<Self, VaultError> {
        let keys = Keys::derive(mk)?;
        let header_ok = header.verify(&keys.header).is_ok();
        // D-104: a vault a newer version opened is refused before anything is written to it:
        // by the header (also checked as it was read), and by the version sealed in the main
        // database, which cannot be edited. A newer version that only migrated the databases
        // and then failed to open did not record itself: the version before still opens the
        // vault (migrations only add, R7).
        version::refuse_newer(header.app_version.as_deref(), version::APP_VERSION)?;
        let main = open_encrypted(&dir.join(MAIN_DB), &keys.db_main)?;
        let sealed_version = sealed_app_version(&main, &keys.settings)?;
        version::refuse_newer(sealed_version.as_deref(), version::APP_VERSION)?;
        backup::remove_stale_scratch(dir);
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
        vault.record_app_version(sealed_version.as_deref())?;
        Ok(vault)
    }

    /// The newest version of the program that opened this vault (D-104), as the header says
    /// while it is locked. Unauthenticated until the vault opens: it only decides what the
    /// lock screen says. `None` for a vault no version recorded itself in yet.
    pub fn opened_by(dir: &Path) -> Result<Option<String>, VaultError> {
        Ok(VaultHeader::read(dir)?.app_version)
    }

    /// While the vault is locked: `Some(version)` when a newer version of the program opened
    /// it and this one will not (D-104); the version is empty when it cannot be shown. Read
    /// from the header alone, so the lock screen says so before anything is typed.
    #[must_use]
    pub fn opened_by_newer(dir: &Path) -> Option<String> {
        match VaultHeader::read(dir) {
            Err(VaultError::NewerVersion(v)) => Some(v),
            _ => None,
        }
    }

    /// The version recorded in the open vault's header.
    #[must_use]
    pub fn app_version(&self) -> Option<&str> {
        self.header.app_version.as_deref()
    }

    /// Record this version as the newest that opened the vault, when it is newer than the one
    /// recorded (never lower, D-104): sealed in the main database, and in the header when the
    /// header is the one this vault signed (a header changed by someone else is never signed
    /// again here). A header from before D-104 moves to the format that records it.
    fn record_app_version(&mut self, sealed: Option<&str>) -> Result<(), VaultError> {
        let current = version::APP_VERSION;
        if version::should_record(sealed, current) {
            let value = seal_str(
                &self.keys.settings,
                &aad("secrets", "value", APP_VERSION_SECRET, ""),
                current,
            )?;
            self.main.execute(
                "INSERT INTO secrets (key, value_enc) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value_enc = excluded.value_enc",
                params![APP_VERSION_SECRET, value],
            )?;
        }
        if self.integrity.header_ok
            && (version::should_record(self.header.app_version.as_deref(), current)
                || self.header.format < FORMAT)
        {
            self.header.app_version = Some(current.to_owned());
            self.header.format = FORMAT;
            self.header.sign(&self.keys.header)?;
            self.header.write(&self.dir)?;
        }
        Ok(())
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
    pub fn lock(self) -> Result<(), VaultError> {
        self.lock_because(None)
    }

    /// Lock, recording why (for example `sleep`) in the same single log entry.
    pub fn lock_because(mut self, reason: Option<&str>) -> Result<(), VaultError> {
        let meta = match reason {
            Some(r) => serde_json::json!({ "reason": r }),
            None => serde_json::json!({}),
        };
        self.record(AuditEvent::Lock, None, &meta)
    }

    /// Replace the password slot. Needs a fresh proof: the current password, or the recovery
    /// kit when the password was forgotten. The master key is not kept in memory after
    /// unlocking (only purpose-specific subkeys are).
    pub fn rekey_password(
        &mut self,
        current: Secret<'_>,
        new_password: &str,
        params: Argon2Params,
    ) -> Result<(), VaultError> {
        self.rekey_password_here(current, new_password, params, None)
    }

    /// A new password, bound to this computer when `computer` is the key sealed here (D-076).
    pub fn rekey_password_here(
        &mut self,
        current: Secret<'_>,
        new_password: &str,
        params: Argon2Params,
        computer: Option<&Key32>,
    ) -> Result<(), VaultError> {
        let slot = NewSlot::password_here(&self.header, current, new_password, params, computer)?;
        self.apply_slot(slot)
    }

    /// Issue a new recovery key (the old one stops working). Needs the current password or kit.
    pub fn rotate_recovery_key(
        &mut self,
        current: Secret<'_>,
    ) -> Result<Zeroizing<String>, VaultError> {
        let (slot, kit) = NewSlot::recovery(&self.header, current)?;
        self.apply_slot(slot)?;
        Ok(kit)
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
            KeySlot::Password { .. } | KeySlot::WindowsHello { .. } | KeySlot::Google { .. } => {
                false
            }
        })
    }

    // ---------------------------------------------------------------- audit

    /// Pseudonymous reference to a case for the audit log (no case id or code in the log).
    #[must_use]
    pub fn case_ref(&self, case_id: &str) -> String {
        hex(&hmac_sha256(
            &self.keys.index,
            format!("case|{case_id}").as_bytes(),
        ))
        .chars()
        .take(16)
        .collect()
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

    /// The newest entry of `event` whose details hold `needle`, without reading the whole log.
    pub fn audit_find(&self, event: &str, needle: &str) -> Result<Option<AuditEntry>, VaultError> {
        audit::find(&self.audit, event, needle)
    }

    /// A page of the log, newest first, below `before` (a `seq`).
    pub fn audit_page(
        &self,
        before: Option<i64>,
        limit: u32,
    ) -> Result<Vec<AuditEntry>, VaultError> {
        audit::page(&self.audit, before, limit)
    }

    /// Walk the whole chain again now (not only at unlock).
    /// The number of the newest log entry (0 when the log is empty), for the rollback seal.
    pub fn audit_seq(&self) -> i64 {
        self.header.audit_anchor.as_ref().map_or(0, |a| a.seq)
    }

    pub fn audit_intact(&self) -> Result<bool, VaultError> {
        Ok(matches!(
            audit::verify(
                &self.audit,
                &self.keys.audit_mac,
                self.header.audit_anchor.as_ref()
            )?,
            ChainStatus::Intact { .. } | ChainStatus::AnchorLagging { .. }
        ))
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

    /// A setting Einat chose: written, and logged as a change of settings.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        self.set_state(key, value)?;
        self.record(
            AuditEvent::SettingsChanged,
            None,
            &serde_json::json!({ "key": key }),
        )
    }

    /// State the program keeps for itself (this month's token counts, when the last backup was
    /// written, an edit kept at lock): the same table, with no log entry and no header write,
    /// since the event it follows is logged on its own and the log would only repeat it.
    pub fn set_state(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        self.main.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// The key under which the name model's spans for `text` are kept: a keyed hash, so the
    /// table says nothing about the text without the vault's keys.
    #[must_use]
    pub fn name_hints_key(&self, revision: &str, text: &str) -> String {
        hex(&hmac_sha256(
            &self.keys.index,
            format!("name_hints|{revision}|{text}").as_bytes(),
        ))
    }

    /// The name model's spans kept for a text (JSON), if it read it before.
    pub fn name_hints(&self, key: &str) -> Result<Option<String>, VaultError> {
        Ok(self
            .main
            .query_row("SELECT hints FROM name_hints WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_name_hints(
        &mut self,
        key: &str,
        case_id: &str,
        hints: &str,
    ) -> Result<(), VaultError> {
        self.main.execute(
            "INSERT OR REPLACE INTO name_hints (key, case_id, hints) VALUES (?1, ?2, ?3)",
            params![key, case_id, hints],
        )?;
        Ok(())
    }

    /// Drop every kept span whose key is not in `keep` (texts that are gone); returns how many.
    pub fn keep_name_hints(
        &mut self,
        keep: &std::collections::HashSet<String>,
    ) -> Result<usize, VaultError> {
        let keys: Vec<String> = self
            .main
            .prepare("SELECT key FROM name_hints")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let tx = self.main.unchecked_transaction()?;
        let mut n = 0;
        for k in keys.iter().filter(|k| !keep.contains(*k)) {
            n += tx.execute("DELETE FROM name_hints WHERE key = ?1", [k])?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Forget every kept span (the model was turned off).
    pub fn clear_name_hints(&mut self) -> Result<(), VaultError> {
        self.main.execute("DELETE FROM name_hints", [])?;
        Ok(())
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

    /// Whether the vault holds a case at all, at work or in the recycle bin: one query and no
    /// decryption, since the backup timer asks every 15 seconds.
    pub fn has_cases(&self) -> Result<bool, VaultError> {
        Ok(self
            .main
            .query_row("SELECT EXISTS(SELECT 1 FROM cases)", [], |r| r.get(0))?)
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
                    failed_at: None,
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

    // ---------------------------------------------------------------- consultations

    /// The key a consultation is sealed with: its case's, or the settings key for a general one.
    fn consultation_key(&self, case_id: Option<&str>) -> Result<Key32, VaultError> {
        match case_id {
            Some(c) => self.case_key(c),
            None => Ok(self.keys.settings.clone()),
        }
    }

    /// Create (`id` = None) or replace a consultation's turns (opaque JSON, sealed). Returns its id.
    pub fn save_consultation(
        &mut self,
        id: Option<&str>,
        case_id: Option<&str>,
        turns_json: &str,
    ) -> Result<String, VaultError> {
        let key = self.consultation_key(case_id)?;
        let t = now();
        let id = match id {
            Some(id) => id.to_owned(),
            None => random_id()?,
        };
        let sealed = seal_str(
            &key,
            &aad("consultations", "turns", &id, case_id.unwrap_or("")),
            turns_json,
        )?;
        let changed = self.main.execute(
            "INSERT INTO consultations (id, case_id, created_at, updated_at, turns_enc)
             VALUES (?1, ?2, ?3, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET updated_at = ?3, turns_enc = ?4
             WHERE consultations.case_id IS ?2",
            params![id, case_id, t, sealed],
        )?;
        // A conversation never moves to another case: nothing is saved, and the caller knows.
        if changed != 1 {
            return Err(VaultError::Refused(
                "the conversation belongs to another case".to_owned(),
            ));
        }
        Ok(id)
    }

    /// Every saved consultation, newest first.
    /// A consultation whose case is in the recycle bin is left out until it comes back.
    pub fn consultations(&self) -> Result<Vec<StoredConsultation>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT c.id, c.case_id, c.created_at, c.updated_at, c.turns_enc FROM consultations c
             LEFT JOIN cases k ON k.id = c.case_id
             WHERE c.case_id IS NULL OR (k.id IS NOT NULL AND k.deleted_at IS NULL)
             ORDER BY c.updated_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    StoredConsultation {
                        id: r.get(0)?,
                        case_id: r.get(1)?,
                        created_at: r.get(2)?,
                        updated_at: r.get(3)?,
                        turns_json: String::new(),
                    },
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(mut row, sealed)| {
                let key = self.consultation_key(row.case_id.as_deref())?;
                row.turns_json = crate::crypto::open_string(
                    &key,
                    &aad(
                        "consultations",
                        "turns",
                        &row.id,
                        row.case_id.as_deref().unwrap_or(""),
                    ),
                    &sealed,
                )?;
                Ok(row)
            })
            .collect()
    }

    /// One saved consultation: `(case_id, turns)`, or `None`. Hidden while its case is in the
    /// recycle bin, like in the list.
    pub fn consultation(&self, id: &str) -> Result<Option<(Option<String>, String)>, VaultError> {
        let row: Option<(Option<String>, Vec<u8>)> = self
            .main
            .query_row(
                "SELECT c.case_id, c.turns_enc FROM consultations c
                 LEFT JOIN cases k ON k.id = c.case_id
                 WHERE c.id = ?1 AND (c.case_id IS NULL OR (k.id IS NOT NULL AND k.deleted_at IS NULL))",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((case_id, sealed)) = row else {
            return Ok(None);
        };
        let key = self.consultation_key(case_id.as_deref())?;
        let json = crate::crypto::open_string(
            &key,
            &aad(
                "consultations",
                "turns",
                id,
                case_id.as_deref().unwrap_or(""),
            ),
            &sealed,
        )?;
        Ok(Some((case_id, json)))
    }

    /// Erase one conversation, as thoroughly as a case: out of the write-ahead log too.
    pub fn delete_consultation(&mut self, id: &str) -> Result<(), VaultError> {
        let case_id: Option<Option<String>> = self
            .main
            .query_row(
                "SELECT case_id FROM consultations WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(case_id) = case_id else {
            return Err(VaultError::NotFound);
        };
        self.main
            .execute("DELETE FROM consultations WHERE id = ?1", [id])?;
        self.main
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.record(
            AuditEvent::ConsultationDeleted,
            case_id.as_deref(),
            &serde_json::json!({}),
        )
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
        for table in [
            "inputs",
            "messages",
            "drafts",
            "draft_versions",
            "transmissions",
            "consultations",
            "name_hints",
            "masks",
        ] {
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
            "SELECT id, role, tag, value_enc, aliases_enc, source, reason FROM identities
             WHERE case_id = ?1 ORDER BY rowid",
        )?;
        let rows: Vec<IdentityRow> = stmt
            .query_map([case_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, role, tag, value, aliases, source, reason)| {
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
                    source: enum_from(&source)?,
                    reason,
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

    /// Replace the case's identity list. Existing identities keep their tags (tags are stable),
    /// and their source while the role stays: a role the psychologist changes is hers.
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
            let (id, tag, source, reason) = match previous {
                Some(p) if p.role == input.role => {
                    (p.id.clone(), p.tag.clone(), p.source, p.reason.clone())
                }
                // A new role gets a new tag; where the name came from stays as it was.
                _ => {
                    let tag = assign_tag(input.role, &used);
                    used.push(tag.clone());
                    (
                        random_id()?,
                        tag,
                        previous.map_or(IdentitySource::Manual, |p| p.source),
                        previous.map_or_else(String::new, |p| p.reason.clone()),
                    )
                }
            };
            let value_enc = seal_str(&key, &aad("identities", "value", &id, case_id), value)?;
            let aliases_enc = seal_str(
                &key,
                &aad("identities", "aliases", &id, case_id),
                &to_json(&aliases)?,
            )?;
            tx.execute(
                "INSERT INTO identities
                    (id, case_id, role, tag, created_at, value_enc, aliases_enc, source, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET role = excluded.role, tag = excluded.tag,
                    value_enc = excluded.value_enc, aliases_enc = excluded.aliases_enc,
                    source = excluded.source, reason = excluded.reason",
                params![
                    id,
                    case_id,
                    enum_str(&input.role)?,
                    tag,
                    t,
                    value_enc,
                    aliases_enc,
                    enum_str(&source)?,
                    reason
                ],
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

    /// Keep names the filter hid on its own as identities of the case, each with a new tag.
    /// A value the case already has (as a value or an alias) is skipped. Returns the identities
    /// added.
    pub fn add_found_names(
        &mut self,
        case_id: &str,
        names: &[FoundName],
    ) -> Result<Vec<Identity>, VaultError> {
        let key = self.case_key(case_id)?;
        let existing = self.identities(case_id)?;
        let mut used: Vec<String> = existing.iter().map(|i| i.tag.clone()).collect();
        let mut known: Vec<String> = existing
            .iter()
            .flat_map(|i| std::iter::once(i.value.clone()).chain(i.aliases.iter().cloned()))
            .map(|v| v.trim().to_owned())
            .collect();
        let t = now();
        let tx = self.identity.unchecked_transaction()?;
        let mut added = Vec::new();
        for name in names {
            let value = name.value.trim();
            if value.is_empty() || known.iter().any(|k| k == value) {
                continue;
            }
            known.push(value.to_owned());
            let tag = assign_tag(name.role, &used);
            used.push(tag.clone());
            let id = random_id()?;
            let value_enc = seal_str(&key, &aad("identities", "value", &id, case_id), value)?;
            let aliases_enc = seal_str(&key, &aad("identities", "aliases", &id, case_id), "[]")?;
            tx.execute(
                "INSERT INTO identities
                    (id, case_id, role, tag, created_at, value_enc, aliases_enc, source, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    case_id,
                    enum_str(&name.role)?,
                    tag,
                    t,
                    value_enc,
                    aliases_enc,
                    enum_str(&name.source)?,
                    name.reason
                ],
            )?;
            added.push(Identity {
                id,
                case_id: case_id.to_owned(),
                role: name.role,
                tag,
                value: value.to_owned(),
                aliases: Vec::new(),
                source: name.source,
                reason: name.reason.clone(),
            });
        }
        tx.commit()?;
        if !added.is_empty() {
            self.touch(case_id)?;
            self.record(
                AuditEvent::IdentitiesChanged,
                Some(case_id),
                &serde_json::json!({ "found": added.len() }),
            )?;
        }
        Ok(added)
    }

    /// Remove one identity of the case (a name the filter added that is not a name).
    pub fn remove_identity(&mut self, case_id: &str, identity_id: &str) -> Result<(), VaultError> {
        self.identity.execute(
            "DELETE FROM identities WHERE id = ?1 AND case_id = ?2",
            params![identity_id, case_id],
        )?;
        self.touch(case_id)?;
        self.record(
            AuditEvent::IdentitiesChanged,
            Some(case_id),
            &serde_json::json!({ "removed": 1 }),
        )
    }

    /// The psychologist confirms a name the filter kept by itself: it is hers from now on,
    /// like one she typed (U2). Its tag, value and aliases stay; only the source changes.
    pub fn confirm_identity(&mut self, case_id: &str, identity_id: &str) -> Result<(), VaultError> {
        let changed = self.identity.execute(
            "UPDATE identities SET source = ?1 WHERE id = ?2 AND case_id = ?3",
            params![enum_str(&IdentitySource::Manual)?, identity_id, case_id],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        self.touch(case_id)?;
        self.record(
            AuditEvent::IdentitiesChanged,
            Some(case_id),
            &serde_json::json!({ "confirmed": 1 }),
        )
    }

    /// Keyed hash of a normalized token, for the "this is not a name" allow-list.
    #[must_use]
    pub fn token_hmac(&self, token: &str) -> String {
        token_hmac(&self.keys.index, token)
    }

    /// The same keyed hash of a word, for work away from the open vault (F2). It holds a copy
    /// of the index key, wiped when it is dropped.
    #[must_use]
    pub fn token_hasher(&self) -> TokenHasher {
        TokenHasher(self.keys.index.clone())
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

    /// Deletes a material and hands back its row exactly as stored (still sealed), so the
    /// deletion can be undone right after. `None` when there was no such material.
    pub fn take_input(
        &mut self,
        case_id: &str,
        input_id: &str,
    ) -> Result<Option<RemovedInput>, VaultError> {
        let row = self
            .main
            .query_row(
                "SELECT kind, created_at, title_enc, content_enc, data_enc, routing_enc
                 FROM inputs WHERE id = ?1 AND case_id = ?2",
                params![input_id, case_id],
                |r| {
                    Ok(RemovedInput {
                        id: input_id.to_owned(),
                        case_id: case_id.to_owned(),
                        kind: r.get(0)?,
                        created_at: r.get(1)?,
                        title_enc: r.get(2)?,
                        content_enc: r.get(3)?,
                        data_enc: r.get(4)?,
                        routing_enc: r.get(5)?,
                    })
                },
            )
            .optional()?;
        if row.is_some() {
            self.delete_input(case_id, input_id)?;
        }
        Ok(row)
    }

    /// Puts back a material removed by `take_input`, with its id, time and sealed contents.
    pub fn restore_input(&mut self, row: &RemovedInput) -> Result<(), VaultError> {
        self.main.execute(
            "INSERT INTO inputs (id, case_id, kind, created_at, title_enc, content_enc, data_enc, routing_enc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.id,
                row.case_id,
                row.kind,
                row.created_at,
                row.title_enc,
                row.content_enc,
                row.data_enc,
                row.routing_enc
            ],
        )?;
        self.touch(&row.case_id)
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
            replaces: None,
        })
    }

    /// Put a paragraph right after `after` in its section (`None`: first), and number the
    /// section's paragraphs again in that order. Only positions change.
    pub fn place_draft(
        &mut self,
        case_id: &str,
        draft_id: &str,
        after: Option<&str>,
    ) -> Result<(), VaultError> {
        let section: String = self
            .main
            .query_row(
                "SELECT section_key FROM drafts WHERE id = ?1 AND case_id = ?2",
                params![draft_id, case_id],
                |r| r.get(0),
            )
            .map_err(|_| VaultError::NotFound)?;
        let mut order: Vec<String> = {
            let mut stmt = self.main.prepare(
                "SELECT id FROM drafts WHERE case_id = ?1 AND section_key = ?2 AND id != ?3
                 ORDER BY position, created_at",
            )?;
            let rows = stmt.query_map(params![case_id, section, draft_id], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let at = match after {
            None => 0,
            Some(a) => {
                order
                    .iter()
                    .position(|x| x == a)
                    .ok_or(VaultError::NotFound)?
                    + 1
            }
        };
        order.insert(at, draft_id.to_owned());
        let tx = self.main.transaction()?;
        for (i, id) in order.iter().enumerate() {
            tx.execute(
                "UPDATE drafts SET position = ?1 WHERE id = ?2 AND case_id = ?3",
                params![u32::try_from(i + 1).unwrap_or(u32::MAX), id, case_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn drafts(
        &self,
        case_id: &str,
        section_key: &str,
    ) -> Result<Vec<DraftParagraph>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.main.prepare(
            "SELECT id, position, status, author, created_at, approved_at, text_tagged_enc, source_refs_enc, replaces
             FROM drafts WHERE case_id = ?1 AND section_key = ?2 ORDER BY position, created_at",
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
            Option<String>,
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
                    r.get(8)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(
                |(id, position, status, author, created_at, approved_at, text, refs, replaces)| {
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
                        replaces,
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
        if status == DraftStatus::Approved {
            // A new wording she approved takes the old one's place.
            self.main.execute(
                "UPDATE drafts SET status = 'superseded'
                 WHERE case_id = ?1 AND status = 'approved'
                   AND id = (SELECT replaces FROM drafts WHERE id = ?2 AND case_id = ?1)",
                params![case_id, draft_id],
            )?;
        }
        self.touch(case_id)
    }

    /// A new draft from Claude replaces the paragraphs it proposed before and she has not
    /// approved (they are kept, marked superseded); hers and the approved ones stay.
    pub fn supersede_proposals(
        &mut self,
        case_id: &str,
        section_key: &str,
    ) -> Result<usize, VaultError> {
        let n = self.main.execute(
            "UPDATE drafts SET status = 'superseded'
             WHERE case_id = ?1 AND section_key = ?2 AND status = 'proposed' AND author = 'ai'",
            params![case_id, section_key],
        )?;
        self.touch(case_id)?;
        Ok(n)
    }

    /// Claude's new wording of a paragraph she approved: a proposal right beside it, which
    /// replaces it only when approved. False when that paragraph is not approved any more.
    pub fn propose_rewording(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text_tagged: &str,
        source_refs: &[String],
    ) -> Result<bool, VaultError> {
        let key = self.case_key(case_id)?;
        let target: Option<(String, u32)> = self
            .main
            .query_row(
                "SELECT section_key, position FROM drafts WHERE id = ?1 AND case_id = ?2 AND status = 'approved'",
                params![draft_id, case_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((section_key, position)) = target else {
            return Ok(false);
        };
        // Only one new wording waits per paragraph.
        self.main.execute(
            "UPDATE drafts SET status = 'superseded' WHERE case_id = ?1 AND replaces = ?2 AND status = 'proposed'",
            params![case_id, draft_id],
        )?;
        let id = random_id()?;
        self.main.execute(
            "INSERT INTO drafts (id, case_id, section_key, position, status, author, created_at, approved_at, text_tagged_enc, source_refs_enc, replaces)
             VALUES (?1, ?2, ?3, ?4, 'proposed', 'ai', ?5, NULL, ?6, ?7, ?8)",
            params![
                id,
                case_id,
                section_key,
                position,
                now(),
                seal_str(&key, &aad("drafts", "text", &id, case_id), text_tagged)?,
                seal_str(&key, &aad("drafts", "refs", &id, case_id), &to_json(&source_refs)?)?,
                draft_id
            ],
        )?;
        self.touch(case_id)?;
        Ok(true)
    }

    /// Claude's rewrite of one paragraph it proposed, in the same place. False when the
    /// paragraph is no longer a proposal (approved, edited or removed meanwhile).
    pub fn rewrite_proposal(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text_tagged: &str,
        source_refs: &[String],
    ) -> Result<bool, VaultError> {
        let key = self.case_key(case_id)?;
        let proposed: Option<String> = self
            .main
            .query_row(
                "SELECT status FROM drafts WHERE id = ?1 AND case_id = ?2",
                params![draft_id, case_id],
                |r| r.get(0),
            )
            .optional()?;
        if proposed.as_deref() == Some("proposed") {
            self.keep_version(case_id, draft_id, Some(text_tagged))?;
        }
        let changed = self.main.execute(
            "UPDATE drafts SET text_tagged_enc = ?1, source_refs_enc = ?2
             WHERE id = ?3 AND case_id = ?4 AND status = 'proposed'",
            params![
                seal_str(&key, &aad("drafts", "text", draft_id, case_id), text_tagged)?,
                seal_str(
                    &key,
                    &aad("drafts", "refs", draft_id, case_id),
                    &to_json(&source_refs)?
                )?,
                draft_id,
                case_id
            ],
        )?;
        if changed > 0 {
            self.touch(case_id)?;
        }
        Ok(changed > 0)
    }

    /// Manual edit by the psychologist: the paragraph becomes hers and is approved.
    pub fn edit_draft(
        &mut self,
        case_id: &str,
        draft_id: &str,
        text_tagged: &str,
    ) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        self.keep_version(case_id, draft_id, Some(text_tagged))?;
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

    /// How many rows were written in the case data and in the identities since the vault was
    /// opened: a pair that has not moved means no text and no name changed in between.
    #[must_use]
    pub fn change_counts(&self) -> (u64, u64) {
        (self.main.total_changes(), self.identity.total_changes())
    }

    /// Rewrites the case's stored tagged text (paragraphs, their earlier wordings, the
    /// conversations) where `change` returns a new text. Status, author and versions stay:
    /// this settles placeholders, it is not her edit. Returns how many texts changed.
    pub fn rewrite_case_texts(
        &mut self,
        case_id: &str,
        change: &dyn Fn(&str) -> Option<String>,
    ) -> Result<usize, VaultError> {
        let key = self.case_key(case_id)?;
        let tx = self.main.unchecked_transaction()?;
        let mut changed = 0;
        for table in ["drafts", "draft_versions", "messages"] {
            let rows: Vec<(String, Vec<u8>)> = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT id, text_tagged_enc FROM {table} WHERE case_id = ?1"
                ))?;
                let rows = stmt
                    .query_map(params![case_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<_, _>>()?;
                rows
            };
            for (id, sealed) in rows {
                let aad = aad(table, "text", &id, case_id);
                let text = open_string(&key, &aad, &sealed)?;
                if let Some(new) = change(&text) {
                    tx.execute(
                        &format!("UPDATE {table} SET text_tagged_enc = ?1 WHERE id = ?2 AND case_id = ?3"),
                        params![seal_str(&key, &aad, &new)?, id, case_id],
                    )?;
                    changed += 1;
                }
            }
        }
        tx.commit()?;
        if changed > 0 {
            self.record(
                AuditEvent::IdentitiesChanged,
                Some(case_id),
                &serde_json::json!({ "settled_tags": changed }),
            )?;
        }
        Ok(changed)
    }

    /// Keep the paragraph's current wording as a version before it changes in place. Nothing
    /// is kept when the new text is the same (`unchanged_if`), or when the paragraph is gone.
    fn keep_version(
        &mut self,
        case_id: &str,
        draft_id: &str,
        unchanged_if: Option<&str>,
    ) -> Result<(), VaultError> {
        let key = self.case_key(case_id)?;
        let row: Option<(String, Vec<u8>)> = self
            .main
            .query_row(
                "SELECT author, text_tagged_enc FROM drafts WHERE id = ?1 AND case_id = ?2",
                params![draft_id, case_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((author, sealed)) = row else {
            return Ok(());
        };
        let current = open_string(&key, &aad("drafts", "text", draft_id, case_id), &sealed)?;
        if unchanged_if == Some(current.as_str()) {
            return Ok(());
        }
        let id = random_id()?;
        self.main.execute(
            "INSERT INTO draft_versions (id, case_id, draft_id, saved_at, author, text_tagged_enc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                case_id,
                draft_id,
                now(),
                author,
                seal_str(&key, &aad("draft_versions", "text", &id, case_id), &current)?
            ],
        )?;
        Ok(())
    }

    /// The earlier wordings of a paragraph, newest first (D-046): what it said before each edit
    /// or rewording in place, and, through `replaces`, the approved paragraphs it took over from.
    pub fn draft_versions(
        &self,
        case_id: &str,
        draft_id: &str,
    ) -> Result<Vec<DraftVersion>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut out = Vec::new();
        let mut current = Some(draft_id.to_owned());
        let mut first = true;
        for _ in 0..MAX_VERSION_CHAIN {
            let Some(id) = current.take() else { break };
            let row: Option<ChainRow> = self
                .main
                .query_row(
                    "SELECT author, created_at, approved_at, text_tagged_enc, replaces
                     FROM drafts WHERE id = ?1 AND case_id = ?2",
                    params![id, case_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?;
            let Some((author, created_at, approved_at, sealed, replaces)) = row else {
                if first {
                    return Err(VaultError::NotFound);
                }
                break;
            };
            if !first {
                // The approved paragraph that a new wording replaced, as it last read.
                out.push(DraftVersion {
                    id: id.clone(),
                    saved_at: approved_at.unwrap_or(created_at),
                    author: enum_from(&author)?,
                    text_tagged: open_string(&key, &aad("drafts", "text", &id, case_id), &sealed)?,
                });
            }
            let mut stmt = self.main.prepare(
                "SELECT id, saved_at, author, text_tagged_enc FROM draft_versions
                 WHERE case_id = ?1 AND draft_id = ?2 ORDER BY saved_at DESC, rowid DESC",
            )?;
            let rows: Vec<(String, i64, String, Vec<u8>)> = stmt
                .query_map(params![case_id, id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .collect::<Result<_, _>>()?;
            for (vid, saved_at, author, sealed) in rows {
                out.push(DraftVersion {
                    text_tagged: open_string(
                        &key,
                        &aad("draft_versions", "text", &vid, case_id),
                        &sealed,
                    )?,
                    author: enum_from(&author)?,
                    id: vid,
                    saved_at,
                });
            }
            first = false;
            current = replaces;
        }
        Ok(out)
    }

    /// Bring back an earlier wording (D-046): the current one is kept as a version, and the
    /// paragraph reads as it did then, approved by her. Who wrote it stays as it was.
    pub fn restore_draft_version(
        &mut self,
        case_id: &str,
        draft_id: &str,
        version_id: &str,
    ) -> Result<(), VaultError> {
        let version = self
            .draft_versions(case_id, draft_id)?
            .into_iter()
            .find(|v| v.id == version_id)
            .ok_or(VaultError::NotFound)?;
        let key = self.case_key(case_id)?;
        self.keep_version(case_id, draft_id, Some(&version.text_tagged))?;
        let changed = self.main.execute(
            "UPDATE drafts SET text_tagged_enc = ?1, author = ?2, status = 'approved', approved_at = ?3
             WHERE id = ?4 AND case_id = ?5 AND status IN ('approved', 'proposed')",
            params![
                seal_str(&key, &aad("drafts", "text", draft_id, case_id), &version.text_tagged)?,
                enum_str(&version.author)?,
                now(),
                draft_id,
                case_id
            ],
        )?;
        if changed == 0 {
            return Err(VaultError::NotFound);
        }
        // A new wording that waited beside it would replace what she just chose.
        self.main.execute(
            "UPDATE drafts SET status = 'superseded' WHERE case_id = ?1 AND replaces = ?2 AND status = 'proposed'",
            params![case_id, draft_id],
        )?;
        self.touch(case_id)
    }

    /// Which of these paragraphs have earlier wordings to show.
    pub fn drafts_with_versions(&self, case_id: &str) -> Result<Vec<String>, VaultError> {
        let mut stmt = self.main.prepare(
            "SELECT DISTINCT draft_id FROM draft_versions WHERE case_id = ?1
             UNION SELECT id FROM drafts WHERE case_id = ?1 AND replaces IS NOT NULL
               AND status = 'approved'",
        )?;
        let rows = stmt.query_map([case_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
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

    /// The case's mask book, in the order its tags were given.
    pub fn masks(&self, case_id: &str) -> Result<Vec<MaskEntry>, VaultError> {
        let key = self.case_key(case_id)?;
        let mut stmt = self.main.prepare(
            "SELECT tag, kind, original_enc FROM masks WHERE case_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([case_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(tag, kind, enc)| {
                Ok(MaskEntry {
                    original: open_string(&key, &aad("masks", "original", &tag, case_id), &enc)?,
                    tag,
                    kind,
                })
            })
            .collect()
    }

    /// Keep new tags of the case's mask book. A tag it already holds keeps its words: a tag
    /// once given never comes to mean other words.
    pub fn add_masks(&mut self, case_id: &str, entries: &[MaskEntry]) -> Result<(), VaultError> {
        if entries.is_empty() {
            return Ok(());
        }
        let key = self.case_key(case_id)?;
        let t = now();
        let tx = self.main.unchecked_transaction()?;
        for e in entries {
            tx.execute(
                "INSERT OR IGNORE INTO masks (case_id, tag, kind, created_at, original_enc)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    case_id,
                    e.tag,
                    e.kind,
                    t,
                    seal_str(
                        &key,
                        &aad("masks", "original", &e.tag, case_id),
                        &e.original
                    )?
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
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
