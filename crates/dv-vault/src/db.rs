//! SQLCipher databases (whole-file encryption) with versioned migrations.

use std::path::Path;

use rusqlite::Connection;
use zeroize::Zeroizing;

use crate::crypto::{hex, Key32};
use crate::VaultError;

/// Open (or create) an encrypted database with a raw 256-bit key (no SQLCipher KDF).
pub fn open_encrypted(path: &Path, key: &Key32) -> Result<Connection, VaultError> {
    let conn = Connection::open(path)?;
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
