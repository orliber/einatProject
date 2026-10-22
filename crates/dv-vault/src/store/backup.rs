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
use crate::db::open_encrypted;
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

/// Attach `path` as the encrypted export target `dv_backup`, with the same key as the database
/// (never a clear-text step).
fn attach(conn: &Connection, path: &Path, key: &Key32) -> Result<(), VaultError> {
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
/// The container's files, by name; exactly the three databases.
fn entries(container: &[u8]) -> Result<Vec<(String, &[u8])>, VaultError> {
    let mut rest: &[u8] = container;
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
    Ok(files)
}

pub(super) fn remove_stale_scratch(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if (name.starts_with(".backup-")
            || name.starts_with(".drill-")
            || name.starts_with(".verify-"))
            && entry.file_type().is_ok_and(|t| t.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// The keys a backup is checked with once it is on the disk: copies, wiped when the job ends.
struct CheckKeys {
    backup: Key32,
    db_main: Key32,
    db_identity: Key32,
    db_audit: Key32,
}

impl CheckKeys {
    fn of(keys: &Keys) -> Self {
        Self {
            backup: keys.backup.clone(),
            db_main: keys.db_main.clone(),
            db_identity: keys.db_identity.clone(),
            db_audit: keys.db_audit.clone(),
        }
    }
}

/// A backup taken from the open vault, to be written and read back away from it (F2): the
/// drive may be a slow removable one, and the check opens every database. Made by
/// [`Vault::begin_backup`] in a moment; [`BackupJob::write`] and [`WrittenBackup::verify`] do
/// the slow part; [`Vault::finish_backup`] records it. It holds only the sealed file and
/// copies of the keys that check it, wiped when it is dropped.
pub struct BackupJob {
    /// The sealed file; empty until the snapshot is exported ([`BackupJob::seal`]).
    file: Vec<u8>,
    /// The snapshot, taken under the lock and exported away from it.
    pending: Option<Snapshot>,
    created_at: i64,
    vault_id: String,
    /// The vault folder: the check's scratch folder goes next to the vault.
    dir: std::path::PathBuf,
    keys: CheckKeys,
    /// Cases in the vault when the snapshot was taken.
    live_cases: i64,
}

/// A backup on the disk, not yet read back.
#[derive(Debug)]
pub struct WrittenBackup {
    job: BackupJob,
    path: std::path::PathBuf,
}

/// A backup written and opened again from where it went: ready to be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupWritten {
    pub info: BackupInfo,
    pub cases: u32,
}

/// A consistent snapshot of the three databases, taken in a moment under the core's lock: one
/// connection to each, its read transaction begun (the vault is in WAL mode, so each keeps
/// seeing the vault as it was while the vault goes on writing). Exporting it, the slow part on
/// a large vault, happens in the job. Dropped unexported (a lock), its scratch folder goes.
///
/// The three read transactions agree with each other only because they are begun together
/// under the core's lock (`Vault::begin_backup` takes `&mut self`) and the vault owns the only
/// connections that write. While a snapshot waits for its job it holds three keyed connections
/// and its open reads keep the WAL from being checkpointed past them.
struct Snapshot {
    scratch: std::path::PathBuf,
    conns: Vec<(&'static str, Connection)>,
    header_bytes: Vec<u8>,
    backup: Key32,
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        self.conns.clear();
        let _ = fs::remove_dir_all(&self.scratch);
    }
}

impl Snapshot {
    /// Open one connection per database, attach its encrypted export target, and begin the
    /// read transactions together: from here each sees the vault as it is now.
    fn take(vault: &Vault) -> Result<(Self, i64), VaultError> {
        let scratch = vault.dir.join(format!(".backup-{}", random_id()?));
        fs::create_dir(&scratch)?;
        let mut snap = Self {
            scratch,
            conns: Vec::new(),
            header_bytes: vault.backup_header()?,
            backup: vault.keys.backup.clone(),
        };
        for (name, key) in [
            (MAIN_DB, &vault.keys.db_main),
            (IDENTITY_DB, &vault.keys.db_identity),
            (AUDIT_DB, &vault.keys.db_audit),
        ] {
            let conn = open_encrypted(&vault.dir.join(name), key)?;
            attach(&conn, &snap.scratch.join(name), key)?;
            conn.execute_batch("BEGIN DEFERRED;")?;
            // The first read fixes what this transaction sees.
            conn.query_row("SELECT count(*) FROM main.sqlite_master", [], |r| {
                r.get::<_, i64>(0)
            })?;
            snap.conns.push((name, conn));
        }
        let live_cases: i64 = snap
            .conns
            .first()
            .ok_or_else(|| corrupt("no main database"))?
            .1
            .query_row("SELECT count(*) FROM main.cases", [], |r| r.get(0))?;
        Ok((snap, live_cases))
    }

    /// Export each database from its transaction (encrypted, never a clear-text step) and seal
    /// them into the backup file.
    fn export(mut self, created_at: i64, vault_id: &str) -> Result<Vec<u8>, VaultError> {
        let mut container = Zeroizing::new(Vec::new());
        for (name, conn) in std::mem::take(&mut self.conns) {
            let exported = conn.query_row("SELECT sqlcipher_export('dv_backup')", [], |_| Ok(()));
            conn.execute_batch("COMMIT;")?;
            conn.execute_batch("DETACH DATABASE dv_backup;")?;
            exported?;
            drop(conn);
            push_entry(&mut container, name, &fs::read(self.scratch.join(name))?);
        }
        let sealed = seal(&self.backup, &aad(vault_id, created_at), &container)?;
        let header_bytes = &self.header_bytes;
        let mut file = Vec::with_capacity(MAGIC.len() + 4 + header_bytes.len() + 8 + sealed.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(
            &u32::try_from(header_bytes.len())
                .map_err(|_| corrupt("header size"))?
                .to_le_bytes(),
        );
        file.extend_from_slice(header_bytes);
        file.extend_from_slice(&created_at.to_le_bytes());
        file.extend_from_slice(&sealed);
        Ok(file)
    }
}

impl std::fmt::Debug for BackupJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupJob")
            .field("bytes", &self.file.len())
            .finish_non_exhaustive()
    }
}

impl BackupJob {
    /// The size of the sealed file (0 before [`BackupJob::seal`]).
    #[must_use]
    pub fn bytes(&self) -> u64 {
        u64::try_from(self.file.len()).unwrap_or(u64::MAX)
    }

    /// Export the snapshot and seal it: the slow part on a large vault, done away from the
    /// core's lock. Does nothing once sealed.
    pub fn seal(mut self) -> Result<Self, VaultError> {
        if let Some(snap) = self.pending.take() {
            self.file = snap.export(self.created_at, &self.vault_id)?;
        }
        Ok(self)
    }

    /// Seal the file if it is not yet, and write it to `dest`, replaced atomically.
    pub fn write(self, dest: &Path) -> Result<WrittenBackup, VaultError> {
        let this = self.seal()?;
        this.write_sealed(dest)
    }

    fn write_sealed(self, dest: &Path) -> Result<WrittenBackup, VaultError> {
        let part = dest.with_extension(format!("{BACKUP_EXTENSION}.part"));
        let written = (|| -> Result<(), VaultError> {
            let mut f = fs::File::create(&part)?;
            f.write_all(&self.file)?;
            f.sync_all()?;
            fs::rename(&part, dest)?;
            Ok(())
        })();
        if written.is_err() {
            let _ = fs::remove_file(&part);
        }
        written?;
        Ok(WrittenBackup {
            job: self,
            path: dest.to_path_buf(),
        })
    }
}

impl WrittenBackup {
    /// Read the backup back from the disk it went to and open it: the seal, each database
    /// with its own key (`PRAGMA integrity_check`), and as many cases as the vault had. A
    /// backup counts only once it is known to open.
    pub fn verify(self) -> Result<BackupWritten, VaultError> {
        self.verify_keeping_file().map(|(done, _)| done)
    }

    /// As [`WrittenBackup::verify`], and also hand back the bytes that were read back from the
    /// disk and opened: the encrypted copy that goes to her Drive is exactly this (D-099).
    pub fn verify_keeping_file(self) -> Result<(BackupWritten, Vec<u8>), VaultError> {
        let job = &self.job;
        let bytes = fs::read(&self.path)?;
        let (created_at, cases) = check_file(&bytes, &job.vault_id, &job.keys, &job.dir)?;
        if created_at != job.created_at {
            return Err(VaultError::Integrity(
                "the backup on the disk is another one",
            ));
        }
        if cases != job.live_cases {
            return Err(VaultError::Integrity(
                "the backup holds fewer cases than the vault",
            ));
        }
        let done = BackupWritten {
            info: BackupInfo {
                bytes: job.bytes(),
                created_at,
            },
            cases: u32::try_from(cases).unwrap_or(u32::MAX),
        };
        Ok((done, bytes))
    }
}

/// Open a backup file with this vault's keys in a scratch folder next to the vault, and count
/// its cases. The snapshots stay encrypted, and the folder is removed.
fn check_file(
    bytes: &[u8],
    vault_id: &str,
    keys: &CheckKeys,
    dir: &Path,
) -> Result<(i64, i64), VaultError> {
    let parsed = parse(bytes)?;
    if parsed.header.vault_id != vault_id {
        return Err(VaultError::Integrity("the backup is not from this vault"));
    }
    let container = open(
        &keys.backup,
        &aad(&parsed.header.vault_id, parsed.created_at),
        parsed.sealed,
    )
    .map_err(|_| VaultError::Integrity("the backup was changed or is not from this vault"))?;
    let files = entries(&container)?;
    let scratch = dir.join(format!(".verify-{}", random_id()?));
    fs::create_dir(&scratch)?;
    let opened = (|| -> Result<i64, VaultError> {
        let mut cases = None;
        for (name, data) in &files {
            let key = match name.as_str() {
                MAIN_DB => &keys.db_main,
                IDENTITY_DB => &keys.db_identity,
                _ => &keys.db_audit,
            };
            let file = scratch.join(name);
            fs::write(&file, data)?;
            let conn = open_encrypted(&file, key)?;
            let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            if check != "ok" {
                return Err(VaultError::Integrity("a database in the backup is damaged"));
            }
            if name == MAIN_DB {
                cases = Some(conn.query_row("SELECT count(*) FROM cases", [], |r| r.get(0))?);
            }
        }
        cases.ok_or_else(|| corrupt("no main database"))
    })();
    let _ = fs::remove_dir_all(&scratch);
    Ok((parsed.created_at, opened?))
}

/// Restore drill on a backup file, without the open vault: open it in a scratch folder next
/// to the vault in `dir`, count what it holds, and remove it. Proves both the file and the
/// secret work. The slow part (the key derivation) runs here; [`Vault::record_drill`] logs it.
pub fn drill_backup(
    dir: &Path,
    backup: &[u8],
    secret: Secret<'_>,
) -> Result<BackupCheck, VaultError> {
    let scratch = dir.join(format!(".drill-{}", random_id()?));
    let result = Vault::restore_backup(backup, &scratch, secret).map(|(v, check)| {
        drop(v);
        check
    });
    let _ = fs::remove_dir_all(&scratch);
    result
}

impl Vault {
    /// Take a consistent snapshot of the three databases: the part of a backup that needs the
    /// vault, a moment (F2). Exporting and sealing it, writing it out and reading it back are
    /// the job's ([`BackupJob::write`]), while the vault goes on working.
    pub fn begin_backup(&mut self) -> Result<BackupJob, VaultError> {
        // Opens with the password on any computer, and with the kit (D-076).
        let (snapshot, live_cases) = Snapshot::take(self)?;
        Ok(BackupJob {
            file: Vec::new(),
            pending: Some(snapshot),
            created_at: now(),
            vault_id: self.header.vault_id.clone(),
            dir: self.dir.clone(),
            keys: CheckKeys::of(&self.keys),
            live_cases,
        })
    }

    /// Record a backup that was written and read back.
    pub fn finish_backup(&mut self, done: &BackupWritten) -> Result<(), VaultError> {
        self.record(
            AuditEvent::BackupWritten,
            None,
            &serde_json::json!({ "bytes": done.info.bytes }),
        )?;
        self.record(
            AuditEvent::BackupChecked,
            None,
            &serde_json::json!({ "cases": done.cases, "ok": true, "auto": true }),
        )
    }

    /// Write an encrypted backup of the whole vault to `dest` (replaced atomically), without
    /// reading it back.
    pub fn write_backup(&mut self, dest: &Path) -> Result<BackupInfo, VaultError> {
        let job = self.begin_backup()?.seal()?;
        let info = BackupInfo {
            bytes: job.bytes(),
            created_at: job.created_at,
        };
        job.write(dest)?;
        self.record(
            AuditEvent::BackupWritten,
            None,
            &serde_json::json!({ "bytes": info.bytes }),
        )?;
        Ok(info)
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

        let files = entries(&container)?;

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
        let check = drill_backup(&self.dir, backup, secret)?;
        self.record_drill(&check)?;
        Ok(check)
    }

    /// Log a restore drill made with [`drill_backup`].
    pub fn record_drill(&mut self, check: &BackupCheck) -> Result<(), VaultError> {
        self.record(
            AuditEvent::BackupChecked,
            None,
            &serde_json::json!({ "cases": check.cases, "ok": check.integrity_ok }),
        )
    }

    /// Read back a backup this vault has written, from the disk it went to, and open it: the
    /// seal, each database with its own key, and as many cases as the vault has now. Needs no
    /// password. The snapshots are opened in a scratch folder next to the vault, still
    /// encrypted, and removed.
    pub fn verify_backup_file(&mut self, path: &Path) -> Result<BackupCheck, VaultError> {
        let bytes = fs::read(path)?;
        let keys = CheckKeys::of(&self.keys);
        let (created_at, cases) = check_file(&bytes, &self.header.vault_id, &keys, &self.dir)?;
        let live: i64 = self
            .main
            .query_row("SELECT count(*) FROM cases", [], |r| r.get(0))?;
        if cases != live {
            return Err(VaultError::Integrity(
                "the backup holds fewer cases than the vault",
            ));
        }
        self.record(
            AuditEvent::BackupChecked,
            None,
            &serde_json::json!({ "cases": cases, "ok": true, "auto": true }),
        )?;
        Ok(BackupCheck {
            created_at,
            cases: u32::try_from(cases).unwrap_or(u32::MAX),
            integrity_ok: true,
        })
    }
}
