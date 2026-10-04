//! Tamper-evident audit log: each entry carries an HMAC over its content and the previous
//! entry's HMAC. Metadata only – never case content (STANDARDS.md §5.5).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::crypto::{hex, hmac_sha256, hmac_verify, unhex, Key32};
use crate::header::AuditAnchor;
use crate::VaultError;

const GENESIS: &str = "genesis";

/// Everything that is audited. Adding an event never changes existing entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEvent {
    VaultCreated,
    Unlock,
    UnlockFailed,
    Lock,
    PasswordChanged,
    RecoveryKeyRotated,
    CaseCreated,
    CaseOpened,
    CaseDeleted,
    CaseTrashed,
    CaseRestored,
    FoldersChanged,
    BackupWritten,
    BackupChecked,
    Restored,
    IdentitiesChanged,
    Send,
    Blocked,
    Override,
    Export,
    SettingsChanged,
    IntegrityWarning,
    /// Einat went over the log (periodic review of access records).
    AuditReviewed,
    ConsultationDeleted,
    WindowsHelloOn,
    WindowsHelloOff,
}

impl AuditEvent {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AuditEvent::VaultCreated => "vault_created",
            AuditEvent::Unlock => "unlock",
            AuditEvent::UnlockFailed => "unlock_failed",
            AuditEvent::Lock => "lock",
            AuditEvent::PasswordChanged => "password_changed",
            AuditEvent::RecoveryKeyRotated => "recovery_key_rotated",
            AuditEvent::CaseCreated => "case_created",
            AuditEvent::CaseOpened => "case_opened",
            AuditEvent::CaseDeleted => "case_deleted",
            AuditEvent::CaseTrashed => "case_trashed",
            AuditEvent::CaseRestored => "case_restored",
            AuditEvent::FoldersChanged => "folders_changed",
            AuditEvent::BackupWritten => "backup_written",
            AuditEvent::BackupChecked => "backup_checked",
            AuditEvent::Restored => "restored",
            AuditEvent::IdentitiesChanged => "identities_changed",
            AuditEvent::Send => "send",
            AuditEvent::Blocked => "blocked",
            AuditEvent::Override => "override",
            AuditEvent::Export => "export",
            AuditEvent::SettingsChanged => "settings_changed",
            AuditEvent::IntegrityWarning => "integrity_warning",
            AuditEvent::AuditReviewed => "audit_reviewed",
            AuditEvent::ConsultationDeleted => "consultation_deleted",
            AuditEvent::WindowsHelloOn => "windows_hello_on",
            AuditEvent::WindowsHelloOff => "windows_hello_off",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: i64,
    pub ts: i64,
    pub event: String,
    pub case_ref: Option<String>,
    pub meta: String,
}

fn mac_input(
    seq: i64,
    ts: i64,
    event: &str,
    case_ref: Option<&str>,
    meta: &str,
    prev: &str,
) -> Vec<u8> {
    format!(
        "{seq}|{ts}|{event}|{}|{meta}|{prev}",
        case_ref.unwrap_or("")
    )
    .into_bytes()
}

/// Append one entry and return the new chain head.
pub fn append(
    conn: &Connection,
    key: &Key32,
    ts: i64,
    event: AuditEvent,
    case_ref: Option<&str>,
    meta: &serde_json::Value,
) -> Result<AuditAnchor, VaultError> {
    let last: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq, mac FROM audit ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (prev_seq, prev_mac) = last.unwrap_or((0, GENESIS.to_owned()));
    let seq = prev_seq + 1;
    let meta = meta.to_string();
    let mac = hex(&hmac_sha256(
        key,
        &mac_input(seq, ts, event.as_str(), case_ref, &meta, &prev_mac),
    ));
    conn.execute(
        "INSERT INTO audit (seq, ts, event, case_ref, meta, prev_mac, mac) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![seq, ts, event.as_str(), case_ref, meta, prev_mac, mac],
    )?;
    Ok(AuditAnchor { seq, mac })
}

/// Result of checking the chain against the anchor stored in the vault header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStatus {
    Intact {
        head: Option<AuditAnchor>,
    },
    /// The last append was interrupted before the header was updated; the chain itself is valid.
    AnchorLagging {
        head: AuditAnchor,
    },
    Broken {
        reason: String,
    },
}

pub fn verify(
    conn: &Connection,
    key: &Key32,
    anchor: Option<&AuditAnchor>,
) -> Result<ChainStatus, VaultError> {
    let mut stmt = conn
        .prepare("SELECT seq, ts, event, case_ref, meta, prev_mac, mac FROM audit ORDER BY seq")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut expected_prev = GENESIS.to_owned();
    let mut expected_seq = 1;
    let mut head: Option<AuditAnchor> = None;
    for row in rows {
        let (seq, ts, event, case_ref, meta, prev, mac) = row?;
        if seq != expected_seq {
            return Ok(ChainStatus::Broken {
                reason: format!("entry {expected_seq} is missing"),
            });
        }
        if prev != expected_prev {
            return Ok(ChainStatus::Broken {
                reason: format!("entry {seq} is not linked to the previous one"),
            });
        }
        let ok = hmac_verify(
            key,
            &mac_input(seq, ts, &event, case_ref.as_deref(), &meta, &prev),
            &unhex(&mac)?,
        );
        if !ok {
            return Ok(ChainStatus::Broken {
                reason: format!("entry {seq} was modified"),
            });
        }
        expected_prev.clone_from(&mac);
        expected_seq += 1;
        head = Some(AuditAnchor { seq, mac });
    }
    match (anchor, head) {
        (None, None) => Ok(ChainStatus::Intact { head: None }),
        (Some(a), Some(h)) if a == &h => Ok(ChainStatus::Intact { head: Some(h) }),
        (None, Some(h)) if h.seq == 1 => Ok(ChainStatus::AnchorLagging { head: h }),
        (Some(a), Some(h)) if h.seq == a.seq + 1 => {
            // The anchor must still match the entry it names.
            let named: Option<String> = conn
                .query_row("SELECT mac FROM audit WHERE seq = ?1", [a.seq], |r| {
                    r.get(0)
                })
                .optional()?;
            if named.as_deref() == Some(a.mac.as_str()) {
                Ok(ChainStatus::AnchorLagging { head: h })
            } else {
                Ok(ChainStatus::Broken {
                    reason: "log does not match the header".to_owned(),
                })
            }
        }
        _ => Ok(ChainStatus::Broken {
            reason: "log was truncated or rolled back".to_owned(),
        }),
    }
}

pub fn recent(conn: &Connection, limit: u32) -> Result<Vec<AuditEntry>, VaultError> {
    page(conn, None, limit)
}

/// Newest first, starting below `before` (a `seq`) when given.
pub fn page(
    conn: &Connection,
    before: Option<i64>,
    limit: u32,
) -> Result<Vec<AuditEntry>, VaultError> {
    let mut stmt = conn.prepare(
        "SELECT seq, ts, event, case_ref, meta FROM audit WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![before.unwrap_or(i64::MAX), limit], |r| {
        Ok(AuditEntry {
            seq: r.get(0)?,
            ts: r.get(1)?,
            event: r.get(2)?,
            case_ref: r.get(3)?,
            meta: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
