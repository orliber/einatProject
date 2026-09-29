//! Encrypted backup of the whole vault (`.vaultbak`, STANDARDS §5.6).
//!
//! ```text
//! "DVBAK\0\0\x01" | u32 header length | header | i64 created_at | sealed container
//! ```
//!
//! * The header is the vault's own `vault.header`: public by design (wrapped keys, KDF
//!   parameters). It is what lets the vault's password or printed recovery kit open the backup,
//!   so a backup needs no new secret that could be lost.
//! * The container holds a consistent snapshot of the three databases, made with
//!   `sqlcipher_export` into files that are encrypted with the databases' own keys: nothing is
//!   ever written in the clear, not even for a moment.
//! * The container is sealed (AES-256-GCM) with a key derived from the master key and bound to
//!   the vault id and the backup time: a changed byte, or a header swapped in from another vault,
//!   and the backup does not open.

use std::fs;
use std::io::Write;
use std::path::Path;

use rusqlite::Connection;
use zeroize::Zeroizing;

use super::{master_key, now, Keys, Secret, Vault, AUDIT_DB, IDENTITY_DB, MAIN_DB};
use crate::audit::AuditEvent;
use crate::crypto::{hex, open, random_id, seal, Key32};
use crate::header::{VaultHeader, HEADER_FILE};
use crate::VaultError;

const MAGIC: &[u8; 8] = b"DVBAK\0\0\x01";
const FILES: [&str; 3] = [MAIN_DB, IDENTITY_DB, AUDIT_DB];
pub const BACKUP_EXTENSION: &str = "vaultbak";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub bytes: u64,
    pub created_at: i64,
}

/// What can be read from a backup without its secret: when it was made, and by which vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupPeek {
    pub created_at: i64,
    pub vault_id: String,
}

/// Read a backup's public part (to show its date before asking for the password).
pub fn peek_backup(backup: &[u8]) -> Result<BackupPeek, VaultError> {
    let parsed = parse(backup)?;
    Ok(BackupPeek {
        created_at: parsed.created_at,
        vault_id: parsed.header.vault_id,
    })
}

/// What a backup holds, found by opening it (in a drill, or on restore).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupCheck {
    pub created_at: i64,
    /// Cases at work and in the recycle bin.
    pub cases: u32,
    pub integrity_ok: bool,
}

fn aad(vault_id: &str, created_at: i64) -> Vec<u8> {
    format!("dv/backup/v1|{vault_id}|{created_at}").into_bytes()
}

fn corrupt(what: &str) -> VaultError {
    VaultError::Corrupt(format!("backup: {what}"))
}

/// Copy a database into `path`, encrypted with the same key (never a clear-text step).
fn export(conn: &Connection, path: &Path, key: &Key32) -> Result<(), VaultError> {
    let target = path
        .to_str()
        .ok_or_else(|| corrupt("path"))?
        .replace('\'', "''");
    let key_hex = Zeroizing::new(hex(key.as_bytes()));
    let attach = Zeroizing::new(format!(
        "ATTACH DATABASE '{target}' AS dv_backup KEY \"x'{}'\";",
        key_hex.as_str()
    ));
    conn.execute_batch(&attach)?;
    let exported = conn.query_row("SELECT sqlcipher_export('dv_backup')", [], |_| Ok(()));
    conn.execute_batch("DETACH DATABASE dv_backup;")?;
    exported?;
    Ok(())
}

fn push_entry(out: &mut Vec<u8>, name: &str, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(name.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&u64::try_from(bytes.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Take `n` bytes from the front of `rest`.
fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], VaultError> {
    if rest.len() < n {
        return Err(corrupt("truncated"));
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn u32_at(rest: &mut &[u8]) -> Result<usize, VaultError> {
    let b: [u8; 4] = take(rest, 4)?.try_into().map_err(|_| corrupt("length"))?;
    usize::try_from(u32::from_le_bytes(b)).map_err(|_| corrupt("length"))
}

fn u64_at(rest: &mut &[u8]) -> Result<usize, VaultError> {
    let b: [u8; 8] = take(rest, 8)?.try_into().map_err(|_| corrupt("length"))?;
    usize::try_from(u64::from_le_bytes(b)).map_err(|_| corrupt("length"))
}

struct Parsed<'a> {
    header_bytes: &'a [u8],
    header: VaultHeader,
    created_at: i64,
    sealed: &'a [u8],
}

fn parse(backup: &[u8]) -> Result<Parsed<'_>, VaultError> {
    let mut rest = backup;
    if take(&mut rest, MAGIC.len())? != MAGIC {
        return Err(corrupt("not a vault backup"));
    }
    let len = u32_at(&mut rest)?;
    let header_bytes = take(&mut rest, len)?;
    let header = VaultHeader::parse(header_bytes)?;
    let created: [u8; 8] = take(&mut rest, 8)?
        .try_into()
        .map_err(|_| corrupt("time"))?;
    Ok(Parsed {
        header_bytes,
        header,
        created_at: i64::from_le_bytes(created),
        sealed: rest,
    })
}

/// Remove scratch folders that a crash may have left behind (they hold only encrypted copies).
pub(super) fn remove_stale_scratch(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with(".backup-") || name.starts_with(".drill-"))
            && entry.file_type().is_ok_and(|t| t.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

impl Vault {
    /// Write an encrypted backup of the whole vault to `dest` (replaced atomically).
    pub fn write_backup(&mut self, dest: &Path) -> Result<BackupInfo, VaultError> {
        // A scratch folder next to the vault, for the encrypted snapshots only.
        let scratch = self.dir.join(format!(".backup-{}", random_id()?));
        fs::create_dir(&scratch)?;
        let snapshots = (|| -> Result<Zeroizing<Vec<u8>>, VaultError> {
            let mut container = Zeroizing::new(Vec::new());
            for (name, conn, key) in [
                (MAIN_DB, &self.main, &self.keys.db_main),
                (IDENTITY_DB, &self.identity, &self.keys.db_identity),
                (AUDIT_DB, &self.audit, &self.keys.db_audit),
            ] {
                let path = scratch.join(name);
                export(conn, &path, key)?;
                push_entry(&mut container, name, &fs::read(&path)?);
            }
            Ok(container)
        })();
        let _ = fs::remove_dir_all(&scratch);
        let container = snapshots?;

        let header_bytes = fs::read(self.dir.join(HEADER_FILE))?;
        let created_at = now();
        let sealed = seal(
            &self.keys.backup,
            &aad(&self.header.vault_id, created_at),
            &container,
        )?;
        let mut out = Vec::with_capacity(MAGIC.len() + 4 + header_bytes.len() + 8 + sealed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(
            &u32::try_from(header_bytes.len())
                .map_err(|_| corrupt("header size"))?
                .to_le_bytes(),
        );
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(&created_at.to_le_bytes());
        out.extend_from_slice(&sealed);

        let part = dest.with_extension(format!("{BACKUP_EXTENSION}.part"));
        {
            let mut f = fs::File::create(&part)?;
            f.write_all(&out)?;
            f.sync_all()?;
        }
        fs::rename(&part, dest)?;
        let bytes = u64::try_from(out.len()).unwrap_or(u64::MAX);
        self.record(
            AuditEvent::BackupWritten,
            None,
            &serde_json::json!({ "bytes": bytes }),
        )?;
        Ok(BackupInfo { bytes, created_at })
    }

    /// Open a backup into `target` (which must not hold a vault) and return the opened vault.
    /// Used to restore on a new computer, and by the restore drill on a scratch folder.
    pub fn restore_backup(
        backup: &[u8],
        target: &Path,
        secret: Secret<'_>,
    ) -> Result<(Vault, BackupCheck), VaultError> {
        if target.join(HEADER_FILE).exists() {
            return Err(VaultError::AlreadyExists);
        }
        let parsed = parse(backup)?;
        let mk = master_key(&parsed.header, secret)?;
        let keys = Keys::derive(&mk)?;
        let container = open(
            &keys.backup,
            &aad(&parsed.header.vault_id, parsed.created_at),
            parsed.sealed,
        )
        .map_err(|_| VaultError::Integrity("the backup was changed or is not from this vault"))?;

        let mut rest: &[u8] = &container;
        let mut files = Vec::new();
        while !rest.is_empty() {
            let name_len = u32_at(&mut rest)?;
            let name = std::str::from_utf8(take(&mut rest, name_len)?)
                .map_err(|_| corrupt("name"))?
                .to_owned();
            let len = u64_at(&mut rest)?;
            files.push((name, take(&mut rest, len)?));
        }
        let mut names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        let mut expected = FILES.to_vec();
        expected.sort_unstable();
        if names != expected {
            return Err(corrupt("unexpected contents"));
        }

        fs::create_dir_all(target)?;
        let opened = (|| -> Result<Vault, VaultError> {
            for (name, bytes) in &files {
                fs::write(target.join(name), bytes)?;
            }
            fs::write(target.join(HEADER_FILE), parsed.header_bytes)?;
            Vault::open_with(target, parsed.header, &mk)
        })();
        let mut vault = match opened {
            Ok(vault) => vault,
            Err(e) => {
                // Leave nothing half-restored behind: only the files this call wrote.
                for name in FILES.iter().chain([&HEADER_FILE]) {
                    for suffix in ["", "-wal", "-shm"] {
                        let _ = fs::remove_file(target.join(format!("{name}{suffix}")));
                    }
                }
                return Err(e);
            }
        };
        let integrity = vault.integrity();
        let check = BackupCheck {
            created_at: parsed.created_at,
            cases: u32::try_from(vault.list_cases()?.len() + vault.list_trash()?.len())
                .unwrap_or(u32::MAX),
            integrity_ok: integrity.header_ok && integrity.audit_ok,
        };
        vault.record(
            AuditEvent::Restored,
            None,
            &serde_json::json!({ "backup_created_at": parsed.created_at }),
        )?;
        Ok((vault, check))
    }

    /// Restore drill: open a backup in a scratch folder next to this vault, check it, and remove
    /// it again. The live vault is not touched (only the drill is written to its log).
    pub fn check_backup(
        &mut self,
        backup: &[u8],
        secret: Secret<'_>,
    ) -> Result<BackupCheck, VaultError> {
        let scratch = self.dir.join(format!(".drill-{}", random_id()?));
        let result = Vault::restore_backup(backup, &scratch, secret).map(|(v, check)| {
            drop(v);
            check
        });
        let _ = fs::remove_dir_all(&scratch);
        let check = result?;
        self.record(
            AuditEvent::BackupChecked,
            None,
            &serde_json::json!({ "cases": check.cases, "ok": check.integrity_ok }),
        )?;
        Ok(check)
    }
}
