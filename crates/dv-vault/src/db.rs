//! SQLCipher databases (whole-file encryption) with versioned migrations.

use std::path::Path;

use rusqlite::Connection;
use zeroize::Zeroizing;

use crate::crypto::{hex, Key32};
use crate::VaultError;

/// Open (or create) an encrypted database with a raw 256-bit key (no SQLCipher KDF).
pub fn open_encrypted(path: &Path, key: &Key32) -> Result<Connection, VaultError> {
    let conn = Connection::open(path)?;
    quiet_sqlcipher_log(&conn)?;
    let key_hex = Zeroizing::new(hex(key.as_bytes()));
    let statement = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", key_hex.as_str()));
    conn.execute_batch(&statement)?;
    conn.execute_batch(
        "PRAGMA cipher_memory_security = ON;
         PRAGMA secure_delete = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;",
    )?;
    // Reading the schema fails if the key is wrong or the file is not ours.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map_err(|_| VaultError::WrongSecret)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(conn)
}

/// SQLCipher logs warnings to stderr by default. On Windows, with `cipher_memory_security`,
/// every allocation is locked in RAM (`VirtualLock`); once the process's lock quota is used
/// up, the failure is logged, the log line is converted to UTF-16 in memory allocated through
/// the same locking allocator, that lock fails and is logged too, and so on until the stack
/// overflows (sqlcipher/sqlcipher#619). The program vanished on Windows the moment a vault was
/// created or opened. Logging off breaks the loop; memory security stays on, so freed memory
/// is still wiped. The program has no console, so nothing that was ever seen is lost.
/// Global in SQLCipher, and set before anything else on every connection, so it is in place
/// before memory security is first turned on.
fn quiet_sqlcipher_log(conn: &Connection) -> Result<(), VaultError> {
    let level: String = conn.query_row("PRAGMA cipher_log_level = NONE", [], |r| r.get(0))?;
    if level != "NONE" {
        return Err(VaultError::Crypto("sqlcipher log level"));
    }
    Ok(())
}

/// Apply migrations in order. Each entry is one schema version; never edit a released one.
pub fn migrate(conn: &Connection, migrations: &[&str]) -> Result<(), VaultError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;
    for (index, sql) in migrations.iter().enumerate() {
        let version = i64::try_from(index + 1).unwrap_or(i64::MAX);
        if version > current {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()?;
        }
    }
    Ok(())
}

pub const MAIN_MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE cases (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        wrapped_case_key BLOB NOT NULL,
        meta_enc BLOB NOT NULL
     );
     CREATE TABLE inputs (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        kind TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        title_enc BLOB NOT NULL,
        content_enc BLOB NOT NULL
     );
     CREATE TABLE messages (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        section_key TEXT NOT NULL,
        role TEXT NOT NULL,
        demo INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        text_tagged_enc BLOB NOT NULL,
        hidden_enc BLOB NOT NULL
     );
     CREATE TABLE drafts (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        section_key TEXT NOT NULL,
        position INTEGER NOT NULL,
        status TEXT NOT NULL,
        author TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        approved_at INTEGER,
        text_tagged_enc BLOB NOT NULL,
        source_refs_enc BLOB NOT NULL
     );
     CREATE TABLE transmissions (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        section_key TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        payload_sha256 TEXT NOT NULL,
        model TEXT NOT NULL,
        payload_tagged_enc BLOB NOT NULL
     );
     CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE secrets (key TEXT PRIMARY KEY, value_enc BLOB NOT NULL);
     CREATE INDEX inputs_case ON inputs(case_id);
     CREATE INDEX messages_case ON messages(case_id, section_key);
     CREATE INDEX drafts_case ON drafts(case_id, section_key);",
    // v2: structured data behind a material (a score sheet), sealed like the rest of the row.
    "ALTER TABLE inputs ADD COLUMN data_enc BLOB;",
    // v3: which report sections a material feeds (D-022), sealed like the rest of the row.
    "ALTER TABLE inputs ADD COLUMN routing_enc BLOB;",
    // v4: folders (names sealed with the settings key) and the recycle bin.
    "CREATE TABLE folders (
        id TEXT PRIMARY KEY,
        parent_id TEXT REFERENCES folders(id),
        created_at INTEGER NOT NULL,
        name_enc BLOB NOT NULL
     );
     ALTER TABLE cases ADD COLUMN folder_id TEXT;
     ALTER TABLE cases ADD COLUMN deleted_at INTEGER;",
    // v5: saved consultations. The turns are sealed with the case key (general ones with the
    // settings key), so a case's conversations are erased with the case.
    "CREATE TABLE consultations (
        id TEXT PRIMARY KEY,
        case_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        turns_enc BLOB NOT NULL
     );
     CREATE INDEX consultations_updated ON consultations(updated_at);",
    // v6: Claude's new wording of a paragraph she approved waits beside it; approving it
    // retires the old one (D-032).
    "ALTER TABLE drafts ADD COLUMN replaces TEXT;",
    // v7: the writing-style profile (D-043). Past reports are kept only as neutralized text;
    // nothing here belongs to a case, so all of it is sealed with the settings key.
    "CREATE TABLE style_sources (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        data_enc BLOB NOT NULL
     );
     CREATE TABLE style_profiles (
        id TEXT PRIMARY KEY,
        version INTEGER NOT NULL,
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        data_enc BLOB NOT NULL
     );
     CREATE TABLE style_learning (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        data_enc BLOB NOT NULL
     );",
    // v8: earlier wordings of a paragraph, kept when it is edited or reworded in place, so
    // she can see them and bring one back (D-046). Sealed with the case key like the draft.
    "CREATE TABLE draft_versions (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
        draft_id TEXT NOT NULL,
        saved_at INTEGER NOT NULL,
        author TEXT NOT NULL,
        text_tagged_enc BLOB NOT NULL
     );
     CREATE INDEX draft_versions_draft ON draft_versions(case_id, draft_id);",
];

pub const IDENTITY_MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE identities (
        id TEXT PRIMARY KEY,
        case_id TEXT NOT NULL,
        role TEXT NOT NULL,
        tag TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        value_enc BLOB NOT NULL,
        aliases_enc BLOB NOT NULL
     );
     CREATE TABLE decisions (
        id TEXT PRIMARY KEY,
        case_id TEXT,
        token_hmac TEXT NOT NULL,
        decision TEXT NOT NULL,
        tag TEXT,
        created_at INTEGER NOT NULL
     );
     CREATE TABLE practitioner (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        value_enc BLOB NOT NULL
     );
     CREATE INDEX identities_case ON identities(case_id);
     CREATE UNIQUE INDEX decisions_token ON decisions(COALESCE(case_id, ''), token_hmac);",
];

pub const AUDIT_MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE audit (
        seq INTEGER PRIMARY KEY,
        ts INTEGER NOT NULL,
        event TEXT NOT NULL,
        case_ref TEXT,
        meta TEXT NOT NULL,
        prev_mac TEXT NOT NULL,
        mac TEXT NOT NULL
     );",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The Windows crash (D-038) needs SQLCipher's log to be off before memory security is on.
    /// Windows CI proves the crash is gone; this keeps the line from being dropped elsewhere.
    #[test]
    fn sqlcipher_logging_is_off_with_memory_security_on() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let conn = open_encrypted(&dir.path().join("t.db"), &Key32::from_bytes([3; 32]))
            .unwrap_or_else(|_| unreachable!());
        let level: String = conn
            .query_row("PRAGMA cipher_log_level", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(level, "NONE");
        let security: String = conn
            .query_row("PRAGMA cipher_memory_security", [], |r| r.get(0))
            .unwrap_or_default();
        assert_eq!(security, "1");
    }
}
